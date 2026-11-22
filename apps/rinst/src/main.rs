//! rinst: a portable uninstaller.
//!
//! The window is driven by winit and drawn on the CPU (tiny-skia + ab_glyph)
//! into a softbuffer surface. The page itself (the programs read on threads,
//! their icons, uninstalling, the leftovers, drawing, keyboard and mouse) is
//! `rinst::page::InstPage`, shared with rcare; this file is the window around it.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rinst::commands::{self, Command};
use rinst::i18n::{self, tf, tr};
use rinst::page::{shortcuts_text, InstPage, Popup};
use rinst::settings::Settings;
use rinst::storage;
use rinst::ui::gfx::{Color, Gfx, Rect};
use rinst::ui::theme::Theme;
use rt_ui::page::SystemDialogs;
use rt_ui::Message;
use std::num::NonZeroU32;
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
    Page(rinst::page::Event),
    #[cfg(windows)]
    Win(rt_sys::win::WinMsg),
    /// The tray icon and other launches (Linux).
    #[cfg(not(windows))]
    Shell(rt_sys::linux::shell::Msg),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(e: accesskit_winit::Event) -> Self {
        UserEvent::A11y(e)
    }
}

#[cfg(windows)]
const IDENT: rt_sys::win::Ident = rt_sys::win::Ident { name: "rinst", copydata: 0x524E_494E }; // "RNIN"

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
    page: InstPage,
    settings: Settings,
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
    dialog: Option<Message>,
    #[cfg(not(windows))]
    menu: rinst::ui::menu::Menu,
    /// Resident mode (`--tray`): closing the window hides it to the
    /// notification area.
    tray: bool,
    hidden: bool,
    maximize_on_show: bool,
    /// Starts with the session (as `rinst --tray`).
    autostart: bool,
    /// "Uninstall with rinst" is on the Explorer menu of folders, for this rinst.exe.
    #[cfg(windows)]
    explorer_menu: bool,
    #[cfg(not(windows))]
    resident: Option<rt_sys::linux::shell::Shell>,
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
        self.gfx.clear_cache();
        self.menu_rebuild = true;
        self.menu_dirty = true;
    }

    fn save_settings(&mut self) {
        if let Some(w) = &self.win {
            rt_ui::window::remember_size(&w.window, &mut self.settings.window);
        }
        self.page.store_into(&mut self.settings);
        let store = self.settings.to_store();
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
            self.page.dragging = false;
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
        let title = if self.page.admin { format!("rinst ({})", tr("Administrator")) } else { "rinst".to_string() };
        if w.window.title() != title {
            w.window.set_title(&title);
        }
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self, el: &ActiveEventLoop, cmd: Command) {
        let mut dialogs = self.dialogs();
        if self.page.command(cmd, &mut dialogs) {
            self.after_page();
            return;
        }
        if !self.enabled(cmd) {
            return;
        }
        match cmd {
            #[cfg(windows)]
            Command::RestartAdmin => {
                self.save_settings();
                if rt_sys::win::restart_elevated("") {
                    self.quit(el);
                }
            }
            Command::Exit => self.quit(el),
            #[cfg(windows)]
            Command::ExplorerMenu => {
                let text = match integrate(!self.explorer_menu) {
                    Ok(t) | Err(t) => t,
                };
                self.explorer_menu = integrated_here();
                self.info(&tr("Windows Integration"), &text);
            }
            Command::Autostart => match rt_sys::autostart::set("rinst", !self.autostart) {
                Ok(()) => self.autostart = rt_sys::autostart::enabled("rinst"),
                Err(e) => self.info("rinst", &tf("Cannot change how it starts: {}", &[&e])),
            },
            Command::Theme(t) => {
                self.settings.theme = t;
                self.apply_window_theme();
            }
            Command::UiLanguage(i) => {
                self.settings.language = (i as usize).checked_sub(1).and_then(|k| i18n::Lang::ALL.get(k)).copied();
                self.apply_language();
            }
            Command::Shortcuts => self.info(&tr("Keyboard shortcuts"), &shortcuts_text()),
            Command::CheckUpdates => rt_ui::update::check("rinst", env!("CARGO_PKG_VERSION"), self.update_sender()),
            Command::About => {
                let text = tf(
                    "rinst {}\n\nLists the programs installed for every user, with their shortcuts, written in Rust.\nNo telemetry, no network access.\n\nMIT License.",
                    &[env!("CARGO_PKG_VERSION")],
                );
                self.info(&tr("About rinst"), &text);
            }
            _ => {}
        }
        self.menu_dirty = true;
        self.redraw();
    }

    fn checked(&self, c: Command) -> bool {
        match c {
            Command::Theme(t) => self.settings.theme == t,
            Command::UiLanguage(0) => self.settings.language.is_none(),
            Command::UiLanguage(i) => self.settings.language == i18n::Lang::ALL.get(i as usize - 1).copied(),
            Command::Autostart => self.autostart,
            #[cfg(windows)]
            Command::ExplorerMenu => self.explorer_menu,
            c => self.page.checked(c),
        }
    }

    fn enabled(&self, c: Command) -> bool {
        self.page.enabled(c)
    }

    fn info(&mut self, title: &str, text: &str) {
        self.dialog = Some(Message::new(title, text));
        self.page.dragging = false;
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

    /// Hides the window to the notification area; an uninstalling goes on.
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
        self.page.release();
        // Release the frame buffer while hidden.
        self.gfx.resize(1, 1);
        self.gfx.trim_cache();
    }

    /// Shows the window (from the icon or another launch), and reads the
    /// programs again when nothing is going on.
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
        let was_hidden = std::mem::take(&mut self.hidden);
        self.autostart = rt_sys::autostart::enabled("rinst");
        self.menu_dirty = true;
        if was_hidden && self.page.idle() {
            self.page.refresh();
            self.after_page();
        }
        self.redraw();
    }

    /// A menu at the pointer, asked for by the page.
    fn popup(&mut self, el: &ActiveEventLoop, which: Popup) {
        let tree = which.items();
        #[cfg(windows)]
        {
            let Some(hwnd) = self.win.as_ref().map(|w| w.hwnd) else { return };
            let me = &*self;
            if let Some(c) = rt_sys::win::menu::popup_at_pointer(hwnd, tree, |c| me.checked(c), |c| me.enabled(c)) {
                self.command(el, c);
            }
        }
        #[cfg(not(windows))]
        {
            let _ = el;
            let me = &*self;
            let items = rinst::ui::menu::build(tree, &|c| me.checked(c), &|c| me.enabled(c));
            let at = self.cursor.unwrap_or((0.0, 0.0));
            self.menu.open(None, items, at, false);
            self.redraw();
        }
    }

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let (w, h, maximized) = self.settings.window.unwrap_or((1200.0, 760.0, false));
        let attrs = Window::default_attributes()
            .with_title("rinst")
            .with_inner_size(LogicalSize::new(w.max(560.0), h.max(400.0)))
            .with_min_inner_size(LogicalSize::new(560.0, 400.0))
            // Maximizing shows the window on Windows: a resident start, which
            // stays hidden, maximizes when it is first shown.
            .with_maximized(maximized && !self.tray)
            .with_window_icon(rt_ui::window::icon(rinst::ui::appicon::render))
            .with_theme(Some(rt_ui::window::theme(self.dark())))
            .with_visible(false);
        let Some((window, surface)) = rt_ui::window::create(el, attrs, "rinst") else { return false };
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
                rt_sys::win::tray::show(hwnd, "rinst");
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
}

/// Starts resident mode on Linux: without an icon, a hidden window could not
/// be reached or closed, so rinst then runs as a normal window.
#[cfg(not(windows))]
fn start_resident(shell: &mut Shell) {
    let proxy = std::sync::Mutex::new(shell.proxy.clone());
    let send = Box::new(move |m| {
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(UserEvent::Shell(m));
        }
    });
    shell.resident = rt_ui::resident::start("rinst", rinst::ui::appicon::render, tr("Open rinst"), tr("Exit"), send);
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
        // A resident start stays in the notification area (and reads nothing
        // until shown); Linux has no hidden window, so it is made when first
        // shown.
        if !(cfg!(not(windows)) && self.tray) && !self.open_window(el) {
            return;
        }
        self.sync_menu();
        if self.tray {
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
            // Another launch, or the tray icon: the window shows.
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::OpenFiles(_)) => self.show(el),
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::Tray(rt_sys::win::tray::TrayEvent::Activate)) => self.show(el),
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::Tray(rt_sys::win::tray::TrayEvent::Menu)) => {
                if let Some(hwnd) = self.win.as_ref().map(|w| w.hwnd) {
                    match rt_sys::win::tray::menu(hwnd, &tr("Open rinst"), &tr("Exit")) {
                        Some(1) => self.show(el),
                        Some(2) => self.quit(el),
                        _ => {}
                    }
                }
            }
            #[cfg(not(windows))]
            UserEvent::Shell(rt_sys::linux::shell::Msg::Open(_)) => self.show(el),
            #[cfg(not(windows))]
            UserEvent::Shell(rt_sys::linux::shell::Msg::Quit) => self.quit(el),
            UserEvent::Update(s) => {
                let who = rt_ui::update::Who {
                    app: "rinst",
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
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.redraw(),
            WindowEvent::ThemeChanged(t) => {
                self.system_dark = t == WinTheme::Dark;
                self.redraw();
            }
            WindowEvent::Focused(false) => self.save_settings(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
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
                self.page.drag(&mut self.gfx, x, y);
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
                ElementState::Released => self.page.release(),
            },
            #[cfg(not(windows))]
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } if self.menu.is_open() => {
                self.menu.close();
                self.redraw();
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Right, .. } if self.dialog.is_none() => {
                let popup = self.page.right_click();
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
        // While uninstalling, the bar and the marks move, so it shows it is
        // working even when the uninstaller shows nothing.
        if self.page.animate() {
            const FRAME: std::time::Duration = std::time::Duration::from_millis(40);
            self.after_page();
            el.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now() + FRAME));
        } else {
            el.set_control_flow(ControlFlow::Wait);
        }
    }
}

/// Whether "Uninstall with rinst" is on the Explorer menu of folders, for this rinst.exe.
#[cfg(windows)]
fn integrated_here() -> bool {
    rt_ui::explorer::folders_here("rinst")
}

/// Adds "Uninstall with rinst" to the Explorer menu of folders and drives
/// (current user only), or takes it out; returns the message to show.
#[cfg(windows)]
fn integrate(on: bool) -> Result<String, String> {
    let texts = rt_ui::explorer::FolderTexts {
        verb: tr("Uninstall with rinst"),
        added: tr("“Uninstall with rinst” was added to the Explorer context menu of folders and drives."),
        removed: tr("“Uninstall with rinst” was removed from the Explorer context menu."),
        failed: tr("Cannot change the Windows registry (error {})."),
    };
    rt_ui::explorer::set_folders("rinst", on, &texts)
}

const USAGE: &str = "rinst [--tray] [--new-window]

  Lists the programs installed on this computer: for every user (the
  system's), for you, and, with administrator rights, for every other user;
  with their size, version, publisher, uninstall command, registry entry and
  the shortcuts that belong to them on the Start menu and the desktop.

  --tray        Stay resident in the notification area: closing the window
                hides it, and the icon (or starting rinst again) shows it,
                reading the programs again. Exit from the icon's menu or
                File > Exit. File > Start with Windows starts it this way
                with the session.
  --new-window  Open a separate window even when rinst is resident.

Windows only:
  --register    Add \"Uninstall with rinst\" to the Explorer context menu of
                folders and drives (current user only).
  --unregister  Remove it.

Settings are stored in the user's settings folder (%APPDATA%\\rinst on
Windows, ~/.config/rinst on Linux), or next to the program when a file
named rinst.portable exists there (portable mode).";

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // What an update left behind.
    rt_sys::update::clean_old();
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
    let tray = raw.iter().any(|a| a == "--tray" || a == "/tray") && !help;
    let new_window = raw.iter().any(|a| a == "--new-window") || help;
    // A resident rinst shows itself instantly.
    #[cfg(windows)]
    if !new_window && rt_sys::win::hand_off(&IDENT, &[], false, false) {
        return;
    }
    #[cfg(not(windows))]
    if !new_window && rt_sys::linux::shell::hand_off("rinst", &[], false) {
        return;
    }
    let mut location = storage::detect();
    // Started with the session under the old name: now under the new one.
    let _ = rt_sys::autostart::rename(storage::OLD_NAME, "rinst");
    let store = location.load();
    let settings = Settings::load(&store);
    let gfx = match Gfx::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rinst: {e}");
            std::process::exit(1);
        }
    };
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("rinst: cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    #[cfg(windows)]
    let admin = rt_sys::win::elevated();
    #[cfg(not(windows))]
    let admin = rinst::linux_pkgs::is_root();
    let data_dir = location
        .dir
        .clone()
        .or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())))
        .unwrap_or_else(std::env::temp_dir);
    let sender = {
        let proxy = std::sync::Mutex::new(event_loop.create_proxy());
        Arc::new(move |e| {
            if let Ok(p) = proxy.lock() {
                let _ = p.send_event(UserEvent::Page(e));
            }
        })
    };
    let page = InstPage::new(&settings, admin, data_dir, sender);
    let mut shell = Shell {
        page,
        settings,
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
        dialog: help.then(|| Message::new("rinst", USAGE)),
        #[cfg(not(windows))]
        menu: rinst::ui::menu::Menu::default(),
        tray,
        hidden: false,
        maximize_on_show: false,
        autostart: rt_sys::autostart::enabled("rinst"),
        #[cfg(windows)]
        explorer_menu: integrated_here(),
        #[cfg(not(windows))]
        resident: None,
    };
    shell.apply_language();
    // A first reading at once, unless it starts hidden (it reads when shown).
    if !tray {
        shell.page.refresh();
        shell.after_page();
    }
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rinst: {e}");
    }
}
