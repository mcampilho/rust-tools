//! rview: a small, fast, portable viewer for PDF, Markdown, HTML, text and
//! CSV documents.
//!
//! The window is driven by winit and drawn on the CPU (tiny-skia +
//! ab_glyph) into a softbuffer surface. Text, CSV, Markdown and HTML are
//! parsed and laid out by rview itself; PDF pages and images are rendered
//! by the engines built into Windows, and by the hayro and image crates on
//! Linux. No network access, no scripts.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clip;
mod commands;
mod input;
mod menus;
mod render;
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
#[cfg(not(windows))]
mod linux;

use commands::Command;
use i18n::{tf, tr};
use settings::Settings;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ui::gfx::{Align, FontId, Gfx, Rect};
use view::{Axis, Hover, LinkAction, Viewer};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use rt_ui::Message;
use winit::window::{CursorIcon, Fullscreen, Theme as WinTheme, Window, WindowId};

enum UserEvent {
    A11y(accesskit_winit::Event),
    /// The media worker has results.
    Wake,
    Loaded(u64, Box<Result<doc::Content, String>>),
    #[cfg(windows)]
    Win(win::WinMsg),
    #[cfg(not(windows))]
    Shell(linux::shell::Msg),
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
    Select,
}

/// Longest pause and distance between the clicks of a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_SLOP: f32 = 4.0;
/// Pause between scroll steps while dragging a selection past the edge.
const AUTOSCROLL_STEP: Duration = Duration::from_millis(30);

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
    /// An in-app message (About, Keyboard shortcuts, the help).
    dialog: Option<Message>,
    /// "Open with rview" is registered for this rview.exe.
    #[cfg(windows)]
    integrated: bool,
    /// Time and place of the last click, and how many came in a row.
    last_click: Option<(Instant, f32, f32, u32)>,
    /// Menus drawn in the window.
    #[cfg(not(windows))]
    menu: ui::menu::Menu,
    #[cfg(not(windows))]
    shell: Option<linux::shell::Shell>,
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
            rt_ui::window::remember_size(&w.window, &mut self.settings.window);
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
        let all = "*.pdf;*.md;*.markdown;*.html;*.htm;*.txt;*.csv";
        let filter = vec![
            (tr("Documents"), all),
            (tr("PDF documents"), "*.pdf"),
            ("Markdown".to_string(), "*.md;*.markdown"),
            (tr("Web pages"), "*.html;*.htm"),
            (tr("Text files"), "*.txt;*.log"),
            (tr("CSV files"), "*.csv;*.tsv"),
            (tr("All files"), if cfg!(windows) { "*.*" } else { "*" }),
        ];
        let dir = (!self.settings.last_dir.is_empty()).then(|| PathBuf::from(&self.settings.last_dir));
        #[cfg(windows)]
        let chosen = self.win.as_ref().and_then(|w| win::dialogs::open_file(w.hwnd, dir.as_deref(), &filter));
        #[cfg(not(windows))]
        let chosen = linux::open_file(dir.as_deref(), &filter);
        if let Some(p) = chosen {
            self.open(&p, false, None);
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
                if let Some(p) = &self.viewer.path {
                    #[cfg(windows)]
                    win::reveal_in_explorer(p);
                    #[cfg(not(windows))]
                    linux::reveal(p);
                }
            }
            Command::Close => self.close(el),
            Command::Exit => self.quit(el),
            Command::Copy => self.copy(false),
            Command::CopyPlain => self.copy(true),
            Command::SelectAll => {
                self.viewer.select_all();
                self.status_note = self.viewer.pdf_text_pending().then(|| tr("Reading the text of this PDF…"));
            }
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
            #[cfg(windows)]
            Command::ExplorerMenu => self.integration(if self.integrated { Integration::Unregister } else { Integration::Register }),
            #[cfg(windows)]
            Command::DefaultViewer => self.integration(Integration::Associate),
            Command::Shortcuts => self.info(&tr("Keyboard shortcuts"), &shortcuts_text()),
            Command::About => {
                let version = [env!("CARGO_PKG_VERSION")];
                let text = if cfg!(windows) {
                    tf("rview {}\n\nA small, fast, portable viewer for PDF, Markdown, HTML, text and CSV documents, written in Rust.\nNo telemetry, no network access, no scripts.\n\nPDF pages and images are rendered by Windows.\n\nMIT License.", &version)
                } else {
                    tf("rview {}\n\nA small, fast, portable viewer for PDF, Markdown, HTML, text and CSV documents, written in Rust.\nNo telemetry, no network access, no scripts.\n\nPDF pages are rendered by hayro.\n\nMIT License.", &version)
                };
                self.info(&tr("About rview"), &text);
            }
        }
        self.menu_dirty = true;
        self.redraw();
    }

    /// Copies the selection, with its formats or as text only, and says
    /// what happened in the status bar.
    fn copy(&mut self, plain: bool) {
        if !self.viewer.has_selection() {
            return;
        }
        let Some(payload) = self.viewer.payload(plain) else {
            self.status_note = Some(tr("The selection has no text."));
            return;
        };
        #[cfg(windows)]
        let written = self.win.as_ref().is_some_and(|w| win::clipboard::set(w.hwnd, &payload));
        #[cfg(not(windows))]
        let written = linux::set_clipboard(&payload);
        self.status_note = Some(if !written {
            tr("The clipboard is not available.")
        } else if payload.too_large {
            tr("The selection is too large for formats; it was copied as plain text.")
        } else if plain {
            tr("Copied to the clipboard as plain text.")
        } else {
            tr("Copied to the clipboard.")
        });
    }

    /// Counts consecutive clicks at a place (1, 2 or 3, then back to 1).
    fn count_click(&mut self, x: f32, y: f32) -> u32 {
        let now = Instant::now();
        let n = match self.last_click {
            Some((t, cx, cy, n)) if now - t < DOUBLE_CLICK && (cx - x).abs() < DOUBLE_CLICK_SLOP && (cy - y).abs() < DOUBLE_CLICK_SLOP => n % 3 + 1,
            _ => 1,
        };
        self.last_click = Some((now, x, y, n));
        n
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
        #[cfg(not(windows))]
        {
            self.shell = None;
        }
        el.exit();
    }

    /// Hides the window to the notification area, keeping the document.
    fn hide(&mut self) {
        self.save_settings();
        if cfg!(windows) {
            if let Some(w) = &self.win {
                w.window.set_visible(false);
            }
        } else {
            // Wayland cannot hide a window, so it is closed and made again.
            self.win = None;
            self.cursor = None;
        }
        self.hidden = true;
        self.drag = None;
        // Release the frame buffer while hidden.
        self.gfx.resize(1, 1);
        self.gfx.trim_cache();
    }

    fn show(&mut self, el: &ActiveEventLoop) {
        if self.win.is_none() && !self.open_window(el) {
            return;
        }
        if let Some(w) = &self.win {
            w.window.set_visible(true);
            w.window.set_minimized(false);
            w.window.focus_window();
        }
        self.hidden = false;
        self.redraw();
    }

    /// Runs a Windows Integration action from the menu and shows the result.
    #[cfg(windows)]
    fn integration(&mut self, action: Integration) {
        let text = match integrate(action) {
            Ok(t) | Err(t) => t,
        };
        self.integrated = integrated_here();
        self.menu_dirty = true;
        self.info(&tr("Windows Integration"), &text);
    }

    fn info(&mut self, title: &str, text: &str) {
        self.dialog = Some(Message::new(title, text));
        self.drag = None;
        self.redraw();
    }

    fn checked(&self, c: Command) -> bool {
        match c {
            Command::WordWrap => self.settings.wrap,
            Command::DarkPages => self.settings.dark_pages,
            #[cfg(windows)]
            Command::ExplorerMenu => self.integrated,
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
            Command::Copy | Command::CopyPlain => self.viewer.has_selection(),
            Command::SelectAll => ready,
            Command::ZoomIn | Command::ZoomOut | Command::ZoomReset => ready,
            Command::FitWidth => ready && self.viewer.kind == doc::Kind::Pdf,
            Command::WordWrap => self.viewer.kind == doc::Kind::Text || self.viewer.path.is_none(),
            Command::DarkPages => self.dark(),
            _ => true,
        }
    }

    // ------------------------------------------------------------ input

    // ------------------------------------------------------------ frame

}

fn shortcuts_text() -> String {
    let rows = [
        ("Ctrl+O", tr("Open a document")),
        ("F5", tr("Reload")),
        ("Ctrl+W", tr("Close rview")),
        ("Drag", tr("Select text; in CSV, cells")),
        ("Double / triple click", tr("Select a word / a line or paragraph")),
        ("Shift+Click", tr("Extend the selection")),
        ("Alt+Drag", tr("Select a rectangle of a PDF page")),
        ("Ctrl+A", tr("Select everything")),
        ("Ctrl+C", tr("Copy with formats")),
        ("Ctrl+Shift+C", tr("Copy as plain text")),
        ("Esc", tr("Clear the selection")),
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
        ("Middle drag / Ctrl+Drag", tr("Pan the document")),
    ];
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
}

/// Starts resident mode: without an icon, a hidden window could not be
/// reached or closed, so rview then runs as a normal window.
#[cfg(not(windows))]
fn start_resident(shell: &mut Shell) {
    let proxy = Mutex::new(shell.proxy.clone());
    let send = Box::new(move |m| {
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(UserEvent::Shell(m));
        }
    });
    shell.shell = rt_ui::resident::start(linux::shell::APP, ui::appicon::render, tr("Open rview"), tr("Exit"), send);
    shell.tray = shell.shell.is_some();
}

impl Shell {
    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let (w, h, maximized) = self.settings.window.unwrap_or((1000.0, 760.0, false));
        let attrs = Window::default_attributes()
            .with_title("rview")
            .with_inner_size(LogicalSize::new(w.max(360.0), h.max(240.0)))
            .with_min_inner_size(LogicalSize::new(360.0, 240.0))
            .with_maximized(maximized)
            .with_window_icon(rt_ui::window::icon(ui::appicon::render))
            .with_theme(Some(rt_ui::window::theme(self.dark())))
            .with_visible(false);
        let Some((window, surface)) = rt_ui::window::create(el, attrs, "rview") else { return false };
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(el, &window, self.proxy.clone());
        #[cfg(windows)]
        let hwnd = rt_ui::window::hwnd_of(&window);
        #[cfg(windows)]
        {
            let proxy = self.proxy.clone();
            win::subclass(
                hwnd,
                &rt_ui::window::mnemonics_of(&commands::menu_tree()),
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
        true
    }
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        #[cfg(not(windows))]
        if self.tray {
            start_resident(self);
        }
        // Linux has no hidden window: a resident start makes it when first shown.
        let no_window = cfg!(not(windows)) && self.tray && self.args.is_empty();
        if !no_window && !self.open_window(el) {
            return;
        }
        self.sync_menu();
        #[cfg(not(windows))]
        if !linux::has_dialogs() {
            self.status_note = Some(tr("Install zenity or kdialog to use the Open dialog."));
        }
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
                    if !self.viewer.pdf_text_pending() && self.status_note.as_deref() == Some(tr("Reading the text of this PDF…").as_str()) {
                        self.status_note = None;
                    }
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
            #[cfg(not(windows))]
            UserEvent::Shell(linux::shell::Msg::Open(paths)) => {
                self.show(el);
                if let Some(p) = paths.first() {
                    self.open(p, false, None);
                }
            }
            #[cfg(not(windows))]
            UserEvent::Shell(linux::shell::Msg::Quit) => self.quit(el),
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
                self.show(el);
                if let Some(p) = paths.first() {
                    self.open(p, false, None);
                }
            }
            #[cfg(windows)]
            UserEvent::Win(win::WinMsg::Tray(win::tray::TrayEvent::Activate)) => self.show(el),
            #[cfg(windows)]
            UserEvent::Win(win::WinMsg::Tray(win::tray::TrayEvent::Menu)) => {
                let hwnd = self.win.as_ref().map(|w| w.hwnd);
                if let Some(hwnd) = hwnd {
                    match win::tray::menu(hwnd, &tr("Open rview"), &tr("Exit")) {
                        Some(1) => self.show(el),
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
            WindowEvent::ModifiersChanged(m) => {
                self.mods = m.state();
                self.update_hover();
            }
            WindowEvent::DroppedFile(p) => self.open(&p, false, None),
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if event.state == ElementState::Pressed {
                    self.key(el, &event);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
                self.cursor = Some((x, y));
                #[cfg(not(windows))]
                self.menu_pointer(el, x, y);
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
                    // Moving far turns a link press into a selection.
                    Some(Drag::Link { x: x0, y: y0, .. }) if (x - *x0).abs() + (y - *y0).abs() > 6.0 => {
                        let (x0, y0) = (*x0, *y0);
                        self.viewer.select_press(&mut self.gfx, x0, y0, false, false, 1);
                        self.viewer.select_drag(&mut self.gfx, x, y);
                        self.drag = Some(Drag::Select);
                        self.redraw();
                    }
                    Some(Drag::Select) => {
                        self.viewer.select_drag(&mut self.gfx, x, y);
                        self.redraw();
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
                        if d.closes_at(x, y) {
                            self.dialog = None;
                            self.redraw();
                        }
                        return;
                    }
                    #[cfg(not(windows))]
                    if self.menu_press(el, x, y) {
                        return;
                    }
                    if let Some(axis) = self.viewer.scrollbar_at(x, y) {
                        self.viewer.drag_scrollbar(axis, x, y);
                        self.drag = Some(Drag::Bar(axis));
                    } else if self.mods.control_key() && self.viewer.in_view(x, y) {
                        self.start_pan(x, y);
                    } else if let Some(href) = self.viewer.link_at(x, y).filter(|_| !self.mods.shift_key()) {
                        self.drag = Some(Drag::Link { href, x, y });
                    } else if self.viewer.in_view(x, y) {
                        let clicks = self.count_click(x, y);
                        self.viewer.select_press(&mut self.gfx, x, y, self.mods.shift_key(), self.mods.alt_key(), clicks);
                        self.drag = Some(Drag::Select);
                        self.status_note = self.viewer.pdf_text_pending().then(|| tr("Reading the text of this PDF…"));
                        self.menu_dirty = true;
                    }
                    self.update_hover();
                    self.redraw();
                }
                ElementState::Released => {
                    match self.drag.take() {
                        Some(Drag::Link { href, .. }) => self.follow(&href),
                        Some(Drag::Select) => {
                            self.menu_dirty = true;
                            self.redraw();
                        }
                        _ => {}
                    }
                    self.update_hover();
                }
            },
            WindowEvent::MouseInput { state, button: MouseButton::Middle, .. } if self.dialog.is_none() => match state {
                ElementState::Pressed => {
                    if let Some((x, y)) = self.cursor.filter(|(x, y)| self.viewer.in_view(*x, *y) && self.viewer.scrollbar_at(*x, *y).is_none()) {
                        self.start_pan(x, y);
                        self.update_hover();
                    }
                }
                ElementState::Released => {
                    self.drag = None;
                    self.update_hover();
                }
            },
            #[cfg(not(windows))]
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } if self.menu.is_open() => {
                self.menu.close();
                self.redraw();
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Right, .. } if self.dialog.is_none() => {
                if self.cursor.is_some_and(|(x, y)| self.viewer.in_view(x, y)) && matches!(self.viewer.state, view::State::Ready(_)) {
                    self.context_menu(el);
                }
            }
            WindowEvent::MouseWheel { .. } if self.dialog.is_some() => {}
            #[cfg(not(windows))]
            WindowEvent::MouseWheel { delta, .. } if self.menu.is_open() => {
                let down = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y < 0.0,
                    MouseScrollDelta::PixelDelta(p) => p.y < 0.0,
                };
                let step = rt_ui::menus::wheel(&mut self.menu, self.cursor, down);
                self.menu_step(el, step);
            }
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
        // A selection dragged past the edge keeps scrolling while the
        // pointer rests there.
        if let (Some(Drag::Select), Some((x, y))) = (&self.drag, self.cursor) {
            if self.viewer.autoscroll(x, y) {
                self.viewer.select_drag(&mut self.gfx, x, y);
                self.redraw();
                el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + AUTOSCROLL_STEP));
                return;
            }
        }
        el.set_control_flow(ControlFlow::Wait);
    }
}

#[cfg(windows)]
#[derive(Clone, Copy)]
enum Integration {
    Register,
    Associate,
    Unregister,
}

#[cfg(windows)]
fn integrated_here() -> bool {
    let exe = rt_sys::win::assoc::current_exe().unwrap_or_default();
    win::assoc::status(&exe) == win::assoc::Status::Here
}

/// Adds or removes rview in Explorer (current user only); returns the
/// message to show, or an error message.
#[cfg(windows)]
fn integrate(action: Integration) -> Result<String, String> {
    use win::assoc;
    let exe = rt_sys::win::assoc::current_exe()?;
    let failed = |e: assoc::Error| tf("Cannot change the Windows registry (error {}).", &[&e.to_string()]);
    let verb = tr("Open with rview");
    match action {
        Integration::Register => {
            assoc::register(&exe, &verb).map_err(failed)?;
            Ok(tf("rview was added to the Explorer context menu and to the “Open with” list for: {}", &[&assoc::list(&assoc::extensions())]))
        }
        Integration::Unregister => {
            assoc::unregister().map_err(failed)?;
            Ok(tr("rview was removed from Explorer and from the file associations."))
        }
        Integration::Associate => {
            let d = assoc::associate(&exe, &verb).map_err(failed)?;
            let mut parts = Vec::new();
            if !d.set.is_empty() {
                parts.push(tf("rview is now the default viewer for: {}", &[&assoc::list(&d.set)]));
            }
            if !d.already.is_empty() {
                parts.push(tf("rview was already the default viewer for: {}", &[&assoc::list(&d.already)]));
            }
            if !d.user_choice.is_empty() {
                assoc::open_default_apps();
                parts.push(tf(
                    "Windows asks you to choose the default app yourself for: {}\nIn the Settings window that opened, use “Choose default apps by file type” and pick rview.",
                    &[&assoc::list(&d.user_choice)],
                ));
            }
            Ok(parts.join("\n\n"))
        }
    }
}

#[cfg(not(windows))]
const USAGE: &str = "rview [--tray] [--new-window] [file]

  Shows a PDF, Markdown, HTML, text or CSV document.

  --tray        Stay resident in the notification area: closing the window
                hides it, and launching rview again (with or without a
                file) or clicking the tray icon shows it instantly. Exit
                from the tray icon's menu or File > Exit. Ignored on desktops
                without a notification area.
  --new-window  Open a separate window even when rview is resident.

Settings are stored in ~/.config/rview, or next to the program when a file
named rview.portable exists there (portable mode).";

#[cfg(windows)]
const USAGE: &str = "rview [--tray] [--new-window] [file]
       rview --register | --associate | --unregister

  Shows a PDF, Markdown, HTML, text or CSV document.

  --tray        Stay resident in the notification area: closing the window
                hides it, and launching rview again (with or without a
                file) or clicking the tray icon shows it instantly. Exit
                from the tray icon's menu or File > Exit.
  --new-window  Open a separate window even when rview is resident.

  --register    Add \"Open with rview\" to the Explorer context menu of
                PDF, Markdown, HTML, text and CSV files, and rview to their
                \"Open with\" list.
  --associate   Also make rview the default viewer for those types where
                Windows allows it; for types whose default you chose
                yourself, Windows Settings > Default apps opens.
  --unregister  Remove everything --register and --associate added and
                restore the previous defaults.
  These change only the current user's settings (no administrator rights).

Settings are stored in %APPDATA%\\rview, or next to rview.exe when a file
named rview.portable exists there (portable mode).";

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // The help goes to the console it was asked from; without one, rview
    // starts and shows it in its own window, in its theme.
    let help = raw.iter().any(|a| a == "--help" || a == "-h" || a == "/?");
    #[cfg(windows)]
    if help && win::print_to_console(USAGE) {
        return;
    }
    #[cfg(not(windows))]
    if help {
        println!("{USAGE}");
        return;
    }
    #[cfg(windows)]
    {
        let flag = |f: &str| raw.iter().any(|a| a == f);
        let action = if flag("--unregister") {
            Some(Integration::Unregister)
        } else if flag("--associate") {
            Some(Integration::Associate)
        } else if flag("--register") {
            Some(Integration::Register)
        } else {
            None
        };
        if let Some(action) = action {
            let mut location = storage::detect();
            let settings = Settings::load(&location.load());
            i18n::set_lang(settings.language.unwrap_or_else(i18n::system_lang));
            let result = integrate(action);
            let ok = result.is_ok();
            let text = match result {
                Ok(t) | Err(t) => t,
            };
            win::report(&tr("Windows Integration"), &text);
            std::process::exit(if ok { 0 } else { 1 });
        }
    }
    let args: Vec<PathBuf> = raw.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    let tray = raw.iter().any(|a| a == "--tray" || a == "/tray");
    let new_window = raw.iter().any(|a| a == "--new-window");
    // A resident instance shows itself (and the file) instantly.
    #[cfg(windows)]
    if !new_window && win::hand_off_to_resident(&args) {
        return;
    }
    #[cfg(not(windows))]
    if !new_window && linux::shell::hand_off_to_resident(&args) {
        return;
    }
    let _ = new_window;
    let mut location = storage::detect();
    let store = location.load();
    let settings = Settings::load(&store);
    let gfx = match Gfx::new() {
        Ok(mut g) => {
            g.set_mono_names(ui::gfx::RVIEW_MONO);
            g
        }
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
    let system_dark = linux::system_dark().unwrap_or(false);
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
        tray: tray && !help,
        hidden: false,
        dialog: help.then(|| Message::new("rview", USAGE)),
        #[cfg(windows)]
        integrated: integrated_here(),
        last_click: None,
        #[cfg(not(windows))]
        menu: ui::menu::Menu::default(),
        #[cfg(not(windows))]
        shell: None,
    };
    shell.apply_language();
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rview: {e}");
    }
}
