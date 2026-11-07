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
use winit::window::{CursorIcon, Fullscreen, Icon as WinIcon, Theme as WinTheme, Window, WindowId};

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
        use win::dialogs::{message, Buttons, Icon};
        message(self.hwnd, title, text, Buttons::Ok, Icon::Info);
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
        linux::dialogs::info(title, text);
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
    /// Theme the native menus were last drawn with (dark or not).
    menu_dark: Option<bool>,
    #[cfg(not(windows))]
    shell: Option<linux::shell::Shell>,
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

    /// Pops up a native menu at the mouse position; runs the chosen command.
    #[cfg(windows)]
    fn popup(&mut self, el: &ActiveEventLoop, items: Vec<M>) {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT { x: 0, y: 0 };
        // SAFETY: writes the cursor position into a local POINT.
        unsafe { GetCursorPos(&mut pt) };
        let app = &self.app;
        let chosen = win::menu::popup(self.host.hwnd, items, pt.x, pt.y, |c| app.checked(c), |c| app.enabled(c));
        self.input.active = None;
        self.pressed = None;
        if let Some(c) = chosen {
            self.command(el, c);
        } else {
            self.redraw();
        }
    }

    /// Opens a menu at the mouse position; the choice arrives as input.
    #[cfg(not(windows))]
    fn popup(&mut self, _el: &ActiveEventLoop, items: Vec<M>) {
        let app = &self.app;
        let items = ui::menu::build(items, &|c| app.checked(c), &|c| app.enabled(c));
        let at = self.cursor.unwrap_or((0.0, 0.0));
        self.app.menu.open(None, items, at, false);
        self.input.active = None;
        self.pressed = None;
        self.redraw();
    }

    /// Opens the top-level menu `i` under its title.
    #[cfg(not(windows))]
    fn open_menu(&mut self, i: usize, keyboard: bool) {
        let tree = app::commands::menu_tree(&self.app.settings.recent);
        let Some((_, items)) = tree.into_iter().nth(i) else { return };
        let app = &self.app;
        let items = ui::menu::build(items, &|c| app.checked(c), &|c| app.enabled(c));
        let Some(rect) = self.app.menu.titles.get(i).map(|t| t.rect) else { return };
        self.app.menu.open(Some(i), items, (rect.x, rect.bottom() + 2.0), keyboard);
        self.redraw();
    }

    #[cfg(not(windows))]
    fn menu_act(&mut self, el: &ActiveEventLoop, act: ui::menu::Act) {
        use ui::menu::Act;
        match act {
            Act::Run(c) => {
                self.app.menu.close();
                self.command(el, c);
            }
            Act::Switch(dir) => {
                let n = self.app.menu.titles.len() as isize;
                if let Some(top) = self.app.menu.top {
                    self.open_menu((top as isize + dir).rem_euclid(n) as usize, true);
                }
            }
            Act::None => self.redraw(),
        }
    }

    /// Keyboard for the menus. Returns true when it used the key.
    #[cfg(not(windows))]
    fn menu_key(&mut self, el: &ActiveEventLoop, key: &Key) -> bool {
        if !self.app.menu.is_open() {
            let found = match key {
                Key::F(10) => Some(0),
                Key::Char(c) if self.mods.alt_key() && !self.mods.control_key() => {
                    let k = c.to_lowercase().next();
                    self.app.menu.titles.iter().position(|t| t.mnemonic == k)
                }
                _ => None,
            };
            if let Some(i) = found {
                self.open_menu(i, true);
            }
            return found.is_some();
        }
        let act = match key {
            Key::Up => {
                self.app.menu.up_down(-1);
                ui::menu::Act::None
            }
            Key::Down => {
                self.app.menu.up_down(1);
                ui::menu::Act::None
            }
            Key::Left => self.app.menu.left(),
            Key::Right => self.app.menu.right(),
            Key::Enter | Key::Char(' ') => self.app.menu.enter(),
            Key::Escape => {
                self.app.menu.back();
                ui::menu::Act::None
            }
            Key::Char(c) => self.app.menu.mnemonic(*c),
            _ => ui::menu::Act::None,
        };
        self.menu_act(el, act);
        true
    }

    /// The pointer follows the open menus.
    #[cfg(not(windows))]
    fn menu_pointer(&mut self) {
        if !self.app.menu.is_open() {
            return;
        }
        match self.input.hot.and_then(|h| self.widget(h)).map(|w| w.action.clone()) {
            Some(UiAction::MenuRow(l, i)) => {
                self.app.menu.hover(l, i);
                self.redraw();
            }
            Some(UiAction::MenuTitle(i)) if self.app.menu.top.is_some_and(|t| t != i) => self.open_menu(i, false),
            _ => {}
        }
    }

    fn status_menu(&mut self, el: &ActiveEventLoop, k: u8) {
        use rt_core::menu::M::*;
        let items = match k {
            0 => {
                let mut v = vec![Item(Command::LangAuto), Sep];
                v.extend((0..syntax::lang::DEFS.len()).map(|i| Item(Command::SetLang(i as u8))));
                v
            }
            1 => {
                let all = io::encoding::Enc::all();
                let mut save: Vec<M> = (0..all.len()).map(|i| Item(Command::SetEncoding(i as u8))).collect();
                save.insert(4, Sep);
                let mut reopen: Vec<M> = (0..all.len()).map(|i| Item(Command::ReopenWithEncoding(i as u8))).collect();
                reopen.insert(4, Sep);
                vec![Sub("&Save with encoding", save), Sub("&Reopen with encoding", reopen)]
            }
            2 => vec![Item(Command::SetEol(0)), Item(Command::SetEol(1)), Item(Command::SetEol(2))],
            3 => vec![
                Item(Command::UseSpaces),
                Item(Command::UseTabs),
                Sep,
                Item(Command::TabWidth(2)),
                Item(Command::TabWidth(4)),
                Item(Command::TabWidth(8)),
                Sep,
                Item(Command::IndentToSpaces),
                Item(Command::IndentToTabs),
            ],
            _ => return self.command(el, Command::GotoLine),
        };
        self.popup(el, items);
    }

    fn editor_menu(&mut self, el: &ActiveEventLoop) {
        self.popup(el, app::commands::editor_menu());
    }

    fn tab_menu(&mut self, el: &ActiveEventLoop, i: usize) {
        self.app.activate(i);
        self.redraw();
        self.popup(el, app::commands::tab_menu());
    }

    // ------------------------------------------------------------ keyboard

    fn key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) {
        let ctrl = self.mods.control_key();
        let alt = self.mods.alt_key();
        let shift = self.mods.shift_key();
        let logical = if ctrl || alt {
            use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
            event.key_without_modifiers()
        } else {
            event.logical_key.clone()
        };
        let key = match &logical {
            WKey::Named(n) => match n {
                NamedKey::Enter => Key::Enter,
                NamedKey::Tab => Key::Tab,
                NamedKey::Backspace => Key::Backspace,
                NamedKey::Delete => Key::Delete,
                NamedKey::Escape => Key::Escape,
                NamedKey::ArrowLeft => Key::Left,
                NamedKey::ArrowRight => Key::Right,
                NamedKey::ArrowUp => Key::Up,
                NamedKey::ArrowDown => Key::Down,
                NamedKey::Home => Key::Home,
                NamedKey::End => Key::End,
                NamedKey::PageUp => Key::PageUp,
                NamedKey::PageDown => Key::PageDown,
                NamedKey::Insert => Key::Insert,
                NamedKey::Space => Key::Char(' '),
                NamedKey::F1 => Key::F(1),
                NamedKey::F2 => Key::F(2),
                NamedKey::F3 => Key::F(3),
                NamedKey::F4 => Key::F(4),
                NamedKey::F5 => Key::F(5),
                NamedKey::F6 => Key::F(6),
                NamedKey::F7 => Key::F(7),
                NamedKey::F8 => Key::F(8),
                NamedKey::F9 => Key::F(9),
                #[cfg(not(windows))]
                NamedKey::F10 => Key::F(10),
                NamedKey::F11 => Key::F(11),
                NamedKey::F12 => Key::F(12),
                _ => return,
            },
            WKey::Character(s) => match s.chars().next() {
                Some(c) => Key::Char(c),
                None => return,
            },
            _ => return,
        };
        #[cfg(not(windows))]
        if self.menu_key(el, &key) {
            return;
        }
        let text = event.text.as_ref().map(|t| t.to_string());
        let k = KeyInput { key, ctrl, shift, alt, text };
        self.input.tooltip = None;
        let consumed = self.app.key(&k, &self.host);
        if !consumed {
            if let Some(t) = &k.text {
                // Plain keys and AltGr (reported as Ctrl+Alt) type text;
                // Ctrl+key and Alt+key are shortcuts only.
                if ctrl == alt {
                    self.app.text_input(t, &self.host);
                }
            }
        }
        self.after_action(el);
    }

    // ------------------------------------------------------------ mouse

    fn mouse_press(&mut self, el: &ActiveEventLoop, button: MouseButton) {
        let Some((x, y)) = self.cursor else { return };
        self.input.tooltip = None;
        self.update_hot();
        let hit = self.hit(x, y).cloned();
        #[cfg(not(windows))]
        if self.app.menu.is_open() {
            let on_menu = hit.is_some() || self.app.menu.contains(x, y);
            if button != MouseButton::Left || !on_menu {
                self.app.menu.close();
                return self.redraw();
            }
            if hit.is_none() {
                return;
            }
        }
        match button {
            MouseButton::Left => {
                if self.app.palette.is_some() && hit.is_none() {
                    self.app.palette = None;
                    self.app.focus = Focus::Editor;
                    return self.after_action(el);
                }
                match hit {
                    Some(w) if w.action != UiAction::Editor => {
                        if let UiAction::Field(f) = w.action {
                            self.app.focus = f;
                            self.reset_blink();
                            self.redraw();
                            return;
                        }
                        self.pressed = Some(w.id);
                        self.input.active = Some(w.id);
                        self.redraw();
                    }
                    _ => {
                        let l = &self.app.layout;
                        if l.rect.contains(x, y) || l.hscroll.contains(x, y) {
                            let (c, s, a) = (self.mods.control_key(), self.mods.shift_key(), self.mods.alt_key());
                            self.app.mouse_down(x, y, c, s, a);
                            self.editor_drag = true;
                            self.after_action(el);
                        }
                    }
                }
            }
            MouseButton::Middle => {
                if let Some(Widget { action: UiAction::Tab(i) | UiAction::CloseTab(i), .. }) = hit {
                    self.app.close_doc(i, &self.host);
                    self.after_action(el);
                }
            }
            MouseButton::Right => match hit.map(|w| w.action) {
                Some(UiAction::Tab(i)) => self.tab_menu(el, i),
                Some(UiAction::Editor) | None if self.app.layout.rect.contains(x, y) => {
                    // Keep the selection when right-clicking inside it.
                    if let Some(pos) = self.app.hit_pos(x, y) {
                        let s = self.app.doc().buf.primary_sel();
                        if self.app.doc().buf.sels.len() == 1 && !(pos >= s.start() && pos <= s.end() && !s.is_empty()) {
                            self.app.doc_mut().buf.set_caret(pos);
                        }
                    }
                    self.app.focus = Focus::Editor;
                    self.redraw();
                    self.editor_menu(el);
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn mouse_release(&mut self, el: &ActiveEventLoop) {
        if self.editor_drag {
            self.editor_drag = false;
            self.app.mouse_up();
            self.redraw();
        }
        let pressed = self.pressed.take();
        self.input.active = None;
        self.update_hot();
        if let (Some(p), Some(h)) = (pressed, self.input.hot) {
            if p == h {
                if let Some(w) = self.widget(p).cloned() {
                    return self.action(el, w.action);
                }
            }
        }
        self.redraw();
    }

    fn wheel(&mut self, delta: MouseScrollDelta) {
        let Some((x, y)) = self.cursor else { return };
        let line_h = self.app.layout.line_h.max(1.0);
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, l) => -l * 3.0,
            MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / line_h,
        };
        let over = self.hit(x, y).map(|w| w.action.clone());
        #[cfg(not(windows))]
        if self.app.menu.is_open() {
            if let Some(UiAction::MenuRow(l, _)) = over {
                self.app.menu.wheel(l, lines.signum() as isize * 2);
                self.redraw();
            }
            return;
        }
        match over {
            Some(UiAction::Outline(_)) => {
                self.app.outline_scroll = (self.app.outline_scroll as isize + lines.round() as isize).max(0) as usize;
            }
            Some(UiAction::Result(_)) => {
                if let Some(r) = self.app.results.as_mut() {
                    let n = r.items.len().saturating_sub(1);
                    r.sel = ((r.sel as isize + lines.round() as isize).max(0) as usize).min(n);
                }
            }
            Some(UiAction::PaletteItem(_)) => {
                if let Some(p) = self.app.palette.as_mut() {
                    let n = p.shown.len().saturating_sub(1);
                    p.sel = ((p.sel as isize + lines.signum() as isize).max(0) as usize).min(n);
                }
            }
            Some(UiAction::None) => self.app.preview.scroll += lines * line_h,
            _ => {
                if self.app.layout.rect.contains(x, y) {
                    self.app.wheel(lines, self.mods.control_key(), self.mods.shift_key());
                    if self.mods.control_key() {
                        self.app.menu_dirty = true;
                    }
                }
            }
        }
        self.redraw();
    }

    // ------------------------------------------------------------ frame

    fn render(&mut self) {
        let theme = self.theme();
        if self.app.font_dirty {
            self.app.font_dirty = false;
            #[cfg(windows)]
            let file = win::dialogs::font_file(&self.app.settings.font_face);
            #[cfg(not(windows))]
            let file = linux::font_file(&self.app.settings.font_face);
            if !self.gfx.set_mono(file.as_deref()) {
                self.app.notify(i18n::tf("Font \"{}\" could not be loaded; using the default.", &[&self.app.settings.font_face]));
            }
        }
        let Some(win) = &mut self.win else { return };
        let size = win.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if win.surface.resize(w, h).is_err() {
            return;
        }
        let scale = win.window.scale_factor() as f32;
        self.gfx.resize(w.get(), h.get());
        let mouse = self.cursor.unwrap_or((-1.0, -1.0));
        let mut u = ui::Ui::new(&mut self.gfx, theme, scale, &self.input);
        let ime = ui::view::build(&mut self.app, &mut u, mouse);
        self.widgets = std::mem::take(&mut u.widgets);
        drop(u);
        if let Ok(mut buf) = win.surface.buffer_mut() {
            self.gfx.copy_to(&mut buf);
            let _ = buf.present();
        }
        self.gfx.trim_cache();
        // Title.
        let d = self.app.doc();
        let title = format!("{}{} - rpad", if d.is_modified() { "● " } else { "" }, d.display_name());
        if win.window.title() != title {
            win.window.set_title(&title);
        }
        // IME position.
        if ime != self.ime_rect {
            self.ime_rect = ime;
            if let Some(r) = ime {
                win.window.set_ime_cursor_area(PhysicalPosition::new(r.x as f64, r.y as f64), PhysicalSize::new(r.w as f64, r.h as f64));
            }
        }
        self.sync_menu();
        self.sync_menu_theme(theme.dark);
        if self.update_hot() {
            self.redraw();
        }
        self.update_cursor_icon();
        let tree = self.tree();
        if let Some(win) = &mut self.win {
            win.adapter.update_if_active(|| tree);
        }
    }

    fn update_cursor_icon(&mut self) {
        let icon = match self.cursor {
            Some((x, y)) => {
                let l = &self.app.layout;
                let in_text = l.rect.contains(x, y) && x >= l.text_x - 2.0 && !l.vscroll.contains(x, y);
                let over_widget = self.hit(x, y).is_some_and(|w| w.action != UiAction::Editor);
                match self.hit(x, y).map(|w| w.role) {
                    Some(Role::TextField) => CursorIcon::Text,
                    _ if in_text && !over_widget => CursorIcon::Text,
                    _ => CursorIcon::Default,
                }
            }
            None => CursorIcon::Default,
        };
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(w) = &self.win {
                w.window.set_cursor(icon);
            }
        }
    }

    #[cfg(windows)]
    fn sync_menu(&mut self) {
        let Some(win) = &mut self.win else { return };
        if !self.app.menu_dirty && win.menu.is_some() {
            return;
        }
        self.app.menu_dirty = false;
        if win.menu.is_none() || self.menu_recent != self.app.settings.recent || self.app.menu_rebuild {
            self.app.menu_rebuild = false;
            self.menu_recent = self.app.settings.recent.clone();
            let hwnd = self.host.hwnd;
            let tree = app::commands::menu_tree(&self.menu_recent);
            win::set_mnemonics(&mnemonics_of(&tree));
            win.menu = Some(win::menu::MenuBar::install(hwnd, tree));
        }
        if let Some(m) = &win.menu {
            let app = &self.app;
            m.sync(|c| app.checked(c), |c| app.enabled(c));
        }
    }

    /// The titles of the drawn menu bar, in the current language.
    #[cfg(not(windows))]
    fn sync_menu(&mut self) {
        if !self.app.menu.titles.is_empty() && !self.app.menu_rebuild {
            return;
        }
        self.app.menu_rebuild = false;
        self.app.menu.titles = app::commands::menu_tree(&[])
            .iter()
            .map(|(title, _)| {
                let (label, mnemonic, mark) = ui::menu::parse_label(&i18n::tr(title));
                ui::menu::Title { label, mnemonic, mark, rect: Rect::new(0.0, 0.0, 0.0, 0.0) }
            })
            .collect();
        self.redraw();
    }

    /// Dark menu bar and popup menus in the dark theme.
    fn sync_menu_theme(&mut self, dark: bool) {
        if self.menu_dark == Some(dark) {
            return;
        }
        self.menu_dark = Some(dark);
        #[cfg(windows)]
        {
            let colors = dark.then_some(win::darkmode::MenuColors { bar: 0x202020, text: 0xE4E4E4, text_disabled: 0x7A7A7A, hot: 0x3D3D3D });
            win::darkmode::apply(self.host.hwnd, colors);
        }
    }

    fn tree(&self) -> accesskit::TreeUpdate {
        use accesskit::{Node, NodeId, Rect as ARect, Role as ARole, Toggled, TreeId, TreeInfo, TreeUpdate};
        let root = NodeId(0);
        let layer = self.top_layer();
        let mut nodes = Vec::with_capacity(self.widgets.len() + 1);
        let mut children = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for w in &self.widgets {
            if w.layer != layer || w.label.is_empty() || !seen.insert(w.id) {
                continue;
            }
            let role = match w.role {
                Role::Button | Role::Toggle => ARole::Button,
                Role::Tab => ARole::Tab,
                Role::TextField => ARole::TextInput,
                Role::ListItem => ARole::ListItem,
                Role::Label => ARole::Label,
                Role::Editor => ARole::MultilineTextInput,
            };
            let mut n = Node::new(role);
            n.set_label(w.label.clone());
            n.set_bounds(ARect { x0: w.rect.x as f64, y0: w.rect.y as f64, x1: w.rect.right() as f64, y1: w.rect.bottom() as f64 });
            if let Some(v) = &w.value {
                n.set_value(v.clone());
            } else if w.role == Role::Label {
                n.set_value(w.label.clone());
            }
            if w.role != Role::Label {
                n.add_action(accesskit::Action::Focus);
                n.add_action(accesskit::Action::Click);
            }
            if !w.enabled {
                n.set_disabled();
            }
            match (w.role, w.checked) {
                (Role::Toggle, Some(c)) => n.set_toggled(if c { Toggled::True } else { Toggled::False }),
                (Role::Tab | Role::ListItem, Some(c)) => n.set_selected(c),
                _ => {}
            }
            children.push(NodeId(w.id));
            nodes.push((NodeId(w.id), n));
        }
        let mut r = Node::new(ARole::Window);
        r.set_label("rpad");
        r.set_children(children);
        nodes.push((root, r));
        let focus_action = match self.app.focus {
            Focus::Editor => UiAction::Editor,
            f => UiAction::Field(f),
        };
        let fid = ui::wid(&focus_action);
        let focus = if seen.contains(&fid) { NodeId(fid) } else { root };
        TreeUpdate { nodes, tree: Some(TreeInfo::new(root)), tree_id: TreeId::ROOT, focus }
    }

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

/// Window icon (same artwork as the embedded .exe icon).
fn app_icon() -> Option<WinIcon> {
    let n = 32;
    WinIcon::from_rgba(ui::appicon::render(n), n, n).ok()
}

/// Access keys of the top-level menus in the current language.
#[cfg(windows)]
fn mnemonics_of(tree: &[(&'static str, Vec<M>)]) -> String {
    tree.iter().filter_map(|(t, _)| i18n::tr(t).split('&').nth(1).and_then(|s| s.chars().next())).collect()
}

#[cfg(windows)]
fn hwnd_of(window: &Window) -> windows_sys::Win32::Foundation::HWND {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get() as _,
        _ => std::ptr::null_mut(),
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
            .with_maximized(maximized)
            .with_window_icon(app_icon())
            .with_theme(theme_for(self.app.settings.theme))
            .with_visible(false);
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("rpad: cannot create window: {e}");
                el.exit();
                return false;
            }
        };
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(el, &window, self.proxy.clone());
        let surface = softbuffer::Context::new(window.clone()).and_then(|ctx| softbuffer::Surface::new(&ctx, window.clone()));
        let surface = match surface {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rpad: cannot create drawing surface: {e}");
                el.exit();
                return false;
            }
        };
        window.set_ime_allowed(true);
        #[cfg(windows)]
        {
            self.host.hwnd = hwnd_of(&window);
            let proxy = self.proxy.clone();
            let mnemonics = mnemonics_of(&app::commands::menu_tree(&[]));
            win::subclass(
                self.host.hwnd,
                &mnemonics,
                Box::new(move |m| {
                    let _ = proxy.send_event(UserEvent::Win(m));
                }),
            );
        }
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

Settings are stored in %APPDATA%\\rpad (~/.config/rpad on Linux), or next
to the program when a file named rpad.portable exists there (portable mode).";

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.iter().any(|a| a == "--help" || a == "-h" || a == "/?") {
        #[cfg(windows)]
        win::dialogs::message(std::ptr::null_mut(), "rpad", USAGE, win::dialogs::Buttons::Ok, win::dialogs::Icon::Info);
        #[cfg(not(windows))]
        println!("{USAGE}");
        return;
    }
    let new_window = raw.iter().any(|a| a == "--new-window");
    let tray = raw.iter().any(|a| a == "--tray" || a == "/tray");
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
        menu_dark: None,
        #[cfg(not(windows))]
        shell: None,
    };
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rpad: {e}");
    }
}
