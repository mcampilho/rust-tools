//! The pages of the window, drawn each frame:
//!
//! - the browser: where it is (the drives, a folder on disk, or inside an
//!   archive), the list, and the bar of what can be done there;
//! - the work page: opening, extracting, testing or hashing, how far it got.
//!
//! [`layout`] decides where things go; `main` uses the same [`Layout`] to
//! find what the pointer is on.

use super::gfx::{Align, Color, FontId, Gfx, Rect};
use super::theme::Theme;
use crate::app::{AddForm, App, Busy, Col, Place, Row, Task};
use crate::create::{Kind as NewKind, Level, ZipMethod};
use crate::format;
use crate::i18n::{tf, tr};
use rt_core::i18n::strip_mnemonic;
use rt_gfx::dialog::{ellipsize, ellipsize_middle};

/// What the pointer is on, for hover effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Hot {
    #[default]
    None,
    /// Work page: the button to stop.
    Stop,
    /// The button up, a place of the path (index into the trail).
    Up,
    Crumb(usize),
    Header(Col),
    Row(usize),
    Hashes,
    Test,
    Change,
    Extract,
    /// The button that opens the "Add to archive" form.
    Add,
    /// In the form: the name, the folder's Change button, a kind, a level, a
    /// zip method, the solid and "each" ticks, Cancel, Create, and elsewhere on it.
    FormName,
    FormFolder,
    FormKind(usize),
    FormLevel(usize),
    FormMethod(usize),
    FormSolid,
    FormEach,
    FormCancel,
    FormOk,
    FormPanel,
}

impl Hot {
    pub fn clickable(self) -> bool {
        !matches!(self, Hot::None | Hot::Row(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Busy,
    Browse,
}

pub fn page(app: &App) -> Page {
    if app.busy.is_some() {
        Page::Busy
    } else {
        Page::Browse
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub full: Rect,
    /// The drawn menu bar (not on Windows).
    pub bar: Rect,
    pub status: Rect,
    pub body: Rect,
    // Work page.
    pub title_y: f32,
    pub stop: Rect,
    pub progress: Rect,
    // Browser.
    pub crumbs: Rect,
    pub up: Rect,
    /// The places of the path, by their place in the trail.
    pub crumb: Vec<Rect>,
    pub header: Rect,
    /// Columns as (column, x, width).
    pub cols: Vec<(Col, f32, f32)>,
    pub list: Rect,
    pub row_h: f32,
    pub track: Rect,
    pub actions: Rect,
    pub hashes: Rect,
    pub test: Rect,
    /// The text in the middle of the bar (where to extract, or a hint).
    pub dest: Rect,
    pub change: Rect,
    pub extract: Rect,
    pub add: Rect,
    /// The "Add to archive" form, when open.
    pub form: Option<FormLayout>,
}

/// Where the parts of the "Add to archive" form go.
#[derive(Clone, Debug, Default)]
pub struct FormLayout {
    pub panel: Rect,
    pub title_y: f32,
    /// The labels of the rows (name, folder, format, level, method).
    pub labels: Vec<(Rect, &'static str)>,
    pub name: Rect,
    pub folder: Rect,
    pub change: Rect,
    pub kinds: Vec<Rect>,
    pub levels: Vec<Rect>,
    pub methods: Vec<Rect>,
    pub solid: Rect,
    pub each: Rect,
    pub cancel: Rect,
    pub ok: Rect,
}

impl FormLayout {
    pub fn hot_at(&self, x: f32, y: f32) -> Hot {
        let at = |v: &[Rect]| v.iter().position(|r| r.contains(x, y));
        if self.name.contains(x, y) {
            Hot::FormName
        } else if self.change.contains(x, y) {
            Hot::FormFolder
        } else if let Some(i) = at(&self.kinds) {
            Hot::FormKind(i)
        } else if let Some(i) = at(&self.levels) {
            Hot::FormLevel(i)
        } else if let Some(i) = at(&self.methods) {
            Hot::FormMethod(i)
        } else if self.solid.contains(x, y) {
            Hot::FormSolid
        } else if self.each.contains(x, y) {
            Hot::FormEach
        } else if self.cancel.contains(x, y) {
            Hot::FormCancel
        } else if self.ok.contains(x, y) {
            Hot::FormOk
        } else if self.panel.contains(x, y) {
            Hot::FormPanel
        } else {
            Hot::None
        }
    }
}

/// The label of a kind of archive in the form.
pub fn kind_label(k: NewKind) -> String {
    k.ext()
}

fn level_label(l: Level) -> String {
    tr(match l {
        Level::Store => "Store",
        Level::Fastest => "Fastest",
        Level::Fast => "Fast",
        Level::Normal => "Normal",
        Level::Maximum => "Maximum",
        Level::Ultra => "Ultra",
    })
}

/// Lays out the form over the window.
fn layout_form(f: &AddForm, full: Rect, s: f32) -> FormLayout {
    let mut l = FormLayout::default();
    let w = (640.0 * s).min(full.w - 32.0 * s);
    let pad = (20.0 * s).round();
    let lw = (90.0 * s).round();
    let (x, cw) = (pad, w - 2.0 * pad - lw);
    let row = (30.0 * s).round();
    let gap = (10.0 * s).round();
    let mut y = 56.0 * s;
    let push_label = |l: &mut FormLayout, y: f32, key: &'static str| l.labels.push((Rect::new(x, y, lw, row), key));
    push_label(&mut l, y, "Name");
    l.name = Rect::new(x + lw, y, cw, row);
    y += row + 6.0 * s;
    push_label(&mut l, y, "Folder");
    let bw = (110.0 * s).round();
    l.folder = Rect::new(x + lw, y, cw - bw - gap, row);
    l.change = Rect::new(x + lw + cw - bw, y, bw, row);
    y += row + gap * 1.5;
    push_label(&mut l, y, "Format");
    let chip = (78.0 * s).round();
    let per = ((cw + 4.0 * s) / (chip + 4.0 * s)).floor().max(1.0) as usize;
    for i in 0..NewKind::ALL.len() {
        let (c, r) = (i % per, i / per);
        l.kinds.push(Rect::new(x + lw + c as f32 * (chip + 4.0 * s), y + r as f32 * (row + 4.0 * s), chip, row));
    }
    y += NewKind::ALL.len().div_ceil(per) as f32 * (row + 4.0 * s) + gap;
    push_label(&mut l, y, "Level");
    let lc = ((cw - 5.0 * 4.0 * s) / 6.0).floor();
    for i in 0..Level::ALL.len() {
        l.levels.push(Rect::new(x + lw + i as f32 * (lc + 4.0 * s), y, lc, row));
    }
    y += row + gap;
    if f.spec.kind == NewKind::Zip {
        push_label(&mut l, y, "Method");
        let mc = ((cw - 3.0 * 4.0 * s) / 4.0).floor().min(120.0 * s);
        for i in 0..ZipMethod::ALL.len() {
            l.methods.push(Rect::new(x + lw + i as f32 * (mc + 4.0 * s), y, mc, row));
        }
        y += row + gap;
    }
    if f.spec.kind == NewKind::SevenZ {
        l.solid = Rect::new(x + lw, y, cw, row);
        y += row;
    }
    if f.paths.len() > 1 {
        l.each = Rect::new(x + lw, y, cw, row);
        y += row;
    }
    y += gap * 2.0;
    let bh = (34.0 * s).round();
    let bw = (130.0 * s).round();
    l.ok = Rect::new(w - pad - bw, y, bw, bh);
    l.cancel = Rect::new(l.ok.x - gap - bw, y, bw, bh);
    let h = y + bh + pad;
    let ox = (full.x + (full.w - w) / 2.0).round();
    let oy = (full.y + ((full.h - h) / 2.0).max(8.0 * s)).round();
    l.title_y = oy + 34.0 * s;
    l.panel = Rect::new(ox, oy, w, h);
    let mv = |r: &mut Rect| {
        r.x += ox;
        r.y += oy;
    };
    for r in [&mut l.name, &mut l.folder, &mut l.change, &mut l.solid, &mut l.each, &mut l.cancel, &mut l.ok] {
        mv(r);
    }
    for r in l.kinds.iter_mut().chain(&mut l.levels).chain(&mut l.methods) {
        mv(r);
    }
    for (r, _) in &mut l.labels {
        mv(r);
    }
    l
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

/// The name of the drives, as the system calls them.
pub fn drives_label() -> String {
    if cfg!(windows) {
        tr("This PC")
    } else {
        tr("Computer")
    }
}

/// The columns of the place shown, from the right: (column, width, the
/// least room left for the name to show it).
fn columns(app: &App) -> &'static [(Col, f32, f32)] {
    match app.place {
        Place::Archive => &[(Col::Method, 90.0, 520.0), (Col::Modified, 140.0, 420.0), (Col::Packed, 90.0, 300.0), (Col::Size, 90.0, 200.0)],
        Place::Folder(_) => &[(Col::Type, 150.0, 480.0), (Col::Modified, 140.0, 360.0), (Col::Size, 90.0, 200.0)],
        Place::Drives => &[(Col::FileSystem, 100.0, 420.0), (Col::Free, 100.0, 300.0), (Col::Size, 100.0, 200.0)],
    }
}

/// Where everything goes in a window of `full` at scale `s`.
pub fn layout(g: &mut Gfx, app: &App, full: Rect, s: f32, menu_bar: bool) -> Layout {
    let mut l = Layout { full, ..Layout::default() };
    let mut area = full;
    if menu_bar {
        l.bar = area.take_top((28.0 * s).round());
    }
    l.status = area.take_bottom((24.0 * s).round());
    l.body = area;
    if let Some(f) = &app.add {
        l.form = Some(layout_form(f, full, s));
    }
    if page(app) == Page::Busy {
        let w = (480.0 * s).min(area.w - 48.0 * s).max(160.0 * s);
        let x = (area.x + (area.w - w) / 2.0).round();
        let cy = area.y + area.h * 0.45;
        l.title_y = (cy - 50.0 * s).round();
        l.progress = Rect::new(x, (cy - 10.0 * s).round(), w, (8.0 * s).round());
        let bw = (140.0 * s).round();
        l.stop = Rect::new((area.x + (area.w - bw) / 2.0).round(), (cy + 70.0 * s).round(), bw, (34.0 * s).round());
        return l;
    }
    let mut top = area;
    l.crumbs = top.take_top((40.0 * s).round());
    let c = l.crumbs;
    let bh = (28.0 * s).round();
    let by = (c.y + (c.h - bh) / 2.0).round();
    l.up = Rect::new(c.x + 8.0 * s, by, bh, bh);
    let p = px(s);
    let mut x = l.up.right() + 8.0 * s;
    for (_, label) in app.trail(&drives_label()) {
        let w = (g.measure(&label, p, FontId::Semibold) + 16.0 * s).min(220.0 * s).round();
        l.crumb.push(Rect::new(x, by, w, bh));
        x += w + 14.0 * s;
    }
    // Too long for the bar: the first places give way to the last.
    let over = x - (c.right() - 12.0 * s);
    if over > 0.0 {
        for r in &mut l.crumb {
            r.x -= over;
        }
    }
    l.actions = top.take_bottom((56.0 * s).round());
    let a = l.actions;
    let (bh, gap) = ((32.0 * s).round(), (8.0 * s).round());
    let by = (a.y + (a.h - bh) / 2.0).round();
    // Narrow windows: the buttons give way before the text does.
    let w = |want: f32, share: f32| (want * s).min(a.w * share).round();
    let mut bx = a.x + 12.0 * s;
    if !app.in_archive() {
        l.add = Rect::new(bx, by, w(110.0, 0.15), bh);
        bx = l.add.right() + gap;
    }
    l.hashes = Rect::new(bx, by, w(110.0, 0.15), bh);
    l.test = Rect::new(l.hashes.right() + gap, by, w(100.0, 0.14), bh);
    l.extract = Rect::new(a.right() - 12.0 * s - w(170.0, 0.22), by, w(170.0, 0.22), bh);
    if app.in_archive() {
        l.change = Rect::new(l.extract.x - gap - w(120.0, 0.16), by, w(120.0, 0.16), bh);
    } else {
        l.change = Rect::new(l.extract.x, by, 0.0, bh);
    }
    let dx = l.test.right() + 14.0 * s;
    l.dest = Rect::new(dx, a.y, (l.change.x - 10.0 * s - dx).max(0.0), a.h);
    l.header = top.take_top((26.0 * s).round());
    l.row_h = (22.0 * s).round();
    l.track = top.take_right((10.0 * s).round());
    l.list = top;
    let mut x1 = top.right();
    for &(col, want, min_rest) in columns(app) {
        let w = (want * s).round();
        if x1 - w - top.x >= min_rest * s {
            x1 -= w;
            l.cols.push((col, x1, w));
        }
    }
    l.cols.push((Col::Name, top.x, x1 - top.x));
    l
}

/// Draws the current page.
pub fn draw(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    g.clear(th.bg);
    match page(app) {
        Page::Busy => draw_busy(g, app, l, th, s, hot),
        Page::Browse => draw_browser(g, app, l, th, s, hot),
    }
    draw_status(g, app, l, th, s, hot);
    if let (Some(f), Some(fl)) = (&app.add, &l.form) {
        draw_form(g, f, fl, l.full, th, s, hot);
    }
}

/// A rounded choice: filled when `on`, greyed when it cannot be chosen.
#[allow(clippy::too_many_arguments)]
fn chip(g: &mut Gfx, r: Rect, label: &str, th: &Theme, s: f32, on: bool, hot: bool, enabled: bool) {
    let rad = (r.h / 2.0).round();
    if on {
        g.fill_rrect(r, rad, th.accent);
    } else {
        g.fill_rrect(r, rad, if hot && enabled { th.panel.mix(th.accent, 0.10) } else { th.panel });
        g.stroke_rrect(r, rad, 1.0, if hot && enabled { th.accent } else { th.border });
    }
    let fg = match (on, th.dark) {
        (true, true) => Color::rgb(0x000000),
        (true, false) => Color::rgb(0xFFFFFF),
        _ if !enabled => th.border,
        _ => th.text,
    };
    let p = px(s) * 0.95;
    let text = ellipsize(g, label, r.w - 8.0 * s, p, FontId::Regular);
    g.text_in(&text, r, p, FontId::Regular, fg, Align::Center);
}

/// A box to tick, with its label.
fn tick(g: &mut Gfx, r: Rect, label: &str, th: &Theme, s: f32, on: bool, hot: bool) {
    let b = (18.0 * s).round();
    let bx = Rect::new(r.x, (r.center_y() - b / 2.0).round(), b, b);
    if on {
        g.fill_rrect(bx, 4.0 * s, th.accent);
        let fg = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
        let pts = [(bx.x + b * 0.24, bx.y + b * 0.52), (bx.x + b * 0.43, bx.y + b * 0.70), (bx.x + b * 0.77, bx.y + b * 0.32)];
        g.polyline(&pts, 2.0 * s, fg, false);
    } else {
        g.fill_rrect(bx, 4.0 * s, th.bg);
        g.stroke_rrect(bx, 4.0 * s, 1.5 * s, if hot { th.accent } else { th.dim });
    }
    let text = ellipsize(g, label, r.w - b - 10.0 * s, px(s), FontId::Regular);
    g.text_in(&text, Rect::new(bx.right() + 8.0 * s, r.y, r.w - b - 8.0 * s, r.h), px(s), FontId::Regular, th.text, Align::Left);
}

fn draw_form(g: &mut Gfx, f: &AddForm, l: &FormLayout, full: Rect, th: &Theme, s: f32, hot: Hot) {
    let p = px(s);
    g.fill_rect(full, Color::rgba(0x000000, if th.dark { 0x90 } else { 0x60 }));
    let panel = l.panel;
    g.fill_rrect(panel, 8.0 * s, th.bg);
    g.stroke_rrect(panel, 8.0 * s, 1.0, th.border);
    let what = if f.paths.len() == 1 {
        f.paths[0].file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())
    } else {
        tf("{} items", &[&format::count(f.paths.len() as u64)])
    };
    let title = ellipsize(g, &tf("Add {} to an archive", &[&what]), panel.w - 40.0 * s, 17.0 * s, FontId::Semibold);
    g.text(&title, panel.x + 20.0 * s, l.title_y, 17.0 * s, FontId::Semibold, th.text);
    for (r, key) in &l.labels {
        g.text_in(&tr(key), *r, p, FontId::Regular, th.dim, Align::Left);
    }
    // The name, with the caret.
    let each = f.each && f.paths.len() > 1;
    let r = l.name;
    g.fill_rrect(r, 4.0 * s, if each { th.panel } else { th.bg });
    g.stroke_rrect(r, 4.0 * s, if each { 1.0 } else { 1.5 * s }, if each { th.border } else { th.accent });
    let inner = r.inset(8.0 * s);
    let shown = if each { tr("Each one named after itself") } else { f.name.clone() };
    let (asc, desc) = g.metrics(p, FontId::Regular);
    let base = (r.center_y() + (asc + desc) / 2.0).round();
    if f.all && !each && !f.name.is_empty() {
        let w = g.measure(&f.name, p, FontId::Regular).min(inner.w);
        g.fill_rect(Rect::new(inner.x, r.y + 5.0 * s, w, r.h - 10.0 * s), th.selection);
    }
    g.set_clip(Some(inner));
    g.text(&shown, inner.x, base, p, FontId::Regular, if each { th.dim } else { th.text });
    if !each && !f.all {
        let before: String = f.name.chars().take(f.caret).collect();
        let cx = inner.x + g.measure(&before, p, FontId::Regular);
        g.fill_rect(Rect::new(cx.round(), r.y + 6.0 * s, (1.5 * s).max(1.0), r.h - 12.0 * s), th.text);
    }
    g.set_clip(None);
    let folder = ellipsize_middle(g, &f.dir.display().to_string(), l.folder.w, p, FontId::Regular);
    g.text_in(&folder, l.folder, p, FontId::Regular, th.text, Align::Left);
    button(g, l.change, &tr("Change..."), th, s, hot == Hot::FormFolder, Look::Secondary);
    let one = f.one_file();
    for (i, (r, k)) in l.kinds.iter().zip(NewKind::ALL).enumerate() {
        let enabled = !k.needs_one_file() || one;
        chip(g, *r, &kind_label(k), th, s, f.spec.kind == k, hot == Hot::FormKind(i), enabled);
    }
    let levels = f.spec.kind.has_levels();
    for (i, (r, lv)) in l.levels.iter().zip(Level::ALL).enumerate() {
        chip(g, *r, &level_label(lv), th, s, levels && f.spec.level == lv, hot == Hot::FormLevel(i), levels);
    }
    for (i, (r, m)) in l.methods.iter().zip(ZipMethod::ALL).enumerate() {
        chip(g, *r, m.label(), th, s, f.spec.zip_method == m, hot == Hot::FormMethod(i), f.spec.level != Level::Store);
    }
    if l.solid.w > 0.0 {
        tick(g, l.solid, &tr("Solid: compress the files together (smaller archive)"), th, s, f.spec.solid, hot == Hot::FormSolid);
    }
    if l.each.w > 0.0 {
        tick(g, l.each, &tr("Each item into an archive of its own"), th, s, f.each, hot == Hot::FormEach);
    }
    button(g, l.cancel, &tr("Cancel"), th, s, hot == Hot::FormCancel, Look::Secondary);
    button(g, l.ok, &tr("Create"), th, s, hot == Hot::FormOk, if f.ready() { Look::Primary } else { Look::Disabled });
}

fn px(s: f32) -> f32 {
    12.5 * s
}

/// How a button looks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Primary,
    Secondary,
    Disabled,
}

fn button(g: &mut Gfx, r: Rect, label: &str, th: &Theme, s: f32, hot: bool, look: Look) {
    if r.w <= 0.0 {
        return;
    }
    let rad = (5.0 * s).round();
    let p = 13.0 * s;
    let label = ellipsize(g, label, r.w - 10.0 * s, p, FontId::Semibold);
    match look {
        Look::Disabled => {
            g.fill_rrect(r, rad, th.panel);
            g.stroke_rrect(r, rad, 1.0, th.border);
            g.text_in(&label, r, p, FontId::Semibold, th.dim, Align::Center);
        }
        Look::Primary => {
            let bg = if hot { th.accent } else { th.accent.mix(th.bg, 0.12) };
            g.fill_rrect(r, rad, bg);
            let fg = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
            g.text_in(&label, r, p, FontId::Semibold, fg, Align::Center);
        }
        Look::Secondary => {
            g.fill_rrect(r, rad, if hot { th.panel.mix(th.accent, 0.10) } else { th.panel });
            g.stroke_rrect(r, rad, 1.0, if hot { th.accent } else { th.border });
            g.text_in(&label, r, p, FontId::Semibold, th.text, Align::Center);
        }
    }
}

fn folder_icon(g: &mut Gfx, x: f32, cy: f32, s: f32) {
    let icon = (14.0 * s).round();
    let c = Color::rgb(0xE8B84A);
    g.fill_rrect(Rect::new(x, cy - icon * 0.45, icon * 0.5, icon * 0.3), 1.5 * s, c);
    g.fill_rrect(Rect::new(x, cy - icon * 0.3, icon, icon * 0.72), 2.0 * s, c);
}

fn file_icon(g: &mut Gfx, x: f32, cy: f32, s: f32, th: &Theme) {
    let (w, h) = ((11.0 * s).round(), (14.0 * s).round());
    let r = Rect::new(x + 1.5 * s, (cy - h / 2.0).round(), w, h);
    g.fill_rrect(r, 1.5 * s, th.panel);
    g.stroke_rrect(r, 1.5 * s, 1.0, th.dim);
}

/// An archive on disk: a parcel with the band of a zip.
fn archive_icon(g: &mut Gfx, x: f32, cy: f32, s: f32) {
    let (w, h) = ((13.0 * s).round(), (13.0 * s).round());
    let r = Rect::new(x + 0.5 * s, (cy - h / 2.0).round(), w, h);
    g.fill_rrect(r, 1.5 * s, Color::rgb(0xE9A24C));
    g.fill_rect(Rect::new((r.x + w / 2.0 - 1.5 * s).round(), r.y, (3.0 * s).round(), h), Color::rgb(0x2B3545));
}

/// A drive: a flat box with a light.
fn drive_icon(g: &mut Gfx, x: f32, cy: f32, s: f32, th: &Theme) {
    let r = Rect::new(x, (cy - 4.5 * s).round(), (15.0 * s).round(), (9.0 * s).round());
    g.fill_rrect(r, 2.0 * s, th.dim);
    g.fill_circle(r.right() - 3.5 * s, r.center_y(), 1.3 * s, Color::rgb(0x6FD08C));
}

/// The title of the work page.
pub fn busy_title(b: &Busy) -> String {
    let name = b.path.file_name().map_or_else(|| b.path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let what = if b.count > 1 { tf("{} and {} more", &[&name, &format::count(b.count as u64 - 1)]) } else { name };
    match b.task {
        Task::Opening => tf("Opening {}", &[&what]),
        Task::Extracting => tf("Extracting {}", &[&what]),
        Task::Testing => tf("Testing {}", &[&what]),
        Task::Hashing => tf("Calculating the hashes of {}", &[&what]),
        Task::Creating => tf("Compressing {}", &[&what]),
    }
}

fn draw_busy(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    use std::sync::atomic::Ordering::Relaxed;
    let Some(b) = &app.busy else { return };
    let w = l.progress.w;
    let title = ellipsize(g, &busy_title(b), w, 18.0 * s, FontId::Semibold);
    g.text_in(&title, Rect::new(l.progress.x, l.title_y - 24.0 * s, w, 30.0 * s), 18.0 * s, FontId::Semibold, th.text, Align::Center);
    let bar = l.progress;
    g.fill_rrect(bar, bar.h / 2.0, th.bar_track);
    match b.fraction() {
        Some(f) => {
            if f > 0.0 {
                g.fill_rrect(Rect::new(bar.x, bar.y, (bar.w * f).max(bar.h), bar.h), bar.h / 2.0, th.accent);
            }
        }
        None => {
            // Unknown total: a piece that moves.
            let t = b.started.elapsed().as_secs_f32();
            let piece = bar.w * 0.25;
            let at = (t * 0.6).fract() * (bar.w - piece);
            g.fill_rrect(Rect::new(bar.x + at, bar.y, piece, bar.h), bar.h / 2.0, th.accent);
        }
    }
    let current = b.meter.current();
    if !current.is_empty() {
        let c = ellipsize_middle(g, &current, w, px(s), FontId::Regular);
        g.text_in(&c, Rect::new(bar.x, bar.bottom() + 8.0 * s, w, 22.0 * s), px(s), FontId::Regular, th.dim, Align::Center);
    }
    let done = if b.by_read { b.meter.read.load(Relaxed) } else { b.meter.done.load(Relaxed) };
    let total = b.total();
    let amount = if total > 0 { tf("{} of {}", &[&format::size(done.min(total)), &format::size(total)]) } else { format::size(done) };
    let line = format!("{amount} · {}", elapsed(b.started.elapsed().as_secs()));
    g.text_in(&line, Rect::new(bar.x, bar.bottom() + 30.0 * s, w, 22.0 * s), px(s), FontId::Regular, th.dim, Align::Center);
    button(g, l.stop, &strip_mnemonic(&tr("&Stop")), th, s, hot == Hot::Stop, Look::Secondary);
}

/// `0:07`, `12:30`, `1:02:03`.
pub fn elapsed(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// The label of the button that extracts: what it will extract.
pub fn extract_label(app: &App) -> String {
    if app.in_archive() {
        let n = app.chosen().len();
        if n == 0 {
            tr("Extract All")
        } else {
            tf("Extract {}", &[&format::count(n as u64)])
        }
    } else {
        match app.chosen_archives().len() {
            0 | 1 => tr("Extract"),
            n => tf("Extract {}", &[&format::count(n as u64)]),
        }
    }
}

fn col_title(c: Col) -> String {
    tr(match c {
        Col::Name => "Name",
        Col::Size => "Size",
        Col::Packed => "Packed",
        Col::Modified => "Modified",
        Col::Method => "Method",
        Col::Type => "Type",
        Col::Free => "Free",
        Col::FileSystem => "File system",
    })
}

thread_local! {
    /// What the system calls each extension, asked once.
    static TYPES: std::cell::RefCell<std::collections::HashMap<String, String>> = Default::default();
}

/// The kind of a file on disk, as the system names it (`Text Document`), or
/// by its extension (`TXT file`).
pub fn type_name(name: &str) -> String {
    let Some((_, ext)) = name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()) else { return tr("File") };
    let ext = ext.to_lowercase();
    TYPES.with(|t| {
        t.borrow_mut()
            .entry(ext.clone())
            .or_insert_with(|| {
                #[cfg(windows)]
                if let Some(n) = rt_sys::win::file_type_name(&ext) {
                    return n;
                }
                tf("{} file", &[&ext.to_uppercase()])
            })
            .clone()
    })
}

/// The text of a row's cell.
fn cell_text(app: &App, row: Row, c: Col) -> String {
    match row {
        Row::Up => String::new(),
        Row::Disk(i) => {
            let d = &app.disk[i];
            if let Some(v) = &d.volume {
                return match c {
                    Col::Name => d.name.clone(),
                    Col::Size if v.total > 0 => format::size(v.total),
                    Col::Free if v.total > 0 => format::size(v.free),
                    Col::FileSystem => v.fs.clone(),
                    _ => String::new(),
                };
            }
            match c {
                Col::Name => d.name.clone(),
                Col::Size if !d.dir => format::size(d.size),
                Col::Modified => format::datetime(d.mtime),
                Col::Type if d.dir => tr("Folder"),
                Col::Type => type_name(&d.name),
                _ => String::new(),
            }
        }
        Row::Dir(n) => {
            let Some(o) = &app.opened else { return String::new() };
            let n = &o.tree.nodes[n];
            match c {
                Col::Name => n.name.clone(),
                Col::Size => format::size(n.size),
                Col::Packed => n.packed.filter(|_| n.count > 0).map_or(String::new(), format::size),
                Col::Modified => format::datetime(n.mtime),
                _ => String::new(),
            }
        }
        Row::File(e) => {
            let Some(o) = &app.opened else { return String::new() };
            let e = &o.listing.entries[e];
            match c {
                // An encrypted entry is marked as 7-Zip marks it.
                Col::Name if e.encrypted => format!("{} *", e.name()),
                Col::Name => e.name().to_string(),
                Col::Size => e.size.map_or(String::new(), format::size),
                Col::Packed => e.packed.map_or(String::new(), format::size),
                Col::Modified => format::datetime(e.mtime),
                Col::Method => e.method.clone(),
                _ => String::new(),
            }
        }
    }
}

fn draw_browser(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let p = px(s);
    let pad = 8.0 * s;
    // The path.
    let c = l.crumbs;
    g.fill_rect(c, th.panel);
    g.fill_rect(Rect::new(c.x, c.bottom() - 1.0, c.w, 1.0), th.border);
    let can_up = app.up_target().is_some();
    let up = l.up;
    if can_up && hot == Hot::Up {
        g.fill_rrect(up, 4.0 * s, th.hover);
    }
    let ac = if can_up { th.text } else { th.border };
    let (mx, my) = (up.x + up.w / 2.0, up.y + up.h / 2.0);
    g.polyline(&[(mx - 5.0 * s, my + 1.0 * s), (mx, my - 4.5 * s), (mx + 5.0 * s, my + 1.0 * s)], 1.8 * s, ac, false);
    g.polyline(&[(mx, my - 4.0 * s), (mx, my + 6.0 * s)], 1.8 * s, ac, false);
    g.set_clip(Some(Rect::new(up.right() + 4.0 * s, c.y, c.right() - up.right() - 4.0 * s, c.h)));
    let trail = app.trail(&drives_label());
    for (i, (r, (_, label))) in l.crumb.iter().zip(&trail).enumerate() {
        let last = i + 1 == trail.len();
        if hot == Hot::Crumb(i) && !last {
            g.fill_rrect(*r, 4.0 * s, th.hover);
        }
        let text = ellipsize(g, label, r.w - 12.0 * s, p, FontId::Semibold);
        g.text_in(&text, *r, p, FontId::Semibold, if last { th.text } else { th.dim }, Align::Center);
        if !last {
            g.text_in("›", Rect::new(r.right(), r.y, 14.0 * s, r.h), p, FontId::Regular, th.dim, Align::Center);
        }
    }
    g.set_clip(None);
    // Header.
    g.fill_rect(l.header, th.header);
    g.fill_rect(Rect::new(l.header.x, l.header.bottom() - 1.0, l.full.w, 1.0), th.border);
    for &(col, x, w) in &l.cols {
        if w <= 0.0 {
            continue;
        }
        let r = Rect::new(x + pad, l.header.y, w - 2.0 * pad, l.header.h);
        if hot == Hot::Header(col) {
            g.fill_rect(Rect::new(x, l.header.y, w, l.header.h - 1.0), th.hover);
        }
        let mut text = col_title(col);
        if app.sort == col {
            text.push_str(if app.descending { " ▾" } else { " ▴" });
        }
        let text = ellipsize(g, &text, r.w, p, FontId::Semibold);
        g.text_in(&text, r, p, FontId::Semibold, th.dim, align_of(col));
    }
    let empty = app.rows.iter().all(|r| *r == Row::Up);
    if let Some(e) = &app.disk_error {
        let below = Rect::new(l.list.x, l.list.y + l.row_h, l.list.w, l.list.h - l.row_h);
        let e = ellipsize(g, e, below.w - 32.0 * s, p, FontId::Regular);
        g.text_in(&e, below, p, FontId::Regular, th.warn, Align::Center);
    } else if empty {
        let text = if app.in_archive() { tr("This archive is empty.") } else { tr("This folder is empty.") };
        g.text_in(&text, l.list, 15.0 * s, FontId::Regular, th.dim, Align::Center);
    }
    g.set_clip(Some(l.list));
    let end = (app.scroll + l.page_rows() + 1).min(app.rows.len());
    let (asc, desc) = g.metrics(p, FontId::Regular);
    for i in app.scroll..end {
        let row = app.rows[i];
        let y = l.list.y + (i - app.scroll) as f32 * l.row_h;
        let r = Rect::new(l.list.x, y, l.list.w, l.row_h);
        let base = (r.center_y() + (asc + desc) / 2.0).round();
        if app.selected.get(i).copied().unwrap_or(false) {
            g.fill_rect(r, th.selection);
        } else if hot == Hot::Row(i) {
            g.fill_rect(r, th.hover);
        }
        if app.cursor == Some(i) {
            g.stroke_rrect(r.inset(1.0), 0.0, 1.0, th.accent.with_alpha(0x90));
        }
        for &(col, x, w) in &l.cols {
            if w <= 0.0 {
                continue;
            }
            if col == Col::Name {
                let ix = x + pad;
                match row {
                    Row::Up => {
                        folder_icon(g, ix, r.center_y(), s);
                        g.text("..", ix + 22.0 * s, base, p, FontId::Regular, th.text);
                        continue;
                    }
                    Row::Dir(_) => folder_icon(g, ix, r.center_y(), s),
                    Row::File(_) => file_icon(g, ix, r.center_y(), s, th),
                    Row::Disk(k) => {
                        let d = &app.disk[k];
                        if d.volume.is_some() {
                            drive_icon(g, ix, r.center_y(), s, th);
                        } else if d.dir {
                            folder_icon(g, ix, r.center_y(), s);
                        } else if crate::app::looks_like_archive(&d.name) {
                            archive_icon(g, ix, r.center_y(), s);
                        } else {
                            file_icon(g, ix, r.center_y(), s, th);
                        }
                    }
                }
                let room = (x + w - ix - 22.0 * s - pad).max(0.0);
                let name = ellipsize(g, &cell_text(app, row, col), room, p, FontId::Regular);
                g.text(&name, ix + 22.0 * s, base, p, FontId::Regular, th.text);
            } else {
                let cell = Rect::new(x + pad, r.y, (w - 2.0 * pad).max(0.0), r.h);
                let text = ellipsize(g, &cell_text(app, row, col), cell.w, p, FontId::Regular);
                g.text_in(&text, cell, p, FontId::Regular, if col == Col::Size { th.text } else { th.dim }, align_of(col));
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
    draw_actions(g, app, l, th, s, hot);
}

fn align_of(c: Col) -> Align {
    match c {
        Col::Name | Col::Method | Col::Type | Col::FileSystem => Align::Left,
        _ => Align::Right,
    }
}

/// Whether the hashes can be calculated now: some file or folder chosen.
pub fn can_hash(app: &App) -> bool {
    if app.in_archive() {
        app.opened.as_ref().is_some_and(|o| o.listing.files() > 0)
    } else {
        app.place != Place::Drives && !app.disk_chosen().is_empty()
    }
}

/// Whether files and folders are chosen to put in an archive.
pub fn can_add(app: &App) -> bool {
    matches!(app.place, Place::Folder(_)) && !app.disk_chosen().is_empty()
}

/// Whether Test and Extract can be used now.
pub fn can_extract(app: &App) -> bool {
    app.in_archive() || !app.chosen_archives().is_empty()
}

/// The bar under the list.
fn draw_actions(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let p = px(s);
    let a = l.actions;
    g.fill_rect(a, th.status);
    g.fill_rect(Rect::new(a.x, a.y, a.w, 1.0), th.border);
    let look = |on: bool| if on { Look::Secondary } else { Look::Disabled };
    if l.add.w > 0.0 {
        button(g, l.add, &tr("Add..."), th, s, hot == Hot::Add, look(can_add(app)));
    }
    button(g, l.hashes, &tr("Hashes..."), th, s, hot == Hot::Hashes, look(can_hash(app)));
    let ok = can_extract(app);
    button(g, l.test, &strip_mnemonic(&tr("&Test")), th, s, hot == Hot::Test, look(ok));
    button(g, l.extract, &extract_label(app), th, s, hot == Hot::Extract, if ok { Look::Primary } else { Look::Disabled });
    let d = l.dest;
    if app.in_archive() {
        let dest = app.dest().map(|d| d.display().to_string()).unwrap_or_default();
        let label = tr("Extract to:");
        let lw = g.measure(&label, p, FontId::Regular);
        if d.w > lw + 40.0 * s {
            g.text_in(&label, Rect::new(d.x, d.y, lw, d.h / 2.0 + 2.0 * s), p, FontId::Regular, th.dim, Align::Left);
            let path = ellipsize_middle(g, &dest, d.w, p, FontId::Semibold);
            g.text_in(&path, Rect::new(d.x, d.y + d.h / 2.0 - 4.0 * s, d.w, d.h / 2.0), p, FontId::Semibold, th.text, Align::Left);
        }
        button(g, l.change, &tr("Change..."), th, s, hot == Hot::Change, Look::Secondary);
    } else {
        let hint = match app.place {
            Place::Drives => tr("Open a drive, then the archives in it."),
            _ if ok => tr("Each archive is extracted into a folder beside it."),
            _ => tr("Open an archive to see what it holds, or select archives to extract them."),
        };
        let hint = ellipsize(g, &hint, d.w, p, FontId::Regular);
        g.text_in(&hint, d, p, FontId::Regular, th.dim, Align::Left);
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

/// Size, packed size and the share it takes: `45.2 MB → 12.1 MB (27%)`.
pub fn sizes_text(size: u64, packed: Option<u64>) -> String {
    match packed {
        Some(p) if size > 0 => format!("{} → {} ({})", format::size(size), format::size(p), format::percent(p, size)),
        _ => format::size(size),
    }
}

/// The status bar: (the row pointed, selected, or the last note; the totals).
pub fn status_text(app: &App, hover: Option<usize>) -> (String, String) {
    if page(app) != Page::Browse {
        return (String::new(), String::new());
    }
    let describe = |row: Row| -> String {
        match row {
            Row::Up => tr("Back to the folder above"),
            Row::Disk(i) => {
                let d = &app.disk[i];
                match &d.volume {
                    Some(v) => format!("{} · {} · {}", d.path.display(), tf("{} free of {}", &[&format::size(v.free), &format::size(v.total)]), v.fs),
                    None if d.dir => d.path.display().to_string(),
                    None => format!("{} · {}", d.path.display(), format::size(d.size)),
                }
            }
            Row::Dir(n) => {
                let Some(o) = &app.opened else { return String::new() };
                let n = &o.tree.nodes[n];
                format!("{} · {} · {}", n.name, tf("{} files", &[&format::count(n.count)]), sizes_text(n.size, n.packed.filter(|_| n.count > 0)))
            }
            Row::File(e) => {
                let Some(o) = &app.opened else { return String::new() };
                let e = &o.listing.entries[e];
                let mut t = format!("{} · {}", e.path, sizes_text(e.size.unwrap_or(0), e.packed));
                if let Some(crc) = e.crc {
                    t.push_str(&format!(" · CRC {crc:08X}"));
                }
                if let Some(link) = &e.link {
                    t.push_str(&format!(" → {link}"));
                }
                t
            }
        }
    };
    let chosen = app.chosen();
    let left = if let Some(row) = hover.and_then(|i| app.rows.get(i)) {
        describe(*row)
    } else if let Some(n) = &app.note {
        n.clone()
    } else if chosen.len() > 1 {
        tf("{} selected", &[&format::count(chosen.len() as u64)])
    } else if let Some(row) = chosen.first() {
        describe(*row)
    } else {
        String::new()
    };
    let right = match (&app.place, &app.opened) {
        (Place::Archive, Some(o)) => {
            let l = &o.listing;
            let all_packed = l.entries.iter().filter(|e| e.kind == crate::archive::Kind::File).all(|e| e.packed.is_some());
            let packed = if all_packed { l.entries.iter().filter_map(|e| e.packed).sum() } else { l.file_size };
            format!("{} · {} · {}", l.format.label(), tf("{} files", &[&format::count(l.files() as u64)]), sizes_text(l.size(), Some(packed)))
        }
        (Place::Folder(_), _) => {
            let (n, bytes) = app.disk.iter().filter(|d| !d.dir).fold((0u64, 0u64), |(n, b), d| (n + 1, b + d.size));
            let dirs = app.disk.iter().filter(|d| d.dir).count() as u64;
            format!("{} · {} · {}", tf("{} folders", &[&format::count(dirs)]), tf("{} files", &[&format::count(n)]), format::size(bytes))
        }
        _ => tf("{} drives", &[&format::count(app.disk.len() as u64)]),
    };
    (left, right)
}

#[cfg(test)]
mod screenshots {
    use super::*;
    use crate::archive::{open, Meter};
    use crate::i18n::{set_lang, Lang};
    use rt_scan::volumes::Volume;
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Draws each page to `target/screens/*.png` for a visual check:
    /// `cargo test -p rpack --lib screenshots -- --ignored`.
    #[test]
    #[ignore]
    fn screenshots() {
        let Ok(mut g) = Gfx::new() else { return };
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&out).unwrap();
        let d = crate::archive::tests::scratch("shots");
        let zip = d.join("Holiday photos 2024.zip");
        crate::archive::tests::make_zip(&zip);
        std::fs::create_dir_all(d.join("Documents")).unwrap();
        std::fs::write(d.join("notes.txt"), "some notes").unwrap();
        std::fs::write(d.join("linux-6.12.tar.xz"), crate::archive::tests::compress(crate::archive::Codec::Xz, b"x")).unwrap();
        let listing = open(&zip, &Meter::default()).unwrap();
        let mut shot = |name: &str, app: &App, dark: bool, hot: Hot, w: u32, h: u32| {
            g.resize(w, h);
            let full = Rect::new(0.0, 0.0, w as f32, h as f32);
            let th = Theme::new(dark, None);
            let l = layout(&mut g, app, full, 1.0, false);
            draw(&mut g, app, &l, &th, 1.0, hot);
            g.pix.save_png(out.join(format!("{name}.png"))).unwrap();
        };
        set_lang(Lang::EnUS);
        // The icon, at the size of the .ico's largest image.
        let px = crate::ui::appicon::render(256);
        let mut icon = tiny_skia::Pixmap::new(256, 256).unwrap();
        for (d, s) in icon.pixels_mut().iter_mut().zip(px.chunks(4)) {
            *d = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
        }
        icon.save_png(out.join("icon.png")).unwrap();
        let gb = 1u64 << 30;
        let vols = vec![
            Volume { path: "C:\\".into(), label: "Windows".into(), fs: "NTFS".into(), total: 451 * gb, free: 38 * gb, remote: false },
            Volume { path: "D:\\".into(), label: "Data".into(), fs: "NTFS".into(), total: 480 * gb, free: 210 * gb, remote: false },
            Volume { path: "T:\\".into(), label: "Backup".into(), fs: "exFAT".into(), total: 931 * gb, free: 702 * gb, remote: false },
        ];
        let mut app = App::new();
        app.show_drives(vols, None);
        shot("drives", &app, false, Hot::Row(1), 960, 680);
        app.show_folder(&d, Some(&zip)).unwrap();
        shot("folder-light", &app, false, Hot::Extract, 960, 680);
        app.select_all();
        shot("folder-dark", &app, true, Hot::Hashes, 960, 680);
        let mut f = crate::app::AddForm::new(vec![zip.clone()], d.clone(), crate::create::Spec::default());
        f.set_kind(NewKind::Zip);
        app.add = Some(f);
        shot("add-zip", &app, false, Hot::FormKind(3), 960, 680);
        let mut f = crate::app::AddForm::new(vec![zip.clone(), d.join("notes.txt")], d.clone(), crate::create::Spec::default());
        f.each = true;
        app.add = Some(f);
        shot("add-each-dark", &app, true, Hot::FormOk, 960, 680);
        set_lang(Lang::PtPT);
        app.add.as_mut().unwrap().each = false;
        shot("add-pt", &app, false, Hot::None, 960, 680);
        set_lang(Lang::EnUS);
        app.add = None;
        let meter = Arc::new(Meter::default());
        meter.done.store(312 << 20, std::sync::atomic::Ordering::Relaxed);
        meter.set_current("docs/img/IMG_2024_0815_123456.jpg");
        app.busy = Some(Busy { task: Task::Extracting, meter, started: std::time::Instant::now(), total: 1 << 30, by_read: false, path: zip.clone(), count: 3 });
        shot("busy", &app, false, Hot::Stop, 960, 680);
        app.busy = None;
        app.set_opened(zip.clone(), listing);
        shot("archive-light", &app, false, Hot::Row(2), 960, 680);
        let docs = app.opened.as_ref().unwrap().tree.nodes[0].dirs[0];
        app.enter(docs);
        app.click(1, false, false);
        app.click(3, false, true);
        shot("archive-dark", &app, true, Hot::Header(Col::Size), 960, 680);
        app.note = Some("4 files extracted (683 KB) to C:\\Users\\Me\\Holiday photos 2024.".into());
        shot("archive-narrow", &app, false, Hot::Crumb(2), 560, 520);
        set_lang(Lang::PtPT);
        app.note = None;
        app.dest = Some(PathBuf::from("D:\\Fotos\\Férias"));
        shot("archive-pt", &app, false, Hot::Extract, 960, 680);
        app.show_folder(&d, None).unwrap();
        shot("folder-pt", &app, false, Hot::None, 960, 680);
        set_lang(Lang::EnUS);
    }
}
