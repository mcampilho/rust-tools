//! Find and replace in the active document.

use super::{App, Focus, Host};
use crate::doc::{After, Edit, Kind, Sel};
use crate::search::{self, Query};
use crate::ui::lineedit::LineEdit;
use std::sync::{mpsc, Arc};

/// Documents larger than this (bytes) are counted on a background thread.
const BACKGROUND_SIZE: usize = 4 * 1024 * 1024;
/// Matches kept for highlighting.
const MAX_HIGHLIGHTS: usize = 100_000;
const MAX_COUNT: usize = 10_000_000;

type MatchKey = (u64, u64, Query, Option<(usize, usize)>);
/// Result of a background match count: key, capped matches, total.
type CountResult = (MatchKey, Vec<(usize, usize)>, usize);

#[derive(Default)]
pub struct FindState {
    pub visible: bool,
    pub replace_mode: bool,
    pub find: LineEdit,
    pub replace: LineEdit,
    pub case: bool,
    pub word: bool,
    pub regex: bool,
    pub in_selection: bool,
    /// Char range the search is limited to (In selection).
    pub scope: Option<(usize, usize)>,
    pub error: Option<String>,
    /// Char ranges of matches (capped) and the total count.
    pub matches: Vec<(usize, usize)>,
    pub total: usize,
    pub counting: bool,
    key: Option<MatchKey>,
    rx: Option<mpsc::Receiver<CountResult>>,
    text: Option<(u64, u64, Arc<String>)>,
}

impl FindState {
    pub fn query(&self) -> Query {
        self.query_with(&self.find.text)
    }

    pub fn query_with(&self, text: &str) -> Query {
        Query { text: text.to_string(), case: self.case, word: self.word, regex: self.regex }
    }

    pub fn invalidate(&mut self) {
        self.key = None;
    }

    /// Index of the match equal to `sel` (for "3 of 12").
    pub fn current_index(&self, sel: (usize, usize)) -> Option<usize> {
        self.matches.binary_search_by(|m| m.0.cmp(&sel.0)).ok().filter(|&i| self.matches[i] == sel)
    }
}

impl App {
    /// Text of the active document as a `String`, cached per version.
    fn search_text(&mut self) -> Arc<String> {
        let d = &self.docs[self.active];
        if let Some((id, v, t)) = &self.find.text {
            if *id == d.id && *v == d.buf.version {
                return t.clone();
            }
        }
        let t = Arc::new(d.buf.text.to_string());
        self.find.text = Some((d.id, d.buf.version, t.clone()));
        t
    }

    pub fn open_find(&mut self, replace: bool) {
        let f = &mut self.find;
        f.visible = true;
        f.replace_mode = replace;
        let d = &self.docs[self.active];
        let sel = d.buf.primary_sel();
        if !sel.is_empty() {
            let t = d.buf.slice(sel.start(), sel.end());
            if t.contains('\n') {
                // A multi-line selection becomes the search scope.
                self.find.in_selection = true;
                self.find.scope = Some((sel.start(), sel.end()));
            } else {
                self.find.find.set(&t);
                self.find.in_selection = false;
                self.find.scope = None;
            }
        }
        self.find.find.select_all();
        self.focus = if replace && !self.find.find.is_empty() && self.focus == Focus::Find { Focus::Replace } else { Focus::Find };
        self.find.invalidate();
    }

    pub fn close_find(&mut self) {
        self.find.visible = false;
        self.find.matches.clear();
        self.find.total = 0;
        self.find.scope = None;
        self.find.in_selection = false;
        self.focus = Focus::Editor;
    }

    pub fn toggle_in_selection(&mut self) {
        self.find.in_selection = !self.find.in_selection;
        self.find.scope = if self.find.in_selection {
            let s = self.doc().buf.primary_sel();
            (!s.is_empty()).then(|| (s.start(), s.end()))
        } else {
            None
        };
        if self.find.in_selection && self.find.scope.is_none() {
            self.find.in_selection = false;
            self.notify("Select the text to search in first");
        }
        self.find.invalidate();
    }

    /// Recomputes match highlights if the query, document or scope changed.
    /// Large documents are counted on a background thread.
    pub fn update_matches(&mut self) {
        // Collect background results.
        if let Some(rx) = &self.find.rx {
            if let Ok((key, m, total)) = rx.try_recv() {
                self.find.rx = None;
                self.find.counting = false;
                if self.find.key.as_ref() == Some(&key) {
                    self.find.matches = m;
                    self.find.total = total;
                }
            }
        }
        if !self.find.visible {
            return;
        }
        let q = self.find.query();
        let d = &self.docs[self.active];
        let key = (d.id, d.buf.version, q.clone(), self.find.scope);
        if self.find.key.as_ref() == Some(&key) {
            return;
        }
        self.find.key = Some(key.clone());
        self.find.error = None;
        self.find.matches.clear();
        self.find.total = 0;
        if q.is_empty() {
            return;
        }
        let re = match q.build() {
            Ok(r) => r,
            Err(e) => {
                self.find.error = Some(e);
                return;
            }
        };
        let text = self.search_text();
        let rope = self.docs[self.active].buf.text.clone();
        let scope = self.find.scope.map(|(a, b)| (rope.char_to_byte(a.min(rope.len_chars())), rope.char_to_byte(b.min(rope.len_chars()))));
        let compute = move |text: &str| {
            let (lo, hi) = scope.unwrap_or((0, text.len()));
            let (m, total) = search::find_all(&text[..hi], &re, MAX_HIGHLIGHTS, MAX_COUNT);
            let m: Vec<(usize, usize)> = m.into_iter().filter(|r| r.0 >= lo).collect();
            let total = if scope.is_some() { m.len() } else { total };
            let chars = m.iter().map(|&(a, b)| (rope.byte_to_char(a), rope.byte_to_char(b))).collect::<Vec<_>>();
            (chars, total)
        };
        if text.len() <= BACKGROUND_SIZE {
            let (m, t) = compute(&text);
            self.find.matches = m;
            self.find.total = t;
        } else {
            let (tx, rx) = mpsc::channel();
            let waker = self.waker.clone();
            self.find.counting = true;
            self.find.rx = Some(rx);
            std::thread::spawn(move || {
                let (m, t) = compute(&text);
                let _ = tx.send((key, m, t));
                if let Some(w) = waker {
                    w();
                }
            });
        }
    }

    /// Find next/previous from the primary selection, wrapping around.
    pub fn find_step(&mut self, forward: bool, host: &dyn Host) {
        let q = self.find.query();
        if q.is_empty() {
            self.open_find(false);
            return;
        }
        let re = match q.build() {
            Ok(r) => r,
            Err(e) => {
                self.find.error = Some(e.clone());
                host.error("Find", &e);
                return;
            }
        };
        super::settings::Settings::push_history(&mut self.settings.find_history, &q.text);
        let text = self.search_text();
        let rope = self.docs[self.active].buf.text.clone();
        let sel = self.doc().buf.primary_sel();
        let from = rope.char_to_byte(if forward { sel.end() } else { sel.start() });
        let scope = self.find.scope.map(|(a, b)| (rope.char_to_byte(a.min(rope.len_chars())), rope.char_to_byte(b.min(rope.len_chars()))));
        let (lo, hi) = scope.unwrap_or((0, text.len()));
        let hay = &text[..hi];
        let found = search::find_next(hay, &re, from.clamp(lo, hi), forward, true).filter(|m| m.0 >= lo).or_else(|| {
            // Wrap inside the scope.
            if forward {
                search::find_next(hay, &re, lo, true, false).filter(|m| m.0 >= lo)
            } else {
                search::find_next(hay, &re, hi, false, false).filter(|m| m.0 >= lo)
            }
        });
        match found {
            Some((a, b)) => {
                let wrapped = if forward { a < from } else { a >= from };
                let (ca, cb) = (rope.byte_to_char(a), rope.byte_to_char(b));
                let d = self.doc_mut();
                let line = d.buf.line_of(ca);
                d.reveal(line);
                d.buf.set_sels(vec![Sel::new(ca, cb)], 0);
                self.reveal_caret = true;
                if wrapped {
                    self.notify(if forward { "Search wrapped to the beginning" } else { "Search wrapped to the end" });
                }
            }
            None => self.notify(format!("\"{}\" was not found", q.text)),
        }
    }

    /// Replaces the current match (if selected) and moves to the next.
    pub fn replace_one(&mut self, host: &dyn Host) {
        if !self.doc().editable() {
            self.notify("This document is read-only");
            return;
        }
        let q = self.find.query();
        let Ok(re) = q.build() else {
            self.find_step(true, host);
            return;
        };
        super::settings::Settings::push_history(&mut self.settings.replace_history, &self.find.replace.text);
        let text = self.search_text();
        let rope = self.docs[self.active].buf.text.clone();
        let sel = self.doc().buf.primary_sel();
        let (sa, sb) = (rope.char_to_byte(sel.start()), rope.char_to_byte(sel.end()));
        let is_match = re.find_at(&text, sa).is_some_and(|m| m.start() == sa && m.end() == sb) && sa != sb;
        if is_match {
            let new = search::replacement_for(&text, &re, (sa, sb), &self.find.replace.text, q.regex);
            let n = new.chars().count();
            let start = sel.start();
            self.edit(|b| b.apply(vec![Edit::new(start, sel.end(), new)], After::Sels(vec![Sel::caret(start + n)]), Kind::Other));
        }
        self.find_step(true, host);
    }

    /// Replaces every match (in the scope) as one undo step, after showing
    /// a preview and asking for confirmation.
    pub fn replace_all(&mut self, host: &dyn Host) {
        if !self.doc().editable() {
            self.notify("This document is read-only");
            return;
        }
        let q = self.find.query();
        let re = match q.build() {
            Ok(r) => r,
            Err(e) => {
                host.error("Replace", &e);
                return;
            }
        };
        super::settings::Settings::push_history(&mut self.settings.replace_history, &self.find.replace.text);
        let text = self.search_text();
        let rope = self.docs[self.active].buf.text.clone();
        let scope = self.find.scope.map(|(a, b)| (rope.char_to_byte(a.min(rope.len_chars())), rope.char_to_byte(b.min(rope.len_chars()))));
        let range = scope.unwrap_or((0, text.len()));
        let plan = search::plan_replace_all(&text, &re, &self.find.replace.text, q.regex, range);
        if plan.is_empty() {
            self.notify(format!("\"{}\" was not found", q.text));
            return;
        }
        let lines: std::collections::BTreeSet<usize> = plan.iter().map(|p| rope.byte_to_line(p.start)).collect();
        let mut preview = String::new();
        for p in plan.iter().take(6) {
            let line = rope.byte_to_line(p.start);
            let old: String = text[p.start..p.end].chars().take(40).collect();
            let new: String = p.text.chars().take(40).collect();
            preview.push_str(&format!("  line {}: \"{}\" \u{2192} \"{}\"\n", line + 1, old.replace('\n', "\u{21B5}"), new.replace('\n', "\u{21B5}")));
        }
        if plan.len() > 6 {
            preview.push_str(&format!("  … and {} more\n", plan.len() - 6));
        }
        let scope_text = if scope.is_some() { "in the selection" } else { "in this document" };
        let ok = plan.len() == 1
            || host.confirm(
                "Replace all",
                &format!("Replace {} occurrence(s) on {} line(s) {scope_text}?\n\n{preview}\nYou can undo this with Ctrl+Z.", plan.len(), lines.len()),
            );
        if !ok {
            return;
        }
        let edits: Vec<Edit> = plan.iter().map(|p| Edit::new(rope.byte_to_char(p.start), rope.byte_to_char(p.end), p.text.clone())).collect();
        let n = edits.len();
        self.edit(|b| b.apply(edits, After::Map, Kind::Other));
        if scope.is_some() {
            // Keep the scope covering the replaced region.
            self.find.scope = None;
            self.find.in_selection = false;
        }
        self.notify(format!("Replaced {n} occurrence(s)"));
    }
}
