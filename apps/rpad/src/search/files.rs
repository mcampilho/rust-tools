//! Find in files: walks a folder on a background thread and reports
//! matching lines. Binary files, very large files and common build/VCS
//! folders are skipped. Cancellable at any time.

use crate::io::encoding::{decode, system_legacy};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    /// 0-based line number.
    pub line: usize,
    /// Line text (truncated for display).
    pub text: String,
    /// Match position within `text` (char columns).
    pub col: (usize, usize),
}

pub const MAX_FILE: u64 = 32 * 1024 * 1024;
pub const MAX_HITS: usize = 20_000;
const SKIP_DIRS: &[&str] = &[".git", ".hg", ".svn", "node_modules", "target", ".vs", "bin", "obj", "__pycache__", ".idea"];

/// `*.rs;*.toml` style filter (empty = all files).
pub fn matches_filter(name: &str, filter: &str) -> bool {
    let pats: Vec<&str> = filter.split([';', ',']).map(str::trim).filter(|p| !p.is_empty()).collect();
    if pats.is_empty() {
        return true;
    }
    let lower = name.to_lowercase();
    pats.iter().any(|p| glob(&p.to_lowercase(), &lower))
}

fn glob(p: &str, s: &str) -> bool {
    let (pb, sb) = (p.as_bytes(), s.as_bytes());
    let (mut pi, mut si, mut star, mut mark) = (0, 0, None, 0);
    while si < sb.len() {
        if pi < pb.len() && (pb[pi] == b'?' || pb[pi] == sb[si]) {
            pi += 1;
            si += 1;
        } else if pi < pb.len() && pb[pi] == b'*' {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < pb.len() && pb[pi] == b'*' {
        pi += 1;
    }
    pi == pb.len()
}

pub struct Progress {
    pub files_searched: usize,
    pub done: bool,
}

/// Searches `root` recursively. `on_hits` receives batches of results and
/// progress; return value of the callback is ignored. Stops when `cancel`
/// is set or `MAX_HITS` is reached.
pub fn search(root: &Path, filter: &str, re: &Regex, cancel: &Arc<AtomicBool>, mut on_hits: impl FnMut(Vec<Hit>, Progress)) {
    let mut stack = vec![root.to_path_buf()];
    let mut total = 0usize;
    let mut files = 0usize;
    let mut batch = Vec::new();
    while let Some(dir) = stack.pop() {
        if cancel.load(Ordering::Relaxed) || total >= MAX_HITS {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries.into_iter().rev() {
            let Ok(ft) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().into_owned();
            if ft.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    stack.push(e.path());
                }
                continue;
            }
            if !ft.is_file() || !matches_filter(&name, filter) {
                continue;
            }
            if e.metadata().map_or(true, |m| m.len() > MAX_FILE) {
                continue;
            }
            let Ok(bytes) = std::fs::read(e.path()) else { continue };
            if bytes[..bytes.len().min(8000)].contains(&0) && !(bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF])) {
                continue; // binary
            }
            files += 1;
            let d = decode(&bytes, None, system_legacy());
            if d.binary {
                continue;
            }
            for (i, line) in d.text.lines().enumerate() {
                if let Some(m) = re.find(line) {
                    let (text, col) = display_line(line, m.start(), m.end());
                    batch.push(Hit { path: e.path(), line: i, text, col });
                    total += 1;
                    if total >= MAX_HITS {
                        break;
                    }
                }
            }
            if batch.len() >= 200 {
                on_hits(std::mem::take(&mut batch), Progress { files_searched: files, done: false });
            }
            if cancel.load(Ordering::Relaxed) {
                break;
            }
        }
    }
    on_hits(batch, Progress { files_searched: files, done: true });
}

/// Trims a long line around the match; returns text and char columns.
fn display_line(line: &str, start: usize, end: usize) -> (String, (usize, usize)) {
    let pre: String = line[..start].chars().rev().take(60).collect::<Vec<_>>().into_iter().rev().collect();
    let cut = line[..start].chars().count() > 60;
    let m: String = line[start..end].chars().take(200).collect();
    let post: String = line[end..].chars().take(200).collect();
    let prefix = if cut { "…" } else { "" };
    let pre = format!("{prefix}{}", pre.trim_start());
    let c0 = pre.chars().count();
    let c1 = c0 + m.chars().count();
    (format!("{pre}{m}{post}").replace('\t', " "), (c0, c1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters() {
        assert!(matches_filter("main.rs", "*.rs;*.toml"));
        assert!(matches_filter("Cargo.TOML", "*.rs, *.toml"));
        assert!(!matches_filter("a.txt", "*.rs"));
        assert!(matches_filter("anything", ""));
        assert!(matches_filter("data1.csv", "data?.csv"));
    }

    #[test]
    fn searches_a_tree() {
        let root = std::env::temp_dir().join(format!("rpad-fif-{}", std::process::id()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("a.txt"), "hello\nworld hello\n").unwrap();
        std::fs::write(root.join("sub").join("b.rs"), "fn hello() {}\n").unwrap();
        std::fs::write(root.join(".git").join("c.txt"), "hello").unwrap();
        std::fs::write(root.join("bin.dat"), b"hello\0\0\0").unwrap();
        let re = Regex::new("hello").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut hits = Vec::new();
        let mut done = false;
        search(&root, "", &re, &cancel, |h, p| {
            hits.extend(h);
            done |= p.done;
        });
        assert!(done);
        assert_eq!(hits.len(), 3);
        assert!(hits.iter().any(|h| h.path.ends_with("b.rs") && h.line == 0 && h.col == (3, 8)));
        let mut only_rs = Vec::new();
        search(&root, "*.rs", &re, &cancel, |h, _| only_rs.extend(h));
        assert_eq!(only_rs.len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }
}
