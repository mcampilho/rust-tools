//! rview: a small, fast, portable viewer for PDF, Markdown, HTML, text and
//! CSV documents.
//!
//! The window is driven by winit and drawn on the CPU (tiny-skia +
//! ab_glyph) into a softbuffer surface. Text, CSV, Markdown and HTML are
//! parsed and laid out by rview itself; PDF pages and images are rendered
//! by the engines built into Windows. No network access, no scripts.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod doc;
mod i18n;
mod io;
mod media;
mod rich;
mod settings;
mod syntax;
mod storage;
mod ui;
mod view;
#[cfg(windows)]
mod win;

use commands::{Command, M};
use i18n::{tf, tr};
use settings::Settings;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use ui::gfx::{Align, FontId, Gfx, Rect};
use view::{Axis, LinkAction, Viewer};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Fullscreen, Icon as WinIcon, Theme as WinTheme, Window, WindowId};

enum UserEvent {
    A11y(accesskit_winit::Event),
    /// The media worker has results.
    Wake,
    Loaded(u64, Box<Result<doc::Content, String>>),
    #[cfg(windows)]
    Win(win::WinMsg),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(e: accesskit_winit::Event) -> Self {
        UserEvent::A11y(e)
    }
}

struct Win {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    adapter: accesskit_winit::Adapter,
    #[cfg(windows)]
    hwnd: windows_sys::Win32::Foundation::HWND,
    #[cfg(windows)]
    menu: Option<win::menu::MenuBar>,
}

/// An in-app message (About, Keyboard shortcuts), drawn in the current
/// theme. Lines containing a tab are shown as two aligned columns.
struct Dialog {
    title: String,
    text: String,
    /// Panel and OK button from the last frame (for clicks).
    panel: Rect,
    ok: Rect,
}

enum Drag {
    Pan { x: f32, y: f32, sx: f32, sy: f32 },
    Bar(Axis),
    Link { href: String, x: f32, y: f32 },
}

struct Shell {
    viewer: Viewer,
    gfx: Gfx,
    settings: Settings,
    location: storage::Location,
    media: media::Media,
    win: Option<Win>,
    proxy: EventLoopProxy<UserEvent>,
    mods: ModifiersState,
    cursor: Option<(f32, f32)>,
    drag: Option<Drag>,
    system_dark: bool,
    menu_dark: Option<bool>,
    menu_dirty: bool,
    menu_rebuild: bool,
    cursor_icon: CursorIcon,
    args: Vec<PathBuf>,
    status_note: Option<String>,
    /// Resident mode: closing the window hides it to the notification area.
    tray: bool,
    hidden: bool,
    dialog: Option<Dialog>,
}

impl Shell {
    fn scale(&self) -> f32 {
        self.win.as_ref().map_or(1.0, |w| w.window.scale_factor() as f32)
    }

    fn dark(&self) -> bool {
        match self.settings.theme {
            1 => false,
            2 => true,
            _ => self.system_dark,
        }
    }

    fn redraw(&self) {
        if let Some(w) = &self.win {
            w.window.request_redraw();
        }
    }

    fn apply_language(&mut self) {
        i18n::set_lang(self.settings.language.unwrap_or_else(i18n::system_lang));
        self.menu_rebuild = true;
        self.menu_dirty = true;
    }

    fn save_settings(&mut self) {
        if let Some(w) = &self.win {
            let max = w.window.is_maximized();
            if !max && w.window.fullscreen().is_none() && !w.window.is_minimized().unwrap_or(false) {
                let s: LogicalSize<f64> = w.window.inner_size().to_logical(w.window.scale_factor());
                self.settings.window = Some((s.width, s.height, false));
            } else if let Some((ww, hh, _)) = self.settings.window {
                self.settings.window = Some((ww, hh, max));
            }
        }
        let store = self.settings.to_store();
        self.location.save(&store);
    }

    // ------------------------------------------------------------ documents

    fn open(&mut self, path: &Path, keep: bool, anchor: Option<String>) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(d) = path.parent() {
            self.settings.last_dir = d.display().to_string();
        }
        let gen = self.viewer.begin(&path, keep, anchor);
        self.status_note = None;
        if self.viewer.kind == doc::Kind::Pdf {
            self.media.send(media::Job::OpenPdf { path: path.clone(), gen });
        } else {
            let proxy = self.proxy.clone();
            let kind = self.viewer.kind;
            let lang = i18n::lang();
            std::thread::spawn(move || {
                i18n::set_lang(lang);
                let result = doc::load(&path, kind);
                let _ = proxy.send_event(UserEvent::Loaded(gen, Box::new(result)));
            });
        }
        self.menu_dirty = true;
        self.update_title();
        self.redraw();
    }

    fn update_title(&self) {
        let Some(w) = &self.win else { return };
        let title = match &self.viewer.path {
            Some(p) => {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                match self.viewer.title() {
                    Some(t) if t != name => format!("{t} — {name} - rview"),
                    _ => format!("{name} - rview"),
                }
            }
            None => "rview".to_string(),
        };
        if w.window.title() != title {
            w.window.set_title(&title);
        }
    }

    fn open_dialog(&mut self) {
        #[cfg(windows)]
        {
            let Some(w) = &self.win else { return };
            let all = "*.pdf;*.md;*.markdown;*.html;*.htm;*.txt;*.csv";
            let filter = vec![
                (tr("Documents"), all),
                (tr("PDF documents"), "*.pdf"),
                ("Markdown".to_string(), "*.md;*.markdown"),
                (tr("Web pages"), "*.html;*.htm"),
                (tr("Text files"), "*.txt;*.log"),
                (tr("CSV files"), "*.csv;*.tsv"),
                (tr("All files"), "*.*"),
            ];
            let dir = (!self.settings.last_dir.is_empty()).then(|| PathBuf::from(&self.settings.last_dir));
            if let Some(p) = win::dialogs::open_file(w.hwnd, dir.as_deref(), &filter) {
                self.open(&p, false, None);
            }
        }
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self, el: &ActiveEventLoop, cmd: Command) {
        match cmd {
            Command::Open => self.open_dialog(),
            Command::Reload => {
                if let Some(p) = self.viewer.path.clone() {
                    self.open(&p, true, None);
                }
            }
            Command::OpenFolder => {
                #[cfg(windows)]
                if let Some(p) = &self.viewer.path {
                    win::reveal_in_explorer(p);
                }
            }
            Command::Close => self.close(el),
            Command::Exit => self.quit(el),
            Command::DarkPages => self.settings.dark_pages = !self.settings.dark_pages,
            Command::ZoomIn => self.viewer.zoom_step(1, None),
            Command::ZoomOut => self.viewer.zoom_step(-1, None),
            Command::ZoomReset => self.viewer.zoom_reset(),
            Command::FitWidth => self.viewer.fit_width(),
            Command::WordWrap => {
                self.settings.wrap = !self.settings.wrap;
                self.viewer.wrap = self.settings.wrap;
            }
            Command::FullScreen => {
                if let Some(w) = &self.win {
                    let fs = w.window.fullscreen().is_some();
                    w.window.set_fullscreen(if fs { None } else { Some(Fullscreen::Borderless(None)) });
                }
            }
            Command::Theme(t) => {
                self.settings.theme = t;
                self.apply_window_theme();
            }
            Command::UiLanguage(i) => {
                self.settings.language = (i as usize).checked_sub(1).and_then(|k| i18n::Lang::ALL.get(k)).copied();
                self.apply_language();
                self.viewer.status();
                if let Some(p) = self.viewer.path.clone() {
                    // Error and status texts follow the new language.
                    if matches!(self.viewer.state, view::State::Error(_)) {
                        self.open(&p, true, None);
                    }
                }
            }
            Command::Shortcuts => self.info(&tr("Keyboard shortcuts"), &shortcuts_text()),
            Command::About => self.info(
                &tr("About rview"),
                &tf(
                    "rview {}\n\nA small, fast, portable viewer for PDF, Markdown, HTML, text and CSV documents, written in Rust.\nNo telemetry, no network access, no scripts.\n\nPDF pages and images are rendered by Windows.\n\nMIT License.",
                    &[env!("CARGO_PKG_VERSION")],
                ),
            ),
        }
        self.menu_dirty = true;
        self.redraw();
    }

    /// The close button: hides to the tray in resident mode.
    fn close(&mut self, el: &ActiveEventLoop) {
        if self.tray {
            self.hide();
        } else {
            self.quit(el);
        }
    }

    fn quit(&mut self, el: &ActiveEventLoop) {
        self.save_settings();
        #[cfg(windows)]
        if let Some(w) = &self.win {
            win::tray::remove(w.hwnd);
        }
        el.exit();
    }

    /// Hides the window to the notification area, keeping the document.
    fn hide(&mut self) {
        self.save_settings();
        if let Some(w) = &self.win {
            w.window.set_visible(false);
        }
        self.hidden = true;
        self.drag = None;
        // Release the frame buffer while hidden.
        self.gfx.resize(1, 1);
        self.gfx.trim_cache();
    }

    fn show(&mut self) {
        if let Some(w) = &self.win {
            w.window.set_visible(true);
            w.window.set_minimized(false);
            w.window.focus_window();
        }
        self.hidden = false;
        self.redraw();
    }

    fn info(&mut self, title: &str, text: &str) {
        self.dialog = Some(Dialog { title: title.to_string(), text: text.to_string(), panel: Rect::default(), ok: Rect::default() });
        self.drag = None;
        self.redraw();
    }

    fn checked(&self, c: Command) -> bool {
        match c {
            Command::WordWrap => self.settings.wrap,
            Command::DarkPages => self.settings.dark_pages,
            Command::FitWidth => self.viewer.fit && self.viewer.kind == doc::Kind::Pdf,
            Command::FullScreen => self.win.as_ref().is_some_and(|w| w.window.fullscreen().is_some()),
            Command::Theme(t) => self.settings.theme == t,
            Command::UiLanguage(0) => self.settings.language.is_none(),
            Command::UiLanguage(i) => self.settings.language == i18n::Lang::ALL.get(i as usize - 1).copied(),
            _ => false,
        }
    }

    fn enabled(&self, c: Command) -> bool {
        let ready = matches!(self.viewer.state, view::State::Ready(_));
        match c {
            Command::Reload | Command::OpenFolder => self.viewer.path.is_some(),
            Command::ZoomIn | Command::ZoomOut | Command::ZoomReset => ready,
            Command::FitWidth => ready && self.viewer.kind == doc::Kind::Pdf,
            Command::WordWrap => self.viewer.kind == doc::Kind::Text || self.viewer.path.is_none(),
            Command::DarkPages => self.dark(),
            _ => true,
        }
    }

    #[cfg(windows)]
    fn sync_menu(&mut self) {
        let Some(win) = &mut self.win else { return };
        if !self.menu_dirty && win.menu.is_some() {
            return;
        }
        self.menu_dirty = false;
        if win.menu.is_none() || self.menu_rebuild {
            self.menu_rebuild = false;
            let tree = commands::menu_tree();
            win::set_mnemonics(&mnemonics_of(&tree));
            win.menu = Some(win::menu::MenuBar::install(win.hwnd, tree));
        }
        let me = &*self;
        if let Some(m) = me.win.as_ref().and_then(|w| w.menu.as_ref()) {
            m.sync(|c| me.checked(c), |c| me.enabled(c));
        }
    }

    #[cfg(not(windows))]
    fn sync_menu(&mut self) {}

    // ------------------------------------------------------------ input

    fn key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) {
        if self.dialog.is_some() {
            if matches!(event.logical_key, WKey::Named(NamedKey::Escape | NamedKey::Enter | NamedKey::Space)) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        let alt = self.mods.alt_key();
        let logical = if ctrl || alt {
            use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
            event.key_without_modifiers()
        } else {
            event.logical_key.clone()
        };
        let s = self.scale();
        let line = 48.0 * s;
        let page = self.viewer.page_height();
        let name = match &logical {
            WKey::Character(c) => c.to_lowercase(),
            WKey::Named(n) => match n {
                NamedKey::F1 => "f1".into(),
                NamedKey::F5 => "f5".into(),
                NamedKey::F11 => "f11".into(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        if !name.is_empty() {
            if let Some(cmd) = commands::lookup(ctrl, shift, alt, &name) {
                return self.command(el, cmd);
            }
        }
        match logical {
            WKey::Named(NamedKey::ArrowDown) => self.viewer.scroll_by(0.0, line),
            WKey::Named(NamedKey::ArrowUp) => self.viewer.scroll_by(0.0, -line),
            WKey::Named(NamedKey::ArrowRight) => self.viewer.scroll_by(line, 0.0),
            WKey::Named(NamedKey::ArrowLeft) => self.viewer.scroll_by(-line, 0.0),
            WKey::Named(NamedKey::PageDown) => {
                if !(ctrl && self.viewer.pdf_page_step(1)) {
                    self.viewer.scroll_by(0.0, page)
                }
            }
            WKey::Named(NamedKey::PageUp) => {
                if !(ctrl && self.viewer.pdf_page_step(-1)) {
                    self.viewer.scroll_by(0.0, -page)
                }
            }
            WKey::Named(NamedKey::Space) => self.viewer.scroll_by(0.0, if shift { -page } else { page }),
            WKey::Named(NamedKey::Home) => self.viewer.scroll_to(0.0),
            WKey::Named(NamedKey::End) => self.viewer.scroll_to(f32::MAX),
            WKey::Named(NamedKey::Escape) => {
                if let Some(w) = &self.win {
                    if w.window.fullscreen().is_some() {
                        w.window.set_fullscreen(None);
                    }
                }
            }
            _ => return,
        }
        self.redraw();
    }

    fn update_hover(&mut self) {
        let (link, icon) = match (self.cursor.filter(|_| self.dialog.is_none()), &self.drag) {
            (_, Some(Drag::Pan { .. })) => (None, CursorIcon::Grabbing),
            (Some((x, y)), _) => match self.viewer.link_at(x, y) {
                Some(l) => (Some(l), CursorIcon::Pointer),
                None => (None, CursorIcon::Default),
            },
            _ => (None, CursorIcon::Default),
        };
        if link != self.viewer.hover_link {
            self.viewer.hover_link = link;
            self.redraw();
        }
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(w) = &self.win {
                w.window.set_cursor(icon);
            }
        }
    }

    fn follow(&mut self, href: &str) {
        match self.viewer.follow(href) {
            LinkAction::Scrolled => {}
            LinkAction::OpenUrl(u) => {
                #[cfg(windows)]
                win::open_url(&u);
            }
            LinkAction::OpenFile(p, anchor) => self.open(&p, false, anchor),
            LinkAction::Unsupported(h) => self.status_note = Some(tf("This link cannot be opened here: {}", &[&h])),
        }
        self.redraw();
    }

    // ------------------------------------------------------------ frame

    fn render(&mut self) {
        let dark = self.dark();
        let theme = ui::theme::Theme::new(dark);
        let s = self.scale();
        let Some(win) = &mut self.win else { return };
        let size = win.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if win.surface.resize(w, h).is_err() {
            return;
        }
        self.gfx.resize(w.get(), h.get());
        let full = Rect::new(0.0, 0.0, w.get() as f32, h.get() as f32);
        let mut area = full;
        let status = area.take_bottom((24.0 * s).round());
        self.viewer.dark_pages = self.settings.dark_pages;
        self.viewer.draw(&mut self.gfx, area, &theme, s, &self.media);
        // Status bar.
        let g = &mut self.gfx;
        g.fill_rect(status, theme.status);
        g.fill_rect(Rect::new(status.x, status.y, status.w, 1.0), theme.border);
        let px = 12.0 * s;
        let pad = 10.0 * s;
        let right = self.viewer.status().join("    ");
        let rw = g.measure(&right, px, FontId::Regular);
        g.text_in(&right, Rect::new(status.right() - rw - pad, status.y, rw, status.h), px, FontId::Regular, theme.dim, Align::Left);
        let left = self.viewer.hover_link.clone().or_else(|| self.status_note.clone()).or_else(|| self.viewer.path.as_ref().map(|p| p.display().to_string())).unwrap_or_default();
        let lw = (status.w - rw - 3.0 * pad).max(0.0);
        let left = fit(g, &left, lw, px);
        g.text_in(&left, Rect::new(status.x + pad, status.y, lw, status.h), px, FontId::Regular, theme.text, Align::Left);
        if let Some(d) = &mut self.dialog {
            let (panel, ok) = draw_dialog(&mut self.gfx, full, &theme, s, &d.title, &d.text);
            d.panel = panel;
            d.ok = ok;
        }
        let Some(win) = &mut self.win else { return };
        if let Ok(mut buf) = win.surface.buffer_mut() {
            self.gfx.copy_to(&mut buf);
            let _ = buf.present();
        }
        self.gfx.trim_cache();
        self.sync_menu();
        self.sync_menu_theme(dark);
        self.update_title();
        let tree = self.tree();
        if let Some(win) = &mut self.win {
            win.adapter.update_if_active(|| tree);
        }
    }

    /// Gives the title bar the resolved light/dark theme. winit is always
    /// passed an explicit theme: with "follow system" it asks
    /// `ShouldAppsUseDarkMode`, which reflects the menu mode rview forced
    /// last, not the Windows setting. System changes arrive as
    /// `WinMsg::SystemTheme`.
    fn apply_window_theme(&self) {
        if let Some(w) = &self.win {
            w.window.set_theme(win_theme(self.dark()));
            #[cfg(windows)]
            win::darkmode::refresh_caption(w.hwnd);
        }
    }

    fn sync_menu_theme(&mut self, dark: bool) {
        if self.menu_dark == Some(dark) {
            return;
        }
        self.menu_dark = Some(dark);
        #[cfg(windows)]
        if let Some(w) = &self.win {
            let colors = dark.then_some(win::darkmode::MenuColors { bar: 0x202020, text: 0xE4E4E4, text_disabled: 0x7A7A7A, hot: 0x3D3D3D });
            win::darkmode::apply(w.hwnd, colors);
        }
    }

    fn tree(&self) -> accesskit::TreeUpdate {
        use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
        let root = NodeId(0);
        let docid = NodeId(1);
        let statusid = NodeId(2);
        let mut d = Node::new(Role::Document);
        d.set_label(self.viewer.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "rview".into()));
        d.set_value(self.viewer.accessible_text());
        d.add_action(accesskit::Action::Focus);
        let mut st = Node::new(Role::Label);
        st.set_value(self.viewer.status().join(", "));
        let mut r = Node::new(Role::Window);
        r.set_label("rview");
        let mut nodes = vec![(docid, d), (statusid, st)];
        let mut children = vec![docid, statusid];
        let mut focus = docid;
        if let Some(dl) = &self.dialog {
            let dlgid = NodeId(3);
            let textid = NodeId(4);
            let okid = NodeId(5);
            let mut dn = Node::new(Role::Dialog);
            dn.set_label(dl.title.clone());
            dn.set_children(vec![textid, okid]);
            let mut tn = Node::new(Role::Label);
            tn.set_value(dl.text.replace('\t', "  "));
            let mut ok = Node::new(Role::Button);
            ok.set_label("OK");
            ok.add_action(accesskit::Action::Click);
            nodes.extend([(dlgid, dn), (textid, tn), (okid, ok)]);
            children.push(dlgid);
            focus = okid;
        }
        r.set_children(children);
        nodes.push((root, r));
        TreeUpdate { nodes, tree: Some(TreeInfo::new(root)), tree_id: TreeId::ROOT, focus }
    }
}

/// Word-wraps `text` to `width` pixels.
fn wrap_words(g: &mut Gfx, text: &str, width: f32, px: f32, font: FontId) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if !line.is_empty() && g.measure(&candidate, px, font) > width {
            out.push(std::mem::take(&mut line));
            line = word.to_string();
        } else {
            line = candidate;
        }
    }
    out.push(line);
    out
}

/// Draws a themed message panel over the window; returns (panel, OK button).
fn draw_dialog(g: &mut Gfx, full: Rect, th: &ui::theme::Theme, s: f32, title: &str, text: &str) -> (Rect, Rect) {
    use ui::gfx::Color;
    g.fill_rect(full, Color::rgba(0x000000, if th.dark { 0x90 } else { 0x60 }));
    let pad = (24.0 * s).round();
    let px = 13.5 * s;
    let lh = (px * 1.55).round();
    let title_px = 17.0 * s;
    let w = (600.0 * s).min(full.w - 32.0 * s).max(200.0 * s);
    let inner = w - 2.0 * pad;
    // Two-column rows: the key column fits the widest key.
    let key_w = text
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(k, _)| g.measure(k, px, FontId::Bold))
        .fold(0.0f32, f32::max);
    let col = if key_w > 0.0 { (key_w + 20.0 * s).min(inner * 0.55) } else { 0.0 };
    let mut rows: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        match line.split_once('\t') {
            Some((k, v)) => {
                for (i, part) in wrap_words(g, v, inner - col, px, FontId::Regular).into_iter().enumerate() {
                    rows.push((if i == 0 { k.to_string() } else { String::new() }, part));
                }
            }
            None => {
                for part in wrap_words(g, line, inner, px, FontId::Regular) {
                    rows.push((String::new(), part));
                }
            }
        }
    }
    let btn_h = (32.0 * s).round();
    let h = (pad + title_px * 1.6 + 8.0 * s + rows.len() as f32 * lh + 18.0 * s + btn_h + pad).min(full.h - 16.0 * s);
    let panel = Rect::new(((full.w - w) / 2.0).round(), ((full.h - h) / 2.0).round().max(8.0 * s), w.round(), h.round());
    let bg = if th.dark { Color::rgb(0x2B2B2B) } else { Color::rgb(0xFFFFFF) };
    let r = (8.0 * s).round();
    for i in 1..=4 {
        let d = i as f32 * s;
        g.fill_rrect(Rect::new(panel.x - d, panel.y - d + 2.0 * s, panel.w + 2.0 * d, panel.h + 2.0 * d), r + d, Color::rgba(0x000000, 0x18));
    }
    g.fill_rrect(panel, r, bg);
    g.stroke_rrect(panel, r, 1.0, th.border);
    let mut y = panel.y + pad;
    let (asc, _) = g.metrics(title_px, FontId::Bold);
    g.text(title, panel.x + pad, y + asc, title_px, FontId::Bold, th.text);
    y += title_px * 1.6 + 8.0 * s;
    let (asc, _) = g.metrics(px, FontId::Regular);
    let bottom = panel.bottom() - pad - btn_h - 18.0 * s;
    for (k, v) in rows {
        if y + lh > bottom + 1.0 {
            break;
        }
        let base = (y + (lh + asc) / 2.0 - px * 0.15).round();
        if !k.is_empty() {
            g.text(&k, panel.x + pad, base, px, FontId::Bold, th.text);
        }
        g.text(&v, panel.x + pad + col, base, px, FontId::Regular, if col > 0.0 { th.dim } else { th.text });
        y += lh;
    }
    let bw = (88.0 * s).round();
    let ok = Rect::new(panel.right() - pad - bw, panel.bottom() - pad - btn_h, bw, btn_h);
    let accent = if th.dark { Color::rgb(0x4CC2FF) } else { Color::rgb(0x0067C0) };
    g.fill_rrect(ok, (4.0 * s).round(), accent);
    g.text_in("OK", ok, 13.0 * s, FontId::Bold, if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) }, Align::Center);
    (panel, ok)
}

fn fit(g: &mut Gfx, s: &str, max: f32, px: f32) -> String {
    if g.measure(s, px, FontId::Regular) <= max {
        return s.to_string();
    }
    let mut out: String = String::new();
    for c in s.chars() {
        out.push(c);
        if g.measure(&out, px, FontId::Regular) + g.measure("…", px, FontId::Regular) > max {
            out.pop();
            break;
        }
    }
    out.push('…');
    out
}

fn shortcuts_text() -> String {
    let rows = [
        ("Ctrl+O", tr("Open a document")),
        ("F5", tr("Reload")),
        ("Ctrl+W", tr("Close rview")),
        ("Ctrl+= / Ctrl+-", tr("Zoom in / out")),
        ("Ctrl+Wheel", tr("Zoom at the mouse pointer")),
        ("Ctrl+0", tr("Actual size (100 %)")),
        ("Ctrl+9", tr("Fit width (PDF)")),
        ("Alt+Z", tr("Word wrap (text)")),
        ("F11", tr("Full screen")),
        ("↑ ↓ ← →  PgUp PgDn", tr("Scroll")),
        ("Space / Shift+Space", tr("Page down / up")),
        ("Home / End", tr("Start / end of the document")),
        ("Ctrl+PgUp / Ctrl+PgDn", tr("Previous / next page (PDF)")),
        ("Drag", tr("Pan the document")),
    ];
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
}

fn win_theme(dark: bool) -> Option<WinTheme> {
    Some(if dark { WinTheme::Dark } else { WinTheme::Light })
}

fn app_icon() -> Option<WinIcon> {
    let n = 32;
    WinIcon::from_rgba(ui::appicon::render(n), n, n).ok()
}

#[cfg(windows)]
fn mnemonics_of(tree: &[(&'static str, Vec<M>)]) -> String {
    tree.iter().filter_map(|(t, _)| tr(t).split('&').nth(1).and_then(|s| s.chars().next())).collect()
}

#[cfg(windows)]
fn hwnd_of(window: &Window) -> windows_sys::Win32::Foundation::HWND {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get() as _,
        _ => std::ptr::null_mut(),
    }
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        let (w, h, maximized) = self.settings.window.unwrap_or((1000.0, 760.0, false));
        let attrs = Window::default_attributes()
            .with_title("rview")
            .with_inner_size(LogicalSize::new(w.max(360.0), h.max(240.0)))
            .with_min_inner_size(LogicalSize::new(360.0, 240.0))
            .with_maximized(maximized)
            .with_window_icon(app_icon())
            .with_theme(win_theme(self.dark()))
            .with_visible(false);
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("rview: cannot create window: {e}");
                el.exit();
                return;
            }
        };
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(el, &window, self.proxy.clone());
        let surface = match softbuffer::Context::new(window.clone()).and_then(|ctx| softbuffer::Surface::new(&ctx, window.clone())) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rview: cannot create drawing surface: {e}");
                el.exit();
                return;
            }
        };
        #[cfg(windows)]
        let hwnd = hwnd_of(&window);
        #[cfg(windows)]
        {
            let proxy = self.proxy.clone();
            win::subclass(
                hwnd,
                &mnemonics_of(&commands::menu_tree()),
                Box::new(move |m| {
                    let _ = proxy.send_event(UserEvent::Win(m));
                }),
            );
            if self.tray {
                win::set_resident(hwnd);
                win::tray::show(hwnd, "rview");
            }
        }
        self.win = Some(Win {
            window,
            surface,
            adapter,
            #[cfg(windows)]
            hwnd,
            #[cfg(windows)]
            menu: None,
        });
        self.sync_menu();
        if let Some(p) = self.args.first().cloned() {
            self.open(&p, false, None);
        }
        // Resident mode starts in the notification area unless a file was given.
        if self.tray && self.args.is_empty() {
            self.hidden = true;
            return;
        }
        if let Some(w) = &self.win {
            w.window.set_visible(true);
            w.window.focus_window();
        }
        self.redraw();
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Wake => {
                let mut any = false;
                while let Ok(done) = self.media.rx.try_recv() {
                    any |= self.viewer.on_media(done);
                }
                if any {
                    self.menu_dirty = true;
                    self.redraw();
                }
            }
            UserEvent::Loaded(gen, result) => {
                self.viewer.finish(gen, *result);
                self.menu_dirty = true;
                self.update_title();
                self.redraw();
            }
            #[cfg(windows)]
            UserEvent::Win(win::WinMsg::Command(id)) => {
                let cmd = self.win.as_ref().and_then(|w| w.menu.as_ref()).and_then(|m| m.command(id));
                if let Some(c) = cmd {
                    self.command(el, c);
                }
            }
            #[cfg(windows)]
            UserEvent::Win(win::WinMsg::SystemTheme) => {
                let dark = win::system_dark().unwrap_or(false);
                if dark != self.system_dark {
                    self.system_dark = dark;
                    self.apply_window_theme();
                    self.redraw();
                }
            }
            #[cfg(windows)]
            UserEvent::Win(win::WinMsg::OpenFiles(paths)) => {
                self.show();
                if let Some(p) = paths.first() {
                    self.open(p, false, None);
                }
            }
            #[cfg(windows)]
            UserEvent::Win(win::WinMsg::Tray(win::tray::TrayEvent::Activate)) => self.show(),
            #[cfg(windows)]
            UserEvent::Win(win::WinMsg::Tray(win::tray::TrayEvent::Menu)) => {
                let hwnd = self.win.as_ref().map(|w| w.hwnd);
                if let Some(hwnd) = hwnd {
                    match win::tray::menu(hwnd, &tr("Open rview"), &tr("Exit")) {
                        Some(1) => self.show(),
                        Some(2) => self.quit(el),
                        _ => {}
                    }
                }
            }
            UserEvent::A11y(ev) => {
                if let accesskit_winit::WindowEvent::ActionRequested(req) = &ev.window_event {
                    if req.target_node == accesskit::NodeId(5) && req.action == accesskit::Action::Click {
                        self.dialog = None;
                        self.redraw();
                    }
                }
                if let accesskit_winit::WindowEvent::InitialTreeRequested = ev.window_event {
                    let tree = self.tree();
                    if let Some(w) = &mut self.win {
                        w.adapter.update_if_active(|| tree);
                    }
                }
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(w) = &mut self.win {
            w.adapter.process_event(&w.window, &event);
        }
        match event {
            WindowEvent::CloseRequested => self.close(el),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.redraw(),
            WindowEvent::ThemeChanged(t) => {
                self.system_dark = t == WinTheme::Dark;
                self.redraw();
            }
            WindowEvent::Focused(false) => self.save_settings(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::DroppedFile(p) => self.open(&p, false, None),
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if event.state == ElementState::Pressed {
                    self.key(el, &event);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
                self.cursor = Some((x, y));
                match &mut self.drag {
                    Some(Drag::Pan { x: x0, y: y0, sx, sy }) => {
                        let (dx, dy) = (*x0 - x, *y0 - y);
                        let (sx, sy) = (*sx, *sy);
                        self.viewer.scroll = (sx + dx, sy + dy);
                        self.viewer.scroll_by(0.0, 0.0);
                        self.redraw();
                    }
                    Some(Drag::Bar(axis)) => {
                        let axis = *axis;
                        self.viewer.drag_scrollbar(axis, x, y);
                        self.redraw();
                    }
                    // Moving far turns a link press into panning.
                    Some(Drag::Link { x: x0, y: y0, .. }) if (x - *x0).abs() + (y - *y0).abs() > 6.0 => {
                        let (sx, sy) = self.viewer.scroll;
                        self.drag = Some(Drag::Pan { x: *x0, y: *y0, sx, sy });
                    }
                    Some(Drag::Link { .. }) | None => {}
                }
                self.update_hover();
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.update_hover();
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => {
                    let Some((x, y)) = self.cursor else { return };
                    if let Some(d) = &self.dialog {
                        if d.ok.contains(x, y) || !d.panel.contains(x, y) {
                            self.dialog = None;
                            self.redraw();
                        }
                        return;
                    }
                    if let Some(axis) = self.viewer.scrollbar_at(x, y) {
                        self.viewer.drag_scrollbar(axis, x, y);
                        self.drag = Some(Drag::Bar(axis));
                    } else if let Some(href) = self.viewer.link_at(x, y) {
                        self.drag = Some(Drag::Link { href, x, y });
                    } else if self.viewer.in_view(x, y) {
                        let (sx, sy) = self.viewer.scroll;
                        self.drag = Some(Drag::Pan { x, y, sx, sy });
                    }
                    self.update_hover();
                    self.redraw();
                }
                ElementState::Released => {
                    if let Some(Drag::Link { href, .. }) = self.drag.take() {
                        self.follow(&href);
                    }
                    self.update_hover();
                }
            },
            WindowEvent::MouseWheel { .. } if self.dialog.is_some() => {}
            WindowEvent::MouseWheel { delta, .. } => {
                let s = self.scale();
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (-x * 48.0 * s, -y * 48.0 * s),
                    MouseScrollDelta::PixelDelta(p) => (-p.x as f32, -p.y as f32),
                };
                if self.mods.control_key() {
                    let anchor = self.cursor.map(|(x, y)| (x - 0.0, y - 0.0));
                    self.viewer.zoom_step(if dy < 0.0 { 1 } else { -1 }, anchor);
                    self.menu_dirty = true;
                } else if self.mods.shift_key() {
                    self.viewer.scroll_by(dy, 0.0);
                } else {
                    self.viewer.scroll_by(dx, dy);
                }
                self.redraw();
                self.update_hover();
            }
            WindowEvent::RedrawRequested if !self.hidden => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        el.set_control_flow(ControlFlow::Wait);
    }
}

const USAGE: &str = "rview [--tray] [--new-window] [file]

  Shows a PDF, Markdown, HTML, text or CSV document.

  --tray        Stay resident in the notification area: closing the window
                hides it, and launching rview again (with or without a
                file) or clicking the tray icon shows it instantly. Exit
                from the tray icon's menu or File > Exit.
  --new-window  Open a separate window even when rview is resident.

Settings are stored in %APPDATA%\\rview, or next to rview.exe when a file
named rview.portable exists there (portable mode).";

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.iter().any(|a| a == "--help" || a == "-h" || a == "/?") {
        #[cfg(windows)]
        win::dialogs::message(std::ptr::null_mut(), "rview", USAGE);
        #[cfg(not(windows))]
        println!("{USAGE}");
        return;
    }
    let args: Vec<PathBuf> = raw.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    let tray = raw.iter().any(|a| a == "--tray" || a == "/tray");
    let new_window = raw.iter().any(|a| a == "--new-window");
    // A resident instance shows itself (and the file) instantly.
    #[cfg(windows)]
    if !new_window && win::hand_off_to_resident(&args) {
        return;
    }
    let _ = new_window;
    let mut location = storage::Location::detect();
    let store = location.load();
    let settings = Settings::load(&store);
    let gfx = match Gfx::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rview: {e}");
            std::process::exit(1);
        }
    };
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("rview: cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    let proxy = Arc::new(Mutex::new(event_loop.create_proxy()));
    let media = media::Media::start(Arc::new(move || {
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(UserEvent::Wake);
        }
    }));
    #[cfg(windows)]
    let system_dark = win::system_dark().unwrap_or(false);
    #[cfg(not(windows))]
    let system_dark = false;
    let mut shell = Shell {
        viewer: Viewer::new(settings.wrap),
        gfx,
        settings,
        location,
        media,
        win: None,
        proxy: event_loop.create_proxy(),
        mods: ModifiersState::empty(),
        cursor: None,
        drag: None,
        system_dark,
        menu_dark: None,
        menu_dirty: true,
        menu_rebuild: true,
        cursor_icon: CursorIcon::Default,
        args,
        status_note: None,
        tray,
        hidden: false,
        dialog: None,
    };
    shell.apply_language();
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rview: {e}");
    }
}
