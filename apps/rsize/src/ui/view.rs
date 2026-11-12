//! The three pages of the window, drawn each frame:
//!
//! - the start page: the drives, with their used space, and a button to
//!   choose a folder;
//! - the scan page: what has been found so far;
//! - the result: what was read and the volume's space on top; the folder
//!   list at the left and the file types at the right, split by a bar that
//!   can be dragged; the treemap (or the pie, or the rings) below, split
//!   from them by another bar.
//!
//! [`layout`] decides where things go; `main` uses the same [`Layout`] to
//! find what the pointer is on.

use super::gfx::{Align, Color, FontId, Gfx, Rect};
use super::theme::Theme;
use crate::app::{items, type_name, App, Col, ExtCol, Sort};
use crate::format;
use crate::i18n::{tf, tr};
use crate::model::{Id, Kind, SizeMode, Tree, F_HARDLINK, F_OTHER_FS, F_UNREADABLE};
use crate::chart::{pop_offset, sector, Chart, Hit, Kind as ChartKind, Segment};
use crate::settings::{ChartType, Panes, ViewMode};
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
    /// A header of the list (sorts it).
    Head(Col),
    /// A row of the table of file types.
    ExtRow(usize),
    /// A header of the table of file types (sorts it).
    ExtHead(ExtCol),
    Splitter,
    /// The bar between the list and the file types.
    VSplitter,
    /// A chart tab above the chart.
    Tab(ChartType),
    /// A layout button above the chart.
    Mode(ViewMode),
}

/// The columns shown, in order, as (column, x, width).
pub type Cols<C> = Vec<(C, f32, f32)>;

/// Lays out columns in `x..x + w`: `order` gives each column, its width
/// (unscaled) and how long it stays when there is little room (0: always);
/// `flex` takes what is left, at least `min_flex`.
fn columns<C: Copy + PartialEq>(x: f32, w: f32, s: f32, order: &[(C, f32, u8)], flex: C, min_flex: f32) -> Cols<C> {
    let mut shown: Vec<(C, f32, u8)> = order.to_vec();
    loop {
        let fixed: f32 = shown.iter().filter(|c| c.0 != flex).map(|c| (c.1 * s).round()).sum();
        if fixed + min_flex * s <= w {
            break;
        }
        // The column that goes first: the highest number.
        match shown.iter().enumerate().filter(|(_, c)| c.0 != flex && c.2 > 0).max_by_key(|(_, c)| c.2) {
            Some((i, _)) => {
                shown.remove(i);
            }
            None => break,
        }
    }
    let fixed: f32 = shown.iter().filter(|c| c.0 != flex).map(|c| (c.1 * s).round()).sum();
    let mut cx = x;
    shown
        .iter()
        .map(|c| {
            let cw = if c.0 == flex { (w - fixed).max(0.0) } else { (c.1 * s).round() };
            let out = (c.0, cx, cw);
            cx += cw;
            out
        })
        .collect()
}

/// Where a column is: (x, width), or None when it is hidden.
pub fn col_at<C: Copy + PartialEq>(cols: &Cols<C>, c: C) -> Option<(f32, f32)> {
    cols.iter().find(|k| k.0 == c).map(|k| (k.1, k.2))
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub full: Rect,
    /// The drawn menu bar (not on Windows).
    pub bar: Rect,
    pub status: Rect,
    pub body: Rect,
    // Result page.
    /// What was read, and the volume's space.
    pub info: Rect,
    pub header: Rect,
    pub list: Rect,
    pub row_h: f32,
    pub indent: f32,
    pub cols: Cols<Col>,
    pub track: Rect,
    /// The bar between the list and the file types.
    pub vsplit: Rect,
    pub ext_header: Rect,
    pub ext_list: Rect,
    pub ext_cols: Cols<ExtCol>,
    pub ext_track: Rect,
    pub splitter: Rect,
    pub map_title: Rect,
    /// The chart tabs, at the right of the title.
    pub tabs: Vec<(ChartType, Rect)>,
    /// The layout buttons, at the right of the chart tabs.
    pub modes: Vec<(ViewMode, Rect)>,
    /// The layout in use.
    pub view: ViewMode,
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
        let ax = self.list.x + 4.0 + depth as f32 * self.indent;
        x >= ax - 2.0 && x < ax + self.indent + 2.0
    }

    /// Rows that fit in the list.
    pub fn page_rows(&self) -> usize {
        rows_in(self.list, self.row_h)
    }

    /// Rows that fit in the table of file types.
    pub fn ext_rows(&self) -> usize {
        rows_in(self.ext_list, self.row_h)
    }

    /// The row of the table of file types under the pointer, if any.
    pub fn ext_row_at(&self, app: &App, x: f32, y: f32) -> Option<usize> {
        if !self.ext_list.contains(x, y) || self.row_h <= 0.0 {
            return None;
        }
        let i = app.types_scroll + ((y - self.ext_list.y) / self.row_h) as usize;
        (i < app.exts.len()).then_some(i)
    }

    /// The header of the list under the pointer.
    pub fn head_at(&self, x: f32, y: f32) -> Option<Col> {
        self.header.contains(x, y).then(|| self.cols.iter().find(|c| x >= c.1 && x < c.1 + c.2).map(|c| c.0)).flatten()
    }

    /// The header of the table of file types under the pointer.
    pub fn ext_head_at(&self, x: f32, y: f32) -> Option<ExtCol> {
        self.ext_header.contains(x, y).then(|| self.ext_cols.iter().find(|c| x >= c.1 && x < c.1 + c.2).map(|c| c.0)).flatten()
    }
}

fn rows_in(r: Rect, row_h: f32) -> usize {
    if row_h <= 0.0 {
        return 1;
    }
    ((r.h / row_h).floor() as usize).max(1)
}

/// The list's columns: (column, width, how long it stays).
const LIST_COLS: [(Col, f32, u8); 8] = [
    (Col::Name, 0.0, 0),
    (Col::Share, 120.0, 1),
    (Col::Size, 80.0, 2),
    (Col::Alloc, 80.0, 5),
    (Col::Items, 84.0, 6),
    (Col::Files, 84.0, 4),
    (Col::Folders, 74.0, 7),
    (Col::Modified, 90.0, 3),
];

const EXT_COLS: [(ExtCol, f32, u8); 6] =
    [(ExtCol::Ext, 116.0, 0), (ExtCol::Type, 0.0, 0), (ExtCol::Share, 110.0, 1), (ExtCol::Size, 78.0, 2), (ExtCol::Alloc, 78.0, 4), (ExtCol::Files, 70.0, 3)];

/// Where everything goes in a window of `full` at scale `s`, in the layout
/// `view`; `panes` places its bars: `split` is the share of the height for
/// the upper part, `split_x` the share of its width for the left part.
pub fn layout(app: &App, view: ViewMode, panes: Panes, full: Rect, s: f32, menu_bar: bool) -> Layout {
    let (settings_split, split_x) = (panes.split, panes.split_x);
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
            l.info = area.take_top((52.0 * s).round());
            mode_buttons(&mut l, s);
            l.view = view;
            let split = (area.h * settings_split).round();
            let mut top = area.take_top(split);
            l.splitter = area.take_top((6.0 * s).round());
            let bottom = area;
            l.row_h = (22.0 * s).round();
            l.indent = (16.0 * s).round();
            // The upper part: a left pane, the bar, the file types at the right.
            let left = top.take_left((top.w * split_x).round());
            l.vsplit = top.take_left((6.0 * s).round());
            let mut right = top;
            // Folders: the list at the left, the treemap below. Map: the chart
            // at the left, the list below.
            let (mut list, mut chart) = match view {
                ViewMode::Folders => (left, bottom),
                ViewMode::Map => (bottom, left),
            };
            l.map_title = chart.take_top((26.0 * s).round());
            title_tabs(&mut l, view, s);
            l.map = Rect::new(chart.x.round(), chart.y.round(), chart.w.round(), chart.h.round());
            let head_h = (26.0 * s).round();
            l.header = list.take_top(head_h);
            l.track = list.take_right((10.0 * s).round());
            l.list = list;
            l.cols = columns(list.x, list.w, s, &LIST_COLS, Col::Name, 170.0);
            l.ext_header = right.take_top(head_h);
            l.ext_track = right.take_right((10.0 * s).round());
            l.ext_list = right;
            l.ext_cols = columns(right.x, right.w, s, &EXT_COLS, ExtCol::Type, 70.0);
        }
    }
    l
}

/// The layout buttons, at the right of the block on top: always in the
/// same place, whatever the layout.
fn mode_buttons(l: &mut Layout, s: f32) {
    let bar = l.info;
    let h = (26.0 * s).round();
    let y = (bar.y + (bar.h - h) / 2.0).round();
    let mut x = bar.right() - 12.0 * s;
    for mode in ViewMode::ALL.into_iter().rev() {
        let w = (tab_w(&view_name(mode)) * s).round();
        x -= w;
        l.modes.push((mode, Rect::new(x.round(), y, w, h)));
        x -= 4.0 * s;
    }
    l.modes.reverse();
}

/// The chart tabs at the right of the chart's title, in the map layout.
fn title_tabs(l: &mut Layout, view: ViewMode, s: f32) {
    let bar = l.map_title;
    let (y, h) = (bar.y + 3.0 * s, bar.h - 6.0 * s);
    let mut x = bar.right() - 6.0 * s;
    if view == ViewMode::Map {
        for kind in ChartType::ALL.into_iter().rev() {
            let w = (tab_w(&chart_name(kind)) * s).round();
            x -= w;
            l.tabs.push((kind, Rect::new(x.round(), y, w, h)));
            x -= 4.0 * s;
        }
        l.tabs.reverse();
    }
}

/// The name of a layout on its button.
pub fn view_name(mode: ViewMode) -> String {
    match mode {
        ViewMode::Folders => tr("Folders"),
        ViewMode::Map => tr("Map"),
    }
}

/// What a frame shows besides the app's state.
#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub mode: SizeMode,
    /// The chart shown below the list.
    pub kind: ChartType,
    /// The treemap for `layout.map`.
    pub map: Option<&'a Map>,
    /// The treemap with the files of the picked type standing out.
    pub lit: Option<&'a tiny_skia::Pixmap>,
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
    let pad = 8.0 * s;
    draw_info(g, app, l, th, s);
    // Headers of the list.
    let heads: Vec<HeadCell> = l
        .cols
        .iter()
        .map(|&(c, x, w)| (x, w, col_title(c), col_align(c), hot == Hot::Head(c), sorted(app.sort, c)))
        .collect();
    draw_header(g, l.header, &heads, th, s);
    // Rows.
    let table = treemap::ext_colors(t);
    g.set_clip(Some(l.list));
    let end = (app.scroll + l.page_rows() + 1).min(app.rows.len());
    let name_col = col_at(&l.cols, Col::Name).unwrap_or((l.list.x, l.list.w));
    for (i, &(id, depth)) in app.rows.iter().enumerate().take(end).skip(app.scroll) {
        let y = l.list.y + (i - app.scroll) as f32 * l.row_h;
        let row = Rect::new(l.list.x, y, l.list.w, l.row_h);
        if app.sel == Some(id) {
            g.fill_rect(row, th.selection);
        } else if hot == Hot::Row(i) || (hover == Some(id) && hot == Hot::None) {
            g.fill_rect(row, th.hover);
        }
        let n = t.node(id);
        let mut x = name_col.0 + 4.0 * s + depth as f32 * l.indent;
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
        let name_w = name_col.0 + name_col.1 - x - pad;
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
        let size = n.size_in(mode);
        let whole = t.parent(id).map_or(size, |pa| t.node(pa).size_in(mode));
        let cell = |c: (f32, f32)| Rect::new(c.0 + pad, row.y, c.1 - 2.0 * pad, row.h);
        let dir = n.kind == Kind::Dir;
        for &(c, cx, cw) in &l.cols {
            let r = cell((cx, cw));
            match c {
                Col::Name => {}
                Col::Share => share_bar(g, Rect::new(cx + pad, row.y + 4.0 * s, cw - 2.0 * pad, row.h - 8.0 * s), size, whole, th, s),
                Col::Size => _ = g.text_in(&format::size(n.size), r, p, FontId::Regular, th.text, Align::Right),
                Col::Alloc => _ = g.text_in(&format::size(n.alloc), r, p, FontId::Regular, th.text, Align::Right),
                Col::Items if dir => _ = g.text_in(&format::count(items(n)), r, p, FontId::Regular, th.dim, Align::Right),
                Col::Files if dir => _ = g.text_in(&format::count(n.files), r, p, FontId::Regular, th.dim, Align::Right),
                Col::Folders if dir => _ = g.text_in(&format::count(n.dirs), r, p, FontId::Regular, th.dim, Align::Right),
                Col::Modified => _ = g.text_in(&format::date(n.mtime), r, p, FontId::Regular, th.dim, Align::Right),
                _ => {}
            }
        }
    }
    g.set_clip(None);
    // Scroll bar.
    let page = l.page_rows();
    if app.rows.len() > page {
        let (y, h) = thumb(l.track, app.rows.len(), page, app.scroll);
        g.fill_rrect(Rect::new(l.track.x + 2.0 * s, y, l.track.w - 4.0 * s, h), 3.0 * s, th.scrollbar);
    }
    draw_exts(g, app, l, th, s, f);
    // The bar between the list and the file types.
    let v = l.vsplit;
    g.fill_rect(v, if hot == Hot::VSplitter { th.accent.with_alpha(0x60) } else { th.status });
    g.fill_rect(Rect::new(v.x, v.y, 1.0, v.h), th.border);
    g.fill_rect(Rect::new(v.right() - 1.0, v.y, 1.0, v.h), th.border);
    let _ = pad;
    // Splitter.
    g.fill_rect(l.splitter, if hot == Hot::Splitter { th.accent.with_alpha(0x60) } else { th.status });
    g.fill_rect(Rect::new(l.splitter.x, l.splitter.y, l.splitter.w, 1.0), th.border);
    g.fill_rect(Rect::new(l.splitter.x, l.splitter.bottom() - 1.0, l.splitter.w, 1.0), th.border);
    // Chart title: the folder shown, a hint, and the chart tabs.
    let title_r = l.map_title;
    g.fill_rect(title_r, th.header);
    let tab = |g: &mut Gfx, r: Rect, text: &str, on: bool, hot: bool, framed: bool| {
        if on {
            g.fill_rrect(r, (4.0 * s).round(), th.accent);
        } else if hot {
            g.fill_rrect(r, (4.0 * s).round(), th.hover);
        }
        if framed && !on {
            g.stroke_rrect(r, (4.0 * s).round(), 1.0, th.border);
        }
        let fg = if on { if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) } } else { th.text };
        g.text_in(text, r, p * 0.95, FontId::Regular, fg, Align::Center);
    };
    for &(kind, r) in &l.tabs {
        tab(g, r, &chart_name(kind), kind == f.kind, hot == Hot::Tab(kind), false);
    }
    // The layouts, framed: they move the parts of the window.
    for &(mode, r) in &l.modes {
        tab(g, r, &view_name(mode), mode == l.view, hot == Hot::Mode(mode), true);
    }
    let tabs_x = l.tabs.first().map_or(title_r.right(), |t| t.1.x);
    let shown = t.path(app.zoom).to_string_lossy().into_owned();
    let title = format!("{shown} — {}", format::size(t.node(app.zoom).size_in(mode)));
    let hint = match (app.zoom != t.root(), f.kind) {
        (_, ChartType::Rings) => tr("Double-click a folder to zoom in, click the middle to zoom out"),
        (true, _) => tr("Backspace or right click: zoom out"),
        (false, ChartType::Treemap) => tr("Click a folder's header to select it, double-click to zoom in"),
        (false, _) => tr("Double-click a folder to zoom in"),
    };
    let hw = g.measure(&hint, p, FontId::Regular);
    let room = tabs_x - title_r.x - 2.0 * pad - hw - 24.0 * s;
    let show_hint = room > 160.0 * s && !hint.is_empty();
    let title_w = if show_hint { room } else { tabs_x - title_r.x - 2.0 * pad };
    let title = ellipsize_middle(g, &title, title_w.max(0.0), p, FontId::Semibold);
    g.text_in(&title, Rect::new(title_r.x + pad, title_r.y, title_r.w, title_r.h), p, FontId::Semibold, th.text, Align::Left);
    if show_hint {
        g.text_in(&hint, Rect::new(title_r.x, title_r.y, tabs_x - title_r.x - 12.0 * s, title_r.h), p, FontId::Regular, th.dim, Align::Right);
    }
    match f.kind {
        ChartType::Treemap => {}
        ChartType::Pie | ChartType::Rings => return draw_chart(g, app, l, th, s, f),
    }
    // Treemap.
    match map {
        Some(m) if !m.tiles.is_empty() => {
            // Stretched while the window is being resized.
            g.draw_image(f.lit.unwrap_or(&m.image), l.map);
            let (sx, sy) = (l.map.w / m.w as f32, l.map.h / m.h as f32);
            draw_heads(g, t, m, l, s, sx, sy, mode, hover);
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

/// The folders' headers in the treemap: name and size, on the strip the
/// treemap left for them.
#[allow(clippy::too_many_arguments)]
fn draw_heads(g: &mut Gfx, t: &Tree, m: &Map, l: &Layout, s: f32, sx: f32, sy: f32, mode: SizeMode, hover: Option<Id>) {
    let fs = 11.0 * s;
    g.set_clip(Some(l.map));
    for tile in m.tiles.iter().filter(|t| t.head > 0.0) {
        let r = Rect::new(l.map.x + tile.x0 * sx, l.map.y + tile.y0 * sy, tile.w() * sx, tile.head * sy);
        if hover == Some(tile.node) {
            g.fill_rect(Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 1.0), Color::rgba(0xFFFFFF, 0x30));
        }
        if r.w < 28.0 * s {
            continue;
        }
        // The folder shown is named by its whole path; the others by their name.
        let name = if tile.node == m.root { t.path(tile.node).to_string_lossy().into_owned() } else { t.name(tile.node).to_string() };
        let text = format!("{name} — {}", format::size(t.node(tile.node).size_in(mode)));
        let inner = Rect::new(r.x + 4.0 * s, r.y, r.w - 8.0 * s, r.h);
        let text = ellipsize(g, &text, inner.w, fs, FontId::Regular);
        g.text_in(&text, inner, fs, FontId::Regular, Color::rgb(0xF2F4F7), Align::Left);
    }
    g.set_clip(None);
}

/// Whether a column sorts its table, and which way (true: down).
fn sorted<C: PartialEq + Copy>(sort: Sort<C>, c: C) -> Option<bool> {
    (sort.col == c).then_some(sort.desc)
}

/// A column's header: x, width, title, alignment, under the pointer, sorted (down).
type HeadCell = (f32, f32, String, Align, bool, Option<bool>);

/// A header row: each column's title, the sort arrow and the hover.
fn draw_header(g: &mut Gfx, r: Rect, heads: &[HeadCell], th: &Theme, s: f32) {
    let p = px(s);
    let pad = 8.0 * s;
    g.fill_rect(r, th.header);
    g.fill_rect(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), th.border);
    g.set_clip(Some(r));
    for (x, w, text, align, hot, sort) in heads {
        let cell = Rect::new(*x, r.y, *w, r.h - 1.0);
        if *hot {
            g.fill_rect(cell, th.hover);
        }
        if *x > r.x {
            g.fill_rect(Rect::new(*x, r.y + 5.0 * s, 1.0, r.h - 10.0 * s), th.border);
        }
        let arrow_w = if sort.is_some() { 12.0 * s } else { 0.0 };
        let room = (w - 2.0 * pad - arrow_w).max(0.0);
        let text = ellipsize(g, text, room, p, FontId::Semibold);
        let tw = g.measure(&text, p, FontId::Semibold);
        let tx = match align {
            Align::Right => x + w - pad - tw,
            _ => x + pad + arrow_w,
        };
        g.text_in(&text, Rect::new(tx, r.y, tw + 1.0, r.h), p, FontId::Semibold, if sort.is_some() { th.text } else { th.dim }, Align::Left);
        if let Some(down) = sort {
            // The arrow at the side of the title away from the numbers.
            let ax = match align {
                Align::Right => tx - 8.0 * s,
                _ => x + pad + 3.0 * s,
            };
            let (cy, a) = (r.center_y(), 3.5 * s);
            let pts = if *down { [(ax - a, cy - a / 2.0), (ax, cy + a / 2.0), (ax + a, cy - a / 2.0)] } else { [(ax - a, cy + a / 2.0), (ax, cy - a / 2.0), (ax + a, cy + a / 2.0)] };
            g.polyline(&pts, 1.4 * s, th.accent, false);
        }
    }
    g.set_clip(None);
}

/// A bar for a share, with the percentage on it.
fn share_bar(g: &mut Gfx, bar: Rect, part: u64, whole: u64, th: &Theme, s: f32) {
    if bar.w <= 0.0 {
        return;
    }
    g.fill_rect(bar, th.bar_track);
    let frac = if whole > 0 { part as f32 / whole as f32 } else { 0.0 };
    g.fill_rect(Rect::new(bar.x, bar.y, (bar.w * frac.min(1.0)).round(), bar.h), th.bar);
    g.text_in(&format::percent(part, whole), bar.inset(4.0 * s), px(s) * 0.92, FontId::Regular, th.text, Align::Right);
}

pub fn col_title(c: Col) -> String {
    match c {
        Col::Name => tr("Name"),
        Col::Share => tr("Share of folder"),
        Col::Size => tr("Size"),
        Col::Alloc => tr("Allocated"),
        Col::Items => tr("Items"),
        Col::Files => tr("Files"),
        Col::Folders => tr("Folders"),
        Col::Modified => tr("Modified"),
    }
}

fn col_align(c: Col) -> Align {
    match c {
        Col::Name | Col::Share => Align::Left,
        _ => Align::Right,
    }
}

pub fn ext_title(c: ExtCol) -> String {
    match c {
        ExtCol::Ext => tr("Extension"),
        ExtCol::Type => tr("File type"),
        ExtCol::Share => tr("Percent"),
        ExtCol::Size => tr("Size"),
        ExtCol::Alloc => tr("Allocated"),
        ExtCol::Files => tr("Files"),
    }
}

fn ext_align(c: ExtCol) -> Align {
    match c {
        ExtCol::Ext | ExtCol::Type | ExtCol::Share => Align::Left,
        _ => Align::Right,
    }
}

/// The block on top: what was read, how, and the volume's space.
fn draw_info(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32) {
    let Some(scan) = &app.scan else { return };
    let t = &scan.tree;
    let r = l.info;
    g.fill_rect(r, th.panel);
    g.fill_rect(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), th.border);
    let p = px(s);
    let root = t.node(t.root());
    let mut folder = t.name(t.root()).to_string();
    if let Some(sp) = app.space.as_ref().filter(|sp| !sp.label.is_empty()) {
        folder = format!("{folder}  ({})", sp.label);
    }
    let how = match scan.method {
        crate::scan::Method::Ntfs => tr("from the NTFS table"),
        crate::scan::Method::Everything => tr("from the Everything index"),
        crate::scan::Method::System => tr("folder by folder"),
    };
    let time = format!("{} · {how}", tf("{} s", &[&format::decimal(scan.elapsed.as_secs_f64(), 1)]));
    let counts = format!("{} · {}", tf("{} files", &[&format::count(root.files)]), tf("{} folders", &[&format::count(root.dirs)]));
    let dash = || "—".to_string();
    let space = app.space.as_ref();
    let (total, used, reserved, free) = match space {
        Some(sp) if sp.total > 0 => (
            format::size(sp.total),
            format!("{} ({})", format::size(sp.used()), format::percent(sp.used(), sp.total)),
            format::size(sp.reserved()),
            format!("{} ({})", format::size(sp.free), format::percent(sp.free, sp.total)),
        ),
        _ => (dash(), dash(), dash(), dash()),
    };
    let rows: [Vec<(String, String, f32)>; 2] = [
        vec![(tr("Folder"), folder, 2.0), (tr("Scan time"), time, 1.0), (tr("Contents"), counts, 1.0)],
        vec![(tr("Total space"), total, 1.0), (tr("Used space"), used, 1.0), (tr("Reserved space"), reserved, 1.0), (tr("Free space"), free, 1.0)],
    ];
    let pad = 12.0 * s;
    // The figures leave room for the layout buttons at the right.
    let text_w = l.modes.first().map_or(r.w, |m| m.1.x - r.x - 12.0 * s);
    let col_w = (text_w - 2.0 * pad) / 4.0;
    let line_h = (r.h - 6.0 * s) / 2.0;
    for (ri, row) in rows.iter().enumerate() {
        let y = r.y + 3.0 * s + ri as f32 * line_h;
        let mut x = r.x + pad;
        for (label, value, span) in row {
            let w = col_w * span;
            let lw = g.measure(label, p, FontId::Regular);
            g.text_in(label, Rect::new(x, y, lw + 1.0, line_h), p, FontId::Regular, th.dim, Align::Left);
            let vx = x + lw + 8.0 * s;
            let room = (x + w - vx - 12.0 * s).max(0.0);
            // A path loses its middle; the other values their end.
            let path = ri == 0 && x == r.x + pad;
            let value = if path { ellipsize_middle(g, value, room, p, FontId::Semibold) } else { ellipsize(g, value, room, p, FontId::Semibold) };
            g.text_in(&value, Rect::new(vx, y, room, line_h), p, FontId::Semibold, th.text, Align::Left);
            x += w;
        }
    }
    // How full the volume is, under the space figures.
    if let Some(sp) = space.filter(|sp| sp.total > 0) {
        let bar = Rect::new(r.x + pad, r.bottom() - 4.0 * s, text_w - 2.0 * pad, (2.0 * s).max(1.0));
        g.fill_rect(bar, th.bar_track);
        let frac = sp.used() as f32 / sp.total as f32;
        let fill = if frac > 0.9 { Color::rgb(0xD9534F) } else { th.accent };
        g.fill_rect(Rect::new(bar.x, bar.y, (bar.w * frac).round(), bar.h), fill);
    }
}

/// Width of a chart tab, in unscaled pixels: room for the longest name in
/// the ten languages.
fn tab_w(name: &str) -> f32 {
    let n = name.chars().count() as f32;
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

/// The table of file types: one row per extension below the folder the
/// chart shows.
fn draw_exts(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, f: &Frame) {
    let Some(scan) = &app.scan else { return };
    let t = &scan.tree;
    let p = px(s);
    let pad = 8.0 * s;
    let heads: Vec<HeadCell> = l
        .ext_cols
        .iter()
        .map(|&(c, x, w)| (x, w, ext_title(c), ext_align(c), f.hot == Hot::ExtHead(c), sorted(app.ext_sort, c)))
        .collect();
    draw_header(g, l.ext_header, &heads, th, s);
    let rows = l.ext_list;
    if app.exts.is_empty() {
        g.text_in(&tr("Nothing here takes up space."), rows, p, FontId::Regular, th.dim, Align::Center);
        return;
    }
    let total = t.node(app.zoom).size_in(f.mode).max(1);
    let table = treemap::ext_colors(t);
    let fit = l.ext_rows();
    g.set_clip(Some(rows));
    for (i, (ext, st)) in app.exts.iter().enumerate().skip(app.types_scroll).take(fit + 1) {
        let y = rows.y + (i - app.types_scroll) as f32 * l.row_h;
        let row = Rect::new(rows.x, y, rows.w, l.row_h);
        if app.ext_sel == Some(*ext) {
            g.fill_rect(row, th.selection);
        } else if f.hot == Hot::ExtRow(i) {
            g.fill_rect(row, th.hover);
        }
        let size = match f.mode {
            SizeMode::Logical => st.size,
            SizeMode::Allocated => st.alloc,
        };
        let cell = |x: f32, w: f32| Rect::new(x + pad, row.y, w - 2.0 * pad, row.h);
        for &(c, cx, cw) in &l.ext_cols {
            match c {
                ExtCol::Ext => {
                    let color = match table.get(*ext as usize) {
                        Some(&k) if (k as usize) < treemap::PALETTE.len() && *ext != 0 => treemap::PALETTE[k as usize],
                        _ => treemap::OTHER,
                    };
                    let sw = (12.0 * s).round();
                    let sx = cx + pad;
                    g.fill_rrect(Rect::new(sx, row.center_y() - sw / 2.0, sw, sw), 2.0 * s, Color::rgb(color));
                    let name = if *ext == 0 { tr("(no extension)") } else { format!(".{}", t.ext_name(*ext)) };
                    let nx = sx + sw + 6.0 * s;
                    let name = ellipsize(g, &name, (cx + cw - nx - pad).max(0.0), p, FontId::Regular);
                    g.text_in(&name, Rect::new(nx, row.y, cx + cw - nx, row.h), p, FontId::Regular, th.text, Align::Left);
                }
                ExtCol::Type => {
                    let name = type_name(&app.type_names, t.ext_name(*ext));
                    let r = cell(cx, cw);
                    let name = ellipsize(g, &name, r.w.max(0.0), p, FontId::Regular);
                    g.text_in(&name, r, p, FontId::Regular, th.dim, Align::Left);
                }
                ExtCol::Share => share_bar(g, Rect::new(cx + pad, row.y + 4.0 * s, cw - 2.0 * pad, row.h - 8.0 * s), size, total, th, s),
                ExtCol::Size => _ = g.text_in(&format::size(st.size), cell(cx, cw), p, FontId::Regular, th.text, Align::Right),
                ExtCol::Alloc => _ = g.text_in(&format::size(st.alloc), cell(cx, cw), p, FontId::Regular, th.text, Align::Right),
                ExtCol::Files => _ = g.text_in(&format::count(st.files), cell(cx, cw), p, FontId::Regular, th.dim, Align::Right),
            }
        }
    }
    g.set_clip(None);
    let n = app.exts.len();
    if n > fit {
        let (y, h) = thumb(l.ext_track, n, fit, app.types_scroll);
        g.fill_rrect(Rect::new(l.ext_track.x + 2.0 * s, y, l.ext_track.w - 4.0 * s, h), 3.0 * s, th.scrollbar);
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

/// The thumb of a scroll bar in `track`: (y, height).
pub fn thumb(track: Rect, rows: usize, page: usize, scroll: usize) -> (f32, f32) {
    let h = (track.h * page as f32 / rows.max(1) as f32).max(24.0f32.min(track.h));
    let max = rows.saturating_sub(page).max(1);
    let y = track.y + (track.h - h) * scroll as f32 / max as f32;
    (y, h)
}

/// The scroll position for a thumb in `track` dragged to `y` (its middle).
pub fn scroll_for(track: Rect, rows: usize, page: usize, y: f32) -> usize {
    let (_, h) = thumb(track, rows, page, 0);
    let room = (track.h - h).max(1.0);
    let frac = ((y - track.y - h / 2.0) / room).clamp(0.0, 1.0);
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
    let note = app.note.clone().filter(|_| hover.is_none());
    let left = note.unwrap_or_else(|| hover.or(app.sel).map_or(String::new(), |id| {
        let n = t.node(id);
        let mut text = format!("{}    {}", t.path(id).display(), format::size(t.size(id)));
        if n.kind == Kind::File && n.ext != 0 {
            text = format!("{text}    .{}", t.ext_name(n.ext));
        }
        text
    }));
    // What was read and how is in the block on top; here, what to know about it.
    let mut parts = Vec::new();
    if scan.admin_would_help {
        parts.push(tr("faster as administrator (File menu)"));
    }
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
        let mut chart_shot = |name: &str, app: &App, dark: bool, kind: ChartType, pointed: Option<usize>, over: Option<Id>| {
            g.resize(w, h);
            let th = Theme::new(dark, None);
            // The pie and the rings are in the map layout, and so is the
            // treemap of a picked file type.
            let map_view = kind != ChartType::Treemap || app.ext_sel.is_some();
            let d = crate::settings::Settings::default();
            let l = if map_view { layout(app, ViewMode::Map, d.map, full, 1.0, false) } else { layout(app, ViewMode::Folders, d.folders, full, 1.0, false) };
            let (mw, mh) = (l.map.w as u32, l.map.h as u32);
            let bg = (th.bg.r as u32) << 16 | (th.bg.g as u32) << 8 | th.bg.b as u32;
            let tree = &app.scan.as_ref().map(|s| &s.tree);
            let map = tree.map(|t| Map::new(t, app.zoom, mw, mh, 16.0));
            let chart = tree.map(|t| match kind {
                ChartType::Pie => Chart::pie(t, app.zoom, mw, mh, 1.0, bg),
                _ => Chart::rings(t, app.zoom, mw, mh, 1.0, bg),
            });
            let hover = pointed.and_then(|i| chart.as_ref()?.segments.get(i)?.node).or(over);
            let hot = if page(app) == Page::Start { Hot::Card(1) } else { Hot::Tab(ChartType::Pie) };
            let lit = map.as_ref().zip(tree.as_ref()).zip(app.ext_sel).map(|((m, t), e)| m.highlighted(t, e));
            let f = Frame { mode: SizeMode::Logical, kind, map: map.as_ref(), lit: lit.as_ref(), chart: chart.as_ref(), hover_seg: pointed.map(Hit::Segment), hot, hover };
            draw(&mut g, app, &l, &th, 1.0, &f);
            g.pix.save_png(out.join(format!("{name}.png"))).unwrap();
        };
        let mut shot = |name: &str, app: &App, dark: bool, over: Option<Id>| chart_shot(name, app, dark, ChartType::Treemap, None, over);
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
        app.space = crate::volumes::space_of(&dir);
        #[cfg(windows)]
        {
            let t = &app.scan.as_ref().unwrap().tree;
            let exts: Vec<String> = t.ext_ranking().iter().map(|&e| t.ext_name(e).to_string()).collect();
            app.type_names = exts.into_iter().filter_map(|e| Some((e.clone(), rt_sys::win::file_type_name(&e)?))).collect();
            app.sort_exts();
        }
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
        app.refresh_exts();
        chart_shot("pie-light", &app, false, ChartType::Pie, Some(1), None);
        chart_shot("pie-dark", &app, true, ChartType::Pie, None, None);
        chart_shot("rings-light", &app, false, ChartType::Rings, Some(0), None);
        chart_shot("rings-dark", &app, true, ChartType::Rings, Some(3), None);
        // Sorted by name, from Z to A; the file types by how many files.
        app.zoom = 0;
        app.refresh_exts();
        app.sort_by(crate::app::Col::Name);
        app.sort_by(crate::app::Col::Name);
        app.sort_exts_by(crate::app::ExtCol::Files);
        app.note = Some("“old.log” was moved to the Recycle Bin.".into());
        chart_shot("sorted-light", &app, false, ChartType::Treemap, None, None);
        set_lang(Lang::PtPT);
        chart_shot("sorted-pt-dark", &app, true, ChartType::Treemap, None, None);
        set_lang(Lang::EsES);
        chart_shot("sorted-es", &app, false, ChartType::Treemap, None, None);
        set_lang(Lang::EnUS);
        // The map layout with the largest file type picked: its files stand out.
        app.sort_exts_by(crate::app::ExtCol::Share);
        app.ext_sel = app.exts.first().map(|e| e.0);
        chart_shot("map-lit-dark", &app, true, ChartType::Treemap, None, None);
        set_lang(Lang::PtPT);
        chart_shot("map-lit-pt", &app, false, ChartType::Treemap, None, None);
        set_lang(Lang::EnUS);
    }
}
