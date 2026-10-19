//! An open document: buffer plus file metadata, view state and caches.

use crate::i18n::tf;
use crate::doc::Buffer;
use crate::io::encoding::{Enc, Eol};
use crate::io::file::{self, Loaded, Stamp, LARGE_FILE_SIZE};
use crate::syntax::{self, Highlighter, Lang};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A message bar shown above the editor for this document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// The file changed on disk; offer Reload / Keep.
    ChangedOnDisk,
    /// The file was deleted or renamed on disk.
    Deleted,
    Info(Msg),
    /// Mixed line endings were found (cleared when the user picks one).
    MixedEol(Msg),
    Warning(Msg),
}

/// A message kept as its English template and arguments, so it is shown
/// in the current interface language even after the language changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Msg {
    pub key: &'static str,
    pub args: Vec<String>,
}

impl Msg {
    pub fn new(key: &'static str) -> Msg {
        Msg { key, args: Vec::new() }
    }

    pub fn with(key: &'static str, args: &[&str]) -> Msg {
        Msg { key, args: args.iter().map(|a| a.to_string()).collect() }
    }

    pub fn text(&self) -> String {
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        tf(self.key, &args)
    }
}

pub struct Doc {
    pub id: u64,
    pub buf: Buffer,
    pub path: Option<PathBuf>,
    /// Title for untitled documents ("Untitled 2") or snapshots.
    pub title: String,
    pub enc: Enc,
    pub eol: Eol,
    pub lang: Lang,
    /// The user picked the language explicitly.
    pub lang_override: bool,
    /// The file on disk is read-only.
    pub read_only: bool,
    /// The document must not be edited (binary file, history snapshot).
    pub view_only: bool,
    pub stamp: Option<Stamp>,
    /// Decoding replaced invalid bytes with U+FFFD.
    pub lossy: bool,
    pub large: bool,
    pub hl: Highlighter,
    pub folds: Vec<(usize, usize)>,
    pub folded: BTreeSet<usize>,
    pub folds_version: u64,
    pub outline_version: u64,
    pub outline: Vec<syntax::outline::Symbol>,
    /// First visible line and wrapped row within it.
    pub top: usize,
    pub top_row: usize,
    pub scroll_x: f32,
    /// Desired cell column per selection for vertical movement.
    pub goals: Vec<Option<usize>>,
    pub notice: Option<Notice>,
    /// Text version last written to the recovery folder.
    pub recovery_version: Option<u64>,
}

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl Doc {
    pub fn untitled(n: usize, enc: Enc, eol: Eol) -> Doc {
        Doc::with_buffer(Buffer::new(""), None, format!("Untitled {n}"), enc, eol, Lang::Plain)
    }

    pub fn with_buffer(buf: Buffer, path: Option<PathBuf>, title: String, enc: Enc, eol: Eol, lang: Lang) -> Doc {
        Doc {
            id: next_id(),
            buf,
            path,
            title,
            enc,
            eol,
            lang,
            lang_override: false,
            read_only: false,
            view_only: false,
            stamp: None,
            lossy: false,
            large: false,
            hl: Highlighter::new(lang),
            folds: Vec::new(),
            folded: BTreeSet::new(),
            folds_version: u64::MAX,
            outline_version: u64::MAX,
            outline: Vec::new(),
            top: 0,
            top_row: 0,
            scroll_x: 0.0,
            goals: Vec::new(),
            notice: None,
            recovery_version: None,
        }
    }

    /// Builds a document from a loaded file, with notices for anything the
    /// user should know before editing.
    pub fn from_loaded(path: &Path, l: Loaded) -> Doc {
        let d = l.decoded;
        let first_line = d.text.lines().next().unwrap_or("").to_string();
        let lang = syntax::detect(Some(path), &first_line);
        let large = d.text.len() as u64 > LARGE_FILE_SIZE;
        let mixed = d.mixed_eol();
        let counts = d.eol_counts;
        let mut doc = Doc::with_buffer(Buffer::new(&d.text), Some(path.to_path_buf()), String::new(), d.enc, d.eol, lang);
        doc.stamp = l.stamp;
        doc.read_only = l.read_only;
        doc.lossy = d.lossy;
        doc.large = large;
        doc.view_only = d.binary;
        if large {
            doc.hl.enabled = false;
        }
        doc.notice = if d.binary {
            Some(Notice::Warning(Msg::new("This file contains binary data (NUL characters). It is open read-only to avoid damaging it.")))
        } else if d.lossy {
            Some(Notice::Warning(Msg::with(
                "Some bytes are not valid {}. They are shown as \u{FFFD} and would be saved as \u{FFFD}. Use Format \u{203A} Reopen with Encoding to choose the correct encoding.",
                &[&d.enc.name()],
            )))
        } else if mixed {
            Some(Notice::MixedEol(Msg::with(
                "Mixed line endings (LF: {}, CRLF: {}, CR: {}). They will be saved as {}. Use Format \u{203A} Line Endings to choose.",
                &[&counts[0].to_string(), &counts[1].to_string(), &counts[2].to_string(), d.eol.name()],
            )))
        } else if large {
            Some(Notice::Info(Msg::new("Large file: syntax highlighting, folding and word wrap are turned off for speed.")))
        } else if l.read_only {
            Some(Notice::Info(Msg::new("This file is read-only on disk. Use Save As to save a copy.")))
        } else {
            None
        };
        doc
    }

    pub fn display_name(&self) -> String {
        match &self.path {
            Some(p) if self.title.is_empty() => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string()),
            // Untitled tabs keep an internal English title (stable in the
            // session file) and are shown in the current language.
            None => match self.title.strip_prefix("Untitled ").filter(|n| n.parse::<usize>().is_ok()) {
                Some(n) => tf("Untitled {}", &[n]),
                None => self.title.clone(),
            },
            _ => self.title.clone(),
        }
    }

    pub fn is_modified(&self) -> bool {
        self.buf.is_modified()
    }

    /// Empty untitled document that was never edited (can be replaced).
    pub fn is_pristine(&self) -> bool {
        self.path.is_none() && !self.is_modified() && self.buf.is_empty() && !self.view_only
    }

    pub fn editable(&self) -> bool {
        !self.view_only
    }

    pub fn set_lang(&mut self, lang: Lang, explicit: bool) {
        self.lang = lang;
        self.lang_override = explicit;
        self.hl.set_lang(lang);
        self.folds.clear();
        self.folded.clear();
        self.folds_version = u64::MAX;
        self.outline_version = u64::MAX;
    }

    /// Saves to `path` with the document's encoding and line endings.
    pub fn save_to(&mut self, path: &Path) -> Result<(), file::SaveError> {
        let stamp = file::save(path, &self.buf.text, self.enc, self.eol)?;
        if self.path.as_deref() != Some(path) {
            self.path = Some(path.to_path_buf());
            self.title.clear();
            if !self.lang_override {
                let first = self.buf.line_text(0);
                let lang = syntax::detect(Some(path), &first);
                self.set_lang(lang, false);
            }
        }
        self.stamp = stamp;
        self.read_only = false;
        self.buf.mark_saved();
        self.lossy = false;
        if matches!(self.notice, Some(Notice::ChangedOnDisk | Notice::Deleted)) {
            self.notice = None;
        }
        Ok(())
    }

    /// True if the file on disk differs from what we loaded or saved.
    pub fn changed_on_disk(&self) -> Option<bool> {
        let p = self.path.as_ref()?;
        match file::stamp(p) {
            None => Some(true),
            Some(s) => Some(Some(s) != self.stamp),
        }
    }

    // ------------------------------------------------------------ folding

    pub fn is_hidden(&self, line: usize) -> bool {
        self.folded.iter().any(|&s| self.fold_end(s).is_some_and(|e| line > s && line <= e))
    }

    pub fn fold_end(&self, start: usize) -> Option<usize> {
        self.folds.iter().find(|(s, _)| *s == start).map(|(_, e)| *e)
    }

    /// Next line after `line` that is not hidden by a fold.
    pub fn next_visible(&self, line: usize) -> usize {
        let mut l = line;
        if self.folded.contains(&l) {
            if let Some(e) = self.fold_end(l) {
                l = e;
            }
        }
        let mut n = l + 1;
        while n < self.buf.line_count() && self.is_hidden(n) {
            n += 1;
        }
        n
    }

    /// Previous line before `line` that is not hidden.
    pub fn prev_visible(&self, line: usize) -> Option<usize> {
        let mut n = line.checked_sub(1)?;
        while self.is_hidden(n) {
            n = n.checked_sub(1)?;
        }
        Some(n)
    }

    /// Unfolds any fold hiding `line`.
    pub fn reveal(&mut self, line: usize) {
        let hide: Vec<usize> = self.folded.iter().copied().filter(|&s| self.fold_end(s).is_some_and(|e| line > s && line <= e)).collect();
        for s in hide {
            self.folded.remove(&s);
        }
    }

    pub fn toggle_fold(&mut self, line: usize) -> bool {
        if self.fold_end(line).is_none() {
            return false;
        }
        if !self.folded.remove(&line) {
            self.folded.insert(line);
        }
        true
    }

    /// Innermost fold range containing `line`.
    pub fn fold_containing(&self, line: usize) -> Option<(usize, usize)> {
        self.folds.iter().filter(|(s, e)| *s <= line && line <= *e).min_by_key(|(s, e)| e - s).copied()
    }

    /// Keeps folds consistent after an edit changed the line count by
    /// `delta` starting at line `from`.
    pub fn shift_folds(&mut self, from: usize, delta: isize) {
        if delta == 0 {
            return;
        }
        let moved: BTreeSet<usize> = self
            .folded
            .iter()
            .filter_map(|&s| {
                if s < from {
                    Some(s)
                } else {
                    let n = s as isize + delta;
                    (n > from as isize || s > from).then_some(n.max(0) as usize)
                }
            })
            .collect();
        self.folded = moved;
        for f in self.folds.iter_mut() {
            if f.0 > from {
                f.0 = (f.0 as isize + delta).max(0) as usize;
            }
            if f.1 >= from {
                f.1 = (f.1 as isize + delta).max(f.0 as isize) as usize;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_navigation() {
        let mut d = Doc::untitled(1, Enc::Utf8, Eol::Lf);
        d.buf = Buffer::new("a\nb\nc\nd\ne");
        d.folds = vec![(0, 2), (3, 4)];
        assert!(d.toggle_fold(0));
        assert!(d.is_hidden(1) && d.is_hidden(2) && !d.is_hidden(3));
        assert_eq!(d.next_visible(0), 3);
        assert_eq!(d.prev_visible(3), Some(0));
        d.reveal(2);
        assert!(!d.is_hidden(2));
        assert!(!d.toggle_fold(1));
        assert_eq!(d.fold_containing(4), Some((3, 4)));
    }

    #[test]
    fn from_loaded_notices() {
        let dir = std::env::temp_dir().join(format!("rpad-doc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("mixed.rs");
        std::fs::write(&p, b"fn a() {}\r\nfn b() {}\n").unwrap();
        let d = Doc::from_loaded(&p, file::load(&p, None).unwrap());
        assert_eq!(d.lang, Lang::Rust);
        assert!(matches!(d.notice, Some(Notice::MixedEol(ref m)) if m.key.contains("Mixed")));
        let p2 = dir.join("bin.dat");
        std::fs::write(&p2, b"ab\0cd").unwrap();
        let d = Doc::from_loaded(&p2, file::load(&p2, None).unwrap());
        assert!(d.view_only && !d.editable());
        let _ = std::fs::remove_dir_all(dir);
    }
}
