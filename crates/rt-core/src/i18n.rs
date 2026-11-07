//! Localization of the user interface into ten locales. Strings are looked
//! up by their US English text, so the code keeps readable literals and
//! anything untranslated falls back to English.
//!
//! Each app has its own table (a [`Catalog`], installed with [`install`]);
//! the shared crates use the common table in `strings.txt` next to this
//! file. A lookup tries the app's table first, then the common one.
//!
//! Table format: one row per string, the en-US key and then the columns of
//! [`FILE_COLUMNS`], separated by a tab (or another character chosen per
//! table). `\n` in a cell is a line break for strings that contain one.
//!
//! Menu labels carry their own `&` mnemonic in each language; Japanese and
//! Chinese use the Windows convention of a Latin mnemonic in parentheses,
//! e.g. `新規(&N)`. [`strip_mnemonic`] removes either form.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    EnUS,
    EnGB,
    DeDE,
    EsES,
    FrFR,
    ItIT,
    JaJP,
    PtBR,
    PtPT,
    ZhCN,
}

impl Lang {
    /// Picker order (by tag).
    pub const ALL: [Lang; 10] = [
        Lang::DeDE,
        Lang::EnGB,
        Lang::EnUS,
        Lang::EsES,
        Lang::FrFR,
        Lang::ItIT,
        Lang::JaJP,
        Lang::PtBR,
        Lang::PtPT,
        Lang::ZhCN,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Lang::EnUS => "en-US",
            Lang::EnGB => "en-GB",
            Lang::DeDE => "de-DE",
            Lang::EsES => "es-ES",
            Lang::FrFR => "fr-FR",
            Lang::ItIT => "it-IT",
            Lang::JaJP => "ja-JP",
            Lang::PtBR => "pt-BR",
            Lang::PtPT => "pt-PT",
            Lang::ZhCN => "zh-CN",
        }
    }

    /// Name of the language in itself (shown in the language menu).
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::EnUS => "English (United States)",
            Lang::EnGB => "English (United Kingdom)",
            Lang::DeDE => "Deutsch (Deutschland)",
            Lang::EsES => "Español (España)",
            Lang::FrFR => "Français (France)",
            Lang::ItIT => "Italiano (Italia)",
            Lang::JaJP => "日本語 (日本)",
            Lang::PtBR => "Português (Brasil)",
            Lang::PtPT => "Português (Portugal)",
            Lang::ZhCN => "中文 (中国)",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.tag().eq_ignore_ascii_case(tag))
    }

    /// Column of this language in a table row.
    pub fn column(self) -> usize {
        self as usize
    }

    /// (decimal separator, thousands separator).
    pub fn separators(self) -> (char, char) {
        match self {
            Lang::EnUS | Lang::EnGB | Lang::JaJP | Lang::ZhCN => ('.', ','),
            Lang::DeDE | Lang::EsES | Lang::ItIT | Lang::PtBR => (',', '.'),
            // CLDR uses a (narrow) no-break space for French and European Portuguese.
            Lang::FrFR => (',', '\u{202F}'),
            Lang::PtPT => (',', '\u{00A0}'),
        }
    }

    pub fn is_cjk(self) -> bool {
        matches!(self, Lang::JaJP | Lang::ZhCN)
    }

    /// Picks the closest supported language for a BCP 47 tag such as
    /// `de-AT`, `pt-PT` or `zh-Hans-CN`.
    pub fn best_match(tag: &str) -> Lang {
        if let Some(l) = Lang::from_tag(tag) {
            return l;
        }
        let lower = tag.to_ascii_lowercase();
        let primary = lower.split(['-', '_']).next().unwrap_or("");
        match primary {
            "de" => Lang::DeDE,
            "es" => Lang::EsES,
            "fr" => Lang::FrFR,
            "it" => Lang::ItIT,
            "ja" => Lang::JaJP,
            "zh" => Lang::ZhCN,
            "pt" if lower.contains("pt-pt") || lower.ends_with("-pt") => Lang::PtPT,
            "pt" => Lang::PtBR,
            "en" if ["-gb", "-ie", "-au", "-nz", "-in", "-za"].iter().any(|r| lower.ends_with(r)) => Lang::EnGB,
            _ => Lang::EnUS,
        }
    }
}

/// Column order in a table file after the en-US key.
pub const FILE_COLUMNS: [Lang; 9] = [
    Lang::EnGB,
    Lang::DeDE,
    Lang::EsES,
    Lang::FrFR,
    Lang::ItIT,
    Lang::JaJP,
    Lang::PtBR,
    Lang::PtPT,
    Lang::ZhCN,
];

/// One row: the text in each language, by [`Lang::column`]. Empty cells
/// fall back (en-GB to en-US, pt-PT to pt-BR, anything to en-US).
pub type Row = [&'static str; 10];

/// A translation table, parsed on first use.
pub struct Catalog {
    text: &'static str,
    sep: char,
    map: OnceLock<HashMap<&'static str, Row>>,
}

impl Catalog {
    /// A table whose columns are separated by `sep` (usually a tab).
    pub const fn new(text: &'static str, sep: char) -> Catalog {
        Catalog {
            text,
            sep,
            map: OnceLock::new(),
        }
    }

    /// The table file's text.
    pub fn text(&self) -> &'static str {
        self.text
    }

    /// The column separator.
    pub fn separator(&self) -> char {
        self.sep
    }

    fn map(&self) -> &HashMap<&'static str, Row> {
        self.map.get_or_init(|| {
            let mut map = HashMap::new();
            for line in self.text.lines() {
                if line.trim().is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut cells = line.split(self.sep);
                let Some(key) = cells.next() else { continue };
                let mut row: Row = [""; 10];
                row[Lang::EnUS.column()] = key;
                for (lang, cell) in FILE_COLUMNS.iter().zip(cells) {
                    row[lang.column()] = cell;
                }
                // A multi-line string is also found by its real text, with
                // the line breaks undone in every column. The table lives for
                // the whole program, so the unescaped copies can too.
                if key.contains("\\n") {
                    let unescaped: Row = row.map(|c| &*Box::leak(c.replace("\\n", "\n").into_boxed_str()));
                    map.insert(unescaped[0], unescaped);
                }
                map.insert(key, row);
            }
            map
        })
    }

    /// The row for `key`: its text as written in the table (`\n` escaped),
    /// or with real line breaks.
    pub fn row(&self, key: &str) -> Option<&Row> {
        self.map().get(key)
    }

    /// Whether the table has a row for `key`.
    pub fn contains(&self, key: &str) -> bool {
        self.map().contains_key(key)
    }

    /// Every row, in no particular order.
    pub fn rows(&self) -> impl Iterator<Item = (&'static str, &Row)> {
        self.map().iter().map(|(k, r)| (*k, r))
    }
}

/// Strings of the shared crates.
pub static COMMON: Catalog = Catalog::new(include_str!("strings.txt"), '\t');

static APP: OnceLock<&'static Catalog> = OnceLock::new();

/// Installs the app's own table. Only the first call counts: an app has one
/// table for its whole life.
pub fn install(table: &'static Catalog) {
    let _ = APP.set(table);
}

thread_local! {
    // Per thread: the UI runs on one thread, and tests can use different
    // languages in parallel without interfering.
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::EnUS) };
    /// Strings looked up without a translation, while recording (tests).
    static MISSES: RefCell<Option<BTreeSet<String>>> = const { RefCell::new(None) };
}

pub fn set_lang(l: Lang) {
    CURRENT.with(|c| c.set(l));
}

pub fn lang() -> Lang {
    CURRENT.with(Cell::get)
}

/// Starts (or stops) recording the strings this thread looks up without a
/// translation, so tests can find text that slipped through in English.
pub fn record_misses(on: bool) {
    MISSES.with(|m| *m.borrow_mut() = on.then(BTreeSet::new));
}

/// The strings recorded since the last call (see [`record_misses`]).
pub fn take_misses() -> BTreeSet<String> {
    MISSES.with(|m| m.borrow_mut().as_mut().map(std::mem::take).unwrap_or_default())
}

fn record_miss(s: &str) {
    MISSES.with(|m| {
        if let Some(set) = m.borrow_mut().as_mut() {
            set.insert(s.to_string());
        }
    });
}

/// The row for `key` in the app's table or, failing that, the common one.
pub fn row(key: &str) -> Option<&'static Row> {
    APP.get().and_then(|t| t.row(key)).or_else(|| COMMON.row(key))
}

/// Whether the app's table or the common one has a row for `key`.
pub fn has_key(key: &str) -> bool {
    row(key).is_some()
}

/// Translation in a specific language, with fallbacks
/// (en-GB → en-US, pt-PT → pt-BR, anything → en-US).
pub fn tr_in(l: Lang, s: &str) -> &str {
    let Some(row) = row(s) else {
        record_miss(s);
        return s;
    };
    let pick = |l: Lang| Some(row[l.column()]).filter(|v| !v.is_empty());
    pick(l)
        .or_else(|| if l == Lang::PtPT { pick(Lang::PtBR) } else { None })
        .unwrap_or(s)
}

/// Translates `s` into the current language (unknown strings pass through).
pub fn tr(s: &str) -> &str {
    match lang() {
        Lang::EnUS => s,
        l => tr_in(l, s),
    }
}

/// Fills `{}` placeholders in order.
pub fn fill(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    let mut i = 0;
    while let Some(p) = rest.find("{}") {
        out.push_str(&rest[..p]);
        if let Some(a) = args.get(i) {
            out.push_str(a);
        }
        i += 1;
        rest = &rest[p + 2..];
    }
    out.push_str(rest);
    out
}

/// Translates a template and fills `{}` placeholders in order.
pub fn tf(template: &str, args: &[&str]) -> String {
    fill(tr(template), args)
}

/// Removes menu mnemonics: `&Open` → `Open`, `開く(&O)...` → `開く...`,
/// `&&` → `&`.
pub fn strip_mnemonic(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '(' && i + 3 < chars.len() && chars[i + 1] == '&' && chars[i + 3] == ')' {
            i += 4;
            continue;
        }
        if c == '&' {
            if chars.get(i + 1) == Some(&'&') {
                out.push('&');
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Checks a table file: ten columns, no duplicate keys, every required
/// language filled in, and the same `{}` placeholders and line breaks in
/// each translation. Menu labels (keys with an access key) need exactly one
/// access key in every language. Returns the problems found.
pub fn check_table(text: &str, sep: char) -> Vec<String> {
    let mut problems = Vec::new();
    let mut keys = BTreeSet::new();
    for (n, line) in text.lines().enumerate().map(|(n, l)| (n + 1, l)) {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = line.split(sep).collect();
        if cells.len() != 10 {
            problems.push(format!("line {n}: expected 10 columns, found {}", cells.len()));
            continue;
        }
        if !keys.insert(cells[0]) {
            problems.push(format!("line {n}: duplicate key {}", cells[0]));
        }
        let placeholders = cells[0].matches("{}").count();
        let menu = access_keys(cells[0]) > 0;
        for (i, c) in cells.iter().enumerate().skip(1) {
            // en-GB and pt-PT may be empty (fallback); every other language is required.
            let optional = i == 1 || i == 8;
            if c.is_empty() {
                if !optional {
                    problems.push(format!("line {n}: missing translation in column {}", i + 1));
                }
                continue;
            }
            if c.matches("{}").count() != placeholders {
                problems.push(format!("line {n}: placeholder mismatch in column {}: {c}", i + 1));
            }
            if c.trim() != *c {
                problems.push(format!("line {n}: stray whitespace in column {}", i + 1));
            }
            if c.matches("\\n").count() != cells[0].matches("\\n").count() {
                problems.push(format!("line {n}: line breaks differ in column {}", i + 1));
            }
            if menu && access_keys(c) != 1 {
                problems.push(format!(
                    "line {n}: menu label needs exactly one access key in column {}: {c}",
                    i + 1
                ));
            }
        }
    }
    problems
}

/// Number of menu access keys (`&x`, not `&&` or an entity like `&amp;`).
pub fn access_keys(s: &str) -> usize {
    let b = s.as_bytes();
    let mut n = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'&' {
            if b.get(i + 1) == Some(&b'&') {
                i += 2;
                continue;
            }
            let rest = &s[i + 1..];
            let entity = rest.find(';').is_some_and(|p| {
                p > 0
                    && p < 8
                    && rest[..p]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '#' || c == '{' || c == '}')
            });
            if !entity && rest.starts_with(|c: char| c.is_alphanumeric() || c == '{') {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    static TABLE: Catalog = Catalog::new("# test\n&Save\t\t&Speichern\t&Guardar\t&Enregistrer\t&Salva\t保存(&S)\t&Salvar\t&Guardar\t保存(&S)\nCopy\t\tKopieren\tCopiar\tCopier\tCopia\tコピー\tCopiar\t\t复制\nA {}:\\n\\n{}\t\tB {}:\\n\\n{}\tC {}:\\n\\n{}\tD {}:\\n\\n{}\tE {}:\\n\\n{}\tF {}:\\n\\n{}\tG {}:\\n\\n{}\t\tH {}:\\n\\n{}\nSee \\n\t\tSiehe \\n\tVer \\n\tVoir \\n\tVedi \\n\t見る \\n\tVer \\n\t\t看 \\n\n", '\t');

    #[test]
    fn common_table_is_well_formed() {
        let problems = check_table(COMMON.text(), '\t');
        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn lookups_and_fallbacks() {
        assert_eq!(TABLE.row("&Save").map(|r| r[Lang::DeDE.column()]), Some("&Speichern"));
        assert!(check_table(TABLE.text(), '\t').is_empty(), "{:?}", check_table(TABLE.text(), '\t'));
        // Multi-line strings are found by their real text; a literal "\n" stays.
        let m = TABLE.row("A {}:\n\n{}").unwrap();
        assert_eq!(m[Lang::DeDE.column()], "B {}:\n\n{}");
        assert_eq!(TABLE.row("See \\n").unwrap()[Lang::FrFR.column()], "Voir \\n");
    }

    #[test]
    fn templates_fill_in_order() {
        assert_eq!(fill("{} of {}", &["2", "9"]), "2 of 9");
        assert_eq!(fill("{} and {}", &["a"]), "a and ");
        set_lang(Lang::EnUS);
        assert_eq!(tf("Saved {}", &["a.txt"]), "Saved a.txt");
    }

    #[test]
    fn common_strings_are_translated() {
        set_lang(Lang::DeDE);
        record_misses(true);
        assert_ne!(tr("Cancel"), "Cancel");
        assert_eq!(tr("zzz unknown"), "zzz unknown");
        assert_eq!(take_misses().into_iter().collect::<Vec<_>>(), vec!["zzz unknown".to_string()]);
        record_misses(false);
        set_lang(Lang::EnUS);
    }

    #[test]
    fn mnemonics_are_stripped() {
        assert_eq!(strip_mnemonic("&Open..."), "Open...");
        assert_eq!(strip_mnemonic("開く(&O)..."), "開く...");
        assert_eq!(strip_mnemonic("Fish && Chips"), "Fish & Chips");
        assert_eq!(strip_mnemonic("Save A&ll"), "Save All");
        assert_eq!(access_keys("&Open"), 1);
        assert_eq!(access_keys("A &amp; B"), 0);
        assert_eq!(access_keys("Fish && Chips"), 0);
    }

    #[test]
    fn language_matching() {
        assert_eq!(Lang::best_match("de-AT"), Lang::DeDE);
        assert_eq!(Lang::best_match("pt-PT"), Lang::PtPT);
        assert_eq!(Lang::best_match("pt-BR"), Lang::PtBR);
        assert_eq!(Lang::best_match("pt-AO"), Lang::PtBR);
        assert_eq!(Lang::best_match("pt"), Lang::PtBR);
        assert_eq!(Lang::best_match("en-GB"), Lang::EnGB);
        assert_eq!(Lang::best_match("en-AU"), Lang::EnGB);
        assert_eq!(Lang::best_match("en-US"), Lang::EnUS);
        assert_eq!(Lang::best_match("zh-Hans-CN"), Lang::ZhCN);
        assert_eq!(Lang::best_match("ja"), Lang::JaJP);
        assert_eq!(Lang::best_match("ko-KR"), Lang::EnUS);
        assert_eq!(Lang::best_match(""), Lang::EnUS);
        for l in Lang::ALL {
            assert_eq!(Lang::from_tag(l.tag()), Some(l));
        }
    }
}
