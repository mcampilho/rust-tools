//! Duplicate folders: folders whose files have copies in another folder.
//!
//! Once the copies of files are known, two folders are compared by the
//! contents of the files below them, whatever their names: they are identical
//! when every file of one has its copy in the other and the other way round,
//! and similar when the files they have in common make up a share of the
//! larger one's bytes.
//!
//! The files that count in a folder are those the filters keep whatever their
//! size: the smallest size only spares reading files, so the smaller files of
//! the folders that may be copies are compared too (by [`find`]'s `small`).
//!
//! 1. **Pairs to look at.** For two copies of a file, their folders, and
//!    those above them level by level, make pairs (`C:\Photos\2024` with
//!    `D:\Backup\2024`, then `C:\Photos` with `D:\Backup`), up to where one
//!    holds the other. Each pair adds up the bytes of the copies they share.
//! 2. **What cannot reach the share** is left out: what they share, plus all
//!    their smaller files, is less than [`MIN_PERMILLE`] of the larger.
//! 3. **The smaller files** of the folders left are compared.
//! 4. **Each pair** is then measured exactly, file by file.

use crate::filter::Filter;
use crate::search::{walk, Cand, Failure};
use rt_scan::model::{Id, Tree, NONE};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// The least share in common, in thousandths, for two folders to be kept.
pub const MIN_PERMILLE: u16 = 500;
/// The shares to choose from to show, in thousandths (1000: identical).
pub const SHARES: [u16; 4] = [1000, 900, 750, 500];
/// Files with more copies than this do not make pairs of folders: a file
/// found in hundreds of folders says little about them.
const MAX_COPIES: usize = 32;
/// The most pairs looked at.
const MAX_PAIRS: usize = 2_000_000;

/// A folder of a pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub path: PathBuf,
    /// The bytes and the number of the files that count in it.
    pub bytes: u64,
    pub files: u64,
    /// Its newest file, seconds since 1970 (UTC).
    pub mtime: i64,
}

/// Two folders with files in common.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    /// Indices into the folders.
    pub a: usize,
    pub b: usize,
    /// The files that have their copy in the other folder, and their bytes.
    pub same_files: u64,
    pub same_bytes: u64,
    /// The bytes in common, in thousandths of the larger folder: 1000 when
    /// they are identical.
    pub permille: u16,
    /// The pair of the folders that hold these, when it is one too.
    pub parent: Option<usize>,
}

impl Pair {
    pub fn identical(&self) -> bool {
        self.permille == 1000
    }
}

/// A folder of a tree: (tree, node).
type Key = (u32, Id);

/// What counts in each folder of a tree, and how much of it is files under the
/// smallest size.
#[derive(Clone, Copy, Debug, Default)]
struct Totals {
    bytes: u64,
    files: u64,
    small_bytes: u64,
}

struct Sums {
    /// The index into `tot` of each folder (NONE for files).
    slot: Vec<u32>,
    tot: Vec<Totals>,
}

impl Sums {
    fn new(t: &Tree, f: &Filter) -> Sums {
        let mut slot = vec![NONE; t.len()];
        let mut tot = Vec::new();
        for id in 0..t.len() as Id {
            if t.node(id).is_dir() {
                slot[id as usize] = tot.len() as u32;
                tot.push(Totals::default());
            }
        }
        walk(t, t.root(), f, &mut |_, n| {
            let s: &mut Totals = &mut tot[slot[n.parent as usize] as usize];
            s.bytes += n.size;
            s.files += 1;
            if n.size < f.min_size {
                s.small_bytes += n.size;
            }
        });
        // Children come after their folder: add up from the last.
        for id in (1..t.len()).rev() {
            let s = slot[id];
            if s != NONE {
                let v = tot[s as usize];
                let p = &mut tot[slot[t.node(id as Id).parent as usize] as usize];
                p.bytes += v.bytes;
                p.files += v.files;
                p.small_bytes += v.small_bytes;
            }
        }
        Sums { slot, tot }
    }

    fn of(&self, id: Id) -> Totals {
        self.tot[self.slot[id as usize] as usize]
    }
}

/// A pair of folders in a fixed order.
fn ordered(a: Key, b: Key) -> (Key, Key) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// A file with no known copy: a key no other file has.
fn unique(k: Key) -> u64 {
    1 << 63 | (k.0 as u64) << 32 | k.1 as u64
}

/// Finds the folders with files in common in `trees`. `copies` are the files
/// with the same contents (each a list of files), and `content` tells each of
/// those files (and more names of them) which list it is in. `small` compares
/// files under the smallest size and returns those with the same contents.
pub(crate) fn find(
    trees: &[Tree],
    f: &Filter,
    copies: &[Vec<Key>],
    content: &mut HashMap<Key, u32>,
    small: &mut dyn FnMut(Vec<Cand>) -> Result<Vec<Vec<Key>>, Failure>,
    cancel: &AtomicBool,
) -> Result<(Vec<Folder>, Vec<Pair>), Failure> {
    if copies.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    // 1. Pairs to look at, with the bytes of the copies they share.
    let mut shared: HashMap<(Key, Key), u64> = HashMap::new();
    for (n, group) in copies.iter().enumerate() {
        if n % 1024 == 0 && cancel.load(Ordering::Relaxed) {
            return Err(Failure::Stopped);
        }
        if group.len() > MAX_COPIES {
            continue;
        }
        let size = trees[group[0].0 as usize].node(group[0].1).size;
        for (i, &(ta, a)) in group.iter().enumerate() {
            for &(tb, b) in &group[i + 1..] {
                let (t, u) = (&trees[ta as usize], &trees[tb as usize]);
                let (mut x, mut y) = (t.node(a).parent, u.node(b).parent);
                loop {
                    if ta == tb && (t.is_within(x, y) || t.is_within(y, x)) {
                        break;
                    }
                    let k = ordered((ta, x), (tb, y));
                    if shared.len() < MAX_PAIRS || shared.contains_key(&k) {
                        *shared.entry(k).or_default() += size;
                    }
                    match (t.parent(x), u.parent(y)) {
                        (Some(px), Some(py)) => (x, y) = (px, py),
                        _ => break,
                    }
                }
            }
        }
    }
    if shared.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    // 2. Leave out what cannot reach the least share.
    let used: HashSet<u32> = shared.keys().flat_map(|(a, b)| [a.0, b.0]).collect();
    let sums: Vec<Option<Sums>> = (0..trees.len() as u32).map(|i| used.contains(&i).then(|| Sums::new(&trees[i as usize], f))).collect();
    let tot = |k: Key| sums[k.0 as usize].as_ref().map_or(Totals::default(), |s| s.of(k.1));
    let reaches = |same: u64, ta: Totals, tb: Totals| (same as u128) * 1000 >= MIN_PERMILLE as u128 * ta.bytes.max(tb.bytes) as u128;
    let mut pairs: Vec<(Key, Key)> = shared
        .into_iter()
        .filter(|&((a, b), same)| {
            let (ta, tb) = (tot(a), tot(b));
            let big = same.min(ta.bytes - ta.small_bytes).min(tb.bytes - tb.small_bytes);
            reaches(big + ta.small_bytes.min(tb.small_bytes), ta, tb)
        })
        .map(|(k, _)| k)
        .collect();
    pairs.sort_unstable();

    // 3. The smaller files of those folders (each once: not again for a folder
    // inside another of them).
    let dirs: HashSet<Key> = pairs.iter().flat_map(|&(a, b)| [a, b]).collect();
    let mut tops: Vec<Key> = dirs
        .iter()
        .copied()
        .filter(|&(t, d)| {
            let tree = &trees[t as usize];
            let mut up = tree.parent(d);
            while let Some(p) = up {
                if dirs.contains(&(t, p)) {
                    return false;
                }
                up = tree.parent(p);
            }
            true
        })
        .collect();
    tops.sort_unstable();
    let mut cands = Vec::new();
    for &(t, d) in &tops {
        if tot((t, d)).small_bytes > 0 {
            walk(&trees[t as usize], d, f, &mut |id, n| {
                if n.size < f.min_size {
                    cands.push(Cand { tree: t, id, size: n.size, mtime: n.mtime });
                }
            });
        }
    }
    if !cands.is_empty() {
        let next = copies.len() as u32;
        for (i, group) in small(cands)?.into_iter().enumerate() {
            for k in group {
                content.insert(k, next + i as u32);
            }
        }
    }

    // 4. Each pair, file by file: the contents of the files of each folder,
    // sorted, with their sizes.
    let mut lists: HashMap<Key, Vec<(u64, u64)>> = HashMap::new();
    for (n, &k) in dirs.iter().enumerate() {
        if n % 256 == 0 && cancel.load(Ordering::Relaxed) {
            return Err(Failure::Stopped);
        }
        let mut v = Vec::new();
        walk(&trees[k.0 as usize], k.1, f, &mut |id, n| {
            let c = content.get(&(k.0, id)).map_or(unique((k.0, id)), |&c| c as u64);
            v.push((c, n.size));
        });
        v.sort_unstable();
        lists.insert(k, v);
    }
    let mut kept: Vec<(Key, Key, u64, u64, u16)> = Vec::new();
    for &(a, b) in &pairs {
        let (la, lb) = (&lists[&a], &lists[&b]);
        let (mut i, mut j, mut files, mut bytes) = (0, 0, 0u64, 0u64);
        while i < la.len() && j < lb.len() {
            match la[i].0.cmp(&lb[j].0) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    files += 1;
                    bytes += la[i].1;
                    i += 1;
                    j += 1;
                }
            }
        }
        let (ta, tb) = (tot(a), tot(b));
        let larger = ta.bytes.max(tb.bytes);
        if bytes == 0 || !reaches(bytes, ta, tb) {
            continue;
        }
        let identical = files == la.len() as u64 && la.len() == lb.len();
        // Never 1000 unless identical.
        let permille = if identical { 1000 } else { ((bytes as u128 * 1000 / larger as u128) as u16).min(999) };
        kept.push((a, b, files, bytes, permille));
    }

    // Most in common first; folders numbered as they come.
    kept.sort_by(|x, y| y.3.cmp(&x.3).then(y.4.cmp(&x.4)).then(x.0.cmp(&y.0)).then(x.1.cmp(&y.1)));
    let mut folders = Vec::new();
    let mut index: HashMap<Key, usize> = HashMap::new();
    let mut folder = |k: Key| {
        *index.entry(k).or_insert_with(|| {
            let t = &trees[k.0 as usize];
            let s = tot(k);
            folders.push(Folder { path: t.path(k.1), bytes: s.bytes, files: s.files, mtime: t.node(k.1).mtime });
            folders.len() - 1
        })
    };
    let at: HashMap<(Key, Key), usize> = kept.iter().enumerate().map(|(i, p)| ((p.0, p.1), i)).collect();
    let out = kept
        .iter()
        .map(|&(a, b, same_files, same_bytes, permille)| {
            let up = |k: Key| trees[k.0 as usize].parent(k.1).map(|p| (k.0, p));
            let parent = match (up(a), up(b)) {
                (Some(pa), Some(pb)) => at.get(&ordered(pa, pb)).copied(),
                _ => None,
            };
            Pair { a: folder(a), b: folder(b), same_files, same_bytes, permille, parent }
        })
        .collect();
    Ok((folders, out))
}

/// Folders shown together: identical ones (any number), or two similar ones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// Indices into the folders, sorted by path.
    pub folders: Vec<usize>,
    /// The share in common, in thousandths (1000: identical).
    pub permille: u16,
    /// The bytes they have in common (for identical folders, of one of them).
    pub same_bytes: u64,
}

impl Group {
    pub fn identical(&self) -> bool {
        self.permille == 1000
    }

    /// The space that keeping one folder would free: all but one when
    /// identical, what they share when similar.
    pub fn wasted(&self) -> u64 {
        if self.identical() {
            self.same_bytes * (self.folders.len() as u64 - 1)
        } else {
            self.same_bytes
        }
    }
}

/// The groups of folders to show for the least share `min` (thousandths):
/// identical pairs that share a folder are one group, and a pair inside a pair
/// with as much in common or more is left out (it says nothing new). A pair
/// inside one with less in common stays: identical folders inside similar
/// ones. So a lower share shows all a higher one does, and more.
pub fn groups(folders: &[Folder], pairs: &[Pair], min: u16) -> Vec<Group> {
    let shown: Vec<bool> = pairs
        .iter()
        .map(|p| {
            let mut up = p.parent;
            while let Some(i) = up {
                if pairs[i].permille >= p.permille {
                    return false;
                }
                up = pairs[i].parent;
            }
            p.permille >= min
        })
        .collect();
    // Identical folders: one group for each set of them.
    let mut root: HashMap<usize, usize> = HashMap::new();
    fn rep(root: &mut HashMap<usize, usize>, x: usize) -> usize {
        let mut r = x;
        while let Some(&p) = root.get(&r).filter(|&&p| p != r) {
            r = p;
        }
        root.insert(x, r);
        r
    }
    let mut out = Vec::new();
    for (p, _) in pairs.iter().zip(&shown).filter(|(_, &s)| s) {
        if p.identical() {
            let (ra, rb) = (rep(&mut root, p.a), rep(&mut root, p.b));
            root.insert(ra, rb);
        } else {
            out.push(Group { folders: vec![p.a, p.b], permille: p.permille, same_bytes: p.same_bytes });
        }
    }
    let mut sets: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut members: Vec<usize> = root.keys().copied().collect();
    members.sort_unstable();
    for x in members {
        let r = rep(&mut root, x);
        sets.entry(r).or_default().push(x);
    }
    for (_, v) in sets {
        out.push(Group { same_bytes: folders[v[0]].bytes, folders: v, permille: 1000 });
    }
    for g in &mut out {
        g.folders.sort_by(|&x, &y| folders[x].path.cmp(&folders[y].path));
    }
    out.sort_by(|x, y| {
        y.wasted().cmp(&x.wasted()).then(y.permille.cmp(&x.permille)).then(folders[x.folders[0]].path.cmp(&folders[y.folders[0]].path))
    });
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::search::tests::data;
    use crate::search::{find_with, Found, Options};
    use rt_scan::testing::TempTree;
    use rt_scan::Ways;
    use std::path::Path;

    /// Photos, a backup of them, and an older copy with one file more.
    pub(crate) fn folders(tag: &str) -> TempTree {
        let tt = TempTree::new(tag);
        for dir in ["photos/2024", "backup/photos/2024", "old/2024"] {
            tt.write(&format!("{dir}/a.jpg"), &data(3000, 1));
            tt.write(&format!("{dir}/b.jpg"), &data(4000, 2));
        }
        tt.write("photos/2024/note.txt", &data(100, 3));
        tt.write("backup/photos/2024/renamed.txt", &data(100, 3));
        tt.write("old/2024/extra.bin", &data(500, 4));
        tt.write("unique.bin", &data(9000, 5));
        tt
    }

    pub(crate) fn search(tt: &TempTree, min: u64) -> Found {
        find_with(std::slice::from_ref(&tt.0), Ways::SYSTEM, Options::with(Filter::min(min)), &AtomicBool::new(false), &|_| {}).unwrap()
    }

    fn names(f: &Found, g: &Group, root: &Path) -> Vec<String> {
        g.folders.iter().map(|&i| f.folders[i].path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/")).collect()
    }

    #[test]
    fn identical_and_similar_folders() {
        let tt = folders("rdup-folders");
        let root = rt_scan::normalize(&tt.0);
        // Any size, and with the smaller files compared only in those folders.
        for min in [1, 1000] {
            let f = search(&tt, min);
            let same = groups(&f.folders, &f.pairs, 1000);
            assert_eq!(same.len(), 1, "min {min}: {:?}", f.pairs);
            assert_eq!(names(&f, &same[0], &root), ["backup/photos", "photos"], "the outer pair; 2024 inside it is hidden");
            assert_eq!((same[0].same_bytes, same[0].wasted()), (7100, 7100));
            let alike = groups(&f.folders, &f.pairs, 900);
            assert_eq!(alike.len(), 3);
            assert_eq!(alike[0], same[0]);
            for g in &alike[1..] {
                assert_eq!((g.permille, g.same_bytes), (933, 7000), "7000 of the 7500 bytes of old");
                assert!(names(&f, g, &root).contains(&"old".to_string()));
            }
            // The pairs inside, shown when the outer one is not.
            let inner = f.pairs.iter().find(|p| f.folders[p.a].path.ends_with("2024") && f.pairs[p.parent.unwrap()].identical()).unwrap();
            assert!(inner.identical());
            assert_eq!(inner.same_files, 3);
        }
        // Too little in common.
        let f = search(&tt, 1);
        assert!(f.pairs.iter().all(|p| p.permille >= MIN_PERMILLE));
        // Files only: no folders looked for.
        let opts = Options { files: true, folders: false, ..Options::default() };
        let f = find_with(std::slice::from_ref(&tt.0), Ways::SYSTEM, opts, &AtomicBool::new(false), &|_| {}).unwrap();
        assert_eq!((f.groups.len(), f.pairs.len(), f.look), (3, 0, [true, false]), "a.jpg, b.jpg and the notes");
    }

    #[test]
    fn groups_join_identical_folders() {
        let folder = |p: &str| Folder { path: p.into(), bytes: 100, files: 1, mtime: 0 };
        let fs = vec![folder("c"), folder("a"), folder("b"), folder("d")];
        let pair = |a, b, permille, parent| Pair { a, b, same_files: 1, same_bytes: 100, permille, parent };
        let pairs = vec![pair(0, 1, 1000, None), pair(1, 2, 1000, None), pair(0, 2, 1000, None), pair(2, 3, 600, None)];
        let g = groups(&fs, &pairs, 1000);
        assert_eq!(g, [Group { folders: vec![1, 2, 0], permille: 1000, same_bytes: 100 }]);
        assert_eq!(g[0].wasted(), 200);
        assert_eq!(groups(&fs, &pairs, 500).len(), 2);
    }

    #[test]
    fn a_lower_share_shows_all_a_higher_one_does() {
        let folder = |p: &str| Folder { path: p.into(), bytes: 100, files: 1, mtime: 0 };
        let fs: Vec<Folder> = ["a", "b", "a/x", "b/x", "a/x/y", "b/x/y"].iter().map(|p| folder(p)).collect();
        let pair = |a, b, permille, parent| Pair { a, b, same_files: 1, same_bytes: 100, permille, parent };
        // Similar folders, identical ones inside them, and inside those again.
        let pairs = vec![pair(0, 1, 600, None), pair(2, 3, 1000, Some(0)), pair(4, 5, 1000, Some(1))];
        let shown = |min| -> Vec<Vec<usize>> { groups(&fs, &pairs, min).into_iter().map(|g| g.folders).collect() };
        assert_eq!(shown(1000), [vec![2, 3]], "the identical ones, not those inside them");
        assert_eq!(shown(900), shown(1000));
        assert_eq!(shown(500), [vec![2, 3], vec![0, 1]]);
        // And on real folders: each share keeps what the one above showed.
        let tt = folders("rdup-folders-shares");
        let f = search(&tt, 1);
        let sets: Vec<Vec<Group>> = SHARES.iter().map(|&s| groups(&f.folders, &f.pairs, s)).collect();
        for w in sets.windows(2) {
            assert!(w[0].iter().all(|g| w[1].contains(g)), "{:?} in {:?}", w[0], w[1]);
        }
    }
}
