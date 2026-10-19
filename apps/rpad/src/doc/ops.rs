//! Editing commands built on [`Buffer::apply`]. Each command works with
//! every selection (multi-caret) and produces a single undo step.

use super::buffer::{After, Buffer, Edit, Kind, Sel};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndentCfg {
    pub use_tabs: bool,
    pub width: usize,
    pub auto: bool,
}

impl Default for IndentCfg {
    fn default() -> Self {
        IndentCfg { use_tabs: false, width: 4, auto: true }
    }
}

impl IndentCfg {
    pub fn unit(&self) -> String {
        if self.use_tabs {
            "\t".into()
        } else {
            " ".repeat(self.width.max(1))
        }
    }
}

pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Display column of char `idx` in `line` (tabs expand to `tab` stops).
pub fn visual_col(line: &str, idx: usize, tab: usize) -> usize {
    let mut col = 0;
    for c in line.chars().take(idx) {
        col = if c == '\t' { (col / tab + 1) * tab } else { col + 1 };
    }
    col
}

/// Char index in `line` closest to display column `col`.
pub fn idx_at_col(line: &str, col: usize, tab: usize) -> usize {
    let mut c = 0;
    for (i, ch) in line.chars().enumerate() {
        let next = if ch == '\t' { (c / tab + 1) * tab } else { c + 1 };
        if next > col {
            return if col - c > (next - c) / 2 { i + 1 } else { i };
        }
        c = next;
    }
    line.chars().count()
}

fn leading_ws(line: &str) -> &str {
    let n = line.len() - line.trim_start_matches([' ', '\t']).len();
    &line[..n]
}

impl Buffer {
    // ------------------------------------------------------------ queries

    pub fn selected_texts(&self) -> Vec<String> {
        self.sels.iter().map(|s| self.slice(s.start(), s.end())).collect()
    }

    pub fn has_selection(&self) -> bool {
        self.sels.iter().any(|s| !s.is_empty())
    }

    /// Merged, inclusive line ranges covered by the selections. A selection
    /// ending at column 0 of a line does not include that line.
    pub fn sel_line_ranges(&self) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = Vec::new();
        for s in &self.sels {
            let a = self.line_of(s.start());
            let mut b = self.line_of(s.end());
            if b > a && s.end() == self.line_start(b) {
                b -= 1;
            }
            match out.last_mut() {
                Some(last) if a <= last.1 + 1 && a >= last.0 => last.1 = last.1.max(b),
                _ => out.push((a, b)),
            }
        }
        out
    }

    pub fn word_at(&self, pos: usize) -> Option<(usize, usize)> {
        let len = self.len();
        let at = |p: usize| self.char_at(p).is_some_and(is_word_char);
        let p = if at(pos) {
            pos
        } else if pos > 0 && at(pos - 1) {
            pos - 1
        } else {
            return None;
        };
        let mut s = p;
        while s > 0 && at(s - 1) {
            s -= 1;
        }
        let mut e = p;
        while e < len && at(e) {
            e += 1;
        }
        Some((s, e))
    }

    // ------------------------------------------------------------ typing

    /// Inserts `s` at every selection (replacing selected text). In
    /// overwrite mode, an empty selection replaces the following chars on
    /// the same line.
    pub fn insert_text(&mut self, s: &str, overwrite: bool) {
        let n = s.chars().count();
        let edits = self
            .sels
            .iter()
            .map(|sel| {
                let mut end = sel.end();
                if overwrite && sel.is_empty() {
                    let line_end = self.line_end(self.line_of(end));
                    end = (end + n).min(line_end);
                }
                Edit::new(sel.start(), end, s)
            })
            .collect();
        let kind = if s.chars().all(|c| c != '\n') && n == 1 { Kind::Insert } else { Kind::Other };
        self.apply(edits, After::Map, kind);
    }

    /// Inserts one string per selection (multi-caret paste of N lines).
    pub fn insert_per_selection(&mut self, texts: &[String]) {
        let edits = self.sels.iter().zip(texts).map(|(sel, t)| Edit::new(sel.start(), sel.end(), t.clone())).collect();
        self.apply(edits, After::Map, Kind::Other);
    }

    /// Enter: newline keeping the current indentation; `increase` decides
    /// (from the text before the caret) whether to indent one more level.
    pub fn newline(&mut self, cfg: &IndentCfg, increase: &dyn Fn(&str) -> bool, close_pairs: bool) {
        let mut edits = Vec::new();
        let mut carets = Vec::new();
        let mut shift: isize = 0;
        for sel in &self.sels {
            let line = self.line_of(sel.start());
            let ls = self.line_start(line);
            let before: String = self.slice(ls, sel.start());
            let mut text = String::from("\n");
            let mut caret_offset;
            if cfg.auto {
                let indent = leading_ws(&before).to_string();
                let more = increase(&before);
                text.push_str(&indent);
                if more {
                    text.push_str(&cfg.unit());
                }
                caret_offset = text.chars().count();
                // `{|}` + Enter puts the closer on its own line.
                let next = self.char_at(sel.end());
                let prev = before.trim_end().chars().last();
                if close_pairs && more && matches!((prev, next), (Some('{'), Some('}')) | (Some('['), Some(']')) | (Some('('), Some(')'))) {
                    text.push('\n');
                    text.push_str(&indent);
                }
            } else {
                caret_offset = 1;
            }
            if !cfg.auto {
                caret_offset = 1;
            }
            let start = (sel.start() as isize + shift) as usize;
            carets.push(Sel::caret(start + caret_offset));
            shift += text.chars().count() as isize - (sel.end() - sel.start()) as isize;
            edits.push(Edit::new(sel.start(), sel.end(), text));
        }
        self.apply(edits, After::Sels(carets), Kind::Other);
    }

    pub fn backspace(&mut self, cfg: &IndentCfg) {
        let edits = self
            .sels
            .iter()
            .filter_map(|sel| {
                if !sel.is_empty() {
                    return Some(Edit::new(sel.start(), sel.end(), ""));
                }
                let p = sel.head;
                if p == 0 {
                    return None;
                }
                // In leading spaces, delete back to the previous indent stop.
                if !cfg.use_tabs {
                    let ls = self.line_start(self.line_of(p));
                    let before = self.slice(ls, p);
                    if !before.is_empty() && before.chars().all(|c| c == ' ') {
                        let n = before.len();
                        let k = ((n - 1) % cfg.width.max(1)) + 1;
                        return Some(Edit::new(p - k, p, ""));
                    }
                }
                Some(Edit::new(p - 1, p, ""))
            })
            .collect();
        self.apply(edits, After::Map, Kind::Delete);
    }

    pub fn delete_forward(&mut self) {
        let len = self.len();
        let edits = self
            .sels
            .iter()
            .filter_map(|sel| {
                if !sel.is_empty() {
                    Some(Edit::new(sel.start(), sel.end(), ""))
                } else if sel.head < len {
                    Some(Edit::new(sel.head, sel.head + 1, ""))
                } else {
                    None
                }
            })
            .collect();
        self.apply(edits, After::Map, Kind::Delete);
    }

    /// Ctrl+Backspace / Ctrl+Delete.
    pub fn delete_word(&mut self, forward: bool) {
        let edits = self
            .sels
            .iter()
            .map(|sel| {
                if !sel.is_empty() {
                    return Edit::new(sel.start(), sel.end(), "");
                }
                let target = self.word_boundary(sel.head, forward);
                Edit::new(sel.head.min(target), sel.head.max(target), "")
            })
            .collect();
        self.apply(edits, After::Map, Kind::Other);
    }

    /// Next word boundary from `pos` (like Ctrl+Left/Right).
    pub fn word_boundary(&self, pos: usize, forward: bool) -> usize {
        let len = self.len();
        let class = |c: char| {
            if c == '\n' {
                3
            } else if c.is_whitespace() {
                0
            } else if is_word_char(c) {
                1
            } else {
                2
            }
        };
        if forward {
            let mut p = pos;
            if p >= len {
                return len;
            }
            if self.text.char(p) == '\n' {
                return p + 1;
            }
            while p < len && class(self.text.char(p)) == 0 {
                p += 1;
            }
            if p < len && self.text.char(p) != '\n' {
                let k = class(self.text.char(p));
                while p < len && class(self.text.char(p)) == k {
                    p += 1;
                }
            }
            p
        } else {
            let mut p = pos;
            if p == 0 {
                return 0;
            }
            if self.text.char(p - 1) == '\n' {
                return p - 1;
            }
            while p > 0 && class(self.text.char(p - 1)) == 0 {
                p -= 1;
            }
            if p > 0 && self.text.char(p - 1) != '\n' {
                let k = class(self.text.char(p - 1));
                while p > 0 && class(self.text.char(p - 1)) == k {
                    p -= 1;
                }
            }
            p
        }
    }

    // ------------------------------------------------------------ indentation

    /// Tab key: indent lines for multi-line selections, else insert an
    /// indent unit (spaces up to the next tab stop in spaces mode).
    pub fn tab(&mut self, cfg: &IndentCfg) {
        let multiline = self.sels.iter().any(|s| self.line_of(s.start()) != self.line_of(s.end()));
        if multiline {
            return self.indent_lines(cfg);
        }
        let edits = self
            .sels
            .iter()
            .map(|sel| {
                let text = if cfg.use_tabs {
                    "\t".to_string()
                } else {
                    let ls = self.line_start(self.line_of(sel.start()));
                    let col = super::ops::visual_col(&self.slice(ls, sel.start()), usize::MAX, cfg.width);
                    " ".repeat(cfg.width - col % cfg.width.max(1))
                };
                Edit::new(sel.start(), sel.end(), text)
            })
            .collect();
        self.apply(edits, After::Map, Kind::Other);
    }

    pub fn indent_lines(&mut self, cfg: &IndentCfg) {
        let unit = cfg.unit();
        let mut edits = Vec::new();
        for (a, b) in self.sel_line_ranges() {
            for l in a..=b {
                if self.line_start(l) == self.line_end(l) {
                    continue; // leave empty lines empty
                }
                let p = self.line_start(l);
                edits.push(Edit::new(p, p, unit.clone()));
            }
        }
        // A selection edge at column 0 stays before the new indentation so
        // whole-line selections keep covering whole lines.
        let map = |p: usize| {
            if p == self.line_start(self.line_of(p)) {
                super::buffer::map_pos_left(p, &edits)
            } else {
                super::buffer::map_pos(p, &edits)
            }
        };
        let sels = self.sels.iter().map(|s| Sel::new(map(s.anchor), map(s.head))).collect();
        self.apply(edits, After::Sels(sels), Kind::Other);
    }

    pub fn outdent_lines(&mut self, cfg: &IndentCfg) {
        let mut edits = Vec::new();
        for (a, b) in self.sel_line_ranges() {
            for l in a..=b {
                let ls = self.line_start(l);
                let text = self.line_text(l);
                let mut n = 0;
                let mut col = 0;
                for c in text.chars() {
                    if col >= cfg.width {
                        break;
                    }
                    match c {
                        ' ' => {
                            col += 1;
                            n += 1;
                        }
                        '\t' => {
                            n += 1;
                            break;
                        }
                        _ => break,
                    }
                }
                if n > 0 {
                    edits.push(Edit::new(ls, ls + n, ""));
                }
            }
        }
        self.apply(edits, After::Map, Kind::Other);
    }

    // ------------------------------------------------------------ line commands

    /// Duplicates selected text, or the current line(s) when nothing is selected.
    pub fn duplicate(&mut self) {
        if self.has_selection() {
            let edits = self.sels.iter().map(|s| Edit::new(s.end(), s.end(), self.slice(s.start(), s.end()))).collect();
            let keep = self.sels.clone();
            self.apply(edits, After::Map, Kind::Other);
            // Keep the original selections (the copy goes after them).
            let mut shift = 0usize;
            let fixed = keep
                .iter()
                .map(|s| {
                    let n = s.end() - s.start();
                    let r = Sel::new(s.anchor + shift, s.head + shift);
                    shift += n;
                    r
                })
                .collect();
            let p = self.primary;
            self.set_sels(fixed, p);
            return;
        }
        let mut edits = Vec::new();
        for (a, b) in self.sel_line_ranges() {
            let start = self.line_start(a);
            let end = self.line_end(b);
            let block = self.slice(start, end);
            edits.push(Edit::new(end, end, format!("\n{block}")));
        }
        // Carets move down onto the copy.
        let ranges = self.sel_line_ranges();
        let mut new_sels = Vec::new();
        let mut added = 0usize;
        for (i, (a, b)) in ranges.iter().enumerate() {
            let block_len = self.line_end(*b) - self.line_start(*a) + 1;
            for s in &self.sels {
                let l = self.line_of(s.head);
                if l >= *a && l <= *b {
                    new_sels.push(Sel::caret(s.head + block_len + added));
                }
            }
            added += block_len;
            let _ = i;
        }
        self.apply(edits, After::Sels(new_sels), Kind::Other);
    }

    /// Deletes every line touched by a selection.
    pub fn delete_lines(&mut self) {
        let len = self.len();
        let edits: Vec<Edit> = self
            .sel_line_ranges()
            .into_iter()
            .map(|(a, b)| {
                let start = self.line_start(a);
                let next = self.line_start(b + 1);
                if b + 1 < self.line_count() {
                    Edit::new(start, next, "")
                } else if start > 0 {
                    Edit::new(start - 1, len, "") // last line: remove the preceding newline
                } else {
                    Edit::new(0, len, "")
                }
            })
            .collect();
        let carets: Vec<Sel> = edits.iter().map(|e| Sel::caret(e.start)).collect();
        // Map the carets through the edits themselves.
        let mapped = carets.iter().map(|c| Sel::caret(super::buffer::map_pos(c.head, &edits))).collect();
        self.apply(edits, After::Sels(mapped), Kind::Other);
    }

    /// Moves the lines touched by selections up or down by one line.
    pub fn move_lines(&mut self, up: bool) {
        let ranges = self.sel_line_ranges();
        let last = self.line_count() - 1;
        if (up && ranges.first().is_some_and(|r| r.0 == 0)) || (!up && ranges.last().is_some_and(|r| r.1 >= last)) {
            return;
        }
        let mut edits = Vec::new();
        let mut delta_for = Vec::new();
        for (a, b) in &ranges {
            let (a, b) = (*a, *b);
            if up {
                let start = self.line_start(a - 1);
                let end = self.line_end(b);
                let above = self.line_text(a - 1);
                let block = self.slice(self.line_start(a), end);
                edits.push(Edit::new(start, end, format!("{block}\n{above}")));
                delta_for.push((a, b, -((above.chars().count() + 1) as isize)));
            } else {
                let start = self.line_start(a);
                let end = self.line_end(b + 1);
                let below = self.line_text(b + 1);
                let block = self.slice(start, self.line_end(b));
                edits.push(Edit::new(start, end, format!("{below}\n{block}")));
                delta_for.push((a, b, (below.chars().count() + 1) as isize));
            }
        }
        let new_sels = self
            .sels
            .iter()
            .map(|s| {
                let l = self.line_of(s.head);
                let d = delta_for.iter().find(|(a, b, _)| l >= *a && l <= *b).map_or(0, |x| x.2);
                Sel::new((s.anchor as isize + d) as usize, (s.head as isize + d) as usize)
            })
            .collect();
        self.apply(edits, After::Sels(new_sels), Kind::Other);
    }

    /// Joins selected lines (or the current line with the next one),
    /// replacing each line break and following indentation with one space.
    pub fn join_lines(&mut self) {
        let mut edits = Vec::new();
        for (a, b) in self.sel_line_ranges() {
            let b = if a == b { (b + 1).min(self.line_count() - 1) } else { b };
            for l in a..b {
                let nl = self.line_end(l);
                let next_text = self.line_text(l + 1);
                let ws = leading_ws(&next_text).chars().count();
                let this = self.line_text(l);
                let trailing = this.len() - this.trim_end_matches([' ', '\t']).len();
                let sep = if next_text.trim().is_empty() || this.trim().is_empty() { "" } else { " " };
                edits.push(Edit::new(nl - trailing, nl + 1 + ws, sep));
            }
        }
        self.apply(edits, After::Map, Kind::Other);
    }

    /// Toggles a line comment on the selected lines.
    pub fn toggle_comment(&mut self, token: &str) {
        let mut edits = Vec::new();
        for (a, b) in self.sel_line_ranges() {
            let lines: Vec<String> = (a..=b).map(|l| self.line_text(l)).collect();
            let nonblank: Vec<&String> = lines.iter().filter(|t| !t.trim().is_empty()).collect();
            if nonblank.is_empty() {
                continue;
            }
            let all = nonblank.iter().all(|t| t.trim_start().starts_with(token));
            let min_indent = nonblank.iter().map(|t| leading_ws(t).chars().count()).min().unwrap_or(0);
            for (i, t) in lines.iter().enumerate() {
                if t.trim().is_empty() {
                    continue;
                }
                let ls = self.line_start(a + i);
                if all {
                    let ws = leading_ws(t).chars().count();
                    let after = &t[leading_ws(t).len() + token.len()..];
                    let extra = usize::from(after.starts_with(' '));
                    edits.push(Edit::new(ls + ws, ls + ws + token.chars().count() + extra, ""));
                } else {
                    edits.push(Edit::new(ls + min_indent, ls + min_indent, format!("{token} ")));
                }
            }
        }
        self.apply(edits, After::Map, Kind::Other);
    }

    // ------------------------------------------------------------ transformations

    /// Applies `f` to each non-empty selection. Returns how many changed.
    pub fn transform_selections(&mut self, f: &dyn Fn(&str) -> String) -> usize {
        let mut edits = Vec::new();
        let mut new_sels = Vec::new();
        let mut shift: isize = 0;
        for s in &self.sels {
            let old = self.slice(s.start(), s.end());
            let new = f(&old);
            let start = (s.start() as isize + shift) as usize;
            let n = new.chars().count();
            new_sels.push(if s.anchor <= s.head { Sel::new(start, start + n) } else { Sel::new(start + n, start) });
            shift += n as isize - (s.end() - s.start()) as isize;
            if new != old {
                edits.push(Edit::new(s.start(), s.end(), new));
            }
        }
        let changed = edits.len();
        if changed > 0 {
            self.apply(edits, After::Sels(new_sels), Kind::Other);
        }
        changed
    }

    /// The line blocks a line tool operates on: the selected lines, or the
    /// whole document when nothing is selected.
    pub fn line_blocks(&self) -> Vec<(usize, usize)> {
        if self.has_selection() {
            self.sel_line_ranges()
        } else {
            vec![(0, self.line_count() - 1)]
        }
    }

    /// Replaces each line block with `f(lines)`. Returns the number of lines
    /// that differ (for confirmation prompts).
    pub fn transform_lines(&mut self, f: &dyn Fn(Vec<String>) -> Vec<String>) -> usize {
        let mut edits = Vec::new();
        let mut changed = 0;
        for (a, b) in self.line_blocks() {
            let lines: Vec<String> = (a..=b).map(|l| self.line_text(l)).collect();
            let out = f(lines.clone());
            changed += count_changed(&lines, &out);
            let new_text = out.join("\n");
            let start = self.line_start(a);
            let end = self.line_end(b);
            if new_text != self.slice(start, end) {
                edits.push(Edit::new(start, end, new_text));
            }
        }
        if !edits.is_empty() {
            let sels: Vec<Sel> = edits.iter().map(|e| Sel::new(e.start, e.start + e.text.chars().count())).collect();
            // Selections cover the transformed blocks (post-edit coordinates).
            let mut shift: isize = 0;
            let mapped = edits
                .iter()
                .zip(sels)
                .map(|(e, s)| {
                    let st = (s.start() as isize + shift) as usize;
                    shift += e.text.chars().count() as isize - (e.end - e.start) as isize;
                    Sel::new(st, st + (s.end() - s.start()))
                })
                .collect();
            let whole = !self.has_selection();
            self.apply(edits, After::Sels(mapped), Kind::Other);
            if whole {
                self.set_caret(0);
            }
        }
        changed
    }

    /// Previews a line transformation without modifying the buffer:
    /// (lines in scope, lines that would change).
    pub fn preview_lines(&self, f: &dyn Fn(Vec<String>) -> Vec<String>) -> (usize, usize) {
        let mut total = 0;
        let mut changed = 0;
        for (a, b) in self.line_blocks() {
            let lines: Vec<String> = (a..=b).map(|l| self.line_text(l)).collect();
            total += lines.len();
            let out = f(lines.clone());
            changed += count_changed(&lines, &out);
        }
        (total, changed)
    }

    // ------------------------------------------------------------ selection commands

    pub fn select_all(&mut self) {
        let len = self.len();
        self.set_sels(vec![Sel::new(0, len)], 0);
    }

    /// Ctrl+Alt+D: selects the word under the caret, then adds the next
    /// occurrence of the primary selection as another selection.
    pub fn add_next_occurrence(&mut self) -> bool {
        let p = self.primary_sel();
        if p.is_empty() {
            if let Some((s, e)) = self.word_at(p.head) {
                let mut sels = self.sels.clone();
                sels[self.primary] = Sel::new(s, e);
                let prim = self.primary;
                self.set_sels(sels, prim);
                return true;
            }
            return false;
        }
        let needle = self.slice(p.start(), p.end());
        let last_end = self.sels.iter().map(|s| s.end()).max().unwrap_or(0);
        let hay = self.text.to_string();
        let start_byte = self.text.char_to_byte(last_end);
        let found = hay[start_byte..]
            .find(&needle)
            .map(|b| start_byte + b)
            .or_else(|| hay.find(&needle))
            .map(|b| self.text.byte_to_char(b));
        if let Some(s) = found {
            let e = s + needle.chars().count();
            if self.sels.iter().any(|x| x.start() == s) {
                return false;
            }
            let mut sels = self.sels.clone();
            sels.push(Sel::new(s, e));
            let n = sels.len() - 1;
            self.set_sels(sels, n);
            return true;
        }
        false
    }

    /// Selects every occurrence of the primary selection (or word).
    pub fn select_all_occurrences(&mut self) -> usize {
        let mut p = self.primary_sel();
        if p.is_empty() {
            match self.word_at(p.head) {
                Some((s, e)) => p = Sel::new(s, e),
                None => return 0,
            }
        }
        let needle = self.slice(p.start(), p.end());
        let hay = self.text.to_string();
        let n = needle.chars().count();
        let sels: Vec<Sel> = hay
            .match_indices(&needle)
            .map(|(b, _)| {
                let s = self.text.byte_to_char(b);
                Sel::new(s, s + n)
            })
            .collect();
        let count = sels.len();
        if count > 0 {
            let prim = sels.iter().position(|s| s.start() == p.start()).unwrap_or(0);
            self.set_sels(sels, prim);
        }
        count
    }

    /// Adds a caret on the line above/below each caret (same display column).
    pub fn add_caret_vertical(&mut self, down: bool, tab: usize) {
        let mut sels = self.sels.clone();
        for s in &self.sels {
            let line = self.line_of(s.head);
            let target = if down { line + 1 } else { line.wrapping_sub(1) };
            if target >= self.line_count() {
                continue;
            }
            let col = visual_col(&self.line_text(line), s.head - self.line_start(line), tab);
            let t = self.line_text(target);
            sels.push(Sel::caret(self.line_start(target) + idx_at_col(&t, col, tab)));
        }
        let n = sels.len() - 1;
        self.set_sels(sels, n);
    }

    // ------------------------------------------------------------ movement

    pub fn move_horizontal(&mut self, forward: bool, word: bool, extend: bool) {
        let len = self.len();
        let sels = self
            .sels
            .iter()
            .map(|s| {
                if !extend && !s.is_empty() && !word {
                    return Sel::caret(if forward { s.end() } else { s.start() });
                }
                let h = if word {
                    self.word_boundary(s.head, forward)
                } else if forward {
                    (s.head + 1).min(len)
                } else {
                    s.head.saturating_sub(1)
                };
                if extend {
                    Sel::new(s.anchor, h)
                } else {
                    Sel::caret(h)
                }
            })
            .collect();
        let p = self.primary;
        self.set_sels(sels, p);
    }

    /// Home: first non-blank char, then column 0 (smart home). End: line end.
    pub fn move_home_end(&mut self, end: bool, extend: bool) {
        let sels = self
            .sels
            .iter()
            .map(|s| {
                let line = self.line_of(s.head);
                let ls = self.line_start(line);
                let h = if end {
                    self.line_end(line)
                } else {
                    let text = self.line_text(line);
                    let first = ls + leading_ws(&text).chars().count();
                    if s.head == first {
                        ls
                    } else {
                        first
                    }
                };
                if extend {
                    Sel::new(s.anchor, h)
                } else {
                    Sel::caret(h)
                }
            })
            .collect();
        let p = self.primary;
        self.set_sels(sels, p);
    }

    pub fn move_doc(&mut self, end: bool, extend: bool) {
        let target = if end { self.len() } else { 0 };
        let anchor = self.primary_sel().anchor;
        let sel = if extend { Sel::new(anchor, target) } else { Sel::caret(target) };
        self.set_sels(vec![sel], 0);
    }
}

fn count_changed(a: &[String], b: &[String]) -> usize {
    let common = a.iter().zip(b).filter(|(x, y)| x != y).count();
    common + a.len().abs_diff(b.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(text_with_caret: &str) -> Buffer {
        // `|` marks carets, `[` `]` mark a selection (anchor, head)
        let mut text = String::new();
        let mut sels = Vec::new();
        let mut anchor = None;
        for c in text_with_caret.chars() {
            let pos = text.chars().count();
            match c {
                '|' => sels.push(Sel::caret(pos)),
                '[' => anchor = Some(pos),
                ']' => sels.push(Sel::new(anchor.take().unwrap(), pos)),
                c => text.push(c),
            }
        }
        let mut b = Buffer::new(&text);
        b.set_sels(sels, 0);
        b
    }

    fn show(b: &Buffer) -> String {
        let mut out: Vec<char> = b.text.to_string().chars().collect();
        let mut marks: Vec<(usize, char)> = Vec::new();
        for s in &b.sels {
            if s.is_empty() {
                marks.push((s.head, '|'));
            } else {
                marks.push((s.start(), '['));
                marks.push((s.end(), ']'));
            }
        }
        marks.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        for (p, c) in marks {
            out.insert(p, c);
        }
        out.into_iter().collect()
    }

    const CFG: IndentCfg = IndentCfg { use_tabs: false, width: 4, auto: true };

    #[test]
    fn typing_and_overwrite() {
        let mut b = buf("ab|cd");
        b.insert_text("X", false);
        assert_eq!(show(&b), "abX|cd");
        b.insert_text("Y", true);
        assert_eq!(show(&b), "abXY|d");
        let mut b = buf("a[bc]d");
        b.insert_text("Z", false);
        assert_eq!(show(&b), "aZ|d");
    }

    #[test]
    fn newline_keeps_indent_and_splits_braces() {
        let mut b = buf("    x|");
        b.newline(&CFG, &|_| false, true);
        assert_eq!(show(&b), "    x\n    |");
        let mut b = buf("fn a() {|}");
        b.newline(&CFG, &|s| s.trim_end().ends_with('{'), true);
        assert_eq!(show(&b), "fn a() {\n    |\n}");
        let no_auto = IndentCfg { auto: false, ..CFG };
        let mut b = buf("  x|");
        b.newline(&no_auto, &|_| true, true);
        assert_eq!(show(&b), "  x\n|");
    }

    #[test]
    fn backspace_soft_tabs() {
        let mut b = buf("        |x");
        b.backspace(&CFG);
        assert_eq!(show(&b), "    |x");
        let mut b = buf("  ab|");
        b.backspace(&CFG);
        assert_eq!(show(&b), "  a|");
        let mut b = buf("|abc");
        b.backspace(&CFG);
        assert_eq!(show(&b), "|abc");
    }

    #[test]
    fn delete_and_words() {
        let mut b = buf("foo bar|");
        b.delete_word(false);
        assert_eq!(show(&b), "foo |");
        let mut b = buf("|foo.bar");
        b.delete_word(true);
        assert_eq!(show(&b), "|.bar");
        let mut b = buf("ab|c");
        b.delete_forward();
        assert_eq!(show(&b), "ab|");
        b.delete_forward();
        assert_eq!(show(&b), "ab|");
    }

    #[test]
    fn tab_and_indent() {
        let mut b = buf("ab|");
        b.tab(&CFG);
        assert_eq!(show(&b), "ab  |");
        let mut b = buf("[a\nb\n\nc]");
        b.indent_lines(&CFG);
        assert_eq!(b.text.to_string(), "    a\n    b\n\n    c");
        b.outdent_lines(&CFG);
        assert_eq!(b.text.to_string(), "a\nb\n\nc");
        let tabs = IndentCfg { use_tabs: true, ..CFG };
        let mut b = buf("[x\ny]");
        b.tab(&tabs);
        assert_eq!(b.text.to_string(), "\tx\n\ty");
        b.outdent_lines(&tabs);
        assert_eq!(b.text.to_string(), "x\ny");
    }

    #[test]
    fn duplicate_delete_move_join() {
        let mut b = buf("one\ntw|o\nthree");
        b.duplicate();
        assert_eq!(show(&b), "one\ntwo\ntw|o\nthree");
        let mut b = buf("a[bc]d");
        b.duplicate();
        assert_eq!(show(&b), "a[bc]bcd");
        let mut b = buf("one\ntw|o\nthree");
        b.delete_lines();
        assert_eq!(show(&b), "one\n|three");
        let mut b = buf("one\ntwo\nthr|ee");
        b.delete_lines();
        assert_eq!(b.text.to_string(), "one\ntwo");
        let mut b = buf("one\ntw|o\nthree");
        b.move_lines(true);
        assert_eq!(show(&b), "tw|o\none\nthree");
        b.move_lines(true);
        assert_eq!(show(&b), "tw|o\none\nthree");
        b.move_lines(false);
        b.move_lines(false);
        assert_eq!(show(&b), "one\nthree\ntw|o");
        b.undo();
        assert_eq!(b.text.to_string(), "one\ntwo\nthree");
        let mut b = buf("a  |\n    b\nc");
        b.join_lines();
        assert_eq!(b.text.to_string(), "a b\nc");
    }

    #[test]
    fn comments() {
        let mut b = buf("[  a\n\n  b]");
        b.toggle_comment("//");
        assert_eq!(b.text.to_string(), "  // a\n\n  // b");
        b.toggle_comment("//");
        assert_eq!(b.text.to_string(), "  a\n\n  b");
        let mut b = buf("#x|");
        b.toggle_comment("#");
        assert_eq!(b.text.to_string(), "x");
    }

    #[test]
    fn transforms_are_undoable() {
        let mut b = buf("[hello] and [world]");
        assert_eq!(b.transform_selections(&|s: &str| s.to_uppercase()), 2);
        assert_eq!(show(&b), "[HELLO] and [WORLD]");
        b.undo();
        assert_eq!(b.text.to_string(), "hello and world");
        let mut b = Buffer::new("c\na\nb\na");
        assert_eq!(b.preview_lines(&|mut v: Vec<String>| { v.sort(); v }), (4, 2));
        let changed = b.transform_lines(&|mut v: Vec<String>| {
            v.sort();
            v.dedup();
            v
        });
        assert_eq!(b.text.to_string(), "a\nb\nc");
        assert!(changed >= 3);
        b.undo();
        assert_eq!(b.text.to_string(), "c\na\nb\na");
    }

    #[test]
    fn multi_caret_editing() {
        let mut b = buf("x|\ny|\nz|");
        b.insert_text(";", false);
        assert_eq!(show(&b), "x;|\ny;|\nz;|");
        b.backspace(&CFG);
        assert_eq!(show(&b), "x|\ny|\nz|");
        b.insert_per_selection(&["1".into(), "2".into(), "3".into()]);
        assert_eq!(b.text.to_string(), "x1\ny2\nz3");
        let mut b = buf("foo |bar foo baz foo");
        assert_eq!(b.select_all_occurrences(), 1);
        let mut b = buf("fo|o bar foo baz foo");
        assert!(b.add_next_occurrence());
        assert!(b.add_next_occurrence());
        assert!(b.add_next_occurrence());
        assert_eq!(b.sels.len(), 3);
        b.insert_text("X", false);
        assert_eq!(b.text.to_string(), "X bar X baz X");
        let mut b = buf("ab|c\nabc");
        b.add_caret_vertical(true, 4);
        assert_eq!(show(&b), "ab|c\nab|c");
    }

    #[test]
    fn movement() {
        let mut b = buf("  foo bar|");
        b.move_home_end(false, false);
        assert_eq!(show(&b), "  |foo bar");
        b.move_home_end(false, false);
        assert_eq!(show(&b), "|  foo bar");
        b.move_home_end(true, true);
        assert_eq!(show(&b), "[  foo bar]");
        let mut b = buf("foo bar|");
        b.move_horizontal(false, true, false);
        assert_eq!(show(&b), "foo |bar");
        b.move_horizontal(false, true, true);
        assert_eq!(b.sels[0], Sel::new(4, 0));
    }

    #[test]
    fn columns() {
        assert_eq!(visual_col("\tab", 1, 4), 4);
        assert_eq!(visual_col("a\tb", 2, 4), 4);
        assert_eq!(idx_at_col("\tab", 5, 4), 2);
        assert_eq!(idx_at_col("abc", 10, 4), 3);
    }
}
