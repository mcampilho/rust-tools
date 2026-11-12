//! rpad: a portable, offline text editor for Windows and Linux.
//!
//! The window is driven by winit and every frame is rendered on the CPU
//! (tiny-skia + ab_glyph) into a softbuffer surface, so the program needs no
//! GPU runtime, DLLs beyond the operating system, or bundled assets. Frames
//! are produced only in response to input, timers (caret blink) or
//! background work.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod doc;
mod input;
mod menus;
mod render;
mod i18n;
mod io;
mod search;
mod storage;
mod syntax;
mod tools;
mod ui;
#[cfg(windows)]
mod win;
#[cfg(not(windows))]
mod linux;

use app::commands::{Command, M};
use app::input::{Key, KeyInput};
use app::settings::{Settings, ThemePref};
use app::{Answer, App, Focus, Host};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ui::gfx::{Color, Gfx, Rect};
use ui::{Input, Role, UiAction, WId, Widget};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Fullscreen, Theme as WinTheme, Window, WindowId};

const DEFAULT_ACCENT: u32 = 0x0067C0;
const TOOLTIP_DELAY: Duration = Duration::from_millis(600);

enum UserEvent {
    A11y(accesskit_winit::Event),
    /// Background work finished (search in files, match counting).
    Wake,
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

// ---------------------------------------------------------------- host

/// Native dialogs and services for the application logic.
struct WinHost {
    #[cfg(windows)]
    hwnd: windows_sys::Win32::Foundation::HWND,
    /// A message (title, text) to show in the window, in its theme.
    message: std::cell::RefCell<Option<(String, String)>>,
}

#[cfg(windows)]
impl Host for WinHost {
    fn ask_save(&self, name: &str) -> Answer {
        use win::dialogs::{message, Answer as A, Buttons, Icon};
        match message(self.hwnd, "rpad", &i18n::tf("Do you want to save changes to {}?", &[name]), Buttons::YesNoCancel, Icon::Warning) {
            A::Yes => Answer::Yes,
            A::No => Answer::No,
            _ => Answer::Cancel,
        }
    }
    fn confirm(&self, title: &str, text: &str) -> bool {
        use win::dialogs::{message, Answer as A, Buttons, Icon};
        matches!(message(self.hwnd, title, text, Buttons::OkCancel, Icon::Question), A::Ok | A::Yes)
    }
    fn info(&self, title: &str, text: &str) {
        *self.message.borrow_mut() = Some((title.to_string(), text.to_string()));
    }
    fn error(&self, title: &str, text: &str) {
        use win::dialogs::{message, Buttons, Icon};
        message(self.hwnd, title, text, Buttons::Ok, Icon::Error);
    }
    fn open_dialog(&self, dir: Option<&Path>) -> Vec<PathBuf> {
        win::dialogs::open_files(self.hwnd, dir)
    }
    fn save_dialog(&self, name: &str, dir: Option<&Path>) -> Option<PathBuf> {
        win::dialogs::save_file(self.hwnd, name, dir)
    }
    fn pick_folder(&self) -> Option<PathBuf> {
        win::dialogs::pick_folder(self.hwnd, &i18n::tr("Choose the folder to search"))
    }
    fn choose_font(&self, face: &str, size: f32) -> Option<(String, f32)> {
        win::dialogs::choose_font(self.hwnd, face, size)
    }
    fn clipboard_get(&self) -> Option<String> {
        win::get_clipboard()
    }
    fn clipboard_set(&self, text: &str) {
        win::set_clipboard(text);
    }
    fn reveal(&self, path: &Path) {
        win::reveal_in_explorer(path);
    }
}

#[cfg(not(windows))]
impl Host for WinHost {
    fn ask_save(&self, name: &str) -> Answer {
        linux::dialogs::ask_save(&i18n::tf("Do you want to save changes to {}?", &[name]))
    }
    fn confirm(&self, title: &str, text: &str) -> bool {
        linux::dialogs::confirm(title, text)
    }
    fn info(&self, title: &str, text: &str) {
        *self.message.borrow_mut() = Some((title.to_string(), text.to_string()));
    }
    fn error(&self, title: &str, text: &str) {
        linux::dialogs::error(title, text);
    }
    fn open_dialog(&self, dir: Option<&Path>) -> Vec<PathBuf> {
        linux::dialogs::open_files(dir)
    }
    fn save_dialog(&self, name: &str, dir: Option<&Path>) -> Option<PathBuf> {
        linux::dialogs::save_file(name, dir)
    }
    fn pick_folder(&self) -> Option<PathBuf> {
        linux::dialogs::pick_folder(&i18n::tr("Choose the folder to search"))
    }
    fn choose_font(&self, face: &str, size: f32) -> Option<(String, f32)> {
        linux::dialogs::choose_font(face, size)
    }
    fn clipboard_get(&self) -> Option<String> {
        linux::get_clipboard()
    }
    fn clipboard_set(&self, text: &str) {
        linux::set_clipboard(text);
    }
    fn reveal(&self, path: &Path) {
        linux::reveal(path);
    }
}

// ---------------------------------------------------------------- shell

struct Win {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    adapter: accesskit_winit::Adapter,
    #[cfg(windows)]
    menu: Option<win::menu::MenuBar>,
}

struct Shell {
    app: App,
    gfx: Gfx,
    input: Input,
    widgets: Vec<Widget>,
    win: Option<Win>,
    host: WinHost,
    proxy: EventLoopProxy<UserEvent>,
    mods: ModifiersState,
    cursor: Option<(f32, f32)>,
    system_dark: bool,
    accent: Color,
    theme_pref: Option<ThemePref>,
    /// Recent files the menu was built with.
    #[cfg(windows)]
    menu_recent: Vec<PathBuf>,
    /// Pressed widget (activates on release over the same widget).
    pressed: Option<WId>,
    editor_drag: bool,
    hover_since: Instant,
    blink: Option<Duration>,
    blink_at: Instant,
    focused: bool,
    last_autoscroll: Instant,
    /// Redraw once edits settle (folds and outline are recomputed lazily).
    idle_redraw: Option<Instant>,
    seen_edit: Instant,
    ime_rect: Option<Rect>,
    cursor_icon: CursorIcon,
    args: Vec<PathBuf>,
    /// Resident mode: closing the window hides it to the notification area.
    tray: bool,
    hidden: bool,
    /// The window was last maximized: done when it is first shown.
    maximize_on_show: bool,
    /// Theme the native menus were last drawn with (dark or not).
    menu_dark: Option<bool>,
    #[cfg(not(windows))]
    shell: Option<linux::shell::Shell>,
    /// The message shown over the window (About, the help...), and where
    /// its panel and OK button were drawn.
    dialog: Option<(String, String, Rect, Rect)>,
}

fn theme_for(pref: ThemePref) -> Option<WinTheme> {
    match pref {
        ThemePref::Dark => Some(WinTheme::Dark),
        ThemePref::Light => Some(WinTheme::Light),
        ThemePref::System => None,
    }
}

impl Shell {
    fn theme(&self) -> ui::theme::Theme {
        let dark = match self.app.settings.theme {
            ThemePref::Dark => true,
            ThemePref::Light => false,
            ThemePref::System => self.system_dark,
        };
        ui::theme::Theme::new(dark, self.accent)
    }

    fn redraw(&self) {
        if let Some(w) = &self.win {
            w.window.request_redraw();
        }
    }

    fn top_layer(&self) -> u8 {
        self.widgets.iter().map(|w| w.layer).max().unwrap_or(0)
    }

    fn hit(&self, x: f32, y: f32) -> Option<&Widget> {
        let layer = self.top_layer();
        self.widgets.iter().rev().filter(|w| w.layer == layer && w.role != Role::Label && w.enabled).find(|w| w.rect.contains(x, y))
    }

    fn widget(&self, id: WId) -> Option<&Widget> {
        self.widgets.iter().find(|w| w.id == id)
    }

    fn update_hot(&mut self) -> bool {
        let hot = self.cursor.and_then(|(x, y)| self.hit(x, y)).filter(|w| w.action != UiAction::Editor).map(|w| w.id);
        if hot != self.input.hot {
            self.input.hot = hot;
            self.input.tooltip = None;
            self.hover_since = Instant::now();
            return true;
        }
        false
    }

    fn reset_blink(&mut self) {
        self.input.caret_on = true;
        self.blink_at = Instant::now();
    }

    /// Runs a command and applies its window-level side effects.
    fn command(&mut self, el: &ActiveEventLoop, cmd: Command) {
        #[cfg(windows)]
        {
            let action = match cmd {
                Command::ExplorerMenu if self.app.explorer_menu => Some(Integration::Unregister),
                Command::ExplorerMenu => Some(Integration::Register),
                Command::DefaultEditor => Some(Integration::Associate),
                _ => None,
            };
            if let Some(a) = action {
                let text = match integrate(a) {
                    Ok(t) | Err(t) => t,
                };
                self.app.explorer_menu = integrated_here();
                self.app.menu_dirty = true;
                self.host.info(&i18n::tr("Windows Integration"), &text);
                return self.after_action(el);
            }
        }
        self.app.dispatch(cmd, &self.host);
        self.after_action(el);
    }

    fn after_action(&mut self, el: &ActiveEventLoop) {
        self.reset_blink();
        if self.app.quit {
            self.app.quit = false;
            self.quit(el);
            return;
        }
        if self.app.close_window {
            self.app.close_window = false;
            self.close(el);
            return;
        }
        if self.app.fullscreen_request {
            self.app.fullscreen_request = false;
            if let Some(w) = &self.win {
                let fs = w.window.fullscreen().is_some();
                w.window.set_fullscreen(if fs { None } else { Some(Fullscreen::Borderless(None)) });
            }
        }
        let pref = self.app.settings.theme;
        if self.theme_pref != Some(pref) {
            self.theme_pref = Some(pref);
            if let Some(w) = &self.win {
                w.window.set_theme(theme_for(pref));
            }
        }
        self.redraw();
    }

    /// The window's close button: hides to the tray in resident mode.
    fn close(&mut self, el: &ActiveEventLoop) {
        if self.tray {
            self.hide();
        } else {
            self.quit(el);
        }
    }

    /// Hides the window to the notification area, keeping documents open.
    fn hide(&mut self) {
        self.app.autosave(&self.host);
        self.app.write_recovery(false);
        self.remember_window();
        self.app.save_settings();
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
        self.focused = false;
        self.input.hot = None;
        self.input.tooltip = None;
        // Release the frame buffer while hidden.
        self.gfx.resize(1, 1);
        self.gfx.trim_cache();
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
        self.app.check_external_changes();
        self.app.reveal_caret = true;
        self.reset_blink();
        self.redraw();
    }

    /// Closes the window: hot exit when crash recovery is on, otherwise
    /// asks about unsaved documents.
    fn quit(&mut self, el: &ActiveEventLoop) {
        if !self.app.settings.recovery && self.app.docs.iter().any(|d| d.is_modified()) && self.hidden {
            self.show(el);
        }
        if !self.app.settings.recovery && !self.app.confirm_all(&self.host) {
            self.redraw();
            return;
        }
        #[cfg(windows)]
        win::tray::remove(self.host.hwnd);
        #[cfg(not(windows))]
        {
            self.shell = None;
        }
        self.app.stop_fif();
        self.app.write_recovery(true);
        self.remember_window();
        self.app.save_settings();
        el.exit();
    }

    fn remember_window(&mut self) {
        if let Some(w) = &self.win {
            let maximized = w.window.is_maximized();
            if !maximized && !w.window.is_minimized().unwrap_or(false) && w.window.fullscreen().is_none() {
                let s: LogicalSize<f64> = w.window.inner_size().to_logical(w.window.scale_factor());
                self.app.settings.window = Some((s.width, s.height, false));
            } else if let Some((ww, hh, _)) = self.app.settings.window {
                self.app.settings.window = Some((ww, hh, maximized));
            } else {
                self.app.settings.window = Some((1000.0, 700.0, maximized));
            }
        }
    }

    fn open_files(&mut self, el: &ActiveEventLoop, paths: &[PathBuf]) {
        if self.hidden {
            self.show(el);
        }
        for p in paths {
            self.app.open_arg(p, &self.host);
        }
        if let Some(w) = &self.win {
            w.window.focus_window();
        }
        self.after_action(el);
    }

    // ------------------------------------------------------------ actions

    fn action(&mut self, el: &ActiveEventLoop, a: UiAction) {
        let host = &self.host;
        let app = &mut self.app;
        match a {
            UiAction::Cmd(c) => return self.command(el, c),
            UiAction::Tab(i) => app.activate(i),
            UiAction::CloseTab(i) => {
                app.close_doc(i, host);
            }
            UiAction::NewTab => return self.command(el, Command::New),
            UiAction::TabScroll(d) => return self.command(el, if d < 0 { Command::PrevTab } else { Command::NextTab }),
            UiAction::FindOpt(i) => {
                match i {
                    0 => app.find.case = !app.find.case,
                    1 => app.find.word = !app.find.word,
                    2 => app.find.regex = !app.find.regex,
                    _ => app.toggle_in_selection(),
                }
                app.find.invalidate();
            }
            UiAction::FindNext => app.find_step(true, host),
            UiAction::FindPrev => app.find_step(false, host),
            UiAction::ReplaceOne => app.replace_one(host),
            UiAction::ReplaceAll => app.replace_all(host),
            UiAction::CloseFind => app.close_find(),
            UiAction::ToggleReplace => {
                app.find.replace_mode = !app.find.replace_mode;
                if !app.find.replace_mode && app.focus == Focus::Replace {
                    app.focus = Focus::Find;
                }
            }
            UiAction::Field(f) => app.focus = f,
            UiAction::NoticeReload => {
                let i = app.active;
                app.reload(i, None, host);
            }
            UiAction::NoticeKeep => app.keep_mine(),
            UiAction::NoticeDismiss => app.doc_mut().notice = None,
            UiAction::NoticeSaveAs => {
                let i = app.active;
                app.save_doc(i, true, host);
            }
            UiAction::PaletteItem(k) => {
                if let Some(p) = app.palette.as_mut() {
                    p.sel = k;
                }
                app.palette_accept(host);
            }
            UiAction::Result(k) => {
                if let Some(r) = app.results.as_mut() {
                    r.sel = k;
                }
                app.open_result(k, host);
            }
            UiAction::CloseResults => {
                app.stop_fif();
                app.results = None;
                app.focus = Focus::Editor;
            }
            UiAction::FifSearch => app.start_fif(host),
            UiAction::FifStop => app.stop_fif(),
            UiAction::FifBrowse => {
                if let Some(d) = host.pick_folder() {
                    app.fif.dir.set(&d.display().to_string());
                }
                app.focus = Focus::FifDir;
            }
            UiAction::Status(k) => return self.status_menu(el, k),
            UiAction::Outline(line) => {
                app.goto_line(line, None);
                app.focus = Focus::Editor;
            }
            #[cfg(not(windows))]
            UiAction::MenuTitle(i) => {
                if self.app.menu.top == Some(i) {
                    self.app.menu.close();
                    return self.redraw();
                }
                return self.open_menu(i, false);
            }
            #[cfg(not(windows))]
            UiAction::MenuRow(l, i) => {
                let act = self.app.menu.click(l, i);
                return self.menu_act(el, act);
            }
            UiAction::Editor | UiAction::Label(_) | UiAction::None => {}
        }
        self.after_action(el);
    }

    // ------------------------------------------------------------ keyboard

    // ------------------------------------------------------------ mouse

    // ------------------------------------------------------------ frame

    /// Next time a timer needs the loop to wake up.
    fn next_wake(&mut self) -> Option<Instant> {
        let now = Instant::now();
        let mut next: Option<Instant> = None;
        let mut at = |t: Instant| next = Some(next.map_or(t, |n: Instant| n.min(t)));
        if self.focused {
            if let Some(b) = self.blink {
                at(self.blink_at + b);
            }
        }
        if let Some(t) = self.app.recovery_due {
            at(t);
        }
        if let Some((_, t)) = &self.app.toast {
            let end = *t + Duration::from_millis(3000);
            if end > now {
                at(end);
            }
        }
        if self.input.hot.is_some() && self.input.tooltip.is_none() {
            at(self.hover_since + TOOLTIP_DELAY);
        }
        if self.editor_drag && self.app.autoscroll_needed().is_some() {
            at(self.last_autoscroll + Duration::from_millis(40));
        }
        if let Some(t) = self.idle_redraw {
            at(t);
        }
        if self.app.fif_running() || self.app.find.counting {
            at(now + Duration::from_millis(100));
        }
        next
    }

    /// Fires due timers.
    fn timers(&mut self) {
        let now = Instant::now();
        if self.app.last_edit != self.seen_edit {
            self.seen_edit = self.app.last_edit;
            self.idle_redraw = Some(self.app.last_edit + Duration::from_millis(350));
        }
        if let Some(t) = self.idle_redraw {
            if now >= t {
                self.idle_redraw = None;
                self.app.menu_dirty = true;
                self.redraw();
            }
        }
        if let Some(b) = self.blink {
            if self.focused && now >= self.blink_at + b {
                self.input.caret_on = !self.input.caret_on;
                self.blink_at = now;
                self.redraw();
            }
        }
        if self.app.recovery_due.is_some_and(|t| now >= t) {
            self.app.write_recovery(false);
        }
        if let Some((_, t)) = &self.app.toast {
            if now >= *t + Duration::from_millis(3000) {
                self.app.toast = None;
                self.redraw();
            }
        }
        if self.input.hot.is_some() && self.input.tooltip.is_none() && now >= self.hover_since + TOOLTIP_DELAY && self.pressed.is_none() {
            if self.input.hot.and_then(|h| self.widget(h)).is_some_and(|w| w.tooltip.is_some()) {
                self.input.tooltip = self.input.hot;
                self.redraw();
            } else {
                // No tooltip: stop waiting for this widget.
                self.hover_since = now + Duration::from_secs(3600);
            }
        }
        if self.editor_drag && now >= self.last_autoscroll + Duration::from_millis(40) {
            if let Some(dir) = self.app.autoscroll_needed() {
                self.last_autoscroll = now;
                self.app.scroll_rows(dir);
                let (x, y) = self.app.input.mouse;
                self.app.mouse_drag(x, y);
                self.redraw();
            }
        }
        if self.app.fif_running() || self.app.find.counting {
            self.app.pump();
            self.redraw();
        }
    }
}

impl Shell {
    /// Starts the single-instance socket and the tray icon.
    #[cfg(not(windows))]
    fn start_shell(&mut self) {
        let proxy = Mutex::new(self.proxy.clone());
        let labels = linux::shell::TrayLabels { tip: "rpad".into(), open: i18n::tr("Open rpad"), exit: i18n::tr("Exit") };
        let cfg = linux::shell::Config { app: linux::shell::APP, icon: ui::appicon::render, tray: self.tray.then_some(labels), listen_without_icon: true };
        let shell = linux::shell::Shell::new(
            cfg,
            Box::new(move |m| {
                if let Ok(p) = proxy.lock() {
                    let _ = p.send_event(UserEvent::Shell(m));
                }
            }),
        );
        // Without an icon, a hidden window could not be reached or closed.
        self.tray = shell.has_icon();
        self.shell = Some(shell);
    }

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let (w, h, maximized) = self.app.settings.window.unwrap_or((1000.0, 700.0, false));
        let attrs = Window::default_attributes()
            .with_title("rpad")
            .with_inner_size(LogicalSize::new(w.max(420.0), h.max(260.0)))
            .with_min_inner_size(LogicalSize::new(420.0, 260.0))
            // Maximizing shows the window on Windows: a resident start, which
            // stays hidden, maximizes when it is first shown.
            .with_maximized(maximized && !self.tray)
            .with_window_icon(rt_ui::window::icon(ui::appicon::render))
            .with_theme(theme_for(self.app.settings.theme))
            .with_visible(false);
        let Some((window, surface)) = rt_ui::window::create(el, attrs, "rpad") else { return false };
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(el, &window, self.proxy.clone());
        window.set_ime_allowed(true);
        #[cfg(windows)]
        {
            self.host.hwnd = rt_ui::window::hwnd_of(&window);
            let proxy = self.proxy.clone();
            let mnemonics = rt_ui::window::mnemonics_of(&app::commands::menu_tree(&[]));
            win::subclass(
                self.host.hwnd,
                &mnemonics,
                Box::new(move |m| {
                    let _ = proxy.send_event(UserEvent::Win(m));
                }),
            );
        }
        self.maximize_on_show = maximized && self.tray;
        self.theme_pref = Some(self.app.settings.theme);
        self.win = Some(Win {
            window,
            surface,
            adapter,
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
        self.start_shell();
        // Linux has no hidden window: a resident start makes it when first shown.
        let no_window = cfg!(not(windows)) && self.tray && self.args.is_empty();
        if !no_window && !self.open_window(el) {
            return;
        }
        self.sync_menu();
        // Restore the previous session, then open the command-line files.
        self.app.restore_session(&self.host);
        let args = std::mem::take(&mut self.args);
        for p in &args {
            self.app.open_arg(p, &self.host);
        }
        if let Some(e) = self.app.location.error.clone() {
            self.app.notify(e);
        }
        #[cfg(not(windows))]
        if !linux::dialogs::available() {
            self.app.notify(i18n::tr("Install zenity or kdialog to use the file dialogs."));
        }
        #[cfg(windows)]
        if self.tray {
            win::set_resident(self.host.hwnd);
            win::tray::show(self.host.hwnd, "rpad");
        }
        // Resident mode starts in the notification area unless files were given.
        if self.tray && args.is_empty() {
            self.hidden = true;
            self.focused = false;
            return;
        }
        if let Some(w) = &self.win {
            if std::mem::take(&mut self.maximize_on_show) {
                w.window.set_maximized(true);
            }
            w.window.set_visible(true);
            w.window.focus_window();
        }
        self.focused = true;
        self.reset_blink();
        self.redraw();
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Wake => {
                self.app.pump();
                self.redraw();
            }
            #[cfg(not(windows))]
            UserEvent::Shell(linux::shell::Msg::Open(paths)) => self.open_files(el, &paths),
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
            UserEvent::Win(win::WinMsg::OpenFiles(paths)) => self.open_files(el, &paths),
            #[cfg(windows)]
            UserEvent::Win(win::WinMsg::Tray(win::tray::TrayEvent::Activate)) => self.show(el),
            #[cfg(windows)]
            UserEvent::Win(win::WinMsg::SystemTheme) => {}
            #[cfg(windows)]
            UserEvent::Win(win::WinMsg::Tray(win::tray::TrayEvent::Menu)) => {
                match win::tray::menu(self.host.hwnd, &i18n::tr("Open rpad"), &i18n::tr("Exit")) {
                    Some(1) => self.show(el),
                    Some(2) => self.quit(el),
                    _ => {}
                }
            }
            UserEvent::A11y(ev) => match ev.window_event {
                accesskit_winit::WindowEvent::InitialTreeRequested => {
                    let tree = self.tree();
                    if let Some(w) = &mut self.win {
                        w.adapter.update_if_active(|| tree);
                    }
                    self.redraw();
                }
                accesskit_winit::WindowEvent::ActionRequested(req) => {
                    let id = req.target_node.0;
                    let Some(w) = self.widget(id).cloned() else { return };
                    match (req.action, &w.action) {
                        (accesskit::Action::Focus, UiAction::Editor) => {
                            self.app.focus = Focus::Editor;
                            self.redraw();
                        }
                        (accesskit::Action::Focus, UiAction::Field(f)) => {
                            self.app.focus = *f;
                            self.redraw();
                        }
                        (accesskit::Action::Click, a) => self.action(el, a.clone()),
                        _ => {}
                    }
                }
                accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
            },
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(w) = &mut self.win {
            w.adapter.process_event(&w.window, &event);
        }
        match event {
            WindowEvent::CloseRequested => self.close(el),
            WindowEvent::Focused(f) => {
                self.focused = f;
                self.input.focused_window = f;
                if f {
                    self.app.check_external_changes();
                    self.reset_blink();
                } else {
                    self.input.caret_on = true;
                    self.app.autosave(&self.host);
                    self.app.write_recovery(false);
                    self.remember_window();
                    self.app.save_settings();
                }
                self.redraw();
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.app.reveal_caret = true;
                self.redraw();
            }
            WindowEvent::ThemeChanged(t) => {
                self.system_dark = t == WinTheme::Dark;
                self.redraw();
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
                self.cursor = Some((x, y));
                if self.editor_drag {
                    if self.app.mouse_drag(x, y) {
                        self.redraw();
                    }
                    self.last_autoscroll = Instant::now() - Duration::from_millis(40);
                }
                if self.update_hot() {
                    self.redraw();
                    #[cfg(not(windows))]
                    self.menu_pointer();
                }
                self.update_cursor_icon();
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                if self.update_hot() {
                    self.redraw();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => match state {
                ElementState::Pressed => self.mouse_press(el, button),
                ElementState::Released if button == MouseButton::Left => self.mouse_release(el),
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => self.wheel(delta),
            WindowEvent::DroppedFile(p) => self.open_files(el, &[p]),
            WindowEvent::Ime(ime) => match ime {
                Ime::Preedit(s, _) => {
                    self.app.input.preedit = s;
                    self.redraw();
                }
                Ime::Commit(s) => {
                    self.app.input.preedit.clear();
                    self.app.text_input(&s, &self.host);
                    self.after_action(el);
                }
                Ime::Disabled => {
                    self.app.input.preedit.clear();
                    self.redraw();
                }
                Ime::Enabled => {}
            },
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if event.state == ElementState::Pressed && self.app.input.preedit.is_empty() {
                    self.key(el, &event);
                }
            }
            WindowEvent::RedrawRequested if !self.hidden => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.timers();
        match self.next_wake() {
            Some(t) => el.set_control_flow(ControlFlow::WaitUntil(t)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }
}

#[cfg(windows)]
#[derive(Clone, Copy)]
enum Integration {
    Register,
    Associate,
    Unregister,
}

/// Whether "Edit with rpad" is on the Explorer context menu, for this rpad.exe.
#[cfg(windows)]
fn integrated_here() -> bool {
    let exe = rt_sys::win::assoc::current_exe().unwrap_or_default();
    win::assoc::status(&exe) == win::assoc::Status::Here
}

/// Adds or removes rpad in Explorer (current user only); returns the
/// message to show, or an error message.
#[cfg(windows)]
fn integrate(action: Integration) -> Result<String, String> {
    use i18n::{tf, tr};
    use win::assoc;
    let exe = rt_sys::win::assoc::current_exe()?;
    let failed = |e: assoc::Error| tf("Cannot change the Windows registry (error {}).", &[&e.to_string()]);
    let verb = tr("Edit with rpad");
    match action {
        Integration::Register => {
            assoc::register(&exe, &verb).map_err(failed)?;
            Ok(tf("rpad was added to the Explorer context menu and to the “Open with” list for: {}", &[&assoc::list(&assoc::extensions())]))
        }
        Integration::Unregister => {
            assoc::unregister().map_err(failed)?;
            Ok(tr("rpad was removed from Explorer and from the file associations."))
        }
        Integration::Associate => {
            let d = assoc::associate(&exe, &verb).map_err(failed)?;
            let mut parts = Vec::new();
            if !d.set.is_empty() {
                parts.push(tf("rpad is now the default editor for: {}", &[&assoc::list(&d.set)]));
            }
            if !d.already.is_empty() {
                parts.push(tf("rpad was already the default editor for: {}", &[&assoc::list(&d.already)]));
            }
            if !d.user_choice.is_empty() {
                assoc::open_default_apps();
                parts.push(tf(
                    "Windows asks you to choose the default app yourself for: {}\nIn the Settings window that opened, use “Choose default apps by file type” and pick rpad.",
                    &[&assoc::list(&d.user_choice)],
                ));
            }
            Ok(parts.join("\n\n"))
        }
    }
}

const USAGE: &str = "rpad [options] [files...]

  files          Files to open. A file that does not exist yet opens as an
                 empty tab and is created when you save it.
  --tray         Stay resident in the notification area: closing the window
                 hides it (documents stay open) and launching rpad again,
                 or clicking the tray icon, shows it instantly. Exit from
                 the tray icon's menu or File > Exit.
  --new-window   Start a separate window instead of opening the files in
                 the running rpad window.
  --help         Show this help.

Windows only:
  --register     Add \"Edit with rpad\" to the Explorer context menu of text,
                 configuration and source files, and rpad to their \"Open
                 with\" list.
  --associate    Also make rpad the default editor for those types where
                 Windows allows it; for types whose default you chose
                 yourself, Windows Settings > Default apps opens.
  --unregister   Remove everything --register and --associate added and
                 restore the previous defaults.
  These change only the current user's settings (no administrator rights).

Settings are stored in %APPDATA%\\rpad (~/.config/rpad on Linux), or next
to the program when a file named rpad.portable exists there (portable mode).";

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // The help goes to the console it was asked from; without one, rpad
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
            rt_sys::win::report(&i18n::tr("Windows Integration"), &text);
            std::process::exit(if ok { 0 } else { 1 });
        }
    }
    let new_window = raw.iter().any(|a| a == "--new-window") || help;
    let tray = raw.iter().any(|a| a == "--tray" || a == "/tray") && !help;
    let files: Vec<PathBuf> = raw.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();

    let mut location = storage::detect();
    let store = location.load();
    let settings = Settings::load(&store);
    // A launch while a resident instance runs shows it (and opens the files
    // there); otherwise files go to a running window if single-window mode is on.
    #[cfg(windows)]
    if !new_window {
        if tray && win::resident_running() && files.is_empty() {
            win::hand_off_to_existing(&files, false);
            return;
        }
        if win::hand_off_to_existing(&files, settings.single_instance && !tray) {
            return;
        }
    }
    #[cfg(not(windows))]
    if !new_window {
        if tray && linux::shell::resident_running() && files.is_empty() {
            linux::shell::hand_off(&files, false);
            return;
        }
        if linux::shell::hand_off(&files, settings.single_instance && !tray) {
            return;
        }
    }
    let _ = new_window;

    let gfx = match Gfx::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rpad: {e}");
            #[cfg(windows)]
            win::dialogs::message(std::ptr::null_mut(), "rpad", &format!("rpad: {e}"), win::dialogs::Buttons::Ok, win::dialogs::Icon::Error);
            std::process::exit(1);
        }
    };
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("rpad: cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new(location, settings);
    app.apply_language();
    #[cfg(windows)]
    {
        app.explorer_menu = integrated_here();
    }
    let proxy = Arc::new(Mutex::new(event_loop.create_proxy()));
    app.waker = Some(Arc::new(move || {
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(UserEvent::Wake);
        }
    }));
    #[cfg(windows)]
    let (accent, system_dark, blink) = (win::accent().unwrap_or(DEFAULT_ACCENT), win::system_dark().unwrap_or(false), win::caret_blink_ms());
    #[cfg(not(windows))]
    let (accent, system_dark, blink) = (DEFAULT_ACCENT, linux::system_dark().unwrap_or(false), Some(530u32));
    let now = Instant::now();
    let mut shell = Shell {
        seen_edit: app.last_edit,
        app,
        gfx,
        input: Input { caret_on: true, focused_window: true, ..Input::default() },
        widgets: Vec::new(),
        win: None,
        host: WinHost {
            #[cfg(windows)]
            hwnd: std::ptr::null_mut(),
            message: std::cell::RefCell::new(help.then(|| ("rpad".to_string(), USAGE.to_string()))),
        },
        proxy: event_loop.create_proxy(),
        mods: ModifiersState::empty(),
        cursor: None,
        system_dark,
        accent: Color::rgb(accent),
        theme_pref: None,
        #[cfg(windows)]
        menu_recent: Vec::new(),
        pressed: None,
        editor_drag: false,
        hover_since: now,
        blink: blink.map(|ms| Duration::from_millis(ms as u64)),
        blink_at: now,
        focused: true,
        last_autoscroll: now,
        idle_redraw: None,
        ime_rect: None,
        cursor_icon: CursorIcon::Default,
        args: files,
        tray,
        hidden: false,
        maximize_on_show: false,
        menu_dark: None,
        #[cfg(not(windows))]
        shell: None,
        dialog: None,
    };
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rpad: {e}");
    }
}
