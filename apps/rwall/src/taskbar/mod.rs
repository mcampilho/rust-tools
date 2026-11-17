//! A see-through taskbar (Windows): clear, blurred or acrylic, with a tint,
//! chosen for each screen. The look is DWM's "accent" of the taskbar
//! windows (`taskbar::win`); Explorer puts its own back whenever it redraws
//! the bar, so it is kept while rwall runs, and the normal bar comes back
//! when it stops.
//!
//! The settings are `taskbar=<screen>:<look>:<tint>` lines in `settings.ini`,
//! one per screen whose bar is not normal.

use crate::storage::Store;

/// How a taskbar looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Look {
    /// As Windows draws it.
    #[default]
    Normal,
    /// The wallpaper shows through, with the tint only.
    Clear,
    /// The wallpaper blurred behind it.
    Blur,
    /// Blurred, with Windows' grain and the tint (Windows 10 1803 and later).
    Acrylic,
}

impl Look {
    pub const ALL: [Look; 4] = [Look::Normal, Look::Clear, Look::Blur, Look::Acrylic];

    pub fn key(self) -> &'static str {
        match self {
            Look::Normal => "normal",
            Look::Clear => "clear",
            Look::Blur => "blur",
            Look::Acrylic => "acrylic",
        }
    }

    pub fn from_key(k: &str) -> Option<Look> {
        Look::ALL.into_iter().find(|l| l.key() == k)
    }

    /// The text shown (a key of the table).
    pub fn label(self) -> &'static str {
        match self {
            Look::Normal => "Normal",
            Look::Clear => "Clear",
            Look::Blur => "Blurred",
            Look::Acrylic => "Acrylic",
        }
    }

    /// Whether the tint means something for it (blur has none).
    pub fn has_tint(self) -> bool {
        matches!(self, Look::Clear | Look::Acrylic)
    }
}

/// Steps of the tint, in percent.
pub const TINT_STEPS: [u8; 11] = [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100];

/// A screen's taskbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub look: Look,
    /// How much of the tint colour covers the bar, 0 to 100.
    pub tint: u8,
}

impl Default for Style {
    fn default() -> Self {
        Style { look: Look::Normal, tint: 30 }
    }
}

impl Style {
    /// The next tint step up (`more`) or down.
    pub fn step(&mut self, more: bool) {
        let i = TINT_STEPS.iter().position(|t| *t >= self.tint).unwrap_or(TINT_STEPS.len() - 1);
        let j = if more { (i + 1).min(TINT_STEPS.len() - 1) } else { i.saturating_sub(1) };
        self.tint = TINT_STEPS[j];
    }

    /// DWM's accent for it: the state (0 normal, 2 a colour that lets the
    /// wallpaper through, 3 blur, 4 acrylic) and the colour as 0xAABBGGRR.
    /// The tint is black on a dark theme, white on a light one.
    pub fn accent(self, dark: bool) -> (i32, u32) {
        let rgb: u32 = if dark { 0x000000 } else { 0xFFFFFF };
        let mut alpha = (u32::from(self.tint.min(100)) * 255 + 50) / 100;
        let state = match self.look {
            Look::Normal => return (0, 0),
            Look::Clear => 2,
            Look::Blur => 3,
            // Acrylic with no colour at all makes the bar lag behind the mouse.
            Look::Acrylic => {
                alpha = alpha.max(1);
                4
            }
        };
        (state, (alpha << 24) | rgb)
    }
}

/// The looks of the screens' taskbars, by screen ("DISPLAY1").
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TaskbarSettings {
    pub screens: Vec<(String, Style)>,
}

impl TaskbarSettings {
    pub fn load(s: &Store) -> TaskbarSettings {
        let screens = s
            .get_all("taskbar")
            .filter_map(|line| {
                let mut p = line.rsplitn(3, ':');
                let tint = p.next()?.trim().parse::<u8>().ok()?.min(100);
                let look = Look::from_key(p.next()?.trim())?;
                let screen = p.next()?.trim();
                (!screen.is_empty()).then(|| (screen.to_string(), Style { look, tint }))
            })
            .collect();
        TaskbarSettings { screens }
    }

    pub fn save(&self, s: &mut Store) {
        for (screen, st) in self.screens.iter().filter(|(_, st)| st.look != Look::Normal) {
            s.push("taskbar", format!("{screen}:{}:{}", st.look.key(), st.tint));
        }
    }

    pub fn get(&self, screen: &str) -> Style {
        self.screens.iter().find(|(s, _)| s == screen).map_or_else(Style::default, |(_, st)| *st)
    }

    pub fn set(&mut self, screen: &str, st: Style) {
        match self.screens.iter_mut().find(|(s, _)| s == screen) {
            Some((_, old)) => *old = st,
            None => self.screens.push((screen.to_string(), st)),
        }
    }

    /// Whether any bar is to look other than normal.
    pub fn any(&self) -> bool {
        self.screens.iter().any(|(_, st)| st.look != Look::Normal)
    }
}

/// A screen, as the settings window lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    /// Windows' name without `\\.\` ("DISPLAY1").
    pub id: String,
    pub primary: bool,
    pub width: u32,
    pub height: u32,
}

#[cfg(windows)]
pub mod win;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        assert_eq!(Style { look: Look::Normal, tint: 50 }.accent(true), (0, 0));
        assert_eq!(Style { look: Look::Clear, tint: 0 }.accent(true), (2, 0x0000_0000));
        assert_eq!(Style { look: Look::Clear, tint: 100 }.accent(false), (2, 0xFFFF_FFFF));
        assert_eq!(Style { look: Look::Blur, tint: 50 }.accent(true), (3, 0x8000_0000));
        // Acrylic never goes fully clear.
        assert_eq!(Style { look: Look::Acrylic, tint: 0 }.accent(true), (4, 0x0100_0000));
    }

    #[test]
    fn steps() {
        let mut st = Style { look: Look::Clear, tint: 35 };
        st.step(true);
        assert_eq!(st.tint, 50);
        st.step(false);
        assert_eq!(st.tint, 40);
        let mut top = Style { look: Look::Clear, tint: 100 };
        top.step(true);
        assert_eq!(top.tint, 100);
        let mut low = Style { look: Look::Clear, tint: 0 };
        low.step(false);
        assert_eq!(low.tint, 0);
    }

    #[test]
    fn round_trip() {
        let mut t = TaskbarSettings::default();
        t.set("DISPLAY1", Style { look: Look::Acrylic, tint: 40 });
        t.set("DISPLAY2", Style { look: Look::Normal, tint: 30 });
        t.set("DISPLAY3", Style { look: Look::Blur, tint: 10 });
        let mut s = Store::default();
        t.save(&mut s);
        let back = TaskbarSettings::load(&Store::parse(&s.serialize("rwall")));
        assert_eq!(back.get("DISPLAY1"), Style { look: Look::Acrylic, tint: 40 });
        assert_eq!(back.get("DISPLAY3").look, Look::Blur);
        // Normal ones are not written; unknown ones come back normal.
        assert_eq!(back.screens.len(), 2);
        assert_eq!(back.get("DISPLAY9"), Style::default());
        assert!(back.any() && !TaskbarSettings::default().any());
        let bad = TaskbarSettings::load(&Store::parse("taskbar=DISPLAY1:glass:20\ntaskbar=:clear:10\ntaskbar=x\n"));
        assert!(bad.screens.is_empty());
    }
}
