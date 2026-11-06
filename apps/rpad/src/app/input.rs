//! Keyboard, mouse and IME handling for the editor and the text fields.

use super::commands::{self, Chord, Command};
use super::{App, Focus, Host, PaletteMode};
use crate::doc::Sel;
use crate::syntax;
use crate::ui::layout;
use crate::ui::lineedit::{LineKey, Mods as LMods};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Delete,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    F(u8),
}

impl Key {
    fn chord_name(self) -> String {
        match self {
            Key::Char(c) => c.to_lowercase().to_string(),
            Key::Enter => "enter".into(),
            Key::Tab => "tab".into(),
            Key::Backspace => "backspace".into(),
            Key::Delete => "delete".into(),
            Key::Escape => "escape".into(),
            Key::Left => "left".into(),
            Key::Right => "right".into(),
            Key::Up => "up".into(),
            Key::Down => "down".into(),
            Key::Home => "home".into(),
            Key::End => "end".into(),
            Key::PageUp => "pageup".into(),
            Key::PageDown => "pagedown".into(),
            Key::Insert => "insert".into(),
            Key::F(n) => format!("f{n}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct KeyInput {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// Text produced by the key (layout-aware), if any.
    pub text: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drag {
    /// Selecting text; `unit` 1 = chars, 2 = words, 3 = lines.
    Select { anchor: usize, unit: u8, word: (usize, usize) },
    /// Alt+drag column selection from (line, cell column).
    Column { line: usize, col: usize },
    VScroll { grab: f32 },
    HScroll { grab: f32 },
}

#[derive(Default)]
pub struct InputState {
    pub drag: Option<Drag>,
    last_click: Option<(Instant, f32, f32, u8)>,
    pub preedit: String,
    pub mouse: (f32, f32),
}

impl App {
    // ------------------------------------------------------------ keyboard

    /// Handles a key press. Returns true if it was consumed.
    pub fn key(&mut self, k: &KeyInput, host: &dyn Host) -> bool {
        let lm = LMods { ctrl: k.ctrl, shift: k.shift };
        if k.key == Key::Escape {
            if self.palette.is_some() {
                self.palette = None;
                self.focus = Focus::Editor;
            } else if matches!(self.focus, Focus::Find | Focus::Replace) || (self.focus == Focus::Editor && self.find.visible && self.doc().buf.sels.len() == 1) {
                self.close_find();
            } else if matches!(self.focus, Focus::FifQuery | Focus::FifDir | Focus::FifFilter | Focus::Results) {
                self.focus = Focus::Editor;
            } else if self.doc().buf.sels.len() > 1 || self.doc().buf.has_selection() {
                let h = self.doc().buf.primary_sel().head;
                self.doc_mut().buf.set_caret(h);
            } else if self.results.is_some() {
                self.stop_fif();
                self.results = None;
            }
            return true;
        }
        // Text fields first.
        match self.focus {
            Focus::Palette => return self.palette_key(k, lm, host),
            Focus::Find | Focus::Replace => {
                if k.key == Key::Enter {
                    if self.focus == Focus::Replace {
                        if k.ctrl && k.alt {
                            self.replace_all(host);
                        } else {
                            self.replace_one(host);
                        }
                    } else {
                        self.find_step(!k.shift, host);
                    }
                    return true;
                }
                if k.key == Key::Tab {
                    self.focus = if self.focus == Focus::Find && self.find.replace_mode { Focus::Replace } else { Focus::Find };
                    return true;
                }
                if k.alt && !k.ctrl {
                    match k.key {
                        Key::Char('c') => self.find.case = !self.find.case,
                        Key::Char('w') => self.find.word = !self.find.word,
                        Key::Char('r') => self.find.regex = !self.find.regex,
                        Key::Char('l') => self.toggle_in_selection(),
                        Key::Char('a') if self.find.replace_mode => {
                            self.replace_all(host);
                            return true;
                        }
                        _ => return false,
                    }
                    self.find.invalidate();
                    return true;
                }
                let hist = if self.focus == Focus::Find { self.settings.find_history.clone() } else { self.settings.replace_history.clone() };
                let edit = if self.focus == Focus::Find { &mut self.find.find } else { &mut self.find.replace };
                if field_key(edit, k, lm, &hist, host) {
                    self.find.invalidate();
                    return true;
                }
            }
            Focus::FifQuery | Focus::FifDir | Focus::FifFilter => {
                if k.key == Key::Enter {
                    self.start_fif(host);
                    return true;
                }
                if k.key == Key::Tab {
                    self.focus = match (self.focus, k.shift) {
                        (Focus::FifQuery, false) | (Focus::FifFilter, true) => Focus::FifDir,
                        (Focus::FifDir, false) | (Focus::FifQuery, true) => Focus::FifFilter,
                        _ => Focus::FifQuery,
                    };
                    return true;
                }
                let hist = self.settings.find_history.clone();
                let edit = match self.focus {
                    Focus::FifQuery => &mut self.fif.query,
                    Focus::FifDir => &mut self.fif.dir,
                    _ => &mut self.fif.filter,
                };
                if field_key(edit, k, lm, if self.focus == Focus::FifQuery { &hist } else { &[] }, host) {
                    return true;
                }
            }
            Focus::Results => {
                if let Some(r) = self.results.as_mut() {
                    let n = r.items.len();
                    match k.key {
                        Key::Up if n > 0 => {
                            r.sel = r.sel.saturating_sub(1);
                            return true;
                        }
                        Key::Down if n > 0 => {
                            r.sel = (r.sel + 1).min(n - 1);
                            return true;
                        }
                        Key::Enter => {
                            let s = r.sel;
                            self.open_result(s, host);
                            return true;
                        }
                        _ => {}
                    }
                }
            }
            Focus::Editor => {}
        }
        // Global shortcuts.
        let chord = Chord { ctrl: k.ctrl, shift: k.shift, alt: k.alt, key: k.key.chord_name() };
        if let Some(cmd) = commands::lookup(&chord) {
            if !(self.focus != Focus::Editor && matches!(cmd, Command::ToggleOverwrite)) {
                self.dispatch(cmd, host);
                return true;
            }
        }
        if self.focus != Focus::Editor {
            return false;
        }
        self.editor_key(k)
    }

    fn palette_key(&mut self, k: &KeyInput, lm: LMods, host: &dyn Host) -> bool {
        let Some(p) = self.palette.as_mut() else { return false };
        let n = p.shown.len();
        match k.key {
            Key::Enter => {
                self.palette_accept(host);
                return true;
            }
            Key::Up if n > 0 => {
                p.sel = (p.sel + n - 1) % n;
                return true;
            }
            Key::Down if n > 0 => {
                p.sel = (p.sel + 1) % n;
                return true;
            }
            Key::PageUp if n > 0 => {
                p.sel = p.sel.saturating_sub(10);
                return true;
            }
            Key::PageDown if n > 0 => {
                p.sel = (p.sel + 10).min(n - 1);
                return true;
            }
            _ => {}
        }
        if field_key(&mut p.edit, k, lm, &[], host) {
            self.palette_filter();
            self.palette_goto_preview();
            return true;
        }
        // Printable keys arrive through `text_input`.
        false
    }

    /// Go to line: moves to the typed line while typing.
    fn palette_goto_preview(&mut self) {
        let Some(p) = self.palette.as_ref().filter(|p| p.mode == PaletteMode::GotoLine) else { return };
        let t = p.edit.text.clone();
        if let Some(l) = t.split([':', ',']).next().and_then(|x| x.trim().parse::<usize>().ok()) {
            if l >= 1 {
                let line = (l - 1).min(self.doc().buf.line_count() - 1);
                let d = self.doc_mut();
                d.reveal(line);
                let p = d.buf.line_start(line);
                d.buf.set_caret(p);
                self.reveal_caret = true;
            }
        }
    }

    /// Typed text (from the keyboard or an IME commit).
    pub fn text_input(&mut self, s: &str, host: &dyn Host) {
        let s: String = s.chars().filter(|c| !c.is_control() || *c == '\t').collect();
        if s.is_empty() {
            return;
        }
        match self.focus {
            Focus::Editor => {
                let ov = self.overwrite;
                if s == "\t" {
                    return;
                }
                self.edit(|b| b.insert_text(&s, ov));
                self.auto_outdent_closer(&s);
            }
            Focus::Palette => {
                if let Some(p) = self.palette.as_mut() {
                    p.edit.insert(&s);
                }
                self.palette_filter();
                self.palette_goto_preview();
            }
            Focus::Find => {
                self.find.find.insert(&s);
                self.find.invalidate();
            }
            Focus::Replace => self.find.replace.insert(&s),
            Focus::FifQuery => self.fif.query.insert(&s),
            Focus::FifDir => self.fif.dir.insert(&s),
            Focus::FifFilter => self.fif.filter.insert(&s),
            Focus::Results => {}
        }
        let _ = host;
    }

    /// Typing a closing brace on a whitespace-only line removes one indent level.
    fn auto_outdent_closer(&mut self, s: &str) {
        if !self.settings.auto_indent || !matches!(s, "}" | "]" | ")") || self.doc().lang == syntax::Lang::Plain {
            return;
        }
        let cfg = self.indent_cfg();
        let b = &self.doc().buf;
        let p = b.primary_sel().head;
        let line = b.line_of(p);
        let text = b.line_text(line);
        if text.trim() == s && b.sels.len() == 1 && line > 0 {
            let prev = b.line_text(line - 1);
            let opens_block = prev.trim_end().ends_with(['{', '[', '(']);
            let cur_indent = text.len() - text.trim_start().len();
            let prev_indent = prev.len() - prev.trim_start().len();
            if !opens_block && cur_indent > 0 && cur_indent > prev_indent.saturating_sub(cfg.width) {
                self.edit(|b| b.outdent_lines(&cfg));
            }
        }
    }

    fn editor_key(&mut self, k: &KeyInput) -> bool {
        let cfg = self.indent_cfg();
        let ext = k.shift;
        match k.key {
            Key::Left | Key::Right => {
                let fwd = k.key == Key::Right;
                if k.alt && k.shift {
                    return false;
                }
                self.doc_mut().buf.move_horizontal(fwd, k.ctrl, ext);
                self.after_move();
            }
            Key::Up | Key::Down => {
                if k.ctrl && !k.shift && !k.alt {
                    self.scroll_rows(if k.key == Key::Up { -1 } else { 1 });
                    return true;
                }
                self.move_vertical(if k.key == Key::Up { -1 } else { 1 }, ext);
            }
            Key::PageUp | Key::PageDown => {
                let n = self.layout.rows_visible.saturating_sub(1).max(1) as isize;
                let n = if k.key == Key::PageUp { -n } else { n };
                self.scroll_rows(n);
                self.move_vertical(n, ext);
            }
            Key::Home | Key::End => {
                let end = k.key == Key::End;
                if k.ctrl {
                    self.doc_mut().buf.move_doc(end, ext);
                } else {
                    self.doc_mut().buf.move_home_end(end, ext);
                }
                self.after_move();
            }
            Key::Enter => {
                let lang = self.doc().lang;
                let def = syntax::def(lang);
                let increase = |before: &str| {
                    let t = before.trim_end();
                    if matches!(lang, syntax::Lang::Xml | syntax::Lang::Html) {
                        // After an opening tag that is not closed on the same line.
                        return t.ends_with('>') && !t.ends_with("/>") && !t.trim_start().starts_with("</") && t.rfind('<').is_some_and(|p| !t[p..].starts_with("</") && !t[p..].starts_with("<!") && !t[p..].starts_with("<?") && !t[p..].contains("</"));
                    }
                    def.indent_after.iter().any(|e| t.ends_with(e))
                };
                let pairs = lang != syntax::Lang::Plain;
                self.edit(|b| b.newline(&cfg, &increase, pairs));
            }
            Key::Tab => {
                if k.ctrl {
                    return false;
                }
                if k.shift {
                    self.edit(|b| b.outdent_lines(&cfg));
                } else {
                    self.edit(|b| b.tab(&cfg));
                }
            }
            Key::Backspace => {
                if k.ctrl {
                    self.edit(|b| b.delete_word(false));
                } else {
                    self.edit(|b| b.backspace(&cfg));
                }
            }
            Key::Delete => {
                if k.ctrl {
                    self.edit(|b| b.delete_word(true));
                } else {
                    self.edit(|b| b.delete_forward());
                }
            }
            _ => {
                if let Some(t) = &k.text {
                    // AltGr is reported as Ctrl+Alt on Windows; plain Ctrl
                    // combinations are shortcuts, not text.
                    if k.ctrl == k.alt && !t.chars().all(char::is_control) {
                        return false; // delivered through text_input
                    }
                }
                return false;
            }
        }
        true
    }

    fn after_move(&mut self) {
        self.doc_mut().goals.clear();
        self.doc_mut().buf.seal();
        self.reveal_caret = true;
    }

    // ------------------------------------------------------------ geometry helpers

    /// Wrapped row starts for `line` (just `[0]` without word wrap).
    pub fn rows_of(&self, line: usize) -> Vec<usize> {
        match self.layout.wrap_cells {
            Some(w) => layout::wrap(&self.doc().buf.line_text(line), w, self.settings.tab_width),
            None => vec![0],
        }
    }

    /// (line, row) of char position `p`.
    fn row_of_pos(&self, p: usize) -> (usize, usize, Vec<usize>) {
        let b = &self.doc().buf;
        let line = b.line_of(p);
        let col = p - b.line_start(line);
        let rows = self.rows_of(line);
        let row = rows.iter().rposition(|&s| s <= col).unwrap_or(0);
        (line, row, rows)
    }

    /// Moves every caret by `n` visual rows (wrap- and fold-aware).
    pub fn move_vertical(&mut self, n: isize, extend: bool) {
        let tab = self.settings.tab_width;
        let sels = self.doc().buf.sels.clone();
        let mut goals = self.doc().goals.clone();
        goals.resize(sels.len(), None);
        let mut out = Vec::with_capacity(sels.len());
        for (i, s) in sels.iter().enumerate() {
            if !extend && !s.is_empty() && n.abs() == 1 {
                // Collapse to the side we move towards first.
            }
            let (mut line, mut row, mut rows) = self.row_of_pos(s.head);
            let text = self.doc().buf.line_text(line);
            let col_in_line = s.head - self.doc().buf.line_start(line);
            let row_base = layout::col_of(&text, rows[row], tab);
            let x = layout::col_of(&text, col_in_line, tab) - row_base;
            let goal = goals[i].unwrap_or(x);
            goals[i] = Some(goal);
            let mut steps = n;
            let mut at_edge = false;
            while steps != 0 {
                if steps < 0 {
                    if row > 0 {
                        row -= 1;
                    } else if let Some(prev) = self.doc().prev_visible(line) {
                        line = prev;
                        rows = self.rows_of(line);
                        row = rows.len() - 1;
                    } else {
                        at_edge = true;
                        break;
                    }
                    steps += 1;
                } else {
                    if row + 1 < rows.len() {
                        row += 1;
                    } else {
                        let next = self.doc().next_visible(line);
                        if next >= self.doc().buf.line_count() {
                            at_edge = true;
                            break;
                        }
                        line = next;
                        rows = self.rows_of(line);
                        row = 0;
                    }
                    steps -= 1;
                }
            }
            let b = &self.doc().buf;
            let new_head = if at_edge && steps != 0 && n.abs() == 1 && (row == 0 && n < 0 && line == 0 || n > 0 && line + 1 >= b.line_count() && row + 1 >= rows.len()) {
                // Moving past the first/last line goes to its start/end.
                if n < 0 {
                    b.line_start(line)
                } else {
                    b.line_end(line)
                }
            } else {
                let t = b.line_text(line);
                let start = rows[row];
                let end = rows.get(row + 1).copied().unwrap_or_else(|| t.chars().count());
                let base = layout::col_of(&t, start, tab);
                let idx = layout::idx_at(&t, (start, end), base, base + goal, tab);
                // Don't land after the last char of a wrapped row (that is the next row).
                let idx = if row + 1 < rows.len() && idx == end { end.saturating_sub(1).max(start) } else { idx };
                b.line_start(line) + idx
            };
            out.push(if extend { Sel::new(s.anchor, new_head) } else { Sel::caret(new_head) });
        }
        let d = self.doc_mut();
        let p = d.buf.primary;
        d.buf.set_sels(out, p);
        if d.buf.sels.len() == goals.len() {
            d.goals = goals;
        } else {
            d.goals.clear();
        }
        self.reveal_caret = true;
    }

    /// Scrolls the view by `n` visual rows.
    pub fn scroll_rows(&mut self, n: isize) {
        let mut steps = n;
        let (mut line, mut row) = (self.doc().top, self.doc().top_row);
        let count = self.doc().buf.line_count();
        while steps != 0 {
            if steps > 0 {
                let rows = self.rows_of(line).len();
                if row + 1 < rows {
                    row += 1;
                } else {
                    let next = self.doc().next_visible(line);
                    if next >= count {
                        break;
                    }
                    line = next;
                    row = 0;
                }
                steps -= 1;
            } else {
                if row > 0 {
                    row -= 1;
                } else if let Some(p) = self.doc().prev_visible(line) {
                    line = p;
                    row = self.rows_of(line).len() - 1;
                } else {
                    break;
                }
                steps += 1;
            }
        }
        let d = self.doc_mut();
        d.top = line;
        d.top_row = row;
    }

    /// Adjusts the scroll position so the primary caret is visible.
    pub fn ensure_caret_visible(&mut self, viewport_rows: usize, view_cells: usize) {
        let viewport_rows = viewport_rows.max(1);
        let head = self.doc().buf.primary_sel().head;
        let caret_line = self.doc().buf.line_of(head);
        if self.doc().is_hidden(caret_line) {
            self.doc_mut().reveal(caret_line);
        }
        let (line, row, _) = self.row_of_pos(head);
        let (top, top_row) = (self.doc().top, self.doc().top_row);
        if (line, row) < (top, top_row) {
            let d = self.doc_mut();
            d.top = line;
            d.top_row = row;
        } else {
            // Count rows from top to the caret; stop once beyond the viewport.
            let mut count = 0usize;
            let (mut l, mut r) = (top, top_row);
            let total = self.doc().buf.line_count();
            while (l, r) < (line, row) && count < viewport_rows + 2 {
                let rows = self.rows_of(l).len();
                if r + 1 < rows {
                    r += 1;
                } else {
                    l = self.doc().next_visible(l);
                    r = 0;
                    if l >= total {
                        break;
                    }
                }
                count += 1;
            }
            if count >= viewport_rows {
                // Put the caret on the last visible row.
                let d = self.doc_mut();
                d.top = line;
                d.top_row = row;
                self.scroll_rows(-(viewport_rows as isize - 1));
            }
        }
        if self.layout.wrap_cells.is_none() && self.layout.char_w > 0.0 {
            let b = &self.doc().buf;
            let ls = b.line_start(line);
            let x = layout::col_of(&b.line_text(line), head - ls, self.settings.tab_width) as f32 * self.layout.char_w;
            let w = view_cells as f32 * self.layout.char_w;
            let margin = self.layout.char_w * 4.0;
            let d = self.doc_mut();
            if x < d.scroll_x + margin {
                d.scroll_x = (x - margin * 2.0).max(0.0);
            } else if x > d.scroll_x + w - margin {
                d.scroll_x = x - w + margin * 2.0;
            }
        } else {
            self.doc_mut().scroll_x = 0.0;
        }
    }

    // ------------------------------------------------------------ mouse

    /// Text position under a point in the editor area (clamped to rows).
    pub fn hit_pos(&self, x: f32, y: f32) -> Option<usize> {
        let l = &self.layout;
        if l.rows.is_empty() || l.char_w <= 0.0 {
            return None;
        }
        let row = l.rows.iter().rev().find(|r| y >= r.y).copied().unwrap_or(l.rows[0]);
        let b = &self.doc().buf;
        let text = b.line_text(row.line);
        let tab = self.settings.tab_width;
        let base = layout::col_of(&text, row.start, tab);
        let scroll = if l.wrap_cells.is_some() { 0.0 } else { self.doc().scroll_x };
        let col = ((x - l.text_x + scroll) / l.char_w).max(0.0);
        let idx = layout::idx_at(&text, (row.start, row.end), base, base + col.round() as usize, tab);
        Some(b.line_start(row.line) + idx)
    }

    /// Cell column under x (for column selection).
    fn hit_col(&self, x: f32) -> usize {
        let scroll = if self.layout.wrap_cells.is_some() { 0.0 } else { self.doc().scroll_x };
        ((x - self.layout.text_x + scroll) / self.layout.char_w.max(1.0)).max(0.0).round() as usize
    }

    pub fn mouse_down(&mut self, x: f32, y: f32, ctrl: bool, shift: bool, alt: bool) {
        self.input.mouse = (x, y);
        let l = self.layout.clone();
        self.focus = Focus::Editor;
        if l.vscroll.contains(x, y) {
            self.input.drag = Some(Drag::VScroll { grab: y });
            self.scrollbar_jump(y, true);
            return;
        }
        if l.hscroll.contains(x, y) {
            self.input.drag = Some(Drag::HScroll { grab: x });
            self.scrollbar_jump(x, false);
            return;
        }
        // Click count (double/triple click).
        let now = Instant::now();
        let count = match self.input.last_click {
            Some((t, px, py, c)) if now.duration_since(t).as_millis() < 500 && (px - x).abs() < 4.0 && (py - y).abs() < 4.0 => c % 3 + 1,
            _ => 1,
        };
        self.input.last_click = Some((now, x, y, count));
        // Gutter: fold markers and line selection.
        if x < l.text_x - 2.0 && l.gutter.contains(x, y) {
            let Some(row) = l.rows.iter().rev().find(|r| y >= r.y).copied() else { return };
            if self.settings.fold_margin && x >= l.fold_x {
                self.ensure_folds(true);
                if self.doc_mut().toggle_fold(row.line) {
                    return;
                }
            }
            let b = &mut self.doc_mut().buf;
            let start = b.line_start(row.line);
            let end = b.line_start(row.line + 1).max(b.line_end(row.line));
            let anchor = if shift { b.primary_sel().anchor } else { start };
            b.set_sels(vec![Sel::new(anchor, end)], 0);
            self.input.drag = Some(Drag::Select { anchor: start, unit: 3, word: (start, end) });
            return;
        }
        let Some(pos) = self.hit_pos(x, y) else { return };
        let b = &mut self.docs[self.active].buf;
        if alt {
            let line = b.line_of(pos);
            let col = self.hit_col(x);
            self.input.drag = Some(Drag::Column { line, col });
            self.doc_mut().buf.set_caret(pos);
            return;
        }
        match count {
            2 => {
                let (s, e) = b.word_at(pos).unwrap_or((pos, pos));
                b.set_sels(vec![Sel::new(s, e)], 0);
                self.input.drag = Some(Drag::Select { anchor: s, unit: 2, word: (s, e) });
            }
            3 => {
                let line = b.line_of(pos);
                let (s, e) = (b.line_start(line), b.line_start(line + 1).max(b.line_end(line)));
                b.set_sels(vec![Sel::new(s, e)], 0);
                self.input.drag = Some(Drag::Select { anchor: s, unit: 3, word: (s, e) });
            }
            _ => {
                if ctrl {
                    let mut sels = b.sels.clone();
                    if let Some(i) = sels.iter().position(|s| s.is_empty() && s.head == pos) {
                        if sels.len() > 1 {
                            sels.remove(i);
                        }
                    } else {
                        sels.push(Sel::caret(pos));
                    }
                    let n = sels.len() - 1;
                    b.set_sels(sels, n);
                } else if shift {
                    let a = b.primary_sel().anchor;
                    b.set_sels(vec![Sel::new(a, pos)], 0);
                } else {
                    b.set_caret(pos);
                }
                let anchor = if shift { self.doc().buf.primary_sel().anchor } else { pos };
                if !ctrl {
                    self.input.drag = Some(Drag::Select { anchor, unit: 1, word: (pos, pos) });
                }
            }
        }
        self.doc_mut().goals.clear();
    }

    /// Mouse moved while a button is held. Returns true if the view changed.
    pub fn mouse_drag(&mut self, x: f32, y: f32) -> bool {
        self.input.mouse = (x, y);
        let Some(drag) = self.input.drag else { return false };
        match drag {
            Drag::VScroll { .. } => self.scrollbar_jump(y, true),
            Drag::HScroll { .. } => self.scrollbar_jump(x, false),
            Drag::Select { anchor, unit, word } => {
                let Some(pos) = self.hit_pos(x, y) else { return false };
                let b = &mut self.docs[self.active].buf;
                let sel = match unit {
                    2 => {
                        let (s, e) = b.word_at(pos).unwrap_or((pos, pos));
                        if pos < word.0 {
                            Sel::new(word.1, s)
                        } else {
                            Sel::new(word.0, e.max(word.1))
                        }
                    }
                    3 => {
                        let line = b.line_of(pos);
                        if pos < word.0 {
                            Sel::new(word.1, b.line_start(line))
                        } else {
                            Sel::new(word.0, b.line_start(line + 1).max(b.line_end(line)))
                        }
                    }
                    _ => Sel::new(anchor, pos),
                };
                b.set_sels(vec![sel], 0);
            }
            Drag::Column { line, col } => {
                let Some(pos) = self.hit_pos(x, y) else { return false };
                let tab = self.settings.tab_width;
                let b = &mut self.docs[self.active].buf;
                let cur_line = b.line_of(pos);
                let cur_col = self.layout.text_x;
                let _ = cur_col;
                let col2 = ((x - self.layout.text_x + if self.layout.wrap_cells.is_some() { 0.0 } else { self.docs[self.active].scroll_x }) / self.layout.char_w.max(1.0)).max(0.0).round() as usize;
                let b = &mut self.docs[self.active].buf;
                let (l0, l1) = (line.min(cur_line), line.max(cur_line));
                let sels: Vec<Sel> = (l0..=l1)
                    .map(|l| {
                        let t = b.line_text(l);
                        let n = t.chars().count();
                        let a = layout::idx_at(&t, (0, n), 0, col, tab);
                        let h = layout::idx_at(&t, (0, n), 0, col2, tab);
                        let ls = b.line_start(l);
                        Sel::new(ls + a, ls + h)
                    })
                    .collect();
                let prim = sels.len() - 1;
                b.set_sels(sels, prim);
            }
        }
        true
    }

    pub fn mouse_up(&mut self) {
        self.input.drag = None;
    }

    /// Whether a selection drag is outside the editor (needs auto-scroll).
    pub fn autoscroll_needed(&self) -> Option<isize> {
        if !matches!(self.input.drag, Some(Drag::Select { .. } | Drag::Column { .. })) {
            return None;
        }
        let r = self.layout.rect;
        let y = self.input.mouse.1;
        if y < r.y {
            Some(-1)
        } else if y > r.bottom() {
            Some(1)
        } else {
            None
        }
    }

    fn scrollbar_jump(&mut self, v: f32, vertical: bool) {
        let l = &self.layout;
        if vertical {
            let r = l.vscroll;
            let frac = ((v - r.y) / r.h.max(1.0)).clamp(0.0, 1.0);
            let total = self.doc().buf.line_count();
            let target = ((total as f32 - l.rows_visible as f32 / 2.0).max(0.0) * frac) as usize;
            let mut line = target.min(total.saturating_sub(1));
            while line > 0 && self.doc().is_hidden(line) {
                line -= 1;
            }
            let d = self.doc_mut();
            d.top = line;
            d.top_row = 0;
        } else {
            let r = l.hscroll;
            let frac = ((v - r.x) / r.w.max(1.0)).clamp(0.0, 1.0);
            let content = l.max_line_cells as f32 * l.char_w;
            let view = l.rect.w - (l.text_x - l.rect.x);
            self.doc_mut().scroll_x = ((content - view).max(0.0) * frac).max(0.0);
        }
    }

    /// Mouse wheel: `lines` rows (positive = down); Ctrl zooms, Shift scrolls sideways.
    pub fn wheel(&mut self, lines: f32, ctrl: bool, shift: bool) {
        if ctrl {
            let cmd = if lines < 0.0 { Command::ZoomIn } else { Command::ZoomOut };
            match cmd {
                Command::ZoomIn => self.settings.zoom = (self.settings.zoom + 10).min(400),
                _ => self.settings.zoom = (self.settings.zoom - 10).max(30),
            }
            return;
        }
        if shift && self.layout.wrap_cells.is_none() {
            let d = self.doc_mut();
            d.scroll_x = (d.scroll_x + lines * 40.0).max(0.0);
            return;
        }
        self.scroll_rows(lines.round() as isize);
    }

}

/// Keys for a single-line field. Returns true if handled.
fn field_key(edit: &mut crate::ui::lineedit::LineEdit, k: &KeyInput, lm: LMods, history: &[String], host: &dyn Host) -> bool {
    if k.ctrl && !k.alt {
        match k.key {
            Key::Char('a') => {
                edit.select_all();
                return true;
            }
            Key::Char('c') | Key::Insert => {
                host.clipboard_set(&edit.selected_text());
                return true;
            }
            Key::Char('x') => {
                let t = edit.cut();
                host.clipboard_set(&t);
                return true;
            }
            Key::Char('v') => {
                if let Some(t) = host.clipboard_get() {
                    edit.insert(&t);
                }
                return true;
            }
            _ => {}
        }
    }
    if k.shift && k.key == Key::Insert {
        if let Some(t) = host.clipboard_get() {
            edit.insert(&t);
        }
        return true;
    }
    let lk = match k.key {
        Key::Left => LineKey::Left,
        Key::Right => LineKey::Right,
        Key::Home => LineKey::Home,
        Key::End => LineKey::End,
        Key::Backspace => LineKey::Backspace,
        Key::Delete => LineKey::Delete,
        Key::Up => LineKey::Up,
        Key::Down => LineKey::Down,
        _ => return false,
    };
    edit.key(lk, lm, history);
    true
}
