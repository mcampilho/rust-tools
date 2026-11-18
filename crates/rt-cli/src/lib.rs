//! What the command-line apps (`*-cli`) share.
//!
//! - [`Texts`]: the program texts of an app, a table with a row per key and
//!   the English, European Portuguese and Spanish text (English fills a text
//!   that is missing). The table is the app's `strings.txt`.
//! - [`language`]: which of the three to use.
//! - [`human`]: sizes as `du -h` shows them.

use std::collections::HashMap;

/// The languages of the tables, in column order after the key.
pub const LANGUAGES: [&str; 3] = ["en", "pt", "es"];

/// The texts of one app in one language.
pub struct Texts {
    rows: HashMap<&'static str, [&'static str; 3]>,
    col: usize,
}

impl Texts {
    /// `table` is the contents of `strings.txt`: lines of `key`, English,
    /// Portuguese and Spanish separated by tabs; `#` starts a comment.
    pub fn new(table: &'static str, lang: &str) -> Texts {
        let mut rows = HashMap::new();
        for line in table.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let cols: Vec<&str> = line.split('\t').collect();
            let en = cols.get(1).copied().unwrap_or("");
            let get = |i: usize| cols.get(i).copied().filter(|s| !s.is_empty()).unwrap_or(en);
            rows.insert(cols[0], [en, get(2), get(3)]);
        }
        Texts { rows, col: LANGUAGES.iter().position(|l| *l == lang).unwrap_or(0) }
    }

    /// The text of `key` (the key itself when it is not in the table).
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.rows.get(key).map_or(key, |r| r[self.col])
    }

    /// The text of `key` with each `{name}` replaced.
    pub fn fill(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut s = self.get(key).to_string();
        for (name, value) in args {
            s = s.replace(&format!("{{{name}}}"), value);
        }
        s
    }
}

/// What is wrong with a table, one line each (for the app's tests): a row
/// that does not have its four columns, an empty text, a duplicate key.
pub fn table_problems(table: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in table.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 1 + LANGUAGES.len() {
            problems.push(format!("{} columns instead of 4: {line}", cols.len()));
        } else if cols.iter().any(|c| c.trim().is_empty()) {
            problems.push(format!("an empty text: {line}"));
        }
        if !seen.insert(cols[0]) {
            problems.push(format!("the key {} is twice", cols[0]));
        }
    }
    problems
}

/// The language to use: the first two letters of the environment variable
/// `env_var` when it names one of [`LANGUAGES`], else the language of the
/// system, else English.
pub fn language(env_var: &str) -> String {
    let wanted = std::env::var(env_var).unwrap_or_default();
    if let Some(l) = pick(&wanted) {
        return l;
    }
    pick(rt_sys::system_lang().tag()).unwrap_or_else(|| "en".into())
}

/// The language of [`LANGUAGES`] a tag such as `pt-PT`, `es_ES.UTF-8` or `PT` starts with.
pub fn pick(tag: &str) -> Option<String> {
    let l: String = tag.chars().take(2).collect::<String>().to_lowercase();
    LANGUAGES.contains(&l.as_str()).then_some(l)
}

/// A size as `du -h` writes it: bytes as they are, then `1.5K`, `23M`, `4.0G`
/// (one decimal under 10, rounded up). With `si` the steps are 1000 and the
/// first unit is `k`.
pub fn human(bytes: u64, si: bool) -> String {
    let base: u64 = if si { 1000 } else { 1024 };
    if bytes < base {
        return bytes.to_string();
    }
    let units: [&str; 6] = if si { ["k", "M", "G", "T", "P", "E"] } else { ["K", "M", "G", "T", "P", "E"] };
    let mut unit = 0;
    let mut scaled = bytes as f64 / base as f64;
    loop {
        // Tenths, rounded up: 1.01K is 1.1K, as in du.
        let tenths = (scaled * 10.0).ceil() as u64;
        if tenths < 100 {
            return format!("{}.{}{}", tenths / 10, tenths % 10, units[unit]);
        }
        let whole = scaled.ceil() as u64;
        if whole < base || unit == units.len() - 1 {
            return format!("{whole}{}", units[unit]);
        }
        scaled /= base as f64;
        unit += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "# comment\nhello\tHello {name}\tOlá {name}\tHola {name}\nonly\tEnglish\t\t\n";

    #[test]
    fn texts_fall_back_to_english_and_fill() {
        let pt = Texts::new(TABLE, "pt");
        assert_eq!(pt.fill("hello", &[("name", "Rui")]), "Olá Rui");
        assert_eq!(pt.get("only"), "English");
        assert_eq!(pt.get("missing"), "missing");
        assert_eq!(Texts::new(TABLE, "es").get("hello"), "Hola {name}");
        assert_eq!(Texts::new(TABLE, "xx").get("hello"), "Hello {name}");
    }

    #[test]
    fn a_table_is_checked() {
        assert!(table_problems("a\tb\tc\td\n").is_empty());
        assert_eq!(table_problems("a\tb\tc\n").len(), 1);
        assert_eq!(table_problems("a\tb\t\td\n").len(), 1);
        assert_eq!(table_problems("a\tb\tc\td\na\tb\tc\td\n").len(), 1);
    }

    #[test]
    fn languages_are_picked_from_tags() {
        assert_eq!(pick("pt-PT").as_deref(), Some("pt"));
        assert_eq!(pick("ES_es.UTF-8").as_deref(), Some("es"));
        assert_eq!(pick("en").as_deref(), Some("en"));
        assert_eq!(pick("de-DE"), None);
        assert_eq!(pick(""), None);
    }

    #[test]
    fn sizes_as_du_writes_them() {
        assert_eq!(human(0, false), "0");
        assert_eq!(human(1023, false), "1023");
        assert_eq!(human(1024, false), "1.0K");
        assert_eq!(human(1025, false), "1.1K");
        assert_eq!(human(4096, false), "4.0K");
        assert_eq!(human(10 * 1024, false), "10K");
        assert_eq!(human(10 * 1024 + 1, false), "11K");
        assert_eq!(human(1024 * 1024 - 1, false), "1.0M");
        assert_eq!(human(1_500_000, false), "1.5M");
        assert_eq!(human(5 * 1024 * 1024 * 1024, false), "5.0G");
        assert_eq!(human(999, true), "999");
        assert_eq!(human(1000, true), "1.0k");
        assert_eq!(human(1_234_567, true), "1.3M");
        assert_eq!(human(u64::MAX, false), "16E");
    }
}
