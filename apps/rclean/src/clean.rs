//! Looking at what a target holds (its files and their size), and deleting
//! them for good: temporary files and caches are made to be thrown away, so
//! they do not fill the Recycle Bin.
//!
//! Links and junctions are never followed nor deleted (what they point to is
//! somewhere else). A file in use cannot be deleted: it is counted and left.
//! On Linux, only the user's own files count (`/tmp` is everyone's).

use crate::targets::{Filter, Kind, Place, Target};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

/// What a target holds now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    /// The files that would go, with their sizes.
    pub files: Vec<(PathBuf, u64)>,
    pub bytes: u64,
    /// Folders inside the places to remove once they are empty, the deepest
    /// first.
    pub dirs: Vec<PathBuf>,
    /// Folders that could not be read (no rights).
    pub unreadable: u64,
    /// The Recycle Bin: its count and size, from the shell.
    pub items: u64,
}

impl Found {
    pub fn count(&self) -> u64 {
        self.files.len() as u64 + self.items
    }
}

/// How cleaning went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cleaned {
    pub bytes: u64,
    pub files: u64,
    /// Files that stayed: in use, or no rights.
    pub failed: u64,
}

/// Whether a file of `place` goes, by its name and when it changed.
fn wanted(place: &Place, name: &str, modified: Option<SystemTime>, now: SystemTime) -> bool {
    match place.filter {
        Filter::All => true,
        Filter::OlderThanHours(h) => modified.and_then(|m| now.duration_since(m).ok()).is_some_and(|age| age >= Duration::from_secs(h * 3600)),
        Filter::Prefix(p) => name.to_lowercase().starts_with(&p.to_lowercase()),
        Filter::Named(n) => name.eq_ignore_ascii_case(n),
    }
}

/// The user running rclean (Linux: the owner of `/proc/self`), to leave
/// others' files in shared folders.
#[cfg(unix)]
fn my_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

#[cfg(unix)]
fn mine(meta: &std::fs::Metadata, uid: Option<u32>) -> bool {
    use std::os::unix::fs::MetadataExt;
    uid.is_none_or(|u| u == 0 || meta.uid() == u)
}

#[cfg(not(unix))]
fn mine(_: &std::fs::Metadata, _: Option<u32>) -> bool {
    true
}

/// Looks at what `t` holds.
pub fn analyse(t: &Target, stop: &AtomicBool) -> Found {
    let mut f = Found::default();
    if t.kind == Kind::RecycleBin {
        if let Some((bytes, items)) = recycle_bin() {
            f.bytes = bytes;
            f.items = items;
        }
        return f;
    }
    #[cfg(unix)]
    let uid = my_uid();
    #[cfg(not(unix))]
    let uid: Option<u32> = None;
    let now = SystemTime::now();
    for place in &t.places {
        let mut stack = vec![(place.dir.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            if stop.load(Ordering::Relaxed) {
                return f;
            }
            let Ok(rd) = std::fs::read_dir(&dir) else {
                if depth > 0 || place.dir.exists() {
                    f.unreadable += 1;
                }
                continue;
            };
            for e in rd.flatten() {
                let Ok(ft) = e.file_type() else { continue };
                // Links and junctions: neither followed nor deleted.
                if ft.is_symlink() {
                    continue;
                }
                let Ok(meta) = e.metadata() else { continue };
                if !mine(&meta, uid) {
                    continue;
                }
                if ft.is_dir() {
                    if place.deep() {
                        // Kept only if it was old enough too (its time changes
                        // when files leave it).
                        if wanted(place, "", meta.modified().ok(), now) {
                            f.dirs.push(e.path());
                        }
                        stack.push((e.path(), depth + 1));
                    }
                    continue;
                }
                if wanted(place, &e.file_name().to_string_lossy(), meta.modified().ok(), now) {
                    f.bytes += meta.len();
                    f.files.push((e.path(), meta.len()));
                }
            }
        }
    }
    // The deepest first, so a folder is empty when its turn comes.
    f.dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    f
}

/// Deletes what `found` lists (for good), then the folders left empty.
pub fn clean(t: &Target, found: &Found, stop: &AtomicBool) -> Cleaned {
    let mut c = Cleaned::default();
    if t.kind == Kind::RecycleBin {
        if empty_recycle_bin() {
            c.bytes = found.bytes;
            c.files = found.items;
        } else {
            c.failed = found.items.max(1);
        }
        return c;
    }
    for (path, size) in &found.files {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match remove_file(path) {
            Ok(()) => {
                c.bytes += size;
                c.files += 1;
            }
            Err(_) if !path.exists() => {}
            Err(_) => c.failed += 1,
        }
    }
    for d in &found.dirs {
        // Only empty ones: what is left in them stays.
        let _ = std::fs::remove_dir(d);
    }
    c
}

/// Deletes a file; a read-only one too (Windows will not delete it otherwise).
fn remove_file(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && cfg!(windows) => {
            let mut perm = std::fs::metadata(path)?.permissions();
            if !perm.readonly() {
                return Err(e);
            }
            #[allow(clippy::permissions_set_readonly_false)]
            perm.set_readonly(false);
            std::fs::set_permissions(path, perm)?;
            std::fs::remove_file(path)
        }
        r => r,
    }
}

/// The Recycle Bin's size and count, on every drive.
#[cfg(windows)]
fn recycle_bin() -> Option<(u64, u64)> {
    use windows_sys::Win32::UI::Shell::{SHQueryRecycleBinW, SHQUERYRBINFO};
    let mut info = SHQUERYRBINFO { cbSize: std::mem::size_of::<SHQUERYRBINFO>() as u32, i64Size: 0, i64NumItems: 0 };
    // SAFETY: a null root asks for every drive; `info` is sized as required.
    let hr = unsafe { SHQueryRecycleBinW(std::ptr::null(), &mut info) };
    (hr >= 0).then_some((info.i64Size.max(0) as u64, info.i64NumItems.max(0) as u64))
}

#[cfg(not(windows))]
fn recycle_bin() -> Option<(u64, u64)> {
    None
}

/// Empties the Recycle Bin of every drive, without asking (rclean has asked).
#[cfg(windows)]
fn empty_recycle_bin() -> bool {
    use windows_sys::Win32::UI::Shell::{SHEmptyRecycleBinW, SHERB_NOCONFIRMATION, SHERB_NOPROGRESSUI, SHERB_NOSOUND};
    // SAFETY: no window, every drive, no question, no sound.
    let hr = unsafe { SHEmptyRecycleBinW(std::ptr::null_mut(), std::ptr::null(), SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND) };
    // An empty bin answers with an error: nothing was there.
    hr >= 0 || recycle_bin().is_some_and(|(_, n)| n == 0)
}

#[cfg(not(windows))]
fn empty_recycle_bin() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::Group;

    fn target(places: Vec<Place>) -> Target {
        Target { id: "t".into(), group: Group::System, name: "", about: "", places, kind: Kind::Files, admin: false, default_on: true, process: &[] }
    }

    #[test]
    fn analyse_and_clean() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("clean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cache = root.join("cache");
        std::fs::create_dir_all(cache.join("a").join("b")).unwrap();
        std::fs::write(cache.join("one.bin"), [0u8; 100]).unwrap();
        std::fs::write(cache.join("a").join("b").join("two.bin"), [0u8; 50]).unwrap();
        let ro = cache.join("a").join("readonly.bin");
        std::fs::write(&ro, [0u8; 10]).unwrap();
        let mut perm = std::fs::metadata(&ro).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&ro, perm).unwrap();
        let other = root.join("explorer");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("thumbcache_256.db"), [0u8; 7]).unwrap();
        std::fs::write(other.join("iconcache_16.db"), [0u8; 7]).unwrap();
        let fresh = root.join("temp");
        std::fs::create_dir_all(&fresh).unwrap();
        std::fs::write(fresh.join("in-use.tmp"), [0u8; 3]).unwrap();
        let t = target(vec![
            Place { dir: cache.clone(), filter: Filter::All },
            Place { dir: other.clone(), filter: Filter::Prefix("thumbcache_") },
            Place { dir: fresh.clone(), filter: Filter::OlderThanHours(24) },
            Place { dir: root.join("missing"), filter: Filter::All },
        ]);
        let stop = AtomicBool::new(false);
        let f = analyse(&t, &stop);
        assert_eq!((f.files.len(), f.bytes, f.unreadable), (4, 167, 0), "{:?}", f.files);
        assert_eq!(f.dirs, [cache.join("a").join("b"), cache.join("a")], "the deepest first");
        let c = clean(&t, &f, &stop);
        assert_eq!((c.files, c.bytes, c.failed), (4, 167, 0));
        assert!(cache.is_dir() && std::fs::read_dir(&cache).unwrap().next().is_none(), "the place itself stays, empty");
        assert!(other.join("iconcache_16.db").exists() && fresh.join("in-use.tmp").exists());
        assert_eq!(analyse(&t, &stop).count(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}
