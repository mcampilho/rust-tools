//! Program texts: `strings.txt`, one row per key with the English, European
//! Portuguese and Spanish texts. The table machinery is `rt_cli::Texts`.

pub use rt_cli::{Texts as T, LANGUAGES};

const TABLE: &str = include_str!("strings.txt");

/// The texts in `lang`.
pub fn texts(lang: &str) -> T {
    T::new(TABLE, lang)
}

/// The default language: RSCOOP_LANG, then the display language of the system.
pub fn system_language() -> String {
    rt_cli::language("RSCOOP_LANG")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_well_formed() {
        assert_eq!(rt_cli::table_problems(TABLE), Vec::<String>::new());
    }

    #[test]
    fn fallback_and_fill() {
        assert_eq!(texts("xx").get("report.title"), "Scoop check");
        assert_eq!(texts("pt").get("missing"), "Aponta para um caminho que não existe.");
        assert_eq!(texts("pt").get("no.such.key"), "no.such.key");
        assert_eq!(texts("en").fill("report.checked", &[("n", "3")]), "3 checked");
    }
}
