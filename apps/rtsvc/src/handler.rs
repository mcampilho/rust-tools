//! What the service answers beyond `Hello`, `Ping` and `Status`: for now,
//! reading the NTFS table of a volume.

use crate::snapshots::{now, Miss, Store, Viewer};
use rt_scan::model::Tree;
use rt_scan::Progress;
use rt_svc::frame::{kind, write_frame, Frame};
use rt_svc::proto::{caps, code, encode_batch, encode_done, encode_error, ScanProgress, ScanRequest, ScanStart, SnapshotRequest};
use rt_svc::server::{Handler, Session};
use rt_svc::Visibility;
use std::cell::RefCell;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

/// The most scans at once: reading a table takes memory and the disk's time.
const MAX_SCANS: u32 = 2;

/// Why a source of trees gave none.
#[derive(Debug, PartialEq, Eq)]
pub enum SourceError {
    NotNtfs,
    Stopped,
    Failed(String),
}

/// Where the trees come from: the NTFS table, or something else in the tests.
/// It receives the root, a flag to stop it, a progress callback, and the
/// function that makes a text of a percentage.
pub type Source = Box<dyn Fn(&Path, &AtomicBool, &dyn Fn(&Progress), &dyn Fn(u64) -> String) -> Result<Tree, SourceError> + Send + Sync>;

pub struct Svc {
    visibility: Visibility,
    scans: AtomicU64,
    running: AtomicU32,
    source: Source,
    store: Arc<Store>,
}

/// A full path on a drive letter (`C:\...`), without `..`.
pub fn valid_root(root: &str) -> Result<PathBuf, &'static str> {
    let p = Path::new(root);
    let mut parts = p.components();
    match (parts.next(), parts.next()) {
        (Some(Component::Prefix(pre)), Some(Component::RootDir)) if matches!(pre.kind(), Prefix::Disk(_)) => {}
        _ => return Err("the folder must be a full path on a drive"),
    }
    // components quietly drops a . in the middle: look at the text.
    if root.split(['/', '\\']).any(|seg| seg == "." || seg == "..") {
        return Err("the folder must not have . or .. in it");
    }
    Ok(p.to_path_buf())
}

/// The NTFS table of a local fixed drive, read directly (the service runs with the rights to).
#[cfg(windows)]
pub fn ntfs_source() -> Source {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    Box::new(|root, cancel, progress, label| {
        let mut drive: Vec<u16> = root.to_string_lossy().chars().take(3).map(|c| c as u16).collect();
        drive.push(0);
        // SAFETY: a valid, terminated string.
        if unsafe { GetDriveTypeW(drive.as_ptr()) } != 3 {
            return Err(SourceError::Failed("the folder is not on a fixed local drive".into()));
        }
        rt_scan::ntfs::scan_volume(root, cancel, progress, label).map_err(|e| match e {
            rt_scan::ntfs::Skip::NotNtfs => SourceError::NotNtfs,
            rt_scan::ntfs::Skip::Stopped => SourceError::Stopped,
            rt_scan::ntfs::Skip::Denied => SourceError::Failed("the volume cannot be opened".into()),
            rt_scan::ntfs::Skip::Failed(e) => SourceError::Failed(e.to_string()),
        })
    })
}

impl Svc {
    pub fn new(visibility: Visibility, source: Source, store: Arc<Store>) -> Svc {
        Svc { visibility, scans: AtomicU64::new(0), running: AtomicU32::new(0), source, store }
    }

    fn snapshot(&self, s: &Session, f: &Frame, out: &mut dyn Write) -> io::Result<()> {
        let fail = |out: &mut dyn Write, c: u16, m: &str| write_frame(out, &Frame::new(kind::ERROR, f.id, encode_error(c, m)));
        let Ok(q) = SnapshotRequest::decode(&f.payload) else { return fail(out, code::MALFORMED, "the request is malformed") };
        if f.kind == kind::REFRESH {
            return if self.store.refresh(&q.name) {
                write_frame(out, &Frame::new(kind::REFRESH_OK, f.id, Vec::new()))
            } else {
                fail(out, code::NO_SNAPSHOT, "there is no such snapshot")
            };
        }
        let viewer = Viewer { level: s.level, account: s.who.name.clone() };
        match self.store.get(&q.name, q.max_age_secs, &viewer, now()) {
            Ok(d) => write_frame(out, &Frame::new(kind::SNAPSHOT, f.id, d.encode())),
            Err(Miss::Hidden) => fail(out, code::DENIED, "this snapshot is not for this client"),
            Err(Miss::Unknown) => fail(out, code::NO_SNAPSHOT, "there is no such snapshot"),
            Err(Miss::NotMade) => fail(out, code::NO_SNAPSHOT, "the snapshot is not made yet"),
            Err(Miss::TooOld(age)) => fail(out, code::NO_SNAPSHOT, &format!("the snapshot is {age} s old")),
        }
    }

    fn scan(&self, s: &Session, f: &Frame, out: &mut dyn Write) -> io::Result<()> {
        let fail = |out: &mut dyn Write, c: u16, m: &str| write_frame(out, &Frame::new(kind::ERROR, f.id, encode_error(c, m)));
        if !self.visibility.allows(s.level) {
            return fail(out, code::DENIED, "reading volumes is not allowed for this client");
        }
        let Ok(q) = ScanRequest::decode(&f.payload) else { return fail(out, code::MALFORMED, "the request is malformed") };
        let root = match valid_root(&q.root) {
            Ok(r) => r,
            Err(m) => return fail(out, code::FAILED, m),
        };
        if self.running.fetch_add(1, Ordering::SeqCst) >= MAX_SCANS {
            self.running.fetch_sub(1, Ordering::SeqCst);
            return fail(out, code::BUSY, "other scans are running: try again soon");
        }
        let result = self.scan_inner(f.id, &root, out);
        self.running.fetch_sub(1, Ordering::SeqCst);
        match result {
            Ok(()) => {
                self.scans.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(Failure::Source(SourceError::NotNtfs)) => fail(out, code::NOT_NTFS, "the volume is not NTFS"),
            Err(Failure::Source(SourceError::Stopped)) => fail(out, code::CANCELLED, "stopped"),
            Err(Failure::Source(SourceError::Failed(m))) => fail(out, code::FAILED, &m),
            Err(Failure::Io(e)) => Err(e),
        }
    }

    fn scan_inner(&self, id: u32, root: &Path, out: &mut dyn Write) -> Result<(), Failure> {
        let cancel = AtomicBool::new(false);
        let tree = {
            let sink = RefCell::new(&mut *out);
            let progress = |p: &Progress| {
                let percent = p.current.strip_prefix("pct:").and_then(|n| n.parse::<u8>().ok()).unwrap_or(255);
                let msg = ScanProgress { files: p.files, dirs: p.dirs, bytes: p.bytes, errors: p.errors, percent };
                // A client that is gone ends the scan.
                if write_frame(*sink.borrow_mut(), &Frame::new(kind::SCAN_PROGRESS, id, msg.encode())).is_err() {
                    cancel.store(true, Ordering::Relaxed);
                }
            };
            (self.source)(root, &cancel, &progress, &|pct| format!("pct:{pct}")).map_err(Failure::Source)?
        };
        let start = ScanStart { root: tree.name(tree.root()).to_string(), mtime: tree.node(tree.root()).mtime };
        write_frame(out, &Frame::new(kind::SCAN_START, id, start.encode()))?;
        let nodes =
            rt_scan::wire::send_tree(&tree, &mut |chunks| write_frame(out, &Frame::new(kind::SCAN_BATCH, id, encode_batch(&chunks))))?;
        write_frame(out, &Frame::new(kind::SCAN_DONE, id, encode_done(nodes)))?;
        Ok(())
    }
}

enum Failure {
    Source(SourceError),
    Io(io::Error),
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Failure {
        Failure::Io(e)
    }
}

impl Handler for Svc {
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    fn caps(&self) -> u32 {
        caps::SCAN
    }

    fn scans(&self) -> u64 {
        self.scans.load(Ordering::Relaxed)
    }

    fn snapshots(&self) -> Vec<(String, u64)> {
        self.store.status(now())
    }

    fn request(&self, s: &Session, f: &Frame, out: &mut dyn Write) -> io::Result<()> {
        match f.kind {
            kind::SCAN_VOLUME => self.scan(s, f, out),
            kind::GET_SNAPSHOT | kind::REFRESH => self.snapshot(s, f, out),
            _ => write_frame(out, &Frame::new(kind::ERROR, f.id, encode_error(code::UNKNOWN, "unknown request"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_scan::model::{Entry, Kind};
    use rt_svc::client::{connect_to, Trust};
    use rt_svc::pipe::Event;
    use rt_svc::server::run;
    use rt_svc::Level;
    use std::sync::Arc;

    fn sample() -> Tree {
        let e = |name: &str, kind: Kind, size: u64| Entry {
            name: name.into(),
            kind,
            size,
            alloc: size.div_ceil(4096) * 4096,
            mtime: 7,
            flags: 0,
        };
        let mut t = Tree::new(r"C:\data", 100);
        t.add_children(0, &[e("a.txt", Kind::File, 10), e("docs", Kind::Dir, 0)]);
        t.add_children(2, &[e("b.md", Kind::File, 5000)]);
        t.finish();
        t
    }

    fn serve(visibility: Visibility, source: Source) -> (String, Arc<Event>, std::thread::JoinHandle<()>) {
        serve_store(visibility, source, Arc::new(Store::new(vec![], None)))
    }

    fn serve_store(visibility: Visibility, source: Source, store: Arc<Store>) -> (String, Arc<Event>, std::thread::JoinHandle<()>) {
        static N: AtomicU32 = AtomicU32::new(0);
        let name = format!(r"\\.\pipe\rtsvc-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
        let stop = Arc::new(Event::new().unwrap());
        let (n, s) = (name.clone(), stop.clone());
        let t = std::thread::spawn(move || run(&n, Arc::new(Svc::new(visibility, source, store)), s, Arc::new(|_| {})).unwrap());
        for _ in 0..100 {
            if rt_svc::pipe::connect(&name, 0).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        (name, stop, t)
    }

    fn fake(tree: fn() -> Tree) -> Source {
        Box::new(move |_, _, progress, label| {
            progress(&Progress { files: 3, dirs: 1, current: label(50), ..Progress::default() });
            Ok(tree())
        })
    }

    #[test]
    fn roots_are_full_paths_on_a_drive() {
        assert!(valid_root(r"C:\Users\me").is_ok() && valid_root(r"d:\").is_ok());
        for bad in ["", "docs", r"\\server\share", r"\\?\C:\x", r"C:", r"C:\a\..\b", r"C:\.\a", r"\Windows"] {
            assert!(valid_root(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_scan_goes_through_the_pipe_and_arrives_whole() {
        let (name, stop, t) = serve(Visibility::User, fake(sample));
        let mut c = connect_to(&name, Trust::Anyone, "test", "1", 1000).unwrap();
        let cancel = AtomicBool::new(false);
        let seen = std::cell::Cell::new(0);
        let progress = |p: &rt_scan::Progress| {
            assert!(p.files == 3);
            seen.set(seen.get() + 1);
        };
        let mut sink = rt_scan::wire::TreeSink::new(&progress);
        let nodes = c.scan_volume(r"C:\data", &cancel, &mut sink).unwrap();
        let tree = sink.finish().unwrap();
        assert_eq!((nodes as usize, tree.len()), (4, 4));
        assert_eq!((tree.node(0).size, tree.node(0).files, tree.name(0)), (5010, 2, r"C:\data"));
        assert_eq!(seen.get(), 1);
        assert_eq!(c.status().unwrap().scans, 1);
        stop.set();
        t.join().unwrap();
    }

    #[test]
    fn visibility_decides_who_may_read() {
        // This test process is not elevated (or is: either way the other setting is exercised below).
        let elevated = rt_svc::access::is_elevated();
        let (name, stop, t) = serve(Visibility::Elevated, fake(sample));
        let mut c = connect_to(&name, Trust::Anyone, "test", "1", 1000).unwrap();
        let mut sink = rt_scan::wire::TreeSink::new(&|_| {});
        let r = c.scan_volume(r"C:\data", &AtomicBool::new(false), &mut sink);
        assert_eq!(r.is_ok(), elevated, "{r:?}");
        if !elevated {
            assert!(r.unwrap_err().to_string().contains("not allowed"));
            assert!(c.ping().is_ok(), "the connection stays good");
        }
        assert_eq!(c.level == Level::Elevated, elevated);
        stop.set();
        t.join().unwrap();
    }

    #[test]
    fn bad_roots_and_sources_are_answers_not_crashes() {
        let sources: Vec<(Source, &str)> = vec![
            (Box::new(|_, _, _, _| Err(SourceError::NotNtfs)), "not NTFS"),
            (Box::new(|_, _, _, _| Err(SourceError::Failed("no way".into()))), "no way"),
        ];
        for (source, text) in sources {
            let (name, stop, t) = serve(Visibility::User, source);
            let mut c = connect_to(&name, Trust::Anyone, "test", "1", 1000).unwrap();
            let mut sink = rt_scan::wire::TreeSink::new(&|_| {});
            let e = c.scan_volume(r"C:\data", &AtomicBool::new(false), &mut sink).unwrap_err();
            assert!(e.to_string().contains(text) || e.to_string().contains("NTFS"), "{e}");
            let e = c.scan_volume(r"..\up", &AtomicBool::new(false), &mut rt_scan::wire::TreeSink::new(&|_| {})).unwrap_err();
            assert!(e.to_string().contains("full path"), "{e}");
            assert!(c.ping().is_ok());
            stop.set();
            t.join().unwrap();
        }
    }

    #[test]
    fn snapshots_are_asked_for_made_and_given() {
        use crate::snapshots::Kind;
        let kind = Kind {
            name: "programs",
            max_age: 1000,
            min_gap: 0,
            produce: Box::new(|_| vec![4, 5, 6]),
            view: Box::new(|v, d| (v.level != Level::Denied).then(|| d.to_vec())),
        };
        let store = Arc::new(Store::new(vec![kind], None));
        let (name, stop, t) = serve_store(Visibility::User, fake(sample), store.clone());
        let mut c = connect_to(&name, Trust::Anyone, "test", "1", 1000).unwrap();
        // Not made yet: the answer says so, and the miss asks for it.
        assert!(c.snapshot("programs", 60).unwrap_err().to_string().contains("not made yet"));
        assert_eq!(store.tick(now(), false, &AtomicBool::new(false)), 1);
        let s = c.snapshot("programs", 60).unwrap();
        assert_eq!(s.data, vec![4, 5, 6]);
        assert!(s.age_secs < 5);
        assert!(c.snapshot("nope", 60).unwrap_err().to_string().contains("no such snapshot"));
        assert!(c.refresh("programs").is_ok() && c.refresh("nope").is_err());
        assert_eq!(c.status().unwrap().snapshots.len(), 1);
        stop.set();
        t.join().unwrap();
    }

    #[test]
    fn a_stopped_scan_ends_with_the_client() {
        let (name, stop, t) = serve(Visibility::User, fake(sample));
        let mut c = connect_to(&name, Trust::Anyone, "test", "1", 1000).unwrap();
        let cancel = AtomicBool::new(true);
        let mut sink = rt_scan::wire::TreeSink::new(&|_| {});
        let e = c.scan_volume(r"C:\data", &cancel, &mut sink).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Interrupted);
        stop.set();
        t.join().unwrap();
    }
}
