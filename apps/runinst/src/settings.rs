//! User preferences in `settings.ini`. Missing or invalid values fall back
//! to defaults.

use crate::app::Whose;
use crate::i18n::Lang;
use crate::storage::Store;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 0 follows the system, 1 light, 2 dark.
    pub theme: u8,
    /// Interface language; None follows the system.
    pub language: Option<Lang>,
    /// Window size (logical pixels) and maximized state.
    pub window: Option<(f64, f64, bool)>,
    pub whose: Whose,
    pub show_system: bool,
    pub show_updates: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { theme: 0, language: None, window: None, whose: Whose::All, show_system: false, show_updates: false }
    }
}

impl Settings {
    pub fn load(s: &Store) -> Settings {
        Settings {
            theme: match s.get("theme") {
                Some("light") => 1,
                Some("dark") => 2,
                _ => 0,
            },
            language: s.get("language").and_then(Lang::from_tag),
            window: s.get("window").and_then(|v| {
                let mut it = v.split(',');
                let w: f64 = it.next()?.trim().parse().ok()?;
                let h: f64 = it.next()?.trim().parse().ok()?;
                let max = it.next() == Some("max");
                (w >= 200.0 && h >= 150.0 && w < 20000.0 && h < 20000.0).then_some((w, h, max))
            }),
            whose: s.get("whose").and_then(|v| v.parse::<usize>().ok()).and_then(|i| Whose::ALL.get(i).copied()).unwrap_or_default(),
            show_system: s.get("show_system") == Some("yes"),
            show_updates: s.get("show_updates") == Some("yes"),
        }
    }

    pub fn to_store(&self) -> Store {
        let mut s = Store::default();
        s.set("theme", ["system", "light", "dark"][self.theme.min(2) as usize]);
        s.set("language", self.language.map_or("", |l| l.tag()));
        if let Some((w, h, max)) = self.window {
            s.set("window", format!("{w:.0},{h:.0},{}", if max { "max" } else { "normal" }));
        }
        s.set("whose", Whose::ALL.iter().position(|w| *w == self.whose).unwrap_or(0).to_string());
        s.set("show_system", if self.show_system { "yes" } else { "no" });
        s.set("show_updates", if self.show_updates { "yes" } else { "no" });
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_bad_values() {
        let s = Settings { theme: 2, language: Some(Lang::PtPT), window: Some((900.0, 700.0, true)), whose: Whose::Others, show_system: true, show_updates: false };
        assert_eq!(Settings::load(&Store::parse(&s.to_store().serialize("runinst"))), s);
        let bad = Settings::load(&Store::parse("theme=??\nwindow=abc\nlanguage=xx-YY\nwhose=9\n"));
        assert_eq!(bad, Settings::default());
    }
}
