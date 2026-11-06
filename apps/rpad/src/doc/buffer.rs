//! Text buffer: a rope, a set of selections (multi-caret) and an undo
//! history. All positions are char indices into the rope. Line endings are
//! always `\n` internally; the file layer converts on load/save.
//!
//! Every modification goes through [`Buffer::apply`], which records an undo
//! transaction, so all editing commands and text tools are undoable.

use ropey::Rope;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sel {
    pub anchor: usize,
    pub head: usize,
}

impl Sel {
    pub fn caret(p: usize) -> Sel {
        Sel { anchor: p, head: p }
    }
    pub fn new(anchor: usize, head: usize) -> Sel {
        Sel { anchor, head }
    }
    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }
    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// Replace chars `start..end` with `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

impl Edit {
    pub fn new(start: usize, end: usize, text: impl Into<String>) -> Edit {
        Edit { start, end, text: text.into() }
    }
    fn inserted_len(&self) -> usize {
        self.text.chars().count()
    }
}

/// How selections are set after an edit.
pub enum After {
    /// Map the current selections through the edits.
    Map,
    /// Explicit selections in post-edit coordinates.
    Sels(Vec<Sel>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Typing: consecutive inserts merge into one undo step.
    Insert,
    /// Backspace/Delete: consecutive deletes merge.
    Delete,
    /// Anything else: always a separate undo step.
    Other,
}

#[derive(Clone, Debug)]
struct Change {
    start: usize,
    removed: String,
    inserted: String,
}

#[derive(Clone, Debug)]
struct Txn {
    changes: Vec<Change>,
    before: Vec<Sel>,
    after: Vec<Sel>,
    kind: Kind,
    at: Instant,
}

#[derive(Default, Debug)]
struct History {
    done: Vec<Txn>,
    undone: Vec<Txn>,
    /// `done.len()` when the text matched the file on disk; `None` if that
    /// state is no longer reachable through undo/redo.
    save_point: Option<usize>,
    /// Prevent merging into the previous step (e.g. after the caret moved).
    sealed: bool,
}

const COALESCE: Duration = Duration::from_millis(1500);

pub struct Buffer {
    pub text: Rope,
    pub sels: Vec<Sel>,
    /// Index of the primary selection (the one the view follows).
    pub primary: usize,
    hist: History,
    /// Increments on every text change (for caches).
    pub version: u64,
    /// Lowest line changed since the last call to [`Buffer::take_dirty`].
    dirty_line: Option<usize>,
}

impl Default for Buffer {
    fn default() -> Self {
        Buffer::new("")
    }
}

impl Buffer {
    pub fn new(text: &str) -> Buffer {
        Buffer {
            text: Rope::from_str(text),
            sels: vec![Sel::caret(0)],
            primary: 0,
            hist: History { save_point: Some(0), ..History::default() },
            version: 0,
            dirty_line: Some(0),
        }
    }

    pub fn len(&self) -> usize {
        self.text.len_chars()
    }

    pub fn is_empty(&self) -> bool {
        self.text.len_chars() == 0
    }

    pub fn line_count(&self) -> usize {
        self.text.len_lines()
    }

    pub fn line_of(&self, pos: usize) -> usize {
        self.text.char_to_line(pos.min(self.len()))
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.text.line_to_char(line.min(self.line_count()))
    }

    /// End of the line's text (before its `\n`).
    pub fn line_end(&self, line: usize) -> usize {
        if line + 1 >= self.line_count() {
            self.len()
        } else {
            self.line_start(line + 1) - 1
        }
    }

    /// Line text without the trailing newline.
    pub fn line_text(&self, line: usize) -> String {
        self.text.slice(self.line_start(line)..self.line_end(line)).to_string()
    }

    pub fn slice(&self, start: usize, end: usize) -> String {
        self.text.slice(start..end).to_string()
    }

    pub fn char_at(&self, pos: usize) -> Option<char> {
        (pos < self.len()).then(|| self.text.char(pos))
    }

    pub fn primary_sel(&self) -> Sel {
        self.sels[self.primary.min(self.sels.len() - 1)]
    }

    pub fn is_modified(&self) -> bool {
        self.hist.save_point != Some(self.hist.done.len())
    }

    pub fn mark_saved(&mut self) {
        self.hist.save_point = Some(self.hist.done.len());
        self.hist.sealed = true;
    }

    /// Marks the buffer as different from the file (e.g. recovered text).
    pub fn mark_unsaved(&mut self) {
        self.hist.save_point = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.hist.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.hist.undone.is_empty()
    }

    /// Ends the current typing run so the next edit starts a new undo step.
    pub fn seal(&mut self) {
        self.hist.sealed = true;
    }

    /// Lowest changed line since the last call (for incremental caches).
    pub fn take_dirty(&mut self) -> Option<usize> {
        self.dirty_line.take()
    }

    fn touch(&mut self, pos: usize) {
        let line = self.line_of(pos.min(self.len()));
        self.dirty_line = Some(self.dirty_line.map_or(line, |d| d.min(line)));
        self.version += 1;
    }

    /// Sets selections (sorted, overlapping ones merged) and ends the
    /// current typing run.
    pub fn set_sels(&mut self, sels: Vec<Sel>, primary: usize) {
        self.store_sels(sels, primary);
        self.hist.sealed = true;
    }

    fn store_sels(&mut self, sels: Vec<Sel>, primary: usize) {
        let len = self.len();
        let prim = sels.get(primary).copied();
        let mut v: Vec<Sel> = sels
            .into_iter()
            .map(|s| Sel::new(s.anchor.min(len), s.head.min(len)))
            .collect();
        if v.is_empty() {
            v.push(Sel::caret(0));
        }
        v.sort_by_key(|s| (s.start(), s.end()));
        let mut merged: Vec<Sel> = Vec::with_capacity(v.len());
        for s in v {
            if let Some(last) = merged.last_mut() {
                if s.start() < last.end() || (s.start() == last.start() && s.end() == last.end()) {
                    let start = last.start().min(s.start());
                    let end = last.end().max(s.end());
                    let forward = last.head >= last.anchor;
                    *last = if forward { Sel::new(start, end) } else { Sel::new(end, start) };
                    continue;
                }
            }
            merged.push(s);
        }
        self.primary = prim
            .and_then(|p| merged.iter().position(|s| s.start() <= p.head && p.head <= s.end()))
            .unwrap_or(merged.len() - 1);
        self.sels = merged;
    }

    pub fn set_caret(&mut self, pos: usize) {
        self.set_sels(vec![Sel::caret(pos)], 0);
    }

    /// Applies non-overlapping edits atomically as one undo step.
    pub fn apply(&mut self, mut edits: Vec<Edit>, after: After, kind: Kind) {
        let len = self.len();
        for e in edits.iter_mut() {
            e.end = e.end.min(len);
            e.start = e.start.min(e.end);
        }
        edits.sort_by_key(|e| (e.start, e.end));
        edits.retain(|e| !(e.start == e.end && e.text.is_empty()));
        // Overlapping edits would corrupt offsets; merge them defensively.
        let mut clean: Vec<Edit> = Vec::with_capacity(edits.len());
        for e in edits {
            if let Some(last) = clean.last_mut() {
                if e.start < last.end {
                    last.end = last.end.max(e.end);
                    last.text.push_str(&e.text);
                    continue;
                }
            }
            clean.push(e);
        }
        let edits = clean;
        let before = self.sels.clone();
        let new_sels = match after {
            After::Map => self.sels.iter().map(|s| Sel::new(map_pos(s.anchor, &edits), map_pos(s.head, &edits))).collect(),
            After::Sels(v) => v,
        };
        if edits.is_empty() {
            let p = self.primary;
            self.set_sels(new_sels, p);
            return;
        }
        let mut changes = Vec::with_capacity(edits.len());
        for e in edits.iter().rev() {
            let removed = self.text.slice(e.start..e.end).to_string();
            if e.end > e.start {
                self.text.remove(e.start..e.end);
            }
            if !e.text.is_empty() {
                self.text.insert(e.start, &e.text);
            }
            changes.push(Change { start: e.start, removed, inserted: e.text.clone() });
        }
        self.touch(edits[0].start);
        let p = self.primary;
        self.store_sels(new_sels, p);
        self.record(Txn { changes, before, after: self.sels.clone(), kind, at: Instant::now() });
    }

    fn record(&mut self, txn: Txn) {
        let h = &mut self.hist;
        h.undone.clear();
        if h.save_point.is_some_and(|sp| sp > h.done.len()) {
            h.save_point = None;
        }
        let mergeable = !h.sealed
            && txn.kind != Kind::Other
            && h.save_point != Some(h.done.len())
            && h.done.last().is_some_and(|last| {
                last.kind == txn.kind
                    && txn.at.duration_since(last.at) < COALESCE
                    && last.after == txn.before
                    && !txn.changes.iter().any(|c| c.inserted.contains('\n'))
            });
        if mergeable {
            let last = h.done.last_mut().expect("checked");
            last.changes.extend(txn.changes);
            last.after = txn.after;
            last.at = txn.at;
        } else {
            h.done.push(txn);
        }
        h.sealed = false;
    }

    pub fn undo(&mut self) -> bool {
        let Some(txn) = self.hist.done.pop() else { return false };
        let mut low = usize::MAX;
        for c in txn.changes.iter().rev() {
            let n = c.inserted.chars().count();
            if n > 0 {
                self.text.remove(c.start..c.start + n);
            }
            if !c.removed.is_empty() {
                self.text.insert(c.start, &c.removed);
            }
            low = low.min(c.start);
        }
        self.touch(low);
        self.sels = txn.before.clone();
        self.primary = self.primary.min(self.sels.len() - 1);
        self.hist.undone.push(txn);
        self.hist.sealed = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(txn) = self.hist.undone.pop() else { return false };
        let mut low = usize::MAX;
        for c in txn.changes.iter() {
            let n = c.removed.chars().count();
            if n > 0 {
                self.text.remove(c.start..c.start + n);
            }
            if !c.inserted.is_empty() {
                self.text.insert(c.start, &c.inserted);
            }
            low = low.min(c.start);
        }
        self.touch(low);
        self.sels = txn.after.clone();
        self.primary = self.primary.min(self.sels.len() - 1);
        self.hist.done.push(txn);
        self.hist.sealed = true;
        true
    }

    /// Replaces the whole text as one undoable step (e.g. reload from disk).
    pub fn replace_all_text(&mut self, text: &str) {
        let caret = self.primary_sel().head;
        let len = self.len();
        self.apply(vec![Edit::new(0, len, text)], After::Sels(vec![Sel::caret(caret)]), Kind::Other);
        let n = self.len();
        self.sels = vec![Sel::caret(caret.min(n))];
        self.primary = 0;
    }
}

/// Maps a position through sorted, non-overlapping edits. Positions inside
/// a replaced range move to the end of the inserted text.
pub fn map_pos(pos: usize, edits: &[Edit]) -> usize {
    let mut delta: isize = 0;
    for e in edits {
        if pos < e.start {
            break;
        }
        if pos >= e.end {
            delta += e.inserted_len() as isize - (e.end - e.start) as isize;
        } else {
            return (e.start as isize + delta) as usize + e.inserted_len();
        }
    }
    (pos as isize + delta).max(0) as usize
}

/// Like [`map_pos`], but a position exactly at an insertion point stays
/// before the inserted text.
pub fn map_pos_left(pos: usize, edits: &[Edit]) -> usize {
    let mut delta: isize = 0;
    for e in edits {
        if pos < e.start || (pos == e.start && e.start == e.end) {
            break;
        }
        if pos >= e.end {
            delta += e.inserted_len() as isize - (e.end - e.start) as isize;
        } else {
            return (e.start as isize + delta) as usize + e.inserted_len();
        }
    }
    (pos as isize + delta).max(0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_and_undo_redo() {
        let mut b = Buffer::new("hello world");
        assert!(!b.is_modified());
        b.set_caret(5);
        b.apply(vec![Edit::new(5, 5, ",")], After::Map, Kind::Insert);
        assert_eq!(b.text.to_string(), "hello, world");
        assert_eq!(b.sels, vec![Sel::caret(6)]);
        assert!(b.is_modified());
        assert!(b.undo());
        assert_eq!(b.text.to_string(), "hello world");
        assert!(!b.is_modified());
        assert!(b.redo());
        assert_eq!(b.text.to_string(), "hello, world");
        assert!(b.is_modified());
    }

    #[test]
    fn typing_coalesces_but_newline_breaks() {
        let mut b = Buffer::new("");
        for c in ["a", "b", "c"] {
            b.apply(vec![Edit::new(b.sels[0].head, b.sels[0].head, c)], After::Map, Kind::Insert);
        }
        b.apply(vec![Edit::new(3, 3, "\n")], After::Map, Kind::Insert);
        assert_eq!(b.text.to_string(), "abc\n");
        b.undo();
        assert_eq!(b.text.to_string(), "abc");
        b.undo();
        assert_eq!(b.text.to_string(), "");
        assert!(!b.can_undo());
    }

    #[test]
    fn save_point_is_respected_across_undo_branches() {
        let mut b = Buffer::new("x");
        b.apply(vec![Edit::new(1, 1, "y")], After::Map, Kind::Insert);
        b.mark_saved();
        assert!(!b.is_modified());
        // typing after save starts a new step (no merge into the saved one)
        b.apply(vec![Edit::new(2, 2, "z")], After::Map, Kind::Insert);
        assert!(b.is_modified());
        b.undo();
        assert!(!b.is_modified());
        b.undo();
        assert!(b.is_modified());
        // new edit after undoing past the save point makes it unreachable
        b.apply(vec![Edit::new(0, 0, "q")], After::Map, Kind::Other);
        b.undo();
        assert!(b.is_modified());
    }

    #[test]
    fn multi_edit_maps_carets() {
        let mut b = Buffer::new("a\nb\nc");
        b.set_sels(vec![Sel::caret(0), Sel::caret(2), Sel::caret(4)], 0);
        let edits = b.sels.iter().map(|s| Edit::new(s.head, s.head, "> ")).collect();
        b.apply(edits, After::Map, Kind::Insert);
        assert_eq!(b.text.to_string(), "> a\n> b\n> c");
        assert_eq!(b.sels, vec![Sel::caret(2), Sel::caret(6), Sel::caret(10)]);
        b.undo();
        assert_eq!(b.text.to_string(), "a\nb\nc");
        assert_eq!(b.sels.len(), 3);
    }

    #[test]
    fn selections_merge_and_clamp() {
        let mut b = Buffer::new("abcdef");
        b.set_sels(vec![Sel::new(0, 3), Sel::new(2, 5), Sel::caret(99)], 0);
        assert_eq!(b.sels, vec![Sel::new(0, 5), Sel::caret(6)]);
    }

    #[test]
    fn map_positions() {
        let edits = vec![Edit::new(2, 4, "XYZ"), Edit::new(6, 6, "!")];
        assert_eq!(map_pos(0, &edits), 0);
        assert_eq!(map_pos(3, &edits), 5);
        assert_eq!(map_pos(4, &edits), 5);
        assert_eq!(map_pos(6, &edits), 8);
        assert_eq!(map_pos(7, &edits), 9);
    }

    #[test]
    fn lines() {
        let b = Buffer::new("ab\ncd\n");
        assert_eq!(b.line_count(), 3);
        assert_eq!(b.line_text(1), "cd");
        assert_eq!(b.line_end(0), 2);
        assert_eq!(b.line_text(2), "");
        assert_eq!(b.line_of(3), 1);
    }
}
