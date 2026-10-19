//! Character encodings and line endings.
//!
//! Decoding never silently loses data: invalid byte sequences are replaced
//! with U+FFFD and the document is flagged as `lossy` so the UI can warn
//! before saving. Encoding refuses characters the target encoding cannot
//! represent instead of writing substitutes.

use encoding_rs::{Encoding, EncoderResult};

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
            Enc::Utf8Bom => "UTF-8 with BOM".into(),
            Enc::Utf16Le => "UTF-16 LE".into(),
            Enc::Utf16Be => "UTF-16 BE".into(),
            Enc::Legacy(e) => legacy_label(e),
        }
    }

    /// Stable identifier for settings files.
    pub fn id(self) -> String {
        match self {
            Enc::Utf8 => "utf-8".into(),
            Enc::Utf8Bom => "utf-8-bom".into(),
            Enc::Utf16Le => "utf-16le".into(),
            Enc::Utf16Be => "utf-16be".into(),
            Enc::Legacy(e) => e.name().to_ascii_lowercase(),
        }
    }

    pub fn from_id(id: &str) -> Option<Enc> {
        Some(match id {
            "utf-8" => Enc::Utf8,
            "utf-8-bom" => Enc::Utf8Bom,
            "utf-16le" => Enc::Utf16Le,
            "utf-16be" => Enc::Utf16Be,
            other => {
                let e = Encoding::for_label(other.as_bytes())?;
                if e == encoding_rs::UTF_8 {
                    Enc::Utf8
                } else if e == encoding_rs::UTF_16LE {
                    Enc::Utf16Le
                } else if e == encoding_rs::UTF_16BE {
                    Enc::Utf16Be
                } else {
                    Enc::Legacy(e)
                }
            }
        })
    }

    #[cfg(test)]
    pub fn is_unicode(self) -> bool {
        !matches!(self, Enc::Legacy(_))
    }

    /// Encodings offered in the menus.
    pub fn all() -> Vec<Enc> {
        use encoding_rs::*;
        let legacy: [&'static Encoding; 22] = [
            WINDOWS_1252, WINDOWS_1250, WINDOWS_1251, WINDOWS_1253, WINDOWS_1254, WINDOWS_1255, WINDOWS_1256, WINDOWS_1257,
            WINDOWS_1258, WINDOWS_874, ISO_8859_2, ISO_8859_5, ISO_8859_7, ISO_8859_15, KOI8_R, KOI8_U, IBM866, SHIFT_JIS,
            EUC_JP, GBK, BIG5, EUC_KR,
        ];
        let mut v = vec![Enc::Utf8, Enc::Utf8Bom, Enc::Utf16Le, Enc::Utf16Be];
        v.extend(legacy.into_iter().map(Enc::Legacy));
        v
    }
}

fn legacy_label(e: &'static Encoding) -> String {
    use encoding_rs::*;
    let desc = if e == WINDOWS_1252 {
        "Western"
    } else if e == WINDOWS_1250 || e == ISO_8859_2 {
        "Central European"
    } else if e == WINDOWS_1251 || e == ISO_8859_5 || e == KOI8_R || e == KOI8_U || e == IBM866 {
        "Cyrillic"
    } else if e == WINDOWS_1253 || e == ISO_8859_7 {
        "Greek"
    } else if e == WINDOWS_1254 {
        "Turkish"
    } else if e == WINDOWS_1255 {
        "Hebrew"
    } else if e == WINDOWS_1256 {
        "Arabic"
    } else if e == WINDOWS_1257 {
        "Baltic"
    } else if e == WINDOWS_1258 {
        "Vietnamese"
    } else if e == WINDOWS_874 {
        "Thai"
    } else if e == ISO_8859_15 {
        "Western (Latin-9)"
    } else if e == SHIFT_JIS || e == EUC_JP {
        "Japanese"
    } else if e == GBK || e == GB18030 {
        "Chinese Simplified"
    } else if e == BIG5 {
        "Chinese Traditional"
    } else if e == EUC_KR {
        "Korean"
    } else {
        ""
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
    pub fn as_str(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
            Eol::Cr => "\r",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::CrLf => "CRLF",
            Eol::Cr => "CR",
        }
    }
    pub fn from_name(s: &str) -> Option<Eol> {
        [Eol::Lf, Eol::CrLf, Eol::Cr].into_iter().find(|e| e.name().eq_ignore_ascii_case(s))
    }
    /// Platform default for new documents.
    pub fn platform() -> Eol {
        if cfg!(windows) {
            Eol::CrLf
        } else {
            Eol::Lf
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    /// Text with every line ending normalized to `\n`.
    pub text: String,
    pub enc: Enc,
    /// Predominant line ending (used when saving).
    pub eol: Eol,
    /// Counts of (LF, CRLF, CR) line endings found.
    pub eol_counts: [usize; 3],
    /// Some bytes were invalid for the encoding and became U+FFFD.
    pub lossy: bool,
    /// Contains NUL characters: probably not a text file.
    pub binary: bool,
}

impl Decoded {
    pub fn mixed_eol(&self) -> bool {
        self.eol_counts.iter().filter(|c| **c > 0).count() > 1
    }
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

/// Detects the encoding (unless `forced`) and decodes, normalizing line endings.
pub fn decode(bytes: &[u8], forced: Option<Enc>, fallback: &'static Encoding) -> Decoded {
    let (enc, body) = match forced {
        Some(Enc::Utf8Bom) | Some(Enc::Utf8) => {
            let has_bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
            let e = if has_bom { Enc::Utf8Bom } else { forced.unwrap_or(Enc::Utf8) };
            (e, if has_bom { &bytes[3..] } else { bytes })
        }
        Some(Enc::Utf16Le) => (Enc::Utf16Le, bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes)),
        Some(Enc::Utf16Be) => (Enc::Utf16Be, bytes.strip_prefix(&[0xFE, 0xFF]).unwrap_or(bytes)),
        Some(e @ Enc::Legacy(_)) => (e, bytes),
        None => {
            if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
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
            }
        }
    };
    let (raw, lossy) = match enc {
        Enc::Utf8 | Enc::Utf8Bom => match std::str::from_utf8(body) {
            Ok(s) => (s.to_string(), false),
            Err(_) => (String::from_utf8_lossy(body).into_owned(), true),
        },
        Enc::Utf16Le | Enc::Utf16Be => {
            let le = enc == Enc::Utf16Le;
            let units: Vec<u16> = body
                .chunks(2)
                .map(|p| if p.len() == 2 { if le { u16::from_le_bytes([p[0], p[1]]) } else { u16::from_be_bytes([p[0], p[1]]) } } else { 0xFFFD })
                .collect();
            let odd = body.len() % 2 == 1;
            let mut lossy = odd;
            let s: String = char::decode_utf16(units)
                .map(|r| {
                    r.unwrap_or_else(|_| {
                        lossy = true;
                        '\u{FFFD}'
                    })
                })
                .collect();
            (s, lossy)
        }
        Enc::Legacy(e) => {
            let (cow, had_errors) = e.decode_without_bom_handling(body);
            (cow.into_owned(), had_errors)
        }
    };
    let binary = raw.contains('\0');
    let (text, eol_counts) = normalize_eol(&raw);
    let eol = predominant(eol_counts);
    Decoded { text, enc, eol, eol_counts, lossy, binary }
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

fn predominant(c: [usize; 3]) -> Eol {
    if c == [0, 0, 0] {
        Eol::platform()
    } else if c[1] >= c[0] && c[1] >= c[2] {
        Eol::CrLf
    } else if c[0] >= c[2] {
        Eol::Lf
    } else {
        Eol::Cr
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EncodeError {
    /// A character the target encoding cannot represent, with its position.
    Unmappable { ch: char, line: usize, col: usize },
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncodeError::Unmappable { ch, line, col } => {
                write!(f, "the character '{ch}' (U+{:04X}) at line {line}, column {col} cannot be represented", *ch as u32)
            }
        }
    }
}

/// Incremental encoder: feed `\n`-normalized text chunks, get bytes.
pub struct Writer {
    enc: Enc,
    eol: Eol,
    legacy: Option<encoding_rs::Encoder>,
    line: usize,
    col: usize,
    tmp: String,
}

impl Writer {
    pub fn new(enc: Enc, eol: Eol) -> Writer {
        let legacy = match enc {
            Enc::Legacy(e) => Some(e.new_encoder()),
            _ => None,
        };
        Writer { enc, eol, legacy, line: 1, col: 1, tmp: String::new() }
    }

    /// Byte order mark to write before the content.
    pub fn bom(&self) -> &'static [u8] {
        match self.enc {
            Enc::Utf8Bom => &[0xEF, 0xBB, 0xBF],
            Enc::Utf16Le => &[0xFF, 0xFE],
            Enc::Utf16Be => &[0xFE, 0xFF],
            _ => &[],
        }
    }

    /// Encodes one chunk, appending to `out`.
    pub fn write(&mut self, chunk: &str, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        self.tmp.clear();
        if self.eol == Eol::Lf {
            self.tmp.push_str(chunk);
        } else {
            for c in chunk.chars() {
                if c == '\n' {
                    self.tmp.push_str(self.eol.as_str());
                } else {
                    self.tmp.push(c);
                }
            }
        }
        match self.enc {
            Enc::Utf8 | Enc::Utf8Bom => out.extend_from_slice(self.tmp.as_bytes()),
            Enc::Utf16Le => {
                for u in self.tmp.encode_utf16() {
                    out.extend_from_slice(&u.to_le_bytes());
                }
            }
            Enc::Utf16Be => {
                for u in self.tmp.encode_utf16() {
                    out.extend_from_slice(&u.to_be_bytes());
                }
            }
            Enc::Legacy(_) => {
                let enc = self.legacy.as_mut().expect("legacy encoder");
                let mut src = self.tmp.as_str();
                loop {
                    let mut buf = [0u8; 8192];
                    let (res, read, written) = enc.encode_from_utf8_without_replacement(src, &mut buf, false);
                    out.extend_from_slice(&buf[..written]);
                    let done = &src[..read];
                    match res {
                        EncoderResult::InputEmpty => break,
                        EncoderResult::OutputFull => {
                            src = &src[read..];
                        }
                        EncoderResult::Unmappable(ch) => {
                            let (line, col) = advance(self.line, self.col, done);
                            // `read` includes the unmappable char itself.
                            return Err(EncodeError::Unmappable { ch, line, col: col.saturating_sub(1).max(1) });
                        }
                    }
                }
            }
        }
        let (l, c) = advance(self.line, self.col, chunk);
        self.line = l;
        self.col = c;
        Ok(())
    }

    /// Flushes stateful encoders (e.g. ISO-2022-JP shift sequences).
    pub fn finish(&mut self, out: &mut Vec<u8>) {
        if let Some(enc) = self.legacy.as_mut() {
            let mut buf = [0u8; 64];
            let (_, _, written) = enc.encode_from_utf8_without_replacement("", &mut buf, true);
            out.extend_from_slice(&buf[..written]);
        }
    }
}

fn advance(mut line: usize, mut col: usize, s: &str) -> (usize, usize) {
    for c in s.chars() {
        if c == '\n' || c == '\r' {
            if c == '\n' {
                line += 1;
                col = 1;
            }
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Encodes a whole `\n`-normalized string (convenience for tests and small files).
#[cfg(test)]
pub fn encode(text: &str, enc: Enc, eol: Eol) -> Result<Vec<u8>, EncodeError> {
    let mut w = Writer::new(enc, eol);
    let mut out = w.bom().to_vec();
    w.write(text, &mut out)?;
    w.finish(&mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::WINDOWS_1252;

    fn dec(b: &[u8]) -> Decoded {
        decode(b, None, WINDOWS_1252)
    }

    #[test]
    fn detects_boms_and_utf8() {
        assert_eq!(dec(b"\xEF\xBB\xBFhi").enc, Enc::Utf8Bom);
        assert_eq!(dec(b"\xEF\xBB\xBFhi").text, "hi");
        assert_eq!(dec(b"\xFF\xFEh\0i\0").enc, Enc::Utf16Le);
        assert_eq!(dec(b"\xFF\xFEh\0i\0").text, "hi");
        assert_eq!(dec(b"\xFE\xFF\0h\0i").text, "hi");
        let d = dec("caf\u{e9} \u{1F600}".as_bytes());
        assert_eq!((d.enc, d.lossy), (Enc::Utf8, false));
    }

    #[test]
    fn detects_bomless_utf16_and_legacy() {
        let le: Vec<u8> = "hello world\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let d = dec(&le);
        assert_eq!((d.enc, d.text.as_str(), d.eol), (Enc::Utf16Le, "hello world\n", Eol::CrLf));
        let be: Vec<u8> = "hello world".encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        assert_eq!(dec(&be).enc, Enc::Utf16Be);
        // "café" in Windows-1252 is not valid UTF-8.
        let d = dec(b"caf\xE9");
        assert_eq!((d.enc, d.text.as_str(), d.lossy), (Enc::Legacy(WINDOWS_1252), "café", false));
    }

    #[test]
    fn forced_utf8_on_invalid_bytes_is_flagged_lossy() {
        let d = decode(b"caf\xE9", Some(Enc::Utf8), WINDOWS_1252);
        assert!(d.lossy);
        assert_eq!(d.text, "caf\u{FFFD}");
    }

    #[test]
    fn line_endings() {
        let d = dec(b"a\r\nb\r\nc\nd");
        assert_eq!(d.text, "a\nb\nc\nd");
        assert_eq!(d.eol_counts, [1, 2, 0]);
        assert_eq!(d.eol, Eol::CrLf);
        assert!(d.mixed_eol());
        let d = dec(b"a\rb\rc");
        assert_eq!((d.text.as_str(), d.eol), ("a\nb\nc", Eol::Cr));
        assert!(!d.mixed_eol());
        assert_eq!(dec(b"x\ny").eol, Eol::Lf);
    }

    #[test]
    fn round_trips() {
        let text = "Grüße, 世界 😀\nline 2\n";
        for enc in [Enc::Utf8, Enc::Utf8Bom, Enc::Utf16Le, Enc::Utf16Be] {
            for eol in [Eol::Lf, Eol::CrLf, Eol::Cr] {
                let bytes = encode(text, enc, eol).unwrap();
                let back = decode(&bytes, None, WINDOWS_1252);
                assert_eq!(back.text, text, "{enc:?} {eol:?}");
                assert_eq!(back.enc, enc);
                assert_eq!(back.eol, eol);
                assert!(!back.lossy);
            }
        }
        let bytes = encode("café\n", Enc::Legacy(WINDOWS_1252), Eol::CrLf).unwrap();
        assert_eq!(bytes, b"caf\xE9\r\n");
        let sjis = Enc::Legacy(encoding_rs::SHIFT_JIS);
        let bytes = encode("日本語", sjis, Eol::Lf).unwrap();
        assert_eq!(decode(&bytes, Some(sjis), WINDOWS_1252).text, "日本語");
    }

    #[test]
    fn unmappable_chars_are_refused() {
        let err = encode("ok\nab€😀", Enc::Legacy(encoding_rs::WINDOWS_1251), Eol::Lf).unwrap_err();
        assert_eq!(err, EncodeError::Unmappable { ch: '😀', line: 2, col: 4 });
        assert!(encode("€", Enc::Legacy(WINDOWS_1252), Eol::Lf).is_ok());
    }

    #[test]
    fn binary_detection_and_ids() {
        assert!(dec(b"\x00\x01\x02text\x00").binary);
        for e in Enc::all() {
            assert_eq!(Enc::from_id(&e.id()), Some(e), "{}", e.name());
        }
        assert_eq!(Enc::Legacy(WINDOWS_1252).name(), "windows-1252 (Western)");
    }
}
