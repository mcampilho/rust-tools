//! JSON files as Scoop reads them, and written as `ConvertToPrettyJson`
//! writes them (four spaces). Reading is `rt_core::json`.

pub use rt_core::json::Json;
use std::path::Path;

/// The document in `path` (any BOM taken off); None when it cannot be read.
pub fn read(path: &Path) -> Option<Json> {
    parse(&read_text(path)?)
}

pub fn parse(text: &str) -> Option<Json> {
    rt_core::json::parse(text.trim_start_matches('\u{feff}')).ok()
}

/// A text file: UTF-8, or UTF-16 with a BOM.
pub fn read_text(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    let utf16 = |le: bool| {
        let (pairs, _) = data[2..].as_chunks::<2>();
        let units: Vec<u16> = pairs.iter().map(|&c| if le { u16::from_le_bytes(c) } else { u16::from_be_bytes(c) }).collect();
        String::from_utf16_lossy(&units)
    };
    Some(match data.as_slice() {
        [0xFF, 0xFE, ..] => utf16(true),
        [0xFE, 0xFF, ..] => utf16(false),
        _ => String::from_utf8_lossy(&data).trim_start_matches('\u{feff}').to_string(),
    })
}

/// An object from pairs, in that order.
pub fn object<const N: usize>(members: [(&str, Json); N]) -> Json {
    Json::Obj(members.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

pub fn string(s: impl Into<String>) -> Json {
    Json::Str(s.into())
}

/// The document with four spaces of indentation, `\n` at the line ends.
pub fn pretty(value: &Json) -> String {
    let mut out = String::new();
    write(&mut out, value, 0);
    out
}

fn write(out: &mut String, value: &Json, depth: usize) {
    let indent = |out: &mut String, d: usize| out.extend(std::iter::repeat_n(' ', d * 4));
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) if n.fract() == 0.0 && n.abs() < 9e15 => out.push_str(&(*n as i64).to_string()),
        Json::Num(n) => out.push_str(&n.to_string()),
        Json::Str(s) => quote(out, s),
        Json::Arr(items) if items.is_empty() => out.push_str("[]"),
        Json::Obj(members) if members.is_empty() => out.push_str("{}"),
        Json::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                indent(out, depth + 1);
                write(out, item, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push(']');
        }
        Json::Obj(members) => {
            out.push('{');
            for (i, (key, item)) in members.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                indent(out, depth + 1);
                quote(out, key);
                out.push_str(": ");
                write(out, item, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push('}');
        }
    }
}

fn quote(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_round_trips() {
        let text = "{\n    \"a\": [\n        1,\n        1.5,\n        true,\n        null\n    ],\n    \"b\": \"x\\\\y \\\"z\\\"\\n\",\n    \"c\": {},\n    \"d\": []\n}";
        assert_eq!(pretty(&parse(text).unwrap()), text);
    }
}
