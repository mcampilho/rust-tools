//! The folders of an archive, built from the paths of its entries: an
//! archive need not store its folders, only the files in them.

use crate::archive::{Entry, Kind};
use std::collections::HashMap;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    /// The folder above (the root's is itself).
    pub parent: usize,
    pub dirs: Vec<usize>,
    /// The entries in this folder (files, links...), by index.
    pub files: Vec<usize>,
    /// The folder's own entry, when the archive stores one.
    pub entry: Option<usize>,
    /// Size of all the files below, packed size when every one is known,
    /// and how many files.
    pub size: u64,
    pub packed: Option<u64>,
    pub count: u64,
    /// The newest file below.
    pub mtime: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tree {
    /// Node 0 is the archive's root.
    pub nodes: Vec<Node>,
}

impl Tree {
    pub fn build(entries: &[Entry]) -> Tree {
        let mut t = Tree { nodes: vec![Node { packed: Some(0), ..Node::default() }] };
        let mut index: HashMap<(usize, String), usize> = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            let parts: Vec<&str> = e.path.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
            let Some((last, folders)) = parts.split_last() else { continue };
            let mut at = 0;
            for name in folders {
                at = t.child(&mut index, at, name);
            }
            if e.kind == Kind::Dir {
                let n = t.child(&mut index, at, last);
                t.nodes[n].entry = Some(i);
                if e.mtime > t.nodes[n].mtime {
                    t.nodes[n].mtime = e.mtime;
                }
            } else {
                t.nodes[at].files.push(i);
            }
        }
        t.sum(0, entries);
        t
    }

    fn child(&mut self, index: &mut HashMap<(usize, String), usize>, at: usize, name: &str) -> usize {
        if let Some(&n) = index.get(&(at, name.to_string())) {
            return n;
        }
        let n = self.nodes.len();
        self.nodes.push(Node { name: name.to_string(), parent: at, packed: Some(0), ..Node::default() });
        self.nodes[at].dirs.push(n);
        index.insert((at, name.to_string()), n);
        n
    }

    /// Adds up the sizes below each folder (children are always after
    /// their parent, so one pass from the end does it).
    fn sum(&mut self, _root: usize, entries: &[Entry]) {
        for n in (0..self.nodes.len()).rev() {
            let (mut size, mut packed, mut count, mut mtime) = (0u64, Some(0u64), 0u64, self.nodes[n].mtime);
            for &f in &self.nodes[n].files {
                let e = &entries[f];
                size += e.size.unwrap_or(0);
                packed = packed.zip(e.packed).map(|(a, b)| a + b);
                count += u64::from(e.kind == Kind::File);
                mtime = mtime.max(e.mtime);
            }
            for &d in &self.nodes[n].dirs {
                let c = &self.nodes[d];
                size += c.size;
                packed = packed.zip(c.packed).map(|(a, b)| a + b);
                count += c.count;
                mtime = mtime.max(c.mtime);
            }
            let node = &mut self.nodes[n];
            (node.size, node.packed, node.count, node.mtime) = (size, packed, count, mtime);
        }
    }

    /// The path of a folder inside the archive (`docs/img`; empty for the root).
    pub fn path(&self, mut n: usize) -> String {
        let mut parts = Vec::new();
        while n != 0 {
            parts.push(self.nodes[n].name.as_str());
            n = self.nodes[n].parent;
        }
        parts.reverse();
        parts.join("/")
    }

    /// The folders from the root down to `n`.
    pub fn trail(&self, mut n: usize) -> Vec<usize> {
        let mut out = vec![n];
        while n != 0 {
            n = self.nodes[n].parent;
            out.push(n);
        }
        out.reverse();
        out
    }

    /// Marks every entry below folder `n` (and its own entry) in `mask`.
    pub fn mark_below(&self, n: usize, mask: &mut [bool]) {
        let node = &self.nodes[n];
        if let Some(e) = node.entry {
            mask[e] = true;
        }
        for &f in &node.files {
            mask[f] = true;
        }
        for &d in &node.dirs {
            self.mark_below(d, mask);
        }
    }

    /// When everything is in one folder (or is one file), the archive
    /// brings its own folder: it can be extracted where it is, without a
    /// folder named after it around.
    pub fn single_top(&self) -> bool {
        let root = &self.nodes[0];
        root.dirs.len() + root.files.len() == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, kind: Kind, size: u64) -> Entry {
        Entry { path: path.into(), kind, size: Some(size), packed: Some(size / 2), mtime: 10, method: String::new(), encrypted: false, crc: None, mode: None, link: None }
    }

    #[test]
    fn folders_from_paths() {
        let entries = vec![
            entry("a/b/c.txt", Kind::File, 100),
            entry("a/d.txt", Kind::File, 10),
            entry("a/b", Kind::Dir, 0),
            entry("e.txt", Kind::File, 1),
            entry("empty", Kind::Dir, 0),
        ];
        let t = Tree::build(&entries);
        let root = &t.nodes[0];
        assert_eq!(root.dirs.len(), 2);
        assert_eq!(root.files, vec![3]);
        assert_eq!((root.size, root.count, root.packed), (111, 3, Some(55)));
        let a = root.dirs[0];
        assert_eq!(t.nodes[a].name, "a");
        assert_eq!(t.nodes[a].entry, None);
        let b = t.nodes[a].dirs[0];
        assert_eq!((t.path(b), t.nodes[b].entry), ("a/b".to_string(), Some(2)));
        assert_eq!(t.trail(b), vec![0, a, b]);
        let mut mask = vec![false; entries.len()];
        t.mark_below(a, &mut mask);
        assert_eq!(mask, vec![true, true, true, false, false]);
        assert!(!t.single_top());
        assert!(Tree::build(&entries[..3]).single_top());
    }
}
