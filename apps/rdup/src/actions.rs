//! What can be done with the copies found: marking all but one copy of each
//! group by a rule, moving the marked ones to the Recycle Bin or to a folder,
//! and writing the list to a CSV file.
//!
//! Nothing here asks the user: the window confirms first. One copy of each
//! group is always kept (`App` refuses to mark the last one).

use crate::i18n::tr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Which copy of each group to keep; the others are marked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rule {
    Newest,
    Oldest,
    /// The one with the shortest path (the first by name when as long).
    Shortest,
    /// Those inside this folder; groups with no copy in it, or all, are left
    /// alone.
    InFolder(PathBuf),
}

/// Whether `path` is `dir` or inside it (without case on Windows).
pub fn within(path: &Path, dir: &Path) -> bool {
    let key = |p: &Path| {
        let s = rt_scan::normalize(p).to_string_lossy().replace('\\', "/");
        let s = if cfg!(windows) { s.to_lowercase() } else { s };
        s.trim_end_matches('/').to_string()
    };
    let (p, d) = (key(path), key(dir));
    p == d || p.strip_prefix(&d).is_some_and(|rest| rest.starts_with('/')) || (d.is_empty() && !p.is_empty())
}

/// The copies of a group to mark by `rule`: all but the ones kept. `items`
/// are (path, last change) in the order shown.
pub fn mark_by(rule: &Rule, items: &[(&Path, i64)]) -> Vec<usize> {
    if items.len() < 2 {
        return Vec::new();
    }
    let keep = match rule {
        Rule::Newest => (0..items.len()).max_by_key(|&i| (items[i].1, std::cmp::Reverse(i))),
        Rule::Oldest => (0..items.len()).min_by_key(|&i| (items[i].1, i)),
        Rule::Shortest => (0..items.len()).min_by_key(|&i| (items[i].0.as_os_str().len(), i)),
        Rule::InFolder(dir) => {
            let inside: Vec<bool> = items.iter().map(|(p, _)| within(p, dir)).collect();
            if inside.iter().all(|&b| b) || !inside.iter().any(|&b| b) {
                return Vec::new();
            }
            return (0..items.len()).filter(|&i| !inside[i]).collect();
        }
    };
    (0..items.len()).filter(|&i| Some(i) != keep).collect()
}

/// What to do with the marked copies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// To the Recycle Bin (the trash on Linux).
    Recycle,
    /// Into this folder, keeping their names (with a number when taken).
    MoveTo(PathBuf),
}

/// Does `op` to each of `paths`, telling `progress` which one it is on.
/// Returns the paths that are gone from where they were, and the others with
/// the reason.
pub fn run(op: &Op, paths: &[PathBuf], progress: &dyn Fn(usize, &Path)) -> (Vec<PathBuf>, Vec<(PathBuf, String)>) {
    let (mut done, mut failed) = (Vec::new(), Vec::new());
    for (i, p) in paths.iter().enumerate() {
        progress(i, p);
        let r = match op {
            Op::Recycle => rt_sys::recycle(p),
            Op::MoveTo(dir) => move_into(p, dir).map(|_| ()),
        };
        match r {
            Ok(()) => done.push(p.clone()),
            // An empty reason: the user said no to deleting it for good.
            Err(e) if e.is_empty() => failed.push((p.clone(), tr("It was left where it is."))),
            Err(e) => failed.push((p.clone(), e)),
        }
    }
    (done, failed)
}

/// A name for `name` in `dir` that is not taken: `name`, then `name (2)`...
fn free_name(dir: &Path, name: &str) -> PathBuf {
    let (stem, ext) = match name.rfind('.').filter(|&d| d > 0) {
        Some(d) => (&name[..d], &name[d..]),
        None => (name, ""),
    };
    let mut n = 1;
    loop {
        let candidate = if n == 1 { dir.join(name) } else { dir.join(format!("{stem} ({n}){ext}")) };
        if candidate.symlink_metadata().is_err() {
            return candidate;
        }
        n += 1;
    }
}

/// Moves the file or folder `path` into `dir`. Returns where it went. On
/// another drive it is copied, then the original is removed once the copy
/// is complete.
pub fn move_into(path: &Path, dir: &Path) -> Result<PathBuf, String> {
    let name = path.file_name().ok_or_else(|| tr("It has no name."))?.to_string_lossy().into_owned();
    if within(dir, path) {
        return Err(tr("A folder cannot be moved into itself."));
    }
    if !dir.is_dir() {
        return Err(tr("The folder to move to is not there."));
    }
    let to = free_name(dir, &name);
    if fs::rename(path, &to).is_ok() {
        return Ok(to);
    }
    // Another drive (or a rename refused): copy, then remove.
    let copied = if path.is_dir() { copy_dir(path, &to) } else { fs::copy(path, &to).map(|_| ()) };
    if let Err(e) = copied {
        let _ = if to.is_dir() { fs::remove_dir_all(&to) } else { fs::remove_file(&to) };
        return Err(e.to_string());
    }
    let removed = if path.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
    removed.map_err(|e| e.to_string())?;
    Ok(to)
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir(to)?;
    for e in fs::read_dir(from)? {
        let e = e?;
        let t = e.file_type()?;
        let dest = to.join(e.file_name());
        if t.is_dir() {
            copy_dir(&e.path(), &dest)?;
        } else if t.is_file() {
            fs::copy(e.path(), &dest)?;
        } else {
            // Links are not followed: what they lead to is not part of the folder.
            return Err(io::Error::other(tr("It holds a link, which is not moved to another drive.")));
        }
    }
    Ok(())
}

/// A CSV file: a header and rows of fields, with a BOM for spreadsheets,
/// commas and CRLF line ends.
pub fn csv(header: &[String], rows: &[Vec<String>]) -> String {
    let field = |s: &str| {
        if s.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let line = |r: &[String]| r.iter().map(|s| field(s)).collect::<Vec<_>>().join(",");
    let mut out = String::from("\u{FEFF}");
    out.push_str(&line(header));
    out.push_str("\r\n");
    for r in rows {
        out.push_str(&line(r));
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_scan::testing::TempTree;

    #[test]
    fn rules_keep_one_copy() {
        let (a, b, c) = (Path::new("C:/x/long/name.bin"), Path::new("D:/b.bin"), Path::new("E:/backup/b.bin"));
        let items = [(a, 300), (b, 100), (c, 200)];
        assert_eq!(mark_by(&Rule::Newest, &items), [1, 2]);
        assert_eq!(mark_by(&Rule::Oldest, &items), [0, 2]);
        assert_eq!(mark_by(&Rule::Shortest, &items), [0, 2]);
        assert_eq!(mark_by(&Rule::InFolder("E:/backup".into()), &items), [0, 1]);
        assert_eq!(mark_by(&Rule::InFolder("E:/back".into()), &items), [] as [usize; 0], "not a folder of them");
        assert_eq!(mark_by(&Rule::InFolder("/".into()), &items[1..2]), [] as [usize; 0]);
        // The same date: the first is kept.
        assert_eq!(mark_by(&Rule::Newest, &[(a, 5), (b, 5)]), [1]);
        assert!(within(Path::new("C:/A/b"), Path::new("c:/a")) || !cfg!(windows));
        assert!(!within(Path::new("C:/ab"), Path::new("C:/a")));
    }

    #[test]
    fn moving_keeps_names_apart() {
        let tt = TempTree::new("rdup-move");
        tt.write("a/x.txt", b"one");
        tt.write("b/x.txt", b"two");
        tt.write("c/inner/y.txt", b"three");
        let dest = tt.0.join("dest");
        fs::create_dir(&dest).unwrap();
        let (done, failed) = run(&Op::MoveTo(dest.clone()), &[tt.0.join("a/x.txt"), tt.0.join("b/x.txt"), tt.0.join("c")], &|_, _| {});
        assert_eq!((done.len(), failed.len()), (3, 0));
        assert_eq!(fs::read(dest.join("x.txt")).unwrap(), b"one");
        assert_eq!(fs::read(dest.join("x (2).txt")).unwrap(), b"two");
        assert_eq!(fs::read(dest.join("c/inner/y.txt")).unwrap(), b"three");
        assert!(!tt.0.join("c").exists());
        // Into itself.
        let (_, failed) = run(&Op::MoveTo(dest.join("c")), std::slice::from_ref(&dest), &|_, _| {});
        assert_eq!(failed.len(), 1);
        assert!(dest.exists());
    }

    #[test]
    fn csv_quotes_what_needs_it() {
        let h = vec!["Name".to_string(), "Size".to_string()];
        let rows = vec![vec!["a, b".to_string(), "12".to_string()], vec!["say \"hi\"".to_string(), "3".to_string()]];
        assert_eq!(csv(&h, &rows), "\u{FEFF}Name,Size\r\n\"a, b\",12\r\n\"say \"\"hi\"\"\",3\r\n");
    }
}
