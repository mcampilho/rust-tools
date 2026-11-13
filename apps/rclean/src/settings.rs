//! User preferences in `settings.ini`. Missing or invalid values fall back
//! to defaults.

use crate::i18n::Lang;
use crate::storage::Store;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Settings {
    /// 0 follows the system, 1 light, 2 dark.
    pub theme: u8,
    /// Interface language; None follows the system.
    pub language: Option<Lang>,
    /// Window size (logical pixels) and maximized state.
    pub window: Option<(f64, f64, bool)>,
    /// The boxes ticked or not unlike their default: (target id, ticked).
    pub ticks: Vec<(String, bool)>,
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
            // `ticks=chrome.cache:0,win.thumbs:1`
            ticks: s
                .get("ticks")
                .unwrap_or("")
                .split(',')
                .filter_map(|t| {
                    let (id, on) = t.trim().rsplit_once(':')?;
                    let on = match on {
                        "1" => true,
                        "0" => false,
                        _ => return None,
                    };
                    (!id.is_empty()).then(|| (id.to_string(), on))
                })
                .collect(),
        }
    }

    pub fn to_store(&self) -> Store {
        let mut s = Store::default();
        s.set("theme", ["system", "light", "dark"][self.theme.min(2) as usize]);
        s.set("language", self.language.map_or("", |l| l.tag()));
        if let Some((w, h, max)) = self.window {
            s.set("window", format!("{w:.0},{h:.0},{}", if max { "max" } else { "normal" }));
        }
        s.set("ticks", self.ticks.iter().map(|(id, on)| format!("{id}:{}", u8::from(*on))).collect::<Vec<_>>().join(","));
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_bad_values() {
        let s = Settings { theme: 2, language: Some(Lang::PtPT), window: Some((900.0, 700.0, true)), ticks: vec![("chrome.cache".into(), false), ("win.thumbs".into(), true)] };
        assert_eq!(Settings::load(&Store::parse(&s.to_store().serialize("rclean"))), s);
        let bad = Settings::load(&Store::parse("theme=??\nwindow=abc\nlanguage=xx-YY\nticks=a:2,:1,b\n"));
        assert_eq!(bad, Settings::default());
    }
}
