//! User preferences in `settings.ini`. Missing or invalid values fall back
//! to defaults.

use crate::i18n::Lang;
use crate::storage::Store;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 0 follows Windows, 1 light, 2 dark.
    pub theme: u8,
    /// Interface language; None follows Windows.
    pub language: Option<Lang>,
    /// Word wrap for text files.
    pub wrap: bool,
    /// Show HTML and PDF pages dark in the dark theme.
    pub dark_pages: bool,
    /// Last folder used in the Open dialog.
    pub last_dir: String,
    /// Window size (logical pixels) and maximized state.
    pub window: Option<(f64, f64, bool)>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { theme: 0, language: None, wrap: true, dark_pages: true, last_dir: String::new(), window: None }
    }
}

impl Settings {
    pub fn load(s: &Store) -> Settings {
        let d = Settings::default();
        Settings {
            theme: match s.get("theme") {
                Some("light") => 1,
                Some("dark") => 2,
                _ => 0,
            },
            language: s.get("language").and_then(Lang::from_tag),
            wrap: s.get("wrap").map_or(d.wrap, |v| v != "0"),
            dark_pages: s.get("dark_pages").map_or(d.dark_pages, |v| v != "0"),
            last_dir: s.get("last_dir").unwrap_or("").to_string(),
            window: s.get("window").and_then(|v| {
                let mut it = v.split(',');
                let w: f64 = it.next()?.trim().parse().ok()?;
                let h: f64 = it.next()?.trim().parse().ok()?;
                let max = it.next() == Some("max");
                (w >= 200.0 && h >= 150.0 && w < 20000.0 && h < 20000.0).then_some((w, h, max))
            }),
        }
    }

    pub fn to_store(&self) -> Store {
        let mut s = Store::default();
        s.set("theme", ["system", "light", "dark"][self.theme.min(2) as usize]);
        s.set("language", self.language.map_or("", |l| l.tag()));
        s.set("wrap", if self.wrap { "1" } else { "0" });
        s.set("dark_pages", if self.dark_pages { "1" } else { "0" });
        s.set("last_dir", self.last_dir.clone());
        if let Some((w, h, max)) = self.window {
            s.set("window", format!("{w:.0},{h:.0},{}", if max { "max" } else { "normal" }));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_bad_values() {
        let s = Settings { theme: 2, language: Some(Lang::JaJP), wrap: false, dark_pages: false, last_dir: "C:\\docs".into(), window: Some((900.0, 700.0, true)) };
        assert_eq!(Settings::load(&Store::parse(&s.to_store().serialize("rview"))), s);
        let bad = Settings::load(&Store::parse("theme=??\nwindow=abc\nlanguage=xx-YY\n"));
        assert_eq!(bad, Settings::default());
    }
}
