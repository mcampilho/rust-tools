//! The three pages of the window, drawn each frame:
//!
//! - the start page: the drives, with their used space, and a button to
//!   choose a folder;
//! - the scan page: what has been found so far;
//! - the result: the folder list on top, the treemap below, split by a bar
//!   that can be dragged.
//!
//! [`layout`] decides where things go; `main` uses the same [`Layout`] to
//! find what the pointer is on.

use super::gfx::{Align, Color, FontId, Gfx, Rect};
use super::theme::Theme;
use crate::app::App;
use crate::format;
use crate::i18n::{tf, tr};
use crate::model::{Id, Kind, SizeMode, Tree, F_HARDLINK, F_OTHER_FS, F_UNREADABLE};
use crate::chart::{pop_offset, sector, Chart, Hit, Kind as ChartKind, Segment};
use crate::settings::ChartType;
use crate::treemap::{self, Map};
use rt_core::i18n::strip_mnemonic;
use rt_gfx::dialog::{ellipsize, ellipsize_middle};

/// What the pointer is on, for hover effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Hot {
    #[default]
    None,
    Card(usize),
    Choose,
    Stop,
    Row(usize),
    Splitter,
    /// A chart tab above the chart.
    Tab(ChartType),
}

/// Column positions of the list, as (x, width); a width of 0 hides a column.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cols {
    pub name: (f32, f32),
    pub share: (f32, f32),
    pub size: (f32, f32),
    pub files: (f32, f32),
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
    pub header: Rect,
    pub list: Rect,
    pub row_h: f32,
    pub indent: f32,
    pub cols: Cols,
    pub track: Rect,
    pub splitter: Rect,
    pub map_title: Rect,
    /// The chart tabs, at the right of the title.
    pub tabs: Vec<(ChartType, Rect)>,
    pub map: Rect,
    // Start page.
    pub cards: Vec<Rect>,
    pub choose: Rect,
    pub title_y: f32,
    pub content_w: f32,
    /// How far the start page can scroll, when it does not fit.
    pub start_max: f32,
    // Scan page.
    pub stop: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Start,
    Scanning,
    Result,
}

pub fn page(app: &App) -> Page {
    if app.scanning.is_some() {
        Page::Scanning
    } else if app.show_drives || !app.has_tree() {
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

    /// Whether `x` is on the open/close arrow of a row at `depth`.
    pub fn on_arrow(&self, x: f32, depth: u16) -> bool {
        let ax = self.cols.name.0 + 4.0 + depth as f32 * self.indent;
        x >= ax - 2.0 && x < ax + self.indent + 2.0
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
pub fn layout(app: &App, settings_split: f32, full: Rect, s: f32, menu_bar: bool) -> Layout {
    let mut l = Layout { full, ..Layout::default() };
    let mut area = full;
    if menu_bar {
        l.bar = area.take_top((28.0 * s).round());
    }
    l.status = area.take_bottom((24.0 * s).round());
    l.body = area;
    match page(app) {
        Page::Start => {
            let n = app.volumes.as_ref().map_or(0, |v| v.len());
            let (card_h, gap) = ((64.0 * s).round(), (10.0 * s).round());
            // Title above, button and notes below.
            let (above, below) = (84.0 * s, 130.0 * s);
            let one_col = above + n as f32 * (card_h + gap) + below;
            // Two columns when one would not fit and the window is wide enough.
            let cols = if one_col > area.h && area.w >= 760.0 * s { 2 } else { 1 };
            let w = ((if cols == 2 { 1000.0 } else { 720.0 }) * s).min(area.w - 48.0 * s).max(200.0 * s);
            let x = (area.x + (area.w - w) / 2.0).round();
            let rows = n.div_ceil(cols);
            let content = above + rows as f32 * (card_h + gap) + if n == 0 { 40.0 * s } else { 0.0 } + below;
            l.start_max = (content - area.h).max(0.0);
            let top = area.y - app.start_scroll.clamp(0.0, l.start_max);
            l.title_y = (top + 48.0 * s).round();
            l.content_w = w;
            let card_w = ((w - gap * (cols as f32 - 1.0)) / cols as f32).floor();
            for i in 0..n {
                let (c, r) = (i % cols, i / cols);
                let y = top + above + r as f32 * (card_h + gap);
                l.cards.push(Rect::new(x + c as f32 * (card_w + gap), y.round(), card_w, card_h));
            }
            let y = top + above + rows as f32 * (card_h + gap) + if n == 0 { 40.0 * s } else { 0.0 };
            l.choose = Rect::new(x, (y + 8.0 * s).round(), (220.0 * s).round(), (34.0 * s).round());
        }
        Page::Scanning => {
            let w = (160.0 * s).round();
            l.stop = Rect::new((area.x + (area.w - w) / 2.0).round(), (area.y + area.h * 0.5 + 120.0 * s).round(), w, (34.0 * s).round());
        }
        Page::Result => {
            let split = (area.h * settings_split).round();
            let mut top = area.take_top(split);
            l.splitter = area.take_top((6.0 * s).round());
            l.map_title = area.take_top((26.0 * s).round());
            let mut x = l.map_title.right() - 6.0 * s;
            for kind in ChartType::ALL.into_iter().rev() {
                let w = (tab_w(kind) * s).round();
                x -= w;
                l.tabs.push((kind, Rect::new(x.round(), l.map_title.y + 3.0 * s, w, l.map_title.h - 6.0 * s)));
                x -= 4.0 * s;
            }
            l.tabs.reverse();
            l.map = Rect::new(area.x.round(), area.y.round(), area.w.round(), area.h.round());
            l.header = top.take_top((26.0 * s).round());
            l.row_h = (22.0 * s).round();
            l.indent = (16.0 * s).round();
            l.track = top.take_right((10.0 * s).round());
            l.list = top;
            // Columns from the right; the name takes what is left.
            let mut x1 = top.right();
            let mut col = |want: f32, min_name: f32| {
                let w = (want * s).round();
                if x1 - w - top.x >= min_name * s {
                    x1 -= w;
                    (x1, w)
                } else {
                    (x1, 0.0)
                }
            };
            l.cols.date = col(100.0, 420.0);
            l.cols.files = col(90.0, 330.0);
            l.cols.size = col(90.0, 200.0);
            l.cols.share = col(130.0, 160.0);
            l.cols.name = (top.x, x1 - top.x);
        }
    }
    l
}

/// What a frame shows besides the app's state.
#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub mode: SizeMode,
    /// The chart shown below the list.
    pub kind: ChartType,
    /// The treemap for `layout.map`.
    pub map: Option<&'a Map>,
    /// The pie or the rings for `layout.map`.
    pub chart: Option<&'a Chart>,
    /// What the pointer is on in the pie or the rings.
    pub hover_seg: Option<Hit>,
    pub hot: Hot,
    /// The node under the pointer.
    pub hover: Option<Id>,
}

/// Draws the current page.
pub fn draw(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, f: &Frame) {
    g.clear(th.bg);
    match page(app) {
        Page::Start => draw_start(g, app, l, th, s, f.hot),
        Page::Scanning => draw_scanning(g, app, l, th, s, f.hot),
        Page::Result => draw_result(g, app, l, th, s, f),
    }
    draw_status(g, app, l, th, s, f.hover);
}

fn px(s: f32) -> f32 {
    12.5 * s
}

fn button(g: &mut Gfx, r: Rect, label: &str, th: &Theme, s: f32, hot: bool) {
    let bg = if hot { th.accent } else { th.accent.mix(th.bg, 0.12) };
    g.fill_rrect(r, (5.0 * s).round(), bg);
    let fg = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
    g.text_in(label, r, 13.0 * s, FontId::Semibold, fg, Align::Center);
}

fn draw_start(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let x = l.choose.x;
    let w = l.content_w;
    let title_y = l.title_y;
    g.set_clip(Some(l.body));
    g.text(&tr("Choose a drive or folder to analyze"), x, title_y, 20.0 * s, FontId::Semibold, th.text);
    match &app.volumes {
        None => {
            g.text(&tr("Looking for drives…"), x, title_y + 40.0 * s, px(s), FontId::Regular, th.dim);
        }
        Some(vols) => {
            for (i, (r, v)) in l.cards.iter().zip(vols).enumerate() {
                let hover = hot == Hot::Card(i);
                g.fill_rrect(*r, (6.0 * s).round(), if hover { th.panel.mix(th.accent, 0.08) } else { th.panel });
                g.stroke_rrect(*r, (6.0 * s).round(), 1.0, if hover { th.accent } else { th.border });
                let pad = 14.0 * s;
                let name = v.path.to_string_lossy();
                let left_w = r.w * 0.5 - pad;
                let (asc, _) = g.metrics(15.0 * s, FontId::Semibold);
                let y1 = r.y + pad + asc;
                let name = ellipsize(g, &name, left_w, 15.0 * s, FontId::Semibold);
                let nw = g.text(&name, r.x + pad, y1, 15.0 * s, FontId::Semibold, th.text);
                if !v.label.is_empty() && nw + 40.0 * s < left_w {
                    let label = ellipsize(g, &v.label, left_w - nw - 10.0 * s, px(s), FontId::Regular);
                    g.text(&label, r.x + pad + nw + 10.0 * s, y1, px(s), FontId::Regular, th.text);
                }
                let mut kind = v.fs.clone();
                if v.remote {
                    kind = if kind.is_empty() { tr("Network") } else { format!("{kind} · {}", tr("Network")) };
                }
                g.text(&kind, r.x + pad, r.bottom() - pad, px(s) * 0.95, FontId::Regular, th.dim);
                // Used space.
                let bar = Rect::new(r.x + r.w * 0.5, r.y + pad + 2.0 * s, r.w * 0.5 - pad, (10.0 * s).round());
                g.fill_rrect(bar, bar.h / 2.0, th.bar_track);
                let frac = if v.total > 0 { v.used() as f32 / v.total as f32 } else { 0.0 };
                let fill = if frac > 0.9 { Color::rgb(0xD9534F) } else { th.accent };
                if frac > 0.0 {
                    g.fill_rrect(Rect::new(bar.x, bar.y, (bar.w * frac).max(bar.h), bar.h), bar.h / 2.0, fill);
                }
                let text = tf("{} free of {}", &[&format::size(v.free), &format::size(v.total)]);
                g.text_in(&text, Rect::new(bar.x, r.bottom() - pad - 12.0 * s, bar.w, 14.0 * s), px(s) * 0.95, FontId::Regular, th.dim, Align::Right);
            }
            if vols.is_empty() {
                g.text(&tr("No drives were found."), x, title_y + 40.0 * s, px(s), FontId::Regular, th.dim);
            }
        }
    }
    button(g, l.choose, &strip_mnemonic(&tr("Choose &Folder...")), th, s, hot == Hot::Choose);
    let hint = tr("Network folders can be chosen too, or given on the command line (\\\\server\\share).");
    let hint_y = l.choose.bottom() + 28.0 * s;
    let hint = ellipsize(g, &hint, w, px(s), FontId::Regular);
    g.text(&hint, x, hint_y, px(s), FontId::Regular, th.dim);
    if let Some(e) = &app.error {
        let e = ellipsize(g, e, w, px(s), FontId::Regular);
        g.text(&e, x, hint_y + 24.0 * s, px(s), FontId::Regular, th.warn);
    }
    g.set_clip(None);
}

fn draw_scanning(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let Some(sc) = &app.scanning else { return };
    let p = &sc.progress;
    let body = l.body;
    let cx = body.x + body.w / 2.0;
    let top = body.y + body.h * 0.5 - 110.0 * s;
    let title = tf("Reading {}…", &[&sc.root.to_string_lossy()]);
    let title = ellipsize_middle(g, &title, body.w - 48.0 * s, 18.0 * s, FontId::Semibold);
    g.text_in(&title, Rect::new(body.x, top, body.w, 28.0 * s), 18.0 * s, FontId::Semibold, th.text, Align::Center);
    let stats = [
        (format::count(p.files), tr("files")),
        (format::count(p.dirs), tr("folders")),
        (format::size(p.bytes), tr("found")),
    ];
    let col_w = (170.0 * s).min((body.w - 32.0 * s) / 3.0);
    for (i, (num, label)) in stats.iter().enumerate() {
        let x = cx + (i as f32 - 1.5) * col_w;
        let r = Rect::new(x, top + 50.0 * s, col_w, 40.0 * s);
        g.text_in(num, r, 26.0 * s, FontId::Semibold, th.accent, Align::Center);
        g.text_in(label, Rect::new(x, r.bottom(), col_w, 20.0 * s), px(s), FontId::Regular, th.dim, Align::Center);
    }
    let cur = ellipsize_middle(g, &p.current, body.w - 48.0 * s, px(s), FontId::Regular);
    g.text_in(&cur, Rect::new(body.x, top + 130.0 * s, body.w, 20.0 * s), px(s), FontId::Regular, th.dim, Align::Center);
    let secs = sc.started.elapsed().as_secs();
    let mut line = tf("{} s", &[&secs.to_string()]);
    if p.errors > 0 {
        line = format!("{line} · {}", tf("{} folders could not be read", &[&format::count(p.errors)]));
    }
    g.text_in(&line, Rect::new(body.x, top + 154.0 * s, body.w, 20.0 * s), px(s), FontId::Regular, th.dim, Align::Center);
    button(g, l.stop, &strip_mnemonic(&tr("&Stop")), th, s, hot == Hot::Stop);
}

/// Text after a node's name for what is special about it.
fn note(tree: &Tree, id: Id) -> Option<String> {
    let n = tree.node(id);
    if n.flags & F_UNREADABLE != 0 {
        Some(tr("(cannot be read)"))
    } else if n.flags & F_OTHER_FS != 0 {
        Some(tr("(other file system)"))
    } else if n.flags & F_HARDLINK != 0 {
        Some(tr("(hard link, counted once)"))
    } else if n.kind == Kind::Link {
        Some(tr("(link, not followed)"))
    } else {
        None
    }
}

fn draw_result(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, f: &Frame) {
    let Frame { mode, map, hot, hover, .. } = *f;
    let Some(scan) = &app.scan else { return };
    let t = &scan.tree;
    let p = px(s);
    // Header.
    g.fill_rect(l.header, th.header);
    g.fill_rect(Rect::new(l.header.x, l.header.bottom() - 1.0, l.full.w, 1.0), th.border);
    let size_title = match mode {
        SizeMode::Logical => tr("Size"),
        SizeMode::Allocated => tr("Size on disk"),
    };
    let pad = 8.0 * s;
    let heads = [
        (l.cols.name, tr("Name"), Align::Left),
        (l.cols.share, tr("Share of folder"), Align::Left),
        (l.cols.size, size_title, Align::Right),
        (l.cols.files, tr("Files"), Align::Right),
        (l.cols.date, tr("Modified"), Align::Right),
    ];
    for ((x, w), text, align) in &heads {
        if *w > 0.0 {
            let r = Rect::new(x + pad, l.header.y, w - 2.0 * pad, l.header.h);
            let text = ellipsize(g, text, r.w, p, FontId::Semibold);
            g.text_in(&text, r, p, FontId::Semibold, th.dim, *align);
        }
    }
    // Rows.
    let table = treemap::ext_colors(t);
    g.set_clip(Some(l.list));
    let end = (app.scroll + l.page_rows() + 1).min(app.rows.len());
    for (i, &(id, depth)) in app.rows.iter().enumerate().take(end).skip(app.scroll) {
        let y = l.list.y + (i - app.scroll) as f32 * l.row_h;
        let row = Rect::new(l.list.x, y, l.list.w, l.row_h);
        if app.sel == Some(id) {
            g.fill_rect(row, th.selection);
        } else if hot == Hot::Row(i) {
            g.fill_rect(row, th.hover);
        }
        let n = t.node(id);
        let mut x = l.cols.name.0 + 4.0 * s + depth as f32 * l.indent;
        let cy = row.center_y();
        // Open/close arrow.
        if !t.children(id).is_empty() {
            let a = 3.5 * s;
            let ax = x + l.indent / 2.0;
            let pts = if app.expanded.contains(&id) {
                [(ax - a, cy - a / 2.0), (ax, cy + a / 2.0), (ax + a, cy - a / 2.0)]
            } else {
                [(ax - a / 2.0, cy - a), (ax + a / 2.0, cy), (ax - a / 2.0, cy + a)]
            };
            g.polyline(&pts, 1.4 * s, th.dim, false);
        }
        x += l.indent + 2.0 * s;
        // Icon: a folder, or the file's colour in the treemap.
        let icon = (12.0 * s).round();
        match n.kind {
            Kind::Dir => {
                let c = Color::rgb(0xE8B84A);
                g.fill_rrect(Rect::new(x, cy - icon * 0.45, icon * 0.5, icon * 0.3), 1.5 * s, c);
                g.fill_rrect(Rect::new(x, cy - icon * 0.3, icon, icon * 0.72), 2.0 * s, c);
            }
            Kind::Link => g.stroke_rrect(Rect::new(x + 1.0, cy - icon / 2.0 + 1.0, icon - 2.0, icon - 2.0), 2.0 * s, 1.0 * s, th.dim),
            Kind::File => g.fill_rrect(Rect::new(x + 1.0, cy - icon / 2.0 + 1.0, icon - 2.0, icon - 2.0), 2.0 * s, Color::rgb(treemap::base_color(t, id, &table))),
        }
        x += icon + 6.0 * s;
        let name_w = l.cols.name.0 + l.cols.name.1 - x - pad;
        let name = t.name(id);
        let (asc, desc) = g.metrics(p, FontId::Regular);
        let base = (cy + (asc + desc) / 2.0).round();
        let shown = ellipsize(g, name, name_w.max(0.0), p, FontId::Regular);
        let color = if n.kind == Kind::Link { th.dim } else { th.text };
        let w = g.text(&shown, x, base, p, FontId::Regular, color);
        if let Some(note) = note(t, id) {
            let room = name_w - w - 6.0 * s;
            if room > 20.0 * s {
                let warn = n.flags & F_UNREADABLE != 0;
                let note = ellipsize(g, &note, room, p, FontId::Regular);
                g.text(&note, x + w + 6.0 * s, base, p, FontId::Regular, if warn { th.warn } else { th.dim });
            }
        }
        // Share of the parent folder.
        let size = n.size_in(mode);
        let whole = t.parent(id).map_or(size, |pa| t.node(pa).size_in(mode));
        if l.cols.share.1 > 0.0 {
            let bar = Rect::new(l.cols.share.0 + pad, row.y + 4.0 * s, l.cols.share.1 - 2.0 * pad, row.h - 8.0 * s);
            g.fill_rect(bar, th.bar_track);
            let frac = if whole > 0 { size as f32 / whole as f32 } else { 0.0 };
            g.fill_rect(Rect::new(bar.x, bar.y, (bar.w * frac).round(), bar.h), th.bar);
            g.text_in(&format::percent(size, whole), bar.inset(4.0 * s), p * 0.92, FontId::Regular, th.text, Align::Right);
        }
        let cell = |c: (f32, f32)| Rect::new(c.0 + pad, row.y, c.1 - 2.0 * pad, row.h);
        if l.cols.size.1 > 0.0 {
            g.text_in(&format::size(size), cell(l.cols.size), p, FontId::Regular, th.text, Align::Right);
        }
        if l.cols.files.1 > 0.0 && n.kind == Kind::Dir {
            g.text_in(&format::count(n.files), cell(l.cols.files), p, FontId::Regular, th.dim, Align::Right);
        }
        if l.cols.date.1 > 0.0 {
            g.text_in(&format::date(n.mtime), cell(l.cols.date), p, FontId::Regular, th.dim, Align::Right);
        }
    }
    g.set_clip(None);
    // Scroll bar.
    let page = l.page_rows();
    if app.rows.len() > page {
        let (y, h) = thumb(l, app.rows.len(), page, app.scroll);
        g.fill_rrect(Rect::new(l.track.x + 2.0 * s, y, l.track.w - 4.0 * s, h), 3.0 * s, th.scrollbar);
    }
    // Splitter.
    g.fill_rect(l.splitter, if hot == Hot::Splitter { th.accent.with_alpha(0x60) } else { th.status });
    g.fill_rect(Rect::new(l.splitter.x, l.splitter.y, l.splitter.w, 1.0), th.border);
    g.fill_rect(Rect::new(l.splitter.x, l.splitter.bottom() - 1.0, l.splitter.w, 1.0), th.border);
    // Chart title: the folder shown, a hint, and the chart tabs.
    let title_r = l.map_title;
    g.fill_rect(title_r, th.header);
    for &(kind, r) in &l.tabs {
        let on = kind == f.kind;
        if on {
            g.fill_rrect(r, (4.0 * s).round(), th.accent);
        } else if hot == Hot::Tab(kind) {
            g.fill_rrect(r, (4.0 * s).round(), th.hover);
        }
        let fg = if on { if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) } } else { th.text };
        g.text_in(&chart_name(kind), r, p * 0.95, FontId::Regular, fg, Align::Center);
    }
    let tabs_x = l.tabs.first().map_or(title_r.right(), |t| t.1.x);
    let shown = t.path(app.zoom).to_string_lossy().into_owned();
    let title = format!("{shown} — {}", format::size(t.node(app.zoom).size_in(mode)));
    let hint = match (app.zoom != t.root(), f.kind) {
        (_, ChartType::Rings) => tr("Double-click a folder to zoom in, click the middle to zoom out"),
        (true, _) => tr("Backspace: zoom out"),
        (false, _) => tr("Double-click a folder to zoom in"),
    };
    let hw = g.measure(&hint, p, FontId::Regular);
    let room = tabs_x - title_r.x - 2.0 * pad - hw - 24.0 * s;
    let show_hint = room > 160.0 * s;
    let title_w = if show_hint { room } else { tabs_x - title_r.x - 2.0 * pad };
    let title = ellipsize_middle(g, &title, title_w.max(0.0), p, FontId::Semibold);
    g.text_in(&title, Rect::new(title_r.x + pad, title_r.y, title_r.w, title_r.h), p, FontId::Semibold, th.text, Align::Left);
    if show_hint {
        g.text_in(&hint, Rect::new(title_r.x, title_r.y, tabs_x - title_r.x - 12.0 * s, title_r.h), p, FontId::Regular, th.dim, Align::Right);
    }
    if f.kind != ChartType::Treemap {
        return draw_chart(g, app, l, th, s, f);
    }
    // Treemap.
    match map {
        Some(m) if !m.tiles.is_empty() => {
            // Stretched while the window is being resized.
            g.draw_image(&m.image, l.map);
            let (sx, sy) = (l.map.w / m.w as f32, l.map.h / m.h as f32);
            let outline = |g: &mut Gfx, id: Id, c: Color, inner: Color| {
                if let Some(tile) = m.tile_of(t, id) {
                    let (x0, y0) = ((tile.x0 * sx).round(), (tile.y0 * sy).round());
                    let (x1, y1) = ((tile.x1 * sx).round(), (tile.y1 * sy).round());
                    let r = Rect::new(l.map.x + x0, l.map.y + y0, (x1 - x0).max(2.0), (y1 - y0).max(2.0));
                    g.stroke_rrect(r, 0.0, (2.0 * s).round(), c);
                    g.stroke_rrect(r.inset((2.0 * s).round()), 0.0, 1.0, inner);
                }
            };
            if let Some(h) = hover.filter(|&h| Some(h) != app.sel) {
                outline(g, h, Color::rgba(0xFFFFFF, 0xB0), Color::rgba(0x000000, 0x40));
            }
            if let Some(sel) = app.sel {
                outline(g, sel, Color::rgb(0xFFFFFF), Color::rgb(0x000000));
            }
        }
        _ => {
            g.text_in(&tr("Nothing here takes up space."), l.map, p, FontId::Regular, th.dim, Align::Center);
        }
    }
}

/// Width of a chart tab, in unscaled pixels: room for the longest name in
/// the ten languages.
fn tab_w(kind: ChartType) -> f32 {
    let n = chart_name(kind).chars().count() as f32;
    (n * 7.0 + 22.0).clamp(56.0, 140.0)
}

/// The name of a chart on its tab.
pub fn chart_name(kind: ChartType) -> String {
    match kind {
        ChartType::Treemap => tr("Treemap"),
        ChartType::Pie => tr("Pie"),
        ChartType::Rings => tr("Rings"),
    }
}

fn sector_at(c: &Chart, seg: &Segment, ox: f32, oy: f32) -> Vec<(f32, f32)> {
    sector(c.cx + ox, c.cy + oy, seg.r0, seg.r1, seg.a0, seg.a1)
}

/// The pie or the rings, the pointed segment moved out, the selection and
/// the texts.
fn draw_chart(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, f: &Frame) {
    let (Some(scan), Some(c)) = (&app.scan, f.chart) else { return };
    let t = &scan.tree;
    let p = px(s);
    let (x0, y0) = (l.map.x, l.map.y);
    g.draw_image(&c.image, Rect::new(x0, y0, c.w as f32, c.h as f32));
    if c.segments.is_empty() {
        g.text_in(&tr("Nothing here takes up space."), l.map, p, FontId::Regular, th.dim, Align::Center);
        return;
    }
    let sel = app.sel.and_then(|id| c.segment_of(t, id));
    let hover = match f.hover_seg {
        Some(Hit::Segment(i)) => Some(i),
        _ => None,
    };
    // The pointed segment, and what is inside it, stands out.
    let moved = match hover {
        Some(h) if c.kind == ChartKind::Rings => c.subtree(h),
        Some(h) => vec![h],
        None => Vec::new(),
    };
    let (dx, dy) = hover.map_or((0.0, 0.0), |h| pop_offset(&c.segments[h], c.radius));
    for &i in &moved {
        let pts = sector_at(c, &c.segments[i], x0, y0);
        g.fill_polygon(&pts, th.bg);
        g.polyline(&pts, 2.0, th.bg, true);
    }
    for &i in &moved {
        let seg = &c.segments[i];
        let pts = sector_at(c, seg, x0 + dx, y0 + dy);
        let color = Color::rgb(seg.color).mix(Color::rgb(0xFFFFFF), if Some(i) == hover { 0.22 } else { 0.1 });
        g.fill_polygon(&pts, color);
        g.polyline(&pts, 1.2 * s, th.bg, true);
    }
    if let Some(i) = sel {
        let (ox, oy) = if moved.contains(&i) { (dx, dy) } else { (0.0, 0.0) };
        let pts = sector_at(c, &c.segments[i], x0 + ox, y0 + oy);
        g.polyline(&pts, (2.5 * s).round(), th.text, true);
    }
    match c.kind {
        ChartKind::Rings => {
            // The folder shown, in the middle.
            let r = c.inner * 0.8;
            let box_ = Rect::new(x0 + c.cx - r, y0 + c.cy - r * 0.55, 2.0 * r, r * 1.1);
            // The scanned folder's name is its whole path: show its last part.
            let full = t.name(c.root);
            let name = std::path::Path::new(full).file_name().map_or(full.to_string(), |n| n.to_string_lossy().into_owned());
            let name = ellipsize(g, &name, box_.w, p, FontId::Semibold);
            let size = format::size(t.size(c.root));
            let hot = f.hover_seg == Some(Hit::Center);
            let color = if hot { th.accent } else { th.text };
            g.text_in(&name, Rect::new(box_.x, box_.y, box_.w, box_.h / 2.0), p, FontId::Semibold, color, Align::Center);
            g.text_in(&size, Rect::new(box_.x, box_.y + box_.h / 2.0, box_.w, box_.h / 2.0), p * 0.95, FontId::Regular, th.dim, Align::Center);
        }
        ChartKind::Pie => {
            let total = t.size(c.root).max(1);
            for &(i, r) in &c.legend {
                let r = Rect::new(x0 + r.x, y0 + r.y, r.w, r.h);
                let seg = &c.segments[i];
                if hover == Some(i) {
                    g.fill_rrect(r, (4.0 * s).round(), th.hover);
                }
                if sel == Some(i) {
                    g.fill_rrect(r, (4.0 * s).round(), th.selection);
                }
                let sw = (12.0 * s).round();
                g.fill_rrect(Rect::new(r.x + 6.0 * s, r.center_y() - sw / 2.0, sw, sw), 3.0 * s, Color::rgb(seg.color));
                let size = format::size(seg.size);
                let pct = format::percent(seg.size, total);
                let (cw1, cw2) = ((72.0 * s).round(), (58.0 * s).round());
                let name_x = r.x + 6.0 * s + sw + 8.0 * s;
                let name_w = (r.right() - cw1 - cw2 - name_x - 8.0 * s).max(0.0);
                let name = match seg.node {
                    Some(id) => t.name(id).to_string(),
                    None => tf("Other ({} items)", &[&format::count(seg.others as u64)]),
                };
                let name = ellipsize(g, &name, name_w, p, FontId::Regular);
                g.text_in(&name, Rect::new(name_x, r.y, name_w, r.h), p, FontId::Regular, th.text, Align::Left);
                g.text_in(&size, Rect::new(r.right() - cw1 - cw2, r.y, cw1, r.h), p, FontId::Regular, th.text, Align::Right);
                g.text_in(&pct, Rect::new(r.right() - cw2, r.y, cw2 - 4.0 * s, r.h), p, FontId::Regular, th.dim, Align::Right);
            }
        }
    }
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

fn draw_status(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hover: Option<Id>) {
    let st = l.status;
    g.fill_rect(st, th.status);
    g.fill_rect(Rect::new(st.x, st.y, st.w, 1.0), th.border);
    let p = 12.0 * s;
    let pad = 10.0 * s;
    let (left, right) = status_text(app, hover);
    let rw = g.measure(&right, p, FontId::Regular);
    g.text_in(&right, Rect::new(st.right() - rw - pad, st.y, rw, st.h), p, FontId::Regular, th.dim, Align::Left);
    let lw = (st.w - rw - 3.0 * pad).max(0.0);
    let left = ellipsize_middle(g, &left, lw, p, FontId::Regular);
    g.text_in(&left, Rect::new(st.x + pad, st.y, lw, st.h), p, FontId::Regular, th.text, Align::Left);
}

/// The status bar: (what the pointer or the selection is on, the totals).
pub fn status_text(app: &App, hover: Option<Id>) -> (String, String) {
    if page(app) != Page::Result {
        return (String::new(), String::new());
    }
    let Some(scan) = &app.scan else { return (String::new(), String::new()) };
    let t = &scan.tree;
    let left = hover.or(app.sel).map_or(String::new(), |id| {
        let n = t.node(id);
        let mut text = format!("{}    {}", t.path(id).display(), format::size(t.size(id)));
        if n.kind == Kind::File && n.ext != 0 {
            text = format!("{text}    .{}", t.ext_name(n.ext));
        }
        text
    });
    let root = t.node(t.root());
    let mut parts = vec![
        tf("{} files", &[&format::count(root.files)]),
        tf("{} folders", &[&format::count(root.dirs)]),
        tf("read in {} s", &[&format::decimal(scan.elapsed.as_secs_f64(), 1)]),
    ];
    let bad = t.unreadable();
    if bad > 0 {
        parts.push(tf("{} folders could not be read", &[&format::count(bad as u64)]));
    }
    if !scan.complete {
        parts.push(tr("stopped before the end"));
    }
    (left, parts.join(" · "))
}

#[cfg(test)]
mod screenshots {
    use super::*;
    use crate::app::Scanning;
    use crate::i18n::{set_lang, Lang};
    use crate::volumes::Volume;
    use std::path::PathBuf;

    /// Draws each page to `target/screens/*.png` for a visual check:
    /// `cargo test -p rsize --lib screenshots -- --ignored`. The result
    /// pages show `RSIZE_SHOT_DIR` (default: the repository).
    #[test]
    #[ignore]
    fn screenshots() {
        let Ok(mut g) = Gfx::new() else { return };
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&out).unwrap();
        let dir = std::env::var_os("RSIZE_SHOT_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
        let scan = crate::scan::scan(&dir, &std::sync::atomic::AtomicBool::new(false), &|_| {}).unwrap();
        let (w, h) = (1100u32, 760u32);
        let full = Rect::new(0.0, 0.0, w as f32, h as f32);
        let gb = 1u64 << 30;
        let vols = vec![
            Volume { path: "C:\\".into(), label: "Windows".into(), fs: "NTFS".into(), total: 451 * gb, free: 38 * gb, remote: false },
            Volume { path: "D:\\".into(), label: "Data".into(), fs: "NTFS".into(), total: 480 * gb, free: 210 * gb, remote: false },
            Volume { path: "T:\\".into(), label: "Backup".into(), fs: "exFAT".into(), total: 931 * gb, free: 702 * gb, remote: false },
            Volume { path: "Z:\\".into(), label: String::new(), fs: "NTFS".into(), total: 2048 * gb, free: 1024 * gb, remote: true },
        ];
        let many: Vec<Volume> = (0..14u8)
            .map(|i| Volume { path: format!("{}:\\", (b'C' + i) as char).into(), label: format!("Disk {i}"), fs: "NTFS".into(), total: 500 * gb, free: (i as u64 * 30) * gb, remote: i > 9 })
            .collect();
        let mut chart_shot = |name: &str, app: &App, dark: bool, kind: ChartType, pointed: Option<usize>| {
            g.resize(w, h);
            let th = Theme::new(dark, None);
            let l = layout(app, 0.45, full, 1.0, false);
            let (mw, mh) = (l.map.w as u32, l.map.h as u32);
            let bg = (th.bg.r as u32) << 16 | (th.bg.g as u32) << 8 | th.bg.b as u32;
            let tree = &app.scan.as_ref().map(|s| &s.tree);
            let map = tree.map(|t| Map::new(t, app.zoom, mw, mh));
            let chart = tree.map(|t| match kind {
                ChartType::Pie => Chart::pie(t, app.zoom, mw, mh, 1.0, bg),
                _ => Chart::rings(t, app.zoom, mw, mh, 1.0, bg),
            });
            let hover = pointed.and_then(|i| chart.as_ref()?.segments.get(i)?.node);
            let hot = if page(app) == Page::Start { Hot::Card(1) } else { Hot::Tab(ChartType::Pie) };
            let f = Frame { mode: SizeMode::Logical, kind, map: map.as_ref(), chart: chart.as_ref(), hover_seg: pointed.map(Hit::Segment), hot, hover };
            draw(&mut g, app, &l, &th, 1.0, &f);
            g.pix.save_png(out.join(format!("{name}.png"))).unwrap();
        };
        let mut shot = |name: &str, app: &App, dark: bool, _: Option<Id>| chart_shot(name, app, dark, ChartType::Treemap, None);
        set_lang(Lang::EnUS);
        let mut app = App::new();
        app.volumes = Some(vols);
        shot("start-light", &app, false, None);
        shot("start-dark", &app, true, None);
        let mut crowded = App::new();
        crowded.volumes = Some(many);
        shot("start-many", &crowded, false, None);
        crowded.start_scroll = 10_000.0;
        shot("start-many-end", &crowded, false, None);
        app.scanning = Some(Scanning {
            root: "C:\\".into(),
            progress: crate::scan::Progress { files: 1_234_567, dirs: 98_765, bytes: 312 * gb, errors: 3, current: "C:\\Windows\\WinSxS\\amd64_microsoft-windows-some-very-long-component-name_31bf3856ad364e35_10.0.19041.1_none".into() },
            started: std::time::Instant::now(),
            generation: 1,
        });
        shot("scanning", &app, false, None);
        app.scanning = None;
        app.set_scan(scan, SizeMode::Logical);
        let first = app.rows.get(1).map(|r| r.0);
        if let Some(f) = first {
            app.toggle(f);
            if let Some(&(c, _)) = app.rows.get(2) {
                app.select(c);
            }
        }
        shot("result-light", &app, false, None);
        shot("result-dark", &app, true, first);
        set_lang(Lang::PtPT);
        shot("result-pt", &app, false, None);
        set_lang(Lang::EnUS);
        // The round charts, of a folder with varied contents when there is one.
        let varied = {
            let t = &app.scan.as_ref().unwrap().tree;
            let find = |from: Id, name: &str| t.children(from).iter().copied().find(|&c| t.name(c) == name);
            find(0, "target").and_then(|d| find(d, "debug")).map(|d| (d, t.children(d)[0]))
        };
        if let Some((debug, first)) = varied {
            app.zoom = debug;
            app.select(first);
        }
        drop(shot);
        chart_shot("pie-light", &app, false, ChartType::Pie, Some(1));
        chart_shot("pie-dark", &app, true, ChartType::Pie, None);
        chart_shot("rings-light", &app, false, ChartType::Rings, Some(0));
        chart_shot("rings-dark", &app, true, ChartType::Rings, Some(3));
    }
}
