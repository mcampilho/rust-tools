//! Localization of the user interface into ten locales (see `rt-core`).
//! rview's own strings are in `strings.txt`: one row per string, keyed by
//! the US English text, tab-separated.

use rt_core::i18n::{self as core, Catalog};

pub use core::{lang, Lang};

static TABLE: Catalog = Catalog::new(include_str!("strings.txt"), '\t');

fn install() {
    core::install(&TABLE);
}

pub fn set_lang(l: Lang) {
    install();
    core::set_lang(l);
}

/// Translation in a specific language, with fallbacks.
#[cfg(test)]
pub fn tr_in(l: Lang, s: &str) -> String {
    install();
    core::tr_in(l, s).to_string()
}

/// Translates `s` into the current language (unknown strings pass through).
pub fn tr(s: &str) -> String {
    install();
    core::tr(s).to_string()
}

/// Translates a template and fills `{}` placeholders in order.
pub fn tf(template: &str, args: &[&str]) -> String {
    install();
    core::tf(template, args)
}

/// The system display language, mapped to a supported locale.
pub fn system_lang() -> Lang {
    rt_sys::system_lang()
}

#[cfg(test)]
mod tests;
