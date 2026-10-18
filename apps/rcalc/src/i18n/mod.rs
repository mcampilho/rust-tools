//! Localization: UI strings, number separators and date formats for ten
//! locales. Strings are looked up by their US English text, so code keeps
//! readable literals and untranslated text simply falls back to English.
//!
//! Internally every number is formatted the en-US way (`1,234.5`); the UI
//! converts separators for display with [`num`] and back for paste with
//! [`delocalize`].

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::OnceLock;

pub mod dates;

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

    /// Name of the language in itself (shown in the language picker).
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

    /// Column in the translation table.
    fn column(self) -> usize {
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

thread_local! {
    // Per thread: the UI runs on one thread, and tests can use different
    // languages in parallel without interfering.
    static CURRENT: Cell<u8> = const { Cell::new(0) };
}

pub fn set_lang(l: Lang) {
    CURRENT.with(|c| c.set(l as u8));
}

pub fn lang() -> Lang {
    match CURRENT.with(Cell::get) {
        1 => Lang::EnGB,
        2 => Lang::DeDE,
        3 => Lang::EsES,
        4 => Lang::FrFR,
        5 => Lang::ItIT,
        6 => Lang::JaJP,
        7 => Lang::PtBR,
        8 => Lang::PtPT,
        9 => Lang::ZhCN,
        _ => Lang::EnUS,
    }
}

const TABLE: &str = include_str!("strings.txt");

/// Column order in `strings.txt` after the en-US key.
const FILE_COLUMNS: [Lang; 9] = [
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

type Row = [&'static str; 10];

fn table() -> &'static HashMap<&'static str, Row> {
    static T: OnceLock<HashMap<&'static str, Row>> = OnceLock::new();
    T.get_or_init(|| {
        let mut map = HashMap::new();
        for line in TABLE.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cells = line.split('|');
            let Some(key) = cells.next() else { continue };
            let mut row: Row = [""; 10];
            row[Lang::EnUS.column()] = key;
            for (lang, cell) in FILE_COLUMNS.iter().zip(cells) {
                row[lang.column()] = cell;
            }
            map.insert(key, row);
        }
        map
    })
}

/// Translation in a specific language, with fallbacks
/// (en-GB → en-US, pt-PT → pt-BR, anything → en-US).
pub fn tr_in(l: Lang, s: &str) -> &str {
    let Some(row) = table().get(s) else {
        #[cfg(test)]
        tests::record_miss(s);
        return s;
    };
    let pick = |l: Lang| Some(row[l.column()]).filter(|v| !v.is_empty());
    pick(l)
        .or_else(|| if l == Lang::PtPT { pick(Lang::PtBR) } else { None })
        .unwrap_or(s)
}

/// Translates `s` into the current language (unknown strings pass through).
pub fn tr(s: &str) -> &str {
    tr_in(lang(), s)
}

/// Translates a template and fills `{}` placeholders in order.
pub fn tf(template: &str, args: &[&str]) -> String {
    let t = tr(template);
    let mut out = String::with_capacity(t.len() + 16);
    let mut rest = t;
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

/// Converts an en-US formatted number (or expression) to the current
/// locale's separators: `1,234.5` → `1.234,5` (de-DE).
pub fn num(s: &str) -> String {
    num_in(lang(), s)
}

pub fn num_in(l: Lang, s: &str) -> String {
    let (dec, group) = l.separators();
    if dec == '.' && group == ',' {
        return s.to_string();
    }
    s.chars()
        .map(|c| match c {
            '.' => dec,
            ',' => group,
            c => c,
        })
        .collect()
}

/// Decimal separator of the current language as a string (keypad label).
pub fn decimal_point() -> &'static str {
    if lang().separators().0 == ',' {
        ","
    } else {
        "."
    }
}

/// Converts pasted text in the current locale's notation to the internal
/// en-US notation understood by the parser.
pub fn delocalize(s: &str) -> String {
    let (dec, group) = lang().separators();
    if dec == '.' {
        return s.to_string();
    }
    s.chars()
        .filter(|c| *c != group && *c != '\u{202F}' && *c != '\u{00A0}' && !(group == '.' && *c == '.'))
        .map(|c| if c == dec { '.' } else { c })
        .collect()
}

/// The Windows display language, mapped to a supported locale.
pub fn system_lang() -> Lang {
    Lang::best_match(&crate::platform::ui_language().unwrap_or_default())
}

#[cfg(test)]
pub(crate) mod tests;
