//! Is a folder safe to run the service from? Only SYSTEM, Administrators and
//! TrustedInstaller may change what is in it, or replace it, or any folder
//! above it. A folder with the permissions of `C:\Program Files` passes.
//!
//! What is checked is in `docs/service.md`, §9. Deny entries are not read: a
//! folder is judged by what it allows, which can only make the check stricter.

use crate::access::{sid_text, SID_ADMINISTRATORS, SID_SYSTEM, SID_TRUSTED_INSTALLER};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    GetAce, GetAclInformation, ACL, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
};
use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;

const DRIVE_FIXED: u32 = 3;
const ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
const ACCESS_ALLOWED_CALLBACK_ACE_TYPE: u8 = 9;
const INHERIT_ONLY_ACE: u8 = 0x08;
/// `ACL_INFORMATION_CLASS::AclSizeInformation`.
const ACL_SIZE_INFORMATION_CLASS: i32 = 2;

/// Rights that change what a folder holds: write and append data, extended
/// and basic attributes, delete (and delete child), change permissions, take
/// ownership, and the generic all and write.
pub const CHANGES: u32 = 0x2 | 0x4 | 0x10 | 0x40 | 0x100 | 0x1_0000 | 0x4_0000 | 0x8_0000 | 0x1000_0000 | 0x4000_0000;
/// Rights that let a folder be removed, renamed, or given other permissions:
/// delete (itself or as a child), change permissions, take ownership, generic all.
pub const REPLACES: u32 = 0x40 | 0x1_0000 | 0x4_0000 | 0x8_0000 | 0x1000_0000;

/// May the account with this SID change what an administrator-only folder holds?
pub fn is_trusted(sid: &str) -> bool {
    // CREATOR OWNER (S-1-3-0) only means the owner of what is created, and the owner is checked too.
    matches!(sid, SID_SYSTEM | SID_ADMINISTRATORS | SID_TRUSTED_INSTALLER) || sid == "S-1-3-0"
}

/// Does an entry that allows `mask` to `sid` make the folder unsafe? `own` is the
/// folder or file itself (all changes count); otherwise a folder above it (only
/// what lets it be replaced).
pub fn allows_too_much(mask: u32, sid: &str, own: bool) -> bool {
    !is_trusted(sid) && mask & if own { CHANGES } else { REPLACES } != 0
}

fn wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// The owner, and the entries of the DACL (mask, SID text, flags, type).
struct Acl {
    owner: String,
    /// None: a null DACL, which gives everyone every right.
    entries: Option<Vec<(u32, String, u8, u8)>>,
}

fn read_acl(path: &Path) -> Result<Acl, String> {
    let w = wide(path);
    let (mut owner, mut dacl, mut sd): (*mut core::ffi::c_void, *mut ACL, *mut core::ffi::c_void) = (null_mut(), null_mut(), null_mut());
    // SAFETY: valid path and out pointers; `sd` is freed below.
    let r = unsafe {
        GetNamedSecurityInfoW(
            w.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut sd,
        )
    };
    if r != 0 {
        return Err(format!("the permissions of {} cannot be read ({})", path.display(), std::io::Error::from_raw_os_error(r as i32)));
    }
    // SAFETY: `owner` points into `sd`, alive until the LocalFree.
    let owner_text = unsafe { sid_text(owner) }.unwrap_or_default();
    let mut entries = None;
    if !dacl.is_null() {
        let mut list = Vec::new();
        let mut info = ACL_SIZE_INFORMATION { AceCount: 0, AclBytesInUse: 0, AclBytesFree: 0 };
        // SAFETY: `dacl` is a valid ACL inside `sd`; `info` has the size asked for.
        if unsafe {
            GetAclInformation(dacl, &mut info as *mut _ as *mut _, size_of::<ACL_SIZE_INFORMATION>() as u32, ACL_SIZE_INFORMATION_CLASS)
        } == 0
        {
            // SAFETY: allocated by GetNamedSecurityInfoW.
            unsafe { LocalFree(sd as *mut _) };
            return Err(format!("the permissions of {} cannot be counted", path.display()));
        }
        for i in 0..info.AceCount {
            let mut ace: *mut core::ffi::c_void = null_mut();
            // SAFETY: the index is below the count.
            if unsafe { GetAce(dacl, i, &mut ace) } == 0 || ace.is_null() {
                continue;
            }
            // SAFETY: an ACE starts with its header; allowed ACEs continue with the mask and the SID.
            unsafe {
                let header = *(ace as *const windows_sys::Win32::Security::ACE_HEADER);
                if header.AceType == ACCESS_ALLOWED_ACE_TYPE || header.AceType == ACCESS_ALLOWED_CALLBACK_ACE_TYPE {
                    let a = &*(ace as *const windows_sys::Win32::Security::ACCESS_ALLOWED_ACE);
                    let sid = sid_text(&a.SidStart as *const u32 as *mut _).unwrap_or_default();
                    list.push((a.Mask, sid, header.AceFlags, header.AceType));
                } else if header.AceType != 1 && header.AceType != 2 && header.AceType != 3 {
                    // An allowing kind this code does not read (object ACEs...): treated as unsafe.
                    list.push((u32::MAX, "?".to_string(), header.AceFlags, header.AceType));
                }
            }
        }
        entries = Some(list);
    }
    // SAFETY: allocated by GetNamedSecurityInfoW.
    unsafe { LocalFree(sd as *mut _) };
    Ok(Acl { owner: owner_text, entries })
}

/// What is wrong with the permissions of one folder or file (`own`: see [`allows_too_much`]).
fn check_object(path: &Path, own: bool, problems: &mut Vec<String>) {
    match read_acl(path) {
        Err(e) => problems.push(e),
        Ok(acl) => {
            if !is_trusted(&acl.owner) || acl.owner == "S-1-3-0" {
                problems.push(format!("{} is owned by {}, not by SYSTEM, Administrators or TrustedInstaller", path.display(), acl.owner));
            }
            match acl.entries {
                None => problems.push(format!("{} has no access control list: everyone may change it", path.display())),
                Some(list) => {
                    for (mask, sid, flags, _kind) in list {
                        // Entries only for what is created inside (inherit only) do not touch this object.
                        if flags & INHERIT_ONLY_ACE != 0 && sid != "?" {
                            continue;
                        }
                        if sid == "?" || allows_too_much(mask, &sid, own) {
                            problems.push(format!(
                                "{} allows {} to {} (rights {:#x})",
                                path.display(),
                                sid,
                                if own { "change it" } else { "replace it" },
                                mask
                            ));
                        }
                    }
                }
            }
        }
    }
}

/// The entries below `dir`, to the depth given, folders and files.
fn below(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        out.push(p.clone());
        if is_dir && depth > 1 {
            below(&p, depth - 1, out);
        }
    }
}

/// Everything wrong with an existing folder of the service's data; empty when it can be used (or does not exist yet).
///
/// Users may create folders in `%ProgramData%`, so this one may have been put there before the service was installed:
/// its owner could change its permissions afterwards, and a junction in it would send what the service writes
/// somewhere else. So nothing in it may be a link or belong to anyone but SYSTEM, Administrators or TrustedInstaller.
/// The permissions are not judged: the service sets them itself.
pub fn check_data_dir(dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    if std::fs::symlink_metadata(dir).is_err() {
        return problems;
    }
    let mut all = vec![dir.to_path_buf()];
    below(dir, 3, &mut all);
    for p in all {
        // Not followed: a junction is a problem by itself.
        if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_attributes() & ATTRIBUTE_REPARSE_POINT != 0) {
            problems.push(format!("{} is a link or a junction", p.display()));
            continue;
        }
        match read_acl(&p) {
            Ok(acl) if is_trusted(&acl.owner) && acl.owner != "S-1-3-0" => {}
            Ok(acl) => {
                problems.push(format!("{} is owned by {}, not by SYSTEM, Administrators or TrustedInstaller", p.display(), acl.owner))
            }
            Err(e) => problems.push(e),
        }
    }
    problems
}

/// Everything wrong with `dir` as a place to run the service from; empty when it is safe.
pub fn check_dir(dir: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    let abs = match std::path::absolute(dir) {
        Ok(a) => a,
        Err(e) => return vec![format!("{} is not a path ({e})", dir.display())],
    };
    let real = match std::fs::canonicalize(&abs) {
        Ok(r) => r,
        Err(e) => return vec![format!("{} cannot be opened ({e})", abs.display())],
    };
    // `\\?\C:\...` to `C:\...`; a share is `\\?\UNC\...`.
    let shown = real.to_string_lossy().into_owned();
    if shown.starts_with(r"\\?\UNC\") || shown.starts_with(r"\\") && !shown.starts_with(r"\\?\") {
        return vec![format!("{} is on the network", abs.display())];
    }
    let plain = PathBuf::from(shown.strip_prefix(r"\\?\").unwrap_or(&shown));
    if plain.to_string_lossy().to_lowercase() != abs.to_string_lossy().trim_end_matches('\\').to_lowercase() {
        problems.push(format!(
            "the path {} leads to {}: a link, a junction or a substituted drive is in the way",
            abs.display(),
            plain.display()
        ));
    }
    // The volume must be a fixed local disk.
    if let Some(root) = plain.ancestors().last() {
        let w = wide(root);
        // SAFETY: a valid, terminated string.
        if unsafe { GetDriveTypeW(w.as_ptr()) } != DRIVE_FIXED {
            problems.push(format!("{} is not on a fixed local disk", root.display()));
        }
    }
    for (i, folder) in plain.ancestors().enumerate() {
        if folder.as_os_str().is_empty() {
            continue;
        }
        if std::fs::symlink_metadata(folder).is_ok_and(|m| m.file_attributes() & ATTRIBUTE_REPARSE_POINT != 0) {
            problems.push(format!("{} is a link or a junction", folder.display()));
        }
        check_object(folder, i == 0, &mut problems);
    }
    let mut inside = Vec::new();
    below(&plain, 4, &mut inside);
    for p in inside {
        if std::fs::symlink_metadata(&p).is_ok_and(|m| m.file_attributes() & ATTRIBUTE_REPARSE_POINT != 0) {
            problems.push(format!("{} is a link or a junction", p.display()));
        }
        check_object(&p, true, &mut problems);
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn who_may_change_what() {
        // Users writing into the folder itself, or being able to delete it, is not safe.
        assert!(allows_too_much(0x2, "S-1-5-32-545", true));
        assert!(allows_too_much(0x1000_0000, "S-1-5-11", true));
        assert!(allows_too_much(0x1_0000, "S-1-5-32-545", false), "delete on a folder above");
        assert!(allows_too_much(0x40, "S-1-1-0", false), "delete child on a folder above");
        // Reading, executing and listing are fine; adding below an ancestor is fine.
        assert!(!allows_too_much(0x1200a9, "S-1-5-32-545", true));
        assert!(!allows_too_much(0x4, "S-1-5-11", false), "create a folder in a folder above");
        // The trusted accounts may do anything.
        assert!(!allows_too_much(0x1fffff, SID_SYSTEM, true));
        assert!(!allows_too_much(0x1fffff, SID_ADMINISTRATORS, true));
        assert!(!allows_too_much(0x1fffff, SID_TRUSTED_INSTALLER, false));
    }

    #[test]
    fn folders_of_the_system_are_safe() {
        // Small folders of the system, as `Program Files` has them (not the whole tree: antivirus
        // programs hide theirs). The first that exists on this PC is checked.
        let pf = std::env::var("ProgramFiles").unwrap();
        let root = std::env::var("SystemRoot").unwrap();
        let dir = [
            format!(r"{pf}\Windows NT\Accessories"),
            format!(r"{pf}\Windows Mail"),
            format!(r"{root}\Boot\EFI"),
            format!(r"{root}\Globalization"),
        ]
        .into_iter()
        .find(|d| Path::new(d).is_dir())
        .expect("a folder of the system");
        // The root of a drive is judged too, and some PCs give `Everyone` full control of it
        // (which lets anyone rename what is below). Those entries are about the drive, not the folder.
        let drive = format!("{}\\", &dir[..2]);
        let problems: Vec<String> = check_dir(Path::new(&dir)).into_iter().filter(|p| !p.starts_with(&drive)).collect();
        assert!(problems.is_empty(), "{dir}: {problems:#?}");
    }

    #[test]
    fn a_folder_a_user_writes_in_is_not_safe() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("rt-svc-secure-test");
        std::fs::create_dir_all(&dir).unwrap();
        let problems = check_dir(&dir);
        assert!(!problems.is_empty());
        assert!(problems.iter().any(|p| p.contains("change it")), "{problems:#?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_data_folder_with_a_junction_or_of_a_user_is_not_used() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("rt-svc-data-test");
        let _ = std::fs::remove_dir_all(&base);
        assert!(check_data_dir(&base).is_empty(), "one that does not exist yet is fine");
        let (data, elsewhere) = (base.join("data"), base.join("elsewhere"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let link = std::path::absolute(data.join("logs")).unwrap();
        let target = std::path::absolute(&elsewhere).unwrap();
        let made = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(&link).arg(&target).output().unwrap();
        assert!(made.status.success(), "{made:?}");
        let problems = check_data_dir(&data);
        assert!(problems.iter().any(|p| p.contains("junction")), "{problems:#?}");
        // Made by this test: a user's, unless it runs elevated (then Administrators own it).
        if !crate::access::is_elevated() {
            assert!(problems.iter().any(|p| p.contains("owned by")), "{problems:#?}");
        }
        std::fs::remove_dir(&link).unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_missing_folder_or_a_share_is_not_safe() {
        assert!(!check_dir(Path::new(r"C:\no-such-folder-for-rt-svc")).is_empty());
        let p = check_dir(Path::new(r"\\localhost\C$\Windows"));
        assert!(!p.is_empty(), "{p:?}");
    }
}
