//! Which files are compared: a range of sizes and of dates, extensions and
//! names to keep or to leave out, and kinds of file (images, videos...).
//!
//! What the user typed is kept as text ([`Choice`], saved in the settings)
//! and turned into a [`Filter`] when a search starts.

use rt_core::i18n::tr;
use std::collections::HashSet;

/// A kind of file to choose with one tick: the label (looked up in the
/// table), its name in the settings, and its extensions (patterns allowed).
pub struct FileKind {
    pub label: &'static str,
    pub id: &'static str,
    pub exts: &'static [&'static str],
}

pub const KINDS: [FileKind; 7] = [
    FileKind {
        label: "Images",
        id: "images",
        exts: &[
            "jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "webp", "tif", "tiff", "svg", "svgz", "ico", "heic", "heif", "avif", "jxl", "psd",
            "xcf", "raw", "dng", "cr2", "cr3", "nef", "arw", "orf", "rw2",
        ],
    },
    FileKind {
        label: "Videos",
        id: "videos",
        exts: &[
            "mp4", "m4v", "mpg", "mpeg", "mpe", "mkv", "avi", "mov", "wmv", "flv", "webm", "vob", "ts", "m2ts", "mts", "3gp", "ogv", "rm", "rmvb",
            "divx", "vid",
        ],
    },
    FileKind {
        label: "Audio",
        id: "audio",
        exts: &["mp3", "m4a", "aac", "wav", "flac", "ogg", "oga", "opus", "wma", "aif", "aiff", "ape", "mid", "midi", "amr", "aud"],
    },
    FileKind {
        label: "Archives",
        id: "archives",
        exts: &["zip", "7z", "rar", "gz", "tgz", "bz2", "tbz2", "xz", "txz", "zst", "lz", "lzma", "tar", "cab", "iso", "img", "dmg", "arj", "lzh"],
    },
    FileKind {
        label: "Office",
        id: "office",
        exts: &["doc*", "dot*", "xl*", "ppt*", "pps*", "pot*", "mdb", "accdb", "pdf", "csv", "db", "rtf", "odt", "ods", "odp", "odg", "epub", "txt"],
    },
    FileKind {
        label: "Programming",
        id: "code",
        exts: &[
            "py", "c", "cc", "cpp", "cxx", "h", "hpp", "cs", "rs", "go", "java", "kt", "swift", "ps1", "psm1", "php", "sh", "bat", "cmd", "vbs",
            "pas", "lua", "rb", "pl", "sql", "js", "mjs", "cjs", "tsx", "jsx", "node", "html", "htm", "css", "json", "toml", "yaml", "yml", "xml",
            "ini", "jar", "class", "crate", "cargo", "pdb", "ipynb",
        ],
    },
    FileKind {
        label: "System",
        id: "system",
        exts: &[
            "dll", "sys", "exe", "msi", "msp", "so", "bin", "reg", "inf", "cat", "drv", "ocx", "cpl", "efi", "mui", "vhd", "vhdx", "ttf", "otf",
            "fon", "cer", "crt",
        ],
    },
];

/// Patterns, without case: `*` stands for any text and `?` for one character.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Patterns {
    exact: HashSet<String>,
    wild: Vec<Vec<char>>,
}

impl Patterns {
    /// Patterns separated by spaces, commas or semicolons.
    pub fn parse(text: &str) -> Patterns {
        let mut p = Patterns::default();
        for word in words(text) {
            p.add(word);
        }
        p
    }

    /// Extensions, with or without their dot (`.tmp`, `*.tmp` and `tmp` are one).
    pub fn parse_exts(text: &str) -> Patterns {
        let mut p = Patterns::default();
        for word in words(text) {
            p.add_ext(word);
        }
        p
    }

    fn add_ext(&mut self, word: &str) {
        let w = ext_key(word);
        if !w.is_empty() {
            self.add(&w);
        }
    }

    fn add(&mut self, word: &str) {
        let w = word.to_lowercase();
        if w.contains(['*', '?']) {
            self.wild.push(w.chars().collect());
        } else {
            self.exact.insert(w);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.wild.is_empty()
    }

    /// Whether `lower` (already in lower case) matches one of the patterns.
    pub fn matches(&self, lower: &str) -> bool {
        if self.exact.contains(lower) {
            return true;
        }
        if self.wild.is_empty() {
            return false;
        }
        let s: Vec<char> = lower.chars().collect();
        self.wild.iter().any(|p| glob(p, &s))
    }
}

/// An extension as compared: without its dot and in lower case (`*.TMP` is `tmp`).
fn ext_key(word: &str) -> String {
    word.trim_start_matches('*').trim_start_matches('.').to_lowercase()
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|w| !w.is_empty())
}

/// Whether `s` matches the pattern `p` (`*` any text, `?` one character).
fn glob(p: &[char], s: &[char]) -> bool {
    let (mut pi, mut si) = (0, 0);
    // Where the last `*` was, and the text it took up to then.
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, si));
            pi += 1;
        } else if let Some((sp, ss)) = star {
            // The `*` takes one more character.
            pi = sp + 1;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Which files are compared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    /// Sizes, inclusive (empty files are always left out).
    pub min_size: u64,
    pub max_size: u64,
    /// Last change from `after` (inclusive) to `before` (exclusive), seconds since 1970.
    pub after: i64,
    pub before: i64,
    /// Extensions to keep (all when empty) and to leave out.
    pub only_ext: Patterns,
    pub skip_ext: Patterns,
    /// Names of files or folders: files are kept when their name or the name
    /// of a folder they are in matches (all when empty); a folder left out is
    /// not looked into.
    pub only_names: Patterns,
    pub skip_names: Patterns,
}

impl Default for Filter {
    fn default() -> Filter {
        Filter {
            min_size: 1,
            max_size: u64::MAX,
            after: i64::MIN,
            before: i64::MAX,
            only_ext: Patterns::default(),
            skip_ext: Patterns::default(),
            only_names: Patterns::default(),
            skip_names: Patterns::default(),
        }
    }
}

impl Filter {
    /// Files of `min_size` bytes or more, all others alike.
    pub fn min(min_size: u64) -> Filter {
        Filter { min_size: min_size.max(1), ..Filter::default() }
    }

    /// Whether it leaves out more than small files.
    pub fn narrows(&self) -> bool {
        self.max_size != u64::MAX
            || self.after != i64::MIN
            || self.before != i64::MAX
            || !self.only_ext.is_empty()
            || !self.skip_ext.is_empty()
            || !self.only_names.is_empty()
            || !self.skip_names.is_empty()
    }

    /// Whether a file of `size` bytes changed at `mtime` can be kept.
    pub fn keeps(&self, size: u64, mtime: i64) -> bool {
        size >= self.min_size.max(1) && self.keeps_any_size(size, mtime)
    }

    /// Whether a file can be kept whatever the smallest size (not when empty):
    /// the files that count in a folder.
    pub fn keeps_any_size(&self, size: u64, mtime: i64) -> bool {
        size >= 1 && size <= self.max_size && mtime >= self.after && mtime < self.before
    }

    /// Whether the extension of the file `name` is kept.
    pub fn keeps_ext(&self, name: &str) -> bool {
        if self.only_ext.is_empty() && self.skip_ext.is_empty() {
            return true;
        }
        let ext = match name.rsplit_once('.') {
            Some((_, e)) => e.to_lowercase(),
            None => String::new(),
        };
        (self.only_ext.is_empty() || (!ext.is_empty() && self.only_ext.matches(&ext))) && !(!ext.is_empty() && self.skip_ext.matches(&ext))
    }

    /// Whether names are looked at.
    pub fn has_names(&self) -> bool {
        !self.only_names.is_empty() || !self.skip_names.is_empty()
    }
}

/// The text boxes of the filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    MinSize,
    MaxSize,
    From,
    To,
    OnlyExt,
    SkipExt,
    OnlyNames,
    SkipNames,
}

impl Field {
    pub const ALL: [Field; 8] =
        [Field::MinSize, Field::MaxSize, Field::From, Field::To, Field::OnlyExt, Field::SkipExt, Field::OnlyNames, Field::SkipNames];

    /// Its name in the settings.
    pub fn key(self) -> &'static str {
        match self {
            Field::MinSize => "min_size",
            Field::MaxSize => "max_size",
            Field::From => "date_from",
            Field::To => "date_to",
            Field::OnlyExt => "only_ext",
            Field::SkipExt => "skip_ext",
            Field::OnlyNames => "only_names",
            Field::SkipNames => "skip_names",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Why the filters cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    Size,
    Date,
    /// The largest size is below the smallest.
    SizeOrder,
    /// The last date is before the first.
    DateOrder,
    /// This extension or name is both to keep and to leave out.
    Both(String),
}

/// A filter that cannot be used: which box, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bad {
    pub field: Field,
    pub why: Why,
}

impl Bad {
    /// Whether the box `f` is shown as wrong: the box named, and for a word in
    /// both, the box to keep beside the one to leave out.
    pub fn marks(&self, f: Field) -> bool {
        let pair = match (self.field, &self.why) {
            (Field::SkipExt, Why::Both(_)) => Some(Field::OnlyExt),
            (Field::SkipNames, Why::Both(_)) => Some(Field::OnlyNames),
            _ => None,
        };
        f == self.field || Some(f) == pair
    }
}

/// The filters as typed: the text of each box. The kinds of file are not
/// kept apart: ticking one writes its extensions in the box of extensions to
/// keep, and it shows as ticked while they are all there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub texts: [String; 8],
}

/// The smallest size when nothing was chosen yet: below a few hundred
/// kilobytes nearly every file shares its size with another.
pub const DEFAULT_MIN: &str = "1 MB";

impl Default for Choice {
    fn default() -> Choice {
        let mut texts: [String; 8] = Default::default();
        texts[Field::MinSize.index()] = DEFAULT_MIN.to_string();
        Choice { texts }
    }
}

/// The first word of `a` also in `b`, as compared by `key`.
fn in_both(a: &str, b: &str, key: fn(&str) -> String) -> Option<String> {
    let other: HashSet<String> = words(b).map(key).collect();
    words(a).find(|w| other.contains(&key(w))).map(str::to_string)
}

impl Choice {
    pub fn text(&self, f: Field) -> &str {
        &self.texts[f.index()]
    }

    fn ext_set(&self) -> HashSet<String> {
        words(self.text(Field::OnlyExt)).map(ext_key).collect()
    }

    /// Whether all the extensions of the kind `i` are among those to keep.
    pub fn has_kind(&self, i: usize) -> bool {
        let set = self.ext_set();
        KINDS[i].exts.iter().all(|e| set.contains(*e))
    }

    /// Adds the extensions of the kind `i` to those to keep, or takes them out
    /// when they are all there.
    pub fn toggle_kind(&mut self, i: usize) {
        let k = &KINDS[i];
        let on = self.has_kind(i);
        let set = self.ext_set();
        let text = &mut self.texts[Field::OnlyExt.index()];
        if on {
            *text = words(text).filter(|w| !k.exts.contains(&ext_key(w).as_str())).collect::<Vec<_>>().join(" ");
        } else {
            let mut out = text.trim_end().to_string();
            for e in k.exts.iter().filter(|e| !set.contains(**e)) {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(e);
            }
            *text = out;
        }
    }

    /// The smallest size from a quick choice (1: any size, an empty box), and
    /// no largest one.
    pub fn set_min(&mut self, text: String) {
        self.texts[Field::MinSize.index()] = text;
        self.texts[Field::MaxSize.index()].clear();
    }

    /// Everything but the smallest size back to nothing.
    pub fn clear(&mut self) {
        let min = std::mem::take(&mut self.texts[Field::MinSize.index()]);
        *self = Choice { texts: Default::default() };
        self.texts[Field::MinSize.index()] = min;
    }

    /// Whether anything other than the smallest size is chosen.
    pub fn narrows(&self) -> bool {
        Field::ALL.iter().any(|&f| f != Field::MinSize && !self.text(f).trim().is_empty())
    }

    /// The smallest size, if it is understood (1 for none).
    pub fn min_size(&self) -> Option<u64> {
        parse_size(self.text(Field::MinSize)).map(|v| v.unwrap_or(1).max(1))
    }

    pub fn filter(&self) -> Result<Filter, Bad> {
        let bad = |field, why: Why| Bad { field, why };
        let min = self.min_size().ok_or(bad(Field::MinSize, Why::Size))?;
        let max = parse_size(self.text(Field::MaxSize)).ok_or(bad(Field::MaxSize, Why::Size))?.unwrap_or(u64::MAX);
        if max < min {
            return Err(bad(Field::MaxSize, Why::SizeOrder));
        }
        let after = parse_date(self.text(Field::From), false).ok_or(bad(Field::From, Why::Date))?.unwrap_or(i64::MIN);
        let before = parse_date(self.text(Field::To), true).ok_or(bad(Field::To, Why::Date))?.unwrap_or(i64::MAX);
        if before <= after {
            return Err(bad(Field::To, Why::DateOrder));
        }
        if let Some(w) = in_both(self.text(Field::SkipExt), self.text(Field::OnlyExt), ext_key) {
            return Err(bad(Field::SkipExt, Why::Both(w)));
        }
        if let Some(w) = in_both(self.text(Field::SkipNames), self.text(Field::OnlyNames), |w| w.to_lowercase()) {
            return Err(bad(Field::SkipNames, Why::Both(w)));
        }
        Ok(Filter {
            min_size: min,
            max_size: max,
            after,
            before,
            only_ext: Patterns::parse_exts(self.text(Field::OnlyExt)),
            skip_ext: Patterns::parse_exts(self.text(Field::SkipExt)),
            only_names: Patterns::parse(self.text(Field::OnlyNames)),
            skip_names: Patterns::parse(self.text(Field::SkipNames)),
        })
    }
}

const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

/// A size such as `2000`, `500 KB`, `1.5 GB` or `1,5 Go` (binary units, also
/// as shown in the interface language). Some(None) when empty, None when not
/// understood.
pub fn parse_size(text: &str) -> Option<Option<u64>> {
    let t = text.trim();
    if t.is_empty() {
        return Some(None);
    }
    let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(t.len());
    let (num, unit) = (t[..split].replace(',', "."), t[split..].trim().to_lowercase());
    let v: f64 = num.parse().ok().filter(|v: &f64| v.is_finite() && *v >= 0.0)?;
    let power = if unit.is_empty() {
        0
    } else {
        UNITS.iter().enumerate().find_map(|(i, u)| {
            let short = &u[..1].to_lowercase();
            let names = [u.to_lowercase(), tr(u).to_lowercase(), if i > 0 { short.clone() } else { "bytes".into() }];
            names.contains(&unit).then_some(i as i32)
        })?
    };
    let bytes = v * 1024f64.powi(power);
    (bytes < u64::MAX as f64).then_some(Some(bytes.round() as u64))
}

/// A day, month or year: `2024-01-31`, `31/01/2024`, `2024-01`, `2024`. With
/// `end`, the moment the period ends (the start of the day after); otherwise
/// when it starts. Some(None) when empty, None when not understood.
pub fn parse_date(text: &str, end: bool) -> Option<Option<i64>> {
    if text.trim().is_empty() {
        return Some(None);
    }
    let (y, m, d) = date_parts(text)?;
    let start = rt_scan::format::day_start(y, m.unwrap_or(1), d.unwrap_or(1))?;
    if !end {
        return Some(Some(start));
    }
    let next = match (m, d) {
        (None, _) => rt_scan::format::day_start(y + 1, 1, 1)?,
        (Some(12), None) => rt_scan::format::day_start(y + 1, 1, 1)?,
        (Some(m), None) => rt_scan::format::day_start(y, m + 1, 1)?,
        // The start of the next day (a change of summer time moves it by an hour at most).
        (Some(_), Some(_)) => {
            let guess = start + 86_400 + 3 * 3600;
            let date = rt_scan::format::date(guess);
            let p: Vec<u32> = date.split('-').filter_map(|s| s.parse().ok()).collect();
            rt_scan::format::day_start(p[0] as i32, p[1], p[2])?
        }
    };
    Some(Some(next))
}

/// A whole day typed in a date box, as (year, month, day), if it is one.
pub fn parse_day(text: &str) -> Option<(i32, u32, u32)> {
    match date_parts(text)? {
        (y, Some(m), Some(d)) if rt_scan::format::day_start(y, m, d).is_some() => Some((y, m, d)),
        _ => None,
    }
}

/// The month of a date typed in a box (January for a year alone), if it is one.
pub fn parse_month(text: &str) -> Option<(i32, u32)> {
    let (y, m, d) = date_parts(text)?;
    rt_scan::format::day_start(y, m.unwrap_or(1), d.unwrap_or(1)).map(|_| (y, m.unwrap_or(1)))
}

/// The year, and the month and day when given, of a date as typed.
fn date_parts(text: &str) -> Option<(i32, Option<u32>, Option<u32>)> {
    let t = text.trim();
    let parts: Vec<&str> = t.split(['-', '/', '.', ' ']).filter(|p| !p.is_empty()).collect();
    if parts.iter().any(|p| !p.chars().all(|c| c.is_ascii_digit()) || p.len() > 4) {
        return None;
    }
    let num = |s: &str| s.parse::<u32>().ok();
    let year_first = parts.first().is_some_and(|p| p.len() == 4);
    let (y, m, d) = match (parts.len(), year_first) {
        (1, true) => (num(parts[0])?, None, None),
        (2, true) => (num(parts[0])?, Some(num(parts[1])?), None),
        (2, false) if parts[1].len() == 4 => (num(parts[1])?, Some(num(parts[0])?), None),
        (3, true) => (num(parts[0])?, Some(num(parts[1])?), Some(num(parts[2])?)),
        (3, false) if parts[2].len() == 4 => (num(parts[2])?, Some(num(parts[1])?), Some(num(parts[0])?)),
        _ => return None,
    };
    Some((y as i32, m, d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_core::i18n::{set_lang, Lang};

    fn g(p: &str, s: &str) -> bool {
        glob(&p.chars().collect::<Vec<_>>(), &s.chars().collect::<Vec<_>>())
    }

    #[test]
    fn patterns_match_without_case() {
        assert!(g("tmp*", "tmp"));
        assert!(g("tmp*", "tmpfile.txt"));
        assert!(!g("tmp*", "atmp"));
        assert!(g("*cache*", "my cache folder"));
        assert!(g("a?c", "abc"));
        assert!(!g("a?c", "ac"));
        assert!(g("*.r*s", "lib.rs"));
        assert!(g("*a*b*c", "xaxxbxxbc"));
        assert!(!g("*a*b*c", "xaxxbxxb"));
        let p = Patterns::parse("Setup*, *Installer*;video");
        assert!(p.matches("setup.exe"));
        assert!(p.matches("the installer"));
        assert!(p.matches("video"));
        assert!(!p.matches("videos"));
        let e = Patterns::parse_exts(".tmp *.log, lnk doc*");
        for ext in ["tmp", "log", "lnk", "docx", "doc"] {
            assert!(e.matches(ext), "{ext}");
        }
        assert!(!e.matches("txt"));
    }

    #[test]
    fn sizes_are_understood() {
        set_lang(Lang::EnUS);
        assert_eq!(parse_size(""), Some(None));
        assert_eq!(parse_size("2000"), Some(Some(2000)));
        assert_eq!(parse_size("500 KB"), Some(Some(500 << 10)));
        assert_eq!(parse_size("1.5 GB"), Some(Some(3 << 29)));
        assert_eq!(parse_size("1,5gb"), Some(Some(3 << 29)));
        assert_eq!(parse_size("10m"), Some(Some(10 << 20)));
        assert_eq!(parse_size("7 bytes"), Some(Some(7)));
        assert_eq!(parse_size("2 TB"), Some(Some(2 << 40)));
        assert_eq!(parse_size("lots"), None);
        assert_eq!(parse_size("5 XB"), None);
        set_lang(Lang::FrFR);
        assert_eq!(parse_size("3 Go"), Some(Some(3 << 30)));
        assert_eq!(parse_size("3 GB"), Some(Some(3 << 30)));
        set_lang(Lang::EnUS);
    }

    #[test]
    fn dates_are_understood() {
        let day = |y, m, d| rt_scan::format::day_start(y, m, d).unwrap();
        assert_eq!(parse_date("", false), Some(None));
        assert_eq!(parse_date("2024-01-31", false), Some(Some(day(2024, 1, 31))));
        assert_eq!(parse_date("2024-01-31", true), Some(Some(day(2024, 2, 1))));
        assert_eq!(parse_date("31/12/2024", true), Some(Some(day(2025, 1, 1))));
        assert_eq!(parse_date("2024", false), Some(Some(day(2024, 1, 1))));
        assert_eq!(parse_date("2024", true), Some(Some(day(2025, 1, 1))));
        assert_eq!(parse_date("2024-02", true), Some(Some(day(2024, 3, 1))));
        assert_eq!(parse_date("12.2024", true), Some(Some(day(2025, 1, 1))));
        assert_eq!(parse_date("2024-03-30", true), Some(Some(day(2024, 3, 31))), "a change of summer time in Europe");
        for bad in ["yesterday", "2024-13-01", "2025-02-29", "1/2/3", "20245"] {
            assert_eq!(parse_date(bad, false), None, "{bad}");
        }
        assert_eq!(parse_day(" 05/12/2024 "), Some((2024, 12, 5)));
        assert_eq!(parse_day("2024-12"), None);
        assert_eq!(parse_day("2024-02-30"), None);
        assert_eq!(parse_month("2024"), Some((2024, 1)));
        assert_eq!(parse_month("06/2023"), Some((2023, 6)));
        assert_eq!(parse_month("2023-13"), None);
    }

    #[test]
    fn the_choice_becomes_a_filter() {
        set_lang(Lang::EnUS);
        let mut c = Choice::default();
        let f = c.filter().unwrap();
        assert_eq!(f.min_size, 1 << 20);
        assert!(!f.narrows() && !c.narrows());
        c.texts[Field::MaxSize.index()] = "500 KB".into();
        assert_eq!(c.filter(), Err(Bad { field: Field::MaxSize, why: Why::SizeOrder }));
        c.texts[Field::MaxSize.index()] = "2 GB".into();
        c.texts[Field::From.index()] = "2024".into();
        c.texts[Field::To.index()] = "2023".into();
        assert_eq!(c.filter(), Err(Bad { field: Field::To, why: Why::DateOrder }));
        c.texts[Field::To.index()] = "2024-06".into();
        c.texts[Field::SkipExt.index()] = "tmp".into();
        c.toggle_kind(0);
        assert!(c.has_kind(0) && !c.has_kind(1));
        let f = c.filter().unwrap();
        assert!(f.narrows() && c.narrows());
        assert!(f.keeps_ext("Photo.JPG"));
        assert!(!f.keeps_ext("notes.txt"));
        assert!(!f.keeps_ext("no extension"));
        let june = rt_scan::format::day_start(2024, 6, 15).unwrap();
        assert!(f.keeps(5 << 20, june));
        assert!(!f.keeps(5 << 20, june + 30 * 86_400));
        assert!(!f.keeps(3 << 30, june));
        c.texts[Field::MinSize.index()] = "huge".into();
        assert_eq!(c.filter().unwrap_err().why, Why::Size);
        c.clear();
        assert_eq!(c.text(Field::MinSize), "huge");
        assert!(!c.narrows());
        c.set_min("10 MB".into());
        c.texts[Field::MaxSize.index()] = "1 GB".into();
        c.set_min(String::new());
        assert_eq!((c.text(Field::MinSize), c.text(Field::MaxSize)), ("", ""), "a quick choice has no largest size");
    }

    #[test]
    fn kinds_write_their_extensions() {
        let mut c = Choice::default();
        c.texts[Field::OnlyExt.index()] = "msi .JPG".into();
        c.toggle_kind(0);
        assert!(c.has_kind(0));
        assert!(c.text(Field::OnlyExt).starts_with("msi .JPG jpeg jpe "), "jpg was there: {}", c.text(Field::OnlyExt));
        c.toggle_kind(2);
        assert!(c.has_kind(0) && c.has_kind(2));
        // Taking one extension out unticks its kind only.
        c.texts[Field::OnlyExt.index()] = c.text(Field::OnlyExt).replace(" png", "");
        assert!(!c.has_kind(0) && c.has_kind(2));
        c.toggle_kind(0);
        assert!(c.has_kind(0));
        c.toggle_kind(0);
        c.toggle_kind(2);
        assert_eq!(c.text(Field::OnlyExt), "msi");
    }

    #[test]
    fn a_word_to_keep_and_to_leave_out_is_refused() {
        let mut c = Choice::default();
        c.texts[Field::OnlyExt.index()] = ".jpg png".into();
        c.texts[Field::SkipExt.index()] = "tmp *.JPG".into();
        let b = c.filter().unwrap_err();
        assert_eq!(b, Bad { field: Field::SkipExt, why: Why::Both("*.JPG".into()) });
        assert!(b.marks(Field::OnlyExt) && b.marks(Field::SkipExt) && !b.marks(Field::OnlyNames));
        c.texts[Field::SkipExt.index()] = "jpeg".into();
        c.texts[Field::OnlyNames.index()] = "Setup* *video*".into();
        c.texts[Field::SkipNames.index()] = "*VIDEO*".into();
        assert_eq!(c.filter().unwrap_err().why, Why::Both("*VIDEO*".into()));
        c.texts[Field::SkipNames.index()] = "*video*s".into();
        assert!(c.filter().is_ok(), "only the same word is refused");
    }
}
