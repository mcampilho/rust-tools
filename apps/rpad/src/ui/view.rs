//! Paints the main window and registers its widgets.

use super::gfx::{Align, Color, FontId, Rect};
use super::layout;
use super::{Role, Style, Ui, UiAction, Widget};
use crate::app::commands::Command;
use crate::i18n::{tf, tr};
use crate::app::doc::Notice;
use crate::app::{App, EditorLayout, Focus, PaletteMode, RowInfo};
use crate::syntax::{Lang, Span, Tok};

/// Editor font size in pixels.
pub fn font_px(app: &App, scale: f32) -> f32 {
    (app.settings.font_size * 96.0 / 72.0 * scale * app.settings.zoom as f32 / 100.0).max(6.0)
}

/// Builds the whole frame. Returns the IME caret rectangle.
pub fn build(app: &mut App, ui: &mut Ui, mouse: (f32, f32)) -> Option<Rect> {
    let t = ui.t;
    ui.g.clear(t.window);
    let full = Rect::new(0.0, 0.0, ui.g.width(), ui.g.height());
    let mut r = full;
    if app.settings.toolbar {
        toolbar(app, ui, r.take_top(ui.px(36.0)));
    }
    tab_bar(app, ui, r.take_top(ui.px(36.0)));
    if app.doc().notice.is_some() {
        notice_bar(app, ui, r.take_top(ui.px(34.0)));
    }
    if app.find.visible {
        let h = if app.find.replace_mode { 76.0 } else { 40.0 };
        find_bar(app, ui, r.take_top(ui.px(h)));
    }
    if app.settings.status_bar {
        status_bar(app, ui, r.take_bottom(ui.px(26.0)));
    }
    if app.results.is_some() {
        let h = (r.h * 0.32).clamp(ui.px(120.0), ui.px(360.0));
        results_panel(app, ui, r.take_bottom(h));
    }
    if app.settings.outline {
        let w = (r.w * 0.22).clamp(ui.px(160.0), ui.px(300.0));
        outline_panel(app, ui, r.take_right(w));
    }
    if app.settings.markdown_preview {
        let w = (r.w * 0.45).round();
        preview_panel(app, ui, r.take_right(w));
    }
    let ime = editor(app, ui, r);
    ui.layer = 1;
    if app.palette.is_some() {
        palette(app, ui, full);
    }
    if let Some((msg, at)) = &app.toast {
        if at.elapsed().as_secs_f32() < 3.0 {
            let px = ui.fs(13.0);
            let tw = ui.g.measure(msg, px, FontId::Regular) + ui.px(28.0);
            let bottom = full.h - if app.settings.status_bar { ui.px(26.0) } else { 0.0 } - ui.px(16.0);
            let rr = Rect::new((full.w - tw - ui.px(16.0)).round(), bottom - ui.px(34.0), tw, ui.px(34.0));
            ui.popup(rr);
            ui.g.text_in(msg, rr, px, FontId::Regular, t.text, Align::Center);
            ui.label(rr, msg);
        }
    }
    ui.draw_tooltip(mouse);
    ui.layer = 0;
    ime
}

// ---------------------------------------------------------------- chrome

fn toolbar(app: &App, ui: &mut Ui, r: Rect) {
    let t = ui.t;
    ui.g.fill_rect(r, t.tab_bar);
    let mut x = r.x + ui.px(6.0);
    let y = r.y + ui.px(4.0);
    let h = r.h - ui.px(8.0);
    let items: &[(Option<Command>, char, &str)] = &[
        (Some(Command::New), '\u{E8A5}', "New"),
        (Some(Command::Open), '\u{E8E5}', "Open"),
        (Some(Command::Save), '\u{E74E}', "Save"),
        (None, ' ', ""),
        (Some(Command::Undo), '\u{E7A7}', "Undo"),
        (Some(Command::Redo), '\u{E7A6}', "Redo"),
        (None, ' ', ""),
        (Some(Command::Cut), '\u{E8C6}', "Cut"),
        (Some(Command::Copy), '\u{E8C8}', "Copy"),
        (Some(Command::Paste), '\u{E77F}', "Paste"),
        (None, ' ', ""),
        (Some(Command::Find), '\u{E721}', "Find"),
    ];
    for (cmd, icon, text) in items {
        match cmd {
            None => {
                ui.g.fill_rect(Rect::new(x + ui.px(4.0), y + ui.px(4.0), 1.0, h - ui.px(8.0)), t.border);
                x += ui.px(9.0);
            }
            Some(c) => {
                let tip = match crate::app::commands::shortcut(*c) {
                    Some(k) => format!("{} ({k})", c.plain_label()),
                    None => c.plain_label(),
                };
                ui.button(UiAction::Cmd(*c), Rect::new(x, y, ui.px(32.0), h), &tr(text), Some(*icon), &c.plain_label(), Some(&tip), Style::Flat, None, app.enabled(*c));
                x += ui.px(34.0);
            }
        }
    }
    x += ui.px(8.0);
    for (cmd, text) in [(Command::WordWrap, "Wrap"), (Command::ShowWhitespace, "¶"), (Command::Outline, "Outline"), (Command::MarkdownPreview, "Preview")] {
        let text = &tr(text);
        let w = ui.g.measure(text, ui.fs(12.5), FontId::Regular) + ui.px(18.0);
        let tip = match crate::app::commands::shortcut(cmd) {
            Some(k) => format!("{} ({k})", cmd.plain_label()),
            None => cmd.plain_label(),
        };
        ui.button(UiAction::Cmd(cmd), Rect::new(x, y, w, h), text, None, &cmd.plain_label(), Some(&tip), Style::Flat, Some(app.checked(cmd)), true);
        x += w + ui.px(2.0);
    }
    ui.g.fill_rect(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), t.border);
}

#[allow(clippy::needless_range_loop)]
fn tab_bar(app: &mut App, ui: &mut Ui, r: Rect) {
    let t = ui.t;
    ui.g.fill_rect(r, t.tab_bar);
    ui.g.fill_rect(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), t.border);
    let px = ui.fs(12.5);
    let plus_w = ui.px(34.0);
    let arrows_w = ui.px(48.0);
    let pad = ui.px(14.0);
    let close_w = ui.px(22.0);
    let widths: Vec<f32> = app
        .docs
        .iter()
        .map(|d| {
            let name = d.display_name();
            (ui.g.measure(&name, px, FontId::Regular) + pad * 2.0 + close_w + ui.px(10.0)).clamp(ui.px(90.0), ui.px(240.0))
        })
        .collect();
    let avail = r.w - plus_w - ui.px(8.0);
    let total: f32 = widths.iter().sum();
    let overflow = total > avail;
    let avail = if overflow { avail - arrows_w } else { avail };
    // Keep the active tab visible.
    if app.tab_scroll > app.active {
        app.tab_scroll = app.active;
    }
    loop {
        let w: f32 = widths[app.tab_scroll..=app.active].iter().sum();
        if w <= avail || app.tab_scroll >= app.active {
            break;
        }
        app.tab_scroll += 1;
    }
    if !overflow {
        app.tab_scroll = 0;
    }
    let mut x = r.x + ui.px(4.0);
    let y = r.y + ui.px(4.0);
    let h = r.h - ui.px(4.0);
    for i in app.tab_scroll..app.docs.len() {
        let w = widths[i];
        if x + w > r.x + ui.px(4.0) + avail + 1.0 {
            break;
        }
        let d = &app.docs[i];
        let active = i == app.active;
        let tab_r = Rect::new(x, y, w - ui.px(2.0), h);
        let id = super::wid(&UiAction::Tab(i));
        let hover = ui.inp.hot == Some(id);
        let bg = if active {
            t.tab_active
        } else if hover {
            t.tab.mix(t.tab_active, 0.5)
        } else {
            t.tab
        };
        ui.g.fill_rrect(Rect::new(tab_r.x, tab_r.y, tab_r.w, tab_r.h + ui.px(6.0)), ui.px(6.0), bg);
        if active {
            ui.g.fill_rect(Rect::new(tab_r.x + ui.px(8.0), tab_r.y, tab_r.w - ui.px(16.0), ui.px(2.0)), t.accent);
        }
        let name = d.display_name();
        let modified = d.is_modified();
        let tip = match &d.path {
            Some(p) => p.display().to_string(),
            None => tf("{} (not saved yet)", &[&name]),
        };
        let mut status = String::new();
        if modified {
            status.push_str(&format!(", {}", tr("modified")));
        }
        if d.view_only || d.read_only {
            status.push_str(&format!(", {}", tr("read-only")));
        }
        ui.register(Widget {
            id,
            rect: tab_r,
            action: UiAction::Tab(i),
            role: Role::Tab,
            label: format!("{name}{status}"),
            tooltip: Some(tip),
            value: None,
            enabled: true,
            checked: Some(active),
            layer: 0,
        });
        let text_r = Rect::new(tab_r.x + pad, tab_r.y, tab_r.w - pad - close_w - ui.px(6.0), tab_r.h);
        let color = if active { t.text } else { t.text_dim };
        let shown = if d.view_only || d.read_only { format!("🔒 {name}") } else { name };
        ui.text_clip(&shown, text_r, 12.5, if active { FontId::Semibold } else { FontId::Regular }, color, Align::Left);
        let cr = Rect::new(tab_r.right() - close_w - ui.px(6.0), tab_r.center_y() - close_w / 2.0, close_w, close_w);
        let close_id = super::wid(&UiAction::CloseTab(i));
        let close_hover = ui.inp.hot == Some(close_id);
        if modified && !close_hover {
            ui.g.fill_circle(cr.x + cr.w / 2.0, cr.center_y(), ui.fs(4.0), if active { t.accent } else { t.text_dim });
            ui.register(Widget {
                id: close_id,
                rect: cr,
                action: UiAction::CloseTab(i),
                role: Role::Button,
                label: tf("Close {}", &[&d.display_name()]),
                tooltip: Some(tr("Close (Ctrl+W)")),
                value: None,
                enabled: true,
                checked: None,
                layer: 0,
            });
        } else if active || hover || close_hover {
            ui.button(UiAction::CloseTab(i), cr, "×", Some('\u{E711}'), &tf("Close {}", &[&app.docs[i].display_name()]), Some(&tr("Close (Ctrl+W)")), Style::Flat, None, true);
        } else {
            ui.register(Widget {
                id: close_id,
                rect: cr,
                action: UiAction::CloseTab(i),
                role: Role::Button,
                label: tf("Close {}", &[&d.display_name()]),
                tooltip: None,
                value: None,
                enabled: true,
                checked: None,
                layer: 0,
            });
        }
        x += w;
    }
    if overflow {
        let ar = Rect::new(r.right() - plus_w - arrows_w - ui.px(4.0), y, arrows_w / 2.0, h - ui.px(4.0));
        ui.button(UiAction::TabScroll(-1), ar, "‹", Some('\u{E76B}'), &tr("Scroll tabs left"), None, Style::Flat, None, app.tab_scroll > 0);
        let ar2 = Rect::new(ar.right(), y, arrows_w / 2.0, h - ui.px(4.0));
        ui.button(UiAction::TabScroll(1), ar2, "›", Some('\u{E76C}'), &tr("Scroll tabs right"), None, Style::Flat, None, app.tab_scroll + 1 < app.docs.len());
    }
    let pr = Rect::new(if overflow { r.right() - plus_w - ui.px(4.0) } else { x + ui.px(2.0) }, y + ui.px(1.0), plus_w - ui.px(4.0), h - ui.px(6.0));
    ui.button(UiAction::NewTab, pr, "+", Some('\u{E710}'), &tr("New tab"), Some(&tr("New document (Ctrl+N)")), Style::Flat, None, true);
}

fn notice_bar(app: &App, ui: &mut Ui, r: Rect) {
    let t = ui.t;
    let Some(n) = app.doc().notice.clone() else { return };
    let (bg, msg) = match &n {
        Notice::ChangedOnDisk => (t.notice_warn, tr("This file was changed by another program.")),
        Notice::Deleted => (t.notice_warn, tr("This file no longer exists on disk. Save it to keep your text.")),
        Notice::Info(m) | Notice::MixedEol(m) => (t.notice_info, m.text()),
        Notice::Warning(m) => (t.notice_warn, m.text()),
    };
    ui.g.fill_rect(r, bg);
    let mut right = r.right() - ui.px(8.0);
    let bh = r.h - ui.px(8.0);
    let button = |ui: &mut Ui, right: &mut f32, a: UiAction, text: &str| {
        let text = &tr(text);
        let w = ui.g.measure(text, ui.fs(12.5), FontId::Regular) + ui.px(20.0);
        *right -= w;
        ui.button(a, Rect::new(*right, r.y + ui.px(4.0), w, bh), text, None, text, None, Style::Push, None, true);
        *right -= ui.px(6.0);
    };
    match n {
        Notice::ChangedOnDisk => {
            button(ui, &mut right, UiAction::NoticeKeep, "Keep my version");
            button(ui, &mut right, UiAction::NoticeReload, "Reload");
        }
        Notice::Deleted => {
            button(ui, &mut right, UiAction::NoticeDismiss, "Dismiss");
            button(ui, &mut right, UiAction::NoticeSaveAs, "Save As…");
        }
        _ => {
            let cr = Rect::new(right - ui.px(28.0), r.y + ui.px(4.0), ui.px(28.0), bh);
            ui.button(UiAction::NoticeDismiss, cr, "×", Some('\u{E711}'), &tr("Dismiss message"), None, Style::Flat, None, true);
            right = cr.x - ui.px(6.0);
        }
    }
    let tr = Rect::new(r.x + ui.px(12.0), r.y, right - r.x - ui.px(16.0), r.h);
    ui.text_clip(&msg, tr, 12.5, FontId::Regular, t.text, Align::Left);
    ui.label(tr, &msg);
}

fn find_bar(app: &mut App, ui: &mut Ui, r: Rect) {
    let t = ui.t;
    ui.g.fill_rect(r, t.window);
    ui.g.fill_rect(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), t.border);
    app.update_matches();
    let row_h = ui.px(30.0);
    let y0 = r.y + ui.px(5.0);
    let mut x = r.x + ui.px(6.0);
    let rm = app.find.replace_mode;
    ui.button(UiAction::ToggleReplace, Rect::new(x, y0, ui.px(24.0), row_h), if rm { "▾" } else { "▸" }, Some(if rm { '\u{E70D}' } else { '\u{E76C}' }), &tr("Toggle replace"), Some(&tr("Toggle replace (Ctrl+H)")), Style::Flat, Some(rm), true);
    x += ui.px(28.0);
    let right_w = ui.px(4.0 * 30.0 + 120.0 + 3.0 * 30.0 + 12.0);
    let fw = (r.right() - x - right_w).clamp(ui.px(120.0), ui.px(520.0));
    let err = app.find.error.is_some();
    let focus = app.focus;
    let fr = Rect::new(x, y0, fw, row_h);
    ui.field(UiAction::Field(Focus::Find), fr, &mut app.find.find, focus == Focus::Find, &tr("Find"), &tr("Find"), err);
    x += fw + ui.px(6.0);
    let opts = [(0u8, "Aa", "Match case (Alt+C)", app.find.case), (1, "W", "Whole word (Alt+W)", app.find.word), (2, ".*", "Regular expression (Alt+R)", app.find.regex), (3, "⊂", "Find in selection (Alt+L)", app.find.in_selection)];
    for (i, txt, tip, on) in opts {
        let tip = &tr(tip);
        ui.button(UiAction::FindOpt(i), Rect::new(x, y0, ui.px(28.0), row_h), txt, None, tip, Some(tip), Style::Flat, Some(on), true);
        x += ui.px(30.0);
    }
    let sel = app.doc().buf.primary_sel();
    let count = if let Some(e) = &app.find.error {
        e.clone()
    } else if app.find.find.is_empty() {
        String::new()
    } else if app.find.counting {
        tr("Counting…")
    } else if app.find.total == 0 {
        tr("No results")
    } else {
        match app.find.current_index((sel.start(), sel.end())) {
            Some(i) => tf("{} of {}", &[&(i + 1).to_string(), &app.find.total.to_string()]),
            None => tf("{} matches", &[&app.find.total.to_string()]),
        }
    };
    let cr = Rect::new(x + ui.px(4.0), y0, ui.px(112.0), row_h);
    ui.text_clip(&count, cr, 12.0, FontId::Regular, if err { t.error } else { t.text_dim }, Align::Left);
    ui.label(cr, &count);
    x += ui.px(120.0);
    ui.button(UiAction::FindPrev, Rect::new(x, y0, ui.px(28.0), row_h), "↑", Some('\u{E74A}'), &tr("Previous match"), Some(&tr("Previous match (Shift+Enter / Shift+F3)")), Style::Flat, None, true);
    x += ui.px(30.0);
    ui.button(UiAction::FindNext, Rect::new(x, y0, ui.px(28.0), row_h), "↓", Some('\u{E74B}'), &tr("Next match"), Some(&tr("Next match (Enter / F3)")), Style::Flat, None, true);
    x += ui.px(30.0);
    ui.button(UiAction::CloseFind, Rect::new(x, y0, ui.px(28.0), row_h), "×", Some('\u{E711}'), &tr("Close find"), Some(&tr("Close (Esc)")), Style::Flat, None, true);
    if rm {
        let y1 = y0 + row_h + ui.px(6.0);
        let rx = r.x + ui.px(34.0);
        let rr = Rect::new(rx, y1, fw, row_h);
        let focus = app.focus;
        ui.field(UiAction::Field(Focus::Replace), rr, &mut app.find.replace, focus == Focus::Replace, &tr("Replace with"), &tr("Replace with"), false);
        let mut bx = rr.right() + ui.px(6.0);
        let (l1, l2) = (tr("Replace"), tr("Replace all"));
        let w1 = (ui.g.measure(&l1, ui.fs(12.5), FontId::Regular) + ui.px(24.0)).max(ui.px(76.0));
        ui.button(UiAction::ReplaceOne, Rect::new(bx, y1, w1, row_h), &l1, None, &l1, Some(&tr("Replace this match and find the next (Enter)")), Style::Push, None, app.doc().editable());
        bx += w1 + ui.px(6.0);
        let w2 = (ui.g.measure(&l2, ui.fs(12.5), FontId::Regular) + ui.px(24.0)).max(ui.px(92.0));
        ui.button(UiAction::ReplaceAll, Rect::new(bx, y1, w2, row_h), &l2, None, &l2, Some(&tr("Replace all matches, with preview (Alt+A / Ctrl+Alt+Enter)")), Style::Push, None, app.doc().editable());
        if app.find.regex {
            let hint = &tr("Use $1, ${name} for groups and \\n, \\t for line breaks and tabs");
            let hr = Rect::new(bx + w2 + ui.px(10.0), y1, r.right() - bx - w2 - ui.px(16.0), row_h);
            ui.text_clip(hint, hr, 11.5, FontId::Regular, t.text_dim, Align::Left);
        }
    }
}

fn status_bar(app: &App, ui: &mut Ui, r: Rect) {
    let t = ui.t;
    ui.g.fill_rect(r, t.status);
    ui.g.fill_rect(Rect::new(r.x, r.y, r.w, 1.0), t.border);
    let d = app.doc();
    let b = &d.buf;
    let sel = b.primary_sel();
    let line = b.line_of(sel.head);
    let col_chars = sel.head - b.line_start(line);
    let vcol = layout::col_of(&b.line_text(line), col_chars, app.settings.tab_width);
    let mut left = vec![tf("Ln {}, Col {}", &[&(line + 1).to_string(), &(vcol + 1).to_string()])];
    let sel_chars: usize = b.sels.iter().map(|s| s.end() - s.start()).sum();
    if sel_chars > 0 {
        let lines: usize = b.sel_line_ranges().iter().map(|(a, z)| z - a + 1).sum();
        left.push(if lines == 1 { tf("{} selected (1 line)", &[&sel_chars.to_string()]) } else { tf("{} selected ({} lines)", &[&sel_chars.to_string(), &lines.to_string()]) });
    }
    if b.sels.len() > 1 {
        left.push(tf("{} carets", &[&b.sels.len().to_string()]));
    }
    left.push(tf("{} lines", &[&b.line_count().to_string()]));
    let mut x = r.x + ui.px(10.0);
    for (k, s) in left.iter().enumerate() {
        let w = ui.g.measure(s, ui.fs(12.0), FontId::Regular) + ui.px(18.0);
        let sr = Rect::new(x, r.y, w, r.h);
        if k == 0 {
            ui.button(UiAction::Status(4), sr, s, None, &tf("{}. Go to line", &[s]), Some(&tr("Go to line (Ctrl+G)")), Style::Flat, None, true);
        } else {
            ui.text(s, sr, 12.0, FontId::Regular, t.text, Align::Center);
            ui.label(sr, s);
        }
        x += w;
    }
    let indent = tf(if app.settings.use_tabs { "Tabs: {}" } else { "Spaces: {}" }, &[&app.settings.tab_width.to_string()]);
    let mut items: Vec<(Option<u8>, String, &str)> = vec![
        (Some(3), indent, "Indentation"),
        (Some(1), d.enc.name(), "Encoding"),
        (Some(2), d.eol.name().to_string(), "Line endings"),
        (Some(0), tr(d.lang.name()), "Language"),
    ];
    if app.settings.zoom != 100 {
        items.insert(0, (None, format!("{}%", app.settings.zoom), "Zoom"));
    }
    items.insert(0, (None, if app.overwrite { "OVR".into() } else { "INS".into() }, "Insert mode"));
    let mut rx = r.right() - ui.px(8.0);
    for (kind, text, name) in items.into_iter().rev() {
        let name = &tr(name);
        let w = ui.g.measure(&text, ui.fs(12.0), FontId::Regular) + ui.px(20.0);
        rx -= w;
        let sr = Rect::new(rx, r.y, w, r.h);
        match kind {
            Some(k) => ui.button(UiAction::Status(k), sr, &text, None, &format!("{name}: {text}"), Some(&tf("{} — click to change", &[name])), Style::Flat, None, true),
            None => {
                ui.text(&text, sr, 12.0, FontId::Regular, t.text_dim, Align::Center);
                ui.label(sr, &format!("{name}: {text}"));
            }
        }
    }
}

// ---------------------------------------------------------------- editor

#[allow(clippy::needless_range_loop)]
fn editor(app: &mut App, ui: &mut Ui, r: Rect) -> Option<Rect> {
    let t = ui.t;
    let s = ui.s;
    let tab = app.settings.tab_width;
    let px = font_px(app, s);
    let char_w = ui.g.advance('M', px, FontId::Mono).max(1.0);
    let line_h = (px * 1.42).round().max(4.0);
    let (asc, desc) = ui.g.metrics(px, FontId::Mono);
    if app.settings.fold_margin {
        app.ensure_folds(false);
    }
    let (line_count, large, lang) = {
        let d = app.doc();
        (d.buf.line_count(), d.large, d.lang)
    };
    let digits = line_count.to_string().len().max(3);
    let num_w = if app.settings.line_numbers { digits as f32 * char_w + ui.px(16.0) } else { ui.px(6.0) };
    let fold_w = if app.settings.fold_margin { (px * 1.1).round() } else { 0.0 };
    let gutter = Rect::new(r.x, r.y, num_w + fold_w, r.h);
    let sb = ui.px(12.0);
    let text_x = gutter.right() + ui.px(6.0);
    let text_right = r.right() - sb;
    let view_cells = ((text_right - text_x) / char_w).floor().max(1.0) as usize;
    let wrap_cells = (app.settings.word_wrap && !large).then_some(view_cells.max(10));
    let show_h = wrap_cells.is_none() && app.layout.max_line_cells > view_cells;
    let text_bottom = if show_h { r.bottom() - sb } else { r.bottom() };
    let rows_visible = ((text_bottom - r.y) / line_h).floor().max(1.0) as usize;
    app.layout.wrap_cells = wrap_cells;
    app.layout.char_w = char_w;
    app.layout.line_h = line_h;
    app.layout.rows_visible = rows_visible;
    if app.reveal_caret {
        app.ensure_caret_visible(rows_visible, view_cells);
        app.reveal_caret = false;
    }
    app.bracket_pair = if lang != Lang::Plain || true { app.matching_bracket_within(200_000) } else { None };
    let bracket = app.bracket_pair;
    let fold_col_x = gutter.x + num_w;
    let matches = app.find.matches.clone();
    let caret_on = ui.inp.caret_on && app.focus == Focus::Editor;
    let show_ws = app.settings.show_whitespace;
    let show_eol = app.settings.show_eol;
    let overwrite = app.overwrite;
    let highlight_line = app.settings.highlight_line;
    let line_numbers = app.settings.line_numbers;
    let fold_margin = app.settings.fold_margin;
    let preedit = app.input.preedit.clone();
    let editor_focus = app.focus == Focus::Editor;

    ui.g.fill_rect(r, t.editor);
    ui.g.fill_rect(gutter, t.gutter);
    let d = &mut app.docs[app.active];
    let eol_name = d.eol.name();
    let sels = d.buf.sels.clone();
    let primary = d.buf.primary_sel();
    let caret_line = d.buf.line_of(primary.head);
    let scroll_x = if wrap_cells.is_some() { 0.0 } else { d.scroll_x };
    let text_clip = Rect::new(text_x - ui.px(2.0), r.y, text_right - text_x + ui.px(2.0), text_bottom - r.y);
    let mut rows: Vec<RowInfo> = Vec::with_capacity(rows_visible + 2);
    let mut spans: Vec<Span> = Vec::new();
    let mut ime_rect = None;
    let mut max_cells = 0usize;
    let mut line = d.top.min(line_count.saturating_sub(1));
    let mut row = d.top_row;
    let mut y = r.y;
    let baseline_off = ((line_h + asc + desc) / 2.0).round();
    while y < text_bottom && line < line_count {
        let text = d.buf.line_text(line);
        let ls = d.buf.line_start(line);
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len();
        let starts = match wrap_cells {
            Some(w) => layout::wrap(&text, w, tab),
            None => vec![0],
        };
        // Cell column of each char (computed up to what is visible).
        let limit_cells = if wrap_cells.is_some() { usize::MAX } else { ((scroll_x + (text_right - text_x)) / char_w) as usize + 2 };
        let mut cols: Vec<usize> = Vec::with_capacity(n.min(4096) + 1);
        let mut c = 0;
        cols.push(0);
        for &ch in &chars {
            c = layout::advance(c, ch, tab);
            cols.push(c);
            if c > limit_cells && wrap_cells.is_none() {
                break;
            }
        }
        let full_cols = cols.len() == n + 1;
        max_cells = max_cells.max(if full_cols { c } else { c + 1 });
        let col_at = |i: usize| -> usize { cols.get(i).copied().unwrap_or(usize::MAX / 2) };
        // Token colors per char.
        d.hl.line_spans(&d.buf.text, line, &mut spans);
        let mut toks: Vec<Tok> = Vec::new();
        if !spans.is_empty() {
            toks = vec![Tok::Text; n];
            let mut ci = 0;
            let mut bi = 0;
            let byte_to_char: Vec<usize> = {
                let mut v = Vec::with_capacity(text.len() + 1);
                for (i, ch) in chars.iter().enumerate() {
                    for _ in 0..ch.len_utf8() {
                        v.push(i);
                    }
                }
                v.push(n);
                v
            };
            let _ = (&mut ci, &mut bi);
            for sp in &spans {
                let a = byte_to_char[sp.start.min(text.len())];
                let b = byte_to_char[sp.end.min(text.len())];
                for k in toks.iter_mut().take(b).skip(a) {
                    *k = sp.tok;
                }
            }
        }
        let folded = d.folded.contains(&line);
        let fold_start = fold_margin && d.folds.iter().any(|f| f.0 == line);
        let line_end_pos = ls + n;
        for ri in row..starts.len() {
            if y >= text_bottom {
                break;
            }
            let rs = starts[ri];
            let re = starts.get(ri + 1).copied().unwrap_or(n);
            let base_col = col_at(rs);
            let x_of = |col: usize| -> f32 {
                if wrap_cells.is_some() {
                    text_x + (col.saturating_sub(base_col)) as f32 * char_w
                } else {
                    text_x - scroll_x + col as f32 * char_w
                }
            };
            let last_row = ri + 1 == starts.len();
            // Current line highlight.
            if highlight_line && line == caret_line && editor_focus {
                ui.g.fill_rect(Rect::new(r.x, y, r.w - sb, line_h), t.current_line);
            }
            ui.g.set_clip(Some(text_clip));
            // Selections.
            for sel in &sels {
                if sel.is_empty() {
                    continue;
                }
                let (a, b) = (sel.start(), sel.end());
                if b < ls + rs || a > ls + re {
                    continue;
                }
                let ca = a.saturating_sub(ls).clamp(rs, re);
                let cb = b.saturating_sub(ls).clamp(rs, re);
                let mut x1 = x_of(col_at(ca));
                let x0 = x_of(col_at(cb.min(n)));
                if b > line_end_pos && last_row {
                    x1 = x1.min(x0);
                    let ext = x0 + char_w * 0.6;
                    ui.g.fill_rect(Rect::new(x1.min(x0), y, ext - x1.min(x0), line_h), t.selection);
                } else if cb > ca {
                    ui.g.fill_rect(Rect::new(x1, y, x0 - x1, line_h), t.selection);
                }
            }
            // Search matches.
            if !matches.is_empty() {
                let lo = ls + rs;
                let hi = ls + re;
                let start_idx = matches.partition_point(|m| m.1 < lo);
                for m in matches[start_idx..].iter().take_while(|m| m.0 <= hi) {
                    if m.1 <= lo && m.0 != m.1 {
                        continue;
                    }
                    let a = m.0.saturating_sub(ls).clamp(rs, re);
                    let b = m.1.saturating_sub(ls).clamp(rs, re);
                    let (xa, xb) = (x_of(col_at(a)), x_of(col_at(b)));
                    let mr = Rect::new(xa, y + 1.0, (xb - xa).max(2.0), line_h - 2.0);
                    ui.g.fill_rect(mr, t.match_bg);
                    ui.g.stroke_rrect(mr, 2.0, 1.0, t.match_border);
                }
            }
            // Bracket pair boxes.
            if let Some((p1, p2)) = bracket {
                for p in [p1, p2] {
                    if p >= ls + rs && p < ls + re.max(rs + 1).min(n.max(1)) && p < line_end_pos {
                        let i = p - ls;
                        let br = Rect::new(x_of(col_at(i)), y + 1.0, (col_at(i + 1) - col_at(i)) as f32 * char_w, line_h - 2.0);
                        ui.g.stroke_rrect(br, 2.0, 1.0, t.bracket);
                    }
                }
            }
            // Text.
            let base = y + baseline_off;
            let mut run = String::new();
            let mut run_x = 0.0;
            let mut run_color = t.tok(Tok::Text);
            let flush = |ui: &mut Ui, run: &mut String, x: f32, color: Color| {
                if !run.is_empty() {
                    ui.g.text(run, x.round(), base, px, FontId::Mono, color);
                    run.clear();
                }
            };
            for i in rs..re {
                let col = col_at(i);
                if col >= usize::MAX / 4 {
                    break;
                }
                let x = x_of(col);
                if x > text_right + char_w {
                    break;
                }
                let ch = chars[i];
                let next_x = x_of(col_at(i + 1));
                if next_x < text_x - char_w && wrap_cells.is_none() {
                    continue;
                }
                let color = toks.get(i).map_or(t.tok(Tok::Text), |k| t.tok(*k));
                let special = ch == '\t' || ch == ' ' || (ch as u32) < 0x20 || ch as u32 == 0x7F || !ch.is_ascii();
                if special || color != run_color {
                    flush(ui, &mut run, run_x, run_color);
                }
                if ch == ' ' {
                    if show_ws {
                        ui.g.text_in("·", Rect::new(x, y, char_w, line_h), px, FontId::Mono, t.whitespace, Align::Center);
                    }
                    continue;
                }
                if ch == '\t' {
                    if show_ws {
                        ui.g.text_in("→", Rect::new(x, y, (next_x - x).min(char_w * 2.0), line_h), px * 0.9, FontId::Mono, t.whitespace, Align::Center);
                    }
                    continue;
                }
                if (ch as u32) < 0x20 || ch as u32 == 0x7F {
                    let pic = char::from_u32(if ch as u32 == 0x7F { 0x2421 } else { 0x2400 + ch as u32 }).unwrap_or('?');
                    let cr = Rect::new(x, y + 2.0, next_x - x, line_h - 4.0);
                    ui.g.fill_rrect(cr, 2.0, t.error.with_alpha(0x30));
                    ui.g.text_in(&pic.to_string(), cr, px, FontId::Mono, t.error, Align::Center);
                    continue;
                }
                if !ch.is_ascii() {
                    // Draw wide/combining chars at their own cell position.
                    if layout::cells(ch) == 0 {
                        ui.g.text(&ch.to_string(), x - char_w, base, px, FontId::Mono, color);
                    } else {
                        let w = ui.g.advance(ch, px, FontId::Mono);
                        let cell_w = next_x - x;
                        ui.g.text(&ch.to_string(), (x + (cell_w - w).max(0.0) / 2.0).round(), base, px, FontId::Mono, color);
                    }
                    continue;
                }
                if run.is_empty() {
                    run_x = x;
                    run_color = color;
                }
                run.push(ch);
            }
            flush(ui, &mut run, run_x, run_color);
            if last_row {
                let end_x = x_of(col_at(n));
                if folded {
                    let fr = Rect::new(end_x + char_w * 0.5, y + 2.0, char_w * 3.0, line_h - 4.0);
                    ui.g.fill_rrect(fr, 3.0, t.fold.with_alpha(0x40));
                    ui.g.text_in("…", fr, px, FontId::Mono, t.text, Align::Center);
                } else if show_eol && line + 1 < line_count {
                    let ew = ui.g.measure(eol_name, px * 0.7, FontId::Regular) + 6.0;
                    let er = Rect::new(end_x + char_w * 0.4, y + line_h * 0.2, ew, line_h * 0.6);
                    ui.g.fill_rrect(er, 2.0, t.whitespace.with_alpha(0x50));
                    ui.g.text_in(eol_name, er, px * 0.7, FontId::Regular, t.text_dim, Align::Center);
                }
            }
            // Carets.
            for sel in &sels {
                let h = sel.head;
                let in_row = h >= ls + rs && (h < ls + re || (last_row && h <= line_end_pos) || (h == ls + re && re == n));
                if !in_row || (h == ls + re && !last_row) {
                    continue;
                }
                let i = h - ls;
                let x = x_of(col_at(i));
                let is_primary = *sel == primary;
                if is_primary {
                    ime_rect = Some(Rect::new(x, y, char_w, line_h));
                    if !preedit.is_empty() {
                        let pw = ui.g.measure(&preedit, px, FontId::Mono);
                        ui.g.fill_rect(Rect::new(x, y, pw, line_h), t.editor);
                        ui.g.text(&preedit, x, base, px, FontId::Mono, t.text);
                        ui.g.fill_rect(Rect::new(x, y + line_h - 2.0, pw, 1.0), t.text);
                    }
                }
                if caret_on {
                    if overwrite {
                        let w = (col_at(i + 1).saturating_sub(col_at(i))).max(1) as f32 * char_w;
                        ui.g.fill_rect(Rect::new(x, y + line_h - ui.px(3.0), w, ui.px(2.0)), t.caret);
                    } else {
                        ui.g.fill_rect(Rect::new(x.round(), y + 1.0, ui.px(2.0).max(1.0), line_h - 2.0), t.caret);
                    }
                }
            }
            ui.g.set_clip(None);
            // Gutter.
            if ri == 0 {
                if line_numbers {
                    let nr = Rect::new(gutter.x, y, num_w - ui.px(8.0), line_h);
                    let color = if line == caret_line { t.line_no_current } else { t.line_no };
                    let numtext = (line + 1).to_string();
                    let w = ui.g.measure(&numtext, px * 0.92, FontId::Mono);
                    ui.g.text(&numtext, nr.right() - w, base, px * 0.92, FontId::Mono, color);
                }
                if fold_start {
                    let fx = fold_col_x + fold_w / 2.0;
                    let fy = y + line_h / 2.0;
                    let k = fold_w * 0.22;
                    let pts = if folded {
                        vec![(fx - k * 0.6, fy - k), (fx + k * 0.8, fy), (fx - k * 0.6, fy + k)]
                    } else {
                        vec![(fx - k, fy - k * 0.6), (fx + k, fy - k * 0.6), (fx, fy + k * 0.8)]
                    };
                    ui.g.polyline(&pts, 1.4, t.fold, true);
                }
            }
            rows.push(RowInfo { line, start: rs, end: re, y });
            y += line_h;
        }
        row = 0;
        line = d.next_visible(line);
    }
    // Ruler.
    if app.settings.show_ruler {
        let rx = text_x - scroll_x + app.settings.ruler_col as f32 * char_w;
        if rx > text_x && rx < text_right {
            ui.g.fill_rect(Rect::new(rx.round(), r.y, 1.0, text_bottom - r.y), t.ruler);
        }
    }
    ui.g.fill_rect(Rect::new(gutter.right(), r.y, 1.0, r.h), t.border.with_alpha(0x60));
    // Vertical scrollbar with match markers.
    let d = &app.docs[app.active];
    let vtrack = Rect::new(r.right() - sb, r.y, sb, text_bottom - r.y);
    let total = line_count.max(1) as f32;
    let thumb_h = (rows_visible as f32 / (total + rows_visible as f32 - 1.0) * vtrack.h).clamp(ui.px(24.0), vtrack.h);
    let frac = d.top as f32 / (total - 1.0).max(1.0);
    let thumb = Rect::new(vtrack.x + ui.px(3.0), vtrack.y + (vtrack.h - thumb_h) * frac, sb - ui.px(6.0), thumb_h);
    if line_count > rows_visible {
        ui.g.fill_rrect(thumb, ui.px(3.0), t.scrollbar);
    }
    if !matches.is_empty() {
        let mut last_y = -10.0;
        for m in matches.iter().take(5000) {
            let l = d.buf.line_of(m.0);
            let my = (vtrack.y + l as f32 / total * vtrack.h).round();
            if my - last_y >= 2.0 {
                ui.g.fill_rect(Rect::new(vtrack.x + ui.px(2.0), my, sb - ui.px(4.0), 2.0), t.match_border);
                last_y = my;
            }
        }
    }
    let mut htrack = Rect::default();
    if show_h {
        htrack = Rect::new(text_x, text_bottom, text_right - text_x, sb);
        let content = app.layout.max_line_cells.max(1) as f32 * char_w;
        let view = text_right - text_x;
        let tw = (view / content * htrack.w).clamp(ui.px(24.0), htrack.w);
        let fx = (scroll_x / (content - view).max(1.0)).clamp(0.0, 1.0);
        ui.g.fill_rrect(Rect::new(htrack.x + (htrack.w - tw) * fx, htrack.y + ui.px(3.0), tw, sb - ui.px(6.0)), ui.px(3.0), t.scrollbar);
    }
    let caret_text = d.buf.line_text(caret_line);
    let name = d.display_name();
    app.layout = EditorLayout {
        rect: Rect::new(r.x, r.y, r.w, text_bottom - r.y),
        text_x,
        char_w,
        line_h,
        rows,
        rows_visible,
        wrap_cells,
        gutter,
        fold_x: fold_col_x,
        vscroll: vtrack,
        hscroll: htrack,
        max_line_cells: if wrap_cells.is_some() { 0 } else { max_cells.max(app.layout.max_line_cells.min(max_cells.max(1) * 4)) },
    };
    ui.register(Widget {
        id: super::wid(&UiAction::Editor),
        rect: Rect::new(text_x, r.y, text_right - text_x, text_bottom - r.y),
        action: UiAction::Editor,
        role: Role::Editor,
        label: tf("Editor, {}, line {} column {}", &[&name, &(caret_line + 1).to_string(), &(primary.head - app.doc().buf.line_start(caret_line) + 1).to_string()]),
        tooltip: None,
        value: Some(caret_text),
        enabled: true,
        checked: None,
        layer: 0,
    });
    ime_rect
}

// ---------------------------------------------------------------- side panels

fn panel_header(ui: &mut Ui, r: &mut Rect, title: &str, close: Command) {
    let title = &tr(title);
    let t = ui.t;
    let h = r.take_top(ui.px(30.0));
    ui.g.fill_rect(h, t.window);
    ui.text_clip(title, Rect::new(h.x + ui.px(10.0), h.y, h.w - ui.px(44.0), h.h), 12.5, FontId::Semibold, t.text, Align::Left);
    ui.label(h, title);
    ui.button(UiAction::Cmd(close), Rect::new(h.right() - ui.px(30.0), h.y + ui.px(3.0), ui.px(26.0), h.h - ui.px(6.0)), "×", Some('\u{E711}'), &tf("Close {}", &[title]), None, Style::Flat, None, true);
}

fn outline_panel(app: &mut App, ui: &mut Ui, r: Rect) {
    let t = ui.t;
    ui.g.fill_rect(r, t.window);
    ui.g.fill_rect(Rect::new(r.x, r.y, 1.0, r.h), t.border);
    let mut r = Rect::new(r.x + 1.0, r.y, r.w - 1.0, r.h);
    panel_header(ui, &mut r, "Outline", Command::Outline);
    if app.last_edit.elapsed().as_millis() > 250 || app.doc().outline_version == u64::MAX {
        app.ensure_outline();
    }
    let d = app.doc();
    if d.outline.is_empty() {
        let msg = &tr(if d.large { "Outline is off for large files." } else if d.lang == Lang::Plain { "Choose a language (Format › Language) to see an outline." } else { "No symbols found." });
        let mr = Rect::new(r.x + ui.px(10.0), r.y + ui.px(6.0), r.w - ui.px(20.0), ui.px(40.0));
        ui.text_clip(msg, mr, 12.0, FontId::Regular, t.text_dim, Align::Left);
        ui.label(mr, msg);
        return;
    }
    let caret_line = d.buf.line_of(d.buf.primary_sel().head);
    let current = d.outline.iter().rposition(|s| s.line <= caret_line);
    let item_h = ui.px(24.0);
    let visible = (r.h / item_h).floor() as usize;
    if let Some(c) = current {
        if c < app.outline_scroll {
            app.outline_scroll = c;
        } else if c >= app.outline_scroll + visible {
            app.outline_scroll = c + 1 - visible;
        }
    }
    let symbols = app.doc().outline.clone();
    app.outline_scroll = app.outline_scroll.min(symbols.len().saturating_sub(visible.max(1)));
    for (k, sym) in symbols.iter().enumerate().skip(app.outline_scroll).take(visible) {
        let y = r.y + (k - app.outline_scroll) as f32 * item_h;
        let ir = Rect::new(r.x + ui.px(4.0), y, r.w - ui.px(8.0), item_h);
        let id = super::wid(&UiAction::Outline(sym.line));
        if Some(k) == current {
            ui.g.fill_rrect(ir, ui.px(4.0), t.accent.with_alpha(0x30));
        } else if ui.inp.hot == Some(id) {
            ui.g.fill_rrect(ir, ui.px(4.0), t.hover);
        }
        let indent = ui.px(10.0 + 12.0 * sym.depth.min(8) as f32);
        let label = sym.name.clone();
        ui.text_clip(&label, Rect::new(ir.x + indent, ir.y, ir.w - indent - ui.px(4.0), ir.h), 12.5, FontId::Regular, t.text, Align::Left);
        ui.register(Widget {
            id,
            rect: ir,
            action: UiAction::Outline(sym.line),
            role: Role::ListItem,
            label: tf("{} {}, line {}", &[&tr(sym.kind), &sym.name, &(sym.line + 1).to_string()]),
            tooltip: Some(tf("{} · line {}", &[&tr(sym.kind), &(sym.line + 1).to_string()])),
            value: None,
            enabled: true,
            checked: Some(Some(k) == current),
            layer: 0,
        });
    }
}

fn preview_panel(app: &mut App, ui: &mut Ui, r: Rect) {
    let t = ui.t;
    ui.g.fill_rect(r, t.editor);
    ui.g.fill_rect(Rect::new(r.x, r.y, 1.0, r.h), t.border);
    let mut r = Rect::new(r.x + 1.0, r.y, r.w - 1.0, r.h);
    panel_header(ui, &mut r, "Markdown preview", Command::MarkdownPreview);
    let d = app.doc();
    if d.lang != Lang::Markdown {
        let msg = &tr("The preview shows Markdown documents (.md). Switch to a Markdown file or set Format › Language › Markdown.");
        let mr = Rect::new(r.x + ui.px(12.0), r.y + ui.px(8.0), r.w - ui.px(24.0), ui.px(60.0));
        ui.text_clip(msg, mr, 12.0, FontId::Regular, t.text_dim, Align::Left);
        ui.label(mr, msg);
        return;
    }
    if d.buf.text.len_bytes() > crate::ui::markdown::MAX_PREVIEW {
        let msg = &tr("This document is too large for the live preview.");
        let mr = Rect::new(r.x + ui.px(12.0), r.y + ui.px(8.0), r.w - ui.px(24.0), ui.px(30.0));
        ui.text_clip(msg, mr, 12.0, FontId::Regular, t.text_dim, Align::Left);
        return;
    }
    let text = d.buf.text.to_string();
    let key = (d.id, d.buf.version);
    let pad = ui.px(16.0);
    let base = ui.fs(14.0);
    app.preview.layout(ui.g, &text, key, r.w - 2.0 * pad, base);
    let max_scroll = (app.preview.height + 2.0 * pad - r.h).max(0.0);
    app.preview.scroll = app.preview.scroll.clamp(0.0, max_scroll);
    ui.g.set_clip(Some(r));
    app.preview.draw(ui.g, &t, r, pad);
    ui.g.set_clip(None);
    ui.register(Widget {
        id: super::wid(&UiAction::Label(9_000_001)),
        rect: r,
        action: UiAction::None,
        role: Role::Label,
        label: tr("Markdown preview"),
        tooltip: None,
        value: None,
        enabled: true,
        checked: None,
        layer: 0,
    });
}

fn results_panel(app: &mut App, ui: &mut Ui, r: Rect) {
    let t = ui.t;
    ui.g.fill_rect(r, t.window);
    ui.g.fill_rect(Rect::new(r.x, r.y, r.w, 1.0), t.border);
    let mut r = Rect::new(r.x, r.y + 1.0, r.w, r.h - 1.0);
    let (title, status, fif) = {
        let res = app.results.as_ref().expect("results");
        (res.title.clone(), res.status.clone(), res.fif)
    };
    let header = r.take_top(ui.px(30.0));
    ui.text_clip(&title, Rect::new(header.x + ui.px(10.0), header.y, header.w * 0.5, header.h), 12.5, FontId::Semibold, t.text, Align::Left);
    ui.text_clip(&status, Rect::new(header.x + header.w * 0.5, header.y, header.w * 0.5 - ui.px(44.0), header.h), 12.0, FontId::Regular, t.text_dim, Align::Right);
    ui.label(header, &format!("{title}. {status}"));
    ui.button(UiAction::CloseResults, Rect::new(header.right() - ui.px(32.0), header.y + ui.px(3.0), ui.px(26.0), header.h - ui.px(6.0)), "×", Some('\u{E711}'), &tr("Close results"), None, Style::Flat, None, true);
    if fif {
        let row = r.take_top(ui.px(36.0));
        let y = row.y + ui.px(3.0);
        let h = ui.px(28.0);
        let mut x = row.x + ui.px(10.0);
        let focus = app.focus;
        let w_q = (row.w * 0.24).max(ui.px(140.0));
        ui.field(UiAction::Field(Focus::FifQuery), Rect::new(x, y, w_q, h), &mut app.fif.query, focus == Focus::FifQuery, &tr("Find"), &tr("Find in files: text"), false);
        x += w_q + ui.px(6.0);
        let w_d = (row.w * 0.3).max(ui.px(160.0));
        ui.field(UiAction::Field(Focus::FifDir), Rect::new(x, y, w_d, h), &mut app.fif.dir, focus == Focus::FifDir, &tr("Folder"), &tr("Folder to search"), false);
        x += w_d + ui.px(2.0);
        ui.button(UiAction::FifBrowse, Rect::new(x, y, ui.px(30.0), h), "…", None, &tr("Browse for folder"), Some(&tr("Browse for a folder")), Style::Push, None, true);
        x += ui.px(36.0);
        let w_f = ui.px(120.0);
        ui.field(UiAction::Field(Focus::FifFilter), Rect::new(x, y, w_f, h), &mut app.fif.filter, focus == Focus::FifFilter, "*.rs;*.txt", &tr("File filter"), false);
        x += w_f + ui.px(6.0);
        let opts = [(0u8, "Aa", "Match case", app.find.case), (1, "W", "Whole word", app.find.word), (2, ".*", "Regular expression", app.find.regex)];
        for (i, txt, tip, on) in opts {
            let tip = &tr(tip);
            ui.button(UiAction::FindOpt(i), Rect::new(x, y, ui.px(28.0), h), txt, None, tip, Some(tip), Style::Flat, Some(on), true);
            x += ui.px(30.0);
        }
        x += ui.px(4.0);
        let (stop, search) = (tr("Stop"), tr("Search"));
        let bw = (ui.g.measure(&stop, ui.fs(12.5), FontId::Regular).max(ui.g.measure(&search, ui.fs(12.5), FontId::Regular)) + ui.px(24.0)).max(ui.px(70.0));
        if app.fif_running() {
            ui.button(UiAction::FifStop, Rect::new(x, y, bw, h), &stop, None, &tr("Stop search"), None, Style::Push, None, true);
        } else {
            ui.button(UiAction::FifSearch, Rect::new(x, y, bw, h), &search, None, &search, Some(&tr("Search (Enter)")), Style::Primary, None, true);
        }
    }
    let res = app.results.as_mut().expect("results");
    let item_h = ui.px(22.0);
    let visible = (r.h / item_h).floor().max(1.0) as usize;
    if res.sel < res.scroll {
        res.scroll = res.sel;
    } else if res.sel >= res.scroll + visible {
        res.scroll = res.sel + 1 - visible;
    }
    res.scroll = res.scroll.min(res.items.len().saturating_sub(visible));
    let base_dir = app.fif.dir.text.clone();
    let focused = app.focus == Focus::Results;
    let res = app.results.as_ref().expect("results");
    for (k, item) in res.items.iter().enumerate().skip(res.scroll).take(visible) {
        let y = r.y + (k - res.scroll) as f32 * item_h;
        let ir = Rect::new(r.x + ui.px(4.0), y, r.w - ui.px(8.0), item_h);
        let id = super::wid(&UiAction::Result(k));
        if k == res.sel {
            ui.g.fill_rect(ir, if focused { t.accent.with_alpha(0x40) } else { t.hover });
        } else if ui.inp.hot == Some(id) {
            ui.g.fill_rect(ir, t.hover);
        }
        let place = match &item.path {
            Some(p) => {
                let s = p.display().to_string();
                let rel = s.strip_prefix(&base_dir).map(|x| x.trim_start_matches(['\\', '/']).to_string()).unwrap_or(s);
                format!("{rel}:{}", item.line + 1)
            }
            None => tf("line {}", &[&(item.line + 1).to_string()]),
        };
        let px = ui.fs(12.0);
        let pw = ui.g.measure(&place, px, FontId::Regular).min(ir.w * 0.45);
        ui.text_clip(&place, Rect::new(ir.x + ui.px(6.0), ir.y, pw + 2.0, ir.h), 12.0, FontId::Regular, t.accent, Align::Left);
        let tx = ir.x + ui.px(16.0) + pw;
        let text: String = item.text.trim_end().to_string();
        let prefix: String = text.chars().take(item.col.0).collect();
        let m: String = text.chars().skip(item.col.0).take(item.col.1 - item.col.0).collect();
        let mono = ui.fs(12.0);
        let x1 = tx + ui.g.measure(&prefix, mono, FontId::Mono);
        let w1 = ui.g.measure(&m, mono, FontId::Mono);
        ui.g.set_clip(Some(Rect::new(tx, ir.y, ir.right() - tx, ir.h)));
        ui.g.fill_rect(Rect::new(x1, ir.y + 2.0, w1, ir.h - 4.0), t.match_bg);
        let (asc, desc) = ui.g.metrics(mono, FontId::Mono);
        ui.g.text(&text, tx, (ir.center_y() + (asc + desc) / 2.0).round(), mono, FontId::Mono, t.text);
        ui.g.set_clip(None);
        ui.register(Widget {
            id,
            rect: ir,
            action: UiAction::Result(k),
            role: Role::ListItem,
            label: format!("{place}: {}", text.trim()),
            tooltip: None,
            value: None,
            enabled: true,
            checked: Some(k == res.sel),
            layer: 0,
        });
    }
}

// ---------------------------------------------------------------- palette

fn palette(app: &mut App, ui: &mut Ui, full: Rect) {
    let t = ui.t;
    let w = (full.w * 0.6).clamp(ui.px(320.0), ui.px(640.0)).min(full.w - ui.px(16.0));
    let item_h = ui.px(28.0);
    let p = app.palette.as_mut().expect("palette");
    let list_n = if matches!(p.mode, PaletteMode::GotoLine | PaletteMode::Prompt(_)) { 0 } else { p.shown.len().min(12) };
    let h = ui.px(52.0) + list_n as f32 * item_h + if list_n > 0 { ui.px(8.0) } else { 0.0 };
    let pr = Rect::new(((full.w - w) / 2.0).round(), ui.px(48.0), w, h);
    ui.popup(pr);
    let fr = Rect::new(pr.x + ui.px(10.0), pr.y + ui.px(10.0), pr.w - ui.px(20.0), ui.px(32.0));
    let title = p.title.clone();
    ui.field(UiAction::Field(Focus::Palette), fr, &mut p.edit, true, &title, &title, false);
    if list_n == 0 {
        return;
    }
    if p.sel < p.scroll {
        p.scroll = p.sel;
    } else if p.sel >= p.scroll + list_n {
        p.scroll = p.sel + 1 - list_n;
    }
    for k in p.scroll..(p.scroll + list_n).min(p.shown.len()) {
        let item = &p.items[p.shown[k]];
        let y = fr.bottom() + ui.px(6.0) + (k - p.scroll) as f32 * item_h;
        let ir = Rect::new(pr.x + ui.px(6.0), y, pr.w - ui.px(12.0), item_h);
        let id = super::wid(&UiAction::PaletteItem(k));
        if k == p.sel {
            ui.g.fill_rrect(ir, ui.px(4.0), t.accent.with_alpha(0x40));
        } else if ui.inp.hot == Some(id) {
            ui.g.fill_rrect(ir, ui.px(4.0), t.hover);
        }
        let dw = if item.detail.is_empty() { 0.0 } else { ui.g.measure(&item.detail, ui.fs(12.0), FontId::Regular).min(ir.w * 0.4) + ui.px(12.0) };
        ui.text_clip(&item.label, Rect::new(ir.x + ui.px(10.0), ir.y, ir.w - dw - ui.px(14.0), ir.h), 13.0, FontId::Regular, t.text, Align::Left);
        if dw > 0.0 {
            ui.text_clip(&item.detail, Rect::new(ir.right() - dw, ir.y, dw - ui.px(8.0), ir.h), 12.0, FontId::Regular, t.text_dim, Align::Right);
        }
        ui.register(Widget {
            id,
            rect: ir,
            action: UiAction::PaletteItem(k),
            role: Role::ListItem,
            label: format!("{} {}", item.label.trim(), item.detail),
            tooltip: None,
            value: None,
            enabled: true,
            checked: Some(k == p.sel),
            layer: 0,
        });
    }
}
