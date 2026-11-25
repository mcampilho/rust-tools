//! Deleting only what is really where it was listed, for who deletes with more
//! rights than those who may write there (the service, as SYSTEM, in folders
//! like `C:\Windows\Temp`). Deleting by path would follow a junction put in the
//! place of a folder between the listing and the deleting, and delete somewhere
//! else.
//!
//! So each file or folder is opened without following it, asked where it really
//! is, and only if that is the path listed is it deleted, through the same
//! handle: what is checked is what is deleted.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FileDispositionInfoEx, GetFinalPathNameByHandleW, SetFileInformationByHandle, DELETE, FILE_DISPOSITION_FLAG_DELETE,
    FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle is ours and closed once.
        unsafe { CloseHandle(self.0) };
    }
}

/// Where the thing behind `h` really is, without the `\\?\` prefix.
fn real_path(h: &Handle) -> io::Result<String> {
    let mut buf = vec![0u16; 1024];
    loop {
        // SAFETY: a valid handle and a buffer of the length given.
        let n = unsafe { GetFinalPathNameByHandleW(h.0, buf.as_mut_ptr(), buf.len() as u32, 0) } as usize;
        if n == 0 {
            return Err(io::Error::last_os_error());
        }
        if n < buf.len() {
            let s = String::from_utf16_lossy(&buf[..n]);
            return Ok(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string());
        }
        buf.resize(n + 1, 0);
    }
}

/// Deletes the file (read-only too), the empty folder, or the link itself at `path`, if it is really there: with a
/// link or a junction anywhere above it, or a path written another way (a short `~1` name), nothing is deleted.
pub fn delete(path: &Path) -> io::Result<()> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: a terminated path; the other arguments are plain values.
    let h = unsafe {
        CreateFileW(
            wide.as_ptr(),
            DELETE | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let h = Handle(h);
    let listed = path.to_string_lossy();
    let listed = listed.strip_prefix(r"\\?\").unwrap_or(&listed).replace('/', "\\");
    if real_path(&h)?.to_lowercase() != listed.to_lowercase() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "it is not where it was listed (a link or a junction is in the way)"));
    }
    let info = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
    };
    // SAFETY: a valid handle with the DELETE right, and the structure its class asks for.
    let ok = unsafe {
        SetFileInformationByHandle(h.0, FileDispositionInfoEx, &info as *const _ as *const _, size_of::<FILE_DISPOSITION_INFO_EX>() as u32)
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("rt-clean-in-place").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // As Windows writes it, without `..`.
        PathBuf::from(d.canonicalize().unwrap().to_string_lossy().trim_start_matches(r"\\?\"))
    }

    #[test]
    fn files_and_empty_folders_where_they_were_listed_go() {
        let d = scratch("plain");
        let (file, readonly, dir) = (d.join("a.txt"), d.join("r.txt"), d.join("empty"));
        std::fs::write(&file, "x").unwrap();
        std::fs::write(&readonly, "x").unwrap();
        let mut p = std::fs::metadata(&readonly).unwrap().permissions();
        p.set_readonly(true);
        std::fs::set_permissions(&readonly, p).unwrap();
        std::fs::create_dir(&dir).unwrap();
        for p in [&file, &readonly, &dir] {
            delete(p).unwrap();
            assert!(!p.exists(), "{}", p.display());
        }
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("kept"), "x").unwrap();
        assert!(delete(&dir).is_err(), "a folder with something in it stays");
        assert_eq!(delete(&d.join("no")).unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn nothing_is_deleted_through_a_junction() {
        let d = scratch("junction");
        let (listed, precious) = (d.join("temp"), d.join("precious"));
        std::fs::create_dir(&precious).unwrap();
        std::fs::write(precious.join("a.txt"), "x").unwrap();
        // The folder that was listed is now a junction to somewhere else.
        let made = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(&listed).arg(&precious).output().unwrap();
        assert!(made.status.success(), "{made:?}");
        assert_eq!(delete(&listed.join("a.txt")).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert!(precious.join("a.txt").exists());
        // The junction itself is where it is: it goes, what it points to stays.
        delete(&listed).unwrap();
        assert!(!listed.exists() && precious.join("a.txt").exists());
    }
}
