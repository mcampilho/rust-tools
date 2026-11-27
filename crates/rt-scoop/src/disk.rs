//! Writing and deleting. Deleting never goes through a junction or a symbolic
//! link: the link goes, what it points to stays (persisted data is linked
//! into the version folders).

use std::io;
use std::path::Path;

/// Is `meta` (of `symlink_metadata`) a junction, a symbolic link or another
/// reparse point?
pub fn is_link(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

/// `path` in the `\\?\` form, so that the Win32 calls take it past 260
/// characters (the standard library does this for its own calls).
#[cfg(windows)]
fn verbatim(path: &Path) -> std::ffi::OsString {
    let full = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let text = full.as_os_str().to_string_lossy();
    if text.starts_with(r"\\?\") || text.starts_with(r"\\.\") {
        full.into_os_string()
    } else if let Some(unc) = text.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{unc}").into()
    } else {
        format!(r"\\?\{text}").into()
    }
}

/// Takes the read-only attribute off `path` itself (not off what a link
/// points to: `SetFileAttributesW` changes the junction, as `attrib -R /L`).
fn make_writable(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_READONLY, INVALID_FILE_ATTRIBUTES,
        };
        let wide: Vec<u16> = verbatim(path).encode_wide().chain(Some(0)).collect();
        // SAFETY: a NUL-terminated path.
        unsafe {
            let attributes = GetFileAttributesW(wide.as_ptr());
            if attributes != INVALID_FILE_ATTRIBUTES && attributes & FILE_ATTRIBUTE_READONLY != 0 {
                SetFileAttributesW(wide.as_ptr(), attributes & !FILE_ATTRIBUTE_READONLY);
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
    }
}

/// Tries `remove`; if it is refused, takes the read-only attribute off and
/// tries again. The attribute is only touched when needed, because a hard
/// link shares it with the persisted file. In practice that is a folder or a
/// junction: `std::fs::remove_file` deletes a read-only file as it is.
fn insist(path: &Path, remove: fn(&Path) -> io::Result<()>) -> io::Result<()> {
    remove(path).or_else(|_| {
        make_writable(path);
        remove(path)
    })
}

fn remove_link(path: &Path) -> io::Result<()> {
    // A junction or a link to a folder goes with `remove_dir`.
    std::fs::remove_dir(path).or_else(|_| std::fs::remove_file(path))
}

fn remove_dir(path: &Path) -> io::Result<()> {
    std::fs::remove_dir(path)
}

fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// Removes `path` and everything under it, without following links.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    let named = |e: io::Error| io::Error::new(e.kind(), format!("{}: {e}", path.display()));
    if is_link(&meta) {
        return insist(path, remove_link).map_err(named);
    }
    if !meta.is_dir() {
        return insist(path, remove_file).map_err(named);
    }
    for entry in std::fs::read_dir(path).map_err(named)? {
        remove_tree(&entry.map_err(named)?.path())?;
    }
    insist(path, remove_dir).map_err(named)
}

/// Writes `text` to `path` through a file beside it, so that a failure never
/// leaves a half-written file.
pub fn write_file(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    let temporary = path.with_file_name(name);
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::testing::{junction, Tree};

    #[test]
    fn links_go_and_their_targets_stay() {
        let t = Tree::new("disk");
        let kept = t.file("persist/data/keep.txt", "keep");
        let single = t.file("persist/file.ini", "ini");
        let version = t.dir("apps/x/1.0");
        t.file("apps/x/1.0/bin/x.exe", "x");
        junction(&version.join("data"), &t.dir("persist/data"));
        std::fs::hard_link(&single, version.join("file.ini")).unwrap();
        junction(&version.join("bin").join("deep"), &t.dir("persist"));
        // A read-only file and a read-only link.
        let readonly = t.file("apps/x/1.0/ro.txt", "r");
        let mut permissions = std::fs::metadata(&readonly).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&readonly, permissions).unwrap();

        let ro = t.file("persist/ro.ini", "ro");
        let mut permissions = std::fs::metadata(&ro).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&ro, permissions).unwrap();
        std::fs::hard_link(&ro, version.join("ro.ini")).unwrap();

        remove_tree(&version).unwrap();
        assert!(!version.exists());
        assert_eq!(std::fs::read_to_string(kept).unwrap(), "keep");
        assert_eq!(std::fs::read_to_string(single).unwrap(), "ini");
        // The standard library deletes a read-only file without taking the
        // attribute off, so the persisted file keeps it.
        assert!(super::tests::readonly(&ro));
        remove_tree(&version).unwrap();
    }

    fn readonly(path: &Path) -> bool {
        use std::os::windows::fs::MetadataExt;
        std::fs::symlink_metadata(path).unwrap().file_attributes() & 1 != 0
    }

    #[test]
    fn a_read_only_current_goes_and_its_version_stays_writable() {
        let t = Tree::new("disk-current");
        let version = t.dir("apps/x/1.0");
        t.file("apps/x/1.0/x.exe", "x");
        let current = t.dir("apps/x").join("current");
        junction(&current, &version);
        // As Scoop leaves it: `attrib +R /L` on the junction itself.
        let made = std::process::Command::new("attrib").arg("+R").arg("/L").arg(&current).output().unwrap();
        assert!(made.status.success() && readonly(&current) && !readonly(&version));

        remove_tree(&current).unwrap();
        assert!(std::fs::symlink_metadata(&current).is_err());
        assert!(!readonly(&version));
        assert_eq!(std::fs::read_to_string(version.join("x.exe")).unwrap(), "x");
    }

    #[test]
    fn read_only_files_and_folders_past_260_characters() {
        let t = Tree::new("disk-long");
        let top = t.dir("long");
        let mut deep = top.clone();
        while deep.as_os_str().len() < 300 {
            deep.push("a-folder-name-of-some-length");
        }
        std::fs::create_dir_all(&deep).unwrap();
        let file = deep.join("ro.txt");
        std::fs::write(&file, "r").unwrap();
        for path in [&file, &deep] {
            let mut permissions = std::fs::metadata(path).unwrap().permissions();
            permissions.set_readonly(true);
            std::fs::set_permissions(path, permissions).unwrap();
        }
        assert!(readonly(&deep));

        remove_tree(&top).unwrap();
        assert!(!top.exists());
    }
}
