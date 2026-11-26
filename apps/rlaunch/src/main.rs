//! rlaunch: a portable application menu.
//!
//! One borderless window, drawn on the CPU, that stays hidden until the
//! notification-area icon, the keyboard shortcut or another launch asks for
//! it, and hides again when something else gets the focus. The folders are
//! read and the icons loaded on threads of their own.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rlaunch::app::{App, Row, View};
use rlaunch::i18n::{self, tf, tr};
use rlaunch::settings::{Hotkey, Settings, SIZES, TRANSPARENCIES};
use rlaunch::sources::{self, Entry};
use rlaunch::storage;
use rlaunch::ui::gfx::{Color, Gfx, Rect};
use rlaunch::ui::theme::Theme;
use rlaunch::ui::view::{self, Drawn, Field, Hit, Model, Place, SIZE};
use rt_ui::Message;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Theme as WinTheme, Window, WindowId, WindowLevel};

/// How long the caret stays on, and off.
const BLINK: Duration = Duration::from_millis(530);

enum UserEvent {
    /// A step of Check for Updates.
    Update(rt_ui::update::Step),
    A11y(accesskit_winit::Event),
    /// The programs of the folders, from the reading with this number.
    Scanned(u64, Vec<Entry>),
    Icon(PathBuf, Option<Box<Pixmap>>),
    /// The keyboard shortcut was typed.
    #[cfg(windows)]
    Hotkey,
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
const IDENT: rt_sys::win::Ident = rt_sys::win::Ident { name: "rlaunch", copydata: 0x524C_4348 }; // "RLCH"

struct Win {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    adapter: accesskit_winit::Adapter,
    #[cfg(windows)]
    hwnd: rt_sys::win::HWND,
}

struct Shell {
    app: App,
    settings: Settings,
    location: storage::Location,
    gfx: Gfx,
    win: Option<Win>,
    proxy: EventLoopProxy<UserEvent>,
    icons: HashMap<PathBuf, Option<Pixmap>>,
    /// Asks the icon thread for the icon of a path.
    wanted: std::sync::mpsc::Sender<PathBuf>,
    drawn: Drawn,
    hot: Option<Hit>,
    cursor: Option<(f32, f32)>,
    mods: ModifiersState,
    system_dark: bool,
    accent: Option<Color>,
    /// Something to tell the user, under the list.
    note: Option<String>,
    /// The keyboard shortcut could not be had.
    hotkey_note: Option<String>,
    dialog: Option<Message>,
    /// With a tray icon, closing hides the menu; without one (a Linux desktop
    /// with no place for it), it quits.
    resident: bool,
    visible: bool,
    focused: bool,
    /// A system dialog is open: losing the focus to it does not hide the menu.
    asking: bool,
    /// Start hidden (`--tray`).
    start_hidden: bool,
    autostart: bool,
    reading: u64,
    disk: Option<(String, u64, u64)>,
    caret: bool,
    caret_at: Instant,
    #[cfg(not(windows))]
    shell: Option<rt_sys::linux::shell::Shell>,
}

/// The folder of rlaunch in portable mode: settings kept there refer to its
/// drive by `*:`.
fn own_dir(location: &storage::Location) -> Option<PathBuf> {
    location.portable.then(|| location.dir.clone()).flatten()
}

/// The drive the footer shows and Explore opens: rlaunch's when portable,
/// else the system's.
fn home_drive(location: &storage::Location) -> PathBuf {
    if cfg!(windows) {
        let dir = own_dir(location);
        match dir.as_deref().and_then(|d| d.components().next()) {
            Some(std::path::Component::Prefix(p)) => PathBuf::from(format!("{}\\", p.as_os_str().to_string_lossy())),
            _ => PathBuf::from(format!("{}\\", std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()))),
        }
    } else {
        own_dir(location).unwrap_or_else(|| PathBuf::from("/"))
    }
}

/// Reads the icons asked for, one at a time, on a thread of its own: some
/// (on network drives, or sleeping disks) take long.
fn icon_thread(proxy: EventLoopProxy<UserEvent>) -> std::sync::mpsc::Sender<PathBuf> {
    let (send, receive) = std::sync::mpsc::channel::<PathBuf>();
    std::thread::spawn(move || {
        #[cfg(windows)]
        // SAFETY: initializes COM on this thread, which only reads icons.
        unsafe {
            windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 2);
        }
        for path in receive {
            #[cfg(windows)]
            let icon = rt_sys::win::icon::of_file(&path).and_then(|(w, h, px)| view::pixmap(w, h, px));
            #[cfg(not(windows))]
            let icon: Option<Pixmap> = None;
            if proxy.send_event(UserEvent::Icon(path, icon.map(Box::new))).is_err() {
                break;
            }
        }
    });
    send
}

/// Opens a program of the menu.
fn open_entry(e: &Entry) -> bool {
    #[cfg(windows)]
    {
        rt_sys::win::launch(&e.path)
    }
    #[cfg(not(windows))]
    {
        match e.exec.split_first() {
            Some((program, args)) => {
                let mut command = std::process::Command::new(program);
                command.args(args);
                if let Some(home) = std::env::var_os("HOME") {
                    command.current_dir(home);
                }
                command.spawn().is_ok()
            }
            None => {
                rt_sys::linux::xdg_open(e.path.clone().into_os_string());
                true
            }
        }
    }
}

fn open_place(place: Place, location: &storage::Location) {
    #[cfg(windows)]
    {
        let target = match place {
            Place::Documents => "shell:Personal".to_string(),
            Place::Downloads => "shell:Downloads".to_string(),
            Place::Pictures => "shell:My Pictures".to_string(),
            Place::Music => "shell:My Music".to_string(),
            Place::Videos => "shell:My Video".to_string(),
            Place::Explore if location.portable => home_drive(location).display().to_string(),
            Place::Explore => "shell:MyComputerFolder".to_string(),
        };
        rt_sys::win::shell_open(&target);
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into());
        let folder = |name: &str| {
            let d = home.join(name);
            if d.is_dir() {
                d
            } else {
                home.clone()
            }
        };
        let target = match place {
            Place::Documents => folder("Documents"),
            Place::Downloads => folder("Downloads"),
            Place::Pictures => folder("Pictures"),
            Place::Music => folder("Music"),
            Place::Videos => folder("Videos"),
            Place::Explore if location.portable => home_drive(location),
            Place::Explore => home.clone(),
        };
        rt_sys::linux::xdg_open(target.into_os_string());
    }
}

impl Shell {
    /// The chosen size of the menu, 1 for the compact one.
    fn zoom(&self) -> f64 {
        f64::from(self.settings.size) / 100.0
    }

    /// Logical pixels of the layout to pixels of the screen.
    fn scale(&self) -> f32 {
        self.win.as_ref().map_or(1.0, |w| w.window.scale_factor() as f32) * self.zoom() as f32
    }

    fn size(&self) -> LogicalSize<f64> {
        LogicalSize::new(SIZE.0 * self.zoom(), SIZE.1 * self.zoom())
    }

    /// Sizes the menu and puts it by the corner of the screen the pointer is
    /// near.
    fn place(&self) {
        let Some(w) = &self.win else { return };
        #[cfg(windows)]
        if let Some((left, top, right, bottom, x, _)) = rt_sys::win::work_area_at_cursor() {
            // Moved first: the size depends on the scale of that screen.
            w.window.set_outer_position(winit::dpi::PhysicalPosition::new(left, top));
            let _ = w.window.request_inner_size(self.size());
            let size = w.window.outer_size();
            let (width, height) = (size.width as i32, size.height as i32);
            let at_x = if x < (left + right) / 2 { left + 8 } else { right - width - 8 };
            w.window.set_outer_position(winit::dpi::PhysicalPosition::new(at_x, (bottom - height - 8).max(top)));
            return;
        }
        let _ = w.window.request_inner_size(self.size());
    }

    /// Applied again after the window is shown (see `set_opacity`).
    fn apply_transparency(&self) {
        #[cfg(windows)]
        if let Some(w) = &self.win {
            let opaque = 100 - u32::from(self.settings.transparency);
            rt_sys::win::set_opacity(w.hwnd, (255 * opaque / 100) as u8);
        }
    }

    fn step_option(&mut self, field: Field, forward: bool) {
        /// The index after `at` (or before) among `n`, around the ends.
        fn next(at: usize, n: usize, forward: bool) -> usize {
            (at + if forward { 1 } else { n - 1 }) % n
        }
        fn pick<T: Copy + PartialEq>(all: &[T], now: T, forward: bool) -> T {
            all[next(all.iter().position(|x| *x == now).unwrap_or(0), all.len(), forward)]
        }
        match field {
            Field::Hotkey => {
                self.settings.hotkey = pick(&Hotkey::ALL, self.settings.hotkey, forward);
                self.apply_hotkey();
            }
            Field::Size => {
                self.settings.size = pick(&SIZES, self.settings.size, forward);
                self.place();
            }
            Field::Transparency => {
                self.settings.transparency = pick(&TRANSPARENCIES, self.settings.transparency, forward);
                self.apply_transparency();
            }
            Field::Theme => self.settings.theme = pick(&[0, 1, 2], self.settings.theme, forward),
            Field::Language => {
                // None (the system's), then each language.
                let at = self.settings.language.and_then(|l| i18n::Lang::ALL.iter().position(|x| *x == l)).map_or(0, |i| i + 1);
                let to = next(at, i18n::Lang::ALL.len() + 1, forward);
                self.settings.language = to.checked_sub(1).map(|i| i18n::Lang::ALL[i]);
                i18n::set_lang(self.settings.language.unwrap_or_else(i18n::system_lang));
                self.gfx.clear_cache();
            }
        }
        self.save();
    }

    fn theme(&self) -> Theme {
        let dark = match self.settings.theme {
            1 => false,
            2 => true,
            _ => self.system_dark,
        };
        Theme::new(dark, self.accent)
    }

    fn redraw(&self) {
        if let Some(w) = &self.win {
            w.window.request_redraw();
        }
    }

    fn save(&mut self) {
        let store = self.settings.to_store(own_dir(&self.location).as_deref());
        if !self.location.save(&store) {
            self.note = self.location.error.clone();
        }
    }

    /// Reads the folders again, on a thread of its own.
    fn scan(&mut self) {
        self.reading += 1;
        let (n, settings, proxy) = (self.reading, self.settings.clone(), self.proxy.clone());
        self.app.loading = self.app.entries.is_empty();
        std::thread::spawn(move || {
            let _ = proxy.send_event(UserEvent::Scanned(n, sources::scan(&settings)));
        });
    }

    /// Registers the keyboard shortcut of the settings.
    fn apply_hotkey(&mut self) {
        #[cfg(windows)]
        {
            let Some(hwnd) = self.win.as_ref().map(|w| w.hwnd) else { return };
            self.hotkey_note = None;
            match self.settings.hotkey.codes() {
                None => rt_sys::win::clear_hotkey(hwnd),
                Some((modifiers, key)) => {
                    let proxy = self.proxy.clone();
                    let pressed = Box::new(move || {
                        let _ = proxy.send_event(UserEvent::Hotkey);
                    });
                    if !rt_sys::win::set_hotkey(hwnd, modifiers, key, pressed) {
                        self.hotkey_note = Some(tf("Another program is using {}.", &[self.settings.hotkey.label()]));
                    }
                }
            }
        }
    }

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title("rlaunch")
            .with_inner_size(self.size())
            .with_resizable(false)
            .with_decorations(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_window_icon(rt_ui::window::icon(rlaunch::ui::appicon::render))
            .with_visible(false);
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_skip_taskbar(true).with_undecorated_shadow(true);
        }
        let Some((window, surface)) = rt_ui::window::create(el, attrs, "rlaunch") else { return false };
        window.set_ime_allowed(true);
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(el, &window, self.proxy.clone());
        #[cfg(windows)]
        let hwnd = rt_ui::window::hwnd_of(&window);
        #[cfg(windows)]
        {
            let proxy = self.proxy.clone();
            rt_sys::win::subclass(
                hwnd,
                &IDENT,
                "",
                Box::new(move |m| {
                    let _ = proxy.send_event(UserEvent::Win(m));
                }),
            );
            rt_sys::win::set_resident(hwnd, &IDENT);
            rt_sys::win::tray::show(hwnd, "rlaunch");
        }
        self.win = Some(Win {
            window,
            surface,
            adapter,
            #[cfg(windows)]
            hwnd,
        });
        true
    }

    /// Opens the menu, as new, by the corner of the screen the pointer is near.
    fn show(&mut self, el: &ActiveEventLoop) {
        if self.win.is_none() && !self.open_window(el) {
            return;
        }
        self.app.reset();
        self.note = None;
        // The help asked at the start stays until closed.
        if self.reading > 0 {
            self.dialog = None;
        }
        self.hot = None;
        self.autostart = rt_sys::autostart::enabled("rlaunch");
        self.disk = rt_scan::volumes::space_of(&home_drive(&self.location))
            .map(|s| (home_drive(&self.location).display().to_string().trim_end_matches('\\').to_string(), s.free, s.total));
        self.scan();
        self.place();
        if let Some(w) = &self.win {
            w.window.set_visible(true);
            rt_ui::window::focus(&w.window);
        }
        self.apply_transparency();
        self.visible = true;
        self.caret = true;
        self.caret_at = Instant::now();
        self.redraw();
    }

    /// Hides the menu; without a tray icon to bring it back, quits.
    fn hide(&mut self, el: &ActiveEventLoop) {
        if !self.resident {
            self.quit(el);
            return;
        }
        if cfg!(windows) {
            if let Some(w) = &self.win {
                w.window.set_visible(false);
            }
        } else {
            // Wayland cannot hide a window, so it is closed and made again.
            self.win = None;
        }
        self.visible = false;
        self.focused = false;
        self.cursor = None;
        // Release the frame buffer while hidden.
        self.gfx.resize(1, 1);
        self.gfx.trim_cache();
    }

    fn quit(&mut self, el: &ActiveEventLoop) {
        #[cfg(windows)]
        if let Some(w) = &self.win {
            rt_sys::win::clear_hotkey(w.hwnd);
            rt_sys::win::tray::remove(w.hwnd);
        }
        #[cfg(not(windows))]
        {
            self.shell = None;
        }
        el.exit();
    }

    fn about(&mut self) {
        let text = tf(
            "rlaunch {}\n\nA portable menu of applications, written in Rust.\nNo telemetry, no network access.\n\nMIT License.",
            &[env!("CARGO_PKG_VERSION")],
        );
        self.dialog = Some(Message::new(&tr("About rlaunch"), &text));
    }

    fn update_sender(&self) -> Box<dyn Fn(rt_ui::update::Step) + Send> {
        let p = std::sync::Mutex::new(self.proxy.clone());
        Box::new(move |s| {
            if let Ok(p) = p.lock() {
                let _ = p.send_event(UserEvent::Update(s));
            }
        })
    }

    fn add_folder(&mut self) {
        self.asking = true;
        #[cfg(windows)]
        let picked = {
            let hwnd = self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd);
            rt_sys::win::dialogs::pick_folder(hwnd, &tr("Choose a folder of programs"))
        };
        #[cfg(not(windows))]
        let picked = rt_sys::linux::dialogs::pick_folder(&tr("Choose a folder of programs"));
        self.asking = false;
        if let Some(folder) = picked {
            if !self.settings.folders.contains(&folder) {
                self.settings.folders.push(folder);
                self.save();
                self.scan();
            }
        }
        if let Some(w) = &self.win {
            rt_ui::window::focus(&w.window);
        }
    }

    fn activate_row(&mut self, el: &ActiveEventLoop, k: usize) {
        match self.app.rows().get(k).copied() {
            Some(Row::Back(_)) => self.app.back(),
            Some(Row::Category(c, _)) => self.app.open_category(c),
            Some(Row::Entry(i)) => {
                let entry = self.app.entries[i].clone();
                if open_entry(&entry) {
                    self.hide(el);
                } else {
                    self.note = Some(tf("Cannot open {}.", &[&entry.name]));
                }
            }
            None => {}
        }
    }

    fn act(&mut self, el: &ActiveEventLoop, hit: Hit) {
        match hit {
            Hit::Row(k) => self.activate_row(el, k),
            Hit::Place(p) => {
                open_place(p, &self.location);
                self.hide(el);
            }
            Hit::Options => {
                self.app.query.clear();
                self.app.view = View::Options;
            }
            Hit::Done => self.app.back(),
            Hit::About => self.about(),
            Hit::Exit => self.quit(el),
            Hit::Close => self.hide(el),
            Hit::Scoop => {
                self.settings.scoop = !self.settings.scoop;
                self.save();
                self.scan();
            }
            Hit::StartMenu => {
                self.settings.start_menu = !self.settings.start_menu;
                self.save();
                self.scan();
            }
            Hit::AddFolder => self.add_folder(),
            Hit::RemoveFolder(i) => {
                if i < self.settings.folders.len() {
                    self.settings.folders.remove(i);
                    self.save();
                    self.scan();
                }
            }
            Hit::Autostart => match rt_sys::autostart::set("rlaunch", !self.autostart) {
                Ok(()) => self.autostart = rt_sys::autostart::enabled("rlaunch"),
                Err(e) => self.note = Some(tf("Cannot change how it starts: {}", &[&e])),
            },
            Hit::Step(field, forward) => self.step_option(field, forward),
            Hit::CheckUpdates => rt_ui::update::check("rlaunch", env!("CARGO_PKG_VERSION"), self.update_sender()),
        }
        self.redraw();
    }

    fn key(&mut self, el: &ActiveEventLoop, event: &KeyEvent) {
        if self.dialog.is_some() {
            if Message::closes_on(&event.logical_key) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        let page = self.drawn.visible.max(1) as isize;
        let command = self.mods.control_key() || self.mods.super_key();
        match &event.logical_key {
            Key::Named(NamedKey::ArrowUp) => self.app.step(-1),
            Key::Named(NamedKey::ArrowDown) => self.app.step(1),
            Key::Named(NamedKey::PageUp) => self.app.step(-page),
            Key::Named(NamedKey::PageDown) => self.app.step(page),
            Key::Named(NamedKey::Home) => self.app.step(isize::MIN / 2),
            Key::Named(NamedKey::End) => self.app.step(isize::MAX / 2),
            Key::Named(NamedKey::Enter) if self.app.view == View::Options => self.app.back(),
            Key::Named(NamedKey::Enter) => {
                self.activate_row(el, self.app.selected);
            }
            Key::Named(NamedKey::ArrowRight) => {
                if let Some(Row::Category(c, _)) = self.app.rows().get(self.app.selected) {
                    self.app.open_category(*c);
                }
            }
            Key::Named(NamedKey::ArrowLeft) if self.app.query.is_empty() => {
                if matches!(self.app.view, View::Inside(_)) {
                    self.app.back();
                }
            }
            Key::Named(NamedKey::Escape) => {
                if !self.app.query.is_empty() {
                    self.app.set_query(String::new());
                } else if self.app.view != View::Categories {
                    self.app.back();
                } else {
                    self.hide(el);
                    return;
                }
            }
            Key::Named(NamedKey::Backspace) => {
                let mut query = self.app.query.clone();
                if query.pop().is_some() {
                    if command {
                        query.clear();
                    }
                    self.app.set_query(query);
                } else if self.app.view != View::Categories {
                    self.app.back();
                }
            }
            Key::Character(c) if command && c.eq_ignore_ascii_case("v") => {
                if let Some(text) = rt_sys::clipboard_text() {
                    self.type_text(text.lines().next().unwrap_or(""));
                }
            }
            _ => {
                // AltGr comes as Ctrl+Alt: those characters are typed.
                let shortcut = self.mods.super_key() || (self.mods.control_key() != self.mods.alt_key());
                if let Some(text) = event.text.as_ref().filter(|_| !shortcut) {
                    self.type_text(text);
                }
            }
        }
        self.app.reveal_selection(self.drawn.visible);
        self.caret = true;
        self.caret_at = Instant::now();
        self.redraw();
    }

    fn type_text(&mut self, text: &str) {
        let typed: String = text.chars().filter(|c| !c.is_control()).collect();
        // A space does not start a search.
        if !typed.is_empty() && !(self.app.query.is_empty() && typed.trim().is_empty()) {
            let query = format!("{}{typed}", self.app.query);
            self.app.set_query(query.chars().take(200).collect());
        }
    }

    fn render(&mut self) {
        let theme = self.theme();
        let s = self.scale();
        let Some(win) = &mut self.win else { return };
        let size = win.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if win.surface.resize(w, h).is_err() {
            return;
        }
        self.gfx.resize(w.get(), h.get());
        let model = Model {
            app: &self.app,
            settings: &self.settings,
            icons: &self.icons,
            note: self.note.as_deref().or(self.hotkey_note.as_deref()),
            autostart: self.autostart,
            hot: self.hot,
            disk: self.disk.clone(),
            caret: self.caret,
        };
        self.drawn = view::draw(&mut self.gfx, &model, &theme, s);
        if let Some(message) = &mut self.dialog {
            let style = rt_gfx::dialog::DialogStyle { dark: theme.dark, text: theme.text, dim: theme.dim, border: theme.border };
            message.draw(&mut self.gfx, Rect::new(0.0, 0.0, w.get() as f32, h.get() as f32), &style, s);
        }
        rt_ui::window::present(&mut win.surface, &self.gfx);
        self.gfx.trim_cache();
        // The icons of what is on screen, once each.
        for &i in &self.drawn.shown {
            let Some(path) = self.app.entries.get(i).map(|e| &e.path) else { continue };
            if !self.icons.contains_key(path) {
                self.icons.insert(path.clone(), None);
                let _ = self.wanted.send(path.clone());
            }
        }
        let tree = self.tree();
        if let Some(w) = &mut self.win {
            w.adapter.update_if_active(|| tree);
        }
    }

    /// What screen readers see: each thing that can be clicked, by name.
    fn tree(&self) -> accesskit::TreeUpdate {
        use accesskit::{Action, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
        let rows = self.app.rows();
        let name = |hit: Hit| -> Option<String> {
            Some(match hit {
                Hit::Row(k) => match *rows.get(k)? {
                    Row::Back(_) => tr("Back"),
                    Row::Category(c, _) => tr(c.name()),
                    Row::Entry(i) => self.app.entries.get(i)?.name.clone(),
                },
                Hit::Options => tr("Options"),
                Hit::About => tr("About"),
                Hit::Exit => tr("Exit"),
                Hit::Done => tr("Done"),
                Hit::AddFolder => tr("Add a folder…"),
                Hit::CheckUpdates => tr("Check for updates"),
                Hit::Autostart => tr("Start with Windows"),
                Hit::Scoop => "Scoop".to_string(),
                Hit::StartMenu => tr("Start menu"),
                Hit::Step(field, forward) => {
                    let option = match field {
                        Field::Hotkey => tr("Keyboard shortcut"),
                        Field::Size => tr("Size"),
                        Field::Transparency => tr("Transparency"),
                        Field::Theme => tr("Theme"),
                        Field::Language => tr("Language"),
                    };
                    format!("{option} {}", if forward { "›" } else { "‹" })
                }
                _ => return None,
            })
        };
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        let mut focus = NodeId(0);
        for (i, (_, hit)) in self.drawn.hits.iter().enumerate() {
            let Some(label) = name(*hit) else { continue };
            let id = NodeId(100 + i as u64);
            let mut n = Node::new(Role::Button);
            n.set_label(label);
            n.add_action(Action::Click);
            if *hit == Hit::Row(self.app.selected) {
                focus = id;
            }
            children.push(id);
            nodes.push((id, n));
        }
        let mut root = Node::new(Role::Window);
        root.set_label("rlaunch");
        root.set_children(children);
        nodes.push((NodeId(0), root));
        TreeUpdate { nodes, tree: Some(TreeInfo::new(NodeId(0))), tree_id: TreeId::ROOT, focus }
    }
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() || self.reading > 0 {
            return;
        }
        #[cfg(not(windows))]
        {
            let proxy = std::sync::Mutex::new(self.proxy.clone());
            let send = Box::new(move |m| {
                if let Ok(p) = proxy.lock() {
                    let _ = p.send_event(UserEvent::Shell(m));
                }
            });
            self.shell = rt_ui::resident::start("rlaunch", rlaunch::ui::appicon::render, tr("Open rlaunch"), tr("Exit"), send);
            self.resident = self.shell.is_some();
        }
        // Linux has no hidden window: it is made when first shown.
        if cfg!(windows) && !self.open_window(el) {
            return;
        }
        self.apply_hotkey();
        // A first reading, so the menu opens full the first time.
        if self.start_hidden && self.resident {
            self.scan();
        } else {
            self.show(el);
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Scanned(n, entries) if n == self.reading => {
                self.app.set_entries(entries);
                self.redraw();
            }
            UserEvent::Scanned(..) => {}
            UserEvent::Icon(path, icon) => {
                self.icons.insert(path, icon.map(|p| *p));
                self.redraw();
            }
            #[cfg(windows)]
            UserEvent::Hotkey => {
                if self.visible && self.focused {
                    self.hide(el);
                } else {
                    self.show(el);
                }
            }
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::OpenFiles(_)) => self.show(el),
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::Tray(rt_sys::win::tray::TrayEvent::Activate)) => self.show(el),
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::Tray(rt_sys::win::tray::TrayEvent::Menu)) => {
                if let Some(hwnd) = self.win.as_ref().map(|w| w.hwnd) {
                    match rt_sys::win::tray::menu(hwnd, &tr("Open rlaunch"), &tr("Exit")) {
                        Some(1) => self.show(el),
                        Some(2) => self.quit(el),
                        _ => {}
                    }
                }
            }
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::SystemTheme) => {
                self.system_dark = rt_sys::system_dark().unwrap_or(false);
                self.accent = rt_sys::accent().map(Color::rgb);
                self.redraw();
            }
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::Command(_)) => {}
            #[cfg(not(windows))]
            UserEvent::Shell(rt_sys::linux::shell::Msg::Open(_)) => self.show(el),
            #[cfg(not(windows))]
            UserEvent::Shell(rt_sys::linux::shell::Msg::Quit) => self.quit(el),
            UserEvent::Update(s) => {
                let who = rt_ui::update::Who {
                    app: "rlaunch",
                    version: env!("CARGO_PKG_VERSION"),
                    #[cfg(windows)]
                    hwnd: self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd),
                };
                self.asking = true;
                let next = rt_ui::update::step(&who, s, self.update_sender());
                self.asking = false;
                if next == rt_ui::update::Next::Quit {
                    self.quit(el);
                }
            }
            UserEvent::A11y(ev) => match ev.window_event {
                accesskit_winit::WindowEvent::ActionRequested(req) if req.action == accesskit::Action::Click => {
                    let hit = req.target_node.0.checked_sub(100).and_then(|i| self.drawn.hits.get(i as usize)).map(|(_, h)| *h);
                    if let Some(hit) = hit {
                        self.act(el, hit);
                    }
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
            WindowEvent::CloseRequested => self.hide(el),
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                // A menu: something else was chosen, so it goes away.
                if !focused && self.visible && self.resident && !self.asking {
                    self.hide(el);
                } else {
                    self.redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } | WindowEvent::Resized(_) => self.redraw(),
            WindowEvent::ThemeChanged(t) => {
                self.system_dark = t == WinTheme::Dark;
                self.redraw();
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } if event.state == ElementState::Pressed => self.key(el, &event),
            WindowEvent::Ime(Ime::Commit(text)) => {
                self.type_text(&text);
                self.redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
                // Only a pointer that moves chooses a row, not one the menu opened under.
                let moved = self.cursor.is_some_and(|c| c != (x, y));
                self.cursor = Some((x, y));
                let hot = if self.dialog.is_none() { self.drawn.hit(x, y) } else { None };
                if let Some(Hit::Row(k)) = hot.filter(|_| moved) {
                    self.app.selected = k;
                }
                if hot != self.hot {
                    self.hot = hot;
                    self.redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                self.hot = None;
                self.redraw();
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                let Some((x, y)) = self.cursor else { return };
                if let Some(message) = &self.dialog {
                    if message.closes_at(x, y) {
                        self.dialog = None;
                        self.redraw();
                    }
                } else if let Some(hit) = self.drawn.hit(x, y) {
                    self.act(el, hit);
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.dialog.is_none() => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 3.0,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / (26.0 * self.scale()),
                };
                self.app.scroll_by(-lines.round() as isize, self.drawn.visible);
                self.redraw();
            }
            WindowEvent::RedrawRequested if self.visible => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if !(self.visible && self.focused) {
            el.set_control_flow(ControlFlow::Wait);
            return;
        }
        if self.caret_at.elapsed() >= BLINK {
            self.caret = !self.caret;
            self.caret_at = Instant::now();
            self.redraw();
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.caret_at + BLINK));
    }
}

const USAGE: &str = "rlaunch [--tray]

  A menu of applications: Scoop's, the Start menu's and those of your own
  folders (on Linux, the installed applications and your folders). It stays
  in the notification area: the icon, Ctrl+Alt+Space (Windows) or starting
  rlaunch again opens it.

  --tray   Start in the notification area, without opening the menu
           (Options > Start with Windows starts it this way).

Settings are stored in the user's settings folder (%APPDATA%\\rlaunch on
Windows, ~/.config/rlaunch on Linux), or next to the program when a file
named rlaunch.portable exists there (portable mode, for a pen drive).";

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
    let tray = raw.iter().any(|a| a == "--tray" || a == "/tray") && !help;
    // One menu: a second launch opens the one running.
    #[cfg(windows)]
    if rt_sys::win::hand_off(&IDENT, &[], false, !tray) {
        return;
    }
    #[cfg(not(windows))]
    if rt_sys::linux::shell::hand_off("rlaunch", &[], false) {
        return;
    }
    let mut location = storage::detect();
    let store = location.load();
    let settings = Settings::load(&store, own_dir(&location).as_deref());
    i18n::set_lang(settings.language.unwrap_or_else(i18n::system_lang));
    let gfx = match Gfx::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rlaunch: {e}");
            std::process::exit(1);
        }
    };
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("rlaunch: cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    let mut shell = Shell {
        app: App::default(),
        settings,
        location,
        gfx,
        win: None,
        proxy: event_loop.create_proxy(),
        icons: HashMap::new(),
        wanted: icon_thread(event_loop.create_proxy()),
        drawn: Drawn::default(),
        hot: None,
        cursor: None,
        mods: ModifiersState::empty(),
        system_dark: rt_sys::system_dark().unwrap_or(false),
        accent: rt_sys::accent().map(Color::rgb),
        note: None,
        hotkey_note: None,
        dialog: help.then(|| Message::new("rlaunch", USAGE)),
        resident: cfg!(windows),
        visible: false,
        focused: false,
        asking: false,
        start_hidden: tray,
        autostart: false,
        reading: 0,
        disk: None,
        caret: true,
        caret_at: Instant::now(),
        #[cfg(not(windows))]
        shell: None,
    };
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rlaunch: {e}");
    }
}
