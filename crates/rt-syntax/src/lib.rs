//! Syntax highlighting for source code: language definitions with detection
//! by file name ([`lang`]) and a tokenizer that works a line at a time from
//! the state left by the previous line ([`lexer`]), so an editor re-lexes
//! only from the changed line on.
//!
//! Colours are given as `0xRRGGBB` for a light and a dark background; the
//! app turns them into its own colour type.

pub mod lang;
pub mod lexer;

pub use lang::{def, detect, Lang};
pub use lexer::{lex_line, Span, State, Tok};

/// Every token kind, in declaration order.
pub const TOKENS: [Tok; 24] = [
    Tok::Text,
    Tok::Keyword,
    Tok::Type,
    Tok::Constant,
    Tok::Comment,
    Tok::String,
    Tok::Number,
    Tok::Preproc,
    Tok::Punct,
    Tok::Function,
    Tok::Variable,
    Tok::Meta,
    Tok::Tag,
    Tok::Attr,
    Tok::Entity,
    Tok::Heading,
    Tok::Emphasis,
    Tok::Strong,
    Tok::Code,
    Tok::Link,
    Tok::Key,
    Tok::Section,
    Tok::Inserted,
    Tok::Deleted,
];

const LIGHT: [u32; 24] = [
    0x1B1B1B, 0x0000FF, 0x267F99, 0x0070C1, 0x008000, 0xA31515, 0x098658, 0xAF00DB, 0x1B1B1B, 0x795E26, 0x001080, 0x795E26, 0x800000,
    0xE50000, 0xC5060B, 0x0451A5, 0x6F42C1, 0x9A6700, 0xA31515, 0x0068B8, 0x0451A5, 0xAF00DB, 0x116329, 0xC42B1C,
];
const DARK: [u32; 24] = [
    0xD4D4D4, 0x569CD6, 0x4EC9B0, 0x4FC1FF, 0x6A9955, 0xCE9178, 0xB5CEA8, 0xC586C0, 0xD4D4D4, 0xDCDCAA, 0x9CDCFE, 0xD7BA7D, 0x569CD6,
    0x9CDCFE, 0xD7BA7D, 0x569CD6, 0xC5A5E5, 0xF0C674, 0xCE9178, 0x4DAAFC, 0x9CDCFE, 0xC586C0, 0x89D185, 0xF48771,
];

/// Colour of a token as `0xRRGGBB` (None for plain text, which keeps the
/// text colour).
pub fn rgb(t: Tok, dark: bool) -> Option<u32> {
    if matches!(t, Tok::Text | Tok::Punct) {
        return None;
    }
    Some(if dark { DARK } else { LIGHT }[t as usize])
}

/// Token from its index (`t as u8`).
pub fn tok_from(i: u8) -> Option<Tok> {
    TOKENS.get(i as usize).copied()
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
    let def = def(lang);
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
        state = lex_line(def, line, state, &mut spans);
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
    fn tokens_and_colours() {
        for t in 0..24u8 {
            assert_eq!(tok_from(t).map(|k| k as u8), Some(t));
        }
        assert_eq!(tok_from(24), None);
        assert_eq!(rgb(Tok::Text, false), None);
        assert_eq!(rgb(Tok::Keyword, false), Some(0x0000FF));
        assert_eq!(rgb(Tok::Keyword, true), Some(0x569CD6));
    }
}
