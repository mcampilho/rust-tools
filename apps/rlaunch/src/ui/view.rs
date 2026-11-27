//! The menu, drawn in one function that gives back what can be clicked: a
//! search box on top, the list on the left, the user's folders on the right
//! and the space of the drive at the bottom.

use super::gfx::{Align, Color, FontId, Gfx, Rect};
use super::theme::Theme;
use crate::app::{App, Row, View};
use crate::category::{Category, Group};
use crate::i18n::{tf, tr, Lang};
use crate::settings::{Hotkey, Settings};
use crate::ui::theme::{colour_name, frame_of, CHOICES};
use rt_core::textedit::TextEdit;
use rt_gfx::dialog::{ellipsize, ellipsize_middle};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tiny_skia::Pixmap;

/// The size of the menu at 100 %, in logical pixels.
pub const SIZE: (f64, f64) = (350.0, 480.0);

/// The shield of the Windows icon font.
const SHIELD: &str = "\u{EA18}";

/// An option chosen by going through its values with ‹ and ›.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Hotkey,
    Size,
    Transparency,
    Theme,
    Language,
    Rows,
    Position,
    /// Categories by name, or as the user arranged them.
    Order,
}

/// The folders of the user in the right column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Documents,
    Downloads,
    Pictures,
    Music,
    Videos,
    /// The folder or drive rlaunch is on.
    Explore,
}

impl Place {
    pub const ALL: [Place; 6] = [Place::Documents, Place::Downloads, Place::Pictures, Place::Music, Place::Videos, Place::Explore];

    fn name(self) -> String {
        match self {
            Place::Documents => tr("Documents"),
            Place::Downloads => tr("Downloads"),
            Place::Pictures => tr("Pictures"),
            Place::Music => tr("Music"),
            Place::Videos => tr("Videos"),
            Place::Explore => tr("Explore"),
        }
    }

    /// A glyph of the Windows icon font.
    fn glyph(self) -> &'static str {
        match self {
            Place::Documents => "\u{E8A5}",
            Place::Downloads => "\u{E896}",
            Place::Pictures => "\u{E8B9}",
            Place::Music => "\u{E8D6}",
            Place::Videos => "\u{E714}",
            Place::Explore => "\u{E8B7}",
        }
    }
}

/// What a click does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Row(usize),
    Place(Place),
    Options,
    About,
    /// Starts rlaunch again with administrator rights (Windows).
    Elevate,
    Exit,
    Close,
    /// Keeps the menu open when something else gets the focus.
    Pin,
    Scoop,
    StartMenu,
    AddFolder,
    AddProgram,
    ShowHidden,
    RemoveFolder(usize),
    /// The next way of taking categories from a folder.
    FolderMode(usize),
    Autostart,
    AlwaysAdmin,
    /// The next value of an option, or the one before.
    Step(Field, bool),
    /// A page of the options: its appearance, or else its programs.
    Page(bool),
    Colour(u8),
    CheckUpdates,
    Done,
}

/// What a name is being typed for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editing {
    /// The name of a program.
    Name(PathBuf),
    /// A new category for a program, typed in its row.
    NewCategory(PathBuf),
    /// The name of a category or subgroup.
    Group(Group),
}

impl Editing {
    /// Whether it is typed in the row of the program at `path`.
    pub fn is_program(&self, path: &Path) -> bool {
        matches!(self, Editing::Name(p) | Editing::NewCategory(p) if p == path)
    }
}

/// Where what is dragged would go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropMark {
    Into,
    Before,
    After,
}

pub struct Model<'a> {
    pub app: &'a App,
    pub settings: &'a Settings,
    /// The icons read so far, by the file they come from.
    pub icons: &'a HashMap<PathBuf, Option<Pixmap>>,
    /// Something to tell the user, over the footer.
    pub note: Option<&'a str>,
    pub autostart: bool,
    pub hot: Option<Hit>,
    /// The drive's name, free and total bytes.
    pub disk: Option<(String, u64, u64)>,
    /// Whether the caret is in its visible half.
    pub caret: bool,
    pub pinned: bool,
    /// rlaunch has administrator rights, so what it opens has them too.
    pub elevated: bool,
    /// The system's accent colour, for the colour made from it.
    pub accent: Option<Color>,
    /// The row something is dragged to.
    pub drop: Option<(usize, DropMark)>,
    /// What is being named, and what was typed.
    pub rename: Option<(&'a Editing, &'a TextEdit)>,
}

/// What was drawn.
#[derive(Default)]
pub struct Drawn {
    pub hits: Vec<(Rect, Hit)>,
    /// Rows of the list that fit.
    pub visible: usize,
    /// The entries shown, whose icons are wanted.
    pub shown: Vec<usize>,
}

impl Drawn {
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h)
    }
}

/// An image from straight RGBA pixels.
pub fn pixmap(w: u32, h: u32, rgba: Vec<u8>) -> Option<Pixmap> {
    let mut pm = Pixmap::new(w, h)?;
    for (d, s) in pm.pixels_mut().iter_mut().zip(rgba.as_chunks::<4>().0) {
        *d = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
    }
    Some(pm)
}

struct Painter<'a, 'm> {
    g: &'a mut Gfx,
    t: &'a Theme,
    m: &'a Model<'m>,
    s: f32,
    out: Drawn,
}

/// The part of `r` that keeps the proportions of a `w`×`h` image, centred.
fn fitted(r: Rect, w: u32, h: u32) -> Rect {
    let scale = (r.w / w.max(1) as f32).min(r.h / h.max(1) as f32);
    let (width, height) = (w as f32 * scale, h as f32 * scale);
    Rect::new(r.x + (r.w - width) / 2.0, r.y + (r.h - height) / 2.0, width, height)
}

fn text_middle(g: &mut Gfx, text: &str, r: Rect, px: f32, font: FontId, c: Color) {
    let cut = ellipsize(g, text, r.w, px, font);
    g.text_in(&cut, r, px, font, c, Align::Left);
}

impl Painter<'_, '_> {
    fn px(&self, v: f32) -> f32 {
        (v * self.s).round()
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(self.px(x), self.px(y), self.px(w), self.px(h))
    }

    fn icon_glyph(&mut self, glyph: &str, r: Rect, size: f32, c: Color) {
        if self.g.has_icons() {
            let px = self.px(size);
            self.g.text_in(glyph, r, px, FontId::Icons, c, Align::Center);
        } else {
            let side = self.px(size);
            let at = Rect::new(r.x + (r.w - side) / 2.0, r.y + (r.h - side) / 2.0, side, side);
            super::symbols::draw(self.g, glyph, at, self.px(1.2).max(1.0), c);
        }
    }

    fn header(&mut self) {
        let t = *self.t;
        let logo = self.rect(11.0, 10.0, 24.0, 24.0);
        // A shield in place of the logo says the menu has administrator rights.
        if self.m.elevated {
            let badge = self.rect(9.0, 8.0, 28.0, 28.0);
            self.g.fill_rrect(badge, self.px(5.0), t.warn);
            self.icon_glyph(SHIELD, badge, 17.0, t.frame);
        } else if let Some(pm) = pixmap(64, 64, super::appicon::render(64)) {
            self.g.draw_image(&pm, logo);
        }
        let pin = self.rect(316.0, 8.0, 28.0, 28.0);
        if self.m.pinned {
            self.g.fill_rrect(pin, self.px(5.0), t.accent);
        } else if self.m.hot == Some(Hit::Pin) {
            self.g.fill_rrect(pin, self.px(5.0), t.frame_hover);
        }
        self.icon_glyph(
            if self.m.pinned { "\u{E840}" } else { "\u{E718}" },
            pin,
            13.0,
            if self.m.pinned { t.on_accent } else { t.frame_text },
        );
        self.out.hits.push((pin, Hit::Pin));
        // The search box: typing goes here wherever the pointer is.
        let field = self.rect(44.0, 8.0, 266.0, 28.0);
        self.g.fill_rrect(field, self.px(6.0), t.list);
        let inner = Rect::new(field.x + self.px(10.0), field.y, field.w - self.px(20.0), field.h);
        let size = self.px(13.0);
        let query = &self.m.app.query;
        let mut caret_x = inner.x;
        if query.is_empty() {
            let hint = tr("Search…");
            self.g.text_in(&hint, Rect::new(inner.x + self.px(4.0), inner.y, inner.w, inner.h), size, FontId::Regular, t.dim, Align::Left);
        } else {
            let shown = ellipsize_middle(self.g, query, inner.w - self.px(4.0), size, FontId::Regular);
            self.g.text_in(&shown, inner, size, FontId::Regular, t.text, Align::Left);
            caret_x += self.g.measure(&shown, size, FontId::Regular) + self.px(1.0);
        }
        if self.m.caret && self.m.app.view != View::Options && self.m.rename.is_none() {
            self.g.fill_rect(Rect::new(caret_x, field.y + self.px(6.0), self.px(1.0).max(1.0), field.h - self.px(12.0)), t.text);
        }
    }

    /// One line of the right column.
    fn side(&mut self, y: f32, glyph: &str, label: &str, hit: Hit) {
        let t = *self.t;
        let r = self.rect(220.0, y, 127.0, 27.0);
        if self.m.hot == Some(hit) {
            self.g.fill_rrect(r, self.px(5.0), t.frame_hover);
        }
        let icon = self.rect(221.0, y, 20.0, 27.0);
        self.icon_glyph(glyph, icon, 14.0, t.frame_dim);
        let size = self.px(13.0);
        let text = self.rect(242.0, y, 106.0, 27.0);
        text_middle(self.g, label, text, size, FontId::Regular, t.frame_text);
        self.out.hits.push((r, hit));
    }

    fn column(&mut self) {
        let mut y = 44.0;
        for place in Place::ALL {
            self.side(y, place.glyph(), &place.name(), Hit::Place(place));
            y += 28.0;
        }
        let line = self.rect(226.0, y + 4.0, 114.0, 1.0);
        let c = self.t.frame_hover;
        self.g.fill_rect(line, c);
        y += 10.0;
        self.side(y, "\u{E713}", &tr("Options"), Hit::Options);
        self.side(y + 28.0, "\u{E946}", &tr("About"), Hit::About);
        y += 56.0;
        if cfg!(windows) && !self.m.elevated {
            self.side(y, SHIELD, &tr("Admin mode"), Hit::Elevate);
            y += 28.0;
        }
        self.side(y, "\u{E7E8}", &tr("Exit"), Hit::Exit);
    }

    fn footer(&mut self) {
        let t = *self.t;
        if let Some((name, free, total)) = &self.m.disk {
            let label = format!("{name}  {}", tf("{} free of {}", &[&rt_scan::format::size(*free), &rt_scan::format::size(*total)]));
            let r = self.rect(10.0, 442.0, 204.0, 16.0);
            let size = self.px(11.0);
            text_middle(self.g, &label, r, size, FontId::Regular, t.frame_dim);
            let bar = self.rect(10.0, 463.0, 204.0, 5.0);
            self.g.fill_rrect(bar, self.px(2.5), t.frame_hover);
            if *total > 0 {
                let used = (total.saturating_sub(*free) as f64 / *total as f64) as f32;
                self.g.fill_rrect(Rect::new(bar.x, bar.y, (bar.w * used).max(bar.h), bar.h), self.px(2.5), t.accent);
            }
        }
        let close = self.rect(314.0, 447.0, 30.0, 26.0);
        if self.m.hot == Some(Hit::Close) {
            self.g.fill_rrect(close, self.px(5.0), t.frame_hover);
        }
        self.icon_glyph("\u{E8BB}", close, 11.0, t.frame_text);
        self.out.hits.push((close, Hit::Close));
    }

    /// The categories or the programs.
    fn list(&mut self, area: Rect) {
        let t = *self.t;
        let app = self.m.app;
        let rows = app.rows();
        let large = self.m.settings.large;
        let row_h = self.px(if large { 36.0 } else { 26.0 });
        // The side of an icon, and where the name starts.
        let side = if large { 24.0 } else { 18.0 };
        let indent = side + 13.0;
        self.out.visible = ((area.h - self.px(8.0)) / row_h).floor().max(1.0) as usize;
        let size = self.px(if large { 14.0 } else { 13.0 });
        if rows.is_empty() {
            let text = if app.loading {
                tr("Reading the programs…")
            } else if app.query.trim().is_empty() {
                tr("No programs found. Turn on a source in Options.")
            } else {
                tr("Nothing matches.")
            };
            let mut y = area.y + self.px(16.0);
            for line in rt_gfx::dialog::wrap_words(self.g, &text, area.w - self.px(28.0), size, FontId::Regular) {
                let r = Rect::new(area.x + self.px(14.0), y, area.w - self.px(28.0), self.px(22.0));
                self.g.text_in(&line, r, size, FontId::Regular, t.dim, Align::Left);
                y += self.px(22.0);
            }
            return;
        }
        let first = app.scroll.min(rows.len().saturating_sub(1));
        for (k, row) in rows.iter().enumerate().skip(first).take(self.out.visible) {
            let r = Rect::new(area.x + self.px(4.0), area.y + self.px(4.0) + (k - first) as f32 * row_h, area.w - self.px(14.0), row_h);
            let marked = matches!(*row, Row::Entry(i) if app.marked.contains(&app.entries[i].path));
            // While programs are marked, only they stand out.
            let selected = k == app.selected && app.marked.is_empty() && self.m.drop.is_none();
            if marked {
                self.g.fill_rrect(r.inset(self.px(1.0).max(1.0)), self.px(5.0), t.accent.mix(t.list, 0.72));
            }
            if selected {
                self.g.fill_rrect(r, self.px(5.0), t.accent);
            } else if self.m.hot == Some(Hit::Row(k)) && !marked {
                self.g.fill_rrect(r, self.px(5.0), t.hover);
            }
            match self.m.drop.filter(|d| d.0 == k) {
                Some((_, DropMark::Into)) => self.g.stroke_rrect(r, self.px(5.0), self.px(2.0), t.accent),
                Some((_, mark)) => {
                    let y = if mark == DropMark::Before { r.y } else { r.bottom() - self.px(2.0) };
                    self.g.fill_rect(Rect::new(r.x + self.px(4.0), y, r.w - self.px(8.0), self.px(2.0).max(2.0)), t.accent);
                }
                None => {}
            }
            let (ink, faint) = if selected { (t.on_accent, t.on_accent) } else { (t.text, t.dim) };
            let icon = Rect::new(r.x + self.px(6.0), r.y + ((r.h - self.px(side)) / 2.0).round(), self.px(side), self.px(side));
            let count_w = match *row {
                Row::Category(..) => 36.0,
                Row::Entry(i) if app.entries[i].admin => 22.0,
                _ => 4.0,
            };
            let text = Rect::new(r.x + self.px(indent), r.y, r.w - self.px(indent + count_w), r.h);
            match *row {
                Row::Category(g, n) => {
                    // A line parts the favourites from the categories.
                    if k > first && matches!(rows[k - 1], Row::Entry(_)) {
                        let line = Rect::new(r.x + self.px(6.0), r.y, r.w - self.px(12.0), self.px(1.0).max(1.0));
                        self.g.fill_rect(line, t.border);
                    }
                    self.folder(icon, g.category);
                    match self.m.rename {
                        Some((Editing::Group(named), edit)) if *named == g => self.field(text, edit),
                        _ => text_middle(self.g, &g.title(), text, size, FontId::Semibold, ink),
                    }
                    let count = Rect::new(r.right() - self.px(38.0), r.y, self.px(31.0), r.h);
                    let small = self.px(11.0);
                    self.g.text_in(&n.to_string(), count, small, FontId::Regular, faint, Align::Right);
                }
                Row::Back => {
                    let title = match app.view {
                        View::Inside(g) => g.title(),
                        _ => tr("Hidden programs"),
                    };
                    self.icon_glyph("\u{E72B}", icon, 13.0, ink);
                    text_middle(self.g, &title, text, size, FontId::Semibold, ink);
                }
                Row::Entry(i) => {
                    let e = &app.entries[i];
                    match self.m.icons.get(e.icon_source()) {
                        Some(Some(pm)) => self.g.draw_image(pm, fitted(icon, pm.width(), pm.height())),
                        _ => self.letter(icon, &e.name),
                    }
                    match self.m.rename {
                        Some((what, edit)) if what.is_program(&e.path) => self.field(text, edit),
                        _ => text_middle(self.g, &e.name, text, size, FontId::Regular, ink),
                    }
                    if e.admin {
                        let shield = Rect::new(r.right() - self.px(22.0), r.y, self.px(18.0), r.h);
                        self.icon_glyph(SHIELD, shield, 13.0, if selected { ink } else { t.warn });
                    }
                    self.out.shown.push(i);
                }
            }
            self.out.hits.push((r, Hit::Row(k)));
        }
        if rows.len() > self.out.visible {
            let track = Rect::new(area.right() - self.px(8.0), area.y + self.px(6.0), self.px(4.0), area.h - self.px(12.0));
            let h = (track.h * self.out.visible as f32 / rows.len() as f32).max(self.px(24.0));
            let y = track.y + (track.h - h) * first as f32 / (rows.len() - self.out.visible).max(1) as f32;
            self.g.fill_rrect(Rect::new(track.x, y, track.w, h), self.px(2.0), t.border);
        }
    }

    /// The name being typed in a row, with its selection and caret.
    fn field(&mut self, text: Rect, edit: &TextEdit) {
        let t = *self.t;
        let size = self.px(13.0);
        let r = Rect::new(text.x - self.px(4.0), text.y + self.px(2.0), text.w + self.px(2.0), text.h - self.px(4.0));
        self.g.fill_rrect(r, self.px(4.0), t.list);
        self.g.stroke_rrect(r, self.px(4.0), self.px(1.0).max(1.0), t.accent);
        // Scrolled so the caret stays in the field.
        let before = self.g.measure(&edit.text[..edit.caret], size, FontId::Regular);
        let x = text.x - (before - (text.w - self.px(6.0))).max(0.0);
        self.g.set_clip(Some(r));
        if let Some((a, b)) = edit.selection() {
            let from = self.g.measure(&edit.text[..a], size, FontId::Regular);
            let to = self.g.measure(&edit.text[..b], size, FontId::Regular);
            self.g.fill_rect(Rect::new(x + from, r.y + self.px(3.0), to - from, r.h - self.px(6.0)), t.hover);
        }
        let wide = self.g.measure(&edit.text, size, FontId::Regular) + self.px(4.0);
        self.g.text_in(&edit.text, Rect::new(x, text.y, wide, text.h), size, FontId::Regular, t.text, Align::Left);
        if self.m.caret {
            self.g.fill_rect(Rect::new(x + before, r.y + self.px(4.0), self.px(1.0).max(1.0), r.h - self.px(8.0)), t.text);
        }
        self.g.set_clip(None);
    }

    /// A folder in the category's colour.
    fn folder(&mut self, r: Rect, c: Category) {
        let color = Color::rgb(match c {
            Category::Internet => 0x2F8FE0,
            Category::Multimedia => 0xD1558B,
            Category::Graphics => 0x8E5BD0,
            Category::Office => 0x2E9E6A,
            Category::Development => 0x4C6FD6,
            Category::System => 0x6E7C8C,
            Category::Security => 0xC7503E,
            Category::Games => 0xE08A22,
            Category::Utilities => 0x1FA0A8,
            Category::Other => 0xB49B3C,
            // The user's: a colour that stays with the name.
            Category::Own(name) => {
                const OWN: [u32; 8] = [0x5B8DB8, 0xA96FB0, 0x6C9A5B, 0xC07A5A, 0x4F9C95, 0x8A82C9, 0xB26B7D, 0x8F8F5E];
                OWN[name.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(usize::from(b))) % OWN.len()]
            }
        });
        let tab = Rect::new(r.x + self.px(1.0), r.y + r.h * 0.11, r.w * 0.45, r.h * 0.28);
        self.g.fill_rrect(tab, self.px(2.0), color);
        let body = Rect::new(r.x + self.px(1.0), r.y + r.h * 0.22, r.w - self.px(2.0), r.h * 0.66);
        self.g.fill_rrect(body, self.px(3.0), color);
    }

    /// In place of an icon not read yet (or not there).
    fn letter(&mut self, r: Rect, name: &str) {
        let t = *self.t;
        self.g.fill_rrect(r, self.px(5.0), t.border);
        let first: String = name.chars().take(1).flat_map(char::to_uppercase).collect();
        let size = self.px(11.0);
        self.g.text_in(&first, r, size, FontId::Semibold, t.text, Align::Center);
    }

    fn check(&mut self, x: f32, y: f32, w: f32, label: &str, on: bool, hit: Hit) {
        let t = *self.t;
        let r = self.rect(x, y, w, 24.0);
        let b = self.rect(x + 2.0, y + 4.0, 16.0, 16.0);
        let radius = self.px(4.0);
        self.g.fill_rrect(b, radius, if on { t.accent } else { t.list });
        self.g.stroke_rrect(b, radius, self.px(1.0).max(1.0), if on { t.accent } else { t.dim });
        if on {
            let p = [(b.x + b.w * 0.22, b.y + b.h * 0.52), (b.x + b.w * 0.42, b.y + b.h * 0.72), (b.x + b.w * 0.8, b.y + b.h * 0.3)];
            self.g.polyline(&p, self.px(2.0), t.on_accent, false);
        }
        let text = self.rect(x + 26.0, y, w - 26.0, 24.0);
        let size = self.px(13.0);
        text_middle(self.g, label, text, size, if self.m.hot == Some(hit) { FontId::Semibold } else { FontId::Regular }, t.text);
        self.out.hits.push((r, hit));
    }

    fn heading(&mut self, y: f32, label: &str) {
        let r = self.rect(16.0, y, 318.0, 20.0);
        let size = self.px(12.0);
        let c = self.t.dim;
        text_middle(self.g, label, r, size, FontId::Semibold, c);
    }

    fn button(&mut self, r: Rect, label: &str, hit: Hit) {
        let t = *self.t;
        self.g.fill_rrect(r, self.px(5.0), if self.m.hot == Some(hit) { t.hover } else { t.list });
        self.g.stroke_rrect(r, self.px(5.0), self.px(1.0).max(1.0), t.border);
        let size = self.px(13.0);
        let cut = ellipsize(self.g, label, r.w - self.px(8.0), size, FontId::Regular);
        self.g.text_in(&cut, r, size, FontId::Regular, t.text, Align::Center);
        self.out.hits.push((r, hit));
    }

    /// A line with the name of an option, its value and ‹ › to change it.
    fn stepper(&mut self, y: f32, label: &str, value: &str, field: Field) {
        let t = *self.t;
        let size = self.px(13.0);
        let name = self.rect(16.0, y, 128.0, 26.0);
        text_middle(self.g, label, name, size, FontId::Regular, t.text);
        let less = self.rect(148.0, y, 26.0, 26.0);
        self.button(less, "‹", Hit::Step(field, false));
        let r = self.rect(176.0, y, 130.0, 26.0);
        let cut = ellipsize(self.g, value, r.w, size, FontId::Semibold);
        self.g.text_in(&cut, r, size, FontId::Semibold, t.text, Align::Center);
        let more = self.rect(308.0, y, 26.0, 26.0);
        self.button(more, "›", Hit::Step(field, true));
    }

    fn options(&mut self) {
        let appearance = self.m.app.appearance;
        for (x, label, page) in [(16.0, tr("Programs"), false), (178.0, tr("Appearance"), true)] {
            let r = self.rect(x, 48.0, 156.0, 26.0);
            self.button(r, &label, Hit::Page(page));
            if page == appearance {
                let line = Rect::new(r.x + self.px(8.0), r.bottom() - self.px(3.0), r.w - self.px(16.0), self.px(2.0));
                let c = self.t.accent;
                self.g.fill_rect(line, c);
            }
        }
        if appearance {
            self.appearance_page();
        } else {
            self.programs_page();
        }
    }

    fn appearance_page(&mut self) {
        let st = self.m.settings;
        let t = *self.t;
        let theme = match st.theme {
            1 => tr("Light"),
            2 => tr("Dark"),
            _ => tr("As the system"),
        };
        self.stepper(86.0, &tr("Theme"), &theme, Field::Theme);
        let name = self.rect(16.0, 116.0, 90.0, 26.0);
        let size = self.px(13.0);
        text_middle(self.g, &tr("Color"), name, size, FontId::Regular, t.text);
        for c in 0..CHOICES {
            let r = self.rect(110.0 + f32::from(c) * 28.0, 116.0, 26.0, 26.0);
            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            if c == st.colour {
                self.g.fill_circle(cx, cy, self.px(13.0), t.accent);
                self.g.fill_circle(cx, cy, self.px(11.0), t.list);
            }
            self.g.fill_circle(cx, cy, self.px(9.0), frame_of(c, false, self.m.accent));
            self.out.hits.push((r, Hit::Colour(c)));
        }
        let chosen = self.rect(110.0, 142.0, 224.0, 18.0);
        let small = self.px(11.0);
        let label = tr(colour_name(st.colour));
        self.g.text_in(&label, chosen, small, FontId::Regular, t.dim, Align::Center);
        self.stepper(168.0, &tr("Size"), &format!("{}%", st.size), Field::Size);
        let mut y = 195.0;
        if cfg!(windows) {
            let value = if st.transparency == 0 { tr("Off") } else { format!("{}%", st.transparency) };
            self.stepper(y, &tr("Transparency"), &value, Field::Transparency);
            y += 27.0;
        }
        let rows = if st.large { tr("Large") } else { tr("Compact") };
        self.stepper(y, &tr("Rows"), &rows, Field::Rows);
        y += 27.0;
        self.stepper(y, &tr("Position"), &tr(st.position.name()), Field::Position);
        y += 27.0;
        let order = if st.custom_order { tr("My order") } else { tr("Alphabetical") };
        self.stepper(y, &tr("Order"), &order, Field::Order);
        y += 27.0;
        let hint = tr("Your own colors: a theme.ini file beside the settings.");
        for line in rt_gfx::dialog::wrap_words(self.g, &hint, self.px(318.0), small, FontId::Regular).into_iter().take(2) {
            let r = self.rect(16.0, y + 6.0, 318.0, 16.0);
            self.g.text_in(&line, r, small, FontId::Regular, t.dim, Align::Left);
            y += 16.0;
        }
        let done = self.rect(234.0, 397.0, 100.0, 26.0);
        self.button(done, &tr("Done"), Hit::Done);
    }

    fn programs_page(&mut self) {
        let st = self.m.settings;
        let t = *self.t;
        self.heading(80.0, &tr("Programs from"));
        if cfg!(windows) {
            self.check(16.0, 102.0, 128.0, &tr("Scoop"), st.scoop, Hit::Scoop);
            self.check(148.0, 102.0, 186.0, &tr("Start menu"), st.start_menu, Hit::StartMenu);
        } else {
            self.check(16.0, 102.0, 318.0, &tr("Installed applications"), st.start_menu, Hit::StartMenu);
        }
        self.heading(130.0, &tr("My folders"));
        let mut y = 152.0;
        // Room for four; more are kept and read, only not listed.
        for (i, folder) in st.folders.iter().take(4).enumerate() {
            let r = self.rect(18.0, y, 178.0, 22.0);
            let size = self.px(12.0);
            let cut = ellipsize_middle(self.g, &folder.to_string_lossy(), r.w, size, FontId::Regular);
            self.g.text_in(&cut, r, size, FontId::Regular, t.text, Align::Left);
            let mode = self.rect(200.0, y, 106.0, 22.0);
            self.button(mode, &tr(st.mode_of(folder).name()), Hit::FolderMode(i));
            let x = self.rect(310.0, y, 24.0, 22.0);
            self.button(x, "×", Hit::RemoveFolder(i));
            y += 25.0;
        }
        let add = self.rect(16.0, y, 156.0, 26.0);
        self.button(add, &tr("Add a folder…"), Hit::AddFolder);
        let program = self.rect(178.0, y, 156.0, 26.0);
        self.button(program, &tr("Add a program…"), Hit::AddProgram);
        y += 32.0;
        let start = if cfg!(windows) { tr("Start with Windows") } else { tr("Start at login") };
        self.check(16.0, y, 188.0, &start, self.m.autostart, Hit::Autostart);
        let hidden = self.rect(210.0, y - 1.0, 124.0, 26.0);
        self.button(hidden, &tf("Hidden ({})", &[&self.m.app.hidden_count().to_string()]), Hit::ShowHidden);
        if cfg!(windows) {
            y += 23.0;
            self.check(16.0, y, 318.0, &tr("Always start as administrator"), st.always_admin, Hit::AlwaysAdmin);
        }
        y += 29.0;
        if cfg!(windows) {
            let keys = if st.hotkey == Hotkey::Off { tr("Off") } else { st.hotkey.label().to_string() };
            self.stepper(y, &tr("Keyboard shortcut"), &keys, Field::Hotkey);
            y += 27.0;
        }
        let language = st.language.map_or_else(|| tr("As the system"), |l: Lang| l.native_name().to_string());
        self.stepper(y, &tr("Language"), &language, Field::Language);
        y += 30.0;
        let updates = self.rect(16.0, y, 208.0, 26.0);
        self.button(updates, &tr("Check for updates"), Hit::CheckUpdates);
        let done = self.rect(234.0, y, 100.0, 26.0);
        self.button(done, &tr("Done"), Hit::Done);
    }
}

/// Draws the menu on `g`, at `s` times its logical size.
pub fn draw(g: &mut Gfx, m: &Model, t: &Theme, s: f32) -> Drawn {
    g.clear(t.frame);
    let mut p = Painter { g, t, m, s, out: Drawn::default() };
    p.header();
    // The options take the place of the column and of the drive too.
    let options = m.app.view == View::Options && m.app.query.is_empty();
    let mut area = if options { p.rect(6.0, 42.0, 338.0, 432.0) } else { p.rect(6.0, 42.0, 210.0, 394.0) };
    p.g.fill_rrect(area, p.px(7.0), t.list);
    if let Some(note) = m.note {
        let h = p.px(34.0);
        let r = area.take_bottom(h);
        let size = p.px(11.0);
        let mut y = r.y + p.px(1.0);
        for line in rt_gfx::dialog::wrap_words(p.g, note, r.w - p.px(20.0), size, FontId::Regular).into_iter().take(2) {
            let line_rect = Rect::new(r.x + p.px(10.0), y, r.w - p.px(20.0), p.px(15.0));
            p.g.text_in(&line, line_rect, size, FontId::Regular, t.warn, Align::Left);
            y += p.px(15.0);
        }
    }
    if options {
        p.options();
    } else {
        p.list(area);
        p.column();
        p.footer();
    }
    let edge = Rect::new(0.0, 0.0, p.g.width(), p.g.height());
    p.g.stroke_rrect(edge.inset(0.5), 0.0, 1.0, t.border);
    p.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::sample;

    #[test]
    fn rows_places_and_close_can_be_clicked() {
        let mut g = Gfx::new().unwrap();
        g.resize(SIZE.0 as u32, SIZE.1 as u32);
        let (app, settings, icons) = (sample(), Settings::default(), HashMap::new());
        let m = Model {
            app: &app,
            settings: &settings,
            icons: &icons,
            note: None,
            autostart: false,
            hot: None,
            disk: None,
            caret: true,
            pinned: false,
            elevated: false,
            accent: None,
            drop: None,
            rename: None,
        };
        let d = draw(&mut g, &m, &Theme::new(false, None, 0), 1.0);
        let hits: Vec<Hit> = d.hits.iter().map(|(_, h)| *h).collect();
        for want in [Hit::Row(0), Hit::Row(2), Hit::Place(Place::Explore), Hit::Options, Hit::Exit, Hit::Close] {
            assert!(hits.contains(&want), "{want:?}");
        }
        assert!(!hits.contains(&Hit::Row(3)));
        assert!(d.visible >= 10);
        let (r, h) = d.hits.iter().find(|(_, h)| *h == Hit::Row(1)).copied().unwrap();
        assert_eq!(d.hit(r.x + 5.0, r.y + 5.0), Some(h));
    }
}
