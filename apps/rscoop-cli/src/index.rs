//! The index of `search` and `list` (`%LOCALAPPDATA%\rscoop\index.db`): what
//! they need from a file, keyed by its path, size and modification time.
//! `search` keeps the version and the programs of the general `bin` of each
//! manifest of the buckets (thousands: the directory listing already gives
//! size and time on Windows); `list` keeps the version of the manifest in
//! `current` and the installation data of each app. Only the files that
//! changed are opened again (and the antivirus reads each file opened).
//!
//! The index only remembers: a missing or damaged file is made again, and
//! with `RSCOOP_INDEX=0` (or no `LOCALAPPDATA`) every manifest is read.

use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// What `search` keeps of a manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub version: String,
    /// The programs: file name and alias (`bin_match_json` looks at both).
    pub bins: Vec<(String, Option<String>)>,
}

impl Entry {
    /// The version on the first line, then one `file<TAB>alias` line per program.
    fn encode(&self) -> String {
        let mut text = self.version.clone();
        for (file, alias) in &self.bins {
            text.push('\n');
            text += file;
            text.push('\t');
            text += alias.as_deref().unwrap_or("");
        }
        text
    }

    fn decode(text: &str) -> Entry {
        let mut lines = text.split('\n');
        let version = lines.next().unwrap_or("").to_string();
        let bins = lines
            .map(|line| {
                let (file, alias) = line.split_once('\t').unwrap_or((line, ""));
                (file.to_string(), (!alias.is_empty()).then(|| alias.to_string()))
            })
            .collect();
        Entry { version, bins }
    }
}

/// A row: size, modification time (ns) and the encoded entry.
type Row = (i64, i64, String);

#[derive(Default)]
pub struct Index {
    file: Option<PathBuf>,
    rows: HashMap<String, Row>,
    fresh: Mutex<Vec<(String, Row)>>,
    seen: Mutex<HashSet<String>>,
}

fn open(file: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(file)?;
    c.busy_timeout(std::time::Duration::from_secs(2))?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS files_v1 (path TEXT PRIMARY KEY, size INTEGER NOT NULL, mtime INTEGER NOT NULL, data TEXT NOT NULL);",
    )?;
    Ok(c)
}

/// Where the index is, or None when it is off.
pub fn location(env: &rt_scoop::install::Env) -> Option<PathBuf> {
    if env.get("RSCOOP_INDEX") == Some("0") {
        return None;
    }
    env.get("LOCALAPPDATA").filter(|v| !v.is_empty()).map(|base| Path::new(base).join("rscoop").join("index.db"))
}

impl Index {
    /// What the index in `file` keeps of the files under the folders `under`
    /// (None: an index that remembers nothing).
    pub fn load(file: Option<PathBuf>, under: &[PathBuf]) -> Index {
        let Some(file) = file else { return Index::default() };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let prefixes: Vec<String> = under.iter().map(|p| p.join("").to_string_lossy().into_owned()).collect();
        let read = |c: &Connection| -> rusqlite::Result<HashMap<String, Row>> {
            let mut st = c.prepare("SELECT path, size, mtime, data FROM files_v1 WHERE substr(path, 1, length(?1)) = ?1")?;
            let mut rows = HashMap::new();
            for prefix in &prefixes {
                let found = st.query_map([prefix], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))?;
                for row in found {
                    let (key, row) = row?;
                    rows.insert(key, row);
                }
            }
            Ok(rows)
        };
        let rows = match open(&file).and_then(|c| read(&c)) {
            Ok(rows) => rows,
            Err(_) => {
                let _ = std::fs::remove_file(&file);
                HashMap::new()
            }
        };
        Index { file: Some(file), rows, ..Index::default() }
    }

    /// What was kept of the file at `path`, from the index when its size and
    /// time (`metadata`) did not change; else `read` (None: unreadable).
    pub fn get(&self, path: &Path, metadata: Option<&std::fs::Metadata>, read: impl FnOnce() -> Option<String>) -> Option<String> {
        let (Some(_), Some(metadata)) = (&self.file, metadata) else { return read() };
        let key = path.to_string_lossy().into_owned();
        let (size, mtime) = crate::cache::stamp_of(metadata);
        self.seen.lock().unwrap().insert(key.clone());
        if let Some((s, m, data)) = self.rows.get(&key) {
            if (*s, *m) == (size, mtime) {
                return Some(data.clone());
            }
        }
        let data = read()?;
        self.fresh.lock().unwrap().push((key, (size, mtime, data.clone())));
        Some(data)
    }

    /// [`get`](Self::get) for a manifest of a bucket, as `search` keeps it.
    pub fn entry(&self, path: &Path, metadata: Option<&std::fs::Metadata>, read: impl FnOnce() -> Option<Entry>) -> Option<Entry> {
        self.get(path, metadata, || read().map(|e| e.encode())).map(|text| Entry::decode(&text))
    }

    /// Write what was read this run, and forget the files under each folder
    /// of `scanned` (looked at whole this run) that were not seen. Nothing is
    /// written when nothing changed.
    pub fn save(&self, scanned: &[PathBuf]) -> Result<(), String> {
        let Some(file) = &self.file else { return Ok(()) };
        let fresh = std::mem::take(&mut *self.fresh.lock().unwrap());
        let seen = std::mem::take(&mut *self.seen.lock().unwrap());
        let prefixes: Vec<String> = scanned.iter().map(|p| p.join("").to_string_lossy().into_owned()).collect();
        let gone: Vec<&String> =
            self.rows.keys().filter(|k| prefixes.iter().any(|p| k.starts_with(p.as_str())) && !seen.contains(*k)).collect();
        if fresh.is_empty() && gone.is_empty() {
            return Ok(());
        }
        let run = || -> rusqlite::Result<()> {
            let mut c = open(file)?;
            let tx = c.transaction()?;
            {
                let mut put = tx.prepare("INSERT OR REPLACE INTO files_v1 (path, size, mtime, data) VALUES (?1, ?2, ?3, ?4)")?;
                for (key, (size, mtime, data)) in &fresh {
                    put.execute(params![key, size, mtime, data])?;
                }
                let mut drop = tx.prepare("DELETE FROM files_v1 WHERE path = ?1")?;
                for key in &gone {
                    drop.execute(params![key])?;
                }
            }
            tx.commit()
        };
        run().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_until_the_manifest_changes() {
        let dir = crate::tests::scratch("index");
        let db = dir.join("index.db");
        let buckets = dir.join("buckets");
        std::fs::create_dir_all(&buckets).unwrap();
        let (a, b) = (buckets.join("a.json"), buckets.join("b.json"));
        std::fs::write(&a, "one").unwrap();
        std::fs::write(&b, "two").unwrap();
        let entry = |v: &str| Entry { version: v.into(), bins: vec![("rg.exe".into(), None), ("x.exe".into(), Some("xx".into()))] };
        let meta = |p: &Path| std::fs::metadata(p).ok();

        let index = Index::load(Some(db.clone()), std::slice::from_ref(&buckets));
        assert_eq!(index.entry(&a, meta(&a).as_ref(), || Some(entry("1"))), Some(entry("1")));
        assert_eq!(index.entry(&b, meta(&b).as_ref(), || Some(entry("2"))), Some(entry("2")));
        index.save(std::slice::from_ref(&buckets)).unwrap();

        let index = Index::load(Some(db.clone()), std::slice::from_ref(&buckets));
        assert_eq!(index.entry(&a, meta(&a).as_ref(), || panic!("read again")), Some(entry("1")));
        std::fs::write(&a, "changed").unwrap();
        assert_eq!(index.entry(&a, meta(&a).as_ref(), || Some(entry("3"))), Some(entry("3")));
        // b.json is not seen this run: it is forgotten.
        index.save(std::slice::from_ref(&buckets)).unwrap();
        let index = Index::load(Some(db), std::slice::from_ref(&buckets));
        assert_eq!(index.rows.len(), 1);
        assert_eq!(Entry::decode(&index.rows[&a.to_string_lossy().into_owned()].2), entry("3"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
