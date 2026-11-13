//! The window, drawn each frame: the buttons (or what is being done), a line
//! about what needs administrator rights, the list of what can be cleaned
//! by group, the details of the row selected, and the status bar.
//!
//! [`layout`] decides where things go; `main` uses the same [`Layout`] to
//! find what the pointer is on.

use super::gfx::{Align, Color, FontId, Gfx, Rect};
use super::theme::Theme;
use crate::app::{App, Item, Row, State};
use crate::format;
use crate::i18n::{tf, tr};
use crate::targets::{Filter, Group, Kind};
use rt_gfx::dialog::{ellipsize, ellipsize_middle, wrap_words};

/// What the pointer is on, for hover effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Hot {
    #[default]
    None,
    /// A button above the list (index into `commands::ACTIONS`).
    Action(usize),
    /// "Stop", while working.
    Stop,
    Restart,
    /// A row's box.
    Check(usize),
    Row(usize),
    Details,
}

impl Hot {
    pub fn clickable(self) -> bool {
        !matches!(self, Hot::None | Hot::Row(_) | Hot::Details)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub full: Rect,
    /// The drawn menu bar (not on Windows).
    pub bar: Rect,
    pub status: Rect,
    pub action_bar: Rect,
    pub actions: Vec<Rect>,
    pub stop: Rect,
    /// The line about what needs administrator rights, and its button.
    pub banner: Rect,
    pub restart: Rect,
    pub header: Rect,
    /// Columns: files, size, notes (x, width); the name takes the rest.
    pub files_col: (f32, f32),
    pub size_col: (f32, f32),
    pub note_col: (f32, f32),
    pub list: Rect,
    pub row_h: f32,
    pub track: Rect,
    /// The details of the row selected (empty when the window is narrow).
    pub details: Rect,
}

impl Layout {
    pub fn row_at(&self, app: &App, x: f32, y: f32) -> Option<usize> {
        if !self.list.contains(x, y) || self.row_h <= 0.0 {
            return None;
        }
        let i = app.scroll + ((y - self.list.y) / self.row_h) as usize;
        (i < app.rows.len()).then_some(i)
    }

    /// Where a row's box is, across: (from, to).
    pub fn check_area(&self, app: &App, row: usize) -> (f32, f32) {
        let indent = if matches!(app.rows.get(row), Some(Row::Item(_))) { 22.0 * self.scale() } else { 0.0 };
        let x = self.list.x + indent;
        (x, x + self.row_h * 1.25)
    }

    fn scale(&self) -> f32 {
        self.row_h / 26.0
    }

    pub fn page_rows(&self) -> usize {
        if self.row_h <= 0.0 {
            return 1;
        }
        ((self.list.h / self.row_h).floor() as usize).max(1)
    }
}

/// A group's name, as shown.
pub fn group_label(g: Group) -> String {
    match g {
        Group::System if cfg!(windows) => "Windows".into(),
        Group::System => "Linux".into(),
        Group::Browser(name) => name.to_string(),
    }
}

/// Whether to say that some items need administrator rights.
pub fn needs_admin(app: &App) -> bool {
    app.items.iter().any(|it| it.state == State::NeedsAdmin)
}

/// A command's label on a button: no access key, no dots.
pub fn action_label(c: crate::commands::Command) -> String {
    rt_core::i18n::strip_mnemonic(&c.label()).trim_end_matches("...").trim_end_matches('…').to_string()
}

fn px(s: f32) -> f32 {
    12.5 * s
}

/// Where everything goes in a window of `full` at scale `s`.
pub fn layout(g: &mut Gfx, app: &App, full: Rect, s: f32, menu_bar: bool) -> Layout {
    let mut l = Layout { full, ..Layout::default() };
    let mut area = full;
    if menu_bar {
        l.bar = area.take_top((28.0 * s).round());
    }
    l.status = area.take_bottom((24.0 * s).round());
    l.action_bar = area.take_top((46.0 * s).round());
    {
        let b = l.action_bar;
        let h = (30.0 * s).round();
        let y = (b.y + (b.h - h) / 2.0).round();
        let p = px(s);
        if app.busy.is_some() {
            let w = (g.measure(&action_label(crate::commands::Command::Stop), p, FontId::Semibold) + 36.0 * s).round();
            l.stop = Rect::new(b.right() - w - 10.0 * s, y, w, h);
        } else {
            let mut x = b.x + 10.0 * s;
            for &c in &crate::commands::ACTIONS {
                let w = (g.measure(&action_label(c), p, FontId::Semibold) + 32.0 * s).round();
                let r = Rect::new(x, y, w, h);
                l.actions.push(if r.right() <= b.right() - 10.0 * s { r } else { Rect::default() });
                x += w + 6.0 * s;
            }
        }
    }
    if needs_admin(app) && cfg!(windows) {
        l.banner = area.take_top((34.0 * s).round());
        let bw = (210.0 * s).round();
        let bh = (26.0 * s).round();
        l.restart = Rect::new(l.banner.right() - bw - 10.0 * s, (l.banner.y + (l.banner.h - bh) / 2.0).round(), bw, bh);
    }
    if area.w >= 820.0 * s {
        let dw = (380.0 * s).min(area.w * 0.42).round();
        l.details = area.take_right(dw);
    }
    l.header = area.take_top((26.0 * s).round());
    l.row_h = (26.0 * s).round();
    l.track = area.take_right((10.0 * s).round());
    l.list = area;
    let mut x1 = area.right();
    let note = if area.w >= 620.0 * s { (220.0 * s).round() } else { 0.0 };
    x1 -= note;
    l.note_col = (x1, note);
    let size = (96.0 * s).round();
    x1 -= size;
    l.size_col = (x1, size);
    let files = (90.0 * s).round();
    x1 -= files;
    l.files_col = (x1, files);
    l
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tick {
    Off,
    On,
    /// Some of a group.
    Some,
}

/// A box to tick, at `x`, centred on `cy`.
fn check_box(g: &mut Gfx, x: f32, cy: f32, th: &Theme, s: f32, state: Tick, usable: bool) {
    let b = (15.0 * s).round();
    let r = Rect::new(x.round(), (cy - b / 2.0).round(), b, b);
    let fg = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
    let accent = if usable { th.accent } else { th.dim.with_alpha(0x80) };
    match state {
        Tick::On => {
            g.fill_rrect(r, 3.0 * s, accent);
            let pts = [(r.x + b * 0.24, r.y + b * 0.52), (r.x + b * 0.43, r.y + b * 0.70), (r.x + b * 0.77, r.y + b * 0.32)];
            g.polyline(&pts, 1.8 * s, fg, false);
        }
        Tick::Some => {
            g.fill_rrect(r, 3.0 * s, accent);
            g.fill_rect(Rect::new(r.x + b * 0.25, (r.center_y() - 1.0 * s).round(), b * 0.5, (2.0 * s).round()), fg);
        }
        Tick::Off => {
            g.fill_rrect(r, 3.0 * s, th.bg);
            g.stroke_rrect(r, 3.0 * s, 1.0, if usable { th.dim } else { th.dim.with_alpha(0x60) });
        }
    }
}

/// What an item holds now, as the Files and Size columns show it.
fn counts(it: &Item) -> (String, String) {
    match &it.state {
        State::Found(f) | State::Cleaning(f) => (format::count(f.count()), format::size(f.bytes)),
        State::Analysing => ("…".into(), "…".into()),
        _ => (String::new(), String::new()),
    }
}

/// The note on an item's row: what is going on, or why not all may go.
pub fn note_text(it: &Item) -> String {
    match &it.state {
        State::Waiting => String::new(),
        State::Analysing => tr("Analyzing…"),
        State::Cleaning(_) => tr("Cleaning…"),
        State::NeedsAdmin => tr("Needs administrator rights"),
        State::Cleaned(c) if c.failed > 0 => tf("{} freed; {} in use stayed", &[&format::size(c.bytes), &format::count(c.failed)]),
        State::Cleaned(c) => tf("{} freed", &[&format::size(c.bytes)]),
        State::Found(_) if it.open => tr("Open: some files may be in use"),
        State::Found(f) if f.unreadable > 0 => tf("{} folders could not be read", &[&format::count(f.unreadable)]),
        State::Found(f) if f.count() == 0 => tr("Nothing to clean"),
        State::Found(_) => String::new(),
    }
}

/// A group's totals: (files, bytes) of what its items hold.
fn group_totals(app: &App, g: usize) -> (u64, u64) {
    app.group_items(g).iter().filter_map(|&i| app.items[i].found()).fold((0, 0), |(n, b), f| (n + f.count(), b + f.bytes))
}

/// The buttons above the list, or what is being done.
fn draw_actions(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot, enabled: &dyn Fn(crate::commands::Command) -> bool) {
    let b = l.action_bar;
    g.fill_rect(b, th.panel);
    g.fill_rect(Rect::new(b.x, b.bottom() - 1.0, b.w, 1.0), th.border);
    let p = px(s);
    if let Some(busy) = &app.busy {
        let text = match (busy.cleaning, busy.stopping) {
            (_, true) => tr("Stopping after this one…"),
            (true, _) => tf("Cleaning {} ({} of {})…", &[&busy.name, &(busy.done + 1).min(busy.total).to_string(), &busy.total.to_string()]),
            (false, _) => tf("Analyzing {} ({} of {})…", &[&busy.name, &(busy.done + 1).min(busy.total).to_string(), &busy.total.to_string()]),
        };
        // A turning arc, then what is going on.
        let (cx, cy, rad) = (b.x + 22.0 * s, b.center_y() - 1.0 * s, 7.0 * s);
        let start = busy.phase * std::f32::consts::TAU * 2.0;
        g.polyline(
            &(0..=24).map(|i| (cx + rad * (start + i as f32 / 24.0 * 4.4).cos(), cy + rad * (start + i as f32 / 24.0 * 4.4).sin())).collect::<Vec<_>>(),
            2.0 * s,
            th.accent,
            false,
        );
        let right = if l.stop.w > 0.0 { l.stop.x } else { b.right() };
        let room = Rect::new(b.x + 38.0 * s, b.y, right - b.x - 52.0 * s, b.h);
        let text = ellipsize(g, &text, room.w, p, FontId::Semibold);
        g.text_in(&text, room, p, FontId::Semibold, th.text, Align::Left);
        let bar = Rect::new(b.x, b.bottom() - (4.0 * s).round(), b.w, (4.0 * s).round());
        g.fill_rect(bar, th.bar_track);
        g.fill_rect(Rect::new(bar.x, bar.y, (bar.w * app.progress()).round(), bar.h), th.accent);
        let r = l.stop;
        if r.w > 0.0 {
            let lit = hot == Hot::Stop && !busy.stopping;
            g.fill_rrect(r, 5.0 * s, if lit { th.hover } else { th.panel });
            g.stroke_rrect(r, 5.0 * s, 1.0, if lit { th.accent } else { th.border });
            let fg = if busy.stopping { th.dim } else { th.text };
            g.text_in(&action_label(crate::commands::Command::Stop), r, p, FontId::Semibold, fg, Align::Center);
        }
        return;
    }
    for (i, (r, c)) in l.actions.iter().zip(crate::commands::ACTIONS).enumerate() {
        if r.w <= 0.0 {
            continue;
        }
        let on = enabled(c);
        let lit = on && hot == Hot::Action(i);
        // Clean is the main button: filled with the accent.
        if c == crate::commands::Command::Clean && on {
            g.fill_rrect(*r, 5.0 * s, if lit { th.accent.mix(th.bg, 0.15) } else { th.accent });
            let fg = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
            g.text_in(&action_label(c), *r, p, FontId::Semibold, fg, Align::Center);
            continue;
        }
        g.fill_rrect(*r, 5.0 * s, if lit { th.hover } else { th.panel });
        g.stroke_rrect(*r, 5.0 * s, 1.0, if lit { th.accent } else { th.border });
        g.text_in(&action_label(c), *r, p, FontId::Semibold, if on { th.text } else { th.dim.with_alpha(0x80) }, Align::Center);
    }
    // How much the ticked ones hold.
    let (n, bytes) = app.ticked_total();
    if bytes > 0 || n > 0 {
        let text = tf("{} to free in {} files", &[&format::size(bytes), &format::count(n)]);
        let w = g.measure(&text, p, FontId::Semibold);
        let x0 = l.actions.iter().map(|r| r.right()).fold(b.x, f32::max) + 12.0 * s;
        if b.right() - 12.0 * s - w > x0 {
            g.text_in(&text, Rect::new(b.right() - 12.0 * s - w, b.y, w, b.h), p, FontId::Semibold, th.text, Align::Left);
        }
    }
}

/// Draws the window.
#[allow(clippy::too_many_arguments)]
pub fn draw(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot, enabled: &dyn Fn(crate::commands::Command) -> bool) {
    g.clear(th.bg);
    draw_actions(g, app, l, th, s, hot, enabled);
    let p = px(s);
    let pad = 8.0 * s;
    if l.banner.h > 0.0 {
        let b = l.banner;
        g.fill_rect(b, th.warn.with_alpha(0x30));
        g.fill_rect(Rect::new(b.x, b.bottom() - 1.0, b.w, 1.0), th.border);
        let text = tr("Some of Windows' own folders need administrator rights to be cleaned.");
        let room = Rect::new(b.x + 12.0 * s, b.y, l.restart.x - b.x - 24.0 * s, b.h);
        let text = ellipsize(g, &text, room.w, p, FontId::Regular);
        g.text_in(&text, room, p, FontId::Regular, th.text, Align::Left);
        let r = l.restart;
        g.fill_rrect(r, 5.0 * s, if hot == Hot::Restart { th.accent } else { th.accent.mix(th.bg, 0.12) });
        let fg = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
        let label = ellipsize(g, &rt_core::i18n::strip_mnemonic(&tr("Restart as Ad&ministrator")), r.w - 10.0 * s, p, FontId::Semibold);
        g.text_in(&label, r, p, FontId::Semibold, fg, Align::Center);
    }
    // Header.
    g.fill_rect(l.header, th.header);
    g.fill_rect(Rect::new(l.header.x, l.header.bottom() - 1.0, l.header.w + l.track.w, 1.0), th.border);
    let name_r = Rect::new(l.header.x + pad, l.header.y, l.files_col.0 - l.header.x - 2.0 * pad, l.header.h);
    g.text_in(&tr("What"), name_r, p, FontId::Semibold, th.dim, Align::Left);
    for (title, (x, w), align) in [("Files", l.files_col, Align::Right), ("Size", l.size_col, Align::Right), ("Note", l.note_col, Align::Left)] {
        if w > 0.0 {
            let r = Rect::new(x + pad, l.header.y, w - 2.0 * pad, l.header.h);
            let t = ellipsize(g, &tr(title), r.w, p, FontId::Semibold);
            g.text_in(&t, r, p, FontId::Semibold, th.dim, align);
        }
    }
    // Rows.
    g.set_clip(Some(l.list));
    let end = (app.scroll + l.page_rows() + 1).min(app.rows.len());
    for r in app.scroll..end {
        let y = l.list.y + (r - app.scroll) as f32 * l.row_h;
        let row = Rect::new(l.list.x, y, l.list.w, l.row_h);
        let group = matches!(app.rows[r], Row::Group(_));
        if group {
            g.fill_rect(row, th.panel);
        }
        if app.sel == Some(r) {
            g.fill_rect(row, th.selection);
        } else if hot == Hot::Row(r) || hot == Hot::Check(r) {
            g.fill_rect(row, th.hover);
        }
        let (x0, x1) = l.check_area(app, r);
        let (name, tick, usable, files, size, note, font, colour) = match app.rows[r] {
            Row::Group(gi) => {
                let (some, all) = app.group_tick(gi);
                let tick = match (some, all) {
                    (_, true) => Tick::On,
                    (true, false) => Tick::Some,
                    _ => Tick::Off,
                };
                let (n, b) = group_totals(app, gi);
                let shown = n > 0 || b > 0;
                let (files, size) = if shown { (format::count(n), format::size(b)) } else { (String::new(), String::new()) };
                (group_label(app.groups[gi]), tick, true, files, size, String::new(), FontId::Semibold, th.text)
            }
            Row::Item(i) => {
                let it = &app.items[i];
                let (files, size) = counts(it);
                let colour = match &it.state {
                    State::NeedsAdmin => th.dim,
                    State::Cleaned(_) => th.good,
                    State::Found(_) if it.open => th.warn,
                    _ => th.dim,
                };
                (tr(it.target.name), if it.ticked { Tick::On } else { Tick::Off }, it.usable(), files, size, note_text(it), FontId::Regular, colour)
            }
        };
        check_box(g, x0 + pad, row.center_y(), th, s, tick, usable);
        let nr = Rect::new(x1, row.y, (l.files_col.0 - x1 - pad).max(0.0), row.h);
        let name = ellipsize(g, &name, nr.w, p, font);
        g.text_in(&name, nr, p, font, if usable { th.text } else { th.dim }, Align::Left);
        for (text, (x, w), align, f) in [(files, l.files_col, Align::Right, font), (size, l.size_col, Align::Right, FontId::Semibold)] {
            if w > 0.0 && !text.is_empty() {
                g.text_in(&text, Rect::new(x + pad, row.y, w - 2.0 * pad, row.h), p, f, th.text, align);
            }
        }
        if l.note_col.1 > 0.0 && !note.is_empty() {
            let nr = Rect::new(l.note_col.0 + pad, row.y, l.note_col.1 - 2.0 * pad, row.h);
            let note = ellipsize(g, &note, nr.w, p * 0.95, FontId::Regular);
            g.text_in(&note, nr, p * 0.95, FontId::Regular, colour, Align::Left);
        }
        if group {
            g.fill_rect(Rect::new(row.x, row.bottom() - 1.0, row.w, 1.0), th.border);
        }
    }
    g.set_clip(None);
    // Scroll bar.
    let page = l.page_rows();
    if app.rows.len() > page {
        let t = l.track;
        let h = (t.h * page as f32 / app.rows.len() as f32).max(24.0 * s);
        let y = t.y + (t.h - h) * app.scroll as f32 / (app.rows.len() - page) as f32;
        g.fill_rrect(Rect::new(t.x + 2.0 * s, y, t.w - 4.0 * s, h), 3.0 * s, th.scrollbar);
    }
    draw_details(g, app, l, th, s);
    // Status bar.
    let st = l.status;
    g.fill_rect(st, th.status);
    g.fill_rect(Rect::new(st.x, st.y, st.w, 1.0), th.border);
    let (left, right) = status_text(app);
    let rw = g.measure(&right, p * 0.95, FontId::Regular);
    g.text_in(&right, Rect::new(st.right() - rw - 10.0 * s, st.y, rw, st.h), p * 0.95, FontId::Regular, th.dim, Align::Left);
    let lr = Rect::new(st.x + 10.0 * s, st.y, (st.w - rw - 30.0 * s).max(0.0), st.h);
    let left = ellipsize(g, &left, lr.w, p * 0.95, FontId::Regular);
    g.text_in(&left, lr, p * 0.95, FontId::Regular, th.dim, Align::Left);
}

/// The scroll position for a pointer at `y` on the scroll bar.
pub fn scroll_for(l: &Layout, rows: usize, page: usize, y: f32) -> usize {
    if rows <= page || l.track.h <= 0.0 {
        return 0;
    }
    let f = ((y - l.track.y) / l.track.h).clamp(0.0, 1.0);
    ((rows - page) as f32 * f).round() as usize
}

/// Which files of a place go, as said in the details.
fn filter_text(f: Filter) -> String {
    match f {
        Filter::All => tr("everything in it"),
        Filter::OlderThanHours(h) => tf("files not changed for {} hours", &[&h.to_string()]),
        Filter::Prefix(p) => tf("files whose name starts with {}", &[p]),
        Filter::Named(n) => n.to_string(),
    }
}

/// The lines of the details panel: (text, kind) where kind is 0 title,
/// 1 heading, 2 text, 3 dim.
fn details_lines(app: &App) -> Vec<(String, u8)> {
    let mut out = Vec::new();
    let Some(row) = app.sel.and_then(|r| app.rows.get(r)) else { return out };
    match *row {
        Row::Group(gi) => {
            out.push((group_label(app.groups[gi]), 0));
            let (n, b) = group_totals(app, gi);
            out.push((tf("{} in {} files", &[&format::size(b), &format::count(n)]), 2));
            for i in app.group_items(gi) {
                let it = &app.items[i];
                let (files, size) = counts(it);
                let what = if size.is_empty() { tr(it.target.name) } else { format!("{}: {size} ({files})", tr(it.target.name)) };
                out.push((what, 3));
            }
        }
        Row::Item(i) => {
            let it = &app.items[i];
            out.push((format!("{} — {}", group_label(it.target.group), tr(it.target.name)), 0));
            out.push((tr(it.target.about), 2));
            let note = note_text(it);
            if !note.is_empty() {
                out.push((note, 2));
            }
            if let Some(f) = it.found().filter(|f| f.count() > 0) {
                out.push((tf("{} in {} files", &[&format::size(f.bytes), &format::count(f.count())]), 2));
            }
            if it.open {
                out.push((tr("Close it to clean everything: files in use are left as they are."), 3));
            }
            out.push((tr("Where"), 1));
            if it.target.kind == Kind::RecycleBin {
                out.push((tr("The Recycle Bin of every drive."), 3));
            }
            for pl in &it.target.places {
                out.push((pl.dir.to_string_lossy().into_owned(), 2));
                out.push((filter_text(pl.filter), 3));
            }
            out.push((tr("Files are deleted for good, not to the Recycle Bin."), 3));
        }
    }
    out
}

/// The details as text (to copy).
pub fn details_text(app: &App) -> String {
    details_lines(app).into_iter().map(|(t, _)| t).collect::<Vec<_>>().join("\n")
}

/// How tall the details are, to scroll them.
pub fn details_height(g: &mut Gfx, app: &App, l: &Layout, s: f32) -> f32 {
    let w = l.details.w - 32.0 * s;
    let mut h = 16.0 * s;
    for (text, kind) in details_lines(app) {
        let (size, font) = line_style(kind, s);
        let n = if kind == 2 && !text.contains(['\\', '/']) { wrap_words(g, &text, w, size, font).len() } else { 1 };
        h += n as f32 * size * 1.45 + if kind <= 1 { 10.0 * s } else { 2.0 * s };
    }
    h
}

fn line_style(kind: u8, s: f32) -> (f32, FontId) {
    match kind {
        0 => (15.0 * s, FontId::Semibold),
        1 => (13.0 * s, FontId::Semibold),
        3 => (11.5 * s, FontId::Regular),
        _ => (12.5 * s, FontId::Regular),
    }
}

fn draw_details(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32) {
    let d = l.details;
    if d.w <= 0.0 {
        return;
    }
    g.fill_rect(d, th.panel);
    g.fill_rect(Rect::new(d.x, d.y, 1.0, d.h), th.border);
    g.set_clip(Some(d));
    let w = d.w - 32.0 * s;
    let x = d.x + 16.0 * s;
    let mut y = d.y + 16.0 * s - app.details_scroll;
    for (text, kind) in details_lines(app) {
        let (size, font) = line_style(kind, s);
        let colour = if kind == 3 { th.dim } else { th.text };
        let lh = size * 1.45;
        if kind == 1 {
            y += 8.0 * s;
        }
        // Paths are cut in the middle; sentences wrap.
        let lines = if text.contains(['\\', '/']) && !text.contains(' ') || kind == 0 {
            vec![if kind == 0 { ellipsize(g, &text, w, size, font) } else { ellipsize_middle(g, &text, w, size, font) }]
        } else {
            wrap_words(g, &text, w, size, font)
        };
        for line in lines {
            g.text_in(&line, Rect::new(x, y, w, lh), size, font, colour, Align::Left);
            y += lh;
        }
        y += if kind == 0 { 8.0 * s } else { 2.0 * s };
    }
    g.set_clip(None);
}

/// The status bar: the row selected (left), and what is ticked or freed
/// (right).
pub fn status_text(app: &App) -> (String, String) {
    let left = match app.note.clone() {
        Some(n) => n,
        None => match app.selected_item() {
            Some(it) => match it.found() {
                Some(f) => format!("{}: {}", tr(it.target.name), tf("{} in {} files", &[&format::size(f.bytes), &format::count(f.count())])),
                None => tr(it.target.name),
            },
            None => String::new(),
        },
    };
    let mut right = tf("{} ticked", &[&format::count(app.chosen().len() as u64)]);
    if let Some(c) = &app.freed {
        right += &format!(" · {}", tf("{} freed", &[&format::size(c.bytes)]));
    }
    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{set_lang, Lang};
    use std::path::PathBuf;
    use tiny_skia::Pixmap;

    /// Draws the window to `target/screens/*.png` for a visual check:
    /// `cargo test -p rclean --lib screenshots -- --ignored`.
    #[test]
    #[ignore]
    fn screenshots() {
        let Ok(mut g) = Gfx::new() else { return };
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&out).unwrap();
        let icon_px = crate::ui::appicon::render(256);
        let mut icon = Pixmap::new(256, 256).unwrap();
        for (d, s) in icon.pixels_mut().iter_mut().zip(icon_px.chunks(4)) {
            *d = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
        }
        icon.save_png(out.join("icon.png")).unwrap();
        let mut shot = |name: &str, app: &App, dark: bool, hot: Hot, w: u32, h: u32| {
            g.resize(w, h);
            let full = Rect::new(0.0, 0.0, w as f32, h as f32);
            let th = Theme::new(dark, None);
            let l = layout(&mut g, app, full, 1.0, false);
            let enabled = |c: crate::commands::Command| c != crate::commands::Command::OpenFolder || app.selected_item().is_some();
            draw(&mut g, app, &l, &th, 1.0, hot, &enabled);
            g.pix.save_png(out.join(format!("{name}.png"))).unwrap();
        };
        set_lang(Lang::EnUS);
        let mut app = crate::app::tests::sample();
        shot("sample", &app, false, Hot::Action(1), 1200, 560);
        shot("sample-dark", &app, true, Hot::Row(5), 1200, 560);
        app.select(5);
        shot("sample-browser", &app, false, Hot::None, 1200, 560);
        app.busy = Some(crate::app::Busy { cleaning: true, done: 1, total: 3, name: "Cache".into(), phase: 0.3, stopping: false });
        shot("cleaning", &app, false, Hot::Stop, 1200, 560);
        app.busy = None;
        set_lang(Lang::PtPT);
        shot("sample-pt", &app, false, Hot::None, 1200, 560);
        shot("sample-narrow", &app, false, Hot::None, 700, 500);
        set_lang(Lang::EnUS);
        // This computer, analysed for real.
        let targets = crate::targets::all(&crate::targets::Env::here());
        let mut app = App::new(targets, rt_sys_admin(), &[]);
        let stop = std::sync::atomic::AtomicBool::new(false);
        let running = crate::procs::running();
        for i in 0..app.items.len() {
            if app.items[i].usable() {
                let f = crate::clean::analyse(&app.items[i].target, &stop);
                app.items[i].open = crate::procs::any_running(&running, app.items[i].target.process);
                app.set_found(i, f);
            }
        }
        shot("here", &app, false, Hot::None, 1200, 700);
    }

    fn rt_sys_admin() -> bool {
        #[cfg(windows)]
        return rt_sys::win::elevated();
        #[cfg(not(windows))]
        false
    }
}
