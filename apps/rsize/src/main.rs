//! rsize: a portable disk space analyzer.
//!
//! The window is driven by winit and drawn on the CPU (tiny-skia + ab_glyph)
//! into a softbuffer surface. Scans run on background threads and report to
//! the window through the event loop. The page itself (state, scanning, the
//! treemap and charts, drawing, keyboard and mouse) is `rsize::page::SizePage`,
//! shared with rcare; this file is the window around it.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rsize::commands::{self, Command};
use rsize::i18n::{self, tf, tr};
use rsize::page::{shortcuts_text, Popup, SizePage};
use rsize::settings::Settings;
use rsize::storage;
use rsize::ui::gfx::{Color, Gfx, Rect};
use rsize::ui::theme::Theme;
use rt_ui::page::SystemDialogs;
use rt_ui::Message;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
#[cfg(not(windows))]
use winit::event::MouseScrollDelta;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, Theme as WinTheme, Window, WindowId};

mod input;
mod menus;
mod render;

enum UserEvent {
    /// A step of Check for Updates.
    Update(rt_ui::update::Step),
    A11y(accesskit_winit::Event),
    /// What a thread of the page reports.
    Page(rsize::page::Event),
    #[cfg(windows)]
    Win(rt_sys::win::WinMsg),
    /// A later launch, or the notification-area icon (Linux).
    #[cfg(not(windows))]
    Shell(rt_sys::linux::shell::Msg),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(e: accesskit_winit::Event) -> Self {
        UserEvent::A11y(e)
    }
}

#[cfg(windows)]
const IDENT: rt_sys::win::Ident = rt_sys::win::Ident { name: "rsize", copydata: 0x5253_495A }; // "RSIZ"

struct Win {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    adapter: accesskit_winit::Adapter,
    #[cfg(windows)]
    hwnd: rt_sys::win::HWND,
    #[cfg(windows)]
    menu: Option<rt_sys::win::menu::MenuBar<Command>>,
}

struct Shell {
    /// The page; its `settings` also hold the theme, the language and the window's size.
    page: SizePage,
    location: storage::Location,
    gfx: Gfx,
    win: Option<Win>,
    proxy: EventLoopProxy<UserEvent>,
    mods: ModifiersState,
    cursor: Option<(f32, f32)>,
    system_dark: bool,
    accent: Option<Color>,
    menu_dark: Option<bool>,
    menu_dirty: bool,
    menu_rebuild: bool,
    cursor_icon: CursorIcon,
    /// An in-app message (About, Keyboard shortcuts, the help).
    dialog: Option<Message>,
    args: Vec<PathBuf>,
    #[cfg(not(windows))]
    menu: rsize::ui::menu::Menu,
    /// Resident mode (`--tray`): closing the window hides it to the
    /// notification area.
    tray: bool,
    hidden: bool,
    /// The window was last maximized: done when it is first shown.
    maximize_on_show: bool,
    /// The notification-area icon and the socket for later launches (Linux).
    #[cfg(not(windows))]
    resident: Option<rt_sys::linux::shell::Shell>,
    /// "Analyze with rsize" is on the Explorer menu of folders, for this rsize.exe.
    #[cfg(windows)]
    explorer_menu: bool,
}

impl Shell {
    fn scale(&self) -> f32 {
        self.win.as_ref().map_or(1.0, |w| w.window.scale_factor() as f32)
    }

    fn dark(&self) -> bool {
        match self.page.settings.theme {
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
        i18n::set_lang(self.page.settings.language.unwrap_or_else(i18n::system_lang));
        self.gfx.clear_cache();
        self.menu_rebuild = true;
        self.menu_dirty = true;
    }

    fn save_settings(&mut self) {
        if let Some(w) = &self.win {
            rt_ui::window::remember_size(&w.window, &mut self.page.settings.window);
        }
        let store = self.page.settings.to_store();
        self.location.save(&store);
    }

    /// The system's dialogs, over this window.
    fn dialogs(&self) -> SystemDialogs {
        SystemDialogs {
            #[cfg(windows)]
            hwnd: self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd),
        }
    }

    /// Takes what the page asked of the window, and its pointer's shape.
    fn after_page(&mut self) {
        let fx = self.page.fx.take();
        if fx.menu_dirty {
            self.menu_dirty = true;
        }
        if fx.save {
            self.save_settings();
        }
        if let Some((title, text)) = fx.info {
            self.dialog = Some(Message::new(&title, &text));
        }
        if fx.redraw {
            self.redraw();
        }
        let icon = self.page.cursor_icon;
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(w) = &self.win {
                w.window.set_cursor(icon);
            }
        }
    }

    fn update_title(&self) {
        let Some(w) = &self.win else { return };
        let title = match self.page.title() {
            Some(t) => format!("{t} - rsize"),
            None => "rsize".to_string(),
        };
        if w.window.title() != title {
            w.window.set_title(&title);
        }
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self, el: &ActiveEventLoop, cmd: Command) {
        if !self.enabled(cmd) {
            return;
        }
        let mut dialogs = self.dialogs();
        if self.page.command(cmd, &mut dialogs) {
            self.after_page();
            return;
        }
        match cmd {
            #[cfg(windows)]
            Command::RestartAdmin => {
                if rt_sys::win::restart_elevated(&self.page.restart_args()) {
                    self.quit(el);
                }
            }
            #[cfg(windows)]
            Command::ExplorerMenu => self.integration(!self.explorer_menu),
            Command::Exit => self.quit(el),
            Command::Autostart => {
                if let Err(e) = rt_sys::autostart::set("rsize", !rt_sys::autostart::enabled("rsize")) {
                    self.info("rsize", &tf("Cannot change how it starts: {}", &[&e]));
                }
            }
            Command::Theme(t) => {
                self.page.settings.theme = t;
                self.apply_window_theme();
            }
            Command::UiLanguage(i) => {
                self.page.settings.language = (i as usize).checked_sub(1).and_then(|k| i18n::Lang::ALL.get(k)).copied();
                self.apply_language();
            }
            Command::Shortcuts => self.info(&tr("Keyboard shortcuts"), &shortcuts_text()),
            Command::CheckUpdates => rt_ui::update::check("rsize", env!("CARGO_PKG_VERSION"), self.update_sender()),
            Command::About => {
                let text = tf(
                    "rsize {}\n\nShows what takes up space on a drive or in a folder, written in Rust.\nNo telemetry, no network access.\n\nMIT License.",
                    &[env!("CARGO_PKG_VERSION")],
                );
                self.info(&tr("About rsize"), &text);
            }
            _ => {}
        }
        self.menu_dirty = true;
        self.redraw();
    }

    fn checked(&self, c: Command) -> bool {
        match c {
            Command::Theme(t) => self.page.settings.theme == t,
            Command::UiLanguage(0) => self.page.settings.language.is_none(),
            Command::UiLanguage(i) => self.page.settings.language == i18n::Lang::ALL.get(i as usize - 1).copied(),
            #[cfg(windows)]
            Command::ExplorerMenu => self.explorer_menu,
            Command::Autostart => rt_sys::autostart::enabled("rsize"),
            c => self.page.checked(c),
        }
    }

    fn enabled(&self, c: Command) -> bool {
        self.page.enabled(c)
    }

    fn info(&mut self, title: &str, text: &str) {
        self.dialog = Some(Message::new(title, text));
        self.redraw();
    }

    /// Where Check for Updates sends its steps.
    fn update_sender(&self) -> Box<dyn Fn(rt_ui::update::Step) + Send> {
        let p = std::sync::Mutex::new(self.proxy.clone());
        Box::new(move |s| {
            if let Ok(p) = p.lock() {
                let _ = p.send_event(UserEvent::Update(s));
            }
        })
    }

    fn quit(&mut self, el: &ActiveEventLoop) {
        if !self.page.may_close(&mut self.dialogs()) {
            return;
        }
        self.page.cancel();
        self.save_settings();
        #[cfg(windows)]
        if let Some(w) = &self.win {
            rt_sys::win::tray::remove(w.hwnd);
        }
        #[cfg(not(windows))]
        {
            self.resident = None;
        }
        el.exit();
    }

    /// The close button: hides to the notification area in resident mode.
    fn close(&mut self, el: &ActiveEventLoop) {
        if self.tray {
            self.hide();
        } else {
            self.quit(el);
        }
    }

    /// Hides the window to the notification area; a scan goes on.
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
        // Release the frame buffer and the drawings while hidden.
        self.gfx.resize(1, 1);
        self.gfx.trim_cache();
        self.page.release_drawings();
    }

    fn show(&mut self, el: &ActiveEventLoop) {
        if self.win.is_none() && !self.open_window(el) {
            return;
        }
        if let Some(w) = &self.win {
            if std::mem::take(&mut self.maximize_on_show) {
                w.window.set_maximized(true);
            }
            w.window.set_visible(true);
            w.window.set_minimized(false);
            w.window.focus_window();
        }
        self.hidden = false;
        self.menu_dirty = true;
        self.redraw();
    }

    /// Folders from a later launch (or the Explorer menu): the window shows,
    /// and reads the first one.
    fn open_paths(&mut self, el: &ActiveEventLoop, paths: Vec<PathBuf>) {
        self.show(el);
        self.page.open_paths(&paths);
        self.after_page();
    }

    /// Adds or removes "Analyze with rsize" in Explorer and says how it went.
    #[cfg(windows)]
    fn integration(&mut self, on: bool) {
        let text = match integrate(on) {
            Ok(t) | Err(t) => t,
        };
        self.explorer_menu = integrated_here();
        self.menu_dirty = true;
        self.info(&tr("Windows Integration"), &text);
    }

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let (w, h, maximized) = self.page.settings.window.unwrap_or((1100.0, 780.0, false));
        let attrs = Window::default_attributes()
            .with_title("rsize")
            .with_inner_size(LogicalSize::new(w.max(480.0), h.max(360.0)))
            .with_min_inner_size(LogicalSize::new(480.0, 360.0))
            // Maximizing shows the window on Windows: a resident start, which
            // stays hidden, maximizes when it is first shown.
            .with_maximized(maximized && !self.tray)
            .with_window_icon(rt_ui::window::icon(rsize::ui::appicon::render))
            .with_theme(Some(rt_ui::window::theme(self.dark())))
            .with_visible(false);
        let Some((window, surface)) = rt_ui::window::create(el, attrs, "rsize") else { return false };
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(el, &window, self.proxy.clone());
        #[cfg(windows)]
        let hwnd = rt_ui::window::hwnd_of(&window);
        #[cfg(windows)]
        {
            let proxy = self.proxy.clone();
            rt_sys::win::subclass(
                hwnd,
                &IDENT,
                &rt_ui::window::mnemonics_of(&commands::menu_tree()),
                Box::new(move |m| {
                    let _ = proxy.send_event(UserEvent::Win(m));
                }),
            );
            if self.tray {
                rt_sys::win::set_resident(hwnd, &IDENT);
                rt_sys::win::tray::show(hwnd, "rsize");
            }
        }
        self.maximize_on_show = maximized && self.tray;
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

    /// A menu at the pointer, asked for by the page.
    fn popup(&mut self, el: &ActiveEventLoop, which: Popup) {
        let items = match which {
            Popup::Item => commands::item_menu(),
        };
        #[cfg(windows)]
        {
            let Some(hwnd) = self.win.as_ref().map(|w| w.hwnd) else { return };
            let items: Vec<_> = items
                .into_iter()
                .map(|m| match m {
                    rt_core::menu::M::Item(c) => Some((c, self.enabled(c))),
                    _ => None,
                })
                .collect();
            if let Some(c) = rt_sys::win::menu::popup_at_cursor(hwnd, &items) {
                self.command(el, c);
            }
        }
        #[cfg(not(windows))]
        {
            let _ = el;
            let me = &*self;
            let built = rsize::ui::menu::build(items, &|c| me.checked(c), &|c| me.enabled(c));
            let at = self.cursor.unwrap_or((0.0, 0.0));
            self.menu.open(None, built, at, false);
            self.redraw();
        }
    }
}

/// Starts resident mode on Linux: without an icon, a hidden window could not
/// be reached or closed, so rsize then runs as a normal window.
#[cfg(not(windows))]
fn start_resident(shell: &mut Shell) {
    let proxy = std::sync::Mutex::new(shell.proxy.clone());
    let send = Box::new(move |m| {
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(UserEvent::Shell(m));
        }
    });
    shell.resident = rt_ui::resident::start("rsize", rsize::ui::appicon::render, tr("Open rsize"), tr("Exit"), send);
    shell.tray = shell.resident.is_some();
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        #[cfg(not(windows))]
        if self.tray && self.resident.is_none() {
            start_resident(self);
        }
        // Linux has no hidden window: a resident start makes it when first shown.
        let no_window = cfg!(not(windows)) && self.tray && self.args.is_empty();
        if !no_window && !self.open_window(el) {
            return;
        }
        self.sync_menu();
        if let Some(p) = self.args.first().cloned() {
            self.page.start_scan(&p);
            self.after_page();
        }
        // Resident mode starts in the notification area unless a folder was given.
        if self.tray && self.args.is_empty() {
            self.hidden = true;
            return;
        }
        if let Some(w) = &self.win {
            if std::mem::take(&mut self.maximize_on_show) {
                w.window.set_maximized(true);
            }
            w.window.set_visible(true);
            w.window.focus_window();
        }
        self.redraw();
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Page(e) => {
                self.page.event(e);
                self.after_page();
            }
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::Command(id)) => {
                let cmd = self.win.as_ref().and_then(|w| w.menu.as_ref()).and_then(|m| m.command(id));
                if let Some(c) = cmd {
                    self.command(el, c);
                }
            }
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::SystemTheme) => {
                let dark = rt_sys::system_dark().unwrap_or(false);
                self.accent = rt_sys::accent().map(Color::rgb);
                if dark != self.system_dark {
                    self.system_dark = dark;
                    self.apply_window_theme();
                }
                self.redraw();
            }
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::OpenFiles(paths)) => self.open_paths(el, paths),
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::Tray(rt_sys::win::tray::TrayEvent::Activate)) => self.show(el),
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::Tray(rt_sys::win::tray::TrayEvent::Menu)) => {
                if let Some(hwnd) = self.win.as_ref().map(|w| w.hwnd) {
                    match rt_sys::win::tray::menu(hwnd, &tr("Open rsize"), &tr("Exit")) {
                        Some(1) => self.show(el),
                        Some(2) => self.quit(el),
                        _ => {}
                    }
                }
            }
            #[cfg(not(windows))]
            UserEvent::Shell(rt_sys::linux::shell::Msg::Open(paths)) => self.open_paths(el, paths),
            #[cfg(not(windows))]
            UserEvent::Shell(rt_sys::linux::shell::Msg::Quit) => self.quit(el),
            UserEvent::Update(s) => {
                let who = rt_ui::update::Who {
                    app: "rsize",
                    version: env!("CARGO_PKG_VERSION"),
                    #[cfg(windows)]
                    hwnd: self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd),
                };
                if rt_ui::update::step(&who, s, self.update_sender()) == rt_ui::update::Next::Quit {
                    self.quit(el);
                }
            }
            UserEvent::A11y(ev) => match ev.window_event {
                accesskit_winit::WindowEvent::ActionRequested(req) if req.action == accesskit::Action::Click => {
                    let id = req.target_node.0;
                    if id == 6 {
                        self.dialog = None;
                    } else if let Some(i) = id.checked_sub(100) {
                        self.page.accessibility_click(i as usize);
                    }
                    let _ = el;
                    self.after_page();
                    self.redraw();
                }
                accesskit_winit::WindowEvent::InitialTreeRequested => {
                    let tree = self.tree();
                    if let Some(w) = &mut self.win {
                        w.adapter.update_if_active(|| tree);
                    }
                }
                _ => {}
            },
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(w) = &mut self.win {
            w.adapter.process_event(&w.window, &event);
        }
        match event {
            WindowEvent::CloseRequested => self.close(el),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.page.resized();
                self.redraw();
            }
            WindowEvent::ThemeChanged(t) => {
                self.system_dark = t == WinTheme::Dark;
                self.redraw();
            }
            WindowEvent::Focused(false) => self.save_settings(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::DroppedFile(p) => {
                self.page.drop_file(p);
                self.after_page();
            }
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if event.state == ElementState::Pressed {
                    self.key(el, &event);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
                self.cursor = Some((x, y));
                #[cfg(not(windows))]
                if self.menu_pointer(el, x, y) {
                    return;
                }
                self.page.drag(x, y);
                self.page.pointer(x, y, self.dialog.is_some());
                self.after_page();
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.page.pointer_left();
                self.after_page();
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => {
                    if let Some((x, y)) = self.cursor {
                        self.press(el, x, y);
                    }
                }
                ElementState::Released => {
                    self.page.release();
                    self.after_page();
                }
            },
            #[cfg(not(windows))]
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } if self.menu.is_open() => {
                self.menu.close();
                self.redraw();
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Right, .. } if self.dialog.is_none() => {
                let popup = self.page.right_click(self.cursor, self.mods.into());
                self.after_page();
                if let Some(p) = popup {
                    self.popup(el, p);
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.dialog.is_none() => {
                #[cfg(not(windows))]
                {
                    let down = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y < 0.0,
                        MouseScrollDelta::PixelDelta(p) => p.y < 0.0,
                    };
                    let step = rt_ui::menus::wheel(&mut self.menu, self.cursor, down);
                    if self.menu_step(el, step) {
                        return;
                    }
                }
                self.page.wheel(delta, self.cursor);
                self.after_page();
            }
            WindowEvent::RedrawRequested if !self.hidden => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let wake = self.page.animate();
        self.after_page();
        match wake {
            Some(at) => el.set_control_flow(ControlFlow::WaitUntil(at)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }
}

/// Whether "Analyze with rsize" is on the Explorer menu of folders, for this rsize.exe.
#[cfg(windows)]
fn integrated_here() -> bool {
    rt_ui::explorer::folders_here("rsize")
}

/// Adds "Analyze with rsize" to the Explorer menu of folders and drives
/// (current user only), or takes it out; returns the message to show.
#[cfg(windows)]
fn integrate(on: bool) -> Result<String, String> {
    let texts = rt_ui::explorer::FolderTexts {
        verb: tr("Analyze with rsize"),
        added: tr("“Analyze with rsize” was added to the Explorer context menu of folders and drives."),
        removed: tr("“Analyze with rsize” was removed from the Explorer context menu."),
        failed: tr("Cannot change the Windows registry (error {})."),
    };
    rt_ui::explorer::set_folders("rsize", on, &texts)
}

const USAGE: &str = "rsize [--tray] [--new-window] [folder]

  Shows what takes up space on a drive or in a folder: a list of the folders
  with their sizes, largest first, and a treemap where each file is a
  rectangle as large as the file.

  folder        Start reading this folder (or drive, or network share) at
                once. Without it, rsize shows the drives to choose from.
  --tray        Stay resident in the notification area: closing the window
                hides it, and launching rsize again (with or without a
                folder) or clicking the tray icon shows it instantly. Exit
                from the tray icon's menu or File > Exit.
  --new-window  Open a separate window even when rsize is resident.

Windows only:
  --register    Add \"Analyze with rsize\" to the Explorer context menu of
                folders and drives (current user only).
  --unregister  Remove it.
  --timings     (as administrator) Read the folder's NTFS table twice,
                without and through the system cache, and report how long
                each part took. Nothing is shown but the report.

Settings are stored in the user's settings folder (%APPDATA%\\rsize on
Windows, ~/.config/rsize on Linux), or next to the program when a file named
rsize.portable exists there (portable mode).";

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // What an update left behind.
    rt_sys::update::clean_old();
    // The help goes to the console it was asked from; without one, rsize
    // starts and shows it in its own window, in its theme.
    let help = raw.iter().any(|a| a == "--help" || a == "-h" || a == "/?");
    #[cfg(windows)]
    if help && rt_sys::win::print_to_console(USAGE) {
        return;
    }
    #[cfg(not(windows))]
    if help {
        println!("{USAGE}");
        return;
    }
    #[cfg(windows)]
    if raw.iter().any(|a| a == "--register" || a == "--unregister") {
        let mut location = storage::detect();
        let settings = Settings::load(&location.load());
        i18n::set_lang(settings.language.unwrap_or_else(i18n::system_lang));
        let result = integrate(raw.iter().any(|a| a == "--register"));
        let ok = result.is_ok();
        let text = match result {
            Ok(t) | Err(t) => t,
        };
        rt_sys::win::report(&tr("Windows Integration"), &text);
        std::process::exit(if ok { 0 } else { 1 });
    }
    #[cfg(windows)]
    if raw.iter().any(|a| a == "--timings") {
        let dir = raw.iter().find(|a| !a.starts_with("--")).map_or_else(|| PathBuf::from("C:\\"), PathBuf::from);
        rt_sys::win::report("rsize", &rsize::scan::ntfs::timings(&rsize::scan::normalize(&dir)));
        return;
    }
    #[cfg(windows)]
    // SAFETY: only changes how this process reports a missing disc or card:
    // with an error code instead of a dialog.
    unsafe {
        use windows_sys::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS};
        SetErrorMode(SEM_FAILCRITICALERRORS);
    }
    // A drive from the Explorer menu comes as `C:"` (see `folder_arg`).
    #[cfg(windows)]
    let args: Vec<PathBuf> =
        raw.iter().filter(|a| !a.starts_with("--")).map(|a| PathBuf::from(rt_sys::win::assoc::folder_arg(a))).collect();
    #[cfg(not(windows))]
    let args: Vec<PathBuf> = raw.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
    let tray = raw.iter().any(|a| a == "--tray" || a == "/tray") && !help;
    let new_window = raw.iter().any(|a| a == "--new-window") || help;
    // A resident rsize shows itself (and reads the folder) instantly.
    #[cfg(windows)]
    if !new_window && rt_sys::win::hand_off(&IDENT, &args, false, false) {
        return;
    }
    #[cfg(not(windows))]
    if !new_window && rt_sys::linux::shell::hand_off("rsize", &args, false) {
        return;
    }
    let mut location = storage::detect();
    let store = location.load();
    let settings = Settings::load(&store);
    let gfx = match Gfx::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rsize: {e}");
            std::process::exit(1);
        }
    };
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("rsize: cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    let sender = {
        let proxy = std::sync::Mutex::new(event_loop.create_proxy());
        Arc::new(move |e| {
            if let Ok(p) = proxy.lock() {
                let _ = p.send_event(UserEvent::Page(e));
            }
        })
    };
    #[cfg(windows)]
    let elevated = rt_sys::win::elevated();
    #[cfg(not(windows))]
    let elevated = false;
    let mut shell = Shell {
        page: SizePage::new(settings, elevated, sender),
        location,
        gfx,
        win: None,
        proxy: event_loop.create_proxy(),
        mods: ModifiersState::empty(),
        cursor: None,
        system_dark: rt_sys::system_dark().unwrap_or(false),
        accent: rt_sys::accent().map(Color::rgb),
        menu_dark: None,
        menu_dirty: true,
        menu_rebuild: true,
        cursor_icon: CursorIcon::Default,
        dialog: help.then(|| Message::new("rsize", USAGE)),
        args,
        #[cfg(not(windows))]
        menu: rsize::ui::menu::Menu::default(),
        tray,
        hidden: false,
        maximize_on_show: false,
        #[cfg(not(windows))]
        resident: None,
        #[cfg(windows)]
        explorer_menu: integrated_here(),
    };
    shell.apply_language();
    shell.page.load_volumes();
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rsize: {e}");
    }
}
