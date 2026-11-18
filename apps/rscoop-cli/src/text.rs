//! Program texts: `strings.txt`, one row per key with the English, European
//! Portuguese and Spanish texts. English fills any missing text.

use std::collections::HashMap;
use std::sync::OnceLock;

pub const LANGUAGES: [&str; 3] = ["en", "pt", "es"];

const TABLE: &str = include_str!("strings.txt");

fn table() -> &'static HashMap<&'static str, [&'static str; 3]> {
    static T: OnceLock<HashMap<&'static str, [&'static str; 3]>> = OnceLock::new();
    T.get_or_init(|| {
        let mut m = HashMap::new();
        for line in TABLE.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            let en = cols.get(1).copied().unwrap_or("");
            let get = |i: usize| cols.get(i).copied().filter(|s| !s.is_empty()).unwrap_or(en);
            m.insert(cols[0], [en, get(2), get(3)]);
        }
        m
    })
}

/// Translator for one language.
#[derive(Clone, Copy)]
pub struct T {
    col: usize,
}

impl T {
    pub fn new(lang: &str) -> T {
        T { col: LANGUAGES.iter().position(|l| *l == lang).unwrap_or(0) }
    }

    /// The text of `key` (the key itself when unknown).
    pub fn get<'a>(&self, key: &'a str) -> &'a str {
        table().get(key).map(|r| r[self.col]).unwrap_or(key)
    }

    /// The text of `key` with `{name}` placeholders filled.
    pub fn fill(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut s = self.get(key).to_string();
        for (k, v) in args {
            s = s.replace(&format!("{{{k}}}"), v);
        }
        s
    }
}

/// The default language: RSCOOP_LANG, then the Windows display language.
pub fn system_language() -> String {
    let env = std::env::var("RSCOOP_LANG").unwrap_or_default();
    let env: String = env.chars().take(2).collect::<String>().to_lowercase();
    if LANGUAGES.contains(&env.as_str()) {
        return env;
    }
    let l = crate::sys::ui_language();
    if LANGUAGES.contains(&l.as_str()) {
        l
    } else {
        "en".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_has_three_languages() {
        for line in TABLE.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let cols: Vec<&str> = line.split('\t').collect();
            assert_eq!(cols.len(), 4, "{line}");
            assert!(cols.iter().all(|c| !c.trim().is_empty()), "{line}");
        }
    }

    #[test]
    fn fallback_and_fill() {
        assert_eq!(T::new("xx").get("report.title"), "Scoop check");
        assert_eq!(T::new("pt").get("missing"), "Aponta para um caminho que não existe.");
        assert_eq!(T::new("en").fill("report.checked", &[("n", "3")]), "3 checked");
    }
}
