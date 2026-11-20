//! A tree on the wire to and from the privileged service: the service reads
//! the NTFS table and sends the folders in breadth-first order; the client
//! builds the same tree. The numbers of the folders are the order the
//! entries are sent in (the root is 0), so nothing else needs to be said.

use crate::model::{Entry, Id, Kind, Tree};
use rt_svc::client::ScanSink;
use rt_svc::proto::{Chunk, ScanProgress, WireEntry};
use std::collections::VecDeque;
use std::io;

/// The most entries of one folder in one chunk.
const CHUNK_ENTRIES: usize = 20_000;
/// A frame is sent when its chunks add up to about this many bytes.
const BATCH_BYTES: usize = 1 << 20;

fn wire_kind(k: Kind) -> u8 {
    match k {
        Kind::File => 0,
        Kind::Dir => 1,
        Kind::Link => 2,
    }
}

fn kind_of(k: u8) -> Kind {
    match k {
        1 => Kind::Dir,
        2 => Kind::Link,
        _ => Kind::File,
    }
}

/// Sends the tree, a batch of chunks at a time, to `emit`. Returns the number of nodes.
pub fn send_tree(tree: &Tree, emit: &mut dyn FnMut(Vec<Chunk>) -> io::Result<()>) -> io::Result<u32> {
    let mut queue: VecDeque<(Id, u32)> = VecDeque::new();
    queue.push_back((tree.root(), 0));
    let mut next = 1u32;
    let mut batch: Vec<Chunk> = Vec::new();
    let mut bytes = 0usize;
    while let Some((dir, number)) = queue.pop_front() {
        let kids = tree.children(dir);
        if kids.is_empty() {
            continue;
        }
        let entries: Vec<WireEntry> = kids
            .iter()
            .map(|&k| {
                let n = tree.node(k);
                let (mut size, mut alloc) = (n.size, n.alloc);
                if n.kind == Kind::Dir {
                    // A folder's total, less what is below it: the client adds it up again.
                    for &g in tree.children(k) {
                        size = size.saturating_sub(tree.node(g).size);
                        alloc = alloc.saturating_sub(tree.node(g).alloc);
                    }
                }
                WireEntry { name: tree.name(k).to_string(), kind: wire_kind(n.kind), flags: n.flags, size, alloc, mtime: n.mtime }
            })
            .collect();
        for (i, &k) in kids.iter().enumerate() {
            if tree.node(k).kind == Kind::Dir {
                queue.push_back((k, next + i as u32));
            }
        }
        next += kids.len() as u32;
        let parts = entries.chunks(CHUNK_ENTRIES).count();
        for (p, part) in entries.chunks(CHUNK_ENTRIES).enumerate() {
            bytes += part.iter().map(|e| e.name.len() + 34).sum::<usize>() + 9;
            batch.push(Chunk { dir: number, last: p + 1 == parts, entries: part.to_vec() });
            if bytes >= BATCH_BYTES {
                emit(std::mem::take(&mut batch))?;
                bytes = 0;
            }
        }
    }
    if !batch.is_empty() {
        emit(batch)?;
    }
    Ok(tree.len() as u32)
}

/// Builds the tree from what the service sends.
pub struct TreeSink<'a> {
    tree: Option<Tree>,
    pending: Vec<Entry>,
    /// The folders already given their entries.
    done: std::collections::HashSet<u32>,
    progress: &'a dyn Fn(&crate::Progress),
    error: Option<String>,
}

impl<'a> TreeSink<'a> {
    pub fn new(progress: &'a dyn Fn(&crate::Progress)) -> TreeSink<'a> {
        TreeSink { tree: None, pending: Vec::new(), done: Default::default(), progress, error: None }
    }

    /// The finished tree, or what was wrong with what the service sent.
    pub fn finish(mut self) -> io::Result<Tree> {
        if let Some(e) = self.error {
            return Err(io::Error::new(io::ErrorKind::InvalidData, e));
        }
        if !self.pending.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "a folder was left open"));
        }
        let mut tree = self.tree.take().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "the service sent no tree"))?;
        tree.finish();
        Ok(tree)
    }
}

impl ScanSink for TreeSink<'_> {
    fn start(&mut self, root: &str, mtime: i64) {
        self.tree = Some(Tree::new(root, mtime));
    }

    fn progress(&mut self, p: &ScanProgress) {
        let current =
            if p.percent <= 100 { rt_core::i18n::tf("Reading the NTFS table… {}%", &[&p.percent.to_string()]) } else { String::new() };
        (self.progress)(&crate::Progress { files: p.files, dirs: p.dirs, bytes: p.bytes, errors: p.errors, current });
    }

    fn chunk(&mut self, dir: u32, last: bool, entries: Vec<WireEntry>) {
        if self.error.is_some() {
            return;
        }
        let Some(tree) = self.tree.as_mut() else {
            self.error = Some("entries came before the root".into());
            return;
        };
        // Only a folder that exists, and has no entries yet, may be given its entries.
        if (dir as usize) >= tree.len() || tree.node(dir).kind != Kind::Dir || (self.pending.is_empty() && self.done.contains(&dir)) {
            self.error = Some(format!("entries for a folder that is not there: {dir}"));
            return;
        }
        self.pending.extend(entries.into_iter().map(|e| Entry {
            name: e.name,
            kind: kind_of(e.kind),
            size: e.size,
            alloc: e.alloc,
            mtime: e.mtime,
            flags: e.flags,
        }));
        if last {
            tree.add_children(dir, &self.pending);
            self.done.insert(dir);
            self.pending.clear();
        }
    }
}

/// Reads the NTFS table of the volume of `root` through the service.
pub fn scan_via_service(
    root: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
    progress: &dyn Fn(&crate::Progress),
) -> io::Result<Tree> {
    let mut client = rt_svc::client::connect("rt-scan", env!("CARGO_PKG_VERSION"))?;
    if client.caps & rt_svc::proto::caps::SCAN == 0 || client.level == rt_svc::Level::Denied {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "the service does not read volumes for this client"));
    }
    client.set_timeout(120_000);
    let mut sink = TreeSink::new(progress);
    client.scan_volume(&root.to_string_lossy(), cancel, &mut sink)?;
    sink.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entry, Tree};

    fn e(name: &str, kind: Kind, size: u64, alloc: u64) -> Entry {
        Entry { name: name.into(), kind, size, alloc, mtime: 5, flags: 0 }
    }

    /// root: a.txt, docs/ (b.md, deep/ (c.bin)), empty/
    fn sample() -> Tree {
        let mut t = Tree::new(r"C:\data", 100);
        t.add_children(0, &[e("a.txt", Kind::File, 10, 4096), e("docs", Kind::Dir, 0, 0), e("empty", Kind::Dir, 0, 0)]);
        t.add_children(2, &[e("b.md", Kind::File, 20, 4096), e("deep", Kind::Dir, 0, 8192)]);
        t.add_children(5, &[e("c.bin", Kind::File, 30, 4096)]);
        t.finish();
        t
    }

    fn rebuild(t: &Tree) -> Tree {
        let mut batches = Vec::new();
        let n = send_tree(t, &mut |b| {
            batches.push(b);
            Ok(())
        })
        .unwrap();
        assert_eq!(n as usize, t.len());
        let progress = |_: &crate::Progress| {};
        let mut sink = TreeSink::new(&progress);
        sink.start(t.name(t.root()), t.node(t.root()).mtime);
        for b in batches {
            for c in b {
                sink.chunk(c.dir, c.last, c.entries);
            }
        }
        sink.finish().unwrap()
    }

    /// Everything about a tree that a reader can see.
    fn dump(t: &Tree, id: Id, out: &mut Vec<String>) {
        let n = t.node(id);
        out.push(format!("{} {:?} {} {} {} {} {}", t.name(id), n.kind, n.size, n.alloc, n.files, n.dirs, n.mtime));
        let mut kids = t.children(id).to_vec();
        kids.sort_by_key(|&k| t.name(k).to_string());
        for k in kids {
            dump(t, k, out);
        }
    }

    #[test]
    fn a_tree_arrives_as_it_left() {
        let t = sample();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        dump(&t, t.root(), &mut a);
        dump(&rebuild(&t), 0, &mut b);
        assert_eq!(a, b);
        assert!(a[0].contains("60 ") || a[0].contains(" 60 "), "{a:?}");
    }

    #[test]
    fn a_wide_folder_is_split_in_chunks() {
        let mut t = Tree::new(r"C:\wide", 1);
        let files: Vec<Entry> = (0..CHUNK_ENTRIES * 2 + 5).map(|i| e(&format!("f{i}"), Kind::File, 1, 512)).collect();
        t.add_children(0, &files);
        t.finish();
        let mut chunks = 0;
        send_tree(&t, &mut |b| {
            chunks += b.len();
            Ok(())
        })
        .unwrap();
        assert_eq!(chunks, 3);
        let r = rebuild(&t);
        assert_eq!((r.len(), r.node(0).size, r.node(0).files), (t.len(), t.node(0).size, t.node(0).files));
    }

    /// With the service installed and running: reads a folder through it (`RT_SCAN_ROOT`, default `C:\Windows`).
    /// `cargo test -p rt-scan --lib service_here -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn service_here() {
        let root = std::env::var("RT_SCAN_ROOT").unwrap_or_else(|_| r"C:\Windows".into());
        let start = std::time::Instant::now();
        let last = std::cell::Cell::new(String::new());
        let progress = |p: &crate::Progress| last.set(format!("{} files, {} folders, {}", p.files, p.dirs, p.current));
        let tree = scan_via_service(std::path::Path::new(&root), &std::sync::atomic::AtomicBool::new(false), &progress)
            .expect("the service answers");
        let n = tree.node(tree.root());
        println!(
            "{root}: {} nodes, {} files, {} bytes, in {:.1?}; last progress: {}",
            tree.len(),
            n.files,
            n.size,
            start.elapsed(),
            last.take()
        );
        assert!(tree.len() > 1000);
    }

    #[test]
    fn a_hostile_server_cannot_break_the_builder() {
        let progress = |_: &crate::Progress| {};
        let entries = || vec![WireEntry { name: "x".into(), kind: 0, flags: 0, size: 1, alloc: 1, mtime: 0 }];
        // Entries before the root.
        let mut s = TreeSink::new(&progress);
        s.chunk(0, true, entries());
        assert!(s.finish().is_err());
        // A folder that does not exist, one that is a file, one given twice, one left open.
        for plan in [(7u32, true), (1, true)] {
            let mut s = TreeSink::new(&progress);
            s.start("C:\\r", 0);
            s.chunk(0, true, entries());
            s.chunk(plan.0, plan.1, entries());
            assert!(s.finish().is_err(), "{plan:?}");
        }
        let mut s = TreeSink::new(&progress);
        s.start("C:\\r", 0);
        s.chunk(0, true, entries());
        s.chunk(0, true, entries());
        assert!(s.finish().is_err());
        let mut s = TreeSink::new(&progress);
        s.start("C:\\r", 0);
        s.chunk(0, false, entries());
        assert!(s.finish().is_err());
    }
}
