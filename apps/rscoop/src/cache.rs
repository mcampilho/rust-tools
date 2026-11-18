//! SQLite, used twice:
//!
//! - our own cache (`%LOCALAPPDATA%\rscoop\cache.db`) of what was read from
//!   each file — shims, install/manifest JSON, .reg files and the paths found
//!   in config files — keyed by path, size and modification time. A file that
//!   did not change is not opened again, which is what made the config check
//!   slow (and the antivirus reads every file on its first open);
//! - Scoop's own `scoop.db` (when `use_sqlite_cache` is on), which lists the
//!   manifests of every bucket: one query instead of one file look-up per app.

use rusqlite::{params, Connection, OpenFlags};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Rows unseen for this many days are dropped.
const KEEP_DAYS: i64 = 30;

struct Row {
    size: i64,
    mtime: i64,
    data: Option<String>,
}

/// A row read this run: key, size, time, data.
type Fresh = (String, i64, i64, Option<String>);

#[derive(Default)]
pub struct Cache {
    file: Option<PathBuf>,
    rows: HashMap<String, Row>,
    fresh: Mutex<Vec<Fresh>>,
    touched: Mutex<Vec<String>>,
    pub hits: AtomicUsize,
    pub misses: AtomicUsize,
}

fn today() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64 / 86400).unwrap_or(0)
}

/// Size and modification time (ns) as stored in the cache.
pub fn stamp_of(md: &std::fs::Metadata) -> (i64, i64) {
    let mtime = md.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_nanos() as i64).unwrap_or(0);
    (md.len() as i64, mtime)
}

fn open(file: &Path) -> rusqlite::Result<Connection> {
    let c = Connection::open(file)?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS files_v1 (
            key TEXT PRIMARY KEY, size INTEGER NOT NULL, mtime INTEGER NOT NULL, data TEXT, seen INTEGER NOT NULL
        );",
    )?;
    Ok(c)
}

impl Cache {
    /// A cache that remembers nothing.
    pub fn disabled() -> Cache {
        Cache::default()
    }

    /// Load the cache from `file` (created when missing). A damaged file is replaced.
    pub fn load(file: &Path) -> Cache {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut cache = Cache { file: Some(file.to_path_buf()), ..Cache::default() };
        let read = |c: &Connection| -> rusqlite::Result<HashMap<String, Row>> {
            let mut st = c.prepare("SELECT key, size, mtime, data FROM files_v1")?;
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, Row { size: r.get(1)?, mtime: r.get(2)?, data: r.get(3)? })))?;
            rows.collect()
        };
        match open(file).and_then(|c| read(&c)) {
            Ok(rows) => cache.rows = rows,
            Err(_) => {
                let _ = std::fs::remove_file(file);
            }
        }
        cache
    }

    /// What `compute` gives for the file at `path`, from the cache when the
    /// file's size and time did not change.
    pub fn get(&self, kind: &str, path: &str, stamp: (i64, i64), compute: impl FnOnce() -> Option<String>) -> Option<String> {
        if self.file.is_none() {
            return compute();
        }
        let key = format!("{kind}|{}", crate::paths::norm(path));
        if let Some(row) = self.rows.get(&key) {
            if (row.size, row.mtime) == stamp {
                self.hits.fetch_add(1, Ordering::Relaxed);
                self.touched.lock().unwrap().push(key);
                return row.data.clone();
            }
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        let data = compute();
        self.fresh.lock().unwrap().push((key, stamp.0, stamp.1, data.clone()));
        data
    }

    /// Like [`get`](Self::get), taking the stamp from the file itself.
    pub fn get_file(&self, kind: &str, path: &str, compute: impl FnOnce() -> Option<String>) -> Option<String> {
        match std::fs::metadata(path) {
            Ok(md) => self.get(kind, path, stamp_of(&md), compute),
            Err(_) => compute(),
        }
    }

    /// Write what was read this run and drop rows unseen for a month.
    pub fn save(&self) -> Result<(), String> {
        let Some(file) = &self.file else { return Ok(()) };
        let fresh = std::mem::take(&mut *self.fresh.lock().unwrap());
        let touched = std::mem::take(&mut *self.touched.lock().unwrap());
        let day = today();
        let run = || -> rusqlite::Result<()> {
            let mut c = open(file)?;
            let tx = c.transaction()?;
            {
                let mut put =
                    tx.prepare("INSERT OR REPLACE INTO files_v1 (key, size, mtime, data, seen) VALUES (?1, ?2, ?3, ?4, ?5)")?;
                for (k, s, m, d) in &fresh {
                    put.execute(params![k, s, m, d, day])?;
                }
                let mut seen = tx.prepare("UPDATE files_v1 SET seen = ?1 WHERE key = ?2 AND seen <> ?1")?;
                for k in &touched {
                    seen.execute(params![day, k])?;
                }
                tx.execute("DELETE FROM files_v1 WHERE seen < ?1", params![day - KEEP_DAYS])?;
            }
            tx.commit()
        };
        run().map_err(|e| e.to_string())
    }

    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }
}

/// The (bucket, app) pairs of Scoop's `scoop.db`, lower case; None when the
/// database is missing or unreadable.
pub fn scoop_db_manifests(root: &str) -> Option<HashSet<(String, String)>> {
    let file = Path::new(root).join("scoop.db");
    if !file.is_file() {
        return None;
    }
    let c = Connection::open_with_flags(&file, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    let mut st = c.prepare("SELECT DISTINCT lower(bucket), lower(name) FROM app").ok()?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).ok()?;
    let set: HashSet<_> = rows.filter_map(Result::ok).collect();
    (!set.is_empty()).then_some(set)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_until_the_file_changes() {
        let dir = crate::tests::scratch("cache");
        let db = dir.join("cache.db");
        let f = dir.join("a.txt");
        std::fs::write(&f, "one").unwrap();
        let p = f.to_string_lossy().to_string();

        let c = Cache::load(&db);
        assert_eq!(c.get_file("t", &p, || Some("first".into())).as_deref(), Some("first"));
        c.save().unwrap();

        let c = Cache::load(&db);
        assert_eq!(c.get_file("t", &p, || Some("second".into())).as_deref(), Some("first"));
        assert_eq!(c.hits.load(Ordering::Relaxed), 1);
        std::fs::write(&f, "changed!").unwrap();
        assert_eq!(c.get_file("t", &p, || Some("third".into())).as_deref(), Some("third"));
        c.save().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reads_a_scoop_db() {
        let dir = crate::tests::scratch("scoopdb");
        let c = Connection::open(dir.join("scoop.db")).unwrap();
        c.execute_batch(
            "CREATE TABLE app (name TEXT, description TEXT, version TEXT, bucket VARCHAR, manifest JSON);
             INSERT INTO app VALUES ('Git', '', '1', 'Main', '{}');",
        )
        .unwrap();
        drop(c);
        let set = scoop_db_manifests(&dir.to_string_lossy()).unwrap();
        assert!(set.contains(&("main".to_string(), "git".to_string())));
        let _ = std::fs::remove_dir_all(dir);
    }
}
