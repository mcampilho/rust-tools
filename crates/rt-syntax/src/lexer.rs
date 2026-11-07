//! Line-at-a-time tokenizer. Each line is lexed from the state left by the
//! previous line (block comments, multi-line strings, XML tags, Markdown
//! code fences), so editing re-lexes only from the changed line onward.

use crate::lang::{Def, Family, Lang};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tok {
    Text,
    Keyword,
    Type,
    Constant,
    Comment,
    String,
    Number,
    Preproc,
    Punct,
    Function,
    Variable,
    Meta,
    Tag,
    Attr,
    Entity,
    Heading,
    Emphasis,
    Strong,
    Code,
    Link,
    Key,
    Section,
    Inserted,
    Deleted,
}

/// A token span in byte offsets of the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub tok: Tok,
}

/// Lexer state carried between lines: kind in the low byte, parameter above.
pub type State = u32;

const NORMAL: u32 = 0;
const BLOCK: u32 = 1; // param: nesting depth
const STRING: u32 = 2; // param: quote index
const RAW: u32 = 3; // Rust raw string; param: number of '#'
const XCOMMENT: u32 = 4;
const XCDATA: u32 = 5;
const XTAG: u32 = 6;
const FENCE: u32 = 7; // param: fence length * 2 + (1 if '~')

fn kind(s: State) -> u32 {
    s & 0xFF
}
fn param(s: State) -> u32 {
    s >> 8
}
fn mk(kind: u32, param: u32) -> State {
    kind | (param << 8)
}

/// Lines longer than this are not highlighted (keeps huge lines fast).
pub const MAX_HIGHLIGHT_LINE: usize = 20_000;

/// Lexes `line` starting in `state`; appends spans to `out` and returns the
/// state for the next line.
pub fn lex_line(def: &Def, line: &str, state: State, out: &mut Vec<Span>) -> State {
    out.clear();
    if line.len() > MAX_HIGHLIGHT_LINE {
        return state;
    }
    match def.family {
        Family::Plain => state,
        Family::Code => lex_code(def, line, 0, state, out),
        Family::Markup => lex_markup(line, state, out),
        Family::Markdown => lex_markdown(line, state, out),
        Family::Ini => lex_ini(def, line, state, out),
        Family::Diff => {
            let tok = if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") || line.starts_with("index ") {
                Some(Tok::Keyword)
            } else if line.starts_with("@@") {
                Some(Tok::Meta)
            } else if line.starts_with('+') {
                Some(Tok::Inserted)
            } else if line.starts_with('-') {
                Some(Tok::Deleted)
            } else {
                None
            };
            if let Some(tok) = tok {
                out.push(Span {
                    start: 0,
                    end: line.len(),
                    tok,
                });
            }
            NORMAL
        }
    }
}

fn push(out: &mut Vec<Span>, start: usize, end: usize, tok: Tok) {
    if end > start {
        out.push(Span { start, end, tok });
    }
}

fn starts_with_ci(s: &str, p: &str, ci: bool) -> bool {
    if ci {
        s.len() >= p.len() && s.is_char_boundary(p.len()) && s[..p.len()].eq_ignore_ascii_case(p)
    } else {
        s.starts_with(p)
    }
}

/// End index of a quoted string starting after the opener at `i`.
fn scan_string(line: &str, i: usize, close: &str, escape: bool) -> Option<usize> {
    let b = line.as_bytes();
    let mut j = i;
    while j < b.len() {
        if escape && b[j] == b'\\' {
            j += 2;
            continue;
        }
        if line[j..].starts_with(close) {
            return Some(j + close.len());
        }
        j += 1;
        while j < b.len() && !line.is_char_boundary(j) {
            j += 1;
        }
    }
    None
}

fn is_ident(c: char, extra: &str) -> bool {
    c.is_alphanumeric() || c == '_' || extra.contains(c)
}

fn lex_code(def: &Def, line: &str, from: usize, state: State, out: &mut Vec<Span>) -> State {
    let b = line.as_bytes();
    let len = line.len();
    let mut i = from;
    // Continue constructs from the previous line.
    match kind(state) {
        BLOCK => {
            let (open, close) = def.block_comment.unwrap_or(("/*", "*/"));
            let mut depth = param(state).max(1);
            let mut j = i;
            while j < len {
                if line[j..].starts_with(close) {
                    depth -= 1;
                    j += close.len();
                    if depth == 0 {
                        break;
                    }
                } else if def.nested_comments && line[j..].starts_with(open) {
                    depth += 1;
                    j += open.len();
                } else {
                    j += line[j..].chars().next().map_or(1, char::len_utf8);
                }
            }
            push(out, i, j, Tok::Comment);
            if depth > 0 {
                return mk(BLOCK, depth);
            }
            i = j;
        }
        STRING => {
            let qi = param(state) as usize;
            if let Some(qd) = def.quotes.get(qi) {
                match scan_string(line, i, qd.close, qd.escape) {
                    Some(end) => {
                        push(out, i, end, Tok::String);
                        i = end;
                    }
                    None => {
                        push(out, i, len, Tok::String);
                        return state;
                    }
                }
            }
        }
        RAW => {
            let close = format!("\"{}", "#".repeat(param(state) as usize));
            match line[i..].find(&close) {
                Some(p) => {
                    let end = i + p + close.len();
                    push(out, i, end, Tok::String);
                    i = end;
                }
                None => {
                    push(out, i, len, Tok::String);
                    return state;
                }
            }
        }
        _ => {}
    }
    let ci = def.case_insensitive;
    let first_nonblank = line.len() - line.trim_start().len();
    while i < len {
        let rest = &line[i..];
        let c = rest.chars().next().unwrap_or(' ');
        if c.is_whitespace() {
            i += c.len_utf8();
            continue;
        }
        // Line comments (word-like markers such as "rem " only at line start).
        if def.line_comments.iter().any(|m| {
            let wordy = m.chars().next().is_some_and(|c| c.is_alphabetic() || c == '@');
            starts_with_ci(rest, m, ci) && (!wordy || i == first_nonblank)
        }) {
            push(out, i, len, Tok::Comment);
            return NORMAL;
        }
        if let Some((open, close)) = def.block_comment {
            if rest.starts_with(open) {
                let mut depth = 1;
                let mut j = i + open.len();
                while j < len {
                    if line[j..].starts_with(close) {
                        depth -= 1;
                        j += close.len();
                        if depth == 0 {
                            break;
                        }
                    } else if def.nested_comments && line[j..].starts_with(open) {
                        depth += 1;
                        j += open.len();
                    } else {
                        j += line[j..].chars().next().map_or(1, char::len_utf8);
                    }
                }
                push(out, i, j, Tok::Comment);
                if depth > 0 {
                    return mk(BLOCK, depth);
                }
                i = j;
                continue;
            }
        }
        if def.preproc && c == '#' && i == first_nonblank {
            push(out, i, len, Tok::Preproc);
            return NORMAL;
        }
        if def.lang == Lang::Rust {
            // Attributes #[...] and #![...]
            if rest.starts_with("#[") || rest.starts_with("#![") {
                let end = rest.find(']').map_or(len, |p| i + p + 1);
                push(out, i, end, Tok::Meta);
                i = end;
                continue;
            }
            // Raw strings r"..", r#".."#, br#".."#
            let raw = rest.strip_prefix("br").or_else(|| rest.strip_prefix('r'));
            if let Some(after) = raw {
                let hashes = after.len() - after.trim_start_matches('#').len();
                if after[hashes..].starts_with('"') && !(i > 0 && is_ident(line[..i].chars().last().unwrap_or(' '), "")) {
                    let open_len = (len - i) - after.len() + hashes + 1;
                    let close = format!("\"{}", "#".repeat(hashes));
                    match line[i + open_len..].find(&close) {
                        Some(p) => {
                            let end = i + open_len + p + close.len();
                            push(out, i, end, Tok::String);
                            i = end;
                            continue;
                        }
                        None => {
                            push(out, i, len, Tok::String);
                            return mk(RAW, hashes as u32);
                        }
                    }
                }
            }
            // Char literal vs lifetime
            if c == '\'' {
                let mut it = rest.char_indices().skip(1);
                let lit_end = match it.next() {
                    Some((_, '\\')) => rest[2..].find('\'').map(|p| p + 3),
                    Some((p, ch)) => {
                        let after = p + ch.len_utf8();
                        rest[after..].starts_with('\'').then_some(after + 1)
                    }
                    None => None,
                };
                match lit_end {
                    Some(e) => push(out, i, i + e, Tok::String),
                    None => {
                        let e = rest[1..].find(|ch: char| !is_ident(ch, "")).map_or(rest.len(), |p| p + 1);
                        push(out, i, i + e, Tok::Type);
                        i += e;
                        continue;
                    }
                }
                i += lit_end.unwrap_or(1);
                continue;
            }
        }
        // Strings
        if let Some((qi, qd)) = def.quotes.iter().enumerate().find(|(_, qd)| rest.starts_with(qd.open)) {
            let body = i + qd.open.len();
            match scan_string(line, body, qd.close, qd.escape) {
                Some(end) => {
                    push(out, i, end, Tok::String);
                    i = end;
                }
                None => {
                    push(out, i, len, Tok::String);
                    return if qd.multiline { mk(STRING, qi as u32) } else { NORMAL };
                }
            }
            continue;
        }
        // Decorators / annotations
        if c == '@'
            && matches!(
                def.lang,
                Lang::Python | Lang::Java | Lang::CSharp | Lang::TypeScript | Lang::JavaScript
            )
            && rest[1..].chars().next().is_some_and(|ch| ch.is_alphabetic())
        {
            let e = rest[1..].find(|ch: char| !is_ident(ch, ".")).map_or(rest.len(), |p| p + 1);
            push(out, i, i + e, Tok::Meta);
            i += e;
            continue;
        }
        // Variables ($x, %x%, @x)
        if def.var_prefix == Some(c) {
            if c == '%' {
                let e = rest[1..].find('%').map_or(1, |p| p + 2);
                push(out, i, i + e, Tok::Variable);
                i += e;
                continue;
            }
            let e = rest[c.len_utf8()..]
                .find(|ch: char| !is_ident(ch, if c == '$' { "{}:" } else { "" }))
                .map_or(rest.len(), |p| p + c.len_utf8());
            if e > 1 {
                let word = &rest[..e];
                let tok = if def.constants.iter().any(|k| k.eq_ignore_ascii_case(word)) {
                    Tok::Constant
                } else {
                    Tok::Variable
                };
                push(out, i, i + e, tok);
                i += e;
                continue;
            }
        }
        // Numbers
        if c.is_ascii_digit() || (c == '.' && rest[1..].starts_with(|ch: char| ch.is_ascii_digit())) {
            let mut j = i + 1;
            while j < len {
                let ch = b[j] as char;
                let exponent_sign = (ch == '+' || ch == '-') && matches!(b[j - 1], b'e' | b'E') && !line[i..j].starts_with("0x");
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' || exponent_sign {
                    j += 1;
                } else {
                    break;
                }
            }
            push(out, i, j, Tok::Number);
            i = j;
            continue;
        }
        // Identifiers and keywords
        if is_ident(c, def.ident_extra) {
            let mut j = i;
            for ch in rest.chars() {
                if is_ident(ch, def.ident_extra) {
                    j += ch.len_utf8();
                } else {
                    break;
                }
            }
            let word = &line[i..j];
            let eq = |k: &&str| if ci { k.eq_ignore_ascii_case(word) } else { *k == word };
            let after = line[j..].trim_start();
            let tok = if def.keywords.iter().any(eq) {
                Some(Tok::Keyword)
            } else if def.constants.iter().any(eq) {
                Some(Tok::Constant)
            } else if def.types.iter().any(eq) {
                Some(Tok::Type)
            } else if after.starts_with('(') || (def.lang == Lang::Rust && after.starts_with('!')) {
                Some(Tok::Function)
            } else if !ci
                && def.lang != Lang::Css
                && def.lang != Lang::Json
                && word.chars().next().is_some_and(|ch| ch.is_uppercase())
                && word.chars().any(|ch| ch.is_lowercase())
            {
                Some(Tok::Type)
            } else if def.lang == Lang::Css && after.starts_with(':') && !after.starts_with("::") {
                Some(Tok::Key)
            } else {
                None
            };
            if let Some(t) = tok {
                push(out, i, j, t);
            }
            i = j;
            continue;
        }
        if matches!(c, '(' | ')' | '[' | ']' | '{' | '}') {
            push(out, i, i + 1, Tok::Punct);
        }
        i += c.len_utf8();
    }
    NORMAL
}

fn lex_markup(line: &str, state: State, out: &mut Vec<Span>) -> State {
    let len = line.len();
    let mut i = 0;
    let mut st = state;
    while i < len {
        match kind(st) {
            XCOMMENT => match line[i..].find("-->") {
                Some(p) => {
                    push(out, i, i + p + 3, Tok::Comment);
                    i += p + 3;
                    st = NORMAL;
                }
                None => {
                    push(out, i, len, Tok::Comment);
                    return XCOMMENT;
                }
            },
            XCDATA => match line[i..].find("]]>") {
                Some(p) => {
                    push(out, i, i + p + 3, Tok::String);
                    i += p + 3;
                    st = NORMAL;
                }
                None => {
                    push(out, i, len, Tok::String);
                    return XCDATA;
                }
            },
            XTAG => {
                let rest = &line[i..];
                let c = rest.chars().next().unwrap_or(' ');
                if c.is_whitespace() {
                    i += c.len_utf8();
                } else if rest.starts_with("/>") {
                    push(out, i, i + 2, Tok::Tag);
                    i += 2;
                    st = NORMAL;
                } else if c == '>' {
                    push(out, i, i + 1, Tok::Tag);
                    i += 1;
                    st = NORMAL;
                } else if c == '"' || c == '\'' {
                    let end = line[i + 1..].find(c).map_or(len, |p| i + 1 + p + 1);
                    push(out, i, end, Tok::String);
                    i = end;
                } else if c == '=' {
                    i += 1;
                } else {
                    let e = rest
                        .find(|ch: char| ch.is_whitespace() || matches!(ch, '=' | '>' | '/' | '"' | '\''))
                        .unwrap_or(rest.len());
                    let e = e.max(c.len_utf8());
                    push(out, i, i + e, Tok::Attr);
                    i += e;
                }
            }
            _ => {
                let rest = &line[i..];
                if rest.starts_with("<!--") {
                    st = XCOMMENT;
                    push(out, i, i + 4, Tok::Comment);
                    i += 4;
                } else if rest.starts_with("<![CDATA[") {
                    push(out, i, i + 9, Tok::Meta);
                    i += 9;
                    st = XCDATA;
                } else if rest.starts_with("<?") || rest.starts_with("<!") {
                    let close = if rest.starts_with("<?") { "?>" } else { ">" };
                    let end = rest.find(close).map_or(len, |p| i + p + close.len());
                    push(out, i, end, Tok::Meta);
                    i = end;
                } else if rest.starts_with('<') && rest[1..].starts_with(|c: char| c.is_alphabetic() || c == '/' || c == '_' || c == ':') {
                    let name_end = rest[1..]
                        .find(|ch: char| ch.is_whitespace() || ch == '>' || (ch == '/' && true))
                        .map_or(rest.len(), |p| p + 1);
                    let name_end = if rest[1..].starts_with('/') {
                        rest[2..]
                            .find(|ch: char| ch.is_whitespace() || ch == '>')
                            .map_or(rest.len(), |p| p + 2)
                    } else {
                        name_end
                    };
                    push(out, i, i + name_end, Tok::Tag);
                    i += name_end;
                    st = XTAG;
                } else if rest.starts_with('&') {
                    let end = rest.find(';').filter(|p| *p < 12).map_or(1, |p| p + 1);
                    push(out, i, i + end, Tok::Entity);
                    i += end;
                } else {
                    i += rest.find(['<', '&']).unwrap_or(rest.len()).max(1);
                }
            }
        }
    }
    st
}

fn lex_markdown(line: &str, state: State, out: &mut Vec<Span>) -> State {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    let fence = |t: &str| -> Option<(u32, bool)> {
        let ch = t.chars().next()?;
        if ch != '`' && ch != '~' {
            return None;
        }
        let n = t.chars().take_while(|c| *c == ch).count();
        (n >= 3).then_some((n as u32, ch == '~'))
    };
    if kind(state) == FENCE {
        push(out, 0, line.len(), Tok::Code);
        let (n, tilde) = (param(state) / 2, param(state) % 2 == 1);
        if let Some((m, t)) = fence(trimmed) {
            if m >= n && t == tilde && trimmed.trim_end().chars().all(|c| c == if tilde { '~' } else { '`' }) {
                return NORMAL;
            }
        }
        return state;
    }
    if indent < 4 {
        if let Some((n, tilde)) = fence(trimmed) {
            push(out, 0, line.len(), Tok::Code);
            return mk(FENCE, n * 2 + u32::from(tilde));
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with([' ', '\t']) || (hashes > 0 && hashes <= 6 && trimmed.len() == hashes)
        {
            push(out, 0, line.len(), Tok::Heading);
            return NORMAL;
        }
        let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.len() >= 3
            && (compact.chars().all(|c| c == '-') || compact.chars().all(|c| c == '*') || compact.chars().all(|c| c == '='))
        {
            push(out, 0, line.len(), Tok::Meta);
            return NORMAL;
        }
    }
    let mut i = indent;
    if trimmed.starts_with('>') {
        push(out, indent, indent + 1, Tok::Meta);
        i += 1;
    } else if let Some(m) = list_marker(trimmed) {
        push(out, indent, indent + m, Tok::Keyword);
        i += m;
    }
    // Inline spans.
    let b = line.as_bytes();
    while i < line.len() {
        let rest = &line[i..];
        let c = b[i];
        if c == b'`' {
            let n = rest.bytes().take_while(|x| *x == b'`').count();
            let delim = &rest[..n];
            if let Some(p) = rest[n..].find(delim) {
                push(out, i, i + n + p + n, Tok::Code);
                i += n + p + n;
                continue;
            }
            i += n;
            continue;
        }
        if (c == b'*' || c == b'_') && i + 1 < line.len() {
            let strong = rest.len() > 2 && b[i + 1] == c;
            let delim = if strong { &rest[..2] } else { &rest[..1] };
            let body = &rest[delim.len()..];
            if !body.starts_with(' ') {
                if let Some(p) = body.find(delim) {
                    if p > 0 {
                        push(out, i, i + delim.len() * 2 + p, if strong { Tok::Strong } else { Tok::Emphasis });
                        i += delim.len() * 2 + p;
                        continue;
                    }
                }
            }
        }
        if c == b'[' {
            if let Some(close) = rest.find("](") {
                if let Some(end) = rest[close..].find(')') {
                    push(out, i, i + close + end + 1, Tok::Link);
                    i += close + end + 1;
                    continue;
                }
            }
        }
        if c == b'<' && (rest.starts_with("<http") || rest.starts_with("<mailto:")) {
            if let Some(p) = rest.find('>') {
                push(out, i, i + p + 1, Tok::Link);
                i += p + 1;
                continue;
            }
        }
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    NORMAL
}

fn list_marker(t: &str) -> Option<usize> {
    let b = t.as_bytes();
    if b.len() >= 2 && matches!(b[0], b'-' | b'*' | b'+') && b[1] == b' ' {
        return Some(1);
    }
    let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits < 10 && b.len() > digits + 1 && matches!(b[digits], b'.' | b')') && b[digits + 1] == b' ' {
        return Some(digits + 1);
    }
    None
}

fn lex_ini(def: &Def, line: &str, state: State, out: &mut Vec<Span>) -> State {
    if kind(state) != NORMAL {
        return lex_code(def, line, 0, state, out);
    }
    let t = line.trim_start();
    let indent = line.len() - t.len();
    if def.line_comments.iter().any(|m| t.starts_with(m)) {
        push(out, indent, line.len(), Tok::Comment);
        return NORMAL;
    }
    if def.lang != Lang::Yaml && t.starts_with('[') {
        let end = t.rfind(']').map_or(line.len(), |p| indent + p + 1);
        push(out, indent, end, Tok::Section);
        return lex_code(def, line, end, NORMAL, out);
    }
    if def.lang == Lang::Yaml && (t.starts_with("---") || t.starts_with("...")) {
        push(out, indent, line.len(), Tok::Meta);
        return NORMAL;
    }
    let mut start = indent;
    if def.lang == Lang::Yaml && t.starts_with("- ") {
        push(out, indent, indent + 1, Tok::Punct);
        start += 2;
    }
    let sep: &[char] = if def.lang == Lang::Yaml { &[':'] } else { &['=', ':'] };
    let rest = &line[start..];
    let key_end = if rest.starts_with(['"', '\'']) {
        None
    } else {
        rest.find(sep)
            .filter(|p| def.lang != Lang::Yaml || rest[p + 1..].is_empty() || rest[p + 1..].starts_with(' '))
    };
    match key_end {
        Some(p) if p > 0 => {
            push(out, start, start + p, Tok::Key);
            lex_code(def, line, start + p + 1, NORMAL, out)
        }
        _ => lex_code(def, line, start, NORMAL, out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::def;

    fn toks(lang: Lang, lines: &[&str]) -> Vec<Vec<(String, Tok)>> {
        let d = def(lang);
        let mut st = 0;
        let mut out = Vec::new();
        let mut res = Vec::new();
        for l in lines {
            st = lex_line(d, l, st, &mut out);
            res.push(out.iter().map(|s| (l[s.start..s.end].to_string(), s.tok)).collect());
        }
        res
    }

    fn has(v: &[(String, Tok)], text: &str, tok: Tok) -> bool {
        v.iter().any(|(t, k)| t == text && *k == tok)
    }

    #[test]
    fn rust() {
        let r = toks(
            Lang::Rust,
            &[
                "fn main() { let s = \"a\\\"b\"; // hi",
                "/* multi",
                " line */ let x = 0x1F; 'a' 'b: loop {}",
                "let r = r#\"raw \"q\"\"#;",
                "#[derive(Debug)] struct Foo;",
            ],
        );
        assert!(has(&r[0], "fn", Tok::Keyword));
        assert!(has(&r[0], "main", Tok::Function));
        assert!(has(&r[0], "\"a\\\"b\"", Tok::String));
        assert!(has(&r[0], "// hi", Tok::Comment));
        assert!(has(&r[1], "/* multi", Tok::Comment));
        assert!(has(&r[2], " line */", Tok::Comment));
        assert!(has(&r[2], "0x1F", Tok::Number));
        assert!(has(&r[2], "'a'", Tok::String));
        assert!(has(&r[2], "'b", Tok::Type));
        assert!(has(&r[3], "r#\"raw \"q\"\"#", Tok::String));
        assert!(has(&r[4], "#[derive(Debug)]", Tok::Meta));
        assert!(has(&r[4], "Foo", Tok::Type));
    }

    #[test]
    fn python_multiline_strings_and_comments() {
        let r = toks(
            Lang::Python,
            &["def f(x):  # c", "    s = \"\"\"doc", "still doc\"\"\" + 'x'", "@decorator"],
        );
        assert!(has(&r[0], "def", Tok::Keyword));
        assert!(has(&r[0], "# c", Tok::Comment));
        assert!(has(&r[1], "\"\"\"doc", Tok::String));
        assert!(has(&r[2], "still doc\"\"\"", Tok::String));
        assert!(has(&r[2], "'x'", Tok::String));
        assert!(has(&r[3], "@decorator", Tok::Meta));
    }

    #[test]
    fn c_preprocessor_and_sql_case() {
        let r = toks(Lang::C, &["#include <stdio.h>", "int x = 1.5e-3f;"]);
        assert!(has(&r[0], "#include <stdio.h>", Tok::Preproc));
        assert!(has(&r[1], "int", Tok::Type));
        assert!(has(&r[1], "1.5e-3f", Tok::Number));
        let r = toks(Lang::Sql, &["SELECT name FROM t -- note"]);
        assert!(has(&r[0], "SELECT", Tok::Keyword));
        assert!(has(&r[0], "-- note", Tok::Comment));
    }

    #[test]
    fn markup() {
        let r = toks(
            Lang::Xml,
            &[
                "<?xml version=\"1.0\"?>",
                "<a href=\"x\">&amp; <!-- c",
                "c --></a>",
                "<![CDATA[ <x> ]]>",
            ],
        );
        assert!(has(&r[0], "<?xml version=\"1.0\"?>", Tok::Meta));
        assert!(has(&r[1], "<a", Tok::Tag));
        assert!(has(&r[1], "href", Tok::Attr));
        assert!(has(&r[1], "\"x\"", Tok::String));
        assert!(has(&r[1], "&amp;", Tok::Entity));
        assert!(has(&r[2], "c -->", Tok::Comment));
        assert!(has(&r[2], "</a", Tok::Tag));
        assert!(has(&r[3], " <x> ]]>", Tok::String));
    }

    #[test]
    fn markdown() {
        let r = toks(
            Lang::Markdown,
            &[
                "# Title",
                "Some **bold** and *em* and `code` [link](http://x)",
                "```rust",
                "# not heading",
                "```",
                "- item",
            ],
        );
        assert!(has(&r[0], "# Title", Tok::Heading));
        assert!(has(&r[1], "**bold**", Tok::Strong));
        assert!(has(&r[1], "*em*", Tok::Emphasis));
        assert!(has(&r[1], "`code`", Tok::Code));
        assert!(has(&r[1], "[link](http://x)", Tok::Link));
        assert!(has(&r[3], "# not heading", Tok::Code));
        assert!(has(&r[5], "-", Tok::Keyword));
    }

    #[test]
    fn config_formats() {
        let r = toks(Lang::Ini, &["[section]", "key = value ; c", "; comment"]);
        assert!(has(&r[0], "[section]", Tok::Section));
        assert!(has(&r[1], "key ", Tok::Key));
        assert!(has(&r[2], "; comment", Tok::Comment));
        let r = toks(Lang::Toml, &["name = \"rpad\"", "ok = true", "[deps]"]);
        assert!(has(&r[0], "\"rpad\"", Tok::String));
        assert!(has(&r[1], "true", Tok::Constant));
        let r = toks(Lang::Yaml, &["key: value", "- item: 3", "url: http://x"]);
        assert!(has(&r[0], "key", Tok::Key));
        assert!(has(&r[1], "3", Tok::Number));
        assert!(has(&r[2], "url", Tok::Key));
        let r = toks(Lang::Json, &["{\"a\": [1, true, null]}"]);
        assert!(has(&r[0], "\"a\"", Tok::String));
        assert!(has(&r[0], "null", Tok::Constant));
    }

    #[test]
    fn shell_batch_powershell_diff() {
        let r = toks(Lang::Shell, &["if [ -f \"$HOME/x\" ]; then echo $USER; fi # c"]);
        assert!(has(&r[0], "if", Tok::Keyword));
        assert!(has(&r[0], "$USER", Tok::Variable));
        assert!(has(&r[0], "# c", Tok::Comment));
        let r = toks(Lang::Batch, &["@echo off", "REM comment", "set X=%PATH%"]);
        assert!(has(&r[1], "REM comment", Tok::Comment));
        assert!(has(&r[2], "%PATH%", Tok::Variable));
        let r = toks(Lang::PowerShell, &["<# block", "#> $x = $true"]);
        assert!(has(&r[1], "$true", Tok::Constant));
        let r = toks(Lang::Diff, &["+added", "-removed", "@@ -1 +1 @@"]);
        assert!(has(&r[0], "+added", Tok::Inserted));
        assert!(has(&r[2], "@@ -1 +1 @@", Tok::Meta));
    }

    #[test]
    fn plain_and_huge_lines() {
        assert!(toks(Lang::Plain, &["fn if while"])[0].is_empty());
        let long = "x".repeat(MAX_HIGHLIGHT_LINE + 1);
        assert!(toks(Lang::Rust, &[&long])[0].is_empty());
    }
}
