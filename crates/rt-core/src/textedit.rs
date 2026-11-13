//! A line of text being edited: the text, the caret and the other end of
//! the selection (byte offsets, always on character boundaries), and what the
//! keys and the pointer do to them. No drawing here: the app draws the text,
//! the selection and the caret, and turns a click into an offset.

/// Words end at spaces and at `,` and `;`.
pub fn is_separator(c: char) -> bool {
    c.is_whitespace() || c == ',' || c == ';'
}

/// `at` within `text`, on the start of a character.
pub fn boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// Where the caret goes from `at` one character (or `word`) forward or back.
pub fn step(text: &str, at: usize, forward: bool, word: bool) -> usize {
    if forward {
        let mut it = text[at..].char_indices().peekable();
        if !word {
            return it.next().map_or(at, |(_, c)| at + c.len_utf8());
        }
        // Past the separators, then past the word.
        while it.next_if(|&(_, c)| is_separator(c)).is_some() {}
        while it.next_if(|&(_, c)| !is_separator(c)).is_some() {}
        it.peek().map_or(text.len(), |&(i, _)| at + i)
    } else {
        let mut it = text[..at].char_indices().rev().peekable();
        if !word {
            return it.next().map_or(at, |(i, _)| i);
        }
        let mut to = at;
        while let Some((i, _)) = it.next_if(|&(_, c)| is_separator(c)) {
            to = i;
        }
        while let Some((i, _)) = it.next_if(|&(_, c)| !is_separator(c)) {
            to = i;
        }
        to
    }
}

/// A line of text with a caret and a selection (from `anchor` to `caret`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextEdit {
    pub text: String,
    pub caret: usize,
    pub anchor: usize,
}

impl TextEdit {
    pub fn new(text: &str) -> TextEdit {
        TextEdit { text: text.to_string(), caret: text.len(), anchor: text.len() }
    }

    /// The selection as (start, end), when there is one.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let (a, b) = (boundary(&self.text, self.anchor), boundary(&self.text, self.caret));
        (a != b).then(|| (a.min(b), a.max(b)))
    }

    pub fn selected_text(&self) -> &str {
        self.selection().map_or("", |(a, b)| &self.text[a..b])
    }

    /// Takes the selection out; whether there was one.
    fn take_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else { return false };
        self.text.replace_range(a..b, "");
        self.caret = a;
        self.anchor = a;
        true
    }

    /// Types (or pastes) `s` at the caret, in place of the selection. Line
    /// breaks become spaces; other control characters are dropped.
    pub fn insert(&mut self, s: &str) {
        let s: String = s.chars().map(|c| if c == '\n' || c == '\t' { ' ' } else { c }).filter(|c| !c.is_control()).collect();
        let s = s.trim_end_matches(' ');
        self.take_selection();
        let at = boundary(&self.text, self.caret);
        self.text.insert_str(at, s);
        self.caret = at + s.len();
        self.anchor = self.caret;
    }

    /// Deletes the selection, or else the character (`word`: the word) before
    /// the caret (`forward`: after it).
    pub fn delete(&mut self, forward: bool, word: bool) {
        if self.take_selection() {
            return;
        }
        let at = boundary(&self.text, self.caret);
        let to = step(&self.text, at, forward, word);
        let (a, b) = (at.min(to), at.max(to));
        self.text.replace_range(a..b, "");
        self.caret = a;
        self.anchor = a;
    }

    /// Takes the selection out and returns it (Ctrl+X).
    pub fn cut(&mut self) -> String {
        let s = self.selected_text().to_string();
        self.take_selection();
        s
    }

    /// Moves the caret a character (`word`: a word) left or right, or to the
    /// start or end; `extend` keeps the other end of the selection.
    pub fn move_caret(&mut self, back: bool, to_end: bool, word: bool, extend: bool) {
        let at = boundary(&self.text, self.caret);
        self.caret = match (to_end, self.selection()) {
            (true, _) => {
                if back {
                    0
                } else {
                    self.text.len()
                }
            }
            // Without Shift, an arrow first goes to that side of the selection.
            (false, Some((a, b))) if !extend && !word => {
                if back {
                    a
                } else {
                    b
                }
            }
            _ => step(&self.text, at, !back, word),
        };
        if !extend {
            self.anchor = self.caret;
        }
    }

    /// Puts the caret at `at`; `extend` keeps the other end (a drag, or
    /// Shift with a click).
    pub fn set_caret(&mut self, at: usize, extend: bool) {
        self.caret = boundary(&self.text, at);
        if !extend {
            self.anchor = self.caret;
        }
    }

    /// Selects the word around `at` (a double click).
    pub fn select_word(&mut self, at: usize) {
        let at = boundary(&self.text, at);
        let t = &self.text;
        let start = t[..at].rfind(is_separator).map_or(0, |i| i + t[i..].chars().next().map_or(1, char::len_utf8));
        let end = t[at..].find(is_separator).map_or(t.len(), |i| at + i);
        self.anchor = start;
        self.caret = end.max(start);
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.text.len();
    }

    pub fn clear(&mut self) {
        *self = TextEdit::default();
    }
}

/// The byte offset in `text` nearest to `x` pixels from its start, given the
/// width of each character (`advance`).
pub fn offset_at(text: &str, x: f32, mut advance: impl FnMut(char) -> f32) -> usize {
    let mut best = (0, x.abs());
    let mut w = 0.0;
    for (i, c) in text.char_indices() {
        w += advance(c);
        let end = i + c.len_utf8();
        if (x - w).abs() < best.1 {
            best = (end, (x - w).abs());
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing() {
        let mut t = TextEdit::new("bit defender");
        t.move_caret(true, false, true, false);
        assert_eq!(t.caret, 4);
        t.move_caret(false, true, false, true);
        assert_eq!(t.selected_text(), "defender");
        t.insert("ctrl\nv");
        assert_eq!(t.text, "bit ctrl v");
        t.select_word(5);
        assert_eq!(t.selected_text(), "ctrl");
        assert_eq!(t.cut(), "ctrl");
        assert_eq!(t.text, "bit  v");
        t.delete(true, false);
        assert_eq!(t.text, "bit v");
        t.move_caret(false, true, false, false);
        t.delete(false, true);
        assert_eq!(t.text, "bit ");
        t.select_all();
        t.insert("é");
        assert_eq!((t.text.as_str(), t.caret), ("é", 2));
        t.set_caret(1, false);
        assert_eq!(t.caret, 0);
        assert_eq!(offset_at("abc", 15.0, |_| 10.0), 1);
        assert_eq!(offset_at("abc", 26.0, |_| 10.0), 3);
    }
}
