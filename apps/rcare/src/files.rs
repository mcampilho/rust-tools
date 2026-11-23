//! File Tools: empty folders and the File Shredder (the user's choice,
//! 2026-11-23, from what was left for later).
//!
//! - **Empty folders**: under a folder the user chooses, the folders with no
//!   file anywhere inside (only the top one of each empty branch is listed,
//!   with how many empty folders it holds; never the chosen folder itself).
//!   Links and junctions are never followed nor counted as empty, a folder
//!   that cannot be read is not empty, and Windows' own (`$Recycle.Bin`,
//!   `System Volume Information`) are left alone. They go to the Recycle Bin.
//! - **File Shredder**: each file is overwritten with random bytes, flushed
//!   to the disk, given a random name and deleted, so ordinary undelete tools
//!   find nothing. On an SSD (and with some file systems) the drive may keep
//!   copies elsewhere: the page says so.

use std::path::{Path, PathBuf};

/// Folders never looked into.
const SKIP: [&str; 3] = ["$recycle.bin", "system volume information", "$windows.~bt"];

fn is_link(meta: &std::fs::Metadata) -> bool {
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT: junctions and mount points too.
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    false
}

/// Whether `dir` holds no file anywhere inside: Some(how many empty folders it is, itself included), or None.
/// The empty branches below a non-empty folder are added to `out`.
fn walk(dir: &Path, out: &mut Vec<(PathBuf, usize)>, stop: &std::sync::atomic::AtomicBool) -> Option<usize> {
    if stop.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return None };
    let mut empty_children: Vec<(PathBuf, usize)> = Vec::new();
    let mut has_something = false;
    for e in rd {
        let Ok(e) = e else {
            has_something = true;
            continue;
        };
        let Ok(meta) = std::fs::symlink_metadata(e.path()) else {
            has_something = true;
            continue;
        };
        if is_link(&meta) || !meta.is_dir() {
            has_something = true;
            continue;
        }
        let name = e.file_name().to_string_lossy().to_lowercase();
        if SKIP.contains(&name.as_str()) {
            has_something = true;
            continue;
        }
        match walk(&e.path(), out, stop) {
            Some(n) => empty_children.push((e.path(), n)),
            None => has_something = true,
        }
    }
    if has_something {
        // Not empty: its empty branches are what is found.
        out.extend(empty_children);
        None
    } else {
        Some(1 + empty_children.iter().map(|(_, n)| n).sum::<usize>())
    }
}

/// The empty folders under `root` (the top one of each empty branch, with how many empty folders it is), by path.
pub fn empty_folders(root: &Path, stop: &std::sync::atomic::AtomicBool) -> Vec<(PathBuf, usize)> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let Ok(meta) = std::fs::symlink_metadata(e.path()) else { continue };
            if is_link(&meta) || !meta.is_dir() || SKIP.contains(&e.file_name().to_string_lossy().to_lowercase().as_str()) {
                continue;
            }
            if let Some(n) = walk(&e.path(), &mut out, stop) {
                out.push((e.path(), n));
            }
        }
    }
    out.sort_by_key(|(p, _)| p.to_string_lossy().to_lowercase());
    out
}

/// The files of a folder, all of them (links left out), to shred it.
pub fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(meta) = std::fs::symlink_metadata(e.path()) else { continue };
            if is_link(&meta) {
                continue;
            }
            if meta.is_dir() {
                stack.push(e.path());
            } else {
                out.push(e.path());
            }
        }
    }
    out
}

/// A small random generator (xorshift), seeded from the clock: the bytes only have to be unlike the file's.
struct Noise(u64);

impl Noise {
    fn new() -> Noise {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        Noise(t | 1)
    }

    fn fill(&mut self, buf: &mut [u8]) {
        for chunk in buf.chunks_mut(8) {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            let b = self.0.to_le_bytes();
            chunk.copy_from_slice(&b[..chunk.len()]);
        }
    }

    fn name(&mut self) -> String {
        let mut b = [0u8; 8];
        self.fill(&mut b);
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}

/// Shreds one file: overwritten with random bytes and flushed, renamed at random, deleted.
pub fn shred(path: &Path) -> Result<(), String> {
    use std::io::{Seek, SeekFrom, Write};
    let meta = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("not a file".into());
    }
    // A read-only file can be written once it is not.
    let mut perm = meta.permissions();
    if perm.readonly() {
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        std::fs::set_permissions(path, perm).map_err(|e| e.to_string())?;
    }
    let len = meta.len();
    let mut noise = Noise::new();
    {
        let mut f = std::fs::OpenOptions::new().write(true).open(path).map_err(|e| e.to_string())?;
        f.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 1 << 20];
        let mut left = len;
        while left > 0 {
            let n = left.min(buf.len() as u64) as usize;
            noise.fill(&mut buf[..n]);
            f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            left -= n as u64;
        }
        f.sync_all().map_err(|e| e.to_string())?;
        f.set_len(0).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    // The name goes too.
    let renamed = path.with_file_name(noise.name());
    let target = if std::fs::rename(path, &renamed).is_ok() { renamed } else { path.to_path_buf() };
    std::fs::remove_file(&target).map_err(|e| e.to_string())
}

/// What a shredding came to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shredded {
    pub done: usize,
    pub bytes: u64,
    pub failed: Vec<(PathBuf, String)>,
}

/// Shreds the files, then the folders given (their files, then the emptied folders, deepest first).
pub fn shred_all(items: &[PathBuf]) -> Shredded {
    let mut s = Shredded::default();
    for item in items {
        let is_dir = std::fs::symlink_metadata(item).is_ok_and(|m| m.is_dir() && !is_link(&m));
        let files = if is_dir { files_in(item) } else { vec![item.clone()] };
        for f in files {
            let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
            match shred(&f) {
                Ok(()) => {
                    s.done += 1;
                    s.bytes += size;
                }
                Err(e) => s.failed.push((f, e)),
            }
        }
        if is_dir {
            // The folders left empty, deepest first.
            let mut dirs: Vec<PathBuf> = Vec::new();
            let mut stack = vec![item.clone()];
            while let Some(d) = stack.pop() {
                if let Ok(rd) = std::fs::read_dir(&d) {
                    for e in rd.flatten() {
                        if std::fs::symlink_metadata(e.path()).is_ok_and(|m| m.is_dir() && !is_link(&m)) {
                            stack.push(e.path());
                        }
                    }
                }
                dirs.push(d);
            }
            for d in dirs.iter().rev() {
                let _ = std::fs::remove_dir(d);
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rcare-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn empty_branches() {
        let root = dir("empty");
        // a/ (empty), b/c/d/ (empty branch of 3), e/f.txt with e/g/ (empty), h/ with a file.
        for p in ["a", "b/c/d", "e/g", "h"] {
            std::fs::create_dir_all(root.join(p)).unwrap();
        }
        std::fs::write(root.join("e/f.txt"), "x").unwrap();
        std::fs::write(root.join("h/file"), "x").unwrap();
        let found = empty_folders(&root, &AtomicBool::new(false));
        let names: Vec<(String, usize)> =
            found.iter().map(|(p, n)| (p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"), *n)).collect();
        assert_eq!(names, vec![("a".to_string(), 1), ("b".to_string(), 3), ("e/g".to_string(), 1)]);
        // Stopped: nothing below is called empty.
        assert!(empty_folders(&root, &AtomicBool::new(true)).is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn shredding() {
        let root = dir("shred");
        let f = root.join("secret.txt");
        std::fs::write(&f, "the secret".repeat(1000)).unwrap();
        let ro = root.join("ro.bin");
        std::fs::write(&ro, [1u8; 3000]).unwrap();
        let mut p = std::fs::metadata(&ro).unwrap().permissions();
        p.set_readonly(true);
        std::fs::set_permissions(&ro, p).unwrap();
        std::fs::create_dir_all(root.join("sub/deeper")).unwrap();
        std::fs::write(root.join("sub/deeper/a"), "a").unwrap();
        let s = shred_all(&[f.clone(), ro.clone(), root.join("sub")]);
        assert_eq!((s.done, s.bytes, s.failed.len()), (3, 10_000 + 3000 + 1, 0), "{:?}", s.failed);
        assert!(!f.exists() && !ro.exists() && !root.join("sub").exists());
        // Nothing of them is left in the folder, not even under another name.
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        assert!(shred(&root.join("gone")).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn noise_is_not_zeros() {
        let mut n = Noise::new();
        let mut b = [0u8; 64];
        n.fill(&mut b);
        assert!(b.iter().filter(|x| **x == 0).count() < 16);
        assert_eq!(n.name().len(), 16);
    }
}
