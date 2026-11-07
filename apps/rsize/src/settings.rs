//! User preferences in `settings.ini`. Missing or invalid values fall back
//! to defaults.

use crate::i18n::Lang;
use crate::model::SizeMode;
use crate::storage::Store;

/// The chart below the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ChartType {
    #[default]
    Treemap,
    Pie,
    Rings,
}

impl ChartType {
    pub const ALL: [ChartType; 3] = [ChartType::Treemap, ChartType::Pie, ChartType::Rings];

    fn tag(self) -> &'static str {
        match self {
            ChartType::Treemap => "treemap",
            ChartType::Pie => "pie",
            ChartType::Rings => "rings",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub chart: ChartType,
    /// 0 follows the system, 1 light, 2 dark.
    pub theme: u8,
    /// Interface language; None follows the system.
    pub language: Option<Lang>,
    /// Which size the views show.
    pub size_mode: SizeMode,
    /// Share of the height given to the folder list (the treemap gets the rest).
    pub split: f32,
    /// Last folder chosen in the folder dialog.
    pub last_dir: String,
    /// Window size (logical pixels) and maximized state.
    pub window: Option<(f64, f64, bool)>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { chart: ChartType::Treemap, theme: 0, language: None, size_mode: SizeMode::Logical, split: 0.45, last_dir: String::new(), window: None }
    }
}

pub const SPLIT_MIN: f32 = 0.15;
pub const SPLIT_MAX: f32 = 0.85;

impl Settings {
    pub fn load(s: &Store) -> Settings {
        let d = Settings::default();
        Settings {
            chart: ChartType::ALL.into_iter().find(|c| s.get("chart") == Some(c.tag())).unwrap_or_default(),
            theme: match s.get("theme") {
                Some("light") => 1,
                Some("dark") => 2,
                _ => 0,
            },
            language: s.get("language").and_then(Lang::from_tag),
            size_mode: match s.get("size") {
                Some("disk") => SizeMode::Allocated,
                _ => SizeMode::Logical,
            },
            split: s
                .get("split")
                .and_then(|v| v.trim().parse::<f32>().ok())
                .filter(|v| (SPLIT_MIN..=SPLIT_MAX).contains(v))
                .unwrap_or(d.split),
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
        s.set("chart", self.chart.tag());
        s.set("theme", ["system", "light", "dark"][self.theme.min(2) as usize]);
        s.set("language", self.language.map_or("", |l| l.tag()));
        s.set(
            "size",
            match self.size_mode {
                SizeMode::Logical => "length",
                SizeMode::Allocated => "disk",
            },
        );
        s.set("split", format!("{:.3}", self.split));
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
        let s = Settings {
            chart: ChartType::Rings,
            theme: 2,
            language: Some(Lang::PtPT),
            size_mode: SizeMode::Allocated,
            split: 0.6,
            last_dir: "C:\\data".into(),
            window: Some((900.0, 700.0, true)),
        };
        assert_eq!(Settings::load(&Store::parse(&s.to_store().serialize("rsize"))), s);
        let bad = Settings::load(&Store::parse("theme=??\nwindow=abc\nlanguage=xx-YY\nsplit=7\nsize=huge\nchart=bars\n"));
        assert_eq!(bad, Settings::default());
    }
}
