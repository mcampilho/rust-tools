//! User preferences in `settings.ini`. Missing or invalid values fall back
//! to defaults.

use crate::i18n::Lang;
use crate::model::SizeMode;
use crate::storage::Store;

/// The chart of the result.
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

/// How the result is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ViewMode {
    /// The folder list at the left, the file types at the right, the treemap
    /// below (as WizTree).
    #[default]
    Folders,
    /// The chart at the left (treemap, pie or rings), the file types at the
    /// right, the folder list below: the charts get the larger area.
    Map,
}

impl ViewMode {
    pub const ALL: [ViewMode; 2] = [ViewMode::Folders, ViewMode::Map];

    fn tag(self) -> &'static str {
        match self {
            ViewMode::Folders => "folders",
            ViewMode::Map => "map",
        }
    }
}

/// Where the bars between the parts of a layout are: the share of the
/// height for the upper part and of the width for the left part.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panes {
    pub split: f32,
    pub split_x: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The chart in the map layout (the folders layout shows the treemap).
    pub chart: ChartType,
    pub view: ViewMode,
    /// 0 follows the system, 1 light, 2 dark.
    pub theme: u8,
    /// Interface language; None follows the system.
    pub language: Option<Lang>,
    /// Which size the views show.
    pub size_mode: SizeMode,
    /// The folders layout: lists above, treemap below.
    pub folders: Panes,
    /// The map layout: chart and file types above, folder list below.
    pub map: Panes,
    /// Last folder chosen in the folder dialog.
    pub last_dir: String,
    /// Window size (logical pixels) and maximized state.
    pub window: Option<(f64, f64, bool)>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            chart: ChartType::Treemap,
            view: ViewMode::Folders,
            theme: 0,
            language: None,
            size_mode: SizeMode::Logical,
            folders: Panes { split: 0.45, split_x: 0.62 },
            map: Panes { split: 0.66, split_x: 0.7 },
            last_dir: String::new(),
            window: None,
        }
    }
}

pub const SPLIT_MIN: f32 = 0.15;
pub const SPLIT_MAX: f32 = 0.85;
pub const SPLIT_X_MIN: f32 = 0.25;
pub const SPLIT_X_MAX: f32 = 0.85;

impl Settings {
    /// The bars of the layout in use.
    pub fn panes(&self) -> Panes {
        match self.view {
            ViewMode::Folders => self.folders,
            ViewMode::Map => self.map,
        }
    }

    pub fn panes_mut(&mut self) -> &mut Panes {
        match self.view {
            ViewMode::Folders => &mut self.folders,
            ViewMode::Map => &mut self.map,
        }
    }

    /// The chart shown: the folders layout has only the treemap.
    pub fn chart_shown(&self) -> ChartType {
        match self.view {
            ViewMode::Folders => ChartType::Treemap,
            ViewMode::Map => self.chart,
        }
    }

    pub fn load(s: &Store) -> Settings {
        let d = Settings::default();
        let frac = |key: &str, min: f32, max: f32, def: f32| {
            s.get(key).and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| (min..=max).contains(v)).unwrap_or(def)
        };
        Settings {
            chart: ChartType::ALL.into_iter().find(|c| s.get("chart") == Some(c.tag())).unwrap_or_default(),
            view: ViewMode::ALL.into_iter().find(|c| s.get("view") == Some(c.tag())).unwrap_or_default(),
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
            folders: Panes {
                split: frac("split", SPLIT_MIN, SPLIT_MAX, d.folders.split),
                split_x: frac("split_x", SPLIT_X_MIN, SPLIT_X_MAX, d.folders.split_x),
            },
            map: Panes {
                split: frac("map_split", SPLIT_MIN, SPLIT_MAX, d.map.split),
                split_x: frac("map_split_x", SPLIT_X_MIN, SPLIT_X_MAX, d.map.split_x),
            },
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
        s.set("view", self.view.tag());
        s.set("theme", ["system", "light", "dark"][self.theme.min(2) as usize]);
        s.set("language", self.language.map_or("", |l| l.tag()));
        s.set(
            "size",
            match self.size_mode {
                SizeMode::Logical => "length",
                SizeMode::Allocated => "disk",
            },
        );
        s.set("split", format!("{:.3}", self.folders.split));
        s.set("split_x", format!("{:.3}", self.folders.split_x));
        s.set("map_split", format!("{:.3}", self.map.split));
        s.set("map_split_x", format!("{:.3}", self.map.split_x));
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
            view: ViewMode::Map,
            theme: 2,
            language: Some(Lang::PtPT),
            size_mode: SizeMode::Allocated,
            folders: Panes { split: 0.6, split_x: 0.5 },
            map: Panes { split: 0.7, split_x: 0.4 },
            last_dir: "C:\\data".into(),
            window: Some((900.0, 700.0, true)),
        };
        assert_eq!(Settings::load(&Store::parse(&s.to_store().serialize("rsize"))), s);
        let bad = Settings::load(&Store::parse("theme=??\nwindow=abc\nlanguage=xx-YY\nsplit=7\nsplit_x=0.01\nmap_split=-1\nsize=huge\nchart=types\nview=3d\n"));
        assert_eq!(bad, Settings::default());
    }

    #[test]
    fn each_layout_keeps_its_bars() {
        let mut s = Settings::default();
        assert_eq!(s.chart_shown(), ChartType::Treemap);
        s.chart = ChartType::Pie;
        assert_eq!(s.chart_shown(), ChartType::Treemap, "the folders layout has the treemap only");
        s.view = ViewMode::Map;
        assert_eq!(s.chart_shown(), ChartType::Pie);
        s.panes_mut().split = 0.3;
        assert_eq!(s.map.split, 0.3);
        assert_eq!(s.folders.split, Settings::default().folders.split);
    }
}
