//! Writing in a sticky note or an annotation: the text, the caret, a
//! selection, and the text broken into lines that fit the widget.

use rt_gfx::{FontId, Gfx};

/// Longest text a note keeps.
pub const MAX_TEXT: usize = 20_000;

/// A line of the laid-out text: bytes `start..end` of the text (without the
/// line break).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub start: usize,
    pub end: usize,
}

/// Breaks `text` into lines no wider than `width`: at line breaks, between
/// words, and inside words longer than a line.
pub fn layout(g: &mut Gfx, text: &str, width: f32, px: f32, font: FontId) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut para_start = 0;
    for para in text.split('\n') {
        let base = para_start;
        para_start += para.len() + 1;
        if para.is_empty() {
            lines.push(Line { start: base, end: base });
            continue;
        }
        let mut start = 0; // within para
        while start < para.len() {
            let rest = &para[start..];
            // The longest prefix that fits.
            let mut x = 0.0;
            let mut fit = rest.len();
            let mut last_space = None;
            for (i, c) in rest.char_indices() {
                let adv = g.advance(c, px, font);
                if x + adv > width && i > 0 {
                    fit = i;
                    break;
                }
                x += adv;
                if c == ' ' {
                    last_space = Some(i + 1);
                }
            }
            let mut end = fit;
            if fit < rest.len() {
                if let Some(sp) = last_space.filter(|s| *s > 0 && *s <= fit) {
                    end = sp;
                }
            }
            lines.push(Line { start: base + start, end: base + start + end });
            start += end;
        }
    }
    lines
}

/// The line holding byte `i` (a caret at a wrap point goes to the next line).
pub fn line_of(lines: &[Line], i: usize) -> usize {
    let mut found = 0;
    for (n, l) in lines.iter().enumerate() {
        if i >= l.start && i <= l.end {
            found = n;
            if i < l.end {
                break;
            }
        }
    }
    found
}

/// The text being written, with the caret and the selection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Editor {
    pub text: String,
    /// Byte index of the caret.
    pub caret: usize,
    /// The other end of the selection, if any.
    pub anchor: Option<usize>,
    /// Where the caret wants to be across lines (Up and Down).
    pub goal_x: Option<f32>,
}

impl Editor {
    pub fn new(text: &str) -> Editor {
        Editor { text: text.to_string(), caret: text.len(), anchor: None, goal_x: None }
    }

    /// The selected range, in order.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.caret).then(|| (a.min(self.caret), a.max(self.caret)))
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|(a, b)| &self.text[a..b])
    }

    fn delete_selection(&mut self) -> bool {
        match self.selection() {
            Some((a, b)) => {
                self.text.replace_range(a..b, "");
                self.caret = a;
                self.anchor = None;
                true
            }
            None => {
                self.anchor = None;
                false
            }
        }
    }

    pub fn insert(&mut self, s: &str) {
        self.delete_selection();
        let s: String = s.chars().filter(|c| *c == '\n' || *c == '\t' || !c.is_control()).map(|c| if c == '\t' { ' ' } else { c }).collect();
        let room = MAX_TEXT.saturating_sub(self.text.len());
        let mut cut = s.len().min(room);
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        self.text.insert_str(self.caret, &s[..cut]);
        self.caret += cut;
        self.goal_x = None;
    }

    fn prev(&self, i: usize) -> usize {
        self.text[..i].char_indices().next_back().map_or(0, |(p, _)| p)
    }

    fn next(&self, i: usize) -> usize {
        self.text[i..].chars().next().map_or(i, |c| i + c.len_utf8())
    }

    /// Backspace (`word`: Ctrl+Backspace).
    pub fn backspace(&mut self, word: bool) {
        if self.delete_selection() || self.caret == 0 {
            return;
        }
        let to = if word { self.word_left(self.caret) } else { self.prev(self.caret) };
        self.text.replace_range(to..self.caret, "");
        self.caret = to;
        self.goal_x = None;
    }

    pub fn delete(&mut self, word: bool) {
        if self.delete_selection() || self.caret >= self.text.len() {
            return;
        }
        let to = if word { self.word_right(self.caret) } else { self.next(self.caret) };
        self.text.replace_range(self.caret..to, "");
        self.goal_x = None;
    }

    fn word_left(&self, mut i: usize) -> usize {
        while i > 0 && self.text[..i].ends_with(char::is_whitespace) {
            i = self.prev(i);
        }
        while i > 0 && !self.text[..i].ends_with(char::is_whitespace) {
            i = self.prev(i);
        }
        i
    }

    fn word_right(&self, mut i: usize) -> usize {
        while i < self.text.len() && !self.text[i..].starts_with(char::is_whitespace) {
            i = self.next(i);
        }
        while i < self.text.len() && self.text[i..].starts_with(char::is_whitespace) {
            i = self.next(i);
        }
        i
    }

    /// Moves the caret to `i`, extending the selection with Shift.
    pub fn move_to(&mut self, i: usize, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = i.min(self.text.len());
    }

    pub fn left(&mut self, select: bool, word: bool) {
        let i = match self.selection() {
            Some((a, _)) if !select => a,
            _ if word => self.word_left(self.caret),
            _ => self.prev(self.caret),
        };
        self.move_to(i, select);
        self.goal_x = None;
    }

    pub fn right(&mut self, select: bool, word: bool) {
        let i = match self.selection() {
            Some((_, b)) if !select => b,
            _ if word => self.word_right(self.caret),
            _ => self.next(self.caret),
        };
        self.move_to(i, select);
        self.goal_x = None;
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    /// Selects the word around `i` (double-click).
    pub fn select_word(&mut self, i: usize) {
        let i = i.min(self.text.len());
        let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
        let mut a = i;
        while a > 0 && self.text[..a].chars().next_back().is_some_and(is_word) {
            a = self.prev(a);
        }
        let mut b = i;
        while b < self.text.len() && self.text[b..].chars().next().is_some_and(is_word) {
            b = self.next(b);
        }
        self.anchor = Some(a);
        self.caret = b;
    }

    /// Byte index nearest to `x` on `line` (x from the line's left edge).
    pub fn index_at(&self, g: &mut Gfx, line: &Line, x: f32, px: f32, font: FontId) -> usize {
        let mut pos = 0.0;
        for (i, c) in self.text[line.start..line.end].char_indices() {
            let adv = g.advance(c, px, font);
            if x < pos + adv / 2.0 {
                return line.start + i;
            }
            pos += adv;
        }
        // The end of a wrapped line is the start of the next: stay before
        // its trailing space.
        let end = line.end;
        if end > line.start && end < self.text.len() && !self.text[end..].starts_with('\n') && self.text[..end].ends_with(' ') {
            return end - 1;
        }
        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_and_deleting() {
        let mut e = Editor::new("olá");
        e.insert(" mundo");
        assert_eq!(e.text, "olá mundo");
        e.backspace(true);
        assert_eq!(e.text, "olá ");
        e.left(false, false);
        e.backspace(false);
        assert_eq!(e.text, "ol ");
        e.select_all();
        e.insert("a\tb\u{7}c");
        assert_eq!(e.text, "a bc");
        e.move_to(1, false);
        e.right(true, false);
        e.right(true, false);
        assert_eq!(e.selected_text(), Some(" b"));
        e.delete(false);
        assert_eq!(e.text, "ac");
        e.select_word(1);
        assert_eq!(e.selected_text(), Some("ac"));
        e.move_to(0, false);
        e.delete(true);
        assert_eq!(e.text, "");
        e.insert(&"x".repeat(MAX_TEXT + 10));
        assert_eq!(e.text.len(), MAX_TEXT);
    }

    #[test]
    fn lines() {
        let Ok(mut g) = Gfx::new() else { return };
        let text = "Comprar pão e leite amanhã de manhã\n\nfim";
        let w = g.measure("Comprar pão e ", 14.0, FontId::Regular) + 1.0;
        let lines = layout(&mut g, text, w, 14.0, FontId::Regular);
        let parts: Vec<&str> = lines.iter().map(|l| &text[l.start..l.end]).collect();
        assert_eq!(parts[0], "Comprar pão e ");
        assert!(parts.contains(&"") && parts.last() == Some(&"fim"), "{parts:?}");
        assert!(lines.iter().all(|l| g.measure(text[l.start..l.end].trim_end(), 14.0, FontId::Regular) <= w));
        // A word longer than a line is cut.
        let long = layout(&mut g, "abcdefghijklmnopqrstuvwxyz", 40.0, 14.0, FontId::Regular);
        assert!(long.len() > 2);
        assert_eq!(line_of(&lines, 0), 0);
        assert_eq!(line_of(&lines, text.len()), lines.len() - 1);
        let e = Editor::new(text);
        assert_eq!(e.index_at(&mut g, &lines[0], 0.0, 14.0, FontId::Regular), 0);
        assert_eq!(e.index_at(&mut g, &lines[0], 9999.0, 14.0, FontId::Regular), lines[0].end - 1);
    }
}
