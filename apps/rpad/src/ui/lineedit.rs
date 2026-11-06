//! Single-line text field used by the find bar, command palette and
//! prompts: caret, selection, word movement, clipboard and history.

#[derive(Clone, Debug, Default)]
pub struct LineEdit {
    pub text: String,
    /// Caret and selection anchor as char indices.
    pub caret: usize,
    pub anchor: usize,
    /// Horizontal scroll in pixels (kept by the renderer).
    pub scroll: f32,
    history_pos: Option<usize>,
}

/// Keys a line edit understands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LineKey {
    Left,
    Right,
    Home,
    End,
    Backspace,
    Delete,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
}

fn byte_of(s: &str, idx: usize) -> usize {
    s.char_indices().nth(idx).map_or(s.len(), |(b, _)| b)
}

impl LineEdit {
    pub fn new(text: &str) -> LineEdit {
        let n = text.chars().count();
        LineEdit { text: text.to_string(), caret: n, anchor: 0, scroll: 0.0, history_pos: None }
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn set(&mut self, text: &str) {
        self.text = text.to_string();
        self.caret = self.len();
        self.anchor = 0;
    }

    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.caret = self.len();
    }

    pub fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    pub fn selected_text(&self) -> String {
        let (a, b) = self.selection();
        self.text.chars().skip(a).take(b - a).collect()
    }

    fn delete_selection(&mut self) -> bool {
        let (a, b) = self.selection();
        if a == b {
            return false;
        }
        let (ba, bb) = (byte_of(&self.text, a), byte_of(&self.text, b));
        self.text.replace_range(ba..bb, "");
        self.caret = a;
        self.anchor = a;
        true
    }

    pub fn insert(&mut self, s: &str) {
        let s: String = s.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        self.delete_selection();
        let b = byte_of(&self.text, self.caret);
        self.text.insert_str(b, &s);
        self.caret += s.chars().count();
        self.anchor = self.caret;
    }

    fn word_boundary(&self, from: usize, forward: bool) -> usize {
        let chars: Vec<char> = self.text.chars().collect();
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut i = from;
        if forward {
            while i < chars.len() && !word(chars[i]) {
                i += 1;
            }
            while i < chars.len() && word(chars[i]) {
                i += 1;
            }
        } else {
            while i > 0 && !word(chars[i - 1]) {
                i -= 1;
            }
            while i > 0 && word(chars[i - 1]) {
                i -= 1;
            }
        }
        i
    }

    /// Applies a key. Returns true if the text changed.
    pub fn key(&mut self, k: LineKey, m: Mods, history: &[String]) -> bool {
        let len = self.len();
        let mv = |s: &mut Self, to: usize| {
            s.caret = to.min(len);
            if !m.shift {
                s.anchor = s.caret;
            }
        };
        match k {
            LineKey::Left => {
                let (a, b) = self.selection();
                if a != b && !m.shift {
                    mv(self, a);
                } else {
                    let t = if m.ctrl { self.word_boundary(self.caret, false) } else { self.caret.saturating_sub(1) };
                    mv(self, t);
                }
            }
            LineKey::Right => {
                let (a, b) = self.selection();
                if a != b && !m.shift {
                    mv(self, b);
                } else {
                    let t = if m.ctrl { self.word_boundary(self.caret, true) } else { self.caret + 1 };
                    mv(self, t);
                }
            }
            LineKey::Home => mv(self, 0),
            LineKey::End => mv(self, len),
            LineKey::Backspace => {
                if !self.delete_selection() && self.caret > 0 {
                    let from = if m.ctrl { self.word_boundary(self.caret, false) } else { self.caret - 1 };
                    self.anchor = from;
                    self.delete_selection();
                }
                return true;
            }
            LineKey::Delete => {
                if !self.delete_selection() && self.caret < len {
                    let to = if m.ctrl { self.word_boundary(self.caret, true) } else { self.caret + 1 };
                    self.anchor = to;
                    self.delete_selection();
                }
                return true;
            }
            LineKey::Up | LineKey::Down => {
                if history.is_empty() {
                    return false;
                }
                let pos = match (self.history_pos, k == LineKey::Up) {
                    (None, true) => 0,
                    (None, false) => return false,
                    (Some(p), true) => (p + 1).min(history.len() - 1),
                    (Some(0), false) => {
                        self.history_pos = None;
                        self.set("");
                        return true;
                    }
                    (Some(p), false) => p - 1,
                };
                self.history_pos = Some(pos);
                self.set(&history[pos]);
                self.select_all();
                return true;
            }
        }
        false
    }

    pub fn cut(&mut self) -> String {
        let s = self.selected_text();
        self.delete_selection();
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: Mods = Mods { ctrl: false, shift: false };
    const S: Mods = Mods { ctrl: false, shift: true };
    const C: Mods = Mods { ctrl: true, shift: false };

    #[test]
    fn editing() {
        let mut e = LineEdit::new("");
        for c in "héllo world".chars() {
            e.insert(&c.to_string());
        }
        assert_eq!(e.text, "héllo world");
        e.key(LineKey::Backspace, C, &[]);
        assert_eq!(e.text, "héllo ");
        e.key(LineKey::Home, N, &[]);
        e.key(LineKey::Right, S, &[]);
        e.key(LineKey::Right, S, &[]);
        assert_eq!(e.selected_text(), "hé");
        e.insert("HE");
        assert_eq!(e.text, "HEllo ");
        e.key(LineKey::End, N, &[]);
        e.key(LineKey::Left, C, &[]);
        assert_eq!(e.caret, 0);
        e.insert("a\nb");
        assert_eq!(e.text, "abHEllo ");
        e.select_all();
        e.key(LineKey::Delete, N, &[]);
        assert!(e.is_empty());
    }

    #[test]
    fn history() {
        let h = vec!["new".to_string(), "old".to_string()];
        let mut e = LineEdit::new("");
        e.key(LineKey::Up, N, &h);
        assert_eq!(e.text, "new");
        e.key(LineKey::Up, N, &h);
        assert_eq!(e.text, "old");
        e.key(LineKey::Down, N, &h);
        assert_eq!(e.text, "new");
        e.key(LineKey::Down, N, &h);
        assert_eq!(e.text, "");
    }
}
