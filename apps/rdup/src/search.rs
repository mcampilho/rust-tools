//! Finding duplicate files: files with the same contents, wherever they are.
//!
//! The drives and folders are read with `rt-scan` (the Everything index, the
//! NTFS table, or folder by folder), and then the files are compared in steps
//! that each leave fewer to read:
//!
//! 1. **Size.** A file whose size no other file has cannot have a copy. This
//!    leaves out most files without reading any of them.
//! 2. **First and last bytes.** The first and the last 4 KB of the files left
//!    are hashed (small files whole). Files that only share a size part here.
//! 3. **Whole contents.** The files still together are hashed whole with
//!    BLAKE3. Those with the same hash are copies.
//!
//! A file with several names (hard links) is one file, not a copy: when two
//! names lead to the same file (same volume and file number), only one stays.
//!
//! Each volume is read by its own thread, one file after another: on a hard
//! disk, reading two files at once makes the head jump between them.

use crate::cache::Cache;
use crate::filter::Filter;
use crate::folders::{Folder, Pair};
use rt_scan::model::{Id, Kind, Node, Tree, F_HARDLINK};
use rt_scan::{Method, Ways};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Bytes hashed at each end of a file in the second step.
const EDGE: u64 = 4096;
/// Files up to this size are hashed whole in the second step.
pub const SMALL: u64 = 64 * 1024;
/// Bytes read at a time when hashing a whole file.
const BUF: usize = 1 << 20;

/// A hash of the contents (or of the first and last bytes).
pub type Hash = [u8; 32];

/// Which step the search is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Stage {
    /// Reading the drives and folders.
    #[default]
    Reading,
    /// Hashing the first and last bytes of the files that share a size.
    Edges,
    /// Hashing the whole files that are still alike.
    Contents,
    /// Comparing the smaller files of folders that may be copies.
    Folders,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub stage: Stage,
    /// Reading: which drive or folder (from 0), of how many.
    pub root: usize,
    pub roots: usize,
    /// Reading: what the reader of the current root reports.
    pub read: rt_scan::Progress,
    /// Files found so far.
    pub files: u64,
    /// Hashing: files and bytes done, of those of this step.
    pub done: u64,
    pub total: u64,
    pub done_bytes: u64,
    pub total_bytes: u64,
    /// The file or folder being read.
    pub current: String,
}

/// What to look for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Which files are compared (empty files never are).
    pub filter: Filter,
    /// Show the duplicate files, and look for duplicate folders. The copies
    /// of files are found either way: the folders are found from them.
    pub files: bool,
    pub folders: bool,
    /// The file of the hash cache (none: every file is read).
    pub cache: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Options {
        Options { filter: Filter::default(), files: true, folders: true, cache: None }
    }
}

impl Options {
    pub fn with(filter: Filter) -> Options {
        Options { filter, ..Options::default() }
    }
}

/// One copy of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Copy {
    pub path: PathBuf,
    /// Last change, seconds since 1970 (UTC).
    pub mtime: i64,
}

/// Files with the same contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub size: u64,
    pub hash: Hash,
    /// Sorted by path.
    pub files: Vec<Copy>,
}

impl Group {
    /// The space all copies but one take.
    pub fn wasted(&self) -> u64 {
        self.size * (self.files.len() as u64).saturating_sub(1)
    }
}

/// How a drive or folder was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Root {
    pub path: PathBuf,
    pub method: Method,
    /// A local NTFS volume read folder by folder: faster as administrator.
    pub admin_would_help: bool,
    /// Folders that could not be read.
    pub unreadable: usize,
}

/// The result of a search.
pub struct Found {
    /// Largest waste first.
    pub groups: Vec<Group>,
    pub roots: Vec<Root>,
    /// Drives or folders that could not be read at all, with the reason.
    pub failed: Vec<(PathBuf, String)>,
    /// Files looked at, and those of them that share a size with another.
    pub files: u64,
    pub candidates: u64,
    /// Bytes read to hash.
    pub hashed: u64,
    /// Files whose hashes came from the cache, not read again.
    pub cached: u64,
    /// Files that could not be opened or read.
    pub unreadable: u64,
    /// Names of files already counted under another name (hard links).
    pub links: u64,
    /// Smaller files were left out.
    pub min_size: u64,
    /// Other filters left files out.
    pub narrowed: bool,
    /// What was looked for: duplicate files, duplicate folders.
    pub look: [bool; 2],
    /// Folders whose files have copies in another folder, and the pairs of
    /// them, most in common first.
    pub folders: Vec<Folder>,
    pub pairs: Vec<Pair>,
    pub elapsed: Duration,
}

impl Found {
    /// The space all copies but one of each group take.
    pub fn wasted(&self) -> u64 {
        self.groups.iter().map(Group::wasted).sum()
    }

    /// Files that are a copy of another (all but one of each group).
    pub fn copies(&self) -> u64 {
        self.groups.iter().map(|g| g.files.len() as u64 - 1).sum()
    }
}

/// Why a search gave no result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Stopped,
    /// None of the drives and folders could be read (the reasons).
    Unreadable(String),
}

pub enum Event {
    Progress(Progress),
    Done(Result<Box<Found>, Failure>),
}

/// A search running in the background.
pub struct Handle {
    cancel: Arc<AtomicBool>,
}

impl Handle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Starts a search of `roots` in the background. `sink` receives progress
/// about ten times a second, then the result.
pub fn start(roots: Vec<PathBuf>, opts: Options, sink: Box<dyn Fn(Event) + Send>) -> Handle {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let lang = rt_core::i18n::lang();
    std::thread::spawn(move || {
        rt_core::i18n::set_lang(lang);
        let r = find(&roots, opts, &flag, &|p| sink(Event::Progress(p.clone())));
        sink(Event::Done(r.map(Box::new)));
    });
    Handle { cancel }
}

/// The roots to read: normalized, without repeats, and without those inside
/// another (C:\Users is already read with C:\).
pub fn distinct_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let key = |p: &Path| {
        let s = p.to_string_lossy();
        PathBuf::from(if cfg!(windows) { s.to_lowercase() } else { s.into_owned() })
    };
    let mut all: Vec<PathBuf> = roots.iter().map(|r| rt_scan::normalize(r)).collect();
    all.sort_by_key(|p| key(p));
    all.dedup_by_key(|p| key(p));
    let keys: Vec<PathBuf> = all.iter().map(|p| key(p)).collect();
    all.into_iter().enumerate().filter(|(i, _)| !keys.iter().enumerate().any(|(j, k)| j != *i && keys[*i].starts_with(k))).map(|(_, p)| p).collect()
}

/// Searches `roots` the fastest way available.
pub fn find(roots: &[PathBuf], opts: Options, cancel: &AtomicBool, progress: &dyn Fn(&Progress)) -> Result<Found, Failure> {
    find_with(roots, Ways::ALL, opts, cancel, progress)
}

/// A file to compare: where it is in the trees.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cand {
    pub tree: u32,
    pub id: Id,
    pub size: u64,
    /// Last change, to know whether a hash in the cache still holds.
    pub mtime: i64,
}

/// What identifies a file on its volume: two names with the same identity
/// are hard links of one file.
type Ident = (u64, u64);

/// Searches `roots`, reading them in the `ways` given (then folder by folder).
pub fn find_with(roots: &[PathBuf], ways: Ways, opts: Options, cancel: &AtomicBool, progress: &dyn Fn(&Progress)) -> Result<Found, Failure> {
    let start = Instant::now();
    let roots = distinct_roots(roots);
    let n = roots.len();
    let mut trees = Vec::new();
    let mut found = Found {
        groups: Vec::new(),
        roots: Vec::new(),
        failed: Vec::new(),
        files: 0,
        candidates: 0,
        hashed: 0,
        cached: 0,
        unreadable: 0,
        links: 0,
        min_size: opts.filter.min_size.max(1),
        narrowed: opts.filter.narrows(),
        look: [opts.files, opts.folders],
        folders: Vec::new(),
        pairs: Vec::new(),
        elapsed: Duration::ZERO,
    };
    let mut files = Vec::new();
    for (i, root) in roots.iter().enumerate() {
        let before = found.files;
        let report = |r: &rt_scan::Progress| {
            progress(&Progress {
                stage: Stage::Reading,
                root: i,
                roots: n,
                read: r.clone(),
                files: before + r.files,
                current: r.current.clone(),
                ..Progress::default()
            })
        };
        report(&rt_scan::Progress { current: root.to_string_lossy().into_owned(), ..Default::default() });
        match rt_scan::scan_with(root, ways, cancel, &report) {
            Ok(scan) => {
                if cancel.load(Ordering::Relaxed) || !scan.complete {
                    return Err(Failure::Stopped);
                }
                found.roots.push(Root {
                    path: root.clone(),
                    method: scan.method,
                    admin_would_help: scan.admin_would_help,
                    unreadable: scan.tree.unreadable(),
                });
                found.files += collect(&scan.tree, trees.len() as u32, &opts.filter, &mut files);
                trees.push(scan.tree);
            }
            Err(e) => found.failed.push((root.clone(), e)),
        }
    }
    if trees.is_empty() {
        let why = found.failed.iter().map(|(p, e)| format!("{}: {e}", p.display())).collect::<Vec<_>>().join("\n");
        return Err(Failure::Unreadable(why));
    }

    let volumes: Vec<String> = trees.iter().map(|t| volume_key(Path::new(t.name(t.root())))).collect();
    let path_of = |c: &Cand| trees[c.tree as usize].path(c.id);
    let mut cache = opts.cache.as_deref().map(Cache::load);

    // 1 to 3: size, first and last bytes, contents.
    let (same_size, compared) = compare(files, [Stage::Edges, Stage::Contents], &volumes, &path_of, &mut cache, cancel, progress, &mut found)?;
    found.candidates = same_size.len() as u64;

    // 4. Folders: whose files have copies elsewhere.
    let copies: Vec<Vec<(u32, Id)>> = compared.groups.iter().map(|(_, _, m)| m.iter().map(|&i| (same_size[i].tree, same_size[i].id)).collect()).collect();
    let key = |i: usize| (same_size[i].tree, same_size[i].id);
    let mut content: HashMap<(u32, Id), u32> = HashMap::new();
    for (g, m) in copies.iter().enumerate() {
        for k in m {
            content.insert(*k, g as u32);
        }
    }
    // Another name of a copy holds the same bytes.
    for &(i, first) in &compared.links {
        if let Some(&g) = content.get(&key(first)) {
            content.insert(key(i), g);
        }
    }
    let mut small = |cands: Vec<Cand>, found: &mut Found| -> Result<Vec<Vec<(u32, Id)>>, Failure> {
        let (same, c) = compare(cands, [Stage::Folders, Stage::Folders], &volumes, &path_of, &mut cache, cancel, progress, found)?;
        let mut groups: Vec<Vec<(u32, Id)>> = c.groups.iter().map(|(_, _, m)| m.iter().map(|&i| (same[i].tree, same[i].id)).collect()).collect();
        let mut of: HashMap<usize, usize> = HashMap::new();
        for (g, (_, _, m)) in c.groups.iter().enumerate() {
            of.extend(m.iter().map(|&i| (i, g)));
        }
        for (i, first) in c.links {
            if let Some(&g) = of.get(&first) {
                groups[g].push((same[i].tree, same[i].id));
            }
        }
        Ok(groups)
    };
    let (folders, pairs) = if opts.folders {
        crate::folders::find(&trees, &opts.filter, &copies, &mut content, &mut |c| small(c, &mut found), cancel)?
    } else {
        (Vec::new(), Vec::new())
    };
    found.folders = folders;
    found.pairs = pairs;
    drop(copies);
    if let (Some(c), Some(file)) = (&mut cache, &opts.cache) {
        // Without it the next search reads again: nothing is lost.
        let _ = c.save(file);
    }

    found.groups = compared
        .groups
        .into_iter()
        .map(|(size, hash, members)| {
            let mut files: Vec<Copy> = members
                .iter()
                .map(|&i| {
                    let c = &same_size[i];
                    Copy { path: path_of(c), mtime: trees[c.tree as usize].node(c.id).mtime }
                })
                .collect();
            files.sort_by(|a, b| a.path.cmp(&b.path));
            Group { size, hash, files }
        })
        .collect();
    found.groups.sort_by(|a, b| b.wasted().cmp(&a.wasted()).then(b.size.cmp(&a.size)).then(a.files[0].path.cmp(&b.files[0].path)));
    found.elapsed = start.elapsed();
    Ok(found)
}

/// Files with the same contents found by [`compare`]: (size, hash, indices
/// into the files that share a size), and the other names of a file met
/// (hard links), each with the index of its first name.
struct Compared {
    groups: Vec<(u64, Hash, Vec<usize>)>,
    links: Vec<(usize, usize)>,
}

/// Compares `files` by size, then first and last bytes, then whole contents,
/// in the two `stages` given (shown as progress). Returns the files that share
/// a size (which the indices of the result point into) and the groups.
#[allow(clippy::too_many_arguments)]
fn compare(
    mut files: Vec<Cand>,
    stages: [Stage; 2],
    volumes: &[String],
    path_of: &(dyn Fn(&Cand) -> PathBuf + Sync),
    cache: &mut Option<Cache>,
    cancel: &AtomicBool,
    progress: &dyn Fn(&Progress),
    found: &mut Found,
) -> Result<(Vec<Cand>, Compared), Failure> {
    // Size.
    files.sort_unstable_by_key(|c| c.size);
    let mut same_size = Vec::new();
    for run in files.chunk_by(|a, b| a.size == b.size) {
        if run.len() > 1 {
            same_size.extend_from_slice(run);
        }
    }
    drop(files);

    // First and last bytes (small files whole), from the cache when the file
    // did not change.
    let edge_bytes = |size: u64| if size <= SMALL { size } else { 2 * EDGE };
    let mut edges: Vec<Hashed> = vec![None; same_size.len()];
    let mut jobs: Vec<usize> = Vec::new();
    for (i, c) in same_size.iter().enumerate() {
        match cache.as_mut().and_then(|k| k.get(&path_of(c), c.size, c.mtime)).and_then(|e| e.edges.map(|h| (h, e.ident))) {
            Some(hit) => {
                edges[i] = Some(hit);
                found.cached += 1;
            }
            None => jobs.push(i),
        }
    }
    let read = hash_step(&same_size, &jobs, stages[0], false, volumes, path_of, &edge_bytes, cancel, progress, found)?;
    found.hashed += jobs.iter().map(|&i| edge_bytes(same_size[i].size)).sum::<u64>();
    for &i in &jobs {
        edges[i] = read[i];
        if let (Some((h, ident)), Some(k)) = (read[i], cache.as_mut()) {
            let c = &same_size[i];
            k.put(&path_of(c), c.size, c.mtime, ident, Some(h), None);
        }
    }
    let mut alike: HashMap<(u64, Hash), Vec<usize>> = HashMap::new();
    let mut seen: HashMap<Ident, usize> = HashMap::new();
    let mut links = Vec::new();
    for (i, r) in edges.iter().enumerate() {
        match r {
            Some((h, ident)) => {
                // Another name of a file already met: not a copy.
                if let Some(id) = ident {
                    if let Some(first) = seen.insert(*id, i) {
                        seen.insert(*id, first);
                        links.push((i, first));
                        found.links += 1;
                        continue;
                    }
                }
                alike.entry((same_size[i].size, *h)).or_default().push(i);
            }
            None => found.unreadable += 1,
        }
    }
    let mut groups: Vec<(u64, Hash, Vec<usize>)> = Vec::new();
    let mut big: Vec<usize> = Vec::new();
    for ((size, h), members) in alike {
        if members.len() < 2 {
            continue;
        }
        if size <= SMALL {
            groups.push((size, h, members));
        } else {
            big.extend(members);
        }
    }

    // Whole contents of the larger files.
    big.sort_unstable();
    let whole = |size: u64| size;
    let mut full: Vec<Hashed> = vec![None; same_size.len()];
    let mut jobs: Vec<usize> = Vec::new();
    for &i in &big {
        let c = &same_size[i];
        match cache.as_mut().and_then(|k| k.get(&path_of(c), c.size, c.mtime)).and_then(|e| e.whole) {
            Some(h) => {
                full[i] = Some((h, None));
                found.cached += 1;
            }
            None => jobs.push(i),
        }
    }
    let read = hash_step(&same_size, &jobs, stages[1], true, volumes, path_of, &whole, cancel, progress, found)?;
    found.hashed += jobs.iter().map(|&i| same_size[i].size).sum::<u64>();
    for &i in &jobs {
        full[i] = read[i];
        if let (Some((h, _)), Some(k)) = (read[i], cache.as_mut()) {
            let c = &same_size[i];
            k.put(&path_of(c), c.size, c.mtime, None, None, Some(h));
        }
    }
    let mut alike: HashMap<(u64, Hash), Vec<usize>> = HashMap::new();
    for &i in &big {
        match &full[i] {
            Some((h, _)) => alike.entry((same_size[i].size, *h)).or_default().push(i),
            None => found.unreadable += 1,
        }
    }
    groups.extend(alike.into_iter().filter(|(_, m)| m.len() > 1).map(|((size, h), m)| (size, h, m)));
    // In a fixed order, whatever the order of the hash map.
    groups.sort_unstable_by(|a, b| a.2.cmp(&b.2));
    Ok((same_size, Compared { groups, links }))
}

/// Names that are not the user's files at the root of a drive: the Recycle
/// Bin, restore points, and the NTFS system files (`$MFT`...) that a direct
/// reading lists.
fn skipped_at_root(name: &str) -> bool {
    name.starts_with('$') || name.eq_ignore_ascii_case("System Volume Information")
}

/// Adds the files of `t` that the filter keeps to `out`. Returns how many
/// files it looked at (not those in folders left out by name).
fn collect(t: &Tree, tree: u32, f: &Filter, out: &mut Vec<Cand>) -> u64 {
    walk(t, t.root(), f, &mut |id, n| {
        if n.size >= f.min_size {
            out.push(Cand { tree, id, size: n.size, mtime: n.mtime });
        }
    })
}

/// Calls `visit` for each file in the folder `start` of `t` (and below it)
/// that the filter keeps whatever its size: the files that count in a folder.
/// Folders left out by name are not entered, nor, at the root of a drive, the
/// Recycle Bin and the system's folders. Returns how many files it looked at.
pub(crate) fn walk(t: &Tree, start: Id, f: &Filter, visit: &mut dyn FnMut(Id, &Node)) -> u64 {
    let drive = Path::new(t.name(t.root())).parent().is_none();
    let names = f.has_names();
    // Whether `start` or a folder it is in has one of the names to keep.
    let mut named = f.only_names.is_empty();
    let mut up = Some(start);
    while let Some(d) = up.filter(|&d| d != t.root() && !named) {
        named = f.only_names.matches(&t.name(d).to_lowercase());
        up = t.parent(d);
    }
    let mut count = 0;
    let mut stack = vec![(start, named)];
    while let Some((dir, named)) = stack.pop() {
        for &c in t.children(dir) {
            let n = t.node(c);
            let name = t.name(c);
            if drive && dir == t.root() && skipped_at_root(name) {
                continue;
            }
            match n.kind {
                Kind::Dir => {
                    if !names {
                        stack.push((c, true));
                        continue;
                    }
                    let lower = name.to_lowercase();
                    if !f.skip_names.matches(&lower) {
                        stack.push((c, named || f.only_names.matches(&lower)));
                    }
                }
                Kind::File => {
                    count += 1;
                    if n.flags & F_HARDLINK != 0 || !f.keeps_any_size(n.size, n.mtime) || !f.keeps_ext(name) {
                        continue;
                    }
                    if names {
                        let lower = name.to_lowercase();
                        if f.skip_names.matches(&lower) || !(named || f.only_names.matches(&lower)) {
                            continue;
                        }
                    }
                    visit(c, n);
                }
                Kind::Link => {}
            }
        }
    }
    count
}

/// Which volume a root is on, to read each volume with its own thread.
fn volume_key(root: &Path) -> String {
    #[cfg(windows)]
    {
        match root.components().next() {
            Some(std::path::Component::Prefix(p)) => p.as_os_str().to_string_lossy().to_lowercase(),
            _ => String::new(),
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(root).map_or(String::new(), |m| m.dev().to_string())
    }
}

/// Threads reading each volume at once. The first and last bytes are small
/// reads, mostly waiting on opening the file and on the disk's head: with a
/// few at once, a hard disk serves them in a better order and an antivirus
/// checks them side by side. Whole files are long reads, one at a time so the
/// head does not jump between them. `RDUP_WORKERS` overrides it (to measure).
fn workers_per_volume(whole: bool) -> usize {
    let env = std::env::var("RDUP_WORKERS").ok().and_then(|v| v.parse::<usize>().ok()).filter(|&n| (1..=64).contains(&n));
    if whole {
        1
    } else {
        env.unwrap_or(4)
    }
}

/// The result of hashing one file: its hash and identity, or None when it
/// could not be read.
type Hashed = Option<(Hash, Option<Ident>)>;

/// Hashes the candidates `jobs` (indices into `cands`), one thread per volume,
/// whole or their first and last bytes, with `bytes(size)` bytes read from
/// each. Returns a result per candidate (None for those not in `jobs`).
#[allow(clippy::too_many_arguments)]
fn hash_step(
    cands: &[Cand],
    jobs: &[usize],
    stage: Stage,
    whole: bool,
    volumes: &[String],
    path_of: &(dyn Fn(&Cand) -> PathBuf + Sync),
    bytes: &(dyn Fn(u64) -> u64 + Sync),
    cancel: &AtomicBool,
    progress: &dyn Fn(&Progress),
    found: &Found,
) -> Result<Vec<Hashed>, Failure> {
    let mut by_volume: HashMap<&str, Vec<usize>> = HashMap::new();
    for &i in jobs {
        by_volume.entry(volumes[cands[i].tree as usize].as_str()).or_default().push(i);
    }
    let total = jobs.len() as u64;
    let total_bytes: u64 = jobs.iter().map(|&i| bytes(cands[i].size)).sum();
    let done = AtomicU64::new(0);
    let done_bytes = AtomicU64::new(0);
    let current = Mutex::new(String::new());
    let report = || {
        progress(&Progress {
            stage,
            roots: found.roots.len(),
            files: found.files,
            done: done.load(Ordering::Relaxed),
            total,
            done_bytes: done_bytes.load(Ordering::Relaxed),
            total_bytes,
            current: current.lock().map(|c| c.clone()).unwrap_or_default(),
            ..Progress::default()
        })
    };
    report();
    let per_volume = workers_per_volume(whole);
    let lists: Vec<(Vec<usize>, AtomicUsize)> = by_volume
        .into_values()
        .map(|mut list| {
            // In the order of the trees: files of a folder together.
            list.sort_unstable_by_key(|&i| (cands[i].tree, cands[i].id));
            (list, AtomicUsize::new(0))
        })
        .collect();
    let results: Vec<(usize, Hashed)> = std::thread::scope(|s| {
        let workers: Vec<_> = lists
            .iter()
            .flat_map(|vol| std::iter::repeat_n(vol, per_volume))
            .map(|(list, next)| {
                let (done, done_bytes, current) = (&done, &done_bytes, &current);
                s.spawn(move || {
                    let mut buf = vec![0u8; BUF];
                    let mut out = Vec::new();
                    // The workers of a volume take its files in turn.
                    while let Some(&i) = list.get(next.fetch_add(1, Ordering::Relaxed)) {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        let c = &cands[i];
                        let path = path_of(c);
                        if let Ok(mut cur) = current.lock() {
                            *cur = path.to_string_lossy().into_owned();
                        }
                        let r = if whole {
                            contents(&path, &mut buf, cancel, done_bytes).map(|h| (h, None))
                        } else {
                            let r = edges(&path, c.size, &mut buf);
                            done_bytes.fetch_add(bytes(c.size), Ordering::Relaxed);
                            r
                        };
                        out.push((i, r.ok()));
                        done.fetch_add(1, Ordering::Relaxed);
                    }
                    out
                })
            })
            .collect();
        let mut last = Instant::now();
        while !workers.iter().all(|w| w.is_finished()) {
            std::thread::sleep(Duration::from_millis(20));
            if last.elapsed() >= Duration::from_millis(100) {
                last = Instant::now();
                report();
            }
        }
        workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect()
    });
    if cancel.load(Ordering::Relaxed) {
        return Err(Failure::Stopped);
    }
    report();
    let mut all = vec![None; cands.len()];
    for (i, r) in results {
        all[i] = r;
    }
    Ok(all)
}

/// The hash of the first and last 4 KB of a file (all of it when small),
/// and the file's identity.
fn edges(path: &Path, size: u64, buf: &mut [u8]) -> io::Result<(Hash, Option<Ident>)> {
    let mut f = File::open(path)?;
    let ident = identity(&f);
    let mut h = blake3::Hasher::new();
    if size <= SMALL {
        let b = &mut buf[..size as usize];
        f.read_exact(b)?;
        h.update(b);
    } else {
        let b = &mut buf[..EDGE as usize];
        f.read_exact(b)?;
        h.update(b);
        f.seek(SeekFrom::Start(size - EDGE))?;
        f.read_exact(b)?;
        h.update(b);
    }
    Ok((*h.finalize().as_bytes(), ident))
}

/// The hash of a whole file, counting the bytes read in `done`.
fn contents(path: &Path, buf: &mut [u8], cancel: &AtomicBool, done: &AtomicU64) -> io::Result<Hash> {
    let mut f = File::open(path)?;
    let mut h = blake3::Hasher::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = match f.read(buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        h.update(&buf[..n]);
        done.fetch_add(n as u64, Ordering::Relaxed);
    }
    Ok(*h.finalize().as_bytes())
}

/// The volume and file number of an open file.
#[cfg(windows)]
fn identity(f: &File) -> Option<Ident> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
    // SAFETY: a plain struct of integers, filled in by the call.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: the handle is open for as long as `f` lives; the output is as stated.
    if unsafe { GetFileInformationByHandle(f.as_raw_handle() as _, &mut info) } == 0 {
        return None;
    }
    let index = (info.nFileIndexHigh as u64) << 32 | info.nFileIndexLow as u64;
    // Some network file systems give no file numbers.
    (index != 0).then_some((info.dwVolumeSerialNumber as u64, index))
}

#[cfg(unix)]
fn identity(f: &File) -> Option<Ident> {
    use std::os::unix::fs::MetadataExt;
    f.metadata().ok().map(|m| (m.dev(), m.ino()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rt_scan::testing::TempTree;

    /// Searches folder by folder: what the tests check, whatever else this PC has.
    pub(crate) fn sys(roots: &[PathBuf]) -> Result<Found, Failure> {
        find_with(roots, Ways::SYSTEM, Options::default(), &AtomicBool::new(false), &|_| {})
    }

    /// `len` bytes that differ from file to file by `seed`.
    pub(crate) fn data(len: usize, seed: u8) -> Vec<u8> {
        (0..len).map(|i| (i as u32 * 31 + seed as u32 * 7) as u8).collect()
    }

    /// A folder with copies of every kind: small and large, in subfolders,
    /// alike only in size or only at the ends.
    pub(crate) fn sample(tag: &str) -> TempTree {
        let tt = TempTree::new(tag);
        let big = data(300_000, 1);
        tt.write("photos/big.bin", &big);
        tt.write("backup/big copy.bin", &big);
        tt.write("backup/old/big.bin", &big);
        // Same size, same first and last bytes, different middle.
        let mut other = big.clone();
        other[150_000] ^= 0xFF;
        tt.write("other/big.bin", &other);
        let small = data(1000, 2);
        tt.write("notes.txt", &small);
        tt.write("docs/notes.txt", &small);
        // Same size, other contents.
        tt.write("docs/plan.txt", &data(1000, 3));
        tt.write("unique.dat", &data(777, 4));
        tt.write("empty1", b"");
        tt.write("docs/empty2", b"");
        tt
    }

    fn names(g: &Group, root: &Path) -> Vec<String> {
        g.files.iter().map(|c| c.path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/")).collect()
    }

    #[test]
    fn finds_copies_by_contents() {
        let tt = sample("rdup-find");
        let root = rt_scan::normalize(&tt.0);
        let f = sys(std::slice::from_ref(&tt.0)).unwrap();
        assert_eq!(f.files, 10);
        assert_eq!(f.candidates, 7, "four of 300 000 bytes, three of 1000");
        assert_eq!(f.groups.len(), 2);
        assert_eq!(names(&f.groups[0], &root), ["backup/big copy.bin", "backup/old/big.bin", "photos/big.bin"]);
        assert_eq!(f.groups[0].wasted(), 600_000);
        assert_eq!(names(&f.groups[1], &root), ["docs/notes.txt", "notes.txt"]);
        assert_eq!((f.wasted(), f.copies()), (601_000, 3));
        assert_eq!((f.unreadable, f.links), (0, 0));
        assert!(f.groups[0].files.iter().all(|c| c.mtime > 1_700_000_000));
    }

    #[test]
    fn the_cache_spares_reading_files_again() {
        let tt = sample("rdup-cached");
        let file = tt.0.with_file_name("rdup-cached-hashes.bin");
        let _ = std::fs::remove_file(&file);
        let opts = || Options { cache: Some(file.clone()), ..Options::default() };
        let run = || find_with(std::slice::from_ref(&tt.0), Ways::SYSTEM, opts(), &AtomicBool::new(false), &|_| {}).unwrap();
        let first = run();
        assert_eq!(first.cached, 0);
        assert!(first.hashed > 0);
        let second = run();
        assert_eq!((second.hashed, second.cached), (0, 11), "7 ends and 4 whole files, none read");
        assert_eq!(second.groups, first.groups);
        // A file changed since (same size, another date): read again.
        let changed = tt.0.join("backup/big copy.bin");
        let mut other = data(300_000, 1);
        other[150_000] ^= 1;
        tt.write("backup/big copy.bin", &other);
        let f = std::fs::File::options().write(true).open(&changed).unwrap();
        f.set_modified(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000)).unwrap();
        drop(f);
        let third = run();
        assert_eq!(third.groups[0].files.len(), 2, "big copy.bin is no longer a copy");
        assert_eq!(third.hashed, 2 * EDGE + 300_000, "its ends, then all of it; the others from the cache");
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn hard_links_are_one_file() {
        let tt = TempTree::new("rdup-links");
        tt.write("a.bin", &data(5000, 9));
        if std::fs::hard_link(tt.0.join("a.bin"), tt.0.join("b.bin")).is_err() {
            return; // a file system without hard links
        }
        let f = sys(std::slice::from_ref(&tt.0)).unwrap();
        assert!(f.groups.is_empty(), "two names of one file are not copies");
        tt.write("c.bin", &data(5000, 9));
        let f = sys(std::slice::from_ref(&tt.0)).unwrap();
        assert_eq!(f.groups.len(), 1);
        assert_eq!(f.groups[0].files.len(), 2, "the file and its real copy");
    }

    #[test]
    fn roots_inside_others_are_read_once() {
        let tt = sample("rdup-roots");
        let inner = tt.0.join("backup");
        let r = distinct_roots(&[inner.clone(), tt.0.clone(), tt.0.join("photos"), tt.0.clone()]);
        assert_eq!(r, vec![rt_scan::normalize(&tt.0)]);
        let f = sys(&[inner, tt.0.join("photos")]).unwrap();
        assert_eq!(f.roots.len(), 2);
        assert_eq!(f.groups.len(), 1, "copies across two folders");
        assert_eq!(f.groups[0].files.len(), 3);
    }

    #[test]
    fn stopping_and_unreadable_roots() {
        let tt = sample("rdup-stop");
        let r = find_with(std::slice::from_ref(&tt.0), Ways::SYSTEM, Options::default(), &AtomicBool::new(true), &|_| {});
        assert_eq!(r.err(), Some(Failure::Stopped));
        // Beside the sample, not inside it (a folder inside another is not read twice).
        let nope = tt.0.with_file_name("rdup-stop-nope");
        let r = sys(std::slice::from_ref(&nope));
        assert!(matches!(r, Err(Failure::Unreadable(_))));
        let f = sys(&[nope, tt.0.clone()]).unwrap();
        assert_eq!(f.failed.len(), 1);
        assert_eq!(f.groups.len(), 2);
    }

    #[test]
    fn the_minimum_size_leaves_out_small_files() {
        let tt = sample("rdup-min");
        let f = with(&tt, Filter::min(2000));
        assert_eq!(f.groups.len(), 1);
        assert_eq!(f.groups[0].size, 300_000);
    }

    fn with(tt: &TempTree, filter: Filter) -> Found {
        find_with(std::slice::from_ref(&tt.0), Ways::SYSTEM, Options::with(filter), &AtomicBool::new(false), &|_| {}).unwrap()
    }

    fn choice(set: &[(crate::filter::Field, &str)], kinds: u8) -> Filter {
        let mut c = crate::filter::Choice { texts: Default::default() };
        for i in 0..crate::filter::KINDS.len() {
            if kinds & (1 << i) != 0 {
                c.toggle_kind(i);
            }
        }
        for (f, t) in set {
            c.texts[f.index()] = t.to_string();
        }
        c.filter().unwrap()
    }

    #[test]
    fn filters_leave_out_extensions_names_and_dates() {
        use crate::filter::Field;
        let tt = sample("rdup-filters");
        let root = rt_scan::normalize(&tt.0);
        // Extensions to keep, and the kinds of file.
        let f = with(&tt, choice(&[(Field::OnlyExt, ".txt")], 0));
        assert_eq!(f.groups.len(), 1);
        assert_eq!(names(&f.groups[0], &root), ["docs/notes.txt", "notes.txt"]);
        assert!(f.narrowed);
        let office = crate::filter::KINDS.iter().position(|k| k.id == "office").unwrap();
        assert_eq!(with(&tt, choice(&[], 1 << office)).groups.len(), 1);
        // Extensions to leave out.
        let f = with(&tt, choice(&[(Field::SkipExt, "txt")], 0));
        assert_eq!(f.groups.len(), 1);
        assert_eq!(f.groups[0].size, 300_000);
        // A folder left out by name is not looked into.
        let f = with(&tt, choice(&[(Field::SkipNames, "back*")], 0));
        assert_eq!(f.groups.len(), 1, "one big.bin is left: no copy");
        assert_eq!(names(&f.groups[0], &root), ["docs/notes.txt", "notes.txt"]);
        assert_eq!(f.files, 8, "the two files in backup are not counted");
        // Names to keep: the file's own name, or a folder it is in.
        let f = with(&tt, choice(&[(Field::OnlyNames, "*old* photos")], 0));
        assert_eq!(names(&f.groups[0], &root), ["backup/old/big.bin", "photos/big.bin"]);
        let f = with(&tt, choice(&[(Field::OnlyNames, "big*")], 0));
        assert_eq!(f.groups.len(), 1);
        assert_eq!(f.groups[0].files.len(), 3, "big copy.bin too; other/big.bin differs");
        // Dates: the sample was written today.
        assert_eq!(with(&tt, choice(&[(Field::To, "2000")], 0)).groups.len(), 0);
        assert_eq!(with(&tt, choice(&[(Field::From, "2000"), (Field::MaxSize, "1 KB")], 0)).groups.len(), 1);
    }

    /// Times a search of `RDUP_BENCH_DIR` (several folders separated by `|`):
    /// `cargo test -p rdup --release --lib bench -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn bench() {
        let Some(dirs) = std::env::var_os("RDUP_BENCH_DIR") else { return };
        let roots: Vec<PathBuf> = dirs.to_string_lossy().split('|').map(PathBuf::from).collect();
        let last = Mutex::new((Stage::Reading, Instant::now()));
        let shown = Mutex::new(Instant::now());
        // RDUP_MIN: the smallest file size, in bytes.
        let min_size = std::env::var("RDUP_MIN").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
        // RDUP_CACHE: a hash cache file, to time a search again.
        let opts = Options { cache: std::env::var_os("RDUP_CACHE").map(PathBuf::from), ..Options::with(Filter::min(min_size)) };
        let f = find(&roots, opts, &AtomicBool::new(false), &|p| {
            let mut l = last.lock().unwrap();
            if l.0 != p.stage {
                println!("{:?} took {:.2?}", l.0, l.1.elapsed());
                *l = (p.stage, Instant::now());
            }
            let mut s = shown.lock().unwrap();
            if s.elapsed() >= Duration::from_secs(5) {
                *s = Instant::now();
                println!("  {:?}: {} files read; {}/{} files, {}/{} MB; {}", p.stage, p.files, p.done, p.total, p.done_bytes >> 20, p.total_bytes >> 20, p.current);
            }
        })
        .unwrap();
        let (stage, t) = *last.lock().unwrap();
        println!("{stage:?} took {:.2?}", t.elapsed());
        for r in &f.roots {
            println!("{}: {:?}", r.path.display(), r.method);
        }
        println!(
            "{} files, {} sharing a size, {} MB hashed, {} from the cache; {} groups, {} copies, {} MB could be freed; {} unreadable, {} hard links; in {:.2?}",
            f.files,
            f.candidates,
            f.hashed >> 20,
            f.cached,
            f.groups.len(),
            f.copies(),
            f.wasted() >> 20,
            f.unreadable,
            f.links,
            f.elapsed
        );
        for g in f.groups.iter().take(5) {
            println!("  {} x {} bytes: {}", g.files.len(), g.size, g.files[0].path.display());
        }
        println!("{} folders in {} pairs", f.folders.len(), f.pairs.len());
        for share in crate::folders::SHARES {
            let groups = crate::folders::groups(&f.folders, &f.pairs, share);
            println!("  >= {share}: {} groups", groups.len());
            for g in groups.iter().take(3) {
                println!("    {} x {} bytes ({}): {}", g.folders.len(), g.same_bytes, g.permille, f.folders[g.folders[0]].path.display());
            }
        }
    }

    /// How many files of `RDUP_BENCH_DIR` share a size, by size range (no
    /// hashing): `cargo test -p rdup --release --lib size_profile -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn size_profile() {
        let Some(dirs) = std::env::var_os("RDUP_BENCH_DIR") else { return };
        let roots: Vec<PathBuf> = distinct_roots(&dirs.to_string_lossy().split('|').map(PathBuf::from).collect::<Vec<_>>());
        let mut files = Vec::new();
        let mut total = 0;
        for (i, r) in roots.iter().enumerate() {
            let s = rt_scan::scan(r, &AtomicBool::new(false), &|_| {}).unwrap();
            println!("{}: {:?} in {:.2?}", r.display(), s.method, s.elapsed);
            total += collect(&s.tree, i as u32, &Filter::default(), &mut files);
        }
        files.sort_unstable_by_key(|c| c.size);
        let mut shared = Vec::new();
        for run in files.chunk_by(|a, b| a.size == b.size) {
            if run.len() > 1 {
                shared.extend_from_slice(run);
            }
        }
        println!("{total} files, {} not empty, {} share a size", files.len(), shared.len());
        for min in [1u64, 1 << 10, 4 << 10, 16 << 10, 64 << 10, 256 << 10, 1 << 20, 10 << 20] {
            let all = files.iter().filter(|c| c.size >= min).count();
            let same: Vec<&Cand> = shared.iter().filter(|c| c.size >= min).collect();
            let bytes: u64 = same.iter().map(|c| c.size).sum();
            println!("  >= {:>8} B: {:>9} files, {:>9} share a size ({:>3}%), {:>8} MB", min, all, same.len(), same.len() * 100 / all.max(1), bytes >> 20);
        }
    }

    #[test]
    fn progress_goes_through_the_steps() {
        let tt = sample("rdup-progress");
        let stages = Mutex::new(Vec::new());
        find_with(std::slice::from_ref(&tt.0), Ways::SYSTEM, Options::default(), &AtomicBool::new(false), &|p| {
            let mut s = stages.lock().unwrap();
            if s.last() != Some(&p.stage) {
                s.push(p.stage);
            }
        })
        .unwrap();
        assert_eq!(*stages.lock().unwrap(), [Stage::Reading, Stage::Edges, Stage::Contents]);
    }
}
