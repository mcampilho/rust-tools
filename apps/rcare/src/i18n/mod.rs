//! Localization of the user interface into ten locales (see `rt-core`).
//! rcare's own strings are in `strings.txt`: one row per string, keyed by
//! the US English text, tab-separated. What a page shows is in the table of
//! the page's own app (rclean's, for the cleaner), added by [`install`].

use rt_core::i18n::{self as core, Catalog};

pub use core::{lang, Lang};

static TABLE: Catalog = Catalog::new(include_str!("strings.txt"), '\t');

/// Installs rcare's table, then the tables of the pages. Safe to call again.
pub fn install() {
    core::install(&TABLE);
    rclean::i18n::add_table();
}

pub fn set_lang(l: Lang) {
    install();
    core::set_lang(l);
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
