//! Text decoding: encoding detection (BOM → UTF-16 heuristics → UTF-8 →
//! the system code page) and line-ending normalization. Invalid byte
//! sequences become U+FFFD.

use encoding_rs::Encoding;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enc {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    /// A single/multi-byte legacy encoding (Windows-1252, Shift_JIS, ...).
    Legacy(&'static Encoding),
}

impl Enc {
    pub fn name(self) -> String {
        match self {
            Enc::Utf8 => "UTF-8".into(),
            Enc::Utf8Bom => crate::i18n::tr("UTF-8 with BOM"),
            Enc::Utf16Le => "UTF-16 LE".into(),
            Enc::Utf16Be => "UTF-16 BE".into(),
            Enc::Legacy(e) => legacy_label(e),
        }
    }
}

fn legacy_label(e: &'static Encoding) -> String {
    use encoding_rs::*;
    let desc = if e == WINDOWS_1252 {
        crate::i18n::tr("Western")
    } else if e == WINDOWS_1250 || e == ISO_8859_2 {
        crate::i18n::tr("Central European")
    } else if e == WINDOWS_1251 || e == ISO_8859_5 || e == KOI8_R || e == KOI8_U || e == IBM866 {
        crate::i18n::tr("Cyrillic")
    } else if e == WINDOWS_1253 || e == ISO_8859_7 {
        crate::i18n::tr("Greek")
    } else if e == WINDOWS_1254 {
        crate::i18n::tr("Turkish")
    } else if e == WINDOWS_1255 {
        crate::i18n::tr("Hebrew")
    } else if e == WINDOWS_1256 {
        crate::i18n::tr("Arabic")
    } else if e == WINDOWS_1257 {
        crate::i18n::tr("Baltic")
    } else if e == WINDOWS_1258 {
        crate::i18n::tr("Vietnamese")
    } else if e == WINDOWS_874 {
        crate::i18n::tr("Thai")
    } else if e == ISO_8859_15 {
        crate::i18n::tr("Western (Latin-9)")
    } else if e == SHIFT_JIS || e == EUC_JP {
        crate::i18n::tr("Japanese")
    } else if e == GBK || e == GB18030 {
        crate::i18n::tr("Chinese Simplified")
    } else if e == BIG5 {
        crate::i18n::tr("Chinese Traditional")
    } else if e == EUC_KR {
        crate::i18n::tr("Korean")
    } else {
        String::new()
    };
    if desc.is_empty() {
        e.name().to_string()
    } else {
        format!("{} ({desc})", e.name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    Lf,
    CrLf,
    Cr,
}

impl Eol {
    pub fn name(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::CrLf => "CRLF",
            Eol::Cr => "CR",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    /// Text with every line ending normalized to `\n`.
    pub text: String,
    pub enc: Enc,
    /// Predominant line ending in the file.
    pub eol: Eol,
}

/// The system's ANSI code page as an encoding (Windows-1252 elsewhere).
pub fn system_legacy() -> &'static Encoding {
    #[cfg(windows)]
    {
        // SAFETY: GetACP has no preconditions.
        let cp = unsafe { windows_sys::Win32::Globalization::GetACP() };
        let label = match cp {
            932 => "shift_jis".to_string(),
            936 => "gbk".to_string(),
            949 => "euc-kr".to_string(),
            950 => "big5".to_string(),
            874 => "windows-874".to_string(),
            1250..=1258 => format!("windows-{cp}"),
            _ => "windows-1252".to_string(),
        };
        Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::WINDOWS_1252)
    }
    #[cfg(not(windows))]
    {
        encoding_rs::WINDOWS_1252
    }
}

fn looks_utf16(bytes: &[u8]) -> Option<Enc> {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let sample = &bytes[..bytes.len().min(64 * 1024)];
    let pairs = sample.len() / 2;
    let even_zero = sample.chunks(2).filter(|p| p[0] == 0 && p[1] != 0).count();
    let odd_zero = sample.chunks(2).filter(|p| p[1] == 0 && p[0] != 0).count();
    // Mostly-ASCII UTF-16 text has a zero in every other byte.
    if odd_zero * 10 > pairs * 4 && even_zero * 10 < pairs {
        Some(Enc::Utf16Le)
    } else if even_zero * 10 > pairs * 4 && odd_zero * 10 < pairs {
        Some(Enc::Utf16Be)
    } else {
        None
    }
}

/// Detects the encoding and decodes, normalizing line endings.
pub fn decode(bytes: &[u8], fallback: &'static Encoding) -> Decoded {
    let (enc, body) = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (Enc::Utf8Bom, rest)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        (Enc::Utf16Le, rest)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (Enc::Utf16Be, rest)
    } else if let Some(e) = looks_utf16(bytes) {
        // Checked first: ASCII-only UTF-16 is also valid UTF-8 (with NULs).
        (e, bytes)
    } else if std::str::from_utf8(bytes).is_ok() {
        (Enc::Utf8, bytes)
    } else {
        (Enc::Legacy(fallback), bytes)
    };
    let raw = match enc {
        Enc::Utf8 | Enc::Utf8Bom => String::from_utf8_lossy(body).into_owned(),
        Enc::Utf16Le | Enc::Utf16Be => {
            let le = enc == Enc::Utf16Le;
            let units = body.chunks(2).map(|p| if p.len() == 2 { if le { u16::from_le_bytes([p[0], p[1]]) } else { u16::from_be_bytes([p[0], p[1]]) } } else { 0xFFFD });
            char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
        }
        Enc::Legacy(e) => e.decode_without_bom_handling(body).0.into_owned(),
    };
    let (text, counts) = normalize_eol(&raw);
    let eol = if counts[1] >= counts[0] && counts[1] >= counts[2] && counts[1] > 0 {
        Eol::CrLf
    } else if counts[2] > counts[0] {
        Eol::Cr
    } else if counts[0] > 0 || !cfg!(windows) {
        Eol::Lf
    } else {
        Eol::CrLf
    };
    Decoded { text, enc, eol }
}

/// Converts CRLF and CR to LF. Returns the text and (LF, CRLF, CR) counts.
pub fn normalize_eol(s: &str) -> (String, [usize; 3]) {
    let mut counts = [0usize; 3];
    if !s.contains('\r') {
        counts[0] = s.bytes().filter(|b| *b == b'\n').count();
        return (s.to_string(), counts);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                    counts[1] += 1;
                } else {
                    counts[2] += 1;
                }
                out.push('\n');
            }
            '\n' => {
                counts[0] += 1;
                out.push('\n');
            }
            c => out.push(c),
        }
    }
    (out, counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::WINDOWS_1252;

    fn dec(b: &[u8]) -> Decoded {
        decode(b, WINDOWS_1252)
    }

    #[test]
    fn boms_utf8_utf16_and_legacy() {
        assert_eq!(dec(b"\xEF\xBB\xBFhi").enc, Enc::Utf8Bom);
        assert_eq!(dec(b"\xFF\xFEh\0i\0").text, "hi");
        assert_eq!(dec(b"\xFE\xFF\0h\0i").text, "hi");
        assert_eq!(dec("café".as_bytes()).enc, Enc::Utf8);
        let le: Vec<u8> = "hello world\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let d = dec(&le);
        assert_eq!((d.enc, d.text.as_str(), d.eol), (Enc::Utf16Le, "hello world\n", Eol::CrLf));
        let d = dec(b"caf\xE9");
        assert_eq!((d.enc, d.text.as_str()), (Enc::Legacy(WINDOWS_1252), "café"));
    }

    #[test]
    fn line_endings() {
        assert_eq!(dec(b"a\r\nb\r\nc\n").eol, Eol::CrLf);
        let d = dec(b"a\rb\rc");
        assert_eq!((d.text.as_str(), d.eol), ("a\nb\nc", Eol::Cr));
        assert_eq!(dec(b"a\nb").eol, Eol::Lf);
        assert_eq!(normalize_eol("a\r\nb\rc\n").1, [1, 1, 1]);
    }
}
