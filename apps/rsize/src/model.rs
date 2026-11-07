//! The scanned tree: every folder and file in one compact arena.
//!
//! Nodes are added a folder at a time: the entries of a folder are appended
//! together, so they get consecutive ids, and always after the folder itself.
//! [`Tree::finish`] then adds the sizes up from the leaves (a single pass from
//! the last id to the first) and sorts every folder's children by size.
//!
//! A node takes about 56 bytes plus its name, so ten million files fit in
//! under a gigabyte.

use std::collections::HashMap;
use std::path::PathBuf;

pub type Id = u32;

/// No node (the root's parent).
pub const NONE: Id = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
    /// A symbolic link, junction or mount point: counted, not followed.
    Link,
}

/// The folder's contents could not be read.
pub const F_UNREADABLE: u8 = 1;
/// Another name of a file already counted (hard link): its size counts once.
pub const F_HARDLINK: u8 = 2;
/// A folder on another file system, not entered.
pub const F_OTHER_FS: u8 = 4;

/// Which size the views show and sort by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SizeMode {
    /// The length of the files.
    #[default]
    Logical,
    /// The space they take on the disk (clusters, compression, sparse files).
    Allocated,
}

#[derive(Clone, Debug)]
pub struct Node {
    name_at: u32,
    name_len: u16,
    pub kind: Kind,
    pub flags: u8,
    /// Index into [`Tree::exts`] (0: no extension).
    pub ext: u32,
    pub parent: Id,
    kids_at: u32,
    kids_len: u32,
    /// Bytes: a file's length, or the total of a folder.
    pub size: u64,
    /// Bytes on the disk.
    pub alloc: u64,
    /// Files below a folder (1 for a file).
    pub files: u64,
    /// Folders below a folder, itself not included.
    pub dirs: u64,
    /// Last change, in seconds since 1970 (UTC): for a folder, its newest file.
    pub mtime: i64,
}

impl Node {
    pub fn size_in(&self, mode: SizeMode) -> u64 {
        match mode {
            SizeMode::Logical => self.size,
            SizeMode::Allocated => self.alloc,
        }
    }

    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }
}

/// One entry of a folder, as a scanner reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    pub size: u64,
    pub alloc: u64,
    pub mtime: i64,
    pub flags: u8,
}

/// Totals for one file extension.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtStat {
    pub size: u64,
    pub alloc: u64,
    pub files: u64,
}

pub struct Tree {
    nodes: Vec<Node>,
    names: String,
    /// Children of each folder, sorted by size: `kids[kids_at..kids_at + kids_len]`.
    kids: Vec<Id>,
    /// Lower-case extensions, without the dot; [0] is "" (none).
    exts: Vec<String>,
    ext_ids: HashMap<String, u32>,
    ext_stats: Vec<ExtStat>,
    /// Extension ids by total size, largest first (for the colours).
    ext_rank: Vec<u32>,
    mode: SizeMode,
    finished: bool,
}

impl Tree {
    /// A tree with only its root, the scanned folder (`root` is its full path).
    pub fn new(root: &str, mtime: i64) -> Tree {
        let mut t = Tree {
            nodes: Vec::new(),
            names: String::new(),
            kids: Vec::new(),
            exts: vec![String::new()],
            ext_ids: HashMap::new(),
            ext_stats: Vec::new(),
            ext_rank: Vec::new(),
            mode: SizeMode::Logical,
            finished: false,
        };
        let e = Entry { name: root.to_string(), kind: Kind::Dir, size: 0, alloc: 0, mtime, flags: 0 };
        t.push(&e, NONE);
        t
    }

    fn push(&mut self, e: &Entry, parent: Id) -> Id {
        let name_at = self.names.len() as u32;
        // Names are at most 255 UTF-16 units; a long root path is cut at a
        // character boundary rather than overflow the length field.
        let mut name = e.name.as_str();
        while name.len() > u16::MAX as usize {
            let mut cut = u16::MAX as usize;
            while !name.is_char_boundary(cut) {
                cut -= 1;
            }
            name = &name[..cut];
        }
        self.names.push_str(name);
        let ext = if e.kind == Kind::File { self.ext_id(name) } else { 0 };
        let id = self.nodes.len() as Id;
        self.nodes.push(Node {
            name_at,
            name_len: name.len() as u16,
            kind: e.kind,
            flags: e.flags,
            ext,
            parent,
            kids_at: 0,
            kids_len: 0,
            size: e.size,
            alloc: e.alloc,
            files: u64::from(e.kind != Kind::Dir),
            dirs: 0,
            mtime: e.mtime,
        });
        id
    }

    fn ext_id(&mut self, name: &str) -> u32 {
        let Some(dot) = name.rfind('.').filter(|&d| d > 0 && d + 1 < name.len()) else { return 0 };
        let ext = &name[dot + 1..];
        if ext.len() > 16 || ext.contains(' ') {
            return 0;
        }
        let lower = ext.to_lowercase();
        if let Some(&id) = self.ext_ids.get(&lower) {
            return id;
        }
        let id = self.exts.len() as u32;
        self.exts.push(lower.clone());
        self.ext_ids.insert(lower, id);
        id
    }

    /// Adds the entries of folder `dir` (once per folder). Returns the id of
    /// the first; the others follow it.
    pub fn add_children(&mut self, dir: Id, entries: &[Entry]) -> Id {
        let first = self.nodes.len() as Id;
        for e in entries {
            self.push(e, dir);
        }
        let d = &mut self.nodes[dir as usize];
        d.kids_at = first;
        d.kids_len = entries.len() as u32;
        first
    }

    /// Marks a folder (its contents could not be read, ...).
    pub fn set_flag(&mut self, id: Id, flag: u8) {
        self.nodes[id as usize].flags |= flag;
    }

    /// Adds up the sizes and sorts the children. Call once, after the last
    /// folder was added.
    pub fn finish(&mut self) {
        for i in (1..self.nodes.len()).rev() {
            let n = &self.nodes[i];
            let (p, size, alloc, files, dirs, mtime) = (n.parent as usize, n.size, n.alloc, n.files, n.dirs, n.mtime);
            let is_dir = u64::from(n.kind == Kind::Dir);
            let parent = &mut self.nodes[p];
            parent.size += size;
            parent.alloc += alloc;
            parent.files += files;
            parent.dirs += dirs + is_dir;
            parent.mtime = parent.mtime.max(mtime);
        }
        self.ext_stats = vec![ExtStat::default(); self.exts.len()];
        for n in self.nodes.iter().filter(|n| n.kind == Kind::File) {
            let st = &mut self.ext_stats[n.ext as usize];
            st.size += n.size;
            st.alloc += n.alloc;
            st.files += 1;
        }
        self.kids = (0..self.nodes.len() as Id).collect();
        self.finished = true;
        self.resort(self.mode);
    }

    /// Sorts every folder's children, and ranks the extensions, by `mode`.
    pub fn sort(&mut self, mode: SizeMode) {
        if mode != self.mode && self.finished {
            self.resort(mode);
        }
        self.mode = mode;
    }

    fn resort(&mut self, mode: SizeMode) {
        self.mode = mode;
        let nodes = &self.nodes;
        for n in nodes.iter().filter(|n| n.kids_len > 1) {
            let range = n.kids_at as usize..(n.kids_at + n.kids_len) as usize;
            self.kids[range].sort_unstable_by(|&a, &b| {
                let (na, nb) = (&nodes[a as usize], &nodes[b as usize]);
                nb.size_in(mode).cmp(&na.size_in(mode)).then(a.cmp(&b))
            });
        }
        let mut rank: Vec<u32> = (1..self.exts.len() as u32).collect();
        let st = &self.ext_stats;
        let key = |e: u32| match mode {
            SizeMode::Logical => st[e as usize].size,
            SizeMode::Allocated => st[e as usize].alloc,
        };
        rank.sort_unstable_by(|&a, &b| key(b).cmp(&key(a)).then(a.cmp(&b)));
        self.ext_rank = rank;
    }

    pub fn mode(&self) -> SizeMode {
        self.mode
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn root(&self) -> Id {
        0
    }

    pub fn node(&self, id: Id) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn name(&self, id: Id) -> &str {
        let n = &self.nodes[id as usize];
        &self.names[n.name_at as usize..n.name_at as usize + n.name_len as usize]
    }

    /// The size of `id` in the current mode.
    pub fn size(&self, id: Id) -> u64 {
        self.node(id).size_in(self.mode)
    }

    /// The children of a folder, largest first (after [`Tree::finish`]).
    pub fn children(&self, id: Id) -> &[Id] {
        let n = &self.nodes[id as usize];
        if n.kids_len == 0 || self.kids.is_empty() {
            return &[];
        }
        &self.kids[n.kids_at as usize..(n.kids_at + n.kids_len) as usize]
    }

    pub fn parent(&self, id: Id) -> Option<Id> {
        Some(self.nodes[id as usize].parent).filter(|&p| p != NONE)
    }

    /// How many folders up to the root.
    pub fn depth(&self, mut id: Id) -> usize {
        let mut d = 0;
        while let Some(p) = self.parent(id) {
            d += 1;
            id = p;
        }
        d
    }

    /// Whether `id` is `ancestor` or inside it.
    pub fn is_within(&self, mut id: Id, ancestor: Id) -> bool {
        loop {
            if id == ancestor {
                return true;
            }
            match self.parent(id) {
                Some(p) => id = p,
                None => return false,
            }
        }
    }

    /// The full path of a node.
    pub fn path(&self, id: Id) -> PathBuf {
        let mut chain = Vec::new();
        let mut i = id;
        while let Some(p) = self.parent(i) {
            chain.push(i);
            i = p;
        }
        let mut path = PathBuf::from(self.name(self.root()));
        for c in chain.into_iter().rev() {
            path.push(self.name(c));
        }
        path
    }

    /// The extension of a file (lower case, without the dot; "" for none).
    pub fn ext_name(&self, ext: u32) -> &str {
        &self.exts[ext as usize]
    }

    pub fn ext_stat(&self, ext: u32) -> &ExtStat {
        &self.ext_stats[ext as usize]
    }

    /// Extension ids by total size in the current mode, largest first.
    pub fn ext_ranking(&self) -> &[u32] {
        &self.ext_rank
    }

    /// Folders whose contents could not be read.
    pub fn unreadable(&self) -> usize {
        self.nodes.iter().filter(|n| n.flags & F_UNREADABLE != 0).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn file(name: &str, size: u64) -> Entry {
        Entry { name: name.into(), kind: Kind::File, size, alloc: size.div_ceil(4096) * 4096, mtime: size as i64, flags: 0 }
    }

    pub(crate) fn dir(name: &str) -> Entry {
        Entry { name: name.into(), kind: Kind::Dir, size: 0, alloc: 0, mtime: 0, flags: 0 }
    }

    /// root/{a.txt 10, big.bin 5000, sub/{x.TXT 300, y 7}}
    pub(crate) fn sample() -> Tree {
        let mut t = Tree::new(if cfg!(windows) { "C:\\data" } else { "/data" }, 0);
        let first = t.add_children(0, &[file("a.txt", 10), file("big.bin", 5000), dir("sub")]);
        t.add_children(first + 2, &[file("x.TXT", 300), file("y", 7)]);
        t.finish();
        t
    }

    #[test]
    fn totals_add_up() {
        let t = sample();
        let root = t.node(0);
        assert_eq!(root.size, 5317);
        assert_eq!(root.files, 4);
        assert_eq!(root.dirs, 1);
        assert_eq!(root.mtime, 5000);
        assert_eq!(root.alloc, 4096 * 5);
        let sub = t.children(0).iter().copied().find(|&c| t.name(c) == "sub").unwrap();
        assert_eq!(t.node(sub).size, 307);
        assert_eq!(t.node(sub).files, 2);
        assert_eq!(t.depth(sub), 1);
    }

    #[test]
    fn children_are_sorted_by_size() {
        let mut t = sample();
        let names: Vec<&str> = t.children(0).iter().map(|&c| t.name(c)).collect();
        assert_eq!(names, ["big.bin", "sub", "a.txt"]);
        // On the disk, sub (two clusters) is larger than big.bin (two clusters, lower id first).
        t.sort(SizeMode::Allocated);
        let sizes: Vec<u64> = t.children(0).iter().map(|&c| t.size(c)).collect();
        assert_eq!(sizes, [8192, 8192, 4096]);
        assert_eq!(t.mode(), SizeMode::Allocated);
    }

    #[test]
    fn paths_and_extensions() {
        let t = sample();
        let sub = t.children(0)[1];
        let x = t.children(sub)[0];
        assert_eq!(t.name(x), "x.TXT");
        let expected: PathBuf = [t.name(0), "sub", "x.TXT"].iter().collect();
        assert_eq!(t.path(x), expected);
        assert!(t.is_within(x, sub) && t.is_within(x, 0) && !t.is_within(sub, x));
        assert_eq!(t.ext_name(t.node(x).ext), "txt");
        let txt = t.node(x).ext;
        assert_eq!(t.ext_stat(txt).files, 2);
        assert_eq!(t.ext_stat(txt).size, 310);
        assert_eq!(t.ext_ranking()[0], t.node(t.children(0)[0]).ext, "bin is the largest");
        assert_eq!(t.node(t.children(sub)[1]).ext, 0, "no extension");
    }

    #[test]
    fn names_without_a_real_extension() {
        let mut t = Tree::new("/r", 0);
        t.add_children(0, &[file(".bashrc", 1), file("a.", 1), file("x.has space", 1), dir("d.ext")]);
        t.finish();
        assert!(t.children(0).iter().all(|&c| t.node(c).ext == 0));
    }
}
