//! Syntax highlighting for source files shown as text and for code blocks
//! in Markdown/HTML. The lexer works a line at a time from the state left
//! by the previous line (block comments, multi-line strings).

pub mod lang;
pub mod lexer;

pub use lang::{detect, Lang};
pub use lexer::{Span, State, Tok};
use crate::ui::gfx::Color;

/// Above this size, text files are not highlighted.
pub const MAX_HIGHLIGHT_SIZE: usize = 16 << 20;

fn index(t: Tok) -> usize {
    t as usize
}

const LIGHT: [u32; 24] = [
    0x1B1B1B, 0x0000FF, 0x267F99, 0x0070C1, 0x008000, 0xA31515, 0x098658, 0xAF00DB, 0x1B1B1B, 0x795E26, 0x001080, 0x795E26, 0x800000, 0xE50000,
    0xC5060B, 0x0451A5, 0x6F42C1, 0x9A6700, 0xA31515, 0x0068B8, 0x0451A5, 0xAF00DB, 0x116329, 0xC42B1C,
];
const DARK: [u32; 24] = [
    0xD4D4D4, 0x569CD6, 0x4EC9B0, 0x4FC1FF, 0x6A9955, 0xCE9178, 0xB5CEA8, 0xC586C0, 0xD4D4D4, 0xDCDCAA, 0x9CDCFE, 0xD7BA7D, 0x569CD6, 0x9CDCFE,
    0xD7BA7D, 0x569CD6, 0xC5A5E5, 0xF0C674, 0xCE9178, 0x4DAAFC, 0x9CDCFE, 0xC586C0, 0x89D185, 0xF48771,
];

/// Color of a token (None for plain text, which keeps the text color).
pub fn color(t: Tok, dark: bool) -> Option<Color> {
    if matches!(t, Tok::Text | Tok::Punct) {
        return None;
    }
    Some(Color::rgb(if dark { DARK } else { LIGHT }[index(t)]))
}

/// Token from its index (`Style::tok` stores index + 1).
pub fn tok_from(i: u8) -> Option<Tok> {
    const ALL: [Tok; 24] = [
        Tok::Text, Tok::Keyword, Tok::Type, Tok::Constant, Tok::Comment, Tok::String, Tok::Number, Tok::Preproc, Tok::Punct, Tok::Function,
        Tok::Variable, Tok::Meta, Tok::Tag, Tok::Attr, Tok::Entity, Tok::Heading, Tok::Emphasis, Tok::Strong, Tok::Code, Tok::Link, Tok::Key,
        Tok::Section, Tok::Inserted, Tok::Deleted,
    ];
    ALL.get(i as usize).copied()
}

/// Language of a code block from its fence info (` ```rust `) or class
/// (`language-js`, `lang-py`, `hljs python`).
pub fn from_tag(tag: &str) -> Lang {
    let t = tag.trim().to_ascii_lowercase();
    let t = t.split(|c: char| c.is_whitespace() || c == ',' || c == '{').next().unwrap_or("");
    let t = t.strip_prefix("language-").or_else(|| t.strip_prefix("lang-")).unwrap_or(t);
    let alias = match t {
        "rs" => "rust",
        "c++" | "cc" | "cxx" | "hpp" => "cpp",
        "cs" | "c#" => "csharp",
        "js" | "jsx" | "mjs" | "node" => "javascript",
        "ts" | "tsx" => "typescript",
        "golang" => "go",
        "py" | "python3" => "python",
        "htm" | "xhtml" => "html",
        "svg" | "xaml" | "csproj" => "xml",
        "md" => "markdown",
        "sh" | "bash" | "zsh" | "console" | "shell-session" => "shell",
        "ps" | "ps1" | "pwsh" => "powershell",
        "bat" | "cmd" | "dos" => "batch",
        "yml" => "yaml",
        "patch" => "diff",
        other => other,
    };
    Lang::from_id(alias).unwrap_or_else(|| detect(Some(std::path::Path::new(&format!("x.{t}"))), ""))
}

/// Splits `text` into (segment, token) runs; line breaks stay in the text.
pub fn highlight(lang: Lang, text: &str) -> Vec<(String, Tok)> {
    let def = lang::def(lang);
    let mut out: Vec<(String, Tok)> = Vec::new();
    let mut push = |s: &str, t: Tok| {
        if s.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((prev, pt)) if *pt == t => prev.push_str(s),
            _ => out.push((s.to_string(), t)),
        }
    };
    let mut state = 0;
    let mut spans = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            push("\n", Tok::Text);
        }
        state = lexer::lex_line(def, line, state, &mut spans);
        let mut at = 0;
        for s in &spans {
            push(&line[at.min(s.start)..s.start], Tok::Text);
            push(&line[s.start..s.end], s.tok);
            at = s.end;
        }
        push(&line[at.min(line.len())..], Tok::Text);
    }
    out
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
    fn tags_and_runs() {
        assert_eq!(from_tag("rust"), Lang::Rust);
        assert_eq!(from_tag("language-js"), Lang::JavaScript);
        assert_eq!(from_tag("py title=\"x\""), Lang::Python);
        assert_eq!(from_tag("JSON"), Lang::Json);
        assert_eq!(from_tag("unknown"), Lang::Plain);
        let runs = highlight(Lang::Rust, "fn main() {\n    // hi\n}");
        assert!(runs.iter().any(|(s, t)| s == "fn" && *t == Tok::Keyword), "{runs:?}");
        assert!(runs.iter().any(|(s, t)| s.contains("// hi") && *t == Tok::Comment));
        let joined: String = runs.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(joined, "fn main() {\n    // hi\n}");
    }

    #[test]
    fn multi_line_state() {
        let lines = ["/* start", "still comment", "end */ let x = 1;"];
        let mut h = Highlighter::new(Lang::Rust);
        let mut out = Vec::new();
        h.spans(&|i| lines[i], 1, &mut out);
        assert_eq!(out[0].tok, Tok::Comment);
        h.spans(&|i| lines[i], 2, &mut out);
        assert!(out.iter().any(|s| s.tok == Tok::Keyword));
        for t in 0..24u8 {
            assert_eq!(tok_from(t).map(|k| k as u8), Some(t));
        }
    }
}
