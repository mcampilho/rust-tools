//! Localization: UI strings (see `rt-core`), number separators and date
//! formats for ten locales. Strings are looked up by their US English text,
//! so code keeps readable literals and untranslated text simply falls back to
//! English. rcalc's own strings are in `strings.txt`, `|`-separated.
//!
//! Internally every number is formatted the en-US way (`1,234.5`); the UI
//! converts separators for display with [`num`] and back for paste with
//! [`delocalize`].

use rt_core::i18n::{self as core, Catalog};

pub use core::{lang, Lang};

pub mod dates;

static TABLE: Catalog = Catalog::new(include_str!("strings.txt"), '|');

fn install() {
    core::install(&TABLE);
}

pub fn set_lang(l: Lang) {
    install();
    core::set_lang(l);
}

/// Translation in a specific language, with fallbacks
/// (en-GB → en-US, pt-PT → pt-BR, anything → en-US).
pub fn tr_in(l: Lang, s: &str) -> &str {
    install();
    core::tr_in(l, s)
}

/// Translates `s` into the current language (unknown strings pass through).
pub fn tr(s: &str) -> &str {
    install();
    core::tr(s)
}

/// Translates a template and fills `{}` placeholders in order.
pub fn tf(template: &str, args: &[&str]) -> String {
    install();
    core::tf(template, args)
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

/// The display language of the system, mapped to a supported locale.
pub fn system_lang() -> Lang {
    rt_sys::system_lang()
}

#[cfg(test)]
pub(crate) mod tests;
