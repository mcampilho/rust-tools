//! rlaunch: a portable application menu.
//!
//! One borderless window, drawn on the CPU, that stays hidden until the
//! notification-area icon, the keyboard shortcut or another launch asks for
//! it, and hides again when something else gets the focus. The folders are
//! read and the icons loaded on threads of their own.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rlaunch::app::{App, Row, View};
use rlaunch::category::Group;
use rlaunch::i18n::{self, tf, tr};
use rlaunch::items::Custom;
use rlaunch::portableapps;
use rlaunch::settings::{FolderMode, Hotkey, Position, Settings, SIZES, TRANSPARENCIES};
use rlaunch::sources::{self, Entry};
use rlaunch::storage;
use rlaunch::ui::gfx::{Color, Gfx, Rect};
use rlaunch::ui::theme::{self, Theme};
use rlaunch::ui::view::{self, Drawn, DropMark, Editing, Field, Hit, Model, Place, SIZE};
use rt_core::textedit::TextEdit;
use rt_gfx::menu::{Act, Item, Menu, MenuStyle};
use rt_ui::Message;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Theme as WinTheme, Window, WindowId, WindowLevel};

/// How long the caret stays on, and off.
const BLINK: Duration = Duration::from_millis(530);

/// What the menu of a program (right click) does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cmd {
    Open,
    #[cfg(windows)]
    Admin,
    #[cfg(windows)]
    AlwaysAdmin,
    Reveal,
    Favourite,
    Rename,
    Category(Group),
    NewCategory,
    RenameGroup,
    RemoveGroup,
    /// Makes the category a subgroup of this one.
    MoveInto(Group),
    /// Makes the subgroup a category.
    Promote,
    Up,
    Down,
    #[cfg(windows)]
    Console,
    ChangeIcon,
    DefaultIcon,
    Hide,
    Remove,
}

/// The ids of the rows of that menu, for screen readers.
const MENU_NODE: u64 = 10_000;

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

/// Given to the instance started by [`Shell::elevate`].
#[cfg(windows)]
const ELEVATED_RESTART: &str = "--elevated-restart";

/// What the pointer can drag.
enum Dragged {
    Programs(Vec<PathBuf>),
    Group(Group),
}

/// A button held down on a row, which becomes a drag when the pointer moves.
struct Press {
    what: Dragged,
    at: (f32, f32),
    dragging: bool,
}

/// Programs opened in a console are of these kinds.
#[cfg(windows)]
fn takes_console(path: &Path) -> bool {
    path.extension().is_some_and(|e| ["exe", "bat", "cmd"].iter().any(|k| e.eq_ignore_ascii_case(k)))
}

struct Win {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    adapter: accesskit_winit::Adapter,
    #[cfg(windows)]
    hwnd: rt_sys::win::HWND,
}

struct Shell {
    app: App,
    /// The programs as the folders give them, before the user's changes.
    raw: Vec<Entry>,
    /// The menu of a program, and which.
    menu: Menu<Cmd>,
    menu_for: PathBuf,
    /// The category or subgroup whose menu it is, instead of a program.
    menu_group: Option<Group>,
    /// A name being typed in a row.
    rename: Option<(Editing, TextEdit)>,
    press: Option<Press>,
    /// Where what is dragged would go.
    drop: Option<(usize, DropMark)>,
    /// Stays open when the focus goes elsewhere.
    pinned: bool,
    /// Running with administrator rights.
    elevated: bool,
    /// The colours of `theme.ini`, read each time the menu opens.
    colours: storage::Store,
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

/// A PortableApps suite beside the folder of rlaunch, or at the usual places
/// of its drive.
fn suite_near(own: &Path) -> Option<PathBuf> {
    let drive = own.ancestors().last()?;
    [own.parent(), Some(drive), Some(&drive.join("Portable"))].into_iter().flatten().find_map(portableapps::suite_in)
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
        #[cfg(not(windows))]
        let dirs = rlaunch::xdg::IconDirs::system();
        for path in receive {
            let icon = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
                std::fs::read(&path).ok().and_then(|b| Pixmap::decode_png(&b).ok())
            } else {
                #[cfg(windows)]
                {
                    rt_sys::win::icon::of_file(&path).and_then(|(w, h, px)| view::pixmap(w, h, px))
                }
                #[cfg(not(windows))]
                {
                    rlaunch::ui::linux_icon::of(&dirs, &path)
                }
            };
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
                // A program of a folder starts in it, as portable ones expect.
                let own = (e.path.as_os_str() == program.as_str()).then(|| e.path.parent()).flatten();
                if let Some(dir) = own.map(Path::to_path_buf).or_else(|| std::env::var_os("HOME").map(PathBuf::from)) {
                    command.current_dir(dir);
                }
                match command.spawn() {
                    Ok(mut child) => {
                        std::thread::spawn(move || child.wait());
                        true
                    }
                    Err(_) => false,
                }
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
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
        let dirs = std::fs::read_to_string(config.join("user-dirs.dirs")).unwrap_or_default();
        let folder = |name: &str| {
            let key = if name == "Downloads" { "DOWNLOAD".to_string() } else { name.to_uppercase() };
            let d = rlaunch::xdg::user_dir(&dirs, &key, &home).unwrap_or_else(|| home.join(name));
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

    /// Sizes the menu and puts it where Options say, on the screen the pointer
    /// is on.
    fn place(&self) {
        let Some(w) = &self.win else { return };
        // X11 puts the window where asked; Wayland decides by itself. The
        // pointer is not known, and a panel is left room at the bottom.
        #[cfg(not(windows))]
        if let Some(screen) = w.window.current_monitor().or_else(|| w.window.primary_monitor()) {
            let _ = w.window.request_inner_size(self.size());
            let (at, size, own) = (screen.position(), screen.size(), w.window.outer_size());
            let area = (at.x, at.y, at.x + size.width as i32, at.y + size.height as i32 - 40);
            let centre = ((area.0 + area.2) / 2, (area.1 + area.3) / 2);
            let size = (own.width as i32, own.height as i32);
            let (x, y) = self.settings.position.place(area, size, (centre.0 - size.0 / 2, centre.1 - size.1 / 2));
            w.window.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
        }
        #[cfg(windows)]
        if let Some((left, top, right, bottom, x, y)) = rt_sys::win::work_area_at_cursor() {
            // Moved first: the size depends on the scale of that screen.
            w.window.set_outer_position(winit::dpi::PhysicalPosition::new(left, top));
            let _ = w.window.request_inner_size(self.size());
            let size = w.window.outer_size();
            let (width, height) = (size.width as i32, size.height as i32);
            let (at_x, at_y) = self.settings.position.place((left, top, right, bottom), (width, height), (x, y));
            w.window.set_outer_position(winit::dpi::PhysicalPosition::new(at_x, at_y));
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
            Field::Rows => self.settings.large = !self.settings.large,
            Field::Order => {
                self.settings.custom_order = !self.settings.custom_order;
                self.refresh(None);
            }
            // Applies when the menu opens next: it does not jump while chosen.
            Field::Position => {
                // Elsewhere than on Windows the pointer is not known.
                let all = if cfg!(windows) { &Position::ALL[..] } else { &Position::ALL[..3] };
                self.settings.position = pick(all, self.settings.position, forward);
            }
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
        Theme::new(dark, self.accent, self.settings.colour).with(&self.colours)
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
            // A normal start still opens an elevated menu: it only shows it.
            if self.elevated {
                rt_sys::win::accept_hand_off(hwnd);
            }
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
        self.colours = self
            .location
            .dir
            .as_ref()
            .and_then(|d| std::fs::read_to_string(d.join("theme.ini")).ok())
            .map(|t| storage::Store::parse(&t))
            .unwrap_or_default();
        self.menu.close();
        self.rename = None;
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
        self.menu.close();
        self.rename = None;
        self.press = None;
        self.drop = None;
        self.pinned = false;
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

    /// Starts again with administrator rights, in place of this instance.
    #[cfg(windows)]
    fn elevate(&mut self, el: &ActiveEventLoop) {
        self.asking = true;
        let started = rt_sys::win::restart_elevated(ELEVATED_RESTART);
        self.asking = false;
        // Refused: everything stays as it was.
        if started {
            self.quit(el);
        }
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

    /// Adds a folder of programs; a PortableApps one brings its menu's choices.
    fn take_folder(&mut self, folder: PathBuf) {
        let taken = portableapps::suite_in(&folder).map_or(0, |suite| portableapps::import(&suite, &mut self.settings.items));
        if !self.settings.folders.contains(&folder) {
            self.settings.folders.push(folder);
        }
        self.save();
        self.scan();
        if taken > 0 {
            self.note = Some(tf("Took {} choices from PortableApps.", &[&taken.to_string()]));
        }
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
            self.take_folder(folder);
        }
        if let Some(w) = &self.win {
            rt_ui::window::focus(&w.window);
        }
    }

    /// The programs with the user's changes, keeping `keep` selected.
    fn refresh(&mut self, keep: Option<&Path>) {
        self.app.custom_order = self.settings.custom_order;
        self.app.order = self.settings.items.order().to_vec();
        self.app.set_entries(self.settings.items.apply(&self.raw));
        let rows = self.app.rows();
        let at = keep.and_then(|p| rows.iter().position(|r| matches!(r, Row::Entry(i) if self.app.entries[*i].path == p)));
        if let Some(at) = at {
            self.app.selected = at;
            self.app.reveal_selection(self.drawn.visible);
        }
    }

    /// Changes what is kept about a program, and shows it.
    fn change(&mut self, path: &Path, edit: impl FnOnce(&mut Custom)) {
        self.settings.items.change(path, edit);
        self.save();
        self.refresh(Some(path));
    }

    fn entry(&self, path: &Path) -> Option<&Entry> {
        self.app.entries.iter().find(|e| e.path == path)
    }

    /// Opens a program; `admin`: with administrator rights (Windows).
    fn launch(&mut self, el: &ActiveEventLoop, entry: &Entry, admin: bool) {
        #[cfg(windows)]
        let opened = if takes_console(&entry.path) && entry.console.unwrap_or_else(|| sources::console_program(&entry.path)) {
            // So that what it writes can be read, and it can be run again.
            rt_sys::win::launch_in_console(&entry.path, admin)
        } else if admin {
            rt_sys::win::launch_as_admin(&entry.path)
        } else {
            open_entry(entry)
        };
        #[cfg(not(windows))]
        let opened = {
            let _ = admin;
            open_entry(entry)
        };
        if !opened {
            self.note = Some(tf("Cannot open {}.", &[&entry.name]));
        } else if !self.pinned {
            self.hide(el);
        }
    }

    /// The menu of the program in row `k`, at `at` (pixels of the window).
    fn open_menu(&mut self, k: usize, at: (f32, f32), keyboard: bool) {
        let item = |label: String, cmd: Cmd| Item::new(&label.replace('&', "&&"), Some(cmd), true);
        let i = match self.app.rows().get(k) {
            Some(&Row::Entry(i)) => i,
            Some(&Row::Category(group, _)) => {
                let items = self.group_menu(group);
                if !items.is_empty() {
                    self.app.clear_marks();
                    self.menu_group = Some(group);
                    self.app.selected = k;
                    self.menu.open(None, items, at, keyboard);
                }
                return;
            }
            _ => return,
        };
        let e = &self.app.entries[i];
        self.menu_group = None;
        // On one of the marked programs: what can be done to all of them.
        if self.app.marked.contains(&e.path) {
            let mut to = Item::new(&tf("Move {} programs to", &[&self.app.marked.len().to_string()]), None, true);
            to.sub = self.category_items(None);
            let items = vec![to, item(tr("Add to favorites"), Cmd::Favourite), item(tr("Hide"), Cmd::Hide)];
            self.menu_for = e.path.clone();
            self.app.selected = k;
            let at = (at.0.min(24.0 * self.scale()), at.1);
            self.menu.open(None, items, at, keyboard);
            return;
        }
        let group = e.group();
        self.app.clear_marks();
        let e = &self.app.entries[i];
        let mut items = vec![item(tr("Open"), Cmd::Open)];
        #[cfg(windows)]
        {
            items.push(item(tr("Run as administrator"), Cmd::Admin));
            let mut always = item(tr("Always run as administrator"), Cmd::AlwaysAdmin);
            always.checked = e.admin;
            items.push(always);
        }
        items.push(item(tr("Open file location"), Cmd::Reveal));
        items.push(Item::separator());
        let favourite = if e.favourite { tr("Remove from favorites") } else { tr("Add to favorites") };
        items.push(item(favourite, Cmd::Favourite));
        items.push(item(tr("Rename"), Cmd::Rename));
        let mut categories = Item::new(&tr("Category"), None, true);
        categories.sub = self.category_items(Some(group));
        items.push(categories);
        #[cfg(windows)]
        if takes_console(&e.path) {
            let mut console = item(tr("Keep the console open"), Cmd::Console);
            console.checked = e.console.unwrap_or_else(|| sources::console_program(&e.path));
            items.push(console);
        }
        items.push(item(tr("Change icon…"), Cmd::ChangeIcon));
        if e.icon.is_some() {
            items.push(item(tr("Default icon"), Cmd::DefaultIcon));
        }
        items.push(Item::separator());
        items.push(item(if e.hidden { tr("Show again") } else { tr("Hide") }, Cmd::Hide));
        if e.added {
            items.push(item(tr("Remove from the menu"), Cmd::Remove));
        }
        self.menu_for = e.path.clone();
        self.app.selected = k;
        // Towards the left, so the categories fit beside it.
        let at = (at.0.min(24.0 * self.scale()), at.1);
        self.menu.open(None, items, at, keyboard);
    }

    /// The groups a program can be moved to, `checked` the one it is in.
    fn category_items(&self, checked: Option<Group>) -> Vec<Item<Cmd>> {
        let item = |label: String, cmd: Cmd| Item::new(&label.replace('&', "&&"), Some(cmd), true);
        self.app
            .groups()
            .into_iter()
            .map(|g| {
                let mut it = item(g.full_title(), Cmd::Category(g));
                it.checked = Some(g) == checked;
                it
            })
            .chain([Item::separator(), item(tr("New category…"), Cmd::NewCategory)])
            .collect()
    }

    /// The menu of a category or subgroup; empty when nothing can be done.
    fn group_menu(&self, group: Group) -> Vec<Item<Cmd>> {
        let item = |label: String, cmd: Cmd| Item::new(&label.replace('&', "&&"), Some(cmd), true);
        let mut items = Vec::new();
        if group.is_own() {
            items.push(item(tr("Rename"), Cmd::RenameGroup));
            if group.sub.is_some() {
                items.push(item(tr("Make it a category"), Cmd::Promote));
            } else {
                let mut into = Item::new(&tr("Move into"), None, true);
                into.sub = self
                    .app
                    .groups()
                    .into_iter()
                    .filter(|g| g.sub.is_none() && g.category != group.category)
                    .map(|g| item(g.title(), Cmd::MoveInto(g)))
                    .collect();
                items.push(into);
            }
        }
        if self.settings.custom_order {
            items.extend([item(tr("Move up"), Cmd::Up), item(tr("Move down"), Cmd::Down)]);
        }
        if group.is_own() {
            items.extend([Item::separator(), item(tr("Remove category"), Cmd::RemoveGroup)]);
        }
        items
    }

    /// Selects the row of `group`, if shown.
    fn select_group(&mut self, group: Group) {
        let rows = self.app.rows();
        if let Some(k) = rows.iter().position(|r| matches!(r, Row::Category(g, _) if *g == group)) {
            self.app.selected = k;
            self.app.reveal_selection(self.drawn.visible);
        }
    }

    /// Renames a category or subgroup, or moves it to another level.
    fn move_group(&mut self, from: Group, to: Group) {
        self.settings.items.rename_group(from, to);
        self.save();
        self.refresh(None);
        self.select_group(to);
    }

    /// `group` as a subgroup of the category `inside`.
    fn move_into(&mut self, group: Group, inside: Group) {
        let to = Group { category: inside.category, sub: Some(rlaunch::category::intern(&group.title())) };
        self.move_group(group, to);
        self.select_group(to.top());
    }

    fn promote(&mut self, group: Group) {
        self.move_group(group, Group::from(rlaunch::category::Category::named(&group.title())));
    }

    fn reorder(&mut self, group: Group, siblings: Option<Vec<Group>>) {
        if let Some(siblings) = siblings {
            self.settings.items.set_order(&siblings);
            self.save();
            self.refresh(None);
            self.select_group(group);
        }
    }

    /// Where dropping at `(x, y)` would put what is dragged.
    fn drop_at(&self, what: &Dragged, x: f32, y: f32) -> Option<(usize, DropMark)> {
        let (r, k) = self.drawn.hits.iter().rev().find_map(|(r, h)| match h {
            Hit::Row(k) if r.contains(x, y) => Some((*r, *k)),
            _ => None,
        })?;
        let mark = match (what, *self.app.rows().get(k)?) {
            (Dragged::Programs(_), Row::Category(..)) => DropMark::Into,
            // Up from a subgroup to its category.
            (Dragged::Programs(_), Row::Back) if matches!(self.app.view, View::Inside(g) if g.sub.is_some()) => DropMark::Into,
            (Dragged::Group(g), Row::Back) if g.sub.is_some() => DropMark::Into,
            (&Dragged::Group(g), Row::Category(t, _)) if t != g => {
                let into = g.is_own() && g.sub.is_none() && t.sub.is_none();
                let beside = self.settings.custom_order && g.sub.is_some() == t.sub.is_some();
                let at = (y - r.y) / r.h.max(1.0);
                // The edges of a row put it beside, the middle inside.
                let edge = if into { 0.3 } else { 0.5 };
                match (beside, into) {
                    (true, _) if at < edge => DropMark::Before,
                    (true, _) if at >= 1.0 - edge => DropMark::After,
                    (_, true) => DropMark::Into,
                    _ => return None,
                }
            }
            _ => return None,
        };
        Some((k, mark))
    }

    fn drop_on(&mut self, what: Dragged, k: usize, mark: DropMark) {
        let Some(row) = self.app.rows().get(k).copied() else { return };
        match (what, row) {
            (Dragged::Programs(paths), Row::Category(to, _)) => self.move_many(&paths, to),
            (Dragged::Programs(paths), Row::Back) => {
                if let View::Inside(g) = self.app.view {
                    self.move_many(&paths, g.top());
                }
            }
            (Dragged::Group(g), Row::Back) => self.promote(g),
            (Dragged::Group(g), Row::Category(t, _)) => match mark {
                DropMark::Into => self.move_into(g, t),
                beside => {
                    let siblings = self.app.reordered(g, t, beside == DropMark::After);
                    self.reorder(g, siblings);
                }
            },
            _ => {}
        }
    }

    /// A button went down on a row: it may become a drag.
    fn press_row(&mut self, x: f32, y: f32) {
        let Some(Hit::Row(k)) = self.drawn.hit(x, y) else { return };
        let what = match self.app.rows().get(k) {
            Some(&Row::Entry(i)) => {
                let path = self.app.entries[i].path.clone();
                if self.app.marked.contains(&path) {
                    Dragged::Programs(self.app.marked.iter().cloned().collect())
                } else {
                    Dragged::Programs(vec![path])
                }
            }
            Some(&Row::Category(g, _)) => Dragged::Group(g),
            _ => return,
        };
        self.press = Some(Press { what, at: (x, y), dragging: false });
    }

    /// The pointer moved with a button held; true while something is dragged.
    fn drag(&mut self, x: f32, y: f32) -> bool {
        let far = 6.0 * self.scale();
        let Some(press) = &mut self.press else { return false };
        press.dragging |= (x - press.at.0).abs() > far || (y - press.at.1).abs() > far;
        if !press.dragging {
            return false;
        }
        let Some(press) = self.press.take() else { return false };
        self.drop = self.drop_at(&press.what, x, y);
        self.press = Some(press);
        self.hot = None;
        if let Some(w) = &self.win {
            w.window.set_cursor(if self.drop.is_some() { CursorIcon::Grabbing } else { CursorIcon::NotAllowed });
        }
        self.redraw();
        true
    }

    /// The button went up; true when it ended a drag.
    fn end_drag(&mut self) -> bool {
        let press = self.press.take();
        let drop = self.drop.take();
        let Some(press) = press.filter(|p| p.dragging) else { return false };
        if let Some(w) = &self.win {
            w.window.set_cursor(CursorIcon::Default);
        }
        if let Some((k, mark)) = drop {
            self.drop_on(press.what, k, mark);
        }
        self.redraw();
        true
    }

    /// A click on a row with Ctrl or Shift marks programs; with programs
    /// marked, a plain click only unmarks them. True when it did.
    fn mark_click(&mut self, k: usize) -> bool {
        if !matches!(self.app.rows().get(k), Some(Row::Entry(_))) {
            return false;
        }
        // Ctrl+Shift runs as administrator.
        match (self.mods.control_key() || self.mods.super_key(), self.mods.shift_key()) {
            (true, false) => self.app.toggle_mark(k),
            (false, true) => self.app.mark_to(k),
            (false, false) if !self.app.marked.is_empty() => self.app.clear_marks(),
            _ => return false,
        }
        self.redraw();
        true
    }

    /// [`Self::open_menu`] from the keyboard, under the selected row.
    fn menu_at_selection(&mut self) {
        let k = self.app.selected;
        if let Some((r, _)) = self.drawn.hits.iter().find(|(_, h)| *h == Hit::Row(k)) {
            self.open_menu(k, (r.x + r.h, r.bottom()), true);
        }
    }

    fn menu_act(&mut self, el: &ActiveEventLoop, act: Act<Cmd>) {
        if let Act::Run(cmd) = act {
            self.menu.close();
            self.run(el, cmd);
        }
        self.redraw();
    }

    /// What the menu of a category or subgroup does.
    fn run_on_group(&mut self, group: Group, cmd: Cmd) {
        match cmd {
            Cmd::RenameGroup => {
                let mut edit = TextEdit::new(&group.title());
                edit.select_all();
                self.rename = Some((Editing::Group(group), edit));
            }
            Cmd::RemoveGroup => {
                self.settings.items.remove_group(group);
                self.save();
                self.refresh(None);
            }
            Cmd::MoveInto(to) => self.move_into(group, to),
            Cmd::Promote => self.promote(group),
            Cmd::Up | Cmd::Down => {
                let siblings = self.app.shifted(group, cmd == Cmd::Down);
                self.reorder(group, siblings);
            }
            _ => {}
        }
    }

    /// Puts programs in a group the user chose.
    fn move_many(&mut self, paths: &[PathBuf], group: Group) {
        self.settings.items.keep_group(group);
        for path in paths {
            self.settings.items.change(path, |c| c.category = Some(group));
        }
        self.save();
        self.app.clear_marks();
        self.refresh(None);
    }

    /// Puts a program in a group the user chose.
    fn move_to(&mut self, path: &Path, group: Group) {
        self.settings.items.keep_group(group);
        self.change(path, |c| c.category = Some(group));
    }

    fn run(&mut self, el: &ActiveEventLoop, cmd: Cmd) {
        if let Some(group) = self.menu_group.take() {
            self.run_on_group(group, cmd);
            return;
        }
        let path = self.menu_for.clone();
        let Some(entry) = self.entry(&path).cloned() else { return };
        // The menu of the marked programs.
        if self.app.marked.contains(&path) {
            let paths: Vec<PathBuf> = self.app.marked.iter().cloned().collect();
            match cmd {
                Cmd::Category(to) => self.move_many(&paths, to),
                Cmd::NewCategory => self.rename = Some((Editing::NewCategory(path), TextEdit::new(""))),
                Cmd::Favourite | Cmd::Hide => {
                    for p in &paths {
                        self.settings.items.change(p, |c| if cmd == Cmd::Hide { c.hidden = true } else { c.favourite = true });
                    }
                    self.save();
                    self.app.clear_marks();
                    self.refresh(None);
                }
                _ => {}
            }
            return;
        }
        match cmd {
            Cmd::Open => self.launch(el, &entry, entry.admin),
            #[cfg(windows)]
            Cmd::Admin => self.launch(el, &entry, true),
            #[cfg(windows)]
            Cmd::AlwaysAdmin => self.change(&path, |c| c.admin = !c.admin),
            Cmd::Reveal => {
                rt_sys::reveal(&path);
                if !self.pinned {
                    self.hide(el);
                }
            }
            Cmd::Favourite => self.change(&path, |c| c.favourite = !c.favourite),
            Cmd::Rename => {
                let mut edit = TextEdit::new(&entry.name);
                edit.select_all();
                self.rename = Some((Editing::Name(path), edit));
            }
            Cmd::Category(to) => self.move_to(&path, to),
            Cmd::NewCategory => self.rename = Some((Editing::NewCategory(path), TextEdit::new(""))),
            Cmd::RenameGroup | Cmd::RemoveGroup | Cmd::MoveInto(_) | Cmd::Promote | Cmd::Up | Cmd::Down => {}
            #[cfg(windows)]
            Cmd::Console => {
                let kind = sources::console_program(&path);
                let on = !entry.console.unwrap_or(kind);
                self.change(&path, |c| c.console = (on != kind).then_some(on));
            }
            Cmd::ChangeIcon => {
                let filter = if cfg!(windows) {
                    [(tr("Icons"), "*.ico;*.png;*.exe;*.dll;*.lnk"), (tr("All files"), "*.*")]
                } else {
                    [(tr("Icons"), "*.png;*.svg;*.desktop"), (tr("All files"), "*")]
                };
                if let Some(icon) = self.pick_file(&tr("Choose an icon"), &filter) {
                    // Read again, should the file have changed.
                    self.icons.remove(&icon);
                    self.change(&path, |c| c.icon = Some(icon));
                }
            }
            Cmd::DefaultIcon => self.change(&path, |c| c.icon = None),
            Cmd::Hide => self.change(&path, |c| c.hidden = !c.hidden),
            Cmd::Remove => {
                self.settings.items.remove(&path);
                self.save();
                self.refresh(None);
            }
        }
    }

    /// Keeps the name typed. For a program, none (or its own) forgets the
    /// change; for a category, none leaves everything as it was.
    fn finish_rename(&mut self) {
        let Some((what, edit)) = self.rename.take() else { return };
        let name = edit.text.trim().to_string();
        match what {
            Editing::Name(path) => {
                let own = self
                    .raw
                    .iter()
                    .find(|e| e.path == path)
                    .map(|e| e.name.clone())
                    .or_else(|| sources::added_entry(&path).map(|e| e.name));
                let name = (!name.is_empty() && Some(&name) != own.as_ref()).then_some(name);
                self.change(&path, |c| c.name = name);
            }
            Editing::NewCategory(path) => {
                if let Some(group) = Group::typed(&name) {
                    // Typed for all the marked programs, when it is one.
                    if self.app.marked.contains(&path) {
                        let paths: Vec<PathBuf> = self.app.marked.iter().cloned().collect();
                        self.move_many(&paths, group);
                    } else {
                        self.move_to(&path, group);
                    }
                }
            }
            Editing::Group(from) => {
                // Only the name of this level is typed.
                let name = name.replace('/', " ");
                let Some(typed) = Group::typed(&name) else { return };
                let to = match from.sub {
                    Some(_) => Group { category: from.category, sub: Some(rlaunch::category::intern(name.trim())) },
                    None => typed,
                };
                self.move_group(from, to);
            }
        }
    }

    fn rename_key(&mut self, event: &KeyEvent) {
        let command = self.mods.control_key() && !self.mods.alt_key();
        let shift = self.mods.shift_key();
        let Some((_, edit)) = &mut self.rename else { return };
        match &event.logical_key {
            Key::Named(NamedKey::Enter) => self.finish_rename(),
            Key::Named(NamedKey::Escape) => self.rename = None,
            Key::Named(NamedKey::Backspace) => edit.delete(false, command),
            Key::Named(NamedKey::Delete) => edit.delete(true, command),
            Key::Named(NamedKey::ArrowLeft) => edit.move_caret(true, false, command, shift),
            Key::Named(NamedKey::ArrowRight) => edit.move_caret(false, false, command, shift),
            Key::Named(NamedKey::Home) => edit.move_caret(true, true, false, shift),
            Key::Named(NamedKey::End) => edit.move_caret(false, true, false, shift),
            Key::Character(c) if command && c.eq_ignore_ascii_case("a") => edit.select_all(),
            Key::Character(c) if command && c.eq_ignore_ascii_case("c") && edit.selection().is_some() => {
                rt_sys::set_clipboard_text(edit.selected_text());
            }
            Key::Character(c) if command && c.eq_ignore_ascii_case("x") && edit.selection().is_some() => {
                let text = edit.cut();
                rt_sys::set_clipboard_text(&text);
            }
            Key::Character(c) if command && c.eq_ignore_ascii_case("v") => {
                if let Some(text) = rt_sys::clipboard_text() {
                    edit.insert(text.lines().next().unwrap_or(""));
                }
            }
            _ => {
                if let Some(text) = event.text.as_ref().filter(|_| !command && !self.mods.super_key()) {
                    edit.insert(text);
                }
            }
        }
    }

    fn pick_file(&mut self, title: &str, filter: &[(String, &str)]) -> Option<PathBuf> {
        self.asking = true;
        #[cfg(windows)]
        let picked = {
            let _ = title;
            let hwnd = self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd);
            rt_sys::win::dialogs::open_file(hwnd, None, &rt_sys::win::dialogs::filter(filter))
        };
        #[cfg(not(windows))]
        let picked = {
            let _ = title;
            rt_sys::linux::dialogs::open_files(None, filter, false).into_iter().next()
        };
        self.asking = false;
        if let Some(w) = &self.win {
            rt_ui::window::focus(&w.window);
        }
        picked
    }

    /// A file (or folder) chosen or dropped on the menu.
    fn add(&mut self, path: PathBuf) {
        if path.is_dir() {
            self.take_folder(path);
        } else {
            self.settings.items.add(&path);
            self.save();
            self.refresh(Some(&path));
        }
    }

    fn activate_row(&mut self, el: &ActiveEventLoop, k: usize) {
        match self.app.rows().get(k).copied() {
            Some(Row::Back) => self.app.back(),
            Some(Row::Category(c, _)) => self.app.open_category(c),
            Some(Row::Entry(i)) => {
                let entry = self.app.entries[i].clone();
                // Ctrl+Shift, as in the Start menu.
                let admin = entry.admin || (self.mods.control_key() && self.mods.shift_key());
                self.launch(el, &entry, admin);
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
            Hit::Page(appearance) => self.app.appearance = appearance,
            Hit::Colour(c) => {
                self.settings.colour = c;
                self.save();
            }
            Hit::About => self.about(),
            Hit::Exit => self.quit(el),
            #[cfg(windows)]
            Hit::Elevate => self.elevate(el),
            #[cfg(not(windows))]
            Hit::Elevate => {}
            Hit::Close => self.hide(el),
            Hit::Pin => self.pinned = !self.pinned,
            Hit::ShowHidden => self.app.open_hidden(),
            Hit::AddProgram => {
                let filter = if cfg!(windows) {
                    vec![(tr("Programs"), "*.exe;*.lnk;*.bat;*.cmd;*.appref-ms"), (tr("All files"), "*.*")]
                } else {
                    // Linux programs have no extension to ask for.
                    vec![(tr("All files"), "*")]
                };
                if let Some(file) = self.pick_file(&tr("Choose a program"), &filter) {
                    self.add(file);
                }
            }
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
                    let folder = self.settings.folders.remove(i);
                    self.settings.modes.remove(&folder);
                    self.save();
                    self.scan();
                }
            }
            Hit::FolderMode(i) => {
                if let Some(folder) = self.settings.folders.get(i).cloned() {
                    let at = FolderMode::ALL.iter().position(|m| *m == self.settings.mode_of(&folder)).unwrap_or(0);
                    self.settings.set_mode(&folder, FolderMode::ALL[(at + 1) % FolderMode::ALL.len()]);
                    self.save();
                    self.scan();
                }
            }
            Hit::AlwaysAdmin => {
                self.settings.always_admin = !self.settings.always_admin;
                self.save();
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
        if self.rename.is_some() {
            self.rename_key(event);
            self.caret = true;
            self.caret_at = Instant::now();
            self.redraw();
            return;
        }
        if self.menu.is_open() {
            let act = match &event.logical_key {
                Key::Named(NamedKey::ArrowUp) => {
                    self.menu.up_down(-1);
                    Act::None
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.menu.up_down(1);
                    Act::None
                }
                Key::Named(NamedKey::ArrowLeft) => self.menu.left(),
                Key::Named(NamedKey::ArrowRight) => self.menu.right(),
                Key::Named(NamedKey::Enter) => self.menu.enter(),
                Key::Named(NamedKey::Escape) => {
                    self.menu.back();
                    Act::None
                }
                _ => Act::None,
            };
            self.menu_act(el, act);
            return;
        }
        let page = self.drawn.visible.max(1) as isize;
        let command = self.mods.control_key() || self.mods.super_key();
        match &event.logical_key {
            // With Shift, marking the programs passed over.
            Key::Named(arrow @ (NamedKey::ArrowUp | NamedKey::ArrowDown)) => {
                let shift = self.mods.shift_key();
                if shift {
                    self.app.mark_to(self.app.selected);
                }
                self.app.step(if *arrow == NamedKey::ArrowUp { -1 } else { 1 });
                if shift {
                    self.app.mark_to(self.app.selected);
                }
            }
            Key::Character(c) if command && c.eq_ignore_ascii_case("a") => self.app.mark_all(),
            Key::Named(NamedKey::PageUp) => self.app.step(-page),
            Key::Named(NamedKey::PageDown) => self.app.step(page),
            Key::Named(NamedKey::Home) => self.app.step(isize::MIN / 2),
            Key::Named(NamedKey::End) => self.app.step(isize::MAX / 2),
            Key::Named(NamedKey::Enter) if self.app.view == View::Options => self.app.back(),
            Key::Named(NamedKey::ContextMenu) => self.menu_at_selection(),
            Key::Named(NamedKey::F10) if self.mods.shift_key() => self.menu_at_selection(),
            Key::Named(NamedKey::Enter) => {
                self.activate_row(el, self.app.selected);
            }
            Key::Named(NamedKey::ArrowRight) => {
                if let Some(Row::Category(c, _)) = self.app.rows().get(self.app.selected) {
                    self.app.open_category(*c);
                }
            }
            Key::Named(NamedKey::ArrowLeft) if self.app.query.is_empty() => {
                if matches!(self.app.view, View::Inside(_) | View::Hidden) {
                    self.app.back();
                }
            }
            Key::Named(NamedKey::Escape) => {
                if !self.app.marked.is_empty() {
                    self.app.clear_marks();
                } else if !self.app.query.is_empty() {
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
        if let Some((_, edit)) = &mut self.rename {
            edit.insert(text);
            return;
        }
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
        let menu_style = MenuStyle { dark: theme.dark, bar: theme.list, border: theme.border, text: theme.text, dim: theme.dim };
        let Some(win) = &mut self.win else { return };
        let size = win.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if win.surface.resize(w, h).is_err() {
            return;
        }
        self.gfx.resize(w.get(), h.get());
        let elevated_note = self.elevated.then(|| tr("Running as administrator: what it opens does too."));
        let new_category_note =
            matches!(self.rename, Some((Editing::NewCategory(_), _))).then(|| tr("New category: type a name, or Name/Subgroup."));
        let marked_note = (!self.app.marked.is_empty())
            .then(|| tf("{} programs selected: drag them or right-click one.", &[&self.app.marked.len().to_string()]));
        let model = Model {
            app: &self.app,
            settings: &self.settings,
            icons: &self.icons,
            note: new_category_note
                .as_deref()
                .or(marked_note.as_deref())
                .or(self.note.as_deref())
                .or(elevated_note.as_deref())
                .or(self.hotkey_note.as_deref()),
            autostart: self.autostart,
            hot: self.hot,
            disk: self.disk.clone(),
            caret: self.caret,
            pinned: self.pinned,
            elevated: self.elevated,
            accent: self.accent,
            drop: self.drop,
            rename: self.rename.as_ref().map(|(what, edit)| (what, edit)),
        };
        self.drawn = view::draw(&mut self.gfx, &model, &theme, s);
        let full = Rect::new(0.0, 0.0, w.get() as f32, h.get() as f32);
        self.menu.draw_popups(&mut self.gfx, full, &menu_style, s);
        if let Some(message) = &mut self.dialog {
            let style = rt_gfx::dialog::DialogStyle { dark: theme.dark, text: theme.text, dim: theme.dim, border: theme.border };
            message.draw(&mut self.gfx, Rect::new(0.0, 0.0, w.get() as f32, h.get() as f32), &style, s);
        }
        rt_ui::window::present(&mut win.surface, &self.gfx);
        self.gfx.trim_cache();
        // The icons of what is on screen, once each.
        for &i in &self.drawn.shown {
            let Some(path) = self.app.entries.get(i).map(Entry::icon_source) else { continue };
            if !self.icons.contains_key(path) {
                self.icons.insert(path.to_path_buf(), None);
                let _ = self.wanted.send(path.to_path_buf());
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
                    Row::Back => tr("Back"),
                    Row::Category(c, _) => c.title(),
                    Row::Entry(i) => self.app.entries.get(i)?.name.clone(),
                },
                Hit::Options => tr("Options"),
                Hit::About => tr("About"),
                Hit::Exit => tr("Exit"),
                Hit::Elevate => tr("Restart as administrator"),
                Hit::Done => tr("Done"),
                Hit::AddFolder => tr("Add a folder…"),
                Hit::AddProgram => tr("Add a program…"),
                Hit::Pin => tr("Keep the menu open"),
                Hit::ShowHidden => tr("Hidden programs"),
                Hit::CheckUpdates => tr("Check for updates"),
                Hit::Autostart => tr("Start with Windows"),
                Hit::AlwaysAdmin => tr("Always start as administrator"),
                Hit::Scoop => "Scoop".to_string(),
                Hit::StartMenu => tr("Start menu"),
                Hit::FolderMode(i) => {
                    let folder = self.settings.folders.get(i)?;
                    format!("{} {}", tr(self.settings.mode_of(folder).name()), folder.display())
                }
                Hit::Page(true) => tr("Appearance"),
                Hit::Page(false) => tr("Programs"),
                Hit::Colour(c) => format!("{} {}", tr("Color"), tr(theme::colour_name(c))),
                Hit::Step(field, forward) => {
                    let option = match field {
                        Field::Hotkey => tr("Keyboard shortcut"),
                        Field::Size => tr("Size"),
                        Field::Transparency => tr("Transparency"),
                        Field::Theme => tr("Theme"),
                        Field::Language => tr("Language"),
                        Field::Rows => tr("Rows"),
                        Field::Position => tr("Position"),
                        Field::Order => tr("Order"),
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
        for (l, level) in self.menu.levels.iter().enumerate() {
            for (i, _) in &level.rows {
                let id = NodeId(MENU_NODE + (l * 100 + i) as u64);
                let mut n = Node::new(Role::MenuItem);
                n.set_label(level.items[*i].label.clone());
                n.add_action(Action::Click);
                if level.sel == Some(*i) {
                    focus = id;
                }
                children.push(id);
                nodes.push((id, n));
            }
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
                self.raw = entries;
                let keep = self.app.selected_entry().map(|e| e.path.clone());
                self.refresh(keep.as_deref());
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
                    use rt_sys::win::tray::Item as Entry;
                    let (open, admin, exit) = (tr("Open rlaunch"), tr("Restart as administrator"), tr("Exit"));
                    let mut items = vec![Entry::new(1, &open), Entry::SEPARATOR];
                    if !self.elevated {
                        items.extend([Entry::new(3, &admin), Entry::SEPARATOR]);
                    }
                    items.push(Entry::new(2, &exit));
                    match rt_sys::win::tray::popup(hwnd, &items, 1) {
                        Some(1) => self.show(el),
                        Some(2) => self.quit(el),
                        Some(3) => self.elevate(el),
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
                    if let Some(n) = req.target_node.0.checked_sub(MENU_NODE) {
                        let act = self.menu.click(n as usize / 100, n as usize % 100);
                        self.menu_act(el, act);
                        return;
                    }
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
                if !focused && self.visible && self.resident && !self.asking && !self.pinned {
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
                if self.menu.is_open() {
                    if let Some((l, i)) = self.menu.row_at(x, y).filter(|_| moved) {
                        self.menu.hover(l, i);
                        self.redraw();
                    }
                    self.hot = None;
                    return;
                }
                if self.drag(x, y) {
                    return;
                }
                let hot = if self.dialog.is_none() { self.drawn.hit(x, y) } else { None };
                if let Some(Hit::Row(k)) = hot.filter(|_| moved && self.rename.is_none()) {
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
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. }
                if self.dialog.is_none() && !self.menu.is_open() && self.rename.is_none() =>
            {
                if let Some((x, y)) = self.cursor {
                    self.press_row(x, y);
                }
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                if self.end_drag() {
                    return;
                }
                let Some((x, y)) = self.cursor else { return };
                if let Some(message) = &self.dialog {
                    if message.closes_at(x, y) {
                        self.dialog = None;
                        self.redraw();
                    }
                } else if self.menu.is_open() {
                    match self.menu.row_at(x, y) {
                        Some((l, i)) => {
                            let act = self.menu.click(l, i);
                            self.menu_act(el, act);
                        }
                        // A click outside only closes the menu.
                        None if !self.menu.contains(x, y) => {
                            self.menu.close();
                            self.redraw();
                        }
                        None => {}
                    }
                } else {
                    let hit = self.drawn.hit(x, y);
                    // A click elsewhere keeps the name typed.
                    if let Some((what, _)) = &self.rename {
                        let there = matches!(hit, Some(Hit::Row(k)) if match self.app.rows().get(k) {
                            Some(&Row::Entry(i)) => what.is_program(&self.app.entries[i].path),
                            Some(&Row::Category(g, _)) => *what == Editing::Group(g),
                            _ => false,
                        });
                        if !there {
                            self.finish_rename();
                            self.redraw();
                        }
                    } else if let Some(hit) = hit {
                        if !matches!(hit, Hit::Row(k) if self.mark_click(k)) {
                            self.act(el, hit);
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Right, .. } if self.dialog.is_none() => {
                let Some((x, y)) = self.cursor else { return };
                self.menu.close();
                self.finish_rename();
                if let Some(Hit::Row(k)) = self.drawn.hit(x, y) {
                    self.open_menu(k, (x, y), false);
                }
                self.redraw();
            }
            WindowEvent::DroppedFile(path) => {
                self.add(path);
                self.redraw();
            }
            WindowEvent::MouseWheel { delta, .. } if self.dialog.is_none() => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 3.0,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / (if self.settings.large { 36.0 } else { 26.0 } * self.scale()),
                };
                // Over a long menu (the categories), the wheel scrolls it.
                let over = self.cursor.and_then(|(x, y)| self.menu.row_at(x, y));
                match over {
                    Some((l, _)) => {
                        self.menu.levels[l].sel = None;
                        self.menu.wheel(l, -lines.round() as isize);
                    }
                    None => {
                        self.menu.close();
                        self.app.scroll_by(-lines.round() as isize, self.drawn.visible);
                    }
                }
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
    // The instance this one replaces is on its way out: wait for it.
    #[cfg(windows)]
    if raw.iter().any(|a| a == ELEVATED_RESTART) {
        for _ in 0..100 {
            if !rt_sys::win::resident_running(&IDENT) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
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
    let mut settings = Settings::load(&store, own_dir(&location).as_deref());
    // The first time on a pen: a PortableApps suite beside rlaunch is found.
    if store.get("theme").is_none() {
        if let Some(suite) = own_dir(&location).and_then(|own| suite_near(&own)) {
            portableapps::import(&suite, &mut settings.items);
            settings.folders.push(suite);
        }
    }
    i18n::set_lang(settings.language.unwrap_or_else(i18n::system_lang));
    // Asked for in Options: start again with the rights. Said no (or already
    // tried): carry on without them.
    #[cfg(windows)]
    if settings.always_admin && !help && !rt_sys::win::elevated() && !raw.iter().any(|a| a == ELEVATED_RESTART) {
        let args = if tray { format!("{ELEVATED_RESTART} --tray") } else { ELEVATED_RESTART.to_string() };
        if rt_sys::win::restart_elevated(&args) {
            return;
        }
    }
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
        raw: Vec::new(),
        menu: Menu::default(),
        menu_for: PathBuf::new(),
        menu_group: None,
        press: None,
        drop: None,
        rename: None,
        pinned: false,
        #[cfg(windows)]
        elevated: rt_sys::win::elevated(),
        #[cfg(not(windows))]
        elevated: false,
        colours: storage::Store::default(),
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
