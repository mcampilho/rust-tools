//! The tab of leftovers: what each is (a folder, a file, a key, a value),
//! where it was found, its size and its program; the details of the one
//! selected, with what removing it does.

use super::gfx::{Align, Color, FontId, Gfx, Rect};
use super::theme::Theme;
use super::view::{check_box, Hot, Layout, Tick};
use crate::app::{App, Col};
use crate::format;
use crate::i18n::{tf, tr};
use crate::leftovers::{Group, Kind, Leftover};
use rt_gfx::dialog::{ellipsize, ellipsize_middle};

fn px(s: f32) -> f32 {
    12.5 * s
}

/// Where it was found, as shown.
pub fn group_label(g: Group) -> String {
    tr(match g {
        Group::Folder => "Program folder",
        Group::Shortcut => "Shortcut",
        Group::Startup => "Starts with Windows",
        Group::Registry => "Registry",
        Group::Settings => "User settings",
        Group::Elsewhere => "Found by Everything",
        Group::Service => "Service",
        Group::Task => "Scheduled task",
        Group::Empty => "Empty folder",
    })
}

/// What it is: the path, or the key and the value's name; a service's name
/// and a task's.
pub fn what(l: &Leftover) -> String {
    if l.kind == Kind::Service {
        return format!("{} · {}", l.value.as_deref().unwrap_or(""), l.data.as_deref().unwrap_or(""));
    }
    if l.kind == Kind::Task {
        return format!("{} · {}", l.path, l.data.as_deref().unwrap_or(""));
    }
    match &l.value {
        Some(v) if v.is_empty() => format!("{} › {}", l.path, tr("(default)")),
        Some(v) => format!("{} › {v}", l.path),
        None => l.path.clone(),
    }
}

/// A small picture of a folder, a file or a registry entry.
fn kind_icon(g: &mut Gfx, r: Rect, kind: Kind, th: &Theme, s: f32) {
    match kind {
        Kind::Folder => {
            let c = Color::rgb(0xE8B84A);
            g.fill_rrect(Rect::new(r.x, r.y + r.h * 0.12, r.w * 0.45, r.h * 0.25), 1.5 * s, c);
            g.fill_rrect(Rect::new(r.x, r.y + r.h * 0.25, r.w, r.h * 0.65), 2.0 * s, c);
        }
        Kind::File => {
            let p = Rect::new(r.x + r.w * 0.15, r.y + r.h * 0.05, r.w * 0.7, r.h * 0.9);
            g.fill_rrect(p, 1.5 * s, th.bg);
            g.stroke_rrect(p, 1.5 * s, 1.0, th.dim);
            for i in 0..3 {
                let y = (p.y + p.h * (0.3 + i as f32 * 0.2)).round();
                g.fill_rect(Rect::new(p.x + p.w * 0.2, y, p.w * 0.6, 1.0), th.dim);
            }
        }
        Kind::Service => {
            // A gear: a ring with teeth.
            let (cx, cy, rad) = (r.x + r.w / 2.0, r.center_y(), r.w * 0.3);
            let c = Color::rgb(0x7A8A99);
            for i in 0..8 {
                let a = i as f32 / 8.0 * std::f32::consts::TAU;
                let (x, y) = (cx + (rad + 1.5 * s) * a.cos(), cy + (rad + 1.5 * s) * a.sin());
                g.fill_rrect(Rect::new(x - 1.6 * s, y - 1.6 * s, 3.2 * s, 3.2 * s), 0.8 * s, c);
            }
            g.polyline(&(0..=20).map(|i| (cx + rad * (i as f32 / 20.0 * std::f32::consts::TAU).cos(), cy + rad * (i as f32 / 20.0 * std::f32::consts::TAU).sin())).collect::<Vec<_>>(), 2.6 * s, c, false);
        }
        Kind::Task => {
            // A clock.
            let (cx, cy, rad) = (r.x + r.w / 2.0, r.center_y(), r.w * 0.42);
            let c = Color::rgb(0x3A96DD);
            g.polyline(&(0..=24).map(|i| (cx + rad * (i as f32 / 24.0 * std::f32::consts::TAU).cos(), cy + rad * (i as f32 / 24.0 * std::f32::consts::TAU).sin())).collect::<Vec<_>>(), 1.6 * s, c, false);
            g.polyline(&[(cx, cy - rad * 0.6), (cx, cy), (cx + rad * 0.5, cy)], 1.6 * s, c, false);
        }
        Kind::Key | Kind::Value => {
            // Four blocks, as regedit draws its keys.
            let c = if kind == Kind::Key { Color::rgb(0x3A96DD) } else { Color::rgb(0x8E6CC8) };
            let b = (r.w * 0.42).round();
            for (dx, dy) in [(0.0, 0.0), (r.w - b, 0.0), (0.0, r.h - b), (r.w - b, r.h - b)] {
                g.fill_rrect(Rect::new(r.x + dx, r.y + dy, b, b), 1.5 * s, c);
            }
        }
    }
}

pub fn draw_rows(g: &mut Gfx, app: &App, l: &Layout, th: &Theme, s: f32, hot: Hot) {
    let Some(lo) = &app.leftovers else { return };
    if lo.searching {
        g.text_in(&tr("Looking for what the programs left behind…"), l.list, 15.0 * s, FontId::Regular, th.dim, Align::Center);
        return;
    }
    if app.rows.is_empty() {
        let text = if lo.items.is_empty() { tr("Nothing was left behind.") } else { tr("Nothing matches.") };
        g.text_in(&text, l.list, 15.0 * s, FontId::Regular, th.dim, Align::Center);
        return;
    }
    let p = px(s);
    let pad = 8.0 * s;
    let bad = Color::rgb(if th.dark { 0xFF99A4 } else { 0xC42B1C });
    g.set_clip(Some(l.list));
    let end = (app.scroll + l.page_rows() + 1).min(app.rows.len());
    for r in app.scroll..end {
        let i = app.rows[r];
        let Some(it) = lo.items.get(i) else { continue };
        let failed = lo.failed.get(i).and_then(|f| f.as_ref());
        let y = l.list.y + (r - app.scroll) as f32 * l.row_h;
        let row = Rect::new(l.list.x, y, l.list.w, l.row_h);
        if app.is_marked(r) {
            g.fill_rect(row, th.selection);
        } else if hot == Hot::Row(r) {
            g.fill_rect(row, th.hover);
        }
        if failed.is_some() {
            g.fill_rect(row, bad.with_alpha(0x22));
            g.fill_rect(Rect::new(row.x, row.y, (3.0 * s).round(), row.h), bad);
        }
        let fg = if it.admin { th.dim } else { th.text };
        for &(col, x, w) in &l.cols {
            let c = Rect::new(x + pad, row.y, (w - 2.0 * pad).max(0.0), row.h);
            match col {
                Col::Name => {
                    let (x0, x1) = l.check_area();
                    if hot == Hot::Check(r) {
                        g.fill_rect(Rect::new(x0, row.y, x1 - x0, row.h), th.hover);
                    }
                    if app.can_tick(i) {
                        check_box(g, x0 + pad, row.center_y(), th, s, if app.is_ticked(r) { Tick::On } else { Tick::Off });
                    } else {
                        // Cannot be ticked: a faint box.
                        let b = (15.0 * s).round();
                        g.stroke_rrect(Rect::new((x0 + pad).round(), (row.center_y() - b / 2.0).round(), b, b), 3.0 * s, 1.0, th.border);
                    }
                    let ir = Rect::new(x1.round(), (row.center_y() - 8.0 * s).round(), (16.0 * s).round(), (16.0 * s).round());
                    kind_icon(g, ir, it.kind, th, s);
                    let tx = ir.right() + 8.0 * s;
                    let room = (x + w - tx - pad).max(0.0);
                    let text = ellipsize_middle(g, &what(it), room, p, FontId::Regular);
                    g.text_in(&text, Rect::new(tx, row.y, room, row.h), p, FontId::Regular, fg, Align::Left);
                }
                Col::Source => {
                    let mut t = group_label(it.group);
                    if it.admin {
                        t = format!("{t} · {}", tr("administrator"));
                    }
                    let t = ellipsize(g, &t, c.w, p, FontId::Regular);
                    g.text_in(&t, c, p, FontId::Regular, th.dim, Align::Left);
                }
                Col::Size => {
                    let t = it.size.filter(|n| *n > 0).map_or(String::new(), format::size);
                    g.text_in(&t, c, p, FontId::Regular, fg, Align::Right);
                }
                _ => {
                    let t = ellipsize(g, &it.program, c.w, p, FontId::Regular);
                    g.text_in(&t, c, p, FontId::Regular, th.dim, Align::Left);
                }
            }
        }
    }
    g.set_clip(None);
}

/// The lines of the details of a leftover: (label, value).
pub fn lines_of(l: &Leftover, failed: Option<&String>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut add = |k: &str, v: String| {
        if !v.trim().is_empty() {
            out.push((tr(k), v));
        }
    };
    add("Program", l.program.clone());
    add("Kind", group_label(l.group));
    match l.kind {
        Kind::Service => {
            add("Service", l.value.clone().unwrap_or_default());
            add("Runs", l.data.clone().unwrap_or_default());
            add("Registry key", l.path.clone());
        }
        Kind::Task => {
            add("Scheduled task", l.path.clone());
            add("Runs", l.data.clone().unwrap_or_default());
        }
        _ => {
            add(if matches!(l.kind, Kind::Key | Kind::Value) { "Registry key" } else { "Where" }, l.path.clone());
            add("Value", l.value.as_ref().map(|v| if v.is_empty() { tr("(default)") } else { v.clone() }).unwrap_or_default());
            add(if l.kind == Kind::File { "Opens" } else { "Data" }, l.data.clone().unwrap_or_default());
        }
    }
    add("Size", l.size.filter(|n| *n > 0).map_or(String::new(), format::size));
    let what_happens = match l.kind {
        Kind::Folder | Kind::File => tr("Removing it moves it to the Recycle Bin."),
        Kind::Key | Kind::Value => tr("Removing it saves it first to a .reg file (open the file to put it back), then deletes it."),
        Kind::Service => tr("Removing it saves its key to the .reg file, stops the service and deletes it."),
        Kind::Task => tr("Removing it saves a copy of the task beside the .reg file (schtasks /Create /XML puts it back), then deletes it."),
    };
    add("Removing", what_happens);
    let note = match l.group {
        _ if l.admin => tr("It needs administrator rights: use Restart as Administrator."),
        Group::Settings => tr("The program's settings and data: not ticked at first, in case you want to keep them."),
        Group::Elsewhere => tr("A folder with the program's name outside the usual places: look at it before ticking it."),
        _ if l.doubt => tr("Found from a hint only (what it ran is gone): not ticked at first; look at it before ticking it."),
        _ => String::new(),
    };
    add("Note", note);
    add("Did not work", failed.cloned().unwrap_or_default());
    out
}

pub fn draw_details(g: &mut Gfx, app: &App, area: Rect, th: &Theme, s: f32) {
    g.fill_rect(area, th.panel);
    g.fill_rect(Rect::new(area.x, area.y, 1.0, area.h), th.border);
    let Some(lo) = &app.leftovers else { return };
    let Some(i) = app.selected_index() else { return };
    let Some(l) = lo.items.get(i) else { return };
    let pad = 16.0 * s;
    let w = area.w - 2.0 * pad;
    let x = area.x + pad;
    g.set_clip(Some(area));
    let mut y = area.y + pad - app.details_scroll;
    let ir = Rect::new(x, y, (28.0 * s).round(), (28.0 * s).round());
    kind_icon(g, ir, l.kind, th, s);
    let name = l.value.clone().filter(|v| !v.is_empty()).unwrap_or_else(|| l.path.rsplit(['\\', '/']).next().unwrap_or(&l.path).to_string());
    let title = ellipsize(g, &name, w - ir.w - 10.0 * s, 15.0 * s, FontId::Semibold);
    g.text(&title, ir.right() + 10.0 * s, y + 19.0 * s, 15.0 * s, FontId::Semibold, th.text);
    y += ir.h + 16.0 * s;
    let small = 11.5 * s;
    let p12 = px(s);
    for (k, v) in lines_of(l, lo.failed.get(i).and_then(|f| f.as_ref())) {
        g.text(&k, x, y + small, small, FontId::Regular, th.dim);
        // Long texts on as many lines as they need.
        for line in wrap(g, &v, w, p12) {
            g.text(&line, x, y + small + 17.0 * s, p12, FontId::Regular, th.text);
            y += 17.0 * s;
        }
        y += 23.0 * s;
    }
    g.set_clip(None);
}

/// Words on lines no wider than `w`; a word too long is cut in the middle.
fn wrap(g: &mut Gfx, text: &str, w: f32, p: f32) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let try_ = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if g.measure(&try_, p, FontId::Regular) <= w || line.is_empty() {
            line = try_;
        } else {
            out.push(std::mem::take(&mut line));
            line = word.to_string();
        }
    }
    out.push(line);
    out.into_iter().map(|l| if g.measure(&l, p, FontId::Regular) > w { ellipsize_middle(g, &l, w, p, FontId::Regular) } else { l }).collect()
}

pub fn details_height(app: &App, s: f32) -> f32 {
    let n = app.selected_index().and_then(|i| app.leftover(i)).map_or(0, |l| lines_of(l, None).len() + 2);
    16.0 * s + 44.0 * s + n as f32 * 60.0 * s
}

/// Every leftover shown, ticked or not (to copy).
pub fn details_text(app: &App) -> String {
    let Some(lo) = &app.leftovers else { return String::new() };
    app.rows
        .iter()
        .filter_map(|&i| lo.items.get(i).map(|l| (i, l)))
        .map(|(i, l)| format!("[{}] {} · {} · {}", if lo.ticked.get(i).copied().unwrap_or(false) { "x" } else { " " }, group_label(l.group), what(l), l.program))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The status bar: the leftover pointed; how many, how many ticked, their size.
pub fn status_text(app: &App, hover: Option<usize>) -> (String, String) {
    let Some(lo) = &app.leftovers else { return (String::new(), String::new()) };
    let left = hover.and_then(|r| app.rows.get(r)).and_then(|&i| lo.items.get(i)).map_or(String::new(), what);
    let ticked = lo.chosen();
    let size: u64 = ticked.iter().filter_map(|&i| lo.items[i].size).sum();
    let mut right = tf("{} leftovers, {} ticked", &[&format::count(lo.items.len() as u64), &format::count(ticked.len() as u64)]);
    if size > 0 {
        right += &format!(" · {}", format::size(size));
    }
    (left, right)
}
