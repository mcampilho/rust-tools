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
    Menu(u32),
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
            Command::Exit => {
                self.save_settings();
                el.exit();
            }
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
                if let Some(w) = &self.win {
                    w.window.set_theme(theme_for(t));
                }
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

    fn info(&self, title: &str, text: &str) {
        #[cfg(windows)]
        if let Some(w) = &self.win {
            win::dialogs::message(w.hwnd, title, text);
        }
        #[cfg(not(windows))]
        println!("{title}\n{text}");
    }

    fn checked(&self, c: Command) -> bool {
        match c {
            Command::WordWrap => self.settings.wrap,
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
        let (link, icon) = match (self.cursor, &self.drag) {
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
        r.set_children(vec![docid, statusid]);
        TreeUpdate { nodes: vec![(root, r), (docid, d), (statusid, st)], tree: Some(TreeInfo::new(root)), tree_id: TreeId::ROOT, focus: docid }
    }
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

fn theme_for(t: u8) -> Option<WinTheme> {
    match t {
        1 => Some(WinTheme::Light),
        2 => Some(WinTheme::Dark),
        _ => None,
    }
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
            .with_theme(theme_for(self.settings.theme))
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
            win::subclass(hwnd, &mnemonics_of(&commands::menu_tree()), Box::new(move |id| {
                let _ = proxy.send_event(UserEvent::Menu(id));
            }));
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
            UserEvent::Menu(id) => {
                #[cfg(windows)]
                {
                    let cmd = self.win.as_ref().and_then(|w| w.menu.as_ref()).and_then(|m| m.command(id));
                    if let Some(c) = cmd {
                        self.command(el, c);
                    }
                }
                let _ = (id, el);
            }
            UserEvent::A11y(ev) => {
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
            WindowEvent::CloseRequested => {
                self.save_settings();
                el.exit();
            }
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
            WindowEvent::RedrawRequested => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        el.set_control_flow(ControlFlow::Wait);
    }
}

const USAGE: &str = "rview [file]

  Shows a PDF, Markdown, HTML, text or CSV document.

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
    };
    shell.apply_language();
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rview: {e}");
    }
}
