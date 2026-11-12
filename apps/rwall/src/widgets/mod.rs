//! Desktop widgets: clocks, calendars, notes, annotations, the weather,
//! fences (a box showing a folder) and photos.
//!
//! Each widget is a window of its own over the wallpaper and the icons (see
//! `host`, Windows only): pinned to the desktop, under every other window,
//! unless it is set to stay on top. This module holds what is remembered
//! (`widget.*` in `settings.ini`); `paint` draws a widget into a picture, so
//! it can be tested anywhere; `calendar`, `edit` and `weather` hold the
//! parts without windows.

pub mod calendar;
pub mod edit;
pub mod holidays;
pub mod paint;
pub mod region;
pub mod weather;

#[cfg(windows)]
pub mod host;

use crate::storage::Store;
use std::path::PathBuf;

/// The kinds of widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WKind {
    Analog,
    Digital,
    Calendar,
    /// A clock and the month, side by side.
    ClockCalendar,
    Note,
    /// Text straight on the wallpaper, without a box.
    Annotation,
    Weather,
    /// A box showing the files of a folder.
    Fence,
    Photos,
}

impl WKind {
    pub const ALL: [WKind; 9] =
        [WKind::Analog, WKind::Digital, WKind::Calendar, WKind::ClockCalendar, WKind::Note, WKind::Annotation, WKind::Weather, WKind::Fence, WKind::Photos];

    pub fn key(self) -> &'static str {
        match self {
            WKind::Analog => "analog",
            WKind::Digital => "digital",
            WKind::Calendar => "calendar",
            WKind::ClockCalendar => "clock-calendar",
            WKind::Note => "note",
            WKind::Annotation => "annotation",
            WKind::Weather => "weather",
            WKind::Fence => "fence",
            WKind::Photos => "photos",
        }
    }

    pub fn from_key(k: &str) -> Option<WKind> {
        WKind::ALL.into_iter().find(|w| w.key() == k)
    }

    /// Text in the settings window (translated by the caller).
    pub fn label(self) -> &'static str {
        match self {
            WKind::Analog => "Analog clock",
            WKind::Digital => "Digital clock",
            WKind::Calendar => "Calendar",
            WKind::ClockCalendar => "Clock and calendar",
            WKind::Note => "Sticky note",
            WKind::Annotation => "Annotation",
            WKind::Weather => "Weather",
            WKind::Fence => "Fence",
            WKind::Photos => "Photos",
        }
    }

    /// Size of a new widget, at 100 % scale.
    pub fn default_size(self) -> (i32, i32) {
        match self {
            WKind::Analog => (190, 190),
            WKind::Digital => (300, 128),
            WKind::Calendar => (260, 250),
            WKind::ClockCalendar => (470, 250),
            WKind::Note => (230, 210),
            WKind::Annotation => (300, 70),
            WKind::Weather => (290, 200),
            WKind::Fence => (380, 250),
            WKind::Photos => (300, 220),
        }
    }

    /// The smallest it can be made, at 100 % scale.
    pub fn min_size(self) -> (i32, i32) {
        match self {
            WKind::Analog => (90, 90),
            WKind::Digital => (150, 70),
            WKind::Calendar => (190, 180),
            WKind::ClockCalendar => (340, 180),
            WKind::Note => (130, 100),
            WKind::Annotation => (80, 30),
            WKind::Weather => (200, 150),
            WKind::Fence => (160, 110),
            WKind::Photos => (120, 90),
        }
    }

    /// Keeps its width equal to its height.
    pub fn square(self) -> bool {
        self == WKind::Analog
    }

    /// Takes the keyboard (to write in it).
    pub fn editable(self) -> bool {
        matches!(self, WKind::Note | WKind::Annotation)
    }

    /// Shows the time.
    pub fn has_clock(self) -> bool {
        matches!(self, WKind::Analog | WKind::Digital | WKind::ClockCalendar)
    }

    /// Shows what is in a folder.
    pub fn has_folder(self) -> bool {
        matches!(self, WKind::Fence | WKind::Photos)
    }

    /// Shows a month (and can show its holidays).
    pub fn has_month(self) -> bool {
        matches!(self, WKind::Calendar | WKind::ClockCalendar)
    }

    /// One is enough: clocks, calendars and the weather.
    pub fn single(self) -> bool {
        !matches!(self, WKind::Note | WKind::Annotation | WKind::Fence | WKind::Photos)
    }
}

/// Colours of the sticky notes.
pub const NOTE_COLORS: [(&str, u32); 6] =
    [("Yellow", 0xFFE97A), ("Green", 0xBDF0B0), ("Pink", 0xFFC4DA), ("Blue", 0xBFE0FF), ("Purple", 0xDCC9FF), ("Gray", 0xE4E4E4)];

/// Colours of the annotations.
pub const TEXT_COLORS: [(&str, u32); 4] = [("White", 0xFFFFFF), ("Black", 0x141414), ("Yellow", 0xFFD84A), ("Red", 0xFF6464)];

/// Sizes of the annotations' text, at 100 % scale.
pub const TEXT_SIZES: [(&str, f32); 3] = [("Small", 16.0), ("Medium", 24.0), ("Large", 38.0)];

/// How often the photos change, in minutes.
pub const PHOTO_STEPS: [u32; 8] = [1, 2, 5, 10, 15, 30, 60, 240];

/// One widget on the desktop.
#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    pub id: u32,
    pub kind: WKind,
    /// Where it is, in pixels of the whole desktop (screen coordinates);
    /// `w == 0`: not placed yet.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Over every window, instead of pinned to the desktop.
    pub on_top: bool,
    /// Cannot be moved or resized by mistake.
    pub locked: bool,
    /// Clocks: show the seconds.
    pub seconds: bool,
    /// Clocks: 24-hour time (else 12-hour with AM/PM).
    pub h24: bool,
    /// Calendars: mark the holidays of the country of the regional settings.
    pub holidays: bool,
    /// Notes and annotations.
    pub text: String,
    /// Index in `NOTE_COLORS` or `TEXT_COLORS`.
    pub color: u8,
    /// Annotations: index in `TEXT_SIZES`.
    pub size: u8,
    /// Fences and photos.
    pub folder: PathBuf,
    /// Photos: minutes between photos.
    pub minutes: u32,
    /// Weather: what the user typed.
    pub city: String,
    /// Weather: the place found for `city` ("Lisboa, Portugal"), with its
    /// coordinates; empty until found.
    pub place: String,
    pub lat: f64,
    pub lon: f64,
    pub fahrenheit: bool,
}

impl Widget {
    /// A new widget, with the regional settings' clock, units and city.
    pub fn new(id: u32, kind: WKind) -> Widget {
        let r = region::region();
        Widget {
            id,
            kind,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            on_top: false,
            locked: false,
            seconds: kind == WKind::Analog,
            h24: r.h24,
            holidays: true,
            text: String::new(),
            color: 0,
            size: 1,
            folder: PathBuf::new(),
            minutes: 5,
            city: if kind == WKind::Weather { r.city() } else { String::new() },
            place: String::new(),
            lat: 0.0,
            lon: 0.0,
            fahrenheit: r.fahrenheit,
        }
    }

    /// Placed on the desktop already.
    pub fn placed(&self) -> bool {
        self.w > 0 && self.h > 0
    }

    pub fn has_place(&self) -> bool {
        !self.place.is_empty() && self.lat.is_finite() && self.lon.is_finite()
    }

    /// One line about it, for the list in the settings window.
    pub fn summary(&self) -> String {
        match self.kind {
            WKind::Note | WKind::Annotation => self.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string(),
            WKind::Weather if self.has_place() => self.place.clone(),
            WKind::Weather => self.city.clone(),
            WKind::Fence | WKind::Photos => self.folder.display().to_string(),
            _ => String::new(),
        }
    }

    fn load(s: &Store, id: u32) -> Option<Widget> {
        let key = |f: &str| format!("widget.{id}.{f}");
        let kind = s.get(&key("kind")).and_then(WKind::from_key)?;
        let d = Widget::new(id, kind);
        let int = |f: &str, def: i32| s.get(&key(f)).and_then(|v| v.parse().ok()).unwrap_or(def);
        let flag = |f: &str, def: bool| s.get(&key(f)).map_or(def, |v| v == "1");
        let num = |f: &str| s.get(&key(f)).and_then(|v| v.parse::<f64>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0);
        let text = |f: &str| s.get(&key(f)).unwrap_or("").to_string();
        let (w, h) = (int("w", 0).clamp(0, 10_000), int("h", 0).clamp(0, 10_000));
        Some(Widget {
            x: int("x", 0),
            y: int("y", 0),
            w,
            h: if w == 0 { 0 } else { h },
            on_top: flag("top", false),
            locked: flag("locked", false),
            seconds: flag("seconds", d.seconds),
            h24: flag("h24", d.h24),
            holidays: flag("holidays", d.holidays),
            text: text("text"),
            color: int("color", 0).clamp(0, 255) as u8,
            size: int("size", 1).clamp(0, TEXT_SIZES.len() as i32 - 1) as u8,
            folder: PathBuf::from(text("folder")),
            minutes: int("minutes", d.minutes as i32).clamp(1, 24 * 60) as u32,
            city: s.get(&key("city")).map_or(d.city.clone(), str::to_string),
            place: text("place"),
            lat: num("lat"),
            lon: num("lon"),
            fahrenheit: flag("fahrenheit", d.fahrenheit),
            ..d
        })
    }

    fn save(&self, s: &mut Store) {
        let key = |f: &str| format!("widget.{}.{f}", self.id);
        let flag = |b: bool| if b { "1" } else { "0" };
        s.set(&key("kind"), self.kind.key());
        for (f, v) in [("x", self.x), ("y", self.y), ("w", self.w), ("h", self.h)] {
            s.set(&key(f), v.to_string());
        }
        s.set(&key("top"), flag(self.on_top));
        s.set(&key("locked"), flag(self.locked));
        if self.kind.has_clock() {
            s.set(&key("seconds"), flag(self.seconds));
            s.set(&key("h24"), flag(self.h24));
        }
        if self.kind.has_month() {
            s.set(&key("holidays"), flag(self.holidays));
        }
        if self.kind.editable() {
            s.set(&key("text"), self.text.as_str());
            s.set(&key("color"), self.color.to_string());
        }
        if self.kind == WKind::Annotation {
            s.set(&key("size"), self.size.to_string());
        }
        if self.kind.has_folder() {
            s.set(&key("folder"), self.folder.to_string_lossy());
        }
        if self.kind == WKind::Photos {
            s.set(&key("minutes"), self.minutes.to_string());
        }
        if self.kind == WKind::Weather {
            s.set(&key("city"), self.city.as_str());
            s.set(&key("place"), self.place.as_str());
            s.set(&key("lat"), self.lat.to_string());
            s.set(&key("lon"), self.lon.to_string());
            s.set(&key("fahrenheit"), flag(self.fahrenheit));
        }
    }
}

/// All the widgets (`widgets` and `widget.<id>.*` in `settings.ini`).
#[derive(Clone, Debug, PartialEq)]
pub struct WidgetSettings {
    /// Shown on the desktop.
    pub on: bool,
    pub list: Vec<Widget>,
}

impl Default for WidgetSettings {
    fn default() -> Self {
        WidgetSettings { on: true, list: Vec::new() }
    }
}

/// Most widgets the settings window adds.
pub const MAX_WIDGETS: usize = 16;

/// Most widgets read from the settings (more were allowed before).
const MAX_LOADED: usize = 64;

impl WidgetSettings {
    pub fn load(s: &Store) -> WidgetSettings {
        let ids = s.get("widgets.ids").unwrap_or("");
        let mut list: Vec<Widget> = Vec::new();
        for id in ids.split(',').filter_map(|v| v.trim().parse::<u32>().ok()) {
            if list.len() < MAX_LOADED && !list.iter().any(|w| w.id == id) {
                list.extend(Widget::load(s, id));
            }
        }
        WidgetSettings { on: s.get("widgets").is_none_or(|v| v != "0"), list }
    }

    pub fn save(&self, s: &mut Store) {
        s.set("widgets", if self.on { "1" } else { "0" });
        s.set("widgets.ids", self.list.iter().map(|w| w.id.to_string()).collect::<Vec<_>>().join(","));
        for w in &self.list {
            w.save(s);
        }
    }

    /// Another widget of `kind` fits: not too many, and not a second clock,
    /// calendar or weather of the same kind.
    pub fn can_add(&self, kind: WKind) -> bool {
        self.list.len() < MAX_WIDGETS && !(kind.single() && self.list.iter().any(|w| w.kind == kind))
    }

    /// Adds a widget of `kind`, not placed yet; None when it cannot be added.
    pub fn add(&mut self, kind: WKind) -> Option<&mut Widget> {
        if !self.can_add(kind) {
            return None;
        }
        let id = self.list.iter().map(|w| w.id).max().unwrap_or(0) + 1;
        self.list.push(Widget::new(id, kind));
        self.list.last_mut()
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Widget> {
        self.list.iter_mut().find(|w| w.id == id)
    }

    pub fn remove(&mut self, id: u32) {
        self.list.retain(|w| w.id != id);
    }

    /// Puts `w` in place of the widget with its id (or adds it).
    pub fn replace(&mut self, w: Widget) {
        match self.list.iter().position(|x| x.id == w.id) {
            Some(i) => self.list[i] = w,
            None if self.list.len() < MAX_LOADED => self.list.push(w),
            None => {}
        }
    }

    /// Something to show on the desktop.
    pub fn showing(&self) -> bool {
        self.on && !self.list.is_empty()
    }
}

/// Steps of `PHOTO_STEPS` up or down from `m`.
pub fn photo_step(m: u32, up: bool) -> u32 {
    if up {
        PHOTO_STEPS.into_iter().find(|s| *s > m).unwrap_or(PHOTO_STEPS[PHOTO_STEPS.len() - 1])
    } else {
        PHOTO_STEPS.into_iter().rev().find(|s| *s < m).unwrap_or(PHOTO_STEPS[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut ws = WidgetSettings { on: false, ..Default::default() };
        for k in WKind::ALL {
            let w = ws.add(k).unwrap();
            w.x = -1200;
            w.y = 40;
            w.w = 300;
            w.h = 200;
        }
        let n = ws.get_mut(5).unwrap();
        assert_eq!(n.kind, WKind::Note);
        n.text = "Comprar pão\n= leite; ovos\\ok".into();
        n.color = 3;
        n.on_top = true;
        ws.get_mut(3).unwrap().holidays = false;
        let wx = ws.get_mut(7).unwrap();
        wx.city = "Lisboa".into();
        wx.place = "Lisboa, Portugal".into();
        wx.lat = 38.716_89;
        wx.lon = -9.139_66;
        wx.fahrenheit = false;
        ws.get_mut(8).unwrap().folder = PathBuf::from(r"C:\Users\Me\Desktop\Projetos");
        ws.get_mut(9).unwrap().minutes = 15;
        let mut s = Store::default();
        ws.save(&mut s);
        let back = WidgetSettings::load(&Store::parse(&s.serialize("rwall")));
        assert_eq!(back, ws);
        // Removing keeps the others; a new one gets a new id.
        ws.remove(2);
        assert_eq!(ws.add(WKind::Note).unwrap().id, 10);
        assert!(ws.showing() == ws.on);
        // One clock of a kind, any number of notes, up to the most.
        assert!(ws.add(WKind::Analog).is_none());
        assert!(ws.add(WKind::Digital).is_some());
        while ws.add(WKind::Note).is_some() {}
        assert_eq!(ws.list.len(), MAX_WIDGETS);
        assert!(!ws.can_add(WKind::Fence));
    }

    #[test]
    fn bad_values() {
        let s = Store::parse("widgets.ids=3,x,3,4\nwidget.3.kind=note\nwidget.3.w=-5\nwidget.3.h=90\nwidget.3.size=9\nwidget.4.kind=zzz\n");
        let ws = WidgetSettings::load(&s);
        assert!(ws.on);
        assert_eq!(ws.list.len(), 1);
        let w = &ws.list[0];
        assert!(!w.placed() && w.h == 0);
        assert_eq!(w.size as usize, TEXT_SIZES.len() - 1);
        assert_eq!(photo_step(5, true), 10);
        assert_eq!(photo_step(1, false), 1);
        assert_eq!(photo_step(240, true), 240);
        assert_eq!(photo_step(7, false), 5);
    }
}
