//! Localization of the user interface into ten locales. Strings are looked
//! up by their US English text, so the code keeps readable literals and
//! anything untranslated falls back to English.
//!
//! Menu labels carry their own `&` mnemonic in each language; Japanese and
//! Chinese use the Windows convention of a Latin mnemonic in parentheses,
//! e.g. `新規(&N)`. [`strip_mnemonic`] removes either form.

use std::cell::Cell;
use std::collections::HashMap;
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
    pub const ALL: [Lang; 10] = [Lang::DeDE, Lang::EnGB, Lang::EnUS, Lang::EsES, Lang::FrFR, Lang::ItIT, Lang::JaJP, Lang::PtBR, Lang::PtPT, Lang::ZhCN];

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

    fn column(self) -> usize {
        self as usize
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
            "pt" if lower.ends_with("-pt") => Lang::PtPT,
            "pt" => Lang::PtBR,
            "en" if ["-gb", "-ie", "-au", "-nz", "-in", "-za"].iter().any(|r| lower.ends_with(r)) => Lang::EnGB,
            _ => Lang::EnUS,
        }
    }
}

thread_local! {
    // Per thread: the UI runs on one thread, and tests can use different
    // languages in parallel without interfering.
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::EnUS) };
}

pub fn set_lang(l: Lang) {
    CURRENT.with(|c| c.set(l));
}

pub fn lang() -> Lang {
    CURRENT.with(Cell::get)
}

const TABLE: &str = include_str!("strings.txt");

/// Column order in `strings.txt` after the en-US key.
const FILE_COLUMNS: [Lang; 9] = [Lang::EnGB, Lang::DeDE, Lang::EsES, Lang::FrFR, Lang::ItIT, Lang::JaJP, Lang::PtBR, Lang::PtPT, Lang::ZhCN];

type Row = [&'static str; 10];

fn table() -> &'static HashMap<&'static str, Row> {
    static T: OnceLock<HashMap<&'static str, Row>> = OnceLock::new();
    T.get_or_init(|| {
        let mut map = HashMap::new();
        for line in TABLE.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cells = line.split('\t');
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

/// Undoes the table's escapes: `\n` for a line break.
fn unescape(s: &'static str) -> std::borrow::Cow<'static, str> {
    if s.contains("\\n") {
        std::borrow::Cow::Owned(s.replace("\\n", "\n"))
    } else {
        std::borrow::Cow::Borrowed(s)
    }
}

/// Translation in a specific language, with fallbacks
/// (en-GB → en-US, pt-PT → pt-BR, anything → en-US).
pub fn tr_in(l: Lang, s: &str) -> String {
    let key = s.replace('\n', "\\n");
    let Some(row) = table().get(key.as_str()) else {
        #[cfg(test)]
        tests::record_miss(s);
        return s.to_string();
    };
    let pick = |l: Lang| Some(row[l.column()]).filter(|v| !v.is_empty());
    match pick(l).or_else(|| if l == Lang::PtPT { pick(Lang::PtBR) } else { None }) {
        // `\n` in the table is a line break only for multi-line strings, so
        // text that shows a literal `\n` (regex hints) stays as it is.
        Some(v) if s.contains('\n') => unescape(v).into_owned(),
        Some(v) => v.to_string(),
        None => s.to_string(),
    }
}

/// Translates `s` into the current language (unknown strings pass through).
pub fn tr(s: &str) -> String {
    if lang() == Lang::EnUS {
        return s.to_string();
    }
    tr_in(lang(), s)
}

/// Translates a template and fills `{}` placeholders in order.
pub fn tf(template: &str, args: &[&str]) -> String {
    let t = tr(template);
    let mut out = String::with_capacity(t.len() + 16);
    let mut rest = t.as_str();
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

/// The Windows display language, mapped to a supported locale.
pub fn system_lang() -> Lang {
    #[cfg(windows)]
    let tag = crate::win::ui_language().unwrap_or_default();
    #[cfg(not(windows))]
    let tag = std::env::var("LANG").unwrap_or_default().split('.').next().unwrap_or("").replace('_', "-");
    Lang::best_match(&tag)
}

#[cfg(test)]
mod tests;
