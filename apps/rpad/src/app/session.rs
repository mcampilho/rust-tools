//! Session restore, crash recovery of unsaved text, and local history.
//!
//! * `session.ini` lists the open tabs (paths, caret, encoding, ...).
//! * `recovery/<id>.txt` holds the text of modified documents. It is written
//!   a few seconds after edits (on a background thread from an O(1) rope
//!   snapshot) and on exit, and deleted when the document is saved/closed.
//! * `history/<hash>/<timestamp>.<ext>` keeps copies of files as they were
//!   before each save (only when enabled).

use crate::io::encoding::{Enc, Eol};
use crate::storage::{Location, Store};
use crate::syntax::Lang;
use ropey::Rope;
use std::path::{Path, PathBuf};

pub const SESSION_FILE: &str = "session.ini";
pub const RECOVERY_DIR: &str = "recovery";
pub const HISTORY_DIR: &str = "history";
pub const SNIPPETS_FILE: &str = "snippets.txt";

#[derive(Clone, Debug, PartialEq)]
pub struct SessionTab {
    pub path: Option<PathBuf>,
    /// Recovery file name in the recovery folder (unsaved text).
    pub recovery: Option<String>,
    pub title: String,
    pub enc: Enc,
    pub eol: Eol,
    pub lang: Option<Lang>,
    pub caret: usize,
    pub top: usize,
    pub active: bool,
}

pub fn serialize(tabs: &[SessionTab]) -> String {
    let mut s = Store::default();
    for t in tabs {
        s.push(
            "tab",
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                t.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
                t.recovery.clone().unwrap_or_default(),
                t.title,
                t.enc.id(),
                t.eol.name(),
                t.lang.map_or("", |l| l.id()),
                t.caret,
                t.top,
                u8::from(t.active)
            ),
        );
    }
    s.serialize("rpad")
}

pub fn parse(text: &str) -> Vec<SessionTab> {
    Store::parse(text)
        .get_all("tab")
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 9 {
                return None;
            }
            Some(SessionTab {
                path: (!f[0].is_empty()).then(|| PathBuf::from(f[0])),
                recovery: (!f[1].is_empty()).then(|| f[1].to_string()),
                title: f[2].to_string(),
                enc: Enc::from_id(f[3]).unwrap_or(Enc::Utf8),
                eol: Eol::from_name(f[4]).unwrap_or(Eol::platform()),
                lang: Lang::from_id(f[5]),
                caret: f[6].parse().unwrap_or(0),
                top: f[7].parse().unwrap_or(0),
                active: f[8] == "1",
            })
        })
        .filter(|t| t.path.is_some() || t.recovery.is_some())
        .collect()
}

pub fn recovery_name(doc_id: u64) -> String {
    format!("{}-{doc_id}.txt", std::process::id())
}

/// Writes recovery text atomically. Runs on a background thread.
pub fn write_recovery(dir: &Path, name: &str, text: Rope) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("{name}.tmp"));
    {
        use std::io::Write;
        let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        for chunk in text.chunks() {
            f.write_all(chunk.as_bytes())?;
        }
        f.flush()?;
    }
    std::fs::rename(&tmp, dir.join(name))
}

pub fn read_recovery(loc: &Location, name: &str) -> Option<String> {
    let p = loc.file(RECOVERY_DIR)?.join(name);
    std::fs::read_to_string(p).ok()
}

/// Removes recovery files not referenced by `keep`.
pub fn prune_recovery(loc: &Location, keep: &[String]) {
    let Some(dir) = loc.file(RECOVERY_DIR) else { return };
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !keep.contains(&name) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

// ---------------------------------------------------------------- local history

fn path_hash(p: &Path) -> String {
    let s = p.display().to_string().to_lowercase();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Copies the current on-disk version of `file` into the history folder
/// before it is overwritten, keeping the newest `keep` snapshots.
pub fn snapshot_before_save(loc: &Location, file: &Path, stamp: &str, keep: usize) -> std::io::Result<()> {
    if !file.exists() {
        return Ok(());
    }
    let dir = loc.file(HISTORY_DIR).ok_or_else(|| std::io::Error::other("no data folder"))?.join(path_hash(file));
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("path.txt"), file.display().to_string())?;
    let ext = file.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    std::fs::copy(file, dir.join(format!("{stamp}{ext}")))?;
    let mut snaps = list_snapshots(loc, file);
    while snaps.len() > keep {
        if let Some((p, _)) = snaps.pop() {
            let _ = std::fs::remove_file(p);
        }
    }
    Ok(())
}

/// Snapshots of `file`, newest first: (path, timestamp label).
pub fn list_snapshots(loc: &Location, file: &Path) -> Vec<(PathBuf, String)> {
    let Some(dir) = loc.file(HISTORY_DIR).map(|d| d.join(path_hash(file))) else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut v: Vec<(PathBuf, String)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n != "path.txt"))
        .map(|p| {
            let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            (p, stem)
        })
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

// ---------------------------------------------------------------- snippets

/// Parses `## name` blocks.
pub fn parse_snippets(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            out.push((name.trim().to_string(), String::new()));
        } else if let Some((_, body)) = out.last_mut() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(line);
        }
    }
    for (_, body) in out.iter_mut() {
        while body.ends_with('\n') {
            body.pop();
        }
    }
    out.retain(|(n, _)| !n.is_empty());
    out
}

pub const SNIPPETS_TEMPLATE: &str = "# rpad snippets. Each snippet starts with a line \"## name\";\n# the following lines are inserted as-is.\n\n## Example: TODO comment\nTODO: \n\n## Example: MIT license header\nSPDX-License-Identifier: MIT\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_round_trip() {
        let tabs = vec![
            SessionTab { path: Some("C:\\x\\a.rs".into()), recovery: None, title: String::new(), enc: Enc::Utf8Bom, eol: Eol::Lf, lang: Some(Lang::Rust), caret: 42, top: 3, active: true },
            SessionTab { path: None, recovery: Some("1-2.txt".into()), title: "Untitled 1".into(), enc: Enc::Utf8, eol: Eol::CrLf, lang: None, caret: 0, top: 0, active: false },
        ];
        assert_eq!(parse(&serialize(&tabs)), tabs);
        assert!(parse("tab=garbage\nnot a tab\n").is_empty());
    }

    #[test]
    fn recovery_and_history_files() {
        let dir = std::env::temp_dir().join(format!("rpad-sess-{}", std::process::id()));
        let loc = Location::at(crate::storage::PLACE, dir.clone());
        let rec = loc.file(RECOVERY_DIR).unwrap();
        write_recovery(&rec, "a.txt", Rope::from_str("unsaved ✓")).unwrap();
        write_recovery(&rec, "b.txt", Rope::from_str("x")).unwrap();
        assert_eq!(read_recovery(&loc, "a.txt").as_deref(), Some("unsaved ✓"));
        prune_recovery(&loc, &["a.txt".to_string()]);
        assert!(read_recovery(&loc, "b.txt").is_none());
        let f = dir.join("doc.txt");
        std::fs::write(&f, "v1").unwrap();
        for (i, v) in ["v2", "v3", "v4"].iter().enumerate() {
            snapshot_before_save(&loc, &f, &format!("2026-01-0{}_00-00-00", i + 1), 2).unwrap();
            std::fs::write(&f, v).unwrap();
        }
        let snaps = list_snapshots(&loc, &f);
        assert_eq!(snaps.len(), 2);
        assert_eq!(std::fs::read_to_string(&snaps[0].0).unwrap(), "v3");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn snippets() {
        let s = parse_snippets(SNIPPETS_TEMPLATE);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], ("Example: TODO comment".to_string(), "TODO: ".to_string()));
    }
}
