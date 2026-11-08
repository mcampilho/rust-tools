//! The hashes of files already read, kept between searches in `hashes.bin`
//! next to the settings: a file whose size and last change are the same as
//! when it was hashed is not read again.
//!
//! Each entry holds what the steps found: the hash of the first and last
//! bytes (of the whole file when small), the hash of the whole file, and the
//! file's identity (to see two names of one file). An entry not used for
//! [`KEEP_DAYS`] days is dropped when the cache is saved.
//!
//! The file: `RDUPHC1\n`, the number of entries (u64), then each entry: the
//! path (u32 length, UTF-8), size (u64), last change (i64), day last used
//! (u32), which parts it has (u8), identity (2 × u64), hashes (32 bytes each,
//! only those it has). All little-endian. A file that cannot be read whole is
//! ignored: the cache starts empty.

use crate::search::Hash;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"RDUPHC1\n";
/// Entries not used for this long are dropped.
pub const KEEP_DAYS: u32 = 120;
/// The most entries kept (the most recently used).
const MAX_ENTRIES: usize = 4_000_000;

const HAS_IDENT: u8 = 1;
const HAS_EDGES: u8 = 2;
const HAS_WHOLE: u8 = 4;

/// What is known of one file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub size: u64,
    pub mtime: i64,
    /// The day (since 1970) it was last used.
    pub used: u32,
    pub ident: Option<(u64, u64)>,
    pub edges: Option<Hash>,
    pub whole: Option<Hash>,
}

#[derive(Default)]
pub struct Cache {
    entries: HashMap<String, Entry>,
    today: u32,
    changed: bool,
}

/// Days since 1970, today.
fn today() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| (d.as_secs() / 86_400) as u32)
}

fn key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl Cache {
    pub fn new() -> Cache {
        Cache { today: today(), ..Cache::default() }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// What is known of `path`, if it still has this size and last change.
    /// Marks it used today.
    pub fn get(&mut self, path: &Path, size: u64, mtime: i64) -> Option<&Entry> {
        let today = self.today;
        let e = self.entries.get_mut(&key(path)).filter(|e| e.size == size && e.mtime == mtime)?;
        if e.used != today {
            e.used = today;
            self.changed = true;
        }
        Some(e)
    }

    /// Notes what was found of `path` (with this size and last change); what
    /// is not given is kept when the file did not change.
    pub fn put(&mut self, path: &Path, size: u64, mtime: i64, ident: Option<(u64, u64)>, edges: Option<Hash>, whole: Option<Hash>) {
        let e = self.entries.entry(key(path)).or_default();
        if e.size != size || e.mtime != mtime {
            *e = Entry { size, mtime, ..Entry::default() };
        }
        e.used = self.today;
        e.ident = ident.or(e.ident);
        e.edges = edges.or(e.edges);
        e.whole = whole.or(e.whole);
        self.changed = true;
    }

    /// Reads the cache from `file` (empty when it is not there or not whole).
    pub fn load(file: &Path) -> Cache {
        let mut c = Cache::new();
        if let Ok(f) = File::open(file) {
            if let Ok(entries) = read(&mut BufReader::new(f)) {
                c.entries = entries;
            }
        }
        c
    }

    /// Writes the cache to `file` when it changed, dropping old entries.
    pub fn save(&mut self, file: &Path) -> io::Result<()> {
        if !self.changed {
            return Ok(());
        }
        let oldest = self.today.saturating_sub(KEEP_DAYS);
        self.entries.retain(|_, e| e.used >= oldest);
        if self.entries.len() > MAX_ENTRIES {
            let mut days: Vec<u32> = self.entries.values().map(|e| e.used).collect();
            days.sort_unstable_by(|a, b| b.cmp(a));
            let cut = days[MAX_ENTRIES];
            self.entries.retain(|_, e| e.used > cut);
        }
        // Written beside, then put in place: a cut-off write never leaves half a file.
        let tmp: PathBuf = file.with_extension("tmp");
        {
            let mut w = BufWriter::new(File::create(&tmp)?);
            write(&mut w, &self.entries)?;
            w.flush()?;
        }
        std::fs::rename(&tmp, file)?;
        self.changed = false;
        Ok(())
    }
}

fn read(r: &mut impl Read) -> io::Result<HashMap<String, Entry>> {
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut b8 = [0u8; 8];
    let mut u64_ = |r: &mut dyn Read| -> io::Result<u64> {
        r.read_exact(&mut b8)?;
        Ok(u64::from_le_bytes(b8))
    };
    let n = u64_(r)?;
    let mut out = HashMap::with_capacity(n.min(MAX_ENTRIES as u64) as usize);
    for _ in 0..n {
        let mut b4 = [0u8; 4];
        r.read_exact(&mut b4)?;
        let len = u32::from_le_bytes(b4) as usize;
        if len > 64 * 1024 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut name = vec![0u8; len];
        r.read_exact(&mut name)?;
        let path = String::from_utf8(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        let size = u64_(r)?;
        let mtime = u64_(r)? as i64;
        r.read_exact(&mut b4)?;
        let used = u32::from_le_bytes(b4);
        let mut has = [0u8; 1];
        r.read_exact(&mut has)?;
        let has = has[0];
        let (a, b) = (u64_(r)?, u64_(r)?);
        let mut hash = |on: bool| -> io::Result<Option<Hash>> {
            if !on {
                return Ok(None);
            }
            let mut h = [0u8; 32];
            r.read_exact(&mut h)?;
            Ok(Some(h))
        };
        let edges = hash(has & HAS_EDGES != 0)?;
        let whole = hash(has & HAS_WHOLE != 0)?;
        let ident = (has & HAS_IDENT != 0).then_some((a, b));
        out.insert(path, Entry { size, mtime, used, ident, edges, whole });
    }
    Ok(out)
}

fn write(w: &mut impl Write, entries: &HashMap<String, Entry>) -> io::Result<()> {
    w.write_all(MAGIC)?;
    w.write_all(&(entries.len() as u64).to_le_bytes())?;
    for (path, e) in entries {
        w.write_all(&(path.len() as u32).to_le_bytes())?;
        w.write_all(path.as_bytes())?;
        w.write_all(&e.size.to_le_bytes())?;
        w.write_all(&e.mtime.to_le_bytes())?;
        w.write_all(&e.used.to_le_bytes())?;
        let has = if e.ident.is_some() { HAS_IDENT } else { 0 } | if e.edges.is_some() { HAS_EDGES } else { 0 } | if e.whole.is_some() { HAS_WHOLE } else { 0 };
        w.write_all(&[has])?;
        let (a, b) = e.ident.unwrap_or_default();
        w.write_all(&a.to_le_bytes())?;
        w.write_all(&b.to_le_bytes())?;
        for h in [e.edges, e.whole].into_iter().flatten() {
            w.write_all(&h)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_scan::testing::TempTree;

    #[test]
    fn entries_survive_a_save_and_change_with_the_file() {
        let tt = TempTree::new("rdup-cache");
        let file = tt.0.join("hashes.bin");
        let mut c = Cache::new();
        let p = Path::new("C:/data/a b/fotó.jpg");
        c.put(p, 100, 5, Some((1, 2)), Some([7; 32]), None);
        c.put(p, 100, 5, None, None, Some([9; 32]));
        c.put(Path::new("D:/x"), 1, 1, None, Some([1; 32]), None);
        c.save(&file).unwrap();
        let mut c = Cache::load(&file);
        assert_eq!(c.len(), 2);
        let e = c.get(p, 100, 5).unwrap().clone();
        assert_eq!((e.ident, e.edges, e.whole), (Some((1, 2)), Some([7; 32]), Some([9; 32])));
        assert!(c.get(p, 101, 5).is_none(), "another size");
        assert!(c.get(p, 100, 6).is_none(), "changed since");
        // Changed: what was known goes.
        c.put(p, 100, 6, None, Some([3; 32]), None);
        assert_eq!(c.get(p, 100, 6).unwrap().whole, None);
        // Old entries are dropped; a broken file gives an empty cache.
        c.entries.get_mut("D:/x").unwrap().used = 0;
        c.save(&file).unwrap();
        assert_eq!(Cache::load(&file).len(), 1);
        std::fs::write(&file, b"RDUPHC1\n\x05\0\0\0\0\0\0\0").unwrap();
        assert!(Cache::load(&file).is_empty());
    }
}
