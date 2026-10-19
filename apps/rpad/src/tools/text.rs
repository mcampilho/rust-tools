//! Pure text transformations used by the Edit and Tools menus. They operate
//! on strings or line lists; the buffer applies them as undoable edits.

use crate::i18n::{tf, tr};

pub fn upper(s: &str) -> String {
    s.to_uppercase()
}

pub fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// Capitalizes the first letter of every word.
pub fn title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphanumeric() || c == '\'' {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = true;
        }
    }
    out
}

/// Lowercases, then capitalizes the first letter of each sentence.
pub fn sentence(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphabetic() {
            if start {
                out.extend(c.to_uppercase());
                start = false;
            } else {
                out.extend(c.to_lowercase());
            }
        } else {
            if matches!(c, '.' | '!' | '?') || (c == '\n' && out.ends_with('\n')) {
                start = true;
            }
            out.push(c);
        }
    }
    out
}

pub fn invert(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_uppercase() {
                c.to_lowercase().collect::<Vec<_>>()
            } else if c.is_lowercase() {
                c.to_uppercase().collect()
            } else {
                vec![c]
            }
        })
        .collect()
}

// ---------------------------------------------------------------- line tools

pub fn trim_trailing(lines: Vec<String>) -> Vec<String> {
    lines.into_iter().map(|l| l.trim_end_matches([' ', '\t']).to_string()).collect()
}

pub fn trim_leading(lines: Vec<String>) -> Vec<String> {
    lines.into_iter().map(|l| l.trim_start_matches([' ', '\t']).to_string()).collect()
}

pub fn remove_blank(lines: Vec<String>) -> Vec<String> {
    lines.into_iter().filter(|l| !l.trim().is_empty()).collect()
}

/// Removes repeated lines, keeping the first occurrence and the order.
pub fn remove_duplicates(lines: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    lines.into_iter().filter(|l| seen.insert(l.clone())).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    Ascending,
    Descending,
    CaseInsensitive,
    /// Numbers compare by value ("file2" < "file10").
    Natural,
}

pub fn sort(mut lines: Vec<String>, mode: SortMode) -> Vec<String> {
    match mode {
        SortMode::Ascending => lines.sort(),
        SortMode::Descending => {
            lines.sort();
            lines.reverse();
        }
        SortMode::CaseInsensitive => lines.sort_by_key(|l| l.to_lowercase()),
        SortMode::Natural => lines.sort_by(|a, b| natural_cmp(a, b)),
    }
    lines
}

pub fn reverse(mut lines: Vec<String>) -> Vec<String> {
    lines.reverse();
    lines
}

fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let mut n1 = String::new();
                while let Some(c) = x.peek().copied().filter(char::is_ascii_digit) {
                    n1.push(c);
                    x.next();
                }
                let mut n2 = String::new();
                while let Some(c) = y.peek().copied().filter(char::is_ascii_digit) {
                    n2.push(c);
                    y.next();
                }
                let (t1, t2) = (n1.trim_start_matches('0'), n2.trim_start_matches('0'));
                let o = t1.len().cmp(&t2.len()).then(t1.cmp(t2));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_lowercase().cmp(d.to_lowercase());
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

pub fn prefix_suffix(lines: Vec<String>, prefix: &str, suffix: &str, skip_blank: bool) -> Vec<String> {
    lines
        .into_iter()
        .map(|l| if skip_blank && l.trim().is_empty() { l } else { format!("{prefix}{l}{suffix}") })
        .collect()
}

/// Expands every tab to spaces, respecting tab stops.
pub fn tabs_to_spaces(lines: Vec<String>, width: usize) -> Vec<String> {
    lines
        .into_iter()
        .map(|l| {
            let mut out = String::with_capacity(l.len());
            let mut col = 0;
            for c in l.chars() {
                if c == '\t' {
                    let n = width - col % width;
                    out.extend(std::iter::repeat_n(' ', n));
                    col += n;
                } else {
                    out.push(c);
                    col += 1;
                }
            }
            out
        })
        .collect()
}

/// Converts leading spaces to tabs (indentation only; text is untouched).
pub fn leading_spaces_to_tabs(lines: Vec<String>, width: usize) -> Vec<String> {
    lines
        .into_iter()
        .map(|l| {
            let ws: String = l.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            let mut col = 0;
            for c in ws.chars() {
                col = if c == '\t' { (col / width + 1) * width } else { col + 1 };
            }
            let indent = format!("{}{}", "\t".repeat(col / width), " ".repeat(col % width));
            format!("{indent}{}", &l[ws.len()..])
        })
        .collect()
}

// ---------------------------------------------------------------- escaping

pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub fn json_unescape(s: &str) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('/') => out.push('/'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let hex: String = it.by_ref().take(4).collect();
                let mut code = u32::from_str_radix(&hex, 16).map_err(|_| tf("Invalid escape {}", &[&format!("\\u{hex}")]))?;
                if (0xD800..0xDC00).contains(&code) {
                    // surrogate pair
                    let rest: String = it.by_ref().take(6).collect();
                    let low = rest.strip_prefix("\\u").and_then(|h| u32::from_str_radix(h, 16).ok()).ok_or_else(|| tr("Unpaired surrogate"))?;
                    code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                }
                out.push(char::from_u32(code).ok_or_else(|| tr("Invalid code point"))?);
            }
            Some(o) => return Err(tf("Invalid escape {}", &[&format!("\\{o}")])),
            None => return Err(tr("Trailing backslash")),
        }
    }
    Ok(out)
}

pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Decodes the XML predefined entities and numeric references (plus a few
/// common HTML entities). Unknown entities are left unchanged.
pub fn xml_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let Some(semi) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..semi];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            "copy" => Some('©'),
            n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Percent-encodes everything except unreserved characters (RFC 3986).
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn url_decode(s: &str) -> Result<String, String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match h {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| tr("The decoded bytes are not valid UTF-8"))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(b.len().div_ceil(3) * 4);
    for chunk in b.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Result<String, String> {
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = Vec::new();
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace()) {
        if c == b'=' {
            break;
        }
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return Err(tr("Invalid Base64 character")),
        };
        bits = bits << 6 | v as u32;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
            bits &= (1 << n) - 1;
        }
    }
    String::from_utf8(out).map_err(|_| tr("The decoded data is not UTF-8 text"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn cases() {
        assert_eq!(title("hello wORLD it's"), "Hello World It's");
        assert_eq!(sentence("HELLO there. how ARE you? fine"), "Hello there. How are you? Fine");
        assert_eq!(invert("Hello ß"), "hELLO SS");
        assert_eq!(upper("straße"), "STRASSE");
    }

    #[test]
    fn line_tools() {
        assert_eq!(trim_trailing(v(&["a  ", "b\t", " c"])), v(&["a", "b", " c"]));
        assert_eq!(remove_blank(v(&["a", " ", "", "b"])), v(&["a", "b"]));
        assert_eq!(remove_duplicates(v(&["b", "a", "b", "a", "c"])), v(&["b", "a", "c"]));
        assert_eq!(sort(v(&["b", "B", "a"]), SortMode::Ascending), v(&["B", "a", "b"]));
        assert_eq!(sort(v(&["b", "C", "a"]), SortMode::CaseInsensitive), v(&["a", "b", "C"]));
        assert_eq!(sort(v(&["file10", "file2", "file1"]), SortMode::Natural), v(&["file1", "file2", "file10"]));
        assert_eq!(sort(v(&["a", "c", "b"]), SortMode::Descending), v(&["c", "b", "a"]));
        assert_eq!(prefix_suffix(v(&["a", "", "b"]), "- ", ";", true), v(&["- a;", "", "- b;"]));
        assert_eq!(tabs_to_spaces(v(&["\ta\tb"]), 4), v(&["    a   b"]));
        assert_eq!(leading_spaces_to_tabs(v(&["      x  y"]), 4), v(&["\t  x  y"]));
        assert_eq!(reverse(v(&["1", "2"])), v(&["2", "1"]));
    }

    #[test]
    fn escapes_round_trip() {
        let s = "Line \"one\"\n\ttab \\ é 😀 <&>";
        assert_eq!(json_unescape(&json_escape(s)).unwrap(), s);
        assert_eq!(json_unescape("\\ud83d\\ude00").unwrap(), "😀");
        assert!(json_unescape("\\q").is_err());
        assert_eq!(xml_unescape(&xml_escape(s)), s);
        assert_eq!(xml_unescape("&#65;&#x42;&unknown; & x"), "AB&unknown; & x");
        assert_eq!(url_decode(&url_encode(s)).unwrap(), s);
        assert_eq!(url_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(url_decode("a+b%2x").unwrap(), "a b%2x");
        assert_eq!(base64_encode("hello"), "aGVsbG8=");
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), "hello");
        assert_eq!(base64_decode(&base64_encode(s)).unwrap(), s);
        assert!(base64_decode("@@@").is_err());
    }
}
