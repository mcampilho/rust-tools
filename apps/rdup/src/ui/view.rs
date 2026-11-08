//! The three pages of the window, drawn each frame:
//!
//! - the start page: the drives, each with a box to tick, the folders added,
//!   and the buttons to add a folder and to search;
//! - the search page: the step the search is in and how far it got;
//! - the result: the groups of copies, largest waste first, each with a
//!   title row and a row per copy.
//!
//! [`layout`] decides where things go; `main` uses the same [`Layout`] to
//! find what the pointer is on.

use super::gfx::{Align, Color, FontId, Gfx, Rect};
use super::theme::Theme;
use crate::app::{App, Calendar, Row, View};
use crate::folders::SHARES;
use crate::i18n::Lang;
use crate::filter::{Bad, Field, Why, KINDS};
use crate::format;
use crate::i18n::{tf, tr};
use crate::search::{Found, Stage};
use crate::settings::MIN_SIZES;
use rt_core::i18n::strip_mnemonic;
use rt_gfx::dialog::{ellipsize, ellipsize_middle};
use rt_scan::Method;
use std::path::Path;

/// What the pointer is on, for hover effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Hot {
    #[default]
    None,
    /// A drive on the start page.
    Card(usize),
    /// The button that takes a folder off the list.
    Remove(usize),
    /// A folder on the list, to tick or untick.
    Folder(usize),
    /// A choice of the smallest file size (index into `MIN_SIZES`).
    MinSize(usize),
    /// A filter box.
    Field(Field),
    /// A kind of file to tick (index into `KINDS`).
    Kind(usize),
    /// Look for duplicate files (0) or folders (1).
    Compare(usize),
    /// The button that empties the filters.
    Clear,
    /// The calendar button of a date box.
    Pick(Field),
    /// In the calendar: the month before, after, a day (index into
    /// `CalLayout::days`), today, no date, and elsewhere on it.
    CalPrev,
    CalNext,
    CalDay(usize),
    CalToday,
    CalClear,
    CalPanel,
    Add,
    Find,
    Stop,
    Row(usize),
    /// Above the result: the files or the folders (0, 1), and a least share
    /// in common of the folders (index into `folders::SHARES`).
    Tab(usize),
    Share(usize),
    /// Under the result: the rules to mark copies, unmark all, move the
    /// marked ones to a folder, to the Recycle Bin.
    Rules,
    Unmark,
    MoveTo,
    Recycle,
}

/// Column positions of the list, as (x, width); a width of 0 hides a column.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cols {
    pub name: (f32, f32),
    pub folder: (f32, f32),
    pub size: (f32, f32),
    pub date: (f32, f32),
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub full: Rect,
    /// The drawn menu bar (not on Windows).
    pub bar: Rect,
    pub status: Rect,
    pub body: Rect,
    // Result page.
    /// The bar above the list, with the tabs and the shares of the folders.
    pub tools: Rect,
    pub tabs: Vec<Rect>,
    pub shares: Vec<Rect>,
    /// The buttons under the list (in `actions`).
    pub rules: Rect,
    pub unmark: Rect,
    pub move_to: Rect,
    pub recycle: Rect,
    pub header: Rect,
    pub list: Rect,
    pub row_h: f32,
    pub cols: Cols,
    pub track: Rect,
    // Start page.
    pub cards: Vec<Rect>,
    /// The folders added, and their remove buttons.
    pub folders: Vec<Rect>,
    pub removes: Vec<Rect>,
    pub folders_title_y: f32,
    /// The filters: the title, the button that empties them, the labels of
    /// the rows (size, date, kinds, extensions, names), the boxes (by
    /// `Field::index`), the kinds of file, and the line that explains them.
    pub filters_title_y: f32,
    pub clear: Rect,
    pub row_labels: Vec<Rect>,
    pub fields: Vec<Rect>,
    pub kinds: Vec<Rect>,
    /// The ticks for duplicate files and duplicate folders.
    pub compare: Vec<Rect>,
    pub help: Rect,
    /// The choices of the smallest file size.
    pub sizes: Vec<Rect>,
    /// The bar at the bottom with the buttons, which does not scroll.
    pub actions: Rect,
    pub add: Rect,
    pub find: Rect,
    pub title_y: f32,
    pub content_x: f32,
    pub content_w: f32,
    /// How far the start page can scroll, when it does not fit.
    pub start_max: f32,
    /// The calendar, when one is open.
    pub cal: Option<CalLayout>,
    // Search page.
    pub stop: Rect,
}

/// Where the parts of an open calendar go.
#[derive(Clone, Debug, Default)]
pub struct CalLayout {
    pub panel: Rect,
    pub title: Rect,
    pub prev: Rect,
    pub next: Rect,
    /// The names of the days of the week, in the order shown.
    pub heads: Vec<(Rect, usize)>,
    /// Six weeks of days: where, the date, and whether it is in the month shown.
    pub days: Vec<(Rect, (i32, u32, u32), bool)>,
    pub today: Rect,
    pub clear: Rect,
}

impl CalLayout {
    /// What the pointer is on, when it is on the calendar.
    pub fn hot_at(&self, x: f32, y: f32) -> Option<Hot> {
        if !self.panel.contains(x, y) {
            return None;
        }
        Some(if self.prev.contains(x, y) {
            Hot::CalPrev
        } else if self.next.contains(x, y) {
            Hot::CalNext
        } else if self.today.contains(x, y) {
            Hot::CalToday
        } else if self.clear.contains(x, y) {
            Hot::CalClear
        } else if let Some(i) = self.days.iter().position(|(r, _, _)| r.contains(x, y)) {
            Hot::CalDay(i)
        } else {
            Hot::CalPanel
        })
    }
}

/// Keys of the days of the week, from Sunday.
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
/// Keys of the months.
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// Whether weeks start on Sunday in the interface language (else on Monday).
fn week_from_sunday() -> bool {
    matches!(rt_core::i18n::lang(), Lang::EnUS | Lang::JaJP | Lang::PtBR)
}

/// Lays out the calendar of `cal` under (or over) its box.
fn layout_calendar(cal: &Calendar, field: Rect, full: Rect, s: f32) -> CalLayout {
    use rt_scan::format::{civil, days_from_civil};
    let cell = (34.0 * s).round();
    let pad = (10.0 * s).round();
    let (head_h, names_h, foot_h) = ((36.0 * s).round(), (22.0 * s).round(), (40.0 * s).round());
    let w = 7.0 * cell + 2.0 * pad;
    let h = pad + head_h + names_h + 6.0 * cell + foot_h + pad / 2.0;
    let x = field.x.min(full.right() - w - 8.0 * s).max(full.x + 4.0 * s);
    let below = field.bottom() + 4.0 * s;
    let y = if below + h <= full.bottom() - 4.0 * s { below } else { (field.y - h - 4.0 * s).max(full.y + 4.0 * s) };
    let panel = Rect::new(x.round(), y.round(), w, h.round());
    let mut c = CalLayout { panel, ..CalLayout::default() };
    let top = panel.y + pad;
    let b = (28.0 * s).round();
    c.prev = Rect::new(panel.x + pad, top + (head_h - b) / 2.0, b, b);
    c.next = Rect::new(panel.right() - pad - b, top + (head_h - b) / 2.0, b, b);
    c.title = Rect::new(c.prev.right(), top, c.next.x - c.prev.right(), head_h);
    let first = if week_from_sunday() { 0 } else { 1 };
    let gx = panel.x + pad;
    for i in 0..7 {
        c.heads.push((Rect::new(gx + i as f32 * cell, top + head_h, cell, names_h), (first + i) % 7));
    }
    let d0 = days_from_civil(cal.year, cal.month, 1);
    // 1970-01-01 was a Thursday (4, from Sunday).
    let weekday = (d0 + 4).rem_euclid(7);
    let start = d0 - (weekday - first as i64).rem_euclid(7);
    let gy = top + head_h + names_h;
    for i in 0..42 {
        let (yy, mm, dd) = civil((start + i) as i32);
        let r = Rect::new(gx + (i % 7) as f32 * cell, gy + (i / 7) as f32 * cell, cell, cell);
        c.days.push((r, (yy, mm, dd), mm == cal.month));
    }
    let fy = gy + 6.0 * cell + (foot_h - b) / 2.0;
    let half = ((w - 2.0 * pad - 8.0 * s) / 2.0).floor();
    c.today = Rect::new(panel.x + pad, fy, half, b);
    c.clear = Rect::new(panel.right() - pad - half, fy, half, b);
    c
}

/// The month and year at the top of a calendar.
pub fn month_title(year: i32, month: u32) -> String {
    tr("{month} {year}").replace("{month}", &tr(MONTHS[(month as usize + 11) % 12])).replace("{year}", &year.to_string())
}

/// Draws the open calendar over the page.
fn draw_calendar(g: &mut Gfx, app: &App, c: &CalLayout, th: &Theme, s: f32, hot: Hot) {
    let Some(cal) = app.calendar else { return };
    let p = px(s);
    let r = (8.0 * s).round();
    for i in 1..=4 {
        let d = i as f32 * s;
        let sh = Rect::new(c.panel.x - d, c.panel.y - d + 2.0 * s, c.panel.w + 2.0 * d, c.panel.h + 2.0 * d);
        g.fill_rrect(sh, r + d, Color::rgba(0x000000, 0x14));
    }
    g.fill_rrect(c.panel, r, th.bg);
    g.stroke_rrect(c.panel, r, 1.0, th.border);
    g.text_in(&month_title(cal.year, cal.month), c.title, 14.0 * s, FontId::Semibold, th.text, Align::Center);
    for (b, hot_here, back) in [(c.prev, hot == Hot::CalPrev, true), (c.next, hot == Hot::CalNext, false)] {
        if hot_here {
            g.fill_rrect(b, 4.0 * s, th.hover);
        }
        let (cx, cy, a) = (b.x + b.w / 2.0, b.center_y(), 4.0 * s);
        let pts = if back { [(cx + a / 2.0, cy - a), (cx - a / 2.0, cy), (cx + a / 2.0, cy + a)] } else { [(cx - a / 2.0, cy - a), (cx + a / 2.0, cy), (cx - a / 2.0, cy + a)] };
        g.polyline(&pts, 1.6 * s, th.text, false);
    }
    for (r, wd) in &c.heads {
        g.text_in(&tr(WEEKDAYS[*wd]), *r, p * 0.9, FontId::Regular, th.dim, Align::Center);
    }
    let t = rt_sys::local_time();
    let today = (t.year, t.month, t.day);
    let chosen = crate::filter::parse_day(app.choice.text(cal.field));
    for (i, (r, date, inside)) in c.days.iter().enumerate() {
        let cell = r.inset(2.0 * s);
        let on = chosen == Some(*date);
        if on {
            g.fill_rrect(cell, cell.h / 2.0, th.accent);
        } else if hot == Hot::CalDay(i) {
            g.fill_rrect(cell, cell.h / 2.0, th.hover);
        }
        if *date == today && !on {
            g.stroke_rrect(cell, cell.h / 2.0, 1.2 * s, th.accent);
        }
        let fg = match (on, th.dark) {
            (true, true) => Color::rgb(0x000000),
            (true, false) => Color::rgb(0xFFFFFF),
            _ if *inside => th.text,
            _ => th.dim,
        };
        g.text_in(&date.2.to_string(), *r, p, FontId::Regular, fg, Align::Center);
    }
    button(g, c.today, &tr("Today"), th, s, hot == Hot::CalToday, Look::Secondary);
    button(g, c.clear, &tr("No Date"), th, s, hot == Hot::CalClear, Look::Secondary);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Start,
    Searching,
    Result,
}

pub fn page(app: &App) -> Page {
    if app.searching.is_some() {
        Page::Searching
    } else if app.show_start || !app.has_result() {
        Page::Start
    } else {
        Page::Result
    }
}

impl Layout {
    /// The list row under `y`, if any.
    pub fn row_at(&self, app: &App, x: f32, y: f32) -> Option<usize> {
        if !self.list.contains(x, y) || self.row_h <= 0.0 {
            return None;
        }
        let i = app.scroll + ((y - self.list.y) / self.row_h) as usize;
        (i < app.rows.len()).then_some(i)
    }

    /// Rows that fit in the list.
    pub fn page_rows(&self) -> usize {
        if self.row_h <= 0.0 {
            return 1;
        }
        ((self.list.h / self.row_h).floor() as usize).max(1)
    }
}

/// Where everything goes in a window of `full` at scale `s`.
pub fn layout(app: &App, full: Rect, s: f32, menu_bar: bool) -> Layout {
    let mut l = Layout { full, ..Layout::default() };
    let mut area = full;
    if menu_bar {
        l.bar = area.take_top((28.0 * s).round());
    }
    l.status = area.take_bottom((24.0 * s).round());
    l.body = area;
    match page(app) {
        Page::Start => {
            l.actions = area.take_bottom((60.0 * s).round());
            let n = app.volumes.as_ref().map_or(0, |v| v.len());
            let folders = app.folders().len();
            let (card_h, gap) = ((58.0 * s).round(), (10.0 * s).round());
            let (row_h, title_h) = ((34.0 * s).round(), (34.0 * s).round());
            // Wide windows: the drives in two columns.
            let wide = area.w >= 760.0 * s;
            let cols = if wide && n > 1 { 2 } else { 1 };
            let w = ((if wide { 1000.0 } else { 720.0 }) * s).min(area.w - 48.0 * s).max(200.0 * s);
            let x = (area.x + (area.w - w) / 2.0).round();
            let rows = n.div_ceil(cols);
            let empty = if n == 0 { 40.0 * s } else { 0.0 };
            // Laid out from the top, then moved up by the scroll.
            let top = area.y;
            let above = 96.0 * s;
            l.title_y = (top + 46.0 * s).round();
            l.content_x = x;
            l.content_w = w;
            let card_w = ((w - gap * (cols as f32 - 1.0)) / cols as f32).floor();
            for i in 0..n {
                let (c, r) = (i % cols, i / cols);
                let y = top + above + r as f32 * (card_h + gap);
                l.cards.push(Rect::new(x + c as f32 * (card_w + gap), y.round(), card_w, card_h));
            }
            let mut y = top + above + rows as f32 * (card_h + gap) + empty;
            if folders > 0 {
                l.folders_title_y = (y + 22.0 * s).round();
                y += title_h;
                for _ in 0..folders {
                    let r = Rect::new(x, y.round(), w, row_h);
                    let b = (24.0 * s).round();
                    l.removes.push(Rect::new(r.right() - b - 6.0 * s, r.y + (r.h - b) / 2.0, b, b));
                    l.folders.push(r);
                    y += row_h + 4.0 * s;
                }
            }
            let y = layout_filters(&mut l, x, y, w, s);
            l.start_max = (y + 16.0 * s - area.bottom()).max(0.0);
            l.shift_start(-app.start_scroll.clamp(0.0, l.start_max).round());
            let a = l.actions;
            let bh = (34.0 * s).round();
            let by = (a.y + (a.h - bh) / 2.0).round();
            l.add = Rect::new(x, by, (180.0 * s).round(), bh);
            l.find = Rect::new(l.add.right() + 10.0 * s, by, (220.0 * s).round(), bh);
            if let Some(cal) = &app.calendar {
                l.cal = Some(layout_calendar(cal, l.fields[cal.field.index()], full, s));
            }
        }
        Page::Searching => {
            let w = (160.0 * s).round();
            l.stop = Rect::new((area.x + (area.w - w) / 2.0).round(), (area.y + area.h * 0.5 + 130.0 * s).round(), w, (34.0 * s).round());
        }
        Page::Result => {
            let mut top = area;
            // Only the files: nothing to choose.
            if app.found.as_ref().is_some_and(|f| !f.groups.is_empty()) {
                l.actions = top.take_bottom((56.0 * s).round());
                let a = l.actions;
                let (bh, gap) = ((32.0 * s).round(), (8.0 * s).round());
                let by = (a.y + (a.h - bh) / 2.0).round();
                let w = |want: f32| (want * s).round().min(((a.w - 24.0 * s - 3.0 * gap) / 4.0).floor());
                l.rules = Rect::new(a.x + 12.0 * s, by, w(160.0), bh);
                l.unmark = Rect::new(l.rules.right() + gap, by, w(130.0), bh);
                l.recycle = Rect::new(a.right() - 12.0 * s - w(210.0), by, w(210.0), bh);
                l.move_to = Rect::new(l.recycle.x - gap - w(130.0), by, w(130.0), bh);
            }
            if app.found.as_ref().is_some_and(|f| !f.groups.is_empty() && f.look[1]) {
                let look = app.found.as_ref().map_or([true; 2], |f| f.look);
                l.tools = top.take_top((44.0 * s).round());
                let t = l.tools;
                let (h, gap) = ((28.0 * s).round(), (6.0 * s).round());
                let y = (t.y + (t.h - h) / 2.0).round();
                let tab_w = (150.0 * s).round().min(((t.w - 24.0 * s) / 2.0).floor());
                let mut x = t.x + 12.0 * s;
                for shown in look {
                    if shown {
                        l.tabs.push(Rect::new(x, y, tab_w, h));
                        x += tab_w + gap;
                    } else {
                        l.tabs.push(Rect::default());
                    }
                }
                if app.view == View::Folders {
                    let n = SHARES.len() as f32;
                    let room = t.right() - 12.0 * s - (x + 18.0 * s);
                    let w = (110.0 * s).min(((room - gap * (n - 1.0)) / n).floor()).max(0.0);
                    let mut x = t.right() - 12.0 * s - n * w - (n - 1.0) * gap;
                    for _ in SHARES {
                        l.shares.push(Rect::new(x.round(), y, w, h));
                        x += w + gap;
                    }
                }
            }
            l.header = top.take_top((26.0 * s).round());
            l.row_h = (22.0 * s).round();
            l.track = top.take_right((10.0 * s).round());
            l.list = top;
            // Columns from the right; the name and the folder share the rest.
            let mut x1 = top.right();
            let mut col = |want: f32, min_rest: f32| {
                let w = (want * s).round();
                if x1 - w - top.x >= min_rest * s {
                    x1 -= w;
                    (x1, w)
                } else {
                    (x1, 0.0)
                }
            };
            l.cols.date = col(100.0, 420.0);
            l.cols.size = col(90.0, 200.0);
            let rest = x1 - top.x;
            let name_w = if rest > 500.0 * s { (rest * 0.4).round() } else { rest };
            l.cols.name = (top.x, name_w);
            l.cols.folder = (top.x + name_w, rest - name_w);
        }
    }
    l
}

/// Lays out the filters from `y` down, in a column at `x` of `w`. Returns
/// where they end.
fn layout_filters(l: &mut Layout, x: f32, mut y: f32, w: f32, s: f32) -> f32 {
    y += 20.0 * s;
    l.filters_title_y = (y + 18.0 * s).round();
    let cw = (150.0 * s).round();
    l.clear = Rect::new(x + w - cw, y.round(), cw, (28.0 * s).round());
    y += 42.0 * s;
    let (lw, fh, gap) = ((110.0 * s).round(), (30.0 * s).round(), (10.0 * s).round());
    let (fx, fw) = (x + lw, w - lw);
    let half = ((fw - gap) / 2.0).floor();
    let small = (180.0 * s).round().min(half);
    l.fields = vec![Rect::default(); Field::ALL.len()];
    // What to look for: duplicate files, duplicate folders.
    l.row_labels.push(Rect::new(x, y.round(), lw, fh));
    let item = (210.0 * s).round().min(half);
    for i in 0..2 {
        l.compare.push(Rect::new(fx + i as f32 * (item + gap), y.round(), item - 4.0 * s, fh));
    }
    y += fh + gap;
    let pair = |l: &mut Layout, y: f32, a: Field, b: Field, bw: f32| {
        l.row_labels.push(Rect::new(x, y.round(), lw, fh));
        l.fields[a.index()] = Rect::new(fx, y.round(), bw, fh);
        l.fields[b.index()] = Rect::new(fx + bw + gap, y.round(), bw, fh);
    };
    // Sizes, with the usual smallest sizes beside them (or below, when narrow).
    pair(l, y, Field::MinSize, Field::MaxSize, small);
    let chip = (68.0 * s).round();
    let mut cx = fx + 2.0 * small + 2.0 * gap;
    if cx + MIN_SIZES.len() as f32 * (chip + 4.0 * s) > x + w {
        y += fh + 6.0 * s;
        cx = fx;
    }
    for _ in MIN_SIZES {
        l.sizes.push(Rect::new(cx.round(), (y + 2.0 * s).round(), chip, fh - (4.0 * s).round()));
        cx += chip + 4.0 * s;
    }
    y += fh + gap;
    pair(l, y, Field::From, Field::To, small);
    y += fh + gap;
    // The kinds of file, in as many rows as they need.
    l.row_labels.push(Rect::new(x, y.round(), lw, fh));
    let item = (126.0 * s).round();
    let per_row = ((fw / item).floor() as usize).max(1);
    for i in 0..KINDS.len() {
        l.kinds.push(Rect::new(fx + (i % per_row) as f32 * item, (y + (i / per_row) as f32 * fh).round(), item - 4.0 * s, fh));
    }
    y += KINDS.len().div_ceil(per_row) as f32 * fh + gap;
    pair(l, y, Field::OnlyExt, Field::SkipExt, half);
    y += fh + gap;
    pair(l, y, Field::OnlyNames, Field::SkipNames, half);
    y += fh + 4.0 * s;
    l.help = Rect::new(fx, y.round(), fw, (20.0 * s).round());
    y + 24.0 * s
}

impl Layout {
    /// Moves the scrolling part of the start page by `dy`.
    fn shift_start(&mut self, dy: f32) {
        let mv = |r: &mut Rect| r.y += dy;
        self.title_y += dy;
        self.folders_title_y += dy;
        self.filters_title_y += dy;
        for r in self.cards.iter_mut().chain(&mut self.folders).chain(&mut self.removes).chain(&mut self.row_labels) {
            mv(r);
        }
        for r in self.fields.iter_mut().chain(&mut self.kinds).chain(&mut self.sizes).chain(&mut self.compare) {
            mv(r);
        }
        mv(&mut self.clear);
        mv(&mut self.help);
    }
}

/// Draws the current page.
pub fn draw(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    g.clear(th.bg);
    match page(app) {
        Page::Start => draw_start(g, app, l, th, s, hot),
        Page::Searching => draw_searching(g, app, l, th, s, hot),
        Page::Result => draw_result(g, app, l, th, s, hot),
    }
    draw_status(g, app, l, th, s, hot);
    if let (Page::Start, Some(c)) = (page(app), &l.cal) {
        draw_calendar(g, app, c, th, s, hot);
    }
}

fn px(s: f32) -> f32 {
    12.5 * s
}

/// How a button looks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    /// Filled with the accent colour: the main action.
    Primary,
    /// Outlined.
    Secondary,
    /// Greyed out: cannot be used now.
    Disabled,
}

fn button(g: &mut Gfx, r: Rect, label: &str, th: &Theme, s: f32, hot: bool, look: Look) {
    let rad = (5.0 * s).round();
    match look {
        Look::Disabled => {
            g.fill_rrect(r, rad, th.panel);
            g.stroke_rrect(r, rad, 1.0, th.border);
            g.text_in(label, r, 13.0 * s, FontId::Semibold, th.dim, Align::Center);
        }
        Look::Primary => {
            let bg = if hot { th.accent } else { th.accent.mix(th.bg, 0.12) };
            g.fill_rrect(r, rad, bg);
            let fg = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
            g.text_in(label, r, 13.0 * s, FontId::Semibold, fg, Align::Center);
        }
        Look::Secondary => {
            g.fill_rrect(r, rad, if hot { th.panel.mix(th.accent, 0.10) } else { th.panel });
            g.stroke_rrect(r, rad, 1.0, if hot { th.accent } else { th.border });
            g.text_in(label, r, 13.0 * s, FontId::Semibold, th.text, Align::Center);
        }
    }
}

/// A rounded choice: filled when `on`, greyed when `dim`.
#[allow(clippy::too_many_arguments)]
fn chip(g: &mut Gfx, r: Rect, label: &str, th: &Theme, s: f32, on: bool, hot: bool, dim: bool) {
    let rad = (r.h / 2.0).round();
    if on {
        g.fill_rrect(r, rad, th.accent);
    } else {
        g.fill_rrect(r, rad, if hot { th.panel.mix(th.accent, 0.10) } else { th.panel });
        g.stroke_rrect(r, rad, 1.0, if hot { th.accent } else { th.border });
    }
    let fg = match (on, th.dark) {
        (true, true) => Color::rgb(0x000000),
        (true, false) => Color::rgb(0xFFFFFF),
        _ if dim => th.dim,
        _ => th.text,
    };
    let p = px(s) * 0.95;
    let text = ellipsize(g, label, r.w - 8.0 * s, p, FontId::Regular);
    g.text_in(&text, r, p, FontId::Regular, fg, Align::Center);
}

/// A box to tick, ticked or not.
fn check_box(g: &mut Gfx, x: f32, cy: f32, th: &Theme, s: f32, on: bool) {
    let b = (18.0 * s).round();
    let r = Rect::new(x, (cy - b / 2.0).round(), b, b);
    if on {
        g.fill_rrect(r, 4.0 * s, th.accent);
        let fg = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
        let pts = [(r.x + b * 0.24, r.y + b * 0.52), (r.x + b * 0.43, r.y + b * 0.70), (r.x + b * 0.77, r.y + b * 0.32)];
        g.polyline(&pts, 2.0 * s, fg, false);
    } else {
        g.fill_rrect(r, 4.0 * s, th.bg);
        g.stroke_rrect(r, 4.0 * s, 1.5 * s, th.dim);
    }
}

fn folder_icon(g: &mut Gfx, x: f32, cy: f32, s: f32) {
    let icon = (14.0 * s).round();
    let c = Color::rgb(0xE8B84A);
    g.fill_rrect(Rect::new(x, cy - icon * 0.45, icon * 0.5, icon * 0.3), 1.5 * s, c);
    g.fill_rrect(Rect::new(x, cy - icon * 0.3, icon, icon * 0.72), 2.0 * s, c);
}

fn draw_start(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let (x, w) = (l.content_x, l.content_w);
    let title_y = l.title_y;
    g.set_clip(Some(l.body));
    g.text(&tr("Choose the drives and folders to search"), x, title_y, 20.0 * s, FontId::Semibold, th.text);
    let sub = tr("Files with the same contents are found in all of them together.");
    let sub = ellipsize(g, &sub, w, px(s), FontId::Regular);
    g.text(&sub, x, title_y + 26.0 * s, px(s), FontId::Regular, th.dim);
    match &app.volumes {
        None => {
            g.text(&tr("Looking for drives…"), x, title_y + 64.0 * s, px(s), FontId::Regular, th.dim);
        }
        Some(vols) => {
            for (i, (r, v)) in l.cards.iter().zip(vols).enumerate() {
                let hover = hot == Hot::Card(i);
                let on = app.is_chosen(&v.path);
                let bg = if on { th.panel.mix(th.accent, 0.10) } else { th.panel };
                g.fill_rrect(*r, (6.0 * s).round(), if hover { bg.mix(th.accent, 0.06) } else { bg });
                g.stroke_rrect(*r, (6.0 * s).round(), 1.0, if hover || on { th.accent } else { th.border });
                let pad = 14.0 * s;
                check_box(g, r.x + pad, r.center_y(), th, s, on);
                let tx = r.x + pad + 30.0 * s;
                let left_w = r.w * 0.5 - (tx - r.x);
                let (asc, _) = g.metrics(15.0 * s, FontId::Semibold);
                let y1 = r.y + 10.0 * s + asc;
                let name = ellipsize(g, &v.path.to_string_lossy(), left_w, 15.0 * s, FontId::Semibold);
                let nw = g.text(&name, tx, y1, 15.0 * s, FontId::Semibold, th.text);
                if !v.label.is_empty() && nw + 40.0 * s < left_w {
                    let label = ellipsize(g, &v.label, left_w - nw - 10.0 * s, px(s), FontId::Regular);
                    g.text(&label, tx + nw + 10.0 * s, y1, px(s), FontId::Regular, th.text);
                }
                let mut kind = v.fs.clone();
                if v.remote {
                    kind = if kind.is_empty() { tr("Network") } else { format!("{kind} · {}", tr("Network")) };
                }
                g.text(&kind, tx, r.bottom() - 11.0 * s, px(s) * 0.95, FontId::Regular, th.dim);
                // Used space.
                let bar = Rect::new(r.x + r.w * 0.5, r.y + 12.0 * s, r.w * 0.5 - pad, (8.0 * s).round());
                g.fill_rrect(bar, bar.h / 2.0, th.bar_track);
                let frac = if v.total > 0 { v.used() as f32 / v.total as f32 } else { 0.0 };
                if frac > 0.0 {
                    g.fill_rrect(Rect::new(bar.x, bar.y, (bar.w * frac).max(bar.h), bar.h), bar.h / 2.0, th.bar);
                }
                let text = tf("{} used of {}", &[&format::size(v.used()), &format::size(v.total)]);
                g.text_in(&text, Rect::new(bar.x, r.bottom() - 24.0 * s, bar.w, 16.0 * s), px(s) * 0.95, FontId::Regular, th.dim, Align::Right);
            }
            if vols.is_empty() {
                g.text(&tr("No drives were found."), x, title_y + 64.0 * s, px(s), FontId::Regular, th.dim);
            }
        }
    }
    if !l.folders.is_empty() {
        g.text(&tr("Folders"), x, l.folders_title_y, 14.0 * s, FontId::Semibold, th.text);
        for (i, (r, path)) in l.folders.iter().zip(app.folders()).enumerate() {
            let on = app.is_chosen(&path);
            let hover = hot == Hot::Folder(i);
            let bg = match (on, hover) {
                (true, _) => th.panel.mix(th.accent, 0.10),
                (false, true) => th.panel.mix(th.accent, 0.05),
                (false, false) => th.panel,
            };
            g.fill_rrect(*r, (6.0 * s).round(), bg);
            g.stroke_rrect(*r, (6.0 * s).round(), 1.0, if on || hover { th.accent } else { th.border });
            check_box(g, r.x + 12.0 * s, r.center_y(), th, s, on);
            folder_icon(g, r.x + 40.0 * s, r.center_y(), s);
            let rm = l.removes[i];
            let tx = r.x + 62.0 * s;
            let shown = ellipsize_middle(g, &path.to_string_lossy(), rm.x - tx - 8.0 * s, px(s), FontId::Regular);
            g.text_in(&shown, Rect::new(tx, r.y, rm.x - tx, r.h), px(s), FontId::Regular, if on { th.text } else { th.dim }, Align::Left);
            if hot == Hot::Remove(i) {
                g.fill_rrect(rm, 4.0 * s, th.hover);
            }
            let (cx, cy, a) = (rm.x + rm.w / 2.0, rm.center_y(), 4.5 * s);
            let c = if hot == Hot::Remove(i) { th.text } else { th.dim };
            g.polyline(&[(cx - a, cy - a), (cx + a, cy + a)], 1.5 * s, c, false);
            g.polyline(&[(cx - a, cy + a), (cx + a, cy - a)], 1.5 * s, c, false);
        }
    }
    draw_filters(g, app, l, th, s, hot);
    g.set_clip(None);
    draw_actions(g, app, l, th, s, hot);
}

/// The placeholder of a filter box: what it is for, with an example.
fn placeholder(f: Field) -> String {
    match f {
        Field::MinSize => tr("At least, e.g. 1 MB"),
        Field::MaxSize => tr("At most, e.g. 4 GB"),
        Field::From => tr("From, e.g. 2024-01-31"),
        Field::To => tr("To, e.g. 2024-12-31"),
        Field::OnlyExt => tr("Only these, e.g. jpg png mp4"),
        Field::SkipExt => tr("Leave out, e.g. tmp log obj"),
        Field::OnlyNames => tr("Only these, e.g. Setup* *video*"),
        Field::SkipNames => tr("Leave out, e.g. tmp* *cache*"),
    }
}

/// Why the filters cannot be used, as shown.
pub fn bad_text(b: Bad) -> String {
    match b.why {
        Why::Size => tr("A size is not understood: use a number and a unit, such as 500 KB or 1.5 GB."),
        Why::Date => tr("A date is not understood: use year-month-day, such as 2024-01-31."),
        Why::SizeOrder => tr("The largest size is smaller than the smallest."),
        Why::DateOrder => tr("The last date is before the first."),
        Why::Both(w) => tf("{} is both to keep and to leave out: take it out of one of the boxes.", &[&w]),
    }
}

/// How far the text of a box is moved left so that the caret stays in view.
fn field_shift(g: &mut Gfx, text: &str, caret: usize, room: f32, px: f32) -> f32 {
    let at = g.measure(&text[..caret.min(text.len())], px, FontId::Regular);
    (at - room + 2.0).max(0.0)
}

/// The inside of a filter box, where its text goes (beside the calendar
/// button of a date box).
fn field_inner(r: Rect, f: Field, s: f32) -> Rect {
    let button = if is_date(f) { pick_button(r, s).w } else { 0.0 };
    Rect::new(r.x + 8.0 * s, r.y, (r.w - 16.0 * s - button).max(0.0), r.h)
}

fn is_date(f: Field) -> bool {
    matches!(f, Field::From | Field::To)
}

/// The button at the right of a date box that opens the calendar.
pub fn pick_button(r: Rect, s: f32) -> Rect {
    let b = r.h - (6.0 * s).round();
    Rect::new(r.right() - b - 3.0 * s, r.y + 3.0 * s, b, b)
}

/// The caret position (a byte offset) for a click at `x` in the box `f`.
pub fn caret_at(g: &mut Gfx, app: &App, l: &Layout, f: Field, x: f32, s: f32) -> usize {
    let Some(&r) = l.fields.get(f.index()) else { return 0 };
    let text = app.choice.text(f);
    let inner = field_inner(r, f, s);
    let shift = if app.focus == Some(f) { field_shift(g, text, app.caret, inner.w, px(s)) } else { 0.0 };
    let want = x - inner.x + shift;
    let mut best = (0, want.abs());
    let mut w = 0.0;
    for (i, c) in text.char_indices() {
        w += g.advance(c, px(s), FontId::Regular);
        let end = i + c.len_utf8();
        if (want - w).abs() < best.1 {
            best = (end, (want - w).abs());
        }
    }
    best.0
}

/// A small calendar drawn in `r`.
fn calendar_icon(g: &mut Gfx, r: Rect, s: f32, c: Color) {
    let w = (14.0 * s).round();
    let b = Rect::new((r.x + (r.w - w) / 2.0).round(), (r.y + (r.h - w) / 2.0 + s).round(), w, (w * 0.9).round());
    g.stroke_rrect(b, 2.0 * s, 1.3 * s, c);
    g.fill_rect(Rect::new(b.x, b.y, b.w, (3.5 * s).round()), c);
    for i in 0..2 {
        let x = b.x + b.w * (0.3 + 0.4 * i as f32) - 0.75 * s;
        g.fill_rect(Rect::new(x.round(), b.y - 2.0 * s, (1.5 * s).max(1.0), 3.0 * s), c);
    }
    for (cx, cy) in [(0.3, 0.55), (0.55, 0.55), (0.8, 0.55), (0.3, 0.8), (0.55, 0.8)] {
        g.fill_rect(Rect::new((b.x + b.w * cx - s).round(), (b.y + b.h * cy - s).round(), (2.0 * s).max(1.0), (2.0 * s).max(1.0)), c);
    }
}

fn draw_field(g: &mut Gfx, app: &App, l: &Layout, f: Field, th: &Theme, s: f32, hot_now: Hot) {
    let hot = hot_now == Hot::Field(f);
    let bad = app.choice.filter().err().is_some_and(|b| b.marks(f));
    let r = l.fields[f.index()];
    let focused = app.focus == Some(f);
    let rad = (4.0 * s).round();
    g.fill_rrect(r, rad, th.bg);
    let border = if bad {
        th.warn
    } else if focused || hot {
        th.accent
    } else {
        th.border
    };
    g.stroke_rrect(r, rad, if focused { 1.5 * s } else { 1.0 }, border);
    if is_date(f) {
        let b = pick_button(r, s);
        let open = app.calendar.is_some_and(|c| c.field == f);
        if open || hot_now == Hot::Pick(f) {
            g.fill_rrect(b, 3.0 * s, th.hover);
        }
        calendar_icon(g, b, s, if open { th.accent } else { th.dim });
    }
    let inner = field_inner(r, f, s);
    let p = px(s);
    let text = app.choice.text(f);
    g.set_clip(Some(inner.intersect(&l.body)));
    if text.is_empty() {
        let ph = ellipsize(g, &placeholder(f), inner.w, p, FontId::Regular);
        g.text_in(&ph, inner, p, FontId::Regular, th.dim, Align::Left);
    }
    let shift = if focused { field_shift(g, text, app.caret, inner.w, p) } else { 0.0 };
    let x0 = inner.x - shift;
    if let (true, Some((a, b))) = (focused, app.selection()) {
        let xa = g.measure(&text[..a], p, FontId::Regular);
        let xb = g.measure(&text[..b], p, FontId::Regular);
        g.fill_rect(Rect::new(x0 + xa, r.y + 6.0 * s, xb - xa, r.h - 12.0 * s), th.selection);
    }
    if !text.is_empty() {
        g.text_in(text, Rect::new(x0, r.y, inner.w + shift + 1000.0, r.h), p, FontId::Regular, th.text, Align::Left);
    }
    if focused {
        let cx = (x0 + g.measure(&text[..app.caret.min(text.len())], p, FontId::Regular)).round();
        g.fill_rect(Rect::new(cx, r.y + 7.0 * s, s.max(1.0).round(), r.h - 14.0 * s), th.text);
    }
    g.set_clip(Some(l.body));
}

fn draw_filters(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let (x, p) = (l.content_x, px(s));
    g.text(&tr("Filters"), x, l.filters_title_y, 14.0 * s, FontId::Semibold, th.text);
    let look = if app.choice.narrows() { Look::Secondary } else { Look::Disabled };
    button(g, l.clear, &tr("Clear Filters"), th, s, hot == Hot::Clear, look);
    let labels = [tr("Look for"), tr("Size"), tr("Modified"), tr("Types"), tr("Extensions"), tr("Names")];
    for (r, text) in l.row_labels.iter().zip(labels) {
        let text = ellipsize(g, &text, r.w - 10.0 * s, p, FontId::Regular);
        g.text_in(&text, *r, p, FontId::Regular, th.text, Align::Left);
    }
    for f in Field::ALL {
        draw_field(g, app, l, f, th, s, hot);
    }
    // With a largest size typed, no quick choice is the one in use.
    let custom = !app.choice.text(Field::MaxSize).trim().is_empty();
    let min = app.choice.min_size().filter(|_| !custom);
    for (i, (r, &size)) in l.sizes.iter().zip(MIN_SIZES.iter()).enumerate() {
        chip(g, *r, &size_label(size), th, s, min == Some(size), hot == Hot::MinSize(i), custom);
    }
    for (i, (r, k)) in l.kinds.iter().zip(KINDS.iter()).enumerate() {
        if hot == Hot::Kind(i) {
            g.fill_rrect(*r, 4.0 * s, th.hover);
        }
        check_box(g, r.x + 4.0 * s, r.center_y(), th, s, app.choice.has_kind(i));
        let tx = r.x + 30.0 * s;
        let text = ellipsize(g, &tr(k.label), r.right() - tx - 4.0 * s, p, FontId::Regular);
        g.text_in(&text, Rect::new(tx, r.y, r.right() - tx, r.h), p, FontId::Regular, th.text, Align::Left);
    }
    for (i, (r, text)) in l.compare.iter().zip([tr("Duplicate files"), tr("Duplicate folders")]).enumerate() {
        if hot == Hot::Compare(i) {
            g.fill_rrect(*r, 4.0 * s, th.hover);
        }
        check_box(g, r.x + 4.0 * s, r.center_y(), th, s, app.compare[i]);
        let tx = r.x + 30.0 * s;
        let text = ellipsize(g, &text, r.right() - tx - 4.0 * s, p, FontId::Regular);
        g.text_in(&text, Rect::new(tx, r.y, r.right() - tx, r.h), p, FontId::Regular, th.text, Align::Left);
    }
    let help = tr("Separate with spaces or commas; * stands for any text and ? for one character. Names are of files or of the folders they are in.");
    let help = ellipsize(g, &help, l.help.w, p * 0.95, FontId::Regular);
    g.text_in(&help, l.help, p * 0.95, FontId::Regular, th.dim, Align::Left);
}

/// The bar at the bottom of the start page: the buttons and a line beside them.
fn draw_actions(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let a = l.actions;
    g.fill_rect(a, th.status);
    g.fill_rect(Rect::new(a.x, a.y, a.w, 1.0), th.border);
    let bad = app.choice.filter().err();
    let nothing = !app.compare.contains(&true);
    let ready = !app.chosen.is_empty() && bad.is_none() && !nothing;
    button(g, l.add, &strip_mnemonic(&tr("&Add Folder...")), th, s, hot == Hot::Add, Look::Secondary);
    let look = if ready { Look::Primary } else { Look::Disabled };
    button(g, l.find, &strip_mnemonic(&tr("Find &Duplicates")), th, s, hot == Hot::Find, look);
    let (text, color) = if let Hot::Kind(i) = hot {
        (format!("{}: {}", tr(KINDS[i].label), KINDS[i].exts.join(" ")), th.dim)
    } else if nothing {
        (tr("Tick duplicate files, duplicate folders or both: there is nothing to look for."), th.warn)
    } else if let Some(b) = bad {
        (bad_text(b), th.warn)
    } else if let Some(e) = &app.error {
        (e.lines().collect::<Vec<_>>().join(" · "), th.warn)
    } else if app.chosen.is_empty() {
        (tr("Tick at least one drive, or add a folder (network folders too)."), th.dim)
    } else {
        (tf("{} chosen", &[&format::count(app.chosen.len() as u64)]), th.dim)
    };
    let tx = l.find.right() + 16.0 * s;
    let room = (a.right() - 16.0 * s - tx).max(0.0);
    let text = ellipsize(g, &text, room, px(s), FontId::Regular);
    g.text_in(&text, Rect::new(tx, a.y, room, a.h), px(s), FontId::Regular, color, Align::Left);
}

/// A smallest file size, in round units when it has them: `Any size`,
/// `4 KB`, `1 MB` (otherwise as sizes are shown: `1.50 MB`).
pub fn size_label(bytes: u64) -> String {
    if bytes <= 1 {
        tr("Any size")
    } else if bytes >= 1 << 20 && bytes.is_multiple_of(1 << 20) {
        format!("{} {}", bytes >> 20, tr("MB"))
    } else if bytes >= 1 << 10 && bytes.is_multiple_of(1 << 10) {
        format!("{} {}", bytes >> 10, tr("KB"))
    } else {
        format::size(bytes)
    }
}

/// The title of the search page for its step.
pub fn stage_title(p: &crate::search::Progress, root: &Path) -> String {
    match p.stage {
        Stage::Reading if p.roots > 1 => {
            tf("Reading {} ({} of {})…", &[&root.to_string_lossy(), &(p.root + 1).to_string(), &p.roots.to_string()])
        }
        Stage::Reading => tf("Reading {}…", &[&root.to_string_lossy()]),
        Stage::Edges => tr("Comparing the first and last bytes…"),
        Stage::Contents => tr("Comparing the contents…"),
        Stage::Folders => tr("Comparing the folders…"),
    }
}

fn draw_searching(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let Some(sc) = &app.searching else { return };
    let p = &sc.progress;
    let body = l.body;
    let cx = body.x + body.w / 2.0;
    let top = body.y + body.h * 0.5 - 120.0 * s;
    let root = sc.roots.get(p.root).map_or(Path::new(""), |r| r.as_path());
    let title = ellipsize_middle(g, &stage_title(p, root), body.w - 48.0 * s, 18.0 * s, FontId::Semibold);
    g.text_in(&title, Rect::new(body.x, top, body.w, 28.0 * s), 18.0 * s, FontId::Semibold, th.text, Align::Center);
    let step = match p.stage {
        Stage::Reading => 1,
        Stage::Edges => 2,
        Stage::Contents => 3,
        Stage::Folders => 4,
    };
    let step = tf("Step {} of 4", &[&step.to_string()]);
    g.text_in(&step, Rect::new(body.x, top + 28.0 * s, body.w, 20.0 * s), px(s), FontId::Regular, th.dim, Align::Center);
    let stats: Vec<(String, String)> = match p.stage {
        Stage::Reading => vec![
            (format::count(p.files), tr("files")),
            (format::count(p.read.dirs), tr("folders")),
            (format::size(p.read.bytes), tr("found")),
        ],
        _ => vec![
            (format::count(p.done), tf("of {} files", &[&format::count(p.total)])),
            (format::size(p.done_bytes), tf("of {} read", &[&format::size(p.total_bytes)])),
        ],
    };
    let col_w = (190.0 * s).min((body.w - 32.0 * s) / stats.len() as f32);
    for (i, (num, label)) in stats.iter().enumerate() {
        let x = cx + (i as f32 - stats.len() as f32 / 2.0) * col_w;
        let r = Rect::new(x, top + 62.0 * s, col_w, 40.0 * s);
        g.text_in(num, r, 26.0 * s, FontId::Semibold, th.accent, Align::Center);
        g.text_in(label, Rect::new(x, r.bottom(), col_w, 20.0 * s), px(s), FontId::Regular, th.dim, Align::Center);
    }
    if p.stage != Stage::Reading && p.total_bytes > 0 {
        let w = (420.0 * s).min(body.w - 48.0 * s);
        let bar = Rect::new((cx - w / 2.0).round(), (top + 132.0 * s).round(), w, (8.0 * s).round());
        g.fill_rrect(bar, bar.h / 2.0, th.bar_track);
        let frac = (p.done_bytes as f32 / p.total_bytes as f32).clamp(0.0, 1.0);
        if frac > 0.0 {
            g.fill_rrect(Rect::new(bar.x, bar.y, (bar.w * frac).max(bar.h), bar.h), bar.h / 2.0, th.accent);
        }
    }
    let cur = ellipsize_middle(g, &p.current, body.w - 48.0 * s, px(s), FontId::Regular);
    g.text_in(&cur, Rect::new(body.x, top + 150.0 * s, body.w, 20.0 * s), px(s), FontId::Regular, th.dim, Align::Center);
    let secs = sc.started.elapsed().as_secs();
    let mut line = tf("{} s", &[&secs.to_string()]);
    if p.read.errors > 0 {
        line = format!("{line} · {}", tf("{} folders could not be read", &[&format::count(p.read.errors)]));
    }
    g.text_in(&line, Rect::new(body.x, top + 174.0 * s, body.w, 20.0 * s), px(s), FontId::Regular, th.dim, Align::Center);
    button(g, l.stop, &strip_mnemonic(&tr("&Stop")), th, s, hot == Hot::Stop, Look::Primary);
}

/// The name of a file and the folder it is in.
fn split_path(p: &Path) -> (String, String) {
    let name = p.file_name().map_or_else(|| p.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
    let folder = p.parent().map_or(String::new(), |d| d.to_string_lossy().into_owned());
    (name, folder)
}

/// The text after a group's name: how many copies and what they waste.
pub fn group_text(f: &Found, gi: usize) -> String {
    let g = &f.groups[gi];
    let n = format::count(g.files.len() as u64);
    tf("{} copies of {} · {} can be freed", &[&n, &format::size(g.size), &format::size(g.wasted())])
}

/// The text after the name of a group of folders: how many and how alike.
pub fn folder_group_text(app: &App, gi: usize) -> String {
    let (Some(f), Some(grp)) = (&app.found, app.folder_groups.get(gi)) else { return String::new() };
    if grp.identical() {
        let n = format::count(grp.folders.len() as u64);
        tf("{} identical folders of {} · {} can be freed", &[&n, &format::size(grp.same_bytes), &format::size(grp.wasted())])
    } else {
        let files: u64 = grp.folders.iter().map(|&i| f.folders[i].files).max().unwrap_or(0);
        let same = f.pairs.iter().find(|p| [p.a, p.b] == [grp.folders[0], grp.folders[1]] || [p.b, p.a] == [grp.folders[0], grp.folders[1]]);
        let same_files = same.map_or(0, |p| p.same_files);
        tf(
            "{} the same · {} in common · {} of {} files",
            &[&format::whole_percent(grp.permille as u64 / 10), &format::size(grp.same_bytes), &format::count(same_files), &format::count(files)],
        )
    }
}

/// The label of a least share of folders: identical, or a percentage or more.
pub fn share_label(share: u16) -> String {
    if share >= 1000 {
        tr("Identical")
    } else {
        tf("{} or more", &[&format::whole_percent(share as u64 / 10)])
    }
}

/// The bar above the list: files or folders, and the least share of folders.
fn draw_tools(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let Some(found) = &app.found else { return };
    g.fill_rect(l.tools, th.status);
    let labels = [
        tf("Files ({})", &[&format::count(found.groups.len() as u64)]),
        tf("Folders ({})", &[&format::count(app.folder_groups.len() as u64)]),
    ];
    for (i, (r, text)) in l.tabs.iter().zip(labels).enumerate().filter(|(_, (r, _))| r.w > 0.0) {
        let on = (i == 1) == (app.view == View::Folders);
        chip(g, *r, &text, th, s, on, hot == Hot::Tab(i), false);
    }
    if let (Some(first), true) = (l.shares.first(), !l.shares.is_empty()) {
        let label = tr("Show:");
        let right = first.x - 10.0 * s;
        let left = l.tabs.last().map_or(l.tools.x, |r| r.right() + 12.0 * s);
        let text = ellipsize(g, &label, (right - left).max(0.0), px(s), FontId::Regular);
        let w = g.measure(&text, px(s), FontId::Regular);
        g.text_in(&text, Rect::new(right - w, l.tools.y, w, l.tools.h), px(s), FontId::Regular, th.dim, Align::Left);
    }
    for (i, (r, &share)) in l.shares.iter().zip(SHARES.iter()).enumerate() {
        chip(g, *r, &share_label(share), th, s, app.share == share, hot == Hot::Share(i), false);
    }
}

fn draw_result(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let Some(found) = &app.found else { return };
    let p = px(s);
    let pad = 8.0 * s;
    let message = |g: &mut Gfx, area: Rect, title: &str, sub: &str| {
        let mut r = area;
        let top = r.take_top(r.h * 0.45);
        g.text_in(title, Rect::new(top.x, top.bottom() - 30.0 * s, top.w, 30.0 * s), 18.0 * s, FontId::Semibold, th.text, Align::Center);
        let sub = ellipsize(g, sub, top.w - 32.0 * s, p, FontId::Regular);
        g.text_in(&sub, Rect::new(top.x, top.bottom() + 4.0 * s, top.w, 20.0 * s), p, FontId::Regular, th.dim, Align::Center);
    };
    if found.groups.is_empty() {
        message(g, l.body, &tr("No duplicates were found."), &tf("{} files were compared.", &[&format::count(found.files)]));
        return;
    }
    draw_tools(g, app, l, th, s, hot);
    if app.rows.is_empty() {
        let area = Rect::new(l.body.x, l.tools.bottom(), l.body.w, l.body.bottom() - l.tools.bottom());
        let title = if found.pairs.is_empty() { tr("No folders hold copies of each other.") } else { tr("No folders have this much in common.") };
        message(g, area, &title, &tr("Folders are compared by the contents of all the files below them, whatever their names."));
        return;
    }
    // Header.
    g.fill_rect(l.header, th.header);
    g.fill_rect(Rect::new(l.header.x, l.header.bottom() - 1.0, l.full.w, 1.0), th.border);
    let heads = [
        (l.cols.name, tr("Name"), Align::Left),
        (l.cols.folder, tr("Folder"), Align::Left),
        (l.cols.size, tr("Size"), Align::Right),
        (l.cols.date, tr("Modified"), Align::Right),
    ];
    for ((x, w), text, align) in &heads {
        if *w > 0.0 {
            let r = Rect::new(x + pad, l.header.y, w - 2.0 * pad, l.header.h);
            let text = ellipsize(g, text, r.w, p, FontId::Semibold);
            g.text_in(&text, r, p, FontId::Semibold, th.dim, *align);
        }
    }
    g.set_clip(Some(l.list));
    let end = (app.scroll + l.page_rows() + 1).min(app.rows.len());
    let (asc, desc) = g.metrics(p, FontId::Regular);
    for (i, row) in app.rows.iter().enumerate().take(end).skip(app.scroll) {
        let y = l.list.y + (i - app.scroll) as f32 * l.row_h;
        let r = Rect::new(l.list.x, y, l.list.w, l.row_h);
        let base = (r.center_y() + (asc + desc) / 2.0).round();
        match *row {
            Row::Group(gi) => {
                g.fill_rect(r, th.header);
                if app.sel == Some(i) {
                    g.fill_rect(r, th.selection);
                } else if hot == Hot::Row(i) {
                    g.fill_rect(r, th.hover);
                }
                let grp = &found.groups[gi];
                let (name, _) = split_path(&grp.files[0].path);
                let x = r.x + pad;
                let room = r.w - 2.0 * pad;
                let text = group_text(found, gi);
                let tw = g.measure(&text, p, FontId::Regular);
                let name = ellipsize(g, &name, (room - tw - 16.0 * s).max(room * 0.3), p, FontId::Semibold);
                let nw = g.text(&name, x, base, p, FontId::Semibold, th.text);
                let text = ellipsize(g, &text, (room - nw - 16.0 * s).max(0.0), p, FontId::Regular);
                g.text(&text, x + nw + 16.0 * s, base, p, FontId::Regular, th.dim);
            }
            Row::File(gi, fi) => {
                if app.sel == Some(i) {
                    g.fill_rect(r, th.selection);
                } else if hot == Hot::Row(i) {
                    g.fill_rect(r, th.hover);
                }
                let grp = &found.groups[gi];
                let c = &grp.files[fi];
                let (name, folder) = split_path(&c.path);
                check_box(g, l.cols.name.0 + 10.0 * s, r.center_y(), th, s, app.marks.contains(&c.path));
                let x = l.cols.name.0 + 38.0 * s;
                let w = l.cols.name.0 + l.cols.name.1 - x - pad;
                let name = ellipsize(g, &name, w.max(0.0), p, FontId::Regular);
                g.text(&name, x, base, p, FontId::Regular, th.text);
                let cell = |c: (f32, f32)| Rect::new(c.0 + pad, r.y, (c.1 - 2.0 * pad).max(0.0), r.h);
                if l.cols.folder.1 > 0.0 {
                    let fr = cell(l.cols.folder);
                    let folder = ellipsize_middle(g, &folder, fr.w, p, FontId::Regular);
                    g.text_in(&folder, fr, p, FontId::Regular, th.dim, Align::Left);
                }
                if l.cols.size.1 > 0.0 {
                    g.text_in(&format::size(grp.size), cell(l.cols.size), p, FontId::Regular, th.text, Align::Right);
                }
                if l.cols.date.1 > 0.0 {
                    g.text_in(&format::date(c.mtime), cell(l.cols.date), p, FontId::Regular, th.dim, Align::Right);
                }
            }
            Row::FolderGroup(gi) => {
                g.fill_rect(r, th.header);
                if app.sel == Some(i) {
                    g.fill_rect(r, th.selection);
                } else if hot == Hot::Row(i) {
                    g.fill_rect(r, th.hover);
                }
                let grp = &app.folder_groups[gi];
                let (name, _) = split_path(&found.folders[grp.folders[0]].path);
                let x = r.x + pad;
                let room = r.w - 2.0 * pad;
                let text = folder_group_text(app, gi);
                let tw = g.measure(&text, p, FontId::Regular);
                let name = ellipsize(g, &name, (room - tw - 16.0 * s).max(room * 0.3), p, FontId::Semibold);
                let nw = g.text(&name, x, base, p, FontId::Semibold, th.text);
                let text = ellipsize(g, &text, (room - nw - 16.0 * s).max(0.0), p, FontId::Regular);
                g.text(&text, x + nw + 16.0 * s, base, p, FontId::Regular, th.dim);
            }
            Row::Folder(gi, fi) => {
                if app.sel == Some(i) {
                    g.fill_rect(r, th.selection);
                } else if hot == Hot::Row(i) {
                    g.fill_rect(r, th.hover);
                }
                let f = &found.folders[app.folder_groups[gi].folders[fi]];
                let (name, folder) = split_path(&f.path);
                if app.folder_groups[gi].identical() {
                    check_box(g, l.cols.name.0 + 10.0 * s, r.center_y(), th, s, app.marks.contains(&f.path));
                }
                folder_icon(g, l.cols.name.0 + 38.0 * s, r.center_y(), s);
                let x = l.cols.name.0 + 58.0 * s;
                let w = l.cols.name.0 + l.cols.name.1 - x - pad;
                let name = ellipsize(g, &name, w.max(0.0), p, FontId::Regular);
                g.text(&name, x, base, p, FontId::Regular, th.text);
                let cell = |c: (f32, f32)| Rect::new(c.0 + pad, r.y, (c.1 - 2.0 * pad).max(0.0), r.h);
                if l.cols.folder.1 > 0.0 {
                    let fr = cell(l.cols.folder);
                    let folder = ellipsize_middle(g, &folder, fr.w, p, FontId::Regular);
                    g.text_in(&folder, fr, p, FontId::Regular, th.dim, Align::Left);
                }
                if l.cols.size.1 > 0.0 {
                    g.text_in(&format::size(f.bytes), cell(l.cols.size), p, FontId::Regular, th.text, Align::Right);
                }
                if l.cols.date.1 > 0.0 {
                    g.text_in(&format::date(f.mtime), cell(l.cols.date), p, FontId::Regular, th.dim, Align::Right);
                }
            }
        }
    }
    g.set_clip(None);
    // Scroll bar.
    let page = l.page_rows();
    if app.rows.len() > page {
        let (y, h) = thumb(l, app.rows.len(), page, app.scroll);
        g.fill_rrect(Rect::new(l.track.x + 2.0 * s, y, l.track.w - 4.0 * s, h), 3.0 * s, th.scrollbar);
    }
    draw_result_actions(g, app, l, th, s, hot);
}

/// The label of the button that moves the marked copies away.
pub fn recycle_label() -> String {
    strip_mnemonic(&crate::commands::Command::Recycle.label())
}

/// The bar under the result: the rules, and what to do with the marked copies.
fn draw_result_actions(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let a = l.actions;
    if a.h <= 0.0 {
        return;
    }
    g.fill_rect(a, th.status);
    g.fill_rect(Rect::new(a.x, a.y, a.w, 1.0), th.border);
    let marked = app.marked();
    let any = !marked.is_empty();
    let can_mark = (0..app.rows.len()).any(|i| app.markable(i));
    let look = |on: bool| if on { Look::Secondary } else { Look::Disabled };
    button(g, l.rules, &tr("Mark Copies..."), th, s, hot == Hot::Rules, look(can_mark));
    button(g, l.unmark, &strip_mnemonic(&tr("&Unmark All")), th, s, hot == Hot::Unmark, look(any));
    button(g, l.move_to, &strip_mnemonic(&tr("Mo&ve To...")), th, s, hot == Hot::MoveTo, look(any));
    button(g, l.recycle, &recycle_label(), th, s, hot == Hot::Recycle, if any { Look::Primary } else { Look::Disabled });
    let text = if any {
        let bytes: u64 = marked.iter().map(|(_, b)| b).sum();
        tf("{} marked · {}", &[&format::count(marked.len() as u64), &format::size(bytes)])
    } else if can_mark {
        tr("Tick the copies to move, or mark them with a rule.")
    } else {
        tr("Similar folders cannot be marked: each holds files of its own.")
    };
    let x = l.unmark.right() + 14.0 * s;
    let room = (l.move_to.x - 14.0 * s - x).max(0.0);
    let text = ellipsize(g, &text, room, px(s), FontId::Regular);
    g.text_in(&text, Rect::new(x, a.y, room, a.h), px(s), FontId::Regular, if any { th.text } else { th.dim }, Align::Left);
}

/// The scroll bar's thumb: (y, height).
pub fn thumb(l: &Layout, rows: usize, page: usize, scroll: usize) -> (f32, f32) {
    let track = l.track;
    let h = (track.h * page as f32 / rows as f32).max(24.0f32.min(track.h));
    let max = rows.saturating_sub(page).max(1);
    let y = track.y + (track.h - h) * scroll as f32 / max as f32;
    (y, h)
}

/// The scroll position for a thumb dragged to `y` (its middle).
pub fn scroll_for(l: &Layout, rows: usize, page: usize, y: f32) -> usize {
    let (_, h) = thumb(l, rows, page, 0);
    let room = (l.track.h - h).max(1.0);
    let frac = ((y - l.track.y - h / 2.0) / room).clamp(0.0, 1.0);
    (frac * rows.saturating_sub(page) as f32).round() as usize
}

fn draw_status(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let st = l.status;
    g.fill_rect(st, th.status);
    g.fill_rect(Rect::new(st.x, st.y, st.w, 1.0), th.border);
    let p = 12.0 * s;
    let pad = 10.0 * s;
    let hover = match hot {
        Hot::Row(i) => Some(i),
        _ => None,
    };
    let (left, right) = status_text(app, hover);
    let rw = g.measure(&right, p, FontId::Regular);
    g.text_in(&right, Rect::new(st.right() - rw - pad, st.y, rw, st.h), p, FontId::Regular, th.dim, Align::Left);
    let lw = (st.w - rw - 3.0 * pad).max(0.0);
    let left = ellipsize_middle(g, &left, lw, p, FontId::Regular);
    g.text_in(&left, Rect::new(st.x + pad, st.y, lw, st.h), p, FontId::Regular, th.text, Align::Left);
}

/// The status bar: (the row pointed or selected, the totals).
pub fn status_text(app: &App, hover: Option<usize>) -> (String, String) {
    if page(app) != Page::Result {
        return (String::new(), String::new());
    }
    let Some(f) = &app.found else { return (String::new(), String::new()) };
    let note = app.note.clone().filter(|_| hover.is_none());
    let left = note.unwrap_or_else(|| match hover.or(app.sel).and_then(|i| app.rows.get(i)) {
        Some(Row::File(gi, fi)) => {
            let grp = &f.groups[*gi];
            format!("{}    {}", grp.files[*fi].path.display(), format::size(grp.size))
        }
        Some(Row::Group(gi)) => group_text(f, *gi),
        Some(Row::Folder(gi, fi)) => {
            let d = &f.folders[app.folder_groups[*gi].folders[*fi]];
            format!("{}    {} · {}", d.path.display(), format::size(d.bytes), tf("{} files", &[&format::count(d.files)]))
        }
        Some(Row::FolderGroup(gi)) => folder_group_text(app, *gi),
        None => String::new(),
    });
    let mut parts = if f.look[0] {
        vec![
            tf("{} groups", &[&format::count(f.groups.len() as u64)]),
            tf("{} copies", &[&format::count(f.copies())]),
            tf("{} can be freed", &[&format::size(f.wasted())]),
        ]
    } else {
        let freed: u64 = app.folder_groups.iter().map(|g| g.wasted()).sum();
        vec![
            tf("{} groups of folders", &[&format::count(app.folder_groups.len() as u64)]),
            tf("{} can be freed", &[&format::size(freed)]),
        ]
    };
    let at = parts.len();
    parts.push(tf("{} files compared", &[&format::count(f.files)]));
    if f.cached > 0 {
        parts.push(tf("{} hashes from the cache", &[&format::count(f.cached)]));
    }
    parts.push(tf("in {} s", &[&format::decimal(f.elapsed.as_secs_f64(), 1)]));
    if f.min_size > 1 {
        parts.insert(at, tf("files of {} or more", &[&size_label(f.min_size)]));
    }
    if f.narrowed {
        parts.insert(at, tr("with filters"));
    }
    let mut methods: Vec<String> = Vec::new();
    for r in &f.roots {
        let m = match r.method {
            Method::Ntfs => tr("from the NTFS table"),
            Method::Everything => tr("from the Everything index"),
            Method::System => tr("folder by folder"),
        };
        if !methods.contains(&m) {
            methods.push(m);
        }
    }
    parts.push(methods.join(", "));
    if f.roots.iter().any(|r| r.admin_would_help) {
        parts.push(tr("faster as administrator (File menu)"));
    }
    if f.unreadable > 0 {
        parts.push(tf("{} files could not be read", &[&format::count(f.unreadable)]));
    }
    let folders: usize = f.roots.iter().map(|r| r.unreadable).sum();
    if folders > 0 {
        parts.push(tf("{} folders could not be read", &[&format::count(folders as u64)]));
    }
    if !f.failed.is_empty() {
        parts.push(tf("{} not read", &[&format::count(f.failed.len() as u64)]));
    }
    (left, parts.join(" · "))
}

#[cfg(test)]
mod screenshots {
    use super::*;
    use crate::app::Searching;
    use crate::i18n::{set_lang, Lang};
    use crate::search::{Progress, Stage};
    use crate::volumes::Volume;
    use std::path::PathBuf;

    /// Draws each page to `target/screens/*.png` for a visual check:
    /// `cargo test -p rdup --lib screenshots -- --ignored`.
    #[test]
    #[ignore]
    fn screenshots() {
        let Ok(mut g) = Gfx::new() else { return };
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&out).unwrap();
        let tt = crate::search::tests::sample("rdup-shots");
        let found = crate::search::tests::sys(std::slice::from_ref(&tt.0)).unwrap();
        let (w, h) = (1100u32, 760u32);
        let full = Rect::new(0.0, 0.0, w as f32, h as f32);
        let gb = 1u64 << 30;
        let vols = vec![
            Volume { path: "C:\\".into(), label: "Windows".into(), fs: "NTFS".into(), total: 451 * gb, free: 38 * gb, remote: false },
            Volume { path: "D:\\".into(), label: "Data".into(), fs: "NTFS".into(), total: 480 * gb, free: 210 * gb, remote: false },
            Volume { path: "T:\\".into(), label: "Backup".into(), fs: "exFAT".into(), total: 931 * gb, free: 702 * gb, remote: false },
            Volume { path: "Z:\\".into(), label: String::new(), fs: "NTFS".into(), total: 2048 * gb, free: 1024 * gb, remote: true },
        ];
        let mut shot = |name: &str, app: &App, dark: bool, hot: Hot| {
            g.resize(w, h);
            let th = Theme::new(dark, None);
            let l = layout(app, full, 1.0, false);
            draw(&mut g, app, &l, &th, 1.0, hot);
            g.pix.save_png(out.join(format!("{name}.png"))).unwrap();
        };
        set_lang(Lang::EnUS);
        let mut app = App::new();
        app.volumes = Some(vols);
        shot("start-empty", &app, false, Hot::None);
        app.toggle_root(Path::new("D:\\"));
        app.toggle_root(Path::new("T:\\"));
        app.add_folder(Path::new("\\\\nas\\photos\\2024"));
        app.add_folder(Path::new("C:\\Users\\Miguel\\Pictures"));
        app.add_folder(Path::new("E:\\Old backups"));
        app.toggle_root(Path::new("E:\\Old backups"));
        shot("start-light", &app, false, Hot::Card(2));
        shot("start-dark", &app, true, Hot::Remove(1));
        {
            use crate::filter::Field;
            let mut a = App::new();
            a.volumes = app.volumes.clone();
            a.chosen = app.chosen.clone();
            a.saved = app.saved.clone();
            a.choice.texts[Field::MaxSize.index()] = "4 GB".into();
            a.choice.texts[Field::From.index()] = "2024".into();
            a.choice.texts[Field::SkipExt.index()] = "tmp log obj lnk".into();
            a.choice.texts[Field::OnlyNames.index()] = "Setup* *Installer* *video*".into();
            a.choice.toggle_kind(1);
            a.choice.toggle_kind(4);
            a.focus_field(Field::OnlyNames, Some(7));
            a.drag_caret(19);
            shot("filters-light", &a, false, Hot::Kind(1));
            a.focus = None;
            a.choice.texts[Field::To.index()] = "2026-11-08".into();
            a.calendar = Some(crate::app::Calendar::open(Field::To, "2026-11-08"));
            a.start_scroll = 400.0;
            shot("calendar-light", &a, false, Hot::CalDay(17));
            set_lang(Lang::PtPT);
            a.calendar = Some(crate::app::Calendar::open(Field::From, "2024"));
            shot("calendar-pt-dark", &a, true, Hot::CalNext);
            set_lang(Lang::EnUS);
            a.calendar = None;
            a.start_scroll = 0.0;
            a.choice.texts[Field::To.index()] = "2023-02-30".into();
            a.focus_field(Field::To, None);
            a.start_scroll = 400.0;
            shot("filters-dark", &a, true, Hot::Field(Field::SkipNames));
            set_lang(Lang::PtPT);
            a.start_scroll = 0.0;
            a.focus = None;
            a.choice.texts[Field::To.index()].clear();
            a.choice.texts[Field::MaxSize.index()].clear();
            a.choice.texts[Field::SkipExt.index()] = "tmp log mp4".into();
            shot("filters-pt", &a, false, Hot::None);
            set_lang(Lang::EnUS);
            a.compare = [false; 2];
            shot("filters-nothing", &a, false, Hot::Compare(1));
            a.compare = [true; 2];
            // A narrow window: the sizes and the kinds wrap.
            let (nw, nh) = (640u32, 760u32);
            let mut g2 = Gfx::new().unwrap();
            g2.resize(nw, nh);
            let nl = layout(&a, Rect::new(0.0, 0.0, nw as f32, nh as f32), 1.0, false);
            draw(&mut g2, &a, &nl, &Theme::new(false, None), 1.0, Hot::None);
            g2.pix.save_png(out.join("filters-narrow.png")).unwrap();
        }
        app.searching = Some(Searching {
            roots: vec!["D:\\".into(), "T:\\".into()],
            progress: Progress { stage: Stage::Reading, root: 1, roots: 2, files: 812_345, read: rt_scan::Progress { files: 412_000, dirs: 38_765, bytes: 312 * gb, errors: 0, current: "T:\\Backup\\2023\\Photos".into() }, current: "T:\\Backup\\2023\\Photos".into(), ..Progress::default() },
            started: std::time::Instant::now(),
            generation: 1,
        });
        shot("searching-read", &app, false, Hot::None);
        app.searching.as_mut().unwrap().progress = Progress {
            stage: Stage::Contents,
            files: 812_345,
            done: 1_234,
            total: 3_456,
            done_bytes: 41 * gb,
            total_bytes: 97 * gb,
            current: "D:\\Videos\\Holidays 2019\\GOPR0042.MP4".into(),
            ..Progress::default()
        };
        shot("searching-hash", &app, true, Hot::Stop);
        app.searching = None;
        app.set_found(found);
        app.page = 30;
        app.select(2);
        app.toggle_mark(1).unwrap();
        app.toggle_mark(5).unwrap();
        shot("result-light", &app, false, Hot::Row(5));
        shot("result-dark", &app, true, Hot::None);
        set_lang(Lang::PtPT);
        shot("result-pt", &app, false, Hot::None);
        app.show_start = true;
        shot("start-pt", &app, false, Hot::None);
        set_lang(Lang::EnUS);
        app.show_start = false;
        // The folders.
        let ft = crate::folders::tests::folders("rdup-shots-folders");
        app.set_found(crate::folders::tests::search(&ft, 1000));
        app.set_view(View::Folders);
        app.select(4);
        app.toggle_mark(2).unwrap();
        shot("folders-light", &app, false, Hot::Share(2));
        app.set_share(1000);
        shot("folders-dark", &app, true, Hot::Tab(0));
        set_lang(Lang::PtPT);
        app.set_share(500);
        shot("folders-pt", &app, false, Hot::None);
        set_lang(Lang::EnUS);
        // Copies in one folder: no folders to compare.
        let one = rt_scan::testing::TempTree::new("rdup-shots-one");
        one.write("a.bin", &crate::search::tests::data(5000, 1));
        one.write("b.bin", &crate::search::tests::data(5000, 1));
        app.set_found(crate::search::tests::sys(std::slice::from_ref(&one.0)).unwrap());
        app.set_view(View::Folders);
        shot("folders-none", &app, false, Hot::None);
        let empty = crate::search::tests::sys(&[tt.0.join("docs")]).unwrap();
        app.set_found(empty);
        shot("result-none", &app, false, Hot::None);
        // The icon, large and at the size of the title bar.
        for n in [256u32, 32] {
            let rgba = crate::ui::appicon::render(n);
            let size = tiny_skia::IntSize::from_wh(n, n).unwrap();
            let mut px = tiny_skia::Pixmap::from_vec(rgba, size).unwrap();
            for p in px.pixels_mut() {
                let c = tiny_skia::ColorU8::from_rgba(p.red(), p.green(), p.blue(), p.alpha());
                *p = c.premultiply();
            }
            px.save_png(out.join(format!("icon-{n}.png"))).unwrap();
        }
    }
}
