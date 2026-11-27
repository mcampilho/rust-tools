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

/// Takes the read-only attribute off `path` itself (not off what a link
/// points to).
fn make_writable(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_READONLY, INVALID_FILE_ATTRIBUTES,
        };
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
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
/// link shares it with the persisted file.
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

        remove_tree(&version).unwrap();
        assert!(!version.exists());
        assert_eq!(std::fs::read_to_string(kept).unwrap(), "keep");
        assert_eq!(std::fs::read_to_string(single).unwrap(), "ini");
        remove_tree(&version).unwrap();
    }
}
