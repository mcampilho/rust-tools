//! Draws a widget into a picture with a transparent background (what is
//! not drawn shows the desktop), and says where its parts are, so a click
//! is tested against exactly what was drawn.

use super::calendar::{self, clock_text, day_month, long_date, month_grid, month_title, WEEKDAYS_SHORT};
use super::holidays::{self, Holiday};
use super::edit::{layout, line_of, Editor, Line};
use super::weather::{degrees, Forecast, Sky};
use super::{WKind, Widget, NOTE_COLORS, TEXT_COLORS, TEXT_SIZES};
use crate::i18n::tr;
use rt_gfx::dialog::{ellipsize, wrap_words};
use rt_gfx::{Align, Color, FontId, Gfx, Rect};
use rt_sys::LocalTime;
use tiny_skia::{FillRule, Mask, Paint, PathBuilder, Pattern, SpreadMode, Transform};

/// How the widgets look: the theme of rwall and the screen's scale.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub dark: bool,
    pub accent: Color,
    /// 1.0 at 96 dpi.
    pub s: f32,
}

/// A file or folder shown in a fence.
pub struct Item {
    pub name: String,
    pub icon: Option<tiny_skia::Pixmap>,
}

/// What a widget shows besides its settings.
pub struct View<'a> {
    pub now: LocalTime,
    /// The pointer is over the widget.
    pub hover: bool,
    pub hot: Option<Zone>,
    /// Writing in it (notes, annotations).
    pub editor: Option<&'a Editor>,
    /// The caret blinks.
    pub caret_on: bool,
    /// The month a calendar shows.
    pub month: (i32, u32),
    /// Calendars: the country whose holidays are shown; empty for none.
    pub country: &'a str,
    pub weather: Option<&'a Forecast>,
    /// A word instead of the content: "Looking for the weather…", an error.
    pub status: Option<&'a str>,
    pub photo: Option<&'a tiny_skia::Pixmap>,
    /// Name of the photo shown, on hover.
    pub caption: &'a str,
    pub items: &'a [Item],
    pub selected: Option<usize>,
    /// Scrolled down by (fences, notes), in pixels.
    pub scroll: f32,
}

impl View<'_> {
    pub fn at(now: LocalTime) -> View<'static> {
        View {
            now,
            hover: false,
            hot: None,
            editor: None,
            caret_on: true,
            month: (now.year, now.month),
            country: "",
            weather: None,
            status: None,
            photo: None,
            caption: "",
            items: &[],
            selected: None,
            scroll: 0.0,
        }
    }
}

/// The parts of a widget a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// Drag to move.
    Move,
    /// The corner to drag to resize.
    Resize,
    /// The text of a note or an annotation.
    Text,
    /// Calendar: the month before, the month after, back to today.
    Prev,
    Next,
    Today,
    /// A file of a fence.
    Item(usize),
    /// A fence's empty space.
    Blank,
    /// Calendars: a holiday (`y * 10000 + m * 100 + d`), to name it on hover.
    Day(u32),
}

/// The text of a note or annotation as drawn: enough to find the caret
/// position under the pointer.
#[derive(Clone, Debug, Default)]
pub struct TextBox {
    pub rect: Rect,
    pub px: f32,
    pub font: Option<FontId>,
    pub line_h: f32,
    pub lines: Vec<Line>,
    pub scroll: f32,
}

impl TextBox {
    /// The line at `y` (pixels of the widget).
    pub fn line_at(&self, y: f32) -> usize {
        let i = ((y - self.rect.y + self.scroll) / self.line_h).floor().max(0.0) as usize;
        i.min(self.lines.len().saturating_sub(1))
    }
}

/// Where the parts of a widget are.
#[derive(Default)]
pub struct Painted {
    pub zones: Vec<(Rect, Zone)>,
    pub text: Option<TextBox>,
    /// Height of what scrolls (fences, notes), and of the room it has.
    pub content_h: f32,
    pub room_h: f32,
    /// Fences: items per row and the height of a row (for the keyboard).
    pub columns: usize,
}

impl Painted {
    pub fn zone(&self, x: f32, y: f32) -> Option<Zone> {
        self.zones.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, z)| *z)
    }
}

/// Colours of the boxes.
#[derive(Clone, Copy)]
struct Pal {
    bg: Color,
    text: Color,
    dim: Color,
    faint: Color,
    border: Color,
    hover: Color,
    accent: Color,
    on_accent: Color,
}

fn pal(look: &Look) -> Pal {
    let a = look.accent;
    let on_accent = if (a.r as u32 * 299 + a.g as u32 * 587 + a.b as u32 * 114) / 1000 > 150 { Color::rgb(0x111111) } else { Color::rgb(0xFFFFFF) };
    if look.dark {
        Pal {
            bg: Color::rgba(0x1B1D22, 205),
            text: Color::rgb(0xF3F4F6),
            dim: Color::rgba(0xC9CDD4, 220),
            faint: Color::rgba(0xC9CDD4, 110),
            border: Color::rgba(0xFFFFFF, 34),
            hover: Color::rgba(0xFFFFFF, 26),
            accent: a,
            on_accent,
        }
    } else {
        Pal {
            bg: Color::rgba(0xF7F7F9, 222),
            text: Color::rgb(0x1C1D21),
            dim: Color::rgba(0x4A4F57, 230),
            faint: Color::rgba(0x4A4F57, 110),
            border: Color::rgba(0x000000, 30),
            hover: Color::rgba(0x000000, 18),
            accent: a,
            on_accent,
        }
    }
}

/// Room left around a box for its shadow.
pub fn margin(s: f32) -> f32 {
    (7.0 * s).round()
}

/// Draws widget `w` into `g` (already sized to it) and says where its parts are.
pub fn paint(g: &mut Gfx, w: &Widget, v: &View, look: &Look) -> Painted {
    g.clear(Color::TRANSPARENT);
    g.set_clip(None);
    let full = Rect::new(0.0, 0.0, g.width(), g.height());
    let mut p = Painted::default();
    p.zones.push((full, Zone::Move));
    match w.kind {
        WKind::Analog => analog(g, w, v, look, full),
        WKind::Digital => digital(g, w, v, look, full),
        WKind::Calendar => {
            let r = card(g, full, look);
            month(g, w, r.inset(12.0 * look.s), v, look, &mut p);
        }
        WKind::ClockCalendar => clock_calendar(g, w, v, look, full, &mut p),
        WKind::Note => note(g, w, v, look, full, &mut p),
        WKind::Annotation => annotation(g, w, v, look, full, &mut p),
        WKind::Weather => weather(g, w, v, look, full),
        WKind::Fence => fence(g, w, v, look, full, &mut p),
        WKind::Photos => photos(g, v, look, full),
    }
    if !w.locked && (v.hover || v.editor.is_some()) {
        let s = look.s;
        let m = if w.kind == WKind::Annotation { 0.0 } else { margin(s) };
        let grip = Rect::new(full.right() - m - 18.0 * s, full.bottom() - m - 18.0 * s, 18.0 * s, 18.0 * s);
        draw_grip(g, grip, w, look);
        p.zones.push((grip, Zone::Resize));
    }
    g.set_clip(None);
    p
}

// ---------------------------------------------------------------- pieces

fn rrect_path(r: Rect, rad: f32) -> Option<tiny_skia::Path> {
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
    let k = rad * 0.447_715;
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    let mut pb = PathBuilder::new();
    pb.move_to(x0 + rad, y0);
    pb.line_to(x1 - rad, y0);
    pb.cubic_to(x1 - k, y0, x1, y0 + k, x1, y0 + rad);
    pb.line_to(x1, y1 - rad);
    pb.cubic_to(x1, y1 - k, x1 - k, y1, x1 - rad, y1);
    pb.line_to(x0 + rad, y1);
    pb.cubic_to(x0 + k, y1, x0, y1 - k, x0, y1 - rad);
    pb.line_to(x0, y0 + rad);
    pb.cubic_to(x0, y0 + k, x0 + k, y0, x0 + rad, y0);
    pb.close();
    pb.finish()
}

/// A soft shadow under a rounded box.
fn shadow(g: &mut Gfx, r: Rect, rad: f32, s: f32) {
    let steps = 6;
    for i in (1..=steps).rev() {
        let d = i as f32 * s;
        let a = (14 - i * 2).max(2) as u8;
        g.fill_rrect(Rect::new(r.x - d * 0.6, r.y - d * 0.3, r.w + d * 1.2, r.h + d * 1.4), rad + d, Color::rgba(0x000000, a));
    }
}

/// The glassy box of most widgets; returns its inside.
fn card(g: &mut Gfx, full: Rect, look: &Look) -> Rect {
    let s = look.s;
    let c = pal(look);
    let r = full.inset(margin(s));
    let rad = 14.0 * s;
    shadow(g, r, rad, s);
    g.fill_rrect(r, rad, c.bg);
    g.stroke_rrect(r, rad, 1.0, c.border);
    r
}

fn draw_grip(g: &mut Gfx, r: Rect, w: &Widget, look: &Look) {
    let c = match w.kind {
        WKind::Note => Color::rgba(0x000000, 90),
        WKind::Annotation | WKind::Photos => Color::rgba(0xFFFFFF, 200),
        _ => pal(look).faint,
    };
    let s = look.s;
    for i in 0..3 {
        let o = (4.0 + i as f32 * 4.0) * s;
        g.polyline(&[(r.right() - 4.0 * s - o, r.bottom() - 4.0 * s), (r.right() - 4.0 * s, r.bottom() - 4.0 * s - o)], 1.3 * s, c, false);
    }
}

fn text_c(g: &mut Gfx, s: &str, r: Rect, px: f32, f: FontId, c: Color, align: Align) -> f32 {
    let s = ellipsize(g, s, r.w, px, f);
    g.text_in(&s, r, px, f, c, align)
}

/// The time, large, with the seconds (or AM/PM) smaller, centred in `r`.
fn big_time(g: &mut Gfx, w: &Widget, now: &LocalTime, r: Rect, c: Pal) {
    let (main, small) = clock_text(now, w.h24, w.seconds);
    let mut px = (r.h * 0.95).max(6.0);
    let ratio = 0.42;
    let width = |g: &mut Gfx, px: f32| g.measure(&main, px, FontId::Semibold) + g.measure(&small, px * ratio, FontId::Regular);
    let total = width(g, px);
    if total > r.w {
        px = (px * r.w / total).floor().max(6.0);
    }
    let total = width(g, px);
    let (asc, desc) = g.metrics(px, FontId::Semibold);
    let base = r.center_y() + (asc + desc) / 2.0;
    let x = r.x + (r.w - total) / 2.0;
    let mw = g.text(&main, x.round(), base, px, FontId::Semibold, c.text);
    g.text(&small, (x + mw).round(), base, px * ratio, FontId::Regular, c.dim);
}

// ---------------------------------------------------------------- clocks

fn analog(g: &mut Gfx, w: &Widget, v: &View, look: &Look, full: Rect) {
    let s = look.s;
    let c = pal(look);
    let (cx, cy) = (full.x + full.w / 2.0, full.y + full.h / 2.0);
    let r = (full.w.min(full.h) / 2.0 - margin(s)).max(10.0);
    for i in (1..=6).rev() {
        let d = i as f32 * s;
        g.fill_circle(cx, cy + d * 0.5, r + d, Color::rgba(0x000000, (14 - i * 2).max(2) as u8));
    }
    g.fill_circle(cx, cy, r, c.bg);
    // A thin ring.
    let ring: Vec<(f32, f32)> = (0..=96).map(|i| point(cx, cy, r - 0.5, i as f32 * 360.0 / 96.0)).collect();
    g.polyline(&ring, 1.0, c.border, false);
    for i in 0..60 {
        let hour = i % 5 == 0;
        let (len, width, col) = if hour { (0.11, 0.022, c.text) } else { (0.045, 0.009, c.faint) };
        let a = i as f32 * 6.0;
        g.polyline(&[point(cx, cy, r * (0.93 - len), a), point(cx, cy, r * 0.93, a)], (r * width).max(1.0), col, false);
    }
    if r >= 60.0 * s {
        let px = r * 0.15;
        for (n, a) in [("12", 0.0), ("3", 90.0), ("6", 180.0), ("9", 270.0)] {
            let (x, y) = point(cx, cy, r * 0.68, a);
            g.text_in(n, Rect::new(x - px, y - px, px * 2.0, px * 2.0), px, FontId::Semibold, c.dim, Align::Center);
        }
    }
    let t = &v.now;
    if r >= 70.0 * s {
        let wd = calendar::weekday(t.year, t.month, t.day);
        let label = format!("{} {}", tr(WEEKDAYS_SHORT[wd]), t.day);
        let px = r * 0.11;
        g.text_in(&label, Rect::new(cx - r * 0.5, cy + r * 0.3, r, px * 1.6), px, FontId::Regular, c.dim, Align::Center);
    }
    let sec = t.second as f32;
    let min = t.minute as f32 + sec / 60.0;
    let hour = (t.hour % 12) as f32 + min / 60.0;
    let hand = |g: &mut Gfx, angle: f32, len: f32, tail: f32, width: f32, col: Color| {
        g.polyline(&[point(cx, cy, -r * tail, angle), point(cx, cy, r * len, angle)], width, col, false);
    };
    // A faint shadow under the hands.
    let sh = Color::rgba(0x000000, 50);
    let off = 1.5 * s;
    g.polyline(&[point(cx + off, cy + off, 0.0, hour * 30.0), point(cx + off, cy + off, r * 0.5, hour * 30.0)], (r * 0.07).max(2.0), sh, false);
    g.polyline(&[point(cx + off, cy + off, 0.0, min * 6.0), point(cx + off, cy + off, r * 0.76, min * 6.0)], (r * 0.045).max(1.5), sh, false);
    hand(g, hour * 30.0, 0.5, 0.08, (r * 0.07).max(2.0), c.text);
    hand(g, min * 6.0, 0.76, 0.08, (r * 0.045).max(1.5), c.text);
    if w.seconds {
        hand(g, sec * 6.0, 0.84, 0.18, (r * 0.016).max(1.0), c.accent);
        g.fill_circle(cx, cy, (r * 0.045).max(2.0), c.accent);
    } else {
        g.fill_circle(cx, cy, (r * 0.045).max(2.0), c.text);
    }
}

/// The point at `len` from the centre, `deg` degrees clockwise from 12 o'clock.
fn point(cx: f32, cy: f32, len: f32, deg: f32) -> (f32, f32) {
    let a = deg.to_radians();
    (cx + len * a.sin(), cy - len * a.cos())
}

fn digital(g: &mut Gfx, w: &Widget, v: &View, look: &Look, full: Rect) {
    let s = look.s;
    let c = pal(look);
    let r = card(g, full, look).inset(14.0 * s);
    let date_h = (r.h * 0.2).clamp(12.0 * s, 26.0 * s);
    let time = Rect::new(r.x, r.y, r.w, r.h - date_h);
    big_time(g, w, &v.now, time, c);
    let date = long_date(&v.now);
    let px = (date_h * 0.72).min(16.0 * s);
    text_c(g, &date, Rect::new(r.x, time.bottom(), r.w, date_h), px, FontId::Regular, c.dim, Align::Center);
}

fn clock_calendar(g: &mut Gfx, w: &Widget, v: &View, look: &Look, full: Rect, p: &mut Painted) {
    let s = look.s;
    let c = pal(look);
    let r = card(g, full, look).inset(14.0 * s);
    let left = Rect::new(r.x, r.y, (r.w * 0.42).round(), r.h);
    let right = Rect::new(left.right() + 14.0 * s, r.y, r.right() - left.right() - 14.0 * s, r.h);
    g.fill_rect(Rect::new(left.right() + 7.0 * s, r.y + 6.0 * s, 1.0, r.h - 12.0 * s), c.border);
    // The time and the date, in the middle of the left side.
    let time_h = (left.h * 0.3).min(left.w * 0.36);
    let t = Rect::new(left.x, left.y + left.h * 0.5 - time_h * 0.85, left.w, time_h);
    big_time(g, w, &v.now, t, c);
    let px = (left.h * 0.075).clamp(11.0 * s, 16.0 * s);
    let date = long_date(&v.now);
    let mut y = t.bottom() + 6.0 * s;
    for l in wrap_words(g, &date, left.w, px, FontId::Regular).iter().take(2) {
        g.text_in(l, Rect::new(left.x, y, left.w, px * 1.4), px, FontId::Regular, c.dim, Align::Center);
        y += px * 1.4;
    }
    month(g, w, right, v, look, p);
}

// ---------------------------------------------------------------- calendar

/// Colour of the holidays' days.
fn holiday_color(look: &Look) -> Color {
    if look.dark {
        Color::rgb(0xFF8A80)
    } else {
        Color::rgb(0xC62828)
    }
}

/// A month's calendar in `r`, with arrows for the months before and after,
/// and the holidays (named under it: the one under the pointer, else the
/// next one).
fn month(g: &mut Gfx, w: &Widget, r: Rect, v: &View, look: &Look, p: &mut Painted) {
    let s = look.s;
    let c = pal(look);
    let (y, m) = v.month;
    let country = if w.holidays { v.country } else { "" };
    let hol: Vec<Holiday> = if country.is_empty() { Vec::new() } else { (y - 1..=y + 1).flat_map(|yy| holidays::of_year(country, yy)).collect() };
    let foot_h = if hol.is_empty() { 0.0 } else { (r.h * 0.1).clamp(14.0 * s, 22.0 * s) };
    let r = Rect::new(r.x, r.y, r.w, r.h - foot_h);
    let head_h = (r.h * 0.14).clamp(18.0 * s, 30.0 * s);
    let head = Rect::new(r.x, r.y, r.w, head_h);
    let ab = head_h;
    let next = Rect::new(head.right() - ab, head.y, ab, ab);
    let prev = Rect::new(next.x - ab - 2.0 * s, head.y, ab, ab);
    let title = Rect::new(head.x + 4.0 * s, head.y, prev.x - head.x - 8.0 * s, head_h);
    let px = (head_h * 0.56).min(16.0 * s);
    let showing_today = (y, m) == (v.now.year, v.now.month);
    if v.hot == Some(Zone::Today) && !showing_today {
        g.fill_rrect(title.inset(-2.0 * s), 5.0 * s, c.hover);
    }
    text_c(g, &month_title(y, m), title, px, FontId::Semibold, c.text, Align::Left);
    p.zones.push((title, Zone::Today));
    for (b, z, left) in [(prev, Zone::Prev, true), (next, Zone::Next, false)] {
        if v.hot == Some(z) {
            g.fill_rrect(b.inset(2.0 * s), 5.0 * s, c.hover);
        }
        let (mx, my, d) = (b.x + b.w / 2.0, b.y + b.h / 2.0, b.h * 0.16);
        let pts = if left { [(mx + d * 0.6, my - d), (mx - d * 0.6, my), (mx + d * 0.6, my + d)] } else { [(mx - d * 0.6, my - d), (mx + d * 0.6, my), (mx - d * 0.6, my + d)] };
        g.polyline(&pts, 1.6 * s, c.dim, false);
        p.zones.push((b, z));
    }
    let first = calendar::first_weekday();
    let grid = Rect::new(r.x, head.bottom() + 4.0 * s, r.w, r.h - head_h - 4.0 * s);
    let cw = grid.w / 7.0;
    let ch = grid.h / 7.0;
    let dpx = (ch * 0.48).min(cw * 0.42).min(15.0 * s);
    for i in 0..7 {
        let wd = (first + i) % 7;
        let cell = Rect::new(grid.x + i as f32 * cw, grid.y, cw, ch);
        g.text_in(&tr(WEEKDAYS_SHORT[wd]), cell, dpx * 0.85, FontId::Semibold, c.faint, Align::Center);
    }
    let hc = holiday_color(look);
    for (i, d) in month_grid(y, m, first).iter().enumerate() {
        let cell = Rect::new(grid.x + (i % 7) as f32 * cw, grid.y + (1 + i / 7) as f32 * ch, cw, ch);
        let today = d.y == v.now.year && d.m == v.now.month && d.d == v.now.day;
        let holiday = holidays::on(&hol, d.y, d.m, d.d).is_some();
        let key = d.y as u32 * 10000 + d.m * 100 + d.d;
        if holiday && d.inside {
            p.zones.push((cell, Zone::Day(key)));
            if v.hot == Some(Zone::Day(key)) && !today {
                g.fill_rrect(cell.inset(1.0 * s), 5.0 * s, c.hover);
            }
        }
        let col = if today {
            let rad = (cw.min(ch) * 0.44).max(4.0);
            g.fill_circle(cell.x + cw / 2.0, cell.y + ch / 2.0, rad, c.accent);
            c.on_accent
        } else if !d.inside {
            c.faint.with_alpha(c.faint.a / 2)
        } else if holiday {
            hc
        } else if d.weekday == 0 || d.weekday == 6 {
            c.dim
        } else {
            c.text
        };
        let bold = today || (holiday && d.inside);
        g.text_in(&d.d.to_string(), cell, dpx, if bold { FontId::Semibold } else { FontId::Regular }, col, Align::Center);
    }
    if foot_h > 0.0 {
        // The holiday under the pointer, else the next one from today (or
        // from the start of the month shown).
        let hovered = match v.hot {
            Some(Zone::Day(k)) => hol.iter().find(|h| h.y as u32 * 10000 + h.m * 100 + h.d == k).copied(),
            _ => None,
        };
        let from = if (y, m) == (v.now.year, v.now.month) { (v.now.year, v.now.month, v.now.day) } else { (y, m, 1) };
        let shown = hovered.or_else(|| holidays::next(country, from.0, from.1, from.2));
        if let Some(h) = shown {
            let when = if (h.y, h.m, h.d) == (v.now.year, v.now.month, v.now.day) { tr("Today") } else { day_month(h.m, h.d) };
            let text = format!("{when} · {}", tr(h.name));
            let foot = Rect::new(r.x + 4.0 * s, r.bottom(), r.w - 8.0 * s, foot_h);
            let px = (foot_h * 0.62).min(12.5 * s);
            g.fill_circle(foot.x + 3.0 * s, foot.center_y(), 3.0 * s, hc);
            text_c(g, &text, Rect::new(foot.x + 10.0 * s, foot.y, foot.w - 10.0 * s, foot.h), px, FontId::Regular, c.dim, Align::Left);
        }
    }
}

// ---------------------------------------------------------------- notes

/// Draws the text being written (or shown), wrapped, with the caret and the
/// selection; returns where it is.
#[allow(clippy::too_many_arguments)]
fn text_box(g: &mut Gfx, text: &str, editor: Option<&Editor>, caret_on: bool, r: Rect, px: f32, font: FontId, col: Color, shadow: Option<Color>, sel: Color, scroll: f32) -> TextBox {
    let shown = editor.map_or(text, |e| e.text.as_str());
    let lines = layout(g, shown, r.w, px, font);
    let line_h = (px * 1.36).round();
    g.set_clip(Some(r));
    let (asc, _) = g.metrics(px, font);
    for (n, l) in lines.iter().enumerate() {
        let y = r.y + n as f32 * line_h - scroll;
        if y + line_h < r.y || y > r.bottom() {
            continue;
        }
        let base = y + (line_h - px) / 2.0 + asc * 0.95;
        if let Some((a, b)) = editor.and_then(|e| e.selection()) {
            let (s0, s1) = (a.max(l.start), b.min(l.end));
            if s0 < s1 || (a <= l.end && b > l.end && l.end < shown.len()) {
                let x0 = g.measure(&shown[l.start..s0.max(l.start).min(l.end)], px, font);
                let x1 = g.measure(&shown[l.start..s1.max(l.start).min(l.end)], px, font) + if b > l.end { px * 0.3 } else { 0.0 };
                g.fill_rect(Rect::new(r.x + x0, y, (x1 - x0).max(1.0), line_h), sel);
            }
        }
        let piece = &shown[l.start..l.end];
        if let Some(sh) = shadow {
            let o = (px / 16.0).max(1.0);
            g.text(piece, r.x + o, base + o, px, font, sh);
            g.text(piece, r.x, base + o, px, font, sh.with_alpha(sh.a / 2));
        }
        g.text(piece, r.x, base, px, font, col);
    }
    if let Some(e) = editor.filter(|_| caret_on) {
        let n = line_of(&lines, e.caret);
        if let Some(l) = lines.get(n) {
            let x = r.x + g.measure(&shown[l.start..e.caret.clamp(l.start, l.end)], px, font);
            let y = r.y + n as f32 * line_h - scroll;
            g.fill_rect(Rect::new(x.round(), y + line_h * 0.12, (px / 14.0).max(1.0), line_h * 0.76), col);
        }
    }
    g.set_clip(None);
    TextBox { rect: r, px, font: Some(font), line_h, lines, scroll }
}

fn note(g: &mut Gfx, w: &Widget, v: &View, look: &Look, full: Rect, p: &mut Painted) {
    let s = look.s;
    let paper = Color::rgb(NOTE_COLORS[(w.color as usize).min(NOTE_COLORS.len() - 1)].1);
    let r = full.inset(margin(s));
    let rad = 6.0 * s;
    shadow(g, r, rad, s);
    g.fill_rrect(r, rad, paper);
    let bar = Rect::new(r.x, r.y, r.w, 22.0 * s);
    g.fill_rrect(bar, rad, paper.mix(Color::rgb(0x000000), 0.07));
    g.fill_rect(Rect::new(bar.x, bar.bottom() - rad, bar.w, rad), paper.mix(Color::rgb(0x000000), 0.07));
    // The folded corner.
    let f = 14.0 * s;
    let (x1, y1) = (r.right(), r.bottom());
    g.fill_polygon(&[(x1 - f, y1), (x1 - f, y1 - f), (x1, y1 - f)], paper.mix(Color::rgb(0x000000), 0.12));
    let ink = Color::rgb(0x23242A);
    let area = Rect::new(r.x + 12.0 * s, bar.bottom() + 8.0 * s, r.w - 24.0 * s, r.h - bar.h - 16.0 * s);
    let px = 15.0 * s;
    if w.text.is_empty() && v.editor.is_none_or(|e| e.text.is_empty()) && !v.editor.is_some_and(|_| v.caret_on) {
        g.text_in(&tr("Write a note…"), Rect::new(area.x, area.y, area.w, px * 1.4), px, FontId::Italic, ink.with_alpha(120), Align::Left);
    }
    let tb = text_box(g, &w.text, v.editor, v.caret_on, area, px, FontId::Regular, ink, None, Color::rgba(0x000000, 40), v.scroll);
    p.content_h = tb.lines.len() as f32 * tb.line_h;
    p.room_h = area.h;
    p.zones.push((Rect::new(r.x, bar.bottom(), r.w, r.bottom() - bar.bottom()), Zone::Text));
    p.text = Some(tb);
}

fn annotation(g: &mut Gfx, w: &Widget, v: &View, look: &Look, full: Rect, p: &mut Painted) {
    let s = look.s;
    // Almost transparent, but the whole box takes the pointer.
    g.fill_rect(full, Color::rgba(0x000000, 1));
    let editing = v.editor.is_some();
    if v.hover || editing {
        g.stroke_rrect(full, 6.0 * s, 1.0, Color::rgba(0xFFFFFF, if editing { 150 } else { 90 }));
        g.stroke_rrect(full.inset(1.0), 6.0 * s, 1.0, Color::rgba(0x000000, if editing { 110 } else { 60 }));
    }
    let col = Color::rgb(TEXT_COLORS[(w.color as usize).min(TEXT_COLORS.len() - 1)].1);
    let light = (col.r as u32 + col.g as u32 + col.b as u32) > 3 * 110;
    let sh = if light { Color::rgba(0x000000, 170) } else { Color::rgba(0xFFFFFF, 150) };
    let px = TEXT_SIZES[(w.size as usize).min(TEXT_SIZES.len() - 1)].1 * s;
    let area = full.inset(6.0 * s);
    if w.text.is_empty() && !editing {
        g.text_in(&tr("Write here"), Rect::new(area.x, area.y, area.w, px * 1.36), px, FontId::Italic, col.with_alpha(150), Align::Left);
    }
    let tb = text_box(g, &w.text, v.editor, v.caret_on, area, px, FontId::Semibold, col, Some(sh), Color::rgba(0x3388FF, 90), v.scroll);
    p.content_h = tb.lines.len() as f32 * tb.line_h;
    p.room_h = area.h;
    if editing {
        p.zones.push((full, Zone::Text));
    }
    p.text = Some(tb);
}

// ---------------------------------------------------------------- weather

fn weather(g: &mut Gfx, w: &Widget, v: &View, look: &Look, full: Rect) {
    let s = look.s;
    let c = pal(look);
    let r = card(g, full, look).inset(14.0 * s);
    let place = if w.has_place() { w.place.clone() } else if w.city.trim().is_empty() { tr("Weather") } else { w.city.trim().to_string() };
    let head_px = (r.h * 0.085).clamp(11.0 * s, 15.0 * s);
    text_c(g, &place, Rect::new(r.x, r.y, r.w, head_px * 1.5), head_px, FontId::Semibold, c.dim, Align::Left);
    let body = Rect::new(r.x, r.y + head_px * 1.6, r.w, r.h - head_px * 1.6);
    let Some(f) = v.weather else {
        let msg = v.status.map(str::to_string).unwrap_or_else(|| tr("Type a city in the Widgets tab of rwall."));
        let px = 12.5 * s;
        let lines = wrap_words(g, &msg, body.w, px, FontId::Regular);
        let lh = px * 1.4;
        let mut y = body.y + (body.h - lines.len() as f32 * lh) / 2.0;
        for l in lines.iter().take(4) {
            g.text_in(l, Rect::new(body.x, y, body.w, lh), px, FontId::Regular, c.dim, Align::Center);
            y += lh;
        }
        return;
    };
    let rows = f.days.len().saturating_sub(1).min(super::weather::DAYS);
    let foot_h = if rows > 0 && body.h > 120.0 * s { (body.h * 0.38).min(64.0 * s) } else { 0.0 };
    let main = Rect::new(body.x, body.y, body.w, body.h - foot_h);
    let icon = (main.h * 0.92).min(main.w * 0.3);
    sky_icon(g, f.sky, f.day, main.x + icon / 2.0, main.y + main.h / 2.0, icon, look);
    let tx = main.x + icon + 10.0 * s;
    let tw = main.right() - tx;
    let tpx = (main.h * 0.5).min(tw * 0.42).max(8.0);
    let temp = degrees(f.temp);
    let (asc, desc) = g.metrics(tpx, FontId::Semibold);
    let base = main.y + main.h * 0.47 + (asc + desc) / 2.0;
    let used = g.text(&temp, tx, base, tpx, FontId::Semibold, c.text);
    let side = Rect::new(tx + used + 8.0 * s, main.y, (tw - used - 8.0 * s).max(0.0), main.h);
    let spx = (main.h * 0.15).clamp(10.0 * s, 15.0 * s);
    let lines = wrap_words(g, &tr(f.sky.label()), side.w, spx, FontId::Regular);
    let mut y = main.y + main.h * 0.47 - spx * 1.4;
    for l in lines.iter().take(2) {
        g.text_in(l, Rect::new(side.x, y, side.w, spx * 1.4), spx, FontId::Regular, c.text, Align::Left);
        y += spx * 1.4;
    }
    if let Some(today) = f.days.first() {
        let hl = format!("↑{}  ↓{}", degrees(today.max), degrees(today.min));
        text_c(g, &hl, Rect::new(side.x, y, side.w, spx * 1.4), spx * 0.95, FontId::Regular, c.dim, Align::Left);
    }
    if foot_h > 0.0 {
        let foot = Rect::new(body.x, main.bottom(), body.w, foot_h);
        g.fill_rect(Rect::new(foot.x, foot.y, foot.w, 1.0), c.border);
        let cw = foot.w / rows as f32;
        let fpx = (foot_h * 0.2).clamp(9.5 * s, 13.0 * s);
        for (i, d) in f.days.iter().skip(1).take(rows).enumerate() {
            let cell = Rect::new(foot.x + i as f32 * cw, foot.y + 4.0 * s, cw, foot_h - 4.0 * s);
            let wd = calendar::weekday(d.y, d.m, d.d);
            let ih = (cell.h - fpx * 2.6).max(8.0).min(cw * 0.4);
            g.text_in(&tr(WEEKDAYS_SHORT[wd]), Rect::new(cell.x, cell.y, cell.w, fpx * 1.3), fpx, FontId::Semibold, c.dim, Align::Center);
            sky_icon(g, d.sky, true, cell.x + cell.w / 2.0, cell.y + fpx * 1.3 + ih / 2.0, ih, look);
            let t = format!("{} {}", degrees(d.max), degrees(d.min));
            text_c(g, &t, Rect::new(cell.x, cell.bottom() - fpx * 1.3, cell.w, fpx * 1.3), fpx, FontId::Regular, c.text, Align::Center);
        }
    }
}

const SUN: Color = Color::rgb(0xFFC23D);
const RAIN: Color = Color::rgb(0x5AAEFF);

fn sun(g: &mut Gfx, cx: f32, cy: f32, r: f32) {
    for i in 0..8 {
        let a = i as f32 * 45.0;
        g.polyline(&[point(cx, cy, r * 1.35, a), point(cx, cy, r * 1.75, a)], (r * 0.22).max(1.0), SUN, false);
    }
    g.fill_circle(cx, cy, r, SUN);
}

fn moon(g: &mut Gfx, cx: f32, cy: f32, r: f32) {
    let (w, h) = (g.pix.width(), g.pix.height());
    let (Some(outer), Some(inner)) = (PathBuilder::from_circle(cx, cy, r), PathBuilder::from_circle(cx + r * 0.55, cy - r * 0.35, r * 0.85)) else { return };
    let Some(mut mask) = Mask::new(w, h) else { return };
    mask.fill_path(&inner, FillRule::Winding, true, Transform::identity());
    mask.invert();
    let mut paint = Paint::default();
    paint.set_color_rgba8(0xF1, 0xE6, 0xB8, 255);
    paint.anti_alias = true;
    g.pix.fill_path(&outer, &paint, FillRule::Winding, Transform::identity(), Some(&mask));
}

/// A cloud whose bottom middle is at (`cx`, `by`), `w` wide.
fn cloud(g: &mut Gfx, cx: f32, by: f32, w: f32, col: Color) {
    let h = w * 0.36;
    g.fill_rrect(Rect::new(cx - w / 2.0, by - h, w, h), h / 2.0, col);
    g.fill_circle(cx - w * 0.16, by - h * 0.95, w * 0.22, col);
    g.fill_circle(cx + w * 0.12, by - h * 1.05, w * 0.28, col);
}

/// The picture of the sky, `size` across, centred at (`cx`, `cy`).
pub fn sky_icon(g: &mut Gfx, sky: Sky, day: bool, cx: f32, cy: f32, size: f32, look: &Look) {
    let u = size / 2.0;
    let light = if look.dark { Color::rgb(0xE6EBF2) } else { Color::rgb(0xC9D1DC) };
    let grey = if look.dark { Color::rgb(0xA8B0BC) } else { Color::rgb(0x9AA3AF) };
    let dark_cloud = if look.dark { Color::rgb(0x7D8592) } else { Color::rgb(0x737C89) };
    let body = |g: &mut Gfx| {
        if day {
            sun(g, cx, cy, u * 0.42);
        } else {
            moon(g, cx, cy, u * 0.55);
        }
    };
    match sky {
        Sky::Clear => body(g),
        Sky::MostlyClear | Sky::PartlyCloudy => {
            if day {
                sun(g, cx - u * 0.22, cy - u * 0.22, u * 0.36);
            } else {
                moon(g, cx - u * 0.2, cy - u * 0.25, u * 0.45);
            }
            let w = if sky == Sky::MostlyClear { u * 1.05 } else { u * 1.45 };
            cloud(g, cx + u * 0.22, cy + u * 0.6, w, light);
        }
        Sky::Overcast => {
            cloud(g, cx - u * 0.25, cy + u * 0.2, u * 1.2, grey);
            cloud(g, cx + u * 0.1, cy + u * 0.55, u * 1.6, light);
        }
        Sky::Fog => {
            for i in 0..4 {
                let y = cy - u * 0.5 + i as f32 * u * 0.33;
                let (x0, x1) = if i % 2 == 0 { (cx - u * 0.8, cx + u * 0.6) } else { (cx - u * 0.6, cx + u * 0.8) };
                g.polyline(&[(x0, y), (x1, y)], (u * 0.14).max(1.5), grey, false);
            }
        }
        Sky::Drizzle | Sky::Rain | Sky::Showers | Sky::Snow | Sky::Thunder => {
            if sky == Sky::Showers {
                if day {
                    sun(g, cx - u * 0.3, cy - u * 0.45, u * 0.3);
                } else {
                    moon(g, cx - u * 0.3, cy - u * 0.45, u * 0.38);
                }
            }
            let col = if sky == Sky::Thunder { dark_cloud } else { light };
            cloud(g, cx, cy + u * 0.15, u * 1.6, col);
            let top = cy + u * 0.3;
            match sky {
                Sky::Snow => {
                    for (i, x) in [-0.5f32, 0.0, 0.5].iter().enumerate() {
                        let y = top + u * if i == 1 { 0.45 } else { 0.25 };
                        g.fill_circle(cx + x * u, y, (u * 0.09).max(1.2), Color::rgb(0xFFFFFF));
                        g.fill_circle(cx + x * u, y, (u * 0.09).max(1.2) - 0.8, Color::rgb(0xDDEBFF));
                    }
                }
                Sky::Drizzle => {
                    for x in [-0.45f32, 0.0, 0.45] {
                        g.fill_circle(cx + x * u, top + u * 0.3, (u * 0.065).max(1.0), RAIN);
                    }
                }
                Sky::Thunder => {
                    let (x, y) = (cx, top - u * 0.05);
                    g.fill_polygon(&[(x + u * 0.08, y), (x - u * 0.22, y + u * 0.42), (x - u * 0.02, y + u * 0.42), (x - u * 0.12, y + u * 0.75), (x + u * 0.24, y + u * 0.28), (x + u * 0.04, y + u * 0.28)], SUN);
                }
                _ => {
                    for x in [-0.45f32, 0.0, 0.45] {
                        let (x0, y0) = (cx + x * u, top + u * 0.12);
                        g.polyline(&[(x0, y0), (x0 - u * 0.12, y0 + u * 0.42)], (u * 0.09).max(1.2), RAIN, false);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- fences

/// Size of a fence's cell, at 100 % scale.
pub const CELL: (f32, f32) = (86.0, 86.0);

fn fence(g: &mut Gfx, w: &Widget, v: &View, look: &Look, full: Rect, p: &mut Painted) {
    let s = look.s;
    let c = pal(look);
    let r = card(g, full, look);
    let bar = Rect::new(r.x, r.y, r.w, 32.0 * s);
    let name = w.folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| tr("Fence"));
    let count = if w.folder.as_os_str().is_empty() { String::new() } else { v.items.len().to_string() };
    let cw = g.measure(&count, 11.5 * s, FontId::Regular) + 4.0 * s;
    text_c(g, &name, Rect::new(bar.x + 12.0 * s, bar.y, bar.w - 30.0 * s - cw, bar.h), 13.0 * s, FontId::Semibold, c.text, Align::Left);
    g.text_in(&count, Rect::new(bar.right() - 12.0 * s - cw, bar.y, cw, bar.h), 11.5 * s, FontId::Regular, c.faint, Align::Right);
    g.fill_rect(Rect::new(r.x + 10.0 * s, bar.bottom(), r.w - 20.0 * s, 1.0), c.border);
    let area = Rect::new(r.x + 6.0 * s, bar.bottom() + 4.0 * s, r.w - 12.0 * s, r.bottom() - bar.bottom() - 8.0 * s);
    p.zones.push((area, Zone::Blank));
    let msg = if w.folder.as_os_str().is_empty() {
        Some(tr("Choose a folder in the Widgets tab of rwall."))
    } else if let Some(st) = v.status {
        Some(st.to_string())
    } else if v.items.is_empty() {
        Some(tr("Drag files here."))
    } else {
        None
    };
    if let Some(msg) = msg {
        let px = 12.0 * s;
        let lines = wrap_words(g, &msg, area.w - 16.0 * s, px, FontId::Regular);
        let lh = px * 1.4;
        let mut y = area.y + (area.h - lines.len() as f32 * lh) / 2.0;
        for l in lines.iter().take(4) {
            g.text_in(l, Rect::new(area.x, y, area.w, lh), px, FontId::Italic, c.faint, Align::Center);
            y += lh;
        }
        return;
    }
    let (cw, ch) = (CELL.0 * s, CELL.1 * s);
    let cols = ((area.w / cw).floor() as usize).max(1);
    let cw = area.w / cols as f32;
    let rows = v.items.len().div_ceil(cols);
    p.columns = cols;
    p.content_h = rows as f32 * ch;
    p.room_h = area.h;
    g.set_clip(Some(area));
    let icon = 40.0 * s;
    let px = 11.5 * s;
    for (i, it) in v.items.iter().enumerate() {
        let cell = Rect::new(area.x + (i % cols) as f32 * cw, area.y + (i / cols) as f32 * ch - v.scroll, cw, ch);
        if cell.bottom() < area.y || cell.y > area.bottom() {
            continue;
        }
        let box_ = cell.inset(2.0 * s);
        if v.selected == Some(i) {
            g.fill_rrect(box_, 6.0 * s, c.accent.with_alpha(70));
            g.stroke_rrect(box_, 6.0 * s, 1.0, c.accent.with_alpha(150));
        } else if v.hot == Some(Zone::Item(i)) {
            g.fill_rrect(box_, 6.0 * s, c.hover);
        }
        let ir = Rect::new(cell.x + (cw - icon) / 2.0, cell.y + 6.0 * s, icon, icon);
        match &it.icon {
            Some(img) => {
                let k = (icon / img.width() as f32).min(icon / img.height() as f32);
                let (iw, ih) = (img.width() as f32 * k, img.height() as f32 * k);
                g.draw_image(img, Rect::new(ir.x + (icon - iw) / 2.0, ir.y + (icon - ih) / 2.0, iw, ih));
            }
            None => {
                g.fill_rrect(ir.inset(6.0 * s), 4.0 * s, c.faint.with_alpha(60));
            }
        }
        let lines = wrap_words(g, &it.name, cw - 8.0 * s, px, FontId::Regular);
        let mut ty = ir.bottom() + 4.0 * s;
        for (n, l) in lines.iter().take(2).enumerate() {
            let l = if n == 1 && lines.len() > 2 { ellipsize(g, &format!("{l}…"), cw - 8.0 * s, px, FontId::Regular) } else { l.clone() };
            g.text_in(&l, Rect::new(cell.x + 4.0 * s, ty, cw - 8.0 * s, px * 1.3), px, FontId::Regular, c.text, Align::Center);
            ty += px * 1.3;
        }
        let vis = cell.intersect(&area);
        if vis.w > 0.0 && vis.h > 0.0 {
            p.zones.push((vis, Zone::Item(i)));
        }
    }
    g.set_clip(None);
    // A scroll bar when it does not all fit.
    if p.content_h > area.h {
        let track = Rect::new(area.right() - 3.0 * s, area.y + 2.0 * s, 3.0 * s, area.h - 4.0 * s);
        let k = area.h / p.content_h;
        let th = (track.h * k).max(16.0 * s);
        let ty = track.y + (track.h - th) * (v.scroll / (p.content_h - area.h)).clamp(0.0, 1.0);
        g.fill_rrect(Rect::new(track.x, ty, track.w, th), 1.5 * s, c.faint);
    }
}

// ---------------------------------------------------------------- photos

fn photos(g: &mut Gfx, v: &View, look: &Look, full: Rect) {
    let s = look.s;
    let c = pal(look);
    let r = full.inset(margin(s));
    let rad = 12.0 * s;
    let Some(img) = v.photo else {
        let r = card(g, full, look);
        let msg = v.status.map(str::to_string).unwrap_or_else(|| tr("Choose a folder in the Widgets tab of rwall."));
        let px = 12.0 * s;
        let lines = wrap_words(g, &msg, r.w - 24.0 * s, px, FontId::Regular);
        let lh = px * 1.4;
        let mut y = r.y + (r.h - lines.len() as f32 * lh) / 2.0;
        for l in lines.iter().take(4) {
            g.text_in(l, Rect::new(r.x, y, r.w, lh), px, FontId::Italic, c.faint, Align::Center);
            y += lh;
        }
        return;
    };
    shadow(g, r, rad, s);
    // The photo covers the box, cut where it sticks out.
    let k = (r.w / img.width() as f32).max(r.h / img.height() as f32);
    let (iw, ih) = (img.width() as f32 * k, img.height() as f32 * k);
    let t = Transform::from_row(k, 0.0, 0.0, k, r.x + (r.w - iw) / 2.0, r.y + (r.h - ih) / 2.0);
    if let Some(path) = rrect_path(r, rad) {
        let mut paint = Paint { shader: Pattern::new(img.as_ref(), SpreadMode::Pad, tiny_skia::FilterQuality::Bicubic, 1.0, t), ..Paint::default() };
        paint.anti_alias = true;
        g.pix.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }
    g.stroke_rrect(r, rad, 1.0, Color::rgba(0xFFFFFF, 40));
    if v.hover && !v.caption.is_empty() {
        let band = Rect::new(r.x, r.bottom() - 30.0 * s, r.w, 30.0 * s);
        // Rounded at the bottom like the photo: a taller rounded box, cut at the top.
        g.set_clip(Some(band));
        g.fill_rrect(Rect::new(band.x, band.y - rad, band.w, band.h + rad), rad, Color::rgba(0x000000, 120));
        g.set_clip(None);
        text_c(g, v.caption, Rect::new(band.x + 12.0 * s, band.y, band.w - 40.0 * s, band.h), 11.5 * s, FontId::Regular, Color::rgb(0xFFFFFF), Align::Left);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::weather::Day;

    fn now() -> LocalTime {
        LocalTime { year: 2026, month: 11, day: 12, hour: 10, minute: 9, second: 30 }
    }

    fn forecast() -> Forecast {
        let d = |d, sky, max, min| Day { y: 2026, m: 11, d, sky, max, min };
        Forecast {
            temp: 17.6,
            sky: Sky::PartlyCloudy,
            day: true,
            days: vec![d(12, Sky::PartlyCloudy, 19.1, 11.0), d(13, Sky::Rain, 16.0, 10.4), d(14, Sky::Clear, 18.2, 9.8), d(15, Sky::Thunder, 15.5, 8.0)],
        }
    }

    fn placed(kind: WKind) -> Widget {
        let mut w = Widget::new(1, kind);
        (w.w, w.h) = kind.default_size();
        w
    }

    /// Every kind draws something, and its parts can be found where drawn.
    #[test]
    fn every_kind_draws() {
        let Ok(mut g) = Gfx::new() else { return };
        let look = Look { dark: true, accent: Color::rgb(0x3B82F6), s: 1.0 };
        let fc = forecast();
        let items: Vec<Item> = (0..12).map(|i| Item { name: format!("Ficheiro {i}.txt"), icon: None }).collect();
        for kind in WKind::ALL {
            let mut w = placed(kind);
            w.folder = "C:\\Pasta".into();
            w.text = "Olá".into();
            g.resize(w.w as u32, w.h as u32);
            let v = View { weather: Some(&fc), items: &items, hover: true, ..View::at(now()) };
            let p = paint(&mut g, &w, &v, &look);
            assert!(g.pix.pixels().iter().filter(|px| px.alpha() > 0).count() > 500, "{kind:?} drew little");
            assert_eq!(p.zone(1.0, 1.0), Some(Zone::Move), "{kind:?}");
            assert_eq!(p.zone(w.w as f32 - 12.0, w.h as f32 - 12.0), Some(Zone::Resize), "{kind:?}");
            match kind {
                WKind::Calendar | WKind::ClockCalendar => assert!(p.zones.iter().any(|(_, z)| *z == Zone::Next)),
                WKind::Note => assert_eq!(p.zone(w.w as f32 / 2.0, w.h as f32 / 2.0), Some(Zone::Text)),
                WKind::Fence => {
                    assert!(p.columns >= 3 && p.content_h > p.room_h);
                    assert_eq!(p.zone(30.0, 60.0), Some(Zone::Item(0)));
                }
                _ => {}
            }
        }
    }

    /// The widgets as PNG files in `target/screens` (run on request:
    /// `cargo test -p rwall --lib widget_pictures -- --ignored`).
    #[test]
    #[ignore]
    fn widget_pictures() {
        use crate::i18n::{set_lang, Lang};
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&out).unwrap();
        let mut g = Gfx::new().unwrap();
        let fc = forecast();
        let photo = {
            let mut pm = tiny_skia::Pixmap::new(400, 300).unwrap();
            for (i, px) in pm.pixels_mut().iter_mut().enumerate() {
                let (x, y) = ((i % 400) as u32, (i / 400) as u32);
                let ground = y > 190 + ((x as f32 / 25.0).sin() * 14.0) as u32;
                *px = if ground { tiny_skia::ColorU8::from_rgba(40, 110, 70, 255) } else { tiny_skia::ColorU8::from_rgba(250 - (y / 2) as u8, 150, 90 + (y / 3) as u8, 255) }.premultiply();
            }
            pm
        };
        let items: Vec<Item> = ["Relatório.docx", "Orçamento 2026.xlsx", "Fotos", "notas.txt", "Apresentação final do projeto.pptx", "Instalador.exe", "Música"]
            .iter()
            .map(|n| Item { name: n.to_string(), icon: None })
            .collect();
        let mut editor = Editor::new("Comprar pão\nLigar ao João às 15h");
        editor.anchor = Some(0);
        editor.caret = 7;
        for (lang, dark, scale, tag) in [(Lang::PtPT, true, 1.0, "pt-dark"), (Lang::EnUS, false, 1.0, "en-light"), (Lang::DeDE, true, 1.5, "de-150")] {
            set_lang(lang);
            let look = Look { dark, accent: Color::rgb(0x2F7DE1), s: scale };
            // All of them on a picture of a desktop, as on the screen.
            let (bw, bh) = ((1180.0 * scale) as u32, (740.0 * scale) as u32);
            let mut board = tiny_skia::Pixmap::new(bw, bh).unwrap();
            for (i, px) in board.pixels_mut().iter_mut().enumerate() {
                let (x, y) = ((i as u32 % bw) as f32 / bw as f32, (i as u32 / bw) as f32 / bh as f32);
                *px = tiny_skia::ColorU8::from_rgba((20.0 + 60.0 * x) as u8, (60.0 + 80.0 * y) as u8, (110.0 + 90.0 * (1.0 - x)) as u8, 255).premultiply();
            }
            let spots: [(WKind, f32, f32); 9] = [
                (WKind::Analog, 20.0, 20.0),
                (WKind::Digital, 230.0, 20.0),
                (WKind::Calendar, 550.0, 20.0),
                (WKind::Weather, 830.0, 20.0),
                (WKind::ClockCalendar, 20.0, 280.0),
                (WKind::Note, 510.0, 280.0),
                (WKind::Photos, 760.0, 280.0),
                (WKind::Fence, 20.0, 520.0),
                (WKind::Annotation, 420.0, 540.0),
            ];
            for (kind, x, y) in spots {
                let mut w = placed(kind);
                w.w = (w.w as f32 * scale) as i32;
                w.h = (w.h as f32 * scale) as i32;
                w.place = "Lisboa, Portugal".into();
                w.lat = 38.7;
                w.folder = "C:\\Users\\Me\\Desktop\\Trabalho".into();
                w.text = if kind == WKind::Annotation { "Backups às sextas!".into() } else { String::new() };
                w.seconds = true;
                g.resize(w.w as u32, w.h as u32);
                let v = View {
                    weather: Some(&fc),
                    items: &items,
                    photo: Some(&photo),
                    caption: "Pôr do sol.jpg",
                    hover: matches!(kind, WKind::Photos | WKind::Fence),
                    hot: match kind {
                        WKind::Fence => Some(Zone::Item(1)),
                        WKind::Calendar => Some(Zone::Day(20261101)),
                        _ => None,
                    },
                    country: match lang {
                        Lang::PtPT => "PT",
                        Lang::DeDE => "DE",
                        _ => "US",
                    },
                    selected: if kind == WKind::Fence { Some(3) } else { None },
                    editor: if kind == WKind::Note { Some(&editor) } else { None },
                    month: (2026, 11),
                    ..View::at(now())
                };
                paint(&mut g, &w, &v, &look);
                board.draw_pixmap((x * scale) as i32, (y * scale) as i32, g.pix.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
            }
            board.save_png(out.join(format!("widgets-{tag}.png"))).unwrap();
        }
        // Each sky, by day and by night.
        let look = Look { dark: true, accent: Color::rgb(0x2F7DE1), s: 1.0 };
        g.resize(10 * 70, 140);
        g.clear(Color::rgb(0x2A2E36));
        for (i, sky) in Sky::ALL.iter().enumerate() {
            sky_icon(&mut g, *sky, true, 35.0 + i as f32 * 70.0, 35.0, 56.0, &look);
            sky_icon(&mut g, *sky, false, 35.0 + i as f32 * 70.0, 105.0, 56.0, &look);
        }
        g.pix.save_png(out.join("widgets-skies.png")).unwrap();
        set_lang(Lang::EnUS);
    }
}
