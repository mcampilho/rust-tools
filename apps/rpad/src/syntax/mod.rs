//! Syntax highlighting, folding and outlines. Language definitions and the
//! lexer come from `rt-syntax`.

pub mod fold;
pub mod outline;

pub use rt_syntax::{lang, lexer};

pub use lang::{def, detect, Lang};
pub use lexer::{Span, State, Tok};
use ropey::Rope;

/// Per-document cache of lexer states at the start of each line, filled
/// lazily up to the lines that are displayed and invalidated from the first
/// changed line, so scrolling and typing never re-lex the whole file.
pub struct Highlighter {
    pub lang: Lang,
    pub enabled: bool,
    states: Vec<State>,
    line_buf: String,
}

impl Highlighter {
    pub fn new(lang: Lang) -> Highlighter {
        Highlighter { lang, enabled: true, states: vec![0], line_buf: String::new() }
    }

    pub fn set_lang(&mut self, lang: Lang) {
        if lang != self.lang {
            self.lang = lang;
            self.invalidate(0);
        }
    }

    /// Drops cached states from `line` on.
    pub fn invalidate(&mut self, line: usize) {
        self.states.truncate(line + 1);
        if self.states.is_empty() {
            self.states.push(0);
        }
    }

    fn line_into(text: &Rope, line: usize, buf: &mut String) {
        buf.clear();
        if line < text.len_lines() {
            for chunk in text.line(line).chunks() {
                buf.push_str(chunk);
            }
            if buf.ends_with('\n') {
                buf.pop();
            }
        }
    }

    /// Lexer state at the start of `line`.
    pub fn state_at(&mut self, text: &Rope, line: usize) -> State {
        let d = def(self.lang);
        let mut spans = Vec::new();
        let target = line.min(text.len_lines());
        while self.states.len() <= target {
            let l = self.states.len() - 1;
            Self::line_into(text, l, &mut self.line_buf);
            let st = lexer::lex_line(d, &self.line_buf, self.states[l], &mut spans);
            self.states.push(st);
        }
        self.states[target]
    }

    /// Token spans (byte offsets) of `line` into `out`.
    pub fn line_spans(&mut self, text: &Rope, line: usize, out: &mut Vec<Span>) {
        out.clear();
        if !self.enabled || self.lang == Lang::Plain {
            return;
        }
        let st = self.state_at(text, line);
        Self::line_into(text, line, &mut self.line_buf);
        let next = lexer::lex_line(def(self.lang), &self.line_buf, st, out);
        if self.states.len() == line + 1 {
            self.states.push(next);
        }
    }

}
