use super::*;
use std::collections::BTreeSet;

#[test]
fn table_rows_are_well_formed() {
    let problems = core::check_table(TABLE.text(), '|');
    assert!(problems.is_empty(), "{problems:#?}");
    let rows = TABLE.text().lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).count();
    assert!(rows > 300);
}

#[test]
fn lookups_and_fallbacks() {
    assert_eq!(tr_in(Lang::DeDE, "Scientific"), "Wissenschaftlich");
    assert_eq!(tr_in(Lang::JaJP, "Scientific"), "関数電卓");
    assert_eq!(tr_in(Lang::EnGB, "Meters"), "Metres");
    // en-GB falls back to en-US, pt-PT to pt-BR.
    assert_eq!(tr_in(Lang::EnGB, "Scientific"), "Scientific");
    assert_eq!(tr_in(Lang::PtPT, "History"), "Histórico");
    assert_eq!(tr_in(Lang::PtPT, "Settings"), "Definições");
    assert_eq!(tr_in(Lang::PtBR, "Settings"), "Configurações");
    // Unknown strings pass through.
    assert_eq!(tr_in(Lang::FrFR, "1,234.5"), "1,234.5");
    set_lang(Lang::EsES);
    assert_eq!(tf("Display is {}", &["5"]), "La pantalla muestra 5");
    set_lang(Lang::EnUS);
}

#[test]
fn number_separators() {
    assert_eq!(num_in(Lang::DeDE, "1,234,567.89"), "1.234.567,89");
    assert_eq!(num_in(Lang::FrFR, "1,234.5"), "1\u{202F}234,5");
    assert_eq!(num_in(Lang::PtPT, "1,234.5"), "1\u{00A0}234,5");
    assert_eq!(num_in(Lang::JaJP, "1,234.5"), "1,234.5");
    assert_eq!(num_in(Lang::ItIT, "2 + 3.5 ="), "2 + 3,5 =");
    set_lang(Lang::DeDE);
    assert_eq!(delocalize("1.234,5"), "1234.5");
    assert_eq!(delocalize("2,5*4"), "2.5*4");
    assert_eq!(decimal_point(), ",");
    set_lang(Lang::FrFR);
    assert_eq!(delocalize("1\u{202F}234,5"), "1234.5");
    set_lang(Lang::EnUS);
    assert_eq!(delocalize("1,234.5"), "1,234.5");
    assert_eq!(decimal_point(), ".");
}

#[test]
fn language_matching() {
    assert_eq!(Lang::best_match("de-AT"), Lang::DeDE);
    assert_eq!(Lang::best_match("pt-PT"), Lang::PtPT);
    assert_eq!(Lang::best_match("pt-BR"), Lang::PtBR);
    assert_eq!(Lang::best_match("pt"), Lang::PtBR);
    assert_eq!(Lang::best_match("en-GB"), Lang::EnGB);
    assert_eq!(Lang::best_match("en-AU"), Lang::EnGB);
    assert_eq!(Lang::best_match("en-US"), Lang::EnUS);
    assert_eq!(Lang::best_match("zh-Hans-CN"), Lang::ZhCN);
    assert_eq!(Lang::best_match("ja"), Lang::JaJP);
    assert_eq!(Lang::best_match("nl-NL"), Lang::EnUS);
    assert_eq!(Lang::best_match(""), Lang::EnUS);
    for l in Lang::ALL {
        assert_eq!(Lang::from_tag(l.tag()), Some(l));
    }
}

/// Sentence-like literals shown to users but intentionally not translated.
const UNTRANSLATED: &[&str] = &[];

/// Every user-visible English literal in the UI sources has a translation.
#[test]
fn ui_strings_have_translations() {
    install();
    let root = env!("CARGO_MANIFEST_DIR");
    let files = [
        "src/ui/view.rs",
        "src/app.rs",
        "src/num/mod.rs",
        "src/calc/programmer.rs",
        "src/convert.rs",
        "src/main.rs",
    ];
    let mut missing = Vec::new();
    for f in files {
        let src = std::fs::read_to_string(format!("{root}/{f}")).unwrap();
        let code = src.split("#[cfg(test)]").next().unwrap();
        for lit in string_literals(code) {
            if looks_like_ui_text(&lit) && !UNTRANSLATED.contains(&lit.as_str()) && !core::has_key(&lit) {
                missing.push(format!("{f}: {lit}"));
            }
        }
    }
    assert!(missing.is_empty(), "untranslated UI strings:\n{}", missing.join("\n"));
}

fn string_literals(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = code.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            // skip char literals like '"'
            b'\'' if i + 2 < b.len() && b[i + 2] == b'\'' => i += 3,
            b'\'' if i + 3 < b.len() && b[i + 1] == b'\\' && b[i + 3] == b'\'' => i += 4,
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'"' => {
                let start = i + 1;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                out.push(code[start..i.min(b.len())].replace("\\\"", "\""));
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// Heuristic: text starting with a capitalized English word, excluding log
/// messages, command-line text, identifiers and unit abbreviations.
fn looks_like_ui_text(s: &str) -> bool {
    if s.starts_with("rcalc") || s.starts_with("--") || s.contains("{e}") || s.contains("{expr}") || s.contains('\n') {
        return false;
    }
    let first = s.split_whitespace().next().unwrap_or("");
    let mut chars = first.chars();
    let capitalized = chars.next().is_some_and(|c| c.is_ascii_uppercase()) && chars.any(|c| c.is_ascii_lowercase());
    let sentence = s.split_whitespace().count() >= 2 || s.chars().filter(|c| c.is_ascii_lowercase()).count() >= 3;
    capitalized && sentence && !first.contains(['_', ':', '/', '{'])
}

/// Strings looked up on this thread that had no translation entry.
pub(crate) fn take_misses() -> BTreeSet<String> {
    let misses = core::take_misses();
    core::record_misses(true);
    misses
}

fn words(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_alphabetic())
        .filter(|w| w.chars().count() >= 4)
        .map(str::to_lowercase)
}

/// True when `s` contains an English word from the source strings that never
/// appears in the translations for `l`, i.e. text that slipped through
/// untranslated (works on composed text such as "Display is 5").
pub(crate) fn has_english_leftover(l: Lang, s: &str) -> bool {
    let mut english = BTreeSet::new();
    let mut foreign = BTreeSet::new();
    install();
    for (key, row) in TABLE.rows() {
        english.extend(words(key));
        foreign.extend(words(row[l.column()]));
    }
    words(s).any(|w| english.contains(&w) && !foreign.contains(&w))
}
