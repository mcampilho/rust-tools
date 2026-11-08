//! Reading a folder tree from the disk, the fastest way there is: the index
//! of Everything (`everything`), the NTFS table read directly (`ntfs`), or
//! folder by folder through the system. Shared by rsize and rdup, with the
//! tree they are read into (`model`), the drives to offer (`volumes`) and
//! how sizes and dates are shown (`format`). Its visible strings are in
//! `rt-core/src/strings.txt`.
//!
//! Folder by folder:
//!
//! A pool of worker threads lists folders, and one coordinator thread adds
//! what they find to the [`Tree`] and gives them the subfolders to read next.
//! Reading folders in parallel matters most on SSDs and network shares, where
//! each listing waits on the device or the server.
//!
//! The listing itself is per system:
//! - Windows (`win.rs`): `GetFileInformationByHandleEx` with
//!   `FileIdBothDirectoryInfo`, which returns a folder's entries with their
//!   sizes, space on the disk, dates and reparse tags in 64 KB batches.
//! - Linux and others (`unix.rs`): `readdir` and `lstat`, without crossing
//!   into other file systems, and counting hard-linked files once.
//!
//! Symbolic links, junctions and mount points are listed but not followed.

// The MFT reader is plain parsing, tested on every system; only Windows
// opens volumes with it.
#[cfg_attr(not(windows), allow(dead_code))]
pub mod ntfs;
// The same for the answers of Everything.
#[cfg_attr(not(windows), allow(dead_code))]
pub mod everything;
#[cfg(not(windows))]
mod unix;
#[cfg(windows)]
mod win;

pub mod format;
pub mod model;
pub mod volumes;

use crate::model::{Entry, Id, Kind, Tree, F_HARDLINK, F_UNREADABLE};
use rt_core::i18n;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// How a tree was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Folder by folder, through the operating system.
    System,
    /// The NTFS Master File Table, read directly (Windows, administrator).
    Ntfs,
    /// The index of a running Everything (Windows).
    Everything,
}

/// An entry as a lister returns it.
pub(crate) struct Raw {
    pub entry: Entry,
    /// The path to read next, for a folder to enter.
    pub descend: Option<PathBuf>,
    /// (device, inode) of a file with more than one name.
    pub link_key: Option<(u64, u64)>,
}

/// Per-scan settings for the lister.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ctx {
    /// The device of the scanned folder (Unix: other devices are not entered).
    #[cfg_attr(windows, allow(dead_code))]
    pub dev: u64,
}

/// What the coordinator has done so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    /// Folders that could not be read.
    pub errors: u64,
    /// The folder read last.
    pub current: String,
}

/// A finished (or stopped) scan.
pub struct Scan {
    pub tree: Tree,
    pub method: Method,
    pub elapsed: Duration,
    /// False when it was stopped: folders not read yet count as empty.
    pub complete: bool,
    /// The folder is on a local NTFS volume that could have been read
    /// directly with administrator rights.
    pub admin_would_help: bool,
}

pub enum Event {
    Progress(Progress),
    Done(Result<Box<Scan>, String>),
}

/// A scan running in the background.
pub struct Handle {
    cancel: Arc<AtomicBool>,
}

impl Handle {
    /// Stops the scan; it then finishes with what it has read.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Starts scanning `root` in the background. `sink` receives progress about
/// ten times a second, then the result.
pub fn start(root: PathBuf, sink: Box<dyn Fn(Event) + Send>) -> Handle {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let lang = i18n::lang();
    std::thread::spawn(move || {
        i18n::set_lang(lang);
        let result = scan(&root, &flag, &|p| sink(Event::Progress(p.clone())));
        sink(Event::Done(result.map(Box::new)));
    });
    Handle { cancel }
}

/// The form of a path the scanner uses: absolute, and on Windows with
/// backslashes and a backslash after a drive letter (`C:` is the current
/// folder on C:, not its root).
pub fn normalize(root: &Path) -> PathBuf {
    let abs = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
    if cfg!(windows) {
        let mut s = abs.to_string_lossy().replace('/', "\\");
        if s.len() == 2 && s.ends_with(':') {
            s.push('\\');
        }
        PathBuf::from(s)
    } else {
        abs
    }
}

struct Queue {
    jobs: Mutex<(Vec<(PathBuf, Id)>, bool)>,
    ready: Condvar,
}

impl Queue {
    fn push(&self, job: (PathBuf, Id)) {
        if let Ok(mut q) = self.jobs.lock() {
            q.0.push(job);
            self.ready.notify_one();
        }
    }

    fn stop(&self) {
        if let Ok(mut q) = self.jobs.lock() {
            q.1 = true;
            q.0.clear();
            self.ready.notify_all();
        }
    }

    /// The next job, or None once stopped.
    fn pop(&self) -> Option<(PathBuf, Id)> {
        let mut q = self.jobs.lock().ok()?;
        loop {
            if q.1 {
                return None;
            }
            if let Some(j) = q.0.pop() {
                return Some(j);
            }
            q = self.ready.wait(q).ok()?;
        }
    }
}

type Listed = (Id, PathBuf, std::io::Result<Vec<Raw>>);

/// The ways to read a folder besides the system's, tried in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ways {
    /// The index of a running Everything (Windows).
    pub everything: bool,
    /// The NTFS table, read directly (Windows, administrator).
    pub ntfs: bool,
}

impl Ways {
    pub const ALL: Ways = Ways { everything: true, ntfs: true };
    /// Folder by folder only.
    pub const SYSTEM: Ways = Ways { everything: false, ntfs: false };
}

/// Scans `root` the fastest way available, calling `progress` now and then.
/// Returns an error only when `root` itself cannot be read.
pub fn scan(root: &Path, cancel: &AtomicBool, progress: &dyn Fn(&Progress)) -> Result<Scan, String> {
    scan_with(root, Ways::ALL, cancel, progress)
}

/// Scans `root`, trying the `ways` first, then folder by folder.
pub fn scan_with(root: &Path, ways: Ways, cancel: &AtomicBool, progress: &dyn Fn(&Progress)) -> Result<Scan, String> {
    let start = Instant::now();
    let root = normalize(root);
    let meta = std::fs::metadata(&root).map_err(|e| e.to_string())?;
    if !meta.is_dir() {
        return Err(std::io::Error::from(std::io::ErrorKind::NotADirectory).to_string());
    }
    let stopped = |method: Method| -> Result<Scan, String> {
        let mut tree = Tree::new(&root.to_string_lossy(), mtime_of(&meta));
        tree.finish();
        Ok(Scan { tree, method, elapsed: start.elapsed(), complete: false, admin_would_help: false })
    };
    #[allow(unused_mut)]
    let mut admin_would_help = false;
    #[cfg(windows)]
    if ways.everything {
        match everything::scan_index(&root, cancel, progress) {
            Ok(tree) => return Ok(Scan { tree, method: Method::Everything, elapsed: start.elapsed(), complete: true, admin_would_help: false }),
            Err(everything::Skip::Stopped) => return stopped(Method::Everything),
            Err(everything::Skip::NotRunning | everything::Skip::NotIndexed) => {}
        }
    }
    #[cfg(windows)]
    if ways.ntfs {
        let label = |pct: u64| i18n::tf("Reading the NTFS table… {}%", &[&pct.to_string()]);
        match ntfs::scan_volume(&root, cancel, progress, &label) {
            Ok(tree) => return Ok(Scan { tree, method: Method::Ntfs, elapsed: start.elapsed(), complete: true, admin_would_help: false }),
            Err(ntfs::Skip::Stopped) => return stopped(Method::Ntfs),
            Err(ntfs::Skip::Denied) => admin_would_help = true,
            // Not NTFS, or something unexpected in the table: read folder by folder.
            Err(ntfs::Skip::NotNtfs | ntfs::Skip::Failed(_)) => {}
        }
    }
    let _ = (ways, &stopped);
    let ctx = Ctx { dev: device_of(&meta) };
    let mut tree = Tree::new(&root.to_string_lossy(), mtime_of(&meta));
    // The root must be readable; anything below may fail on its own.
    let first = list(&root, ctx).map_err(|e| e.to_string())?;

    let queue = Arc::new(Queue { jobs: Mutex::new((Vec::new(), false)), ready: Condvar::new() });
    let (tx, rx) = mpsc::channel::<Listed>();
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
    for _ in 0..workers {
        let (q, tx) = (queue.clone(), tx.clone());
        std::thread::spawn(move || {
            while let Some((path, id)) = q.pop() {
                let r = list(&path, ctx);
                if tx.send((id, path, r)).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);

    let mut p = Progress::default();
    let mut links: HashSet<(u64, u64)> = HashSet::new();
    let mut pending = 0usize;
    let mut last = Instant::now();
    let mut add = |tree: &mut Tree, p: &mut Progress, id: Id, raws: Vec<Raw>, pending: &mut usize| {
        let mut entries = Vec::with_capacity(raws.len());
        let mut descend = Vec::new();
        for (i, mut r) in raws.into_iter().enumerate() {
            if let Some(key) = r.link_key {
                if !links.insert(key) {
                    r.entry.size = 0;
                    r.entry.alloc = 0;
                    r.entry.flags |= F_HARDLINK;
                }
            }
            match r.entry.kind {
                Kind::Dir => p.dirs += 1,
                _ => {
                    p.files += 1;
                    p.bytes += r.entry.size;
                }
            }
            if let Some(path) = r.descend {
                descend.push((i, path));
            }
            entries.push(r.entry);
        }
        let first = tree.add_children(id, &entries);
        for (i, path) in descend {
            queue.push((path, first + i as Id));
            *pending += 1;
        }
    };
    add(&mut tree, &mut p, 0, first, &mut pending);
    p.current = root.to_string_lossy().into_owned();
    progress(&p);

    let mut complete = true;
    while pending > 0 {
        if cancel.load(Ordering::Relaxed) {
            complete = false;
            break;
        }
        let Ok((id, path, result)) = rx.recv_timeout(Duration::from_millis(100)) else { continue };
        pending -= 1;
        match result {
            Ok(raws) => add(&mut tree, &mut p, id, raws, &mut pending),
            Err(_) => {
                tree.set_flag(id, F_UNREADABLE);
                p.errors += 1;
            }
        }
        if last.elapsed() >= Duration::from_millis(100) {
            last = Instant::now();
            p.current = path.to_string_lossy().into_owned();
            progress(&p);
        }
    }
    queue.stop();
    tree.finish();
    Ok(Scan { tree, method: Method::System, elapsed: start.elapsed(), complete, admin_would_help })
}

fn list(path: &Path, ctx: Ctx) -> std::io::Result<Vec<Raw>> {
    #[cfg(windows)]
    return win::list(path, ctx);
    #[cfg(not(windows))]
    return unix::list(path, ctx);
}

#[cfg(unix)]
fn device_of(m: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::dev(m)
}

#[cfg(not(unix))]
fn device_of(_: &std::fs::Metadata) -> u64 {
    0
}

/// Seconds since 1970 of the last change.
pub(crate) fn mtime_of(m: &std::fs::Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// Helpers for the tests of the apps that read with this crate.
pub mod testing {
    use std::path::PathBuf;

    /// A temporary folder tree, removed when dropped.
    pub struct TempTree(pub PathBuf);

    impl TempTree {
        /// A new empty folder in the temporary folder, named after `tag`.
        pub fn new(tag: &str) -> TempTree {
            let dir = std::env::temp_dir().join(format!("rt-scan-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("temporary folder");
            TempTree(dir)
        }

        /// A file at `rel` (folders created as needed) of `len` bytes 'x'.
        pub fn file(&self, rel: &str, len: usize) {
            self.write(rel, &vec![b'x'; len]);
        }

        /// A file at `rel` with these contents.
        pub fn write(&self, rel: &str, data: &[u8]) {
            let p = self.0.join(rel);
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir).expect("folder");
            }
            std::fs::write(p, data).expect("file");
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Folder by folder: what the tests check, whatever else this PC has.
    pub(crate) fn sys(root: &Path, cancel: &AtomicBool, progress: &dyn Fn(&Progress)) -> Result<Scan, String> {
        scan_with(root, Ways::SYSTEM, cancel, progress)
    }

    pub(crate) use crate::testing::TempTree;

    fn child(t: &Tree, parent: Id, name: &str) -> Id {
        *t.children(parent).iter().find(|&&c| t.name(c) == name).unwrap_or_else(|| panic!("{name} not found"))
    }

    #[test]
    fn scans_a_folder_tree() {
        let tt = TempTree::new("scan");
        tt.file("a.txt", 100);
        tt.file("docs/b.md", 2000);
        tt.file("docs/deep/c.bin", 30_000);
        tt.file("docs/deep/ação ü.txt", 5);
        std::fs::create_dir_all(tt.0.join("empty")).unwrap();
        let calls = std::cell::Cell::new(0);
        let s = sys(&tt.0, &AtomicBool::new(false), &|_| calls.set(calls.get() + 1)).unwrap();
        assert!(s.complete && calls.get() >= 1);
        let t = &s.tree;
        let root = t.node(0);
        assert_eq!(root.size, 32_105);
        assert_eq!(root.files, 4);
        assert_eq!(root.dirs, 3);
        let docs = child(t, 0, "docs");
        assert_eq!(t.node(docs).size, 32_005);
        assert_eq!(t.name(t.children(0)[0]), "docs", "largest first");
        let deep = child(t, docs, "deep");
        let odd = child(t, deep, "ação ü.txt");
        assert_eq!(t.path(odd), normalize(&tt.0).join("docs").join("deep").join("ação ü.txt"));
        assert_eq!(t.node(child(t, 0, "empty")).size, 0);
        assert_eq!(t.unreadable(), 0);
    }

    #[test]
    fn a_missing_or_file_root_is_an_error() {
        let tt = TempTree::new("missing");
        assert!(sys(&tt.0.join("nope"), &AtomicBool::new(false), &|_| {}).is_err());
        tt.file("f", 1);
        assert!(sys(&tt.0.join("f"), &AtomicBool::new(false), &|_| {}).is_err());
    }

    #[test]
    fn a_cancelled_scan_keeps_what_it_read() {
        let tt = TempTree::new("cancel");
        for i in 0..20 {
            tt.file(&format!("d{i}/e/f.txt"), 10);
        }
        let s = sys(&tt.0, &AtomicBool::new(true), &|_| {}).unwrap();
        assert!(!s.complete);
        // The root was listed before the first check.
        assert_eq!(s.tree.children(0).len(), 20);
    }

    /// Times a scan of `RSIZE_BENCH_DIR`:
    /// `cargo test -p rt-scan --release --lib bench -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn bench() {
        let Some(dir) = std::env::var_os("RSIZE_BENCH_DIR") else { return };
        let s = scan(Path::new(&dir), &AtomicBool::new(false), &|_| {}).unwrap();
        let t = &s.tree;
        let root = t.node(0);
        println!(
            "{:?}: {} nodes, {} files, {} folders, {} bytes ({} on disk), {} unreadable, in {:.2?}",
            s.method,
            t.len(),
            root.files,
            root.dirs,
            root.size,
            root.alloc,
            t.unreadable(),
            s.elapsed
        );
    }

    #[cfg(windows)]
    #[test]
    fn drive_roots_keep_their_backslash() {
        assert_eq!(normalize(Path::new("C:")), PathBuf::from("C:\\"));
        assert_eq!(normalize(Path::new("C:/Windows")), PathBuf::from("C:\\Windows"));
    }

    #[cfg(unix)]
    #[test]
    fn links_are_not_followed_and_hard_links_count_once() {
        let tt = TempTree::new("links");
        tt.file("real/data.bin", 50_000);
        std::os::unix::fs::symlink(tt.0.join("real"), tt.0.join("link")).unwrap();
        std::fs::hard_link(tt.0.join("real/data.bin"), tt.0.join("twin.bin")).unwrap();
        let s = sys(&tt.0, &AtomicBool::new(false), &|_| {}).unwrap();
        let t = &s.tree;
        assert_eq!(t.node(0).size, 50_000, "the second name of the file adds nothing");
        let link = child(t, 0, "link");
        assert_eq!(t.node(link).kind, Kind::Link);
        assert!(t.children(link).is_empty());
        let names = [child(t, 0, "twin.bin"), child(t, child(t, 0, "real"), "data.bin")];
        assert_eq!(names.iter().filter(|&&n| t.node(n).flags & F_HARDLINK != 0).count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_folders_are_marked() {
        use std::os::unix::fs::PermissionsExt;
        if unsafe { libc::geteuid() } == 0 {
            return; // root reads everything
        }
        let tt = TempTree::new("perm");
        tt.file("locked/x", 10);
        std::fs::set_permissions(tt.0.join("locked"), std::fs::Permissions::from_mode(0o000)).unwrap();
        let s = sys(&tt.0, &AtomicBool::new(false), &|_| {}).unwrap();
        std::fs::set_permissions(tt.0.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(s.tree.unreadable(), 1);
        assert_eq!(s.tree.node(child(&s.tree, 0, "locked")).flags & F_UNREADABLE, F_UNREADABLE);
    }
}
