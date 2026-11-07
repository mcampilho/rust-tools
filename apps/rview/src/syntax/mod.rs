//! Syntax highlighting for source files shown as text and for code blocks
//! in Markdown/HTML. Languages, the lexer and the token colours come from
//! `rt-syntax`; this module adds the line-state cache for text views.

pub use rt_syntax::{detect, from_tag, highlight, lang, lexer, tok_from, Lang, Span, State, Tok};
use crate::ui::gfx::Color;

/// Above this size, text files are not highlighted.
pub const MAX_HIGHLIGHT_SIZE: usize = 16 << 20;

/// Color of a token (None for plain text, which keeps the text color).
pub fn color(t: Tok, dark: bool) -> Option<Color> {
    rt_syntax::rgb(t, dark).map(Color::rgb)
}

/// Lexer states at the start of each line, filled lazily up to the lines
/// that are displayed.
pub struct Highlighter {
    pub lang: Lang,
    states: Vec<State>,
}

impl Highlighter {
    pub fn new(lang: Lang) -> Highlighter {
        Highlighter { lang, states: vec![0] }
    }

    /// Token spans (byte offsets) of `line`; `get` returns a line's text.
    pub fn spans<'a>(&mut self, get: &dyn Fn(usize) -> &'a str, line: usize, out: &mut Vec<Span>) {
        out.clear();
        if self.lang == Lang::Plain {
            return;
        }
        let def = lang::def(self.lang);
        let mut scratch = Vec::new();
        while self.states.len() <= line {
            let l = self.states.len() - 1;
            let st = lexer::lex_line(def, get(l), self.states[l], &mut scratch);
            self.states.push(st);
        }
        let next = lexer::lex_line(def, get(line), self.states[line], out);
        if self.states.len() == line + 1 {
            self.states.push(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_line_state() {
        let lines = ["/* start", "still comment", "end */ let x = 1;"];
        let mut h = Highlighter::new(Lang::Rust);
        let mut out = Vec::new();
        h.spans(&|i| lines[i], 1, &mut out);
        assert_eq!(out[0].tok, Tok::Comment);
        h.spans(&|i| lines[i], 2, &mut out);
        assert!(out.iter().any(|s| s.tok == Tok::Keyword));
        assert_eq!(color(Tok::Keyword, false), Some(Color::rgb(0x0000FF)));
        assert_eq!(color(Tok::Text, true), None);
    }
}
