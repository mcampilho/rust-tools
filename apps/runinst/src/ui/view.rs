//! The window, drawn each frame: the search and the choices of what to show,
//! a line about the users whose programs could not be read, the list of
//! programs, the details of the one selected, and the status bar.
//!
//! [`layout`] decides where things go; `main` uses the same [`Layout`] to
//! find what the pointer is on.

use super::gfx::{Align, Color, FontId, Gfx, Rect};
use super::theme::Theme;
use crate::app::{App, Col, Mark, Tab, Whose};
use crate::format;
use crate::i18n::{tf, tr};
use crate::inventory::{Bits, Program, Scope, Source};
use crate::shortcuts::Place;
use rt_gfx::dialog::{ellipsize, ellipsize_middle};
use std::collections::HashMap;
use tiny_skia::Pixmap;

/// What the pointer is on, for hover effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Hot {
    #[default]
    None,
    /// A tab (index into `App::tabs`).
    Tab(usize),
    Whose(usize),
    System,
    Updates,
    Restart,
    /// The search box.
    Search,
    /// A button above the list (index into `commands::ACTIONS`).
    Action(usize),
    /// "Stop after this one", while uninstalling.
    Stop,
    Header(Col),
    /// The box in the header: tick all shown, or untick all.
    CheckAll,
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
    /// The tabs, when there is more than one list.
    pub tab_bar: Rect,
    pub tabs: Vec<Rect>,
    /// The buttons that act on the programs selected (or, while
    /// uninstalling, what is going on and the stop button).
    pub action_bar: Rect,
    pub actions: Vec<Rect>,
    pub stop: Rect,
    pub tools: Rect,
    pub search: Rect,
    pub whose: Vec<Rect>,
    pub system: Rect,
    pub updates: Rect,
    /// The line about other users, and its button.
    pub banner: Rect,
    pub restart: Rect,
    pub header: Rect,
    pub cols: Vec<(Col, f32, f32)>,
    pub list: Rect,
    pub row_h: f32,
    pub track: Rect,
    /// The details of the program selected (empty when the window is narrow).
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

    /// The width of the boxes to tick, at the start of the Name column.
    pub fn check_area(&self) -> (f32, f32) {
        let x = self.cols.iter().find(|c| c.0 == Col::Name).map_or(self.list.x, |c| c.1);
        (x, x + self.row_h * 1.25)
    }

    pub fn page_rows(&self) -> usize {
        if self.row_h <= 0.0 {
            return 1;
        }
        ((self.list.h / self.row_h).floor() as usize).max(1)
    }
}

/// Whether to say that other users' programs need administrator rights.
pub fn needs_admin(app: &App, admin: bool) -> bool {
    !admin && app.found.as_ref().is_some_and(|f| !f.inv.unread_users.is_empty())
}

pub fn whose_label(w: Whose) -> String {
    rt_core::i18n::strip_mnemonic(&crate::commands::Command::Whose(Whose::ALL.iter().position(|x| *x == w).unwrap_or(0) as u8).label())
}

/// For whom, as shown: `Everyone`, `Me`, or a user's name.
pub fn scope_text(s: &Scope) -> String {
    match s {
        Scope::Machine => tr("Everyone"),
        Scope::User { current: true, .. } => tr("Me"),
        Scope::User { name, .. } => name.clone(),
    }
}

/// A source, as shown in the list.
pub fn source_label(s: Source) -> String {
    match s {
        Source::Registry => tr("Registry"),
        Source::Msi => tr("Installer"),
        Source::Store => tr("Store"),
        Source::Winget => "winget".into(),
        Source::Scoop => "Scoop".into(),
        Source::Chocolatey => "Chocolatey".into(),
    }
}

pub fn sources_text(s: &[Source]) -> String {
    s.iter().map(|x| source_label(*x)).collect::<Vec<_>>().join(" · ")
}

/// A tab's name, with how many it holds.
pub fn tab_label(app: &App, tab: Tab) -> String {
    let n = match tab {
        Tab::Leftovers => app.leftovers.as_ref().map_or(0, |l| l.items.len()),
        t => app.found.as_ref().map_or(0, |f| f.list(t).len()),
    };
    let name = match tab {
        Tab::Programs => tr("Programs"),
        Tab::Scoop => "Scoop".into(),
        Tab::Chocolatey => "Chocolatey".into(),
        Tab::Leftovers => tr("Leftovers"),
    };
    format!("{name} ({})", format::count(n as u64))
}

/// Where everything goes in a window of `full` at scale `s`.
pub fn layout(g: &mut Gfx, app: &App, full: Rect, s: f32, menu_bar: bool, admin: bool) -> Layout {
    let mut l = Layout { full, ..Layout::default() };
    let mut area = full;
    if menu_bar {
        l.bar = area.take_top((28.0 * s).round());
    }
    l.status = area.take_bottom((24.0 * s).round());
    let tabs = app.tabs();
    if tabs.len() > 1 {
        l.tab_bar = area.take_top((38.0 * s).round());
        let mut x = l.tab_bar.x + 10.0 * s;
        let p = px(s);
        for t in tabs {
            let w = (g.measure(&tab_label(app, t), p, FontId::Semibold) + 28.0 * s).round();
            l.tabs.push(Rect::new(x, l.tab_bar.y + 6.0 * s, w, l.tab_bar.h - 6.0 * s));
            x += w + 2.0 * s;
        }
    }
    l.action_bar = area.take_top((42.0 * s).round());
    {
        let b = l.action_bar;
        let h = (28.0 * s).round();
        let y = (b.y + (b.h - h) / 2.0).round();
        let p = px(s);
        if app.busy.as_ref().is_some_and(|b| !b.modify) {
            let w =(g.measure(&tr("Stop after this one"), p, FontId::Semibold) + 28.0 * s).round();
            l.stop = Rect::new(b.right() - w - 10.0 * s, y, w, h);
        } else {
            let mut x = b.x + 10.0 * s;
            for &c in crate::commands::actions(app.tab) {
                let w = (g.measure(&action_label(c), p, FontId::Semibold) + 28.0 * s).round();
                // What does not fit is left to the menus.
                let r = Rect::new(x, y, w, h);
                l.actions.push(if r.right() <= b.right() - 10.0 * s { r } else { Rect::default() });
                x += w + 6.0 * s;
            }
        }
    }
    l.tools = area.take_top((44.0 * s).round());
    let t = l.tools;
    let h = (28.0 * s).round();
    let y = (t.y + (t.h - h) / 2.0).round();
    let sw = (280.0 * s).min(t.w * 0.32).round();
    l.search = Rect::new(t.x + 10.0 * s, y, sw, h);
    // The choices, each as wide as its label; when they do not fit, the
    // search gives way first, then they shrink alike.
    let p = px(s) * 0.95;
    let mut labels: Vec<String> = Whose::ALL.iter().map(|w| whose_label(*w)).collect();
    labels.push(rt_core::i18n::strip_mnemonic(&tr("&System Components")));
    labels.push(rt_core::i18n::strip_mnemonic(&tr("&Updates")));
    let widths: Vec<f32> = labels.iter().map(|l| (g.measure(l, p, FontId::Regular) + 24.0 * s).round()).collect();
    let gaps = 4.0 * s * 5.0 + 10.0 * s + 12.0 * s + 20.0 * s;
    let want: f32 = widths.iter().sum::<f32>() + gaps;
    let room = t.w - 10.0 * s;
    let search_w = (room - want).clamp(140.0 * s, 280.0 * s).round();
    l.search.w = search_w;
    let left = room - search_w - gaps;
    let shrink = (left / widths.iter().sum::<f32>()).min(1.0);
    let mut x = l.search.right() + 12.0 * s;
    // The choices are about programs: not on the list of leftovers.
    let choices = if app.tab == Tab::Leftovers { 0 } else { widths.len() };
    for (i, w) in widths.iter().enumerate().take(choices) {
        let w = (w * shrink).floor();
        let r = Rect::new(x, y, w, h);
        match i {
            0..=3 => l.whose.push(r),
            4 => {
                x += 10.0 * s;
                l.system = Rect::new(x, y, w, h);
            }
            _ => l.updates = r,
        }
        x = if i == 4 { l.system.right() } else { r.right() } + 4.0 * s;
    }
    if needs_admin(app, admin) {
        l.banner = area.take_top((34.0 * s).round());
        let bw = (210.0 * s).round();
        let bh = (26.0 * s).round();
        l.restart = Rect::new(l.banner.right() - bw - 10.0 * s, (l.banner.y + (l.banner.h - bh) / 2.0).round(), bw, bh);
    }
    if area.w >= 860.0 * s {
        let dw = (400.0 * s).min(area.w * 0.4).round();
        l.details = area.take_right(dw);
    }
    l.header = area.take_top((26.0 * s).round());
    l.row_h = (24.0 * s).round();
    l.track = area.take_right((10.0 * s).round());
    l.list = area;
    let mut x1 = area.right();
    let source = if app.tab == Tab::Programs { 150.0 } else { 0.0 };
    // Leftovers: what (Name), its kind (Source), size, the program (Publisher).
    let wants: &[(Col, f32, f32)] = if app.tab == Tab::Leftovers {
        &[(Col::Source, 170.0, 420.0), (Col::Size, 80.0, 340.0), (Col::Publisher, 190.0, 260.0)]
    } else {
        &[(Col::Source, source, 640.0), (Col::For, 90.0, 520.0), (Col::Installed, 90.0, 440.0), (Col::Size, 80.0, 360.0), (Col::Version, 130.0, 300.0), (Col::Publisher, 160.0, 170.0)]
    };
    for &(col, want, min_rest) in wants {
        if want <= 0.0 {
            continue;
        }
        let w = (want * s).round();
        if x1 - w - area.x >= min_rest * s {
            x1 -= w;
            l.cols.push((col, x1, w));
        }
    }
    l.cols.push((Col::Name, area.x, x1 - area.x));
    l
}

fn px(s: f32) -> f32 {
    12.5 * s
}

/// Where the search's text goes, inside its box.
fn search_inner(sr: Rect, s: f32) -> Rect {
    Rect::new(sr.x + 26.0 * s, sr.y, sr.w - 32.0 * s, sr.h)
}

/// How far the search's text is moved left so its caret is in view.
fn search_shift(g: &mut Gfx, app: &App, w: f32, s: f32) -> f32 {
    let e = &app.search;
    let p = px(s);
    let total = g.measure(&e.text, p, FontId::Regular);
    if total <= w - 4.0 * s {
        return 0.0;
    }
    let caret = g.measure(&e.text[..rt_core::textedit::boundary(&e.text, e.caret)], p, FontId::Regular);
    (caret - w + 6.0 * s).max(0.0).min(total - w + 6.0 * s)
}

/// The place in the search's text for a click at `x`.
pub fn search_offset(g: &mut Gfx, app: &App, l: &Layout, x: f32, s: f32) -> usize {
    let inner = search_inner(l.search, s);
    let shift = search_shift(g, app, inner.w, s);
    let p = px(s);
    rt_core::textedit::offset_at(&app.search.text, x - inner.x + shift, |c| g.advance(c, p, FontId::Regular))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tick {
    Off,
    On,
    /// Some of those shown (the header's box).
    Some,
}

/// A box to tick (as in rdup), at `x`, centred on `cy`.
pub(super) fn check_box(g: &mut Gfx, x: f32, cy: f32, th: &Theme, s: f32, state: Tick) {
    let b = (15.0 * s).round();
    let r = Rect::new(x.round(), (cy - b / 2.0).round(), b, b);
    let fg = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
    match state {
        Tick::On => {
            g.fill_rrect(r, 3.0 * s, th.accent);
            let pts = [(r.x + b * 0.24, r.y + b * 0.52), (r.x + b * 0.43, r.y + b * 0.70), (r.x + b * 0.77, r.y + b * 0.32)];
            g.polyline(&pts, 1.8 * s, fg, false);
        }
        Tick::Some => {
            g.fill_rrect(r, 3.0 * s, th.accent);
            g.fill_rect(Rect::new(r.x + b * 0.25, (r.center_y() - 1.0 * s).round(), b * 0.5, (2.0 * s).round()), fg);
        }
        Tick::Off => {
            g.fill_rrect(r, 3.0 * s, th.bg);
            g.stroke_rrect(r, 3.0 * s, 1.2 * s, th.dim);
        }
    }
}

/// The colour of a program in the queue.
fn mark_color(m: Mark, th: &Theme) -> Color {
    match m {
        Mark::Waiting => th.dim,
        Mark::Running => th.accent,
        Mark::Done => Color::rgb(if th.dark { 0x6CCB5F } else { 0x0F7B0F }),
        Mark::Failed => Color::rgb(if th.dark { 0xFF99A4 } else { 0xC42B1C }),
    }
}

/// What is wrong with a program, as shown.
pub fn problems_text(ps: &[crate::inventory::Problem]) -> String {
    use crate::inventory::Problem;
    ps.iter()
        .map(|p| {
            tr(match p {
                Problem::InstallFailed => "Install failed",
                Problem::ManifestRemoved => "Manifest removed",
                Problem::UninstallerMissing => "Uninstaller missing",
            })
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The word on a program in the queue.
fn mark_text(m: Mark, modify: bool) -> String {
    tr(match m {
        Mark::Waiting => "Waiting",
        Mark::Running if modify => "Changing…",
        Mark::Running => "Uninstalling…",
        Mark::Done => "Done",
        Mark::Failed => "Did not work",
    })
}

/// A command's label on a button: no access key, no dots.
pub fn action_label(c: crate::commands::Command) -> String {
    rt_core::i18n::strip_mnemonic(&c.label()).trim_end_matches("...").trim_end_matches('…').to_string()
}

/// The buttons above the list, or what is being uninstalled.
fn draw_actions(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot, enabled: &dyn Fn(crate::commands::Command) -> bool) {
    let b = l.action_bar;
    g.fill_rect(b, th.panel);
    g.fill_rect(Rect::new(b.x, b.bottom() - 1.0, b.w, 1.0), th.border);
    let p = px(s);
    if let Some(busy) = &app.busy {
        let text = if busy.modify {
            tf("Changing {}…", &[&busy.name])
        } else if busy.stopping {
            tf("Uninstalling {} ({} of {}); stopping after this one…", &[&busy.name, &busy.index.to_string(), &busy.total.to_string()])
        } else {
            tf("Uninstalling {} ({} of {})…", &[&busy.name, &busy.index.to_string(), &busy.total.to_string()])
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
        // How far the queue is, with a gleam running over it.
        let bar = Rect::new(b.x, b.bottom() - (4.0 * s).round(), b.w, (4.0 * s).round());
        g.fill_rect(bar, th.bar_track);
        let done = (bar.w * app.progress()).round();
        g.fill_rect(Rect::new(bar.x, bar.y, done, bar.h), th.accent);
        let gw = (bar.w * 0.15).max(40.0 * s);
        let gx = bar.x - gw + (bar.w + gw) * busy.phase;
        g.set_clip(Some(bar));
        g.fill_rect(Rect::new(gx, bar.y, gw, bar.h), th.accent.with_alpha(0x70));
        g.set_clip(None);
        let r = l.stop;
        if r.w <= 0.0 {
            return;
        }
        if busy.stopping {
            g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
            g.text_in(&tr("Stop after this one"), r, p, FontId::Semibold, th.dim, Align::Center);
        } else {
            g.fill_rrect(r, 5.0 * s, if hot == Hot::Stop { th.hover } else { th.panel });
            g.stroke_rrect(r, 5.0 * s, 1.0, if hot == Hot::Stop { th.accent } else { th.border });
            g.text_in(&tr("Stop after this one"), r, p, FontId::Semibold, th.text, Align::Center);
        }
        return;
    }
    for (i, (r, c)) in l.actions.iter().zip(crate::commands::actions(app.tab).iter().copied()).enumerate() {
        if r.w <= 0.0 {
            continue;
        }
        let on = enabled(c);
        let lit = on && hot == Hot::Action(i);
        g.fill_rrect(*r, 5.0 * s, if lit { th.hover } else { th.panel });
        g.stroke_rrect(*r, 5.0 * s, 1.0, if lit { th.accent } else { th.border });
        g.text_in(&action_label(c), *r, p, FontId::Semibold, if on { th.text } else { th.dim.with_alpha(0x80) }, Align::Center);
    }
    let n = app.chosen().len();
    let ticking = !app.ticked.is_empty() || app.tab == Tab::Leftovers;
    if n > 1 || ticking {
        let text = if ticking { tf("{} ticked", &[&format::count(n as u64)]) } else { tf("{} selected", &[&format::count(n as u64)]) };
        let w = g.measure(&text, p, FontId::Regular);
        let x0 = l.actions.iter().map(|r| r.right()).fold(b.x, f32::max) + 12.0 * s;
        if b.right() - 12.0 * s - w > x0 {
            g.text_in(&text, Rect::new(b.right() - 12.0 * s - w, b.y, w, b.h), p, FontId::Regular, th.dim, Align::Left);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn chip(g: &mut Gfx, r: Rect, label: &str, th: &Theme, s: f32, on: bool, hot: bool) {
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
        _ => th.text,
    };
    let p = px(s) * 0.95;
    let text = ellipsize(g, label, r.w - 10.0 * s, p, FontId::Regular);
    g.text_in(&text, r, p, FontId::Regular, fg, Align::Center);
}

fn col_title(c: Col, tab: Tab) -> String {
    if tab == Tab::Leftovers {
        return tr(match c {
            Col::Name => "Where",
            Col::Source => "Kind",
            Col::Publisher => "Program",
            _ => "Size",
        });
    }
    tr(match c {
        Col::Name => "Name",
        Col::Publisher if tab == Tab::Scoop => "Bucket",
        Col::Publisher if tab == Tab::Chocolatey => "Authors",
        Col::Publisher => "Publisher",
        Col::Version => "Version",
        Col::Size => "Size",
        Col::Installed => "Installed",
        Col::For => "For",
        Col::Source => "Source",
    })
}

fn align_of(c: Col) -> Align {
    match c {
        Col::Size | Col::Installed => Align::Right,
        _ => Align::Left,
    }
}

/// The version, with the newer one winget knows: `9.11 → 9.12`.
fn version_text(p: &Program) -> String {
    match p.winget.as_ref().and_then(|w| w.available.as_ref()) {
        Some(new) => format!("{} → {new}", p.version),
        None => p.version.clone(),
    }
}

fn date_text(d: Option<(i32, u32, u32)>) -> String {
    d.map_or(String::new(), |(y, m, d)| format!("{y:04}-{m:02}-{d:02}"))
}

fn cell(p: &Program, c: Col) -> String {
    match c {
        Col::Name => p.name.clone(),
        Col::Publisher => p.publisher.clone(),
        Col::Version => version_text(p),
        Col::Size => p.size.map_or(String::new(), format::size),
        Col::Installed => date_text(p.date),
        Col::For => scope_text(&p.scope),
        Col::Source => sources_text(&p.sources),
    }
}

/// A program without an icon of its own: a small window.
fn default_icon(g: &mut Gfx, r: Rect, th: &Theme, s: f32) {
    g.fill_rrect(r, 2.5 * s, th.panel);
    g.stroke_rrect(r, 2.5 * s, 1.0, th.dim);
    g.fill_rect(Rect::new(r.x, r.y, r.w, (r.h * 0.28).round()), th.dim);
}

/// Draws the window.
#[allow(clippy::too_many_arguments)]
pub fn draw(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot, icons: &HashMap<usize, Pixmap>, enabled: &dyn Fn(crate::commands::Command) -> bool) {
    g.clear(th.bg);
    draw_actions(g, app, l, th, s, hot, enabled);
    let p = px(s);
    let pad = 8.0 * s;
    // The tabs.
    if l.tab_bar.h > 0.0 {
        let b = l.tab_bar;
        g.fill_rect(b, th.status);
        g.fill_rect(Rect::new(b.x, b.bottom() - 1.0, b.w, 1.0), th.border);
        for (i, (r, tab)) in l.tabs.iter().zip(app.tabs()).enumerate() {
            let on = app.tab == tab;
            if on {
                g.fill_rrect(Rect::new(r.x, r.y, r.w, r.h + 6.0 * s), 6.0 * s, th.panel);
                g.fill_rect(Rect::new(r.x, r.bottom() - 2.0 * s, r.w, 2.0 * s), th.accent);
            } else if hot == Hot::Tab(i) {
                g.fill_rrect(*r, 6.0 * s, th.hover);
            }
            g.text_in(&tab_label(app, tab), *r, p, FontId::Semibold, if on { th.text } else { th.dim }, Align::Center);
        }
    }
    // The tools: the search and the choices.
    let t = l.tools;
    g.fill_rect(t, th.panel);
    g.fill_rect(Rect::new(t.x, t.bottom() - 1.0, t.w, 1.0), th.border);
    let sr = l.search;
    g.fill_rrect(sr, 4.0 * s, th.bg);
    if app.search_focus {
        g.stroke_rrect(sr, 4.0 * s, (2.0 * s).round(), th.accent);
    } else {
        g.stroke_rrect(sr, 4.0 * s, 1.0, if app.search.text.is_empty() { th.border } else { th.accent });
    }
    // A magnifier.
    let (mx, my) = (sr.x + 14.0 * s, sr.center_y() - 1.0 * s);
    g.polyline(
        &(0..=16).map(|i| {
            let a = i as f32 / 16.0 * std::f32::consts::TAU;
            (mx + 4.5 * s * a.cos(), my + 4.5 * s * a.sin())
        })
        .collect::<Vec<_>>(),
        1.5 * s,
        th.dim,
        false,
    );
    g.polyline(&[(mx + 3.3 * s, my + 3.3 * s), (mx + 7.0 * s, my + 7.0 * s)], 1.5 * s, th.dim, false);
    let inner = search_inner(sr, s);
    let caret_w = (1.5 * s).max(1.0);
    if app.search.text.is_empty() {
        let hint = ellipsize(g, &tr("Type to find a program"), inner.w - 4.0 * s, p, FontId::Regular);
        g.text_in(&hint, Rect::new(inner.x + 4.0 * s, inner.y, inner.w - 4.0 * s, inner.h), p, FontId::Regular, th.dim, Align::Left);
        if app.search_focus {
            g.fill_rect(Rect::new(inner.x.round(), sr.y + 6.0 * s, caret_w, sr.h - 12.0 * s), th.text);
        }
    } else {
        // The text, moved left so the caret stays in view; the selection
        // behind it.
        let e = &app.search;
        let shift = search_shift(g, app, inner.w, s);
        let x_of = |g: &mut Gfx, at: usize| inner.x - shift + g.measure(&e.text[..at], p, FontId::Regular);
        g.set_clip(Some(inner));
        if let Some((a, b)) = e.selection() {
            let (xa, xb) = (x_of(g, a), x_of(g, b));
            let c = if app.search_focus { th.selection } else { th.hover };
            g.fill_rect(Rect::new(xa, sr.y + 5.0 * s, xb - xa, sr.h - 10.0 * s), c);
        }
        g.text_in(&e.text, Rect::new(inner.x - shift, inner.y, inner.w + shift + 2000.0, inner.h), p, FontId::Regular, th.text, Align::Left);
        if app.search_focus {
            let cx = x_of(g, rt_core::textedit::boundary(&e.text, e.caret));
            g.fill_rect(Rect::new(cx.round(), sr.y + 6.0 * s, caret_w, sr.h - 12.0 * s), th.text);
        }
        g.set_clip(None);
    }
    for (i, (r, w)) in l.whose.iter().zip(Whose::ALL).enumerate() {
        chip(g, *r, &whose_label(w), th, s, app.whose == w, hot == Hot::Whose(i));
    }
    if l.system.w > 0.0 {
        chip(g, l.system, &rt_core::i18n::strip_mnemonic(&tr("&System Components")), th, s, app.show_system, hot == Hot::System);
        chip(g, l.updates, &rt_core::i18n::strip_mnemonic(&tr("&Updates")), th, s, app.show_updates, hot == Hot::Updates);
    }
    // Other users' programs need administrator rights.
    if l.banner.h > 0.0 {
        let b = l.banner;
        g.fill_rect(b, th.warn.with_alpha(0x30));
        g.fill_rect(Rect::new(b.x, b.bottom() - 1.0, b.w, 1.0), th.border);
        let users = app.found.as_ref().map(|f| f.inv.unread_users.join(", ")).unwrap_or_default();
        let text = tf("The programs of other users ({}) need administrator rights to be seen.", &[&users]);
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
    for &(col, x, w) in &l.cols {
        let r = Rect::new(x + pad, l.header.y, (w - 2.0 * pad).max(0.0), l.header.h);
        if hot == Hot::Header(col) {
            g.fill_rect(Rect::new(x, l.header.y, w, l.header.h - 1.0), th.hover);
        }
        let mut text = col_title(col, app.tab);
        if app.sort == col && app.tab != Tab::Leftovers {
            text.push_str(if app.descending { " ▾" } else { " ▴" });
        }
        let mut r = r;
        if col == Col::Name {
            // The box that ticks all shown, or unticks all.
            let (x0, x1) = l.check_area();
            if hot == Hot::CheckAll {
                g.fill_rect(Rect::new(x0, l.header.y, x1 - x0, l.header.h - 1.0), th.hover);
            }
            let state = match app.ticks_shown() {
                (false, _) => Tick::Off,
                (true, true) => Tick::On,
                (true, false) => Tick::Some,
            };
            check_box(g, x0 + pad, l.header.center_y(), th, s, state);
            r = Rect::new(x1, r.y, (r.right() - x1).max(0.0), r.h);
        }
        let text = ellipsize(g, &text, r.w, p, FontId::Semibold);
        g.text_in(&text, r, p, FontId::Semibold, th.dim, align_of(col));
    }
    if app.tab == Tab::Leftovers {
        super::leftovers_view::draw_rows(g, app, l, th, s, hot);
    } else if app.found.is_none() || app.loading && app.rows.is_empty() {
        g.text_in(&tr("Reading the programs installed…"), l.list, 15.0 * s, FontId::Regular, th.dim, Align::Center);
    } else if app.rows.is_empty() {
        let text = if cfg!(windows) {
            tr("No program matches.")
        } else {
            tr("runinst lists the programs of Windows; Linux packages come in a later version.")
        };
        g.text_in(&text, l.list, 15.0 * s, FontId::Regular, th.dim, Align::Center);
    }
    g.set_clip(Some(l.list));
    let (asc, desc) = g.metrics(p, FontId::Regular);
    let end = (app.scroll + l.page_rows() + 1).min(app.rows.len());
    for r in app.scroll..end {
        let Some(prog) = app.program(app.rows[r]) else { continue };
        let y = l.list.y + (r - app.scroll) as f32 * l.row_h;
        let row = Rect::new(l.list.x, y, l.list.w, l.row_h);
        let base = (row.center_y() + (asc + desc) / 2.0).round();
        if app.is_marked(r) {
            g.fill_rect(row, th.selection);
        } else if hot == Hot::Row(r) {
            g.fill_rect(row, th.hover);
        }
        if app.sel == Some(r) && app.chosen().len() > 1 {
            g.stroke_rrect(row.inset(1.0), 0.0, 1.0, th.accent.with_alpha(0x90));
        }
        // In the queue: a colour and a word for how it is doing.
        let mark = app.mark_of(prog);
        if let Some(m) = mark {
            let c = mark_color(m, th);
            g.fill_rect(row, c.with_alpha(if m == Mark::Running { 0x38 } else { 0x20 }));
            g.fill_rect(Rect::new(row.x, row.y, (3.0 * s).round(), row.h), c);
        }
        for &(col, x, w) in &l.cols {
            if col == Col::Name {
                let (x0, x1) = l.check_area();
                if hot == Hot::Check(r) {
                    g.fill_rect(Rect::new(x0, row.y, x1 - x0, row.h), th.hover);
                }
                check_box(g, x0 + pad, row.center_y(), th, s, if app.is_ticked(r) { Tick::On } else { Tick::Off });
                let x = x1 - pad;
                let w = w - (x1 - x0) + pad;
                let ir = Rect::new((x + pad).round(), (row.center_y() - 8.0 * s).round(), (16.0 * s).round(), (16.0 * s).round());
                match icons.get(&app.rows[r]).filter(|_| app.tab == Tab::Programs) {
                    Some(pm) => g.draw_image(pm, ir),
                    None => default_icon(g, ir.inset(1.5 * s), th, s),
                }
                let tx = ir.right() + 8.0 * s;
                let mut room = (x + w - tx - pad).max(0.0);
                // A tag at the right: how it is doing in the queue, or what
                // is wrong with it.
                let tag_of = match mark {
                    Some(m) => Some((mark_text(m, app.busy.as_ref().is_some_and(|b| b.modify)), mark_color(m, th), m == Mark::Waiting)),
                    None if !prog.problems.is_empty() => Some((problems_text(&prog.problems), th.warn, false)),
                    None => None,
                };
                if let Some((word, c, dim)) = tag_of {
                    let ww = g.measure(&word, p * 0.92, FontId::Semibold);
                    let tag = Rect::new((x + w - pad - ww - 12.0 * s).round(), (row.center_y() - 9.0 * s).round(), (ww + 12.0 * s).round(), (18.0 * s).round());
                    if tag.w < room * 0.6 {
                        g.fill_rrect(tag, 9.0 * s, c.with_alpha(0x30));
                        g.text_in(&word, tag, p * 0.92, FontId::Semibold, if dim { th.dim } else { c.mix(th.text, 0.35) }, Align::Center);
                        room = (tag.x - tx - 6.0 * s).max(0.0);
                    }
                }
                let name = ellipsize(g, &prog.name, room, p, FontId::Regular);
                let fg = if prog.system || prog.update { th.dim } else { th.text };
                g.text(&name, tx, base, p, FontId::Regular, fg);
            } else {
                let c = Rect::new(x + pad, row.y, (w - 2.0 * pad).max(0.0), row.h);
                let text = ellipsize(g, &cell(prog, col), c.w, p, FontId::Regular);
                let update = col == Col::Version && prog.winget.as_ref().is_some_and(|w| w.available.is_some());
                let fg = if col == Col::Size { th.text } else if update { th.accent } else { th.dim };
                g.text_in(&text, c, p, FontId::Regular, fg, align_of(col));
            }
        }
    }
    g.set_clip(None);
    let page = l.page_rows();
    if app.rows.len() > page {
        let (y, h) = thumb(l, app.rows.len(), page, app.scroll);
        g.fill_rrect(Rect::new(l.track.x + 2.0 * s, y, l.track.w - 4.0 * s, h), 3.0 * s, th.scrollbar);
    }
    if l.details.w > 0.0 {
        if app.tab == Tab::Leftovers {
            super::leftovers_view::draw_details(g, app, l.details, th, s);
        } else {
            draw_details(g, app, l.details, th, s, icons);
        }
    }
    draw_status(g, app, l, th, s, hot);
}

/// The lines of the details of a program: (label, value).
pub fn details_lines(app: &App) -> Vec<(String, String)> {
    app.selected().map_or(Vec::new(), |p| lines_of(app.tab, p))
}

fn lines_of(tab: Tab, p: &Program) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut add = |k: &str, v: String| {
        if !v.trim().is_empty() {
            out.push((tr(k), v));
        }
    };
    add("Problem", problems_text(&p.problems));
    add(if tab == Tab::Scoop { "Bucket" } else { "Publisher" }, p.publisher.clone());
    add("Version", p.version.clone());
    add("Newer version (winget)", p.winget.as_ref().and_then(|w| w.available.clone()).unwrap_or_default());
    add("Found in", sources_text(&p.sources));
    add("winget id", p.winget.as_ref().map(|w| if w.repo.is_empty() { w.id.clone() } else { format!("{} ({})", w.id, w.repo) }).unwrap_or_default());
    add("Package", p.package.clone().unwrap_or_default());
    add("For", scope_text(&p.scope));
    add(
        "Kind",
        [
            match p.bits {
                Bits::B64 => tr("64-bit"),
                Bits::B32 => tr("32-bit"),
                Bits::Any => String::new(),
            },
            if p.msi.is_some() { tr("Windows Installer") } else { String::new() },
            if p.system { tr("System component") } else { String::new() },
            if p.update { tr("Update") } else { String::new() },
            if p.no_remove { tr("Cannot be removed") } else { String::new() },
        ]
        .into_iter()
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" · "),
    );
    add("Size", p.size.map_or(String::new(), format::size));
    add("Installed", date_text(p.date));
    add("Install folder", p.location.as_ref().map(|l| l.display().to_string()).unwrap_or_default());
    add("Uninstall command", p.uninstall.clone().unwrap_or_default());
    add("Quiet uninstall command", p.quiet.clone().unwrap_or_default());
    add("Modify command", p.modify.clone().unwrap_or_default());
    add("Product code", p.msi.clone().unwrap_or_default());
    add(if p.sources.contains(&Source::Registry) { "Registry key" } else { "Where" }, p.key.clone());
    out
}

fn place_text(pl: Place) -> String {
    match pl {
        Place::StartMenu => tr("Start menu"),
        Place::Desktop => tr("Desktop"),
    }
}

/// The text of everything known about the program selected (to copy).
pub fn details_text(app: &App) -> String {
    if app.tab == Tab::Leftovers {
        return super::leftovers_view::details_text(app);
    }
    let chosen = app.chosen_programs();
    if chosen.len() > 1 {
        // Each program selected, without their registry values.
        return chosen
            .iter()
            .map(|p| {
                let mut out = vec![p.name.clone()];
                out.extend(lines_of(app.tab, p).into_iter().map(|(k, v)| format!("{k}: {v}")));
                out.join("\n")
            })
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    let Some(p) = app.selected() else { return String::new() };
    let mut out = vec![p.name.clone()];
    out.extend(details_lines(app).into_iter().map(|(k, v)| format!("{k}: {v}")));
    for sc in app.selected_shortcuts() {
        out.push(format!("{} ({}, {}): {}", tr("Shortcut"), place_text(sc.place), scope_text(&sc.scope), sc.path.display()));
    }
    out.push(String::new());
    out.extend(p.values.iter().map(|(n, v)| format!("{n} = {v}")));
    out.join("\n")
}

fn draw_details(g: &mut Gfx, app: &App, area: Rect, th: &Theme, s: f32, icons: &HashMap<usize, Pixmap>) {
    g.fill_rect(area, th.panel);
    g.fill_rect(Rect::new(area.x, area.y, 1.0, area.h), th.border);
    let chosen = app.chosen_programs();
    if chosen.len() > 1 {
        return draw_several(g, app, &chosen, area, th, s);
    }
    let Some(p) = app.selected() else { return };
    let pad = 16.0 * s;
    let w = area.w - 2.0 * pad;
    let x = area.x + pad;
    g.set_clip(Some(area));
    let mut y = area.y + pad - app.details_scroll;
    let ir = Rect::new(x, y, (32.0 * s).round(), (32.0 * s).round());
    match app.selected_index().and_then(|i| icons.get(&i)).filter(|_| app.tab == Tab::Programs) {
        Some(pm) => g.draw_image(pm, ir),
        None => default_icon(g, ir.inset(2.0 * s), th, s),
    }
    let title = ellipsize(g, &p.name, w - ir.w - 10.0 * s, 15.0 * s, FontId::Semibold);
    g.text(&title, ir.right() + 10.0 * s, y + 21.0 * s, 15.0 * s, FontId::Semibold, th.text);
    y += ir.h + 14.0 * s;
    let small = 11.5 * s;
    let p12 = px(s);
    for (k, v) in details_lines(app) {
        g.text(&k, x, y + small, small, FontId::Regular, th.dim);
        let v = ellipsize_middle(g, &v, w, p12, FontId::Regular);
        g.text(&v, x, y + small + 17.0 * s, p12, FontId::Regular, th.text);
        y += 40.0 * s;
    }
    let shortcuts = app.selected_shortcuts();
    y += 6.0 * s;
    g.text(&tf("Shortcuts ({})", &[&shortcuts.len().to_string()]), x, y + 13.0 * s, 13.0 * s, FontId::Semibold, th.text);
    y += 24.0 * s;
    if shortcuts.is_empty() {
        g.text(&tr("None found."), x, y + p12, p12, FontId::Regular, th.dim);
        y += 22.0 * s;
    }
    for sc in shortcuts {
        let head = format!("{} · {}", place_text(sc.place), scope_text(&sc.scope));
        let head = ellipsize(g, &head, w, small, FontId::Regular);
        g.text(&head, x, y + small, small, FontId::Regular, th.dim);
        let name = ellipsize_middle(g, &sc.path.display().to_string(), w, p12, FontId::Regular);
        g.text(&name, x, y + small + 17.0 * s, p12, FontId::Regular, if sc.broken() { th.warn } else { th.text });
        y += 40.0 * s;
    }
    y += 6.0 * s;
    g.text(&tr(if p.sources.contains(&Source::Registry) { "Registry values" } else { "More" }), x, y + 13.0 * s, 13.0 * s, FontId::Semibold, th.text);
    y += 24.0 * s;
    for (n, v) in &p.values {
        let line = ellipsize(g, &format!("{n} = {v}"), w, small, FontId::Regular);
        g.text(&line, x, y + small, small, FontId::Regular, th.dim);
        y += 18.0 * s;
    }
    g.set_clip(None);
}

/// The details of several programs: how many, their size, and each one.
fn draw_several(g: &mut Gfx, app: &App, chosen: &[&Program], area: Rect, th: &Theme, s: f32) {
    let pad = 16.0 * s;
    let w = area.w - 2.0 * pad;
    let x = area.x + pad;
    g.set_clip(Some(area));
    let mut y = area.y + pad - app.details_scroll;
    g.text(&tf("{} programs selected", &[&format::count(chosen.len() as u64)]), x, y + 15.0 * s, 15.0 * s, FontId::Semibold, th.text);
    y += 30.0 * s;
    let size: u64 = chosen.iter().filter_map(|p| p.size).sum();
    let small = 11.5 * s;
    let p12 = px(s);
    if size > 0 {
        g.text(&tr("Size"), x, y + small, small, FontId::Regular, th.dim);
        g.text(&format::size(size), x, y + small + 17.0 * s, p12, FontId::Regular, th.text);
        y += 46.0 * s;
    }
    for p in chosen {
        let name = ellipsize(g, &p.name, w, p12, FontId::Regular);
        g.text(&name, x, y + p12, p12, FontId::Regular, th.text);
        let sub = [p.version.clone(), scope_text(&p.scope), p.size.map_or(String::new(), format::size)].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" · ");
        let sub = ellipsize(g, &sub, w, small, FontId::Regular);
        g.text(&sub, x, y + p12 + 16.0 * s, small, FontId::Regular, th.dim);
        y += 40.0 * s;
    }
    g.set_clip(None);
}

/// How tall the details are, to bound their scrolling.
pub fn details_height(app: &App, s: f32) -> f32 {
    if app.tab == Tab::Leftovers {
        return super::leftovers_view::details_height(app, s);
    }
    let n = app.chosen().len();
    if n > 1 {
        return 16.0 * s + 30.0 * s + 46.0 * s + n as f32 * 40.0 * s + 16.0 * s;
    }
    let Some(p) = app.selected() else { return 0.0 };
    let n = app.selected_shortcuts().len().max(1) as f32;
    16.0 * s + 46.0 * s + details_lines(app).len() as f32 * 40.0 * s + 30.0 * s + n * 40.0 * s + 30.0 * s + p.values.len() as f32 * 18.0 * s + 16.0 * s
}

pub fn thumb(l: &Layout, rows: usize, page: usize, scroll: usize) -> (f32, f32) {
    let track = l.track;
    let h = (track.h * page as f32 / rows as f32).max(24.0f32.min(track.h));
    let max = rows.saturating_sub(page).max(1);
    let y = track.y + (track.h - h) * scroll as f32 / max as f32;
    (y, h)
}

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
    let (left, right) = status_text(app, match hot {
        Hot::Row(i) => Some(i),
        _ => None,
    });
    let rw = g.measure(&right, p, FontId::Regular);
    g.text_in(&right, Rect::new(st.right() - rw - pad, st.y, rw, st.h), p, FontId::Regular, th.dim, Align::Left);
    let lw = (st.w - rw - 3.0 * pad).max(0.0);
    let left = ellipsize_middle(g, &left, lw, p, FontId::Regular);
    g.text_in(&left, Rect::new(st.x + pad, st.y, lw, st.h), p, FontId::Regular, th.text, Align::Left);
}

/// The status bar: (the row pointed or the last note; the totals).
pub fn status_text(app: &App, hover: Option<usize>) -> (String, String) {
    if app.tab == Tab::Leftovers {
        return super::leftovers_view::status_text(app, hover);
    }
    let Some(f) = &app.found else { return (String::new(), String::new()) };
    let left = match hover.and_then(|r| app.rows.get(r)).and_then(|&i| f.list(app.tab).get(i)) {
        Some(p) => format!("{} · {}", p.name, p.location.as_ref().map(|l| l.display().to_string()).or(p.uninstall.clone()).unwrap_or_default()),
        None => app.note.clone().unwrap_or_default(),
    };
    let list = f.list(app.tab);
    let size: u64 = app.rows.iter().filter_map(|&i| list[i].size).sum();
    let updates = list.iter().filter(|p| p.winget.as_ref().is_some_and(|w| w.available.is_some())).count();
    let mut right = tf("{} of {} programs", &[&format::count(app.rows.len() as u64), &format::count(list.len() as u64)]);
    if size > 0 {
        right += &format!(" · {}", format::size(size));
    }
    let broken = list.iter().filter(|p| !p.problems.is_empty()).count();
    if broken > 0 {
        right += &format!(" · {}", tf("{} with problems", &[&format::count(broken as u64)]));
    }
    if app.tab == Tab::Programs {
        right += &format!(" · {}", tf("{} shortcuts", &[&format::count(f.shortcuts.len() as u64)]));
        right += &format!(" · {}", if !f.winget_done { tr("asking winget…") } else { tf("{} updates (winget)", &[&format::count(updates as u64)]) });
    }
    (left, right)
}

#[cfg(test)]
mod screenshots {
    use super::*;
    use crate::i18n::{set_lang, Lang};
    use std::path::PathBuf;

    /// Draws the window to `target/screens/*.png` for a visual check:
    /// `cargo test -p runinst --lib screenshots -- --ignored`.
    #[test]
    #[ignore]
    fn screenshots() {
        let Ok(mut g) = Gfx::new() else { return };
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&out).unwrap();
        let icon = {
            let px = crate::ui::appicon::render(32);
            let mut pm = Pixmap::new(32, 32).unwrap();
            for (d, s) in pm.pixels_mut().iter_mut().zip(px.chunks(4)) {
                *d = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
            }
            pm
        };
        let icon_png = crate::ui::appicon::render(256);
        let mut big = Pixmap::new(256, 256).unwrap();
        for (d, s) in big.pixels_mut().iter_mut().zip(icon_png.chunks(4)) {
            *d = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
        }
        big.save_png(out.join("icon.png")).unwrap();
        let mut shot = |name: &str, app: &App, dark: bool, hot: Hot, admin: bool, w: u32, h: u32| {
            g.resize(w, h);
            let full = Rect::new(0.0, 0.0, w as f32, h as f32);
            let th = Theme::new(dark, None);
            let l = layout(&mut g, app, full, 1.0, false, admin);
            let enabled = |c: crate::commands::Command| !matches!(c, crate::commands::Command::Modify) || app.chosen().len() == 1;
            draw(&mut g, app, &l, &th, 1.0, hot, &icons_of(app, &icon), &enabled);
            g.pix.save_png(out.join(format!("{name}.png"))).unwrap();
        };
        set_lang(Lang::EnUS);
        // The programs' own icons (Windows), or the app's.
        fn icons_of(app: &App, fallback: &Pixmap) -> HashMap<usize, Pixmap> {
            let Some(f) = &app.found else { return HashMap::new() };
            let mut out = HashMap::new();
            for (i, p) in f.inv.programs.iter().enumerate() {
                #[cfg(windows)]
                if let Some((w, h, px)) = p.icon.as_ref().and_then(|(file, n)| rt_sys::win::icon::extract(file, *n)).or_else(|| p.app_id.as_ref().and_then(|a| crate::appx::icon(a, 32))) {
                    let mut pm = Pixmap::new(w, h).unwrap();
                    for (d, s) in pm.pixels_mut().iter_mut().zip(px.chunks(4)) {
                        *d = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
                    }
                    out.insert(i, pm);
                    continue;
                }
                if p.name.starts_with('A') {
                    out.insert(i, fallback.clone());
                }
            }
            out
        }
        let mut app = App::new();
        app.loading = true;
        shot("loading", &app, false, Hot::None, false, 1100, 700);
        // This computer, for real (Windows), or the sample.
        #[cfg(windows)]
        let found = {
            let inv = crate::inventory::read();
            let sc = crate::shortcuts::read(&inv.me, &inv.other_users);
            let mut f = crate::app::Found::new(inv, sc);
            let scoop = crate::managers::scoop_all(&f.inv.me);
            f.scoop = (!scoop.is_empty()).then_some(scoop);
            f
        };
        #[cfg(not(windows))]
        let found = crate::app::tests::sample();
        app.set_found(found);
        shot("list-waiting-winget", &app, false, Hot::Row(2), false, 1300, 760);
        app.set_winget(crate::winget::run().map(|t| crate::winget::parse(&t)));
        let chat = app.rows.iter().position(|&i| app.program(i).is_some_and(|p| p.name == "ChatGPT"));
        app.select(chat.unwrap_or(1));
        app.keep_visible(app.sel.unwrap_or(0));
        app.search_focus = true;
        shot("list-light", &app, false, Hot::Row(2), false, 1300, 760);
        // Part of the search selected.
        app.type_text("microsoft visual");
        app.edit_search(|t| {
            t.move_caret(true, false, true, false);
            t.move_caret(false, true, false, true);
        });
        shot("search-selected", &app, false, Hot::Search, false, 1300, 500);
        app.clear_search();
        app.search_focus = false;
        shot("list-dark", &app, true, Hot::Whose(2), true, 1300, 760);
        if app.tabs().len() > 1 {
            app.set_tab(app.tabs()[1]);
            app.select(2);
            shot("tab-scoop", &app, false, Hot::Tab(0), false, 1300, 760);
            // An app whose install failed.
            if let Some(r) = app.rows.iter().position(|&i| app.program(i).is_some_and(|p| !p.problems.is_empty())) {
                app.select(r);
                app.page = 10;
                app.keep_visible(r);
                shot("scoop-problem", &app, false, Hot::None, false, 1300, 500);
            }
            app.set_tab(crate::app::Tab::Programs);
        }
        // Several selected, then uninstalling.
        app.select(1);
        app.click(3, false, true);
        shot("several", &app, false, Hot::Action(0), false, 1300, 760);
        app.toggle_tick(5);
        app.toggle_tick(7);
        shot("ticked", &app, false, Hot::Check(5), false, 1300, 760);
        shot("ticked-dark", &app, true, Hot::CheckAll, false, 1300, 760);
        app.ticked.clear();
        // The leftovers of two programs.
        {
            use crate::leftovers::{Group, Kind, Leftover};
            let l = |program: &str, group, kind, path: &str, value: Option<&str>, size, admin| Leftover {
                program: program.into(),
                group,
                kind,
                path: path.into(),
                value: value.map(Into::into),
                data: value.map(|_| "\"C:\\Program Files\\Zeta\\zeta.exe\" --tray".into()),
                size,
                admin,
                doubt: false,
            };
            let items = vec![
                l("Zeta Editor", Group::Folder, Kind::Folder, "C:\\Program Files\\Zeta Editor", None, Some(48_300_000), false),
                l("Zeta Editor", Group::Folder, Kind::Folder, "C:\\ProgramData\\Zeta", None, Some(1_200_000), true),
                l("Zeta Editor", Group::Shortcut, Kind::File, "C:\\Users\\me\\Desktop\\Zeta Editor.lnk", None, Some(1_400), false),
                l("Zeta Editor", Group::Startup, Kind::Value, "HKEY_USERS\\S-1-5-21-1\\Software\\Microsoft\\Windows\\CurrentVersion\\Run", Some("Zeta"), None, false),
                l("Zeta Editor", Group::Registry, Kind::Key, "HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Zeta", None, None, false),
                l("Zeta Editor", Group::Settings, Kind::Folder, "C:\\Users\\me\\AppData\\Roaming\\Zeta Editor", None, Some(3_500_000), false),
                l("Zeta Editor", Group::Settings, Kind::Key, "HKEY_USERS\\S-1-5-21-1\\Software\\Zeta", None, None, false),
                l("Alpha Player", Group::Elsewhere, Kind::Folder, "D:\\Media\\Alpha Player", None, Some(820_000_000), false),
                l("Zeta Updater", Group::Service, Kind::Service, "HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services\\ZetaUpd", Some("ZetaUpd"), None, false),
                l("ZetaCheck", Group::Task, Kind::Task, "\\Zeta\\ZetaCheck", Some("C:\\Windows\\System32\\Tasks\\Zeta\\ZetaCheck"), None, false),
                l("Gamma", Group::Empty, Kind::Folder, "C:\\Users\\me\\AppData\\Local\\Gamma", None, Some(0), false),
            ];
            let mut items = items;
            items[8].doubt = true;
            let mut lo = crate::app::Leftovers::new(items);
            lo.failed[4] = Some("Access is denied.".into());
            app.set_leftovers(Some(lo), true);
            app.select(3);
            shot("leftovers", &app, false, Hot::Check(0), false, 1300, 760);
            app.select(5);
            shot("leftovers-dark", &app, true, Hot::Action(0), false, 1300, 760);
            app.set_leftovers(Some(crate::app::Leftovers { searching: true, ..Default::default() }), true);
            shot("leftovers-searching", &app, false, Hot::None, false, 1000, 500);
            app.set_leftovers(None, false);
        }
        let marks = [Mark::Done, Mark::Running, Mark::Waiting, Mark::Failed];
        let items: Vec<_> = app.chosen_programs().iter().zip(marks).map(|(p, m)| (p.key.clone(), p.name.clone(), m)).collect();
        let name = items.get(1).map_or(String::new(), |i| i.1.clone());
        app.busy = Some(crate::app::Busy { items, phase: 0.3, name, index: 2, total: 3, stopping: false, modify: false });
        shot("uninstalling", &app, true, Hot::Stop, false, 1300, 760);
        shot("uninstalling-light", &app, false, Hot::None, false, 1300, 760);
        app.busy = None;
        app.set_found(crate::app::tests::sample());
        app.type_text("a");
        shot("sample-narrow", &app, false, Hot::Restart, false, 760, 560);
        set_lang(Lang::PtPT);
        app.clear_search();
        app.show_system = true;
        app.rebuild();
        shot("sample-pt", &app, false, Hot::System, false, 1200, 760);
        set_lang(Lang::EnUS);
    }
}
