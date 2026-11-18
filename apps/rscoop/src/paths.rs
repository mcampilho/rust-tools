//! Path helpers: normalisation, finding absolute paths in text, reading text files.

use std::path::Path;

/// Sub-folders that only exist under a Scoop root; used to spot other roots.
pub const SCOOP_SUBDIRS: [&str; 6] = ["apps", "shims", "persist", "buckets", "cache", "modules"];

/// Remove the `\\?\` and `\??\` prefixes that links and APIs return.
pub fn strip_prefix(p: &str) -> String {
    if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = p.strip_prefix(r"\\?\").or_else(|| p.strip_prefix(r"\??\")) {
        return rest.to_string();
    }
    p.to_string()
}

/// Turn a path as written in text into a usable Windows path.
pub fn clean(p: &str) -> String {
    let p = strip_prefix(p);
    let p = p.trim().trim_matches(|c| c == '"' || c == '\'');
    let mut s: Vec<char> = p.chars().collect();
    if s.len() >= 2 && s[1] == ':' {
        let mut out: Vec<char> = s[..2].to_vec();
        let mut last_sep = false;
        for &c in &s[2..] {
            if c == '\\' || c == '/' {
                if !last_sep {
                    out.push('\\');
                }
                last_sep = true;
            } else {
                out.push(c);
                last_sep = false;
            }
        }
        s = out;
    }
    loop {
        while s.len() > 3 && matches!(s[s.len() - 1], ' ' | '.' | '\t' | '\\') {
            s.pop();
        }
        if s.last() == Some(&')') {
            let open = s.iter().filter(|&&c| c == '(').count();
            let close = s.iter().filter(|&&c| c == ')').count();
            if open < close {
                s.pop();
                continue;
            }
        }
        return s.into_iter().collect();
    }
}

/// Like Python's ntpath.normpath: one separator, no `.` or `..` parts.
pub fn normpath(p: &str) -> String {
    let p = p.replace('/', "\\");
    let (prefix, rest) = if let Some(r) = p.strip_prefix(r"\\") {
        (r"\\".to_string(), r)
    } else if p.len() >= 2 && p.as_bytes()[1] == b':' && p.is_char_boundary(2) {
        (p[..2].to_string(), &p[2..])
    } else {
        (String::new(), &p[..])
    };
    let absolute = prefix == r"\\" || rest.starts_with('\\');
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('\\') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|l| *l != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            _ => parts.push(part),
        }
    }
    let mut out = prefix.clone();
    if absolute && prefix != r"\\" {
        out.push('\\');
    }
    out.push_str(&parts.join("\\"));
    if out.is_empty() {
        out.push('.');
    }
    out
}

/// Normalised form for comparisons (case, separators, no trailing slash).
pub fn norm(p: &str) -> String {
    let mut s = normpath(&clean(p)).to_lowercase();
    if s.chars().count() > 3 {
        while s.ends_with('\\') {
            s.pop();
        }
    }
    s
}

/// Both arguments must already be normalised with [`norm`].
pub fn is_under(path_n: &str, base_n: &str) -> bool {
    if path_n == base_n {
        return true;
    }
    let base = base_n.trim_end_matches('\\');
    path_n.len() > base.len() && path_n.starts_with(base) && path_n[base.len()..].starts_with('\\')
}

/// Part of `path` below `base` (both may be un-normalised), or None.
pub fn relative_to(path: &str, base: &str) -> Option<String> {
    let (pn, bn) = (norm(path), norm(base));
    if !is_under(&pn, &bn) {
        return None;
    }
    let shown = normpath(&clean(path));
    let rest: String = shown.chars().skip(bn.chars().count()).collect();
    Some(rest.trim_start_matches('\\').to_string())
}

pub fn join(base: &str, rel: &str) -> String {
    if rel.is_empty() {
        return base.to_string();
    }
    if base.ends_with('\\') || base.ends_with('/') {
        format!("{base}{rel}")
    } else {
        format!("{base}\\{rel}")
    }
}

/// Parent folder of a path as text ("" for none).
pub fn dirname(p: &str) -> String {
    match p.rfind(['\\', '/']) {
        Some(0) => p[..1].to_string(),
        Some(i) if i == 2 && p.as_bytes()[1] == b':' => p[..3].to_string(),
        Some(i) => p[..i].to_string(),
        None => String::new(),
    }
}

/// The Scoop root inside `path` (`X:\...\scoop` followed by apps, shims...), if any.
pub fn scoop_root_of(path: &str) -> Option<String> {
    let p = normpath(&clean(path));
    let b = p.as_bytes();
    if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' {
        return None;
    }
    let comps: Vec<&str> = p[3..].split('\\').collect();
    for i in 0..comps.len().saturating_sub(1) {
        if comps[i].eq_ignore_ascii_case("scoop") && SCOOP_SUBDIRS.iter().any(|s| comps[i + 1].eq_ignore_ascii_case(s)) {
            return Some(format!("{}{}", &p[..3], comps[..=i].join("\\")));
        }
    }
    None
}

pub fn exists(p: &str) -> bool {
    !p.is_empty() && Path::new(p).exists()
}

pub fn is_dir(p: &str) -> bool {
    !p.is_empty() && Path::new(p).is_dir()
}

pub fn is_file(p: &str) -> bool {
    !p.is_empty() && Path::new(p).is_file()
}

/// An absolute path found in text, with the candidate cuts to try.
///
/// Unquoted paths may run into following words (`C:\x.exe --flag`), so every
/// cut at a space is a candidate; the path exists if any candidate exists.
#[derive(Clone, Debug, PartialEq)]
pub struct PathRef {
    pub candidates: Vec<String>,
}

impl PathRef {
    pub fn new(path: &str) -> PathRef {
        PathRef { candidates: vec![clean(path)] }
    }

    pub fn shown_with(&self, exists: &dyn Fn(&str) -> bool) -> String {
        self.candidates.iter().find(|c| exists(c)).unwrap_or_else(|| self.candidates.last().unwrap()).clone()
    }

    pub fn exists_with(&self, exists: &dyn Fn(&str) -> bool) -> bool {
        self.candidates.iter().any(|c| exists(c))
    }

    pub fn shown(&self) -> String {
        self.shown_with(&exists)
    }
}

const STOP: &[char] = &['"', '\'', '<', '>', '|', '*', '?', '\r', '\n', '\t', ';', ',', '%', '='];

/// All absolute drive paths in `text` (no duplicates).
pub fn extract_paths(text: &str) -> Vec<PathRef> {
    let t: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut i = 0;
    while i + 2 < t.len() {
        let ok = t[i].is_ascii_alphabetic()
            && t[i + 1] == ':'
            && (t[i + 2] == '\\' || t[i + 2] == '/')
            && (i == 0 || !t[i - 1].is_ascii_alphanumeric());
        if !ok {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 3;
        while end < t.len() && !STOP.contains(&t[end]) {
            end += 1;
        }
        i = end.max(start + 1);
        let group: String = t[start..end].iter().collect();
        // Quoted only when the quotes sit right around the path; in
        // `"C:\x.exe -arg \"y\""` the first quote belongs to the whole value.
        // In .reg files an escaped pair \"...\" also counts as quotes.
        let opener = if start > 0 { Some(t[start - 1]) } else { None };
        let closer = t.get(end).copied();
        let escaped_open = start >= 2 && t[start - 2] == '\\';
        let quoted = match opener {
            Some(q @ ('"' | '\'')) if closer == Some(q) => group.ends_with('\\') == escaped_open,
            _ => false,
        };
        let raw = clean(&group);
        if raw.chars().count() < 3 {
            continue;
        }
        let mut cands = vec![raw.clone()];
        if !quoted {
            let mut cut = raw;
            while let Some(pos) = cut.rfind(' ') {
                cut = clean(&cut[..pos]);
                if cut.chars().count() >= 3 {
                    cands.push(cut.clone());
                }
            }
        }
        if seen.insert(norm(&cands[0])) {
            found.push(PathRef { candidates: cands });
        }
    }
    found
}

/// Decode Windows-1252 (Python's "replace" for the five undefined bytes).
pub fn cp1252_decode(data: &[u8]) -> String {
    const HIGH: [u16; 32] = [
        0x20AC, 0xFFFD, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0xFFFD, 0x017D, 0xFFFD,
        0xFFFD, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0xFFFD, 0x017E, 0x0178,
    ];
    data.iter()
        .map(|&b| if (0x80..0xA0).contains(&b) { char::from_u32(HIGH[(b - 0x80) as usize] as u32).unwrap() } else { b as char })
        .collect()
}

/// Encode back to Windows-1252; None when a character has no byte.
pub fn cp1252_encode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        let u = c as u32;
        if u < 0x80 || (0xA0..0x100).contains(&u) {
            out.push(u as u8);
            continue;
        }
        let b = (0x80u8..0xA0).find(|&b| cp1252_decode(&[b]).starts_with(c) && c != '\u{FFFD}')?;
        out.push(b);
    }
    Some(out)
}

pub fn looks_utf16le(data: &[u8]) -> bool {
    let head = &data[..data.len().min(200)];
    if head.len() < 4 {
        return false;
    }
    let odd_zero = head.iter().skip(1).step_by(2).filter(|&&b| b == 0).count();
    let even_zero = head.iter().step_by(2).filter(|&&b| b == 0).count();
    odd_zero > head.len() / 4 && even_zero == 0
}

/// UTF-16 code units of `data` (an odd last byte is dropped).
pub fn u16s(data: &[u8], big: bool) -> Vec<u16> {
    data.as_chunks::<2>().0.iter().map(|c| if big { u16::from_be_bytes(*c) } else { u16::from_le_bytes(*c) }).collect()
}

fn utf16(data: &[u8], big: bool) -> String {
    String::from_utf16_lossy(&u16s(data, big))
}

/// The text of a file's first bytes; None for binary data.
pub fn decode_text(data: &[u8]) -> Option<String> {
    if let Some(d) = data.strip_prefix(b"\xff\xfe") {
        return Some(utf16(d, false));
    }
    if let Some(d) = data.strip_prefix(b"\xfe\xff") {
        return Some(utf16(d, true));
    }
    if let Some(d) = data.strip_prefix(b"\xef\xbb\xbf") {
        return Some(String::from_utf8_lossy(d).into_owned());
    }
    if looks_utf16le(data) {
        return Some(utf16(data, false));
    }
    if data.contains(&0) {
        return None;
    }
    match std::str::from_utf8(data) {
        Ok(s) => Some(s.to_string()),
        Err(_) => Some(cp1252_decode(data)),
    }
}

/// Read a text file up to `limit` bytes; None for binary or unreadable.
pub fn read_text(path: &str, limit: usize) -> Option<String> {
    use std::io::Read;
    let f = std::fs::File::open(path).ok()?;
    let mut data = Vec::new();
    f.take(limit as u64).read_to_end(&mut data).ok()?;
    decode_text(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_and_unquoted() {
        let refs = extract_paths(r#""C:\Program Files\App\app.exe" --x  D:\tools\bin\x.exe --flag"#);
        assert_eq!(refs[0].candidates, vec![r"C:\Program Files\App\app.exe"]);
        assert_eq!(refs[1].candidates.last().unwrap(), r"D:\tools\bin\x.exe");
    }

    #[test]
    fn json_escapes_and_slashes() {
        let refs = extract_paths(r#"{"a": "E:\\Portable\\scoop\\apps", "b": "D:/x/y.exe"}"#);
        let firsts: Vec<_> = refs.iter().map(|r| r.candidates[0].as_str()).collect();
        assert_eq!(firsts, vec![r"E:\Portable\scoop\apps", r"D:\x\y.exe"]);
    }

    #[test]
    fn reg_escaped_quotes() {
        let text = r#"@="\"Q:\\Portable\\scoop\\apps\\a\\a.exe\" \"%1\"""#;
        assert_eq!(extract_paths(text)[0].candidates[0], r"Q:\Portable\scoop\apps\a\a.exe");
        let text = r#"@="\"C:\\Program Files\\App\\app.exe\" \"%1\"""#;
        assert_eq!(extract_paths(text)[0].candidates, vec![r"C:\Program Files\App\app.exe"]);
    }

    #[test]
    fn reg_value_quote_is_not_path_quote() {
        let text = r#"@="D:\\scoop\\apps\\pwsh\\current\\pwsh.exe -NoExit -WorkingDirectory \"%V!\"""#;
        assert_eq!(extract_paths(text)[0].candidates.last().unwrap(), r"D:\scoop\apps\pwsh\current\pwsh.exe");
    }

    #[test]
    fn other_root_pattern() {
        assert_eq!(scoop_root_of(r"D:\Portable\scoop\apps\git\current\x.exe").as_deref(), Some(r"D:\Portable\scoop"));
        assert_eq!(scoop_root_of(r"D:\Portable\myscoop\apps\x"), None);
        assert_eq!(scoop_root_of(r"D:\Portable\scoop\other\x"), None);
    }

    #[test]
    fn normalising() {
        assert_eq!(norm("E:/Portable//Scoop/"), norm(r"e:\portable\scoop"));
        assert_eq!(norm(r"C:\a\b\..\c\.\d"), r"c:\a\c\d");
        assert_eq!(relative_to(r"Q:\Portable\Scoop\apps\x", r"q:\portable\scoop").as_deref(), Some(r"apps\x"));
        assert!(is_under(r"c:\a\b", r"c:\a") && !is_under(r"c:\ab", r"c:\a"));
    }

    #[test]
    fn cp1252_round_trip() {
        let s = cp1252_decode(b"caf\xe9 \x80");
        assert_eq!(s, "café €");
        assert_eq!(cp1252_encode(&s).unwrap(), b"caf\xe9 \x80");
    }
}
