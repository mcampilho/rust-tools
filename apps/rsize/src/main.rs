//! rsize: a portable disk space analyzer.
//!
//! The window is driven by winit and drawn on the CPU (tiny-skia + ab_glyph)
//! into a softbuffer surface. Scans run on background threads and report to
//! the window through the event loop.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rsize::app::{App, Scanning};
use rsize::chart::{Chart, Hit};
use rsize::settings::ChartType;
use rsize::commands::{self, Command};
use rsize::i18n::{self, tf, tr};
use rsize::model::{Id, SizeMode};
use rsize::scan::{self, Event as ScanEvent};
use rsize::settings::{Settings, SPLIT_MAX, SPLIT_MIN};
use rsize::storage;
use rsize::treemap::Map;
use rsize::ui::gfx::{Color, Gfx, Rect};
use rsize::ui::theme::Theme;
use rsize::ui::view::{self, Hot, Layout, Page};
use rsize::volumes::{self, Volume};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Icon as WinIcon, Theme as WinTheme, Window, WindowId};

enum UserEvent {
    A11y(accesskit_winit::Event),
    Scan(u64, ScanEvent),
    Volumes(Vec<Volume>),
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

/// An in-app message (About, Keyboard shortcuts).
struct Dialog {
    title: String,
    text: String,
    panel: Rect,
    ok: Rect,
}

#[derive(Clone, Copy)]
enum Drag {
    Splitter,
    Thumb,
}

const DOUBLE_CLICK: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_SLOP: f32 = 4.0;
/// How long the window size must stay the same before the treemap is made again.
const RESIZE_PAUSE: Duration = Duration::from_millis(150);

/// What the cached treemap was drawn for: zoom, size, order, tree.
type MapKey = (Id, u32, u32, SizeMode, u64);
/// What the cached pie or rings were drawn for: the same, plus the chart,
/// the background and the scale.
type ChartKey = (ChartType, MapKey, u32, u32);

struct Shell {
    app: App,
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
    layout: Layout,
    map: Option<Map>,
    map_key: Option<MapKey>,
    /// When to make the treemap again at the new window size.
    map_due: Option<Instant>,
    /// The pie or the rings, and what they were drawn for.
    chart: Option<Chart>,
    chart_key: Option<ChartKey>,
    /// What the pointer is on in the pie or the rings.
    hover_seg: Option<Hit>,
    /// The table of file types, and the folder and order it was made for.
    types: Option<Vec<(u32, rsize::model::ExtStat)>>,
    types_key: Option<MapKey>,
    /// The last change of the window size.
    resized: Instant,
    hot: Hot,
    /// The node under the pointer (list or treemap).
    hover: Option<Id>,
    drag: Option<Drag>,
    cursor_icon: CursorIcon,
    handle: Option<scan::Handle>,
    generation: u64,
    /// Which tree the treemap was made from (changes with each scan).
    tree_gen: u64,
    dialog: Option<Dialog>,
    last_click: Option<(Instant, f32, f32, u32)>,
    args: Vec<PathBuf>,
    /// Running with administrator rights (NTFS can be read directly).
    #[cfg(windows)]
    elevated: bool,
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
            let max = w.window.is_maximized();
            if !max && !w.window.is_minimized().unwrap_or(false) {
                let s: LogicalSize<f64> = w.window.inner_size().to_logical(w.window.scale_factor());
                self.settings.window = Some((s.width, s.height, false));
            } else if let Some((ww, hh, _)) = self.settings.window {
                self.settings.window = Some((ww, hh, max));
            }
        }
        let store = self.settings.to_store();
        self.location.save(&store);
    }

    // ------------------------------------------------------------ scans

    fn load_volumes(&self) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let _ = proxy.send_event(UserEvent::Volumes(volumes::list()));
        });
    }

    fn start_scan(&mut self, path: &Path) {
        if let Some(h) = self.handle.take() {
            h.cancel();
        }
        self.generation += 1;
        let gen = self.generation;
        let root = scan::normalize(path);
        self.app.scanning = Some(Scanning { root: root.clone(), progress: Default::default(), started: Instant::now(), generation: gen });
        self.app.error = None;
        let proxy = self.proxy.clone();
        let lang = i18n::lang();
        self.handle = Some(scan::start(
            root,
            Box::new(move |e| {
                i18n::set_lang(lang);
                let _ = proxy.send_event(UserEvent::Scan(gen, e));
            }),
        ));
        self.drag = None;
        self.menu_dirty = true;
        self.update_title();
        self.redraw();
    }

    fn on_scan(&mut self, gen: u64, ev: ScanEvent) {
        if gen != self.generation {
            return;
        }
        match ev {
            ScanEvent::Progress(p) => {
                if let Some(s) = &mut self.app.scanning {
                    s.progress = p;
                }
            }
            ScanEvent::Done(result) => {
                let root = self.app.scanning.take().map(|s| s.root).unwrap_or_default();
                self.handle = None;
                match result {
                    Ok(scan) => {
                        self.app.set_scan(*scan, self.settings.size_mode);
                        self.tree_gen += 1;
                        self.map = None;
                        self.hover = None;
                    }
                    Err(e) => {
                        self.app.error = Some(tf("Could not read {}: {}", &[&root.to_string_lossy(), &e]));
                        self.app.show_drives = true;
                    }
                }
                self.menu_dirty = true;
                self.update_title();
            }
        }
        self.redraw();
    }

    fn update_title(&self) {
        let Some(w) = &self.win else { return };
        let title = match (&self.app.scanning, &self.app.scan) {
            (Some(s), _) => format!("{} - rsize", s.root.display()),
            (None, Some(s)) if !self.app.show_drives => format!("{} - rsize", s.tree.name(s.tree.root())),
            _ => "rsize".to_string(),
        };
        if w.window.title() != title {
            w.window.set_title(&title);
        }
    }

    fn choose_folder(&mut self) {
        let title = tr("Choose a folder to analyze");
        #[cfg(windows)]
        let chosen = self.win.as_ref().and_then(|w| rt_sys::win::dialogs::pick_folder(w.hwnd, &title));
        #[cfg(not(windows))]
        let chosen = rt_sys::linux::dialogs::pick_folder(&title);
        if let Some(p) = chosen {
            self.settings.last_dir = p.display().to_string();
            self.start_scan(&p);
        }
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self, el: &ActiveEventLoop, cmd: Command) {
        if !self.enabled(cmd) {
            return;
        }
        match cmd {
            Command::ChooseFolder => self.choose_folder(),
            Command::Drives => {
                self.app.show_drives = true;
                self.app.volumes = None;
                self.load_volumes();
                self.update_title();
            }
            Command::Rescan => {
                if let Some(root) = self.app.scan.as_ref().map(|s| PathBuf::from(s.tree.name(s.tree.root()))) {
                    self.start_scan(&root);
                }
            }
            Command::Stop => {
                if let Some(h) = &self.handle {
                    h.cancel();
                }
            }
            #[cfg(windows)]
            Command::RestartAdmin => {
                // The new instance reads the same folder, now directly.
                let root = self.app.scanning.as_ref().map(|s| s.root.clone()).or_else(|| self.app.scan.as_ref().map(|s| PathBuf::from(s.tree.name(s.tree.root()))));
                // Without its last backslash, which would escape the closing quote:
                // "C:" is read as C:\ again (see `scan::normalize`).
                let args = root.map_or(String::new(), |r| format!("\"{}\"", r.display().to_string().trim_end_matches('\\')));
                if rt_sys::win::restart_elevated(&args) {
                    self.quit(el);
                }
            }
            #[cfg(windows)]
            Command::ExplorerMenu => self.integration(!self.explorer_menu),
            Command::Exit => self.quit(el),
            Command::ExportCsv => self.export_csv(),
            Command::Delete => self.delete(),
            Command::CopyPath => {
                if let Some(p) = self.app.selected_path() {
                    rt_sys::set_clipboard_text(&p.to_string_lossy());
                }
            }
            Command::OpenItem => {
                if let Some(p) = self.app.selected_path() {
                    open_item(&p);
                }
            }
            Command::Reveal => {
                if let Some(p) = self.app.selected_path() {
                    rt_sys::reveal(&p);
                }
            }
            Command::ZoomIn => self.app.zoom_in(),
            Command::ZoomOut => self.app.zoom_out(),
            Command::ZoomTop => self.app.zoom_top(),
            Command::Chart(kind) => {
                self.settings.chart = kind;
                self.hover = None;
                self.hover_seg = None;
            }
            Command::SizeLogical | Command::SizeAllocated => {
                let mode = if cmd == Command::SizeLogical { SizeMode::Logical } else { SizeMode::Allocated };
                self.settings.size_mode = mode;
                self.app.set_mode(mode);
            }
            Command::Theme(t) => {
                self.settings.theme = t;
                self.apply_window_theme();
            }
            Command::UiLanguage(i) => {
                self.settings.language = (i as usize).checked_sub(1).and_then(|k| i18n::Lang::ALL.get(k)).copied();
                self.apply_language();
            }
            Command::Shortcuts => self.info(&tr("Keyboard shortcuts"), &shortcuts_text()),
            Command::About => {
                let text = tf(
                    "rsize {}\n\nShows what takes up space on a drive or in a folder, written in Rust.\nNo telemetry, no network access.\n\nMIT License.",
                    &[env!("CARGO_PKG_VERSION")],
                );
                self.info(&tr("About rsize"), &text);
            }
        }
        self.menu_dirty = true;
        self.redraw();
    }

    /// Writes the folder shown, with all it holds, to a CSV file.
    fn export_csv(&mut self) {
        let Some(scan) = &self.app.scan else { return };
        let name = rsize::export::suggested_name(&scan.tree, self.app.zoom);
        let dir = (!self.settings.last_dir.is_empty()).then(|| PathBuf::from(&self.settings.last_dir));
        #[cfg(windows)]
        let chosen = self.win.as_ref().and_then(|w| {
            let filter = rt_sys::win::dialogs::filter(&[(tr("CSV files"), "*.csv")]);
            rt_sys::win::dialogs::save_file(w.hwnd, &name, dir.as_deref(), &filter)
        });
        #[cfg(not(windows))]
        let chosen = rt_sys::linux::dialogs::save_file(&name, dir.as_deref());
        let Some(path) = chosen else { return };
        let result = std::fs::File::create(&path).and_then(|f| rsize::export::write_csv(&scan.tree, self.app.zoom, f));
        let shown = path.display().to_string();
        self.app.note = Some(match result {
            Ok(rows) => tf("{} rows written to {}", &[&rsize::format::count(rows), &shown]),
            Err(e) => tf("Could not write {}: {}", &[&shown, &e.to_string()]),
        });
        if let Some(d) = path.parent() {
            self.settings.last_dir = d.display().to_string();
        }
    }

    /// Moves the selection to the Recycle Bin (the trash on Linux), after
    /// asking, and takes it out of the tree.
    fn delete(&mut self) {
        let Some(scan) = &self.app.scan else { return };
        let Some(id) = self.app.sel.filter(|&s| s != scan.tree.root()) else { return };
        let t = &scan.tree;
        let path = t.path(id);
        let name = t.name(id).to_string();
        let n = t.node(id);
        let what = if n.is_dir() {
            tf("{} in {} files", &[&rsize::format::size(n.size), &rsize::format::count(n.files)])
        } else {
            rsize::format::size(n.size)
        };
        let question = if cfg!(windows) {
            tf("Move “{}” to the Recycle Bin?\n\n{}\n{}", &[&name, &path.display().to_string(), &what])
        } else {
            tf("Move “{}” to the trash?\n\n{}\n{}", &[&name, &path.display().to_string(), &what])
        };
        let title = tr("Delete");
        #[cfg(windows)]
        let (yes, result) = {
            use rt_sys::win::dialogs::{message, Answer, Buttons, Icon};
            let hwnd = self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd);
            let yes = message(hwnd, &title, &question, Buttons::OkCancel, Icon::Warning) == Answer::Ok;
            (yes, if yes { rt_sys::win::recycle(hwnd, &path) } else { Err(String::new()) })
        };
        #[cfg(not(windows))]
        let (yes, result) = {
            let yes = rt_sys::linux::dialogs::confirm(&title, &question);
            (yes, if yes { rt_sys::recycle(&path) } else { Err(String::new()) })
        };
        if !yes {
            return;
        }
        match result {
            Ok(()) => {
                self.app.removed(id);
                self.tree_gen += 1;
                self.hover = None;
                self.hover_seg = None;
                self.app.note = Some(if cfg!(windows) {
                    tf("“{}” was moved to the Recycle Bin.", &[&name])
                } else {
                    tf("“{}” was moved to the trash.", &[&name])
                });
            }
            Err(e) if e.is_empty() => {}
            Err(e) => self.app.note = Some(tf("Could not delete “{}”: {}", &[&name, &e])),
        }
    }

    fn checked(&self, c: Command) -> bool {
        match c {
            Command::Chart(kind) => self.settings.chart == kind,
            Command::SizeLogical => self.settings.size_mode == SizeMode::Logical,
            Command::SizeAllocated => self.settings.size_mode == SizeMode::Allocated,
            Command::Theme(t) => self.settings.theme == t,
            Command::UiLanguage(0) => self.settings.language.is_none(),
            Command::UiLanguage(i) => self.settings.language == i18n::Lang::ALL.get(i as usize - 1).copied(),
            #[cfg(windows)]
            Command::ExplorerMenu => self.explorer_menu,
            _ => false,
        }
    }

    fn enabled(&self, c: Command) -> bool {
        let result = view::page(&self.app) == Page::Result;
        match c {
            Command::Rescan => self.app.has_tree() && self.app.scanning.is_none(),
            Command::Stop => self.app.scanning.is_some(),
            #[cfg(windows)]
            Command::RestartAdmin => !self.elevated,
            Command::CopyPath | Command::OpenItem | Command::Reveal => result && self.app.sel.is_some(),
            Command::ExportCsv => result,
            // Anything but the folder that was read.
            Command::Delete => result && self.app.sel.is_some_and(|s| s != 0),
            Command::ZoomIn => result && self.app.sel.is_some(),
            Command::ZoomOut | Command::ZoomTop => result && self.app.scan.as_ref().is_some_and(|s| s.tree.parent(self.app.zoom).is_some()),
            _ => true,
        }
    }

    fn info(&mut self, title: &str, text: &str) {
        self.dialog = Some(Dialog { title: title.to_string(), text: text.to_string(), panel: Rect::default(), ok: Rect::default() });
        self.drag = None;
        self.redraw();
    }

    fn quit(&mut self, el: &ActiveEventLoop) {
        if let Some(h) = &self.handle {
            h.cancel();
        }
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
        self.drag = None;
        // Release the frame buffer and the drawings while hidden.
        self.gfx.resize(1, 1);
        self.gfx.trim_cache();
        self.map = None;
        self.map_key = None;
        self.chart = None;
        self.chart_key = None;
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
        if let Some(p) = paths.first() {
            self.start_scan(p);
        }
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

    fn count_click(&mut self, x: f32, y: f32) -> u32 {
        let now = Instant::now();
        let n = match self.last_click {
            Some((t, cx, cy, n)) if now - t < DOUBLE_CLICK && (cx - x).abs() < DOUBLE_CLICK_SLOP && (cy - y).abs() < DOUBLE_CLICK_SLOP => n % 2 + 1,
            _ => 1,
        };
        self.last_click = Some((now, x, y, n));
        n
    }

    // ------------------------------------------------------------ menus

    /// The context menu of the selection, at the pointer.
    #[cfg(windows)]
    fn context_menu(&mut self, el: &ActiveEventLoop) {
        let Some(hwnd) = self.win.as_ref().map(|w| w.hwnd) else { return };
        let mut items = Vec::new();
        for m in commands::item_menu() {
            items.push(match m {
                rt_core::menu::M::Item(c) => Some((c, self.enabled(c))),
                _ => None,
            });
        }
        if let Some(c) = rt_sys::win::menu::popup_at_cursor(hwnd, &items) {
            self.command(el, c);
        }
    }

    #[cfg(not(windows))]
    fn context_menu(&mut self, _el: &ActiveEventLoop) {
        let me = &*self;
        let items = rsize::ui::menu::build(commands::item_menu(), &|c| me.checked(c), &|c| me.enabled(c));
        let at = self.cursor.unwrap_or((0.0, 0.0));
        self.menu.open(None, items, at, false);
        self.redraw();
    }

    #[cfg(not(windows))]
    fn open_menu(&mut self, i: usize, keyboard: bool) {
        let Some((_, tree)) = commands::menu_tree().into_iter().nth(i) else { return };
        let me = &*self;
        let items = rsize::ui::menu::build(tree, &|c| me.checked(c), &|c| me.enabled(c));
        let Some(rect) = self.menu.titles.get(i).map(|t| t.rect) else { return };
        self.menu.open(Some(i), items, (rect.x, rect.bottom() + 2.0), keyboard);
        self.redraw();
    }

    #[cfg(not(windows))]
    fn menu_act(&mut self, el: &ActiveEventLoop, act: rsize::ui::menu::Act) {
        use rsize::ui::menu::Act;
        match act {
            Act::Run(c) => {
                self.menu.close();
                self.command(el, c);
            }
            Act::Switch(dir) => {
                let n = self.menu.titles.len() as isize;
                if let Some(top) = self.menu.top {
                    self.open_menu((top as isize + dir).rem_euclid(n) as usize, true);
                }
            }
            Act::None => self.redraw(),
        }
    }

    /// Keyboard for the menus. Returns true when it used the key.
    #[cfg(not(windows))]
    fn menu_key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) -> bool {
        use rsize::ui::menu::Act;
        use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
        let key = event.key_without_modifiers();
        if !self.menu.is_open() {
            let found = match &key {
                WKey::Named(NamedKey::F10) => Some(0),
                WKey::Character(c) if self.mods.alt_key() && !self.mods.control_key() => {
                    let k = c.chars().next().and_then(|c| c.to_lowercase().next());
                    self.menu.titles.iter().position(|t| t.mnemonic == k)
                }
                _ => None,
            };
            if let Some(i) = found {
                self.open_menu(i, true);
            }
            return found.is_some();
        }
        let act = match &key {
            WKey::Named(NamedKey::ArrowUp) => {
                self.menu.up_down(-1);
                Act::None
            }
            WKey::Named(NamedKey::ArrowDown) => {
                self.menu.up_down(1);
                Act::None
            }
            WKey::Named(NamedKey::ArrowLeft) => self.menu.left(),
            WKey::Named(NamedKey::ArrowRight) => self.menu.right(),
            WKey::Named(NamedKey::Enter | NamedKey::Space) => self.menu.enter(),
            WKey::Named(NamedKey::Escape) => {
                self.menu.back();
                Act::None
            }
            WKey::Character(c) => c.chars().next().map_or(Act::None, |c| self.menu.mnemonic(c)),
            _ => Act::None,
        };
        self.menu_act(el, act);
        true
    }

    #[cfg(not(windows))]
    fn menu_pointer(&mut self, x: f32, y: f32) -> bool {
        let title = self.menu.title_at(x, y);
        if title != self.menu.hot {
            self.menu.hot = title;
            self.redraw();
        }
        if !self.menu.is_open() {
            return false;
        }
        if let Some((l, i)) = self.menu.row_at(x, y) {
            self.menu.hover(l, i);
            self.redraw();
        } else if let Some(i) = title.filter(|i| self.menu.top.is_some_and(|t| t != *i)) {
            self.open_menu(i, false);
        }
        true
    }

    #[cfg(not(windows))]
    fn menu_press(&mut self, el: &ActiveEventLoop, x: f32, y: f32) -> bool {
        if let Some(i) = self.menu.title_at(x, y) {
            if self.menu.top == Some(i) {
                self.menu.close();
                self.redraw();
            } else {
                self.open_menu(i, false);
            }
            return true;
        }
        if !self.menu.is_open() {
            return false;
        }
        if let Some((l, i)) = self.menu.row_at(x, y) {
            let act = self.menu.click(l, i);
            self.menu_act(el, act);
        } else if !self.menu.contains(x, y) {
            self.menu.close();
            self.redraw();
        }
        true
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
            rt_sys::win::set_mnemonics(&mnemonics_of(&tree));
            win.menu = Some(rt_sys::win::menu::MenuBar::install(win.hwnd, tree));
        }
        let me = &*self;
        if let Some(m) = me.win.as_ref().and_then(|w| w.menu.as_ref()) {
            m.sync(|c| me.checked(c), |c| me.enabled(c));
        }
    }

    #[cfg(not(windows))]
    fn sync_menu(&mut self) {
        self.menu_dirty = false;
        if !self.menu.titles.is_empty() && !self.menu_rebuild {
            return;
        }
        self.menu_rebuild = false;
        self.menu.titles = commands::menu_tree().iter().map(|(title, _)| rsize::ui::menu::Title::new(&tr(title))).collect();
        self.redraw();
    }

    fn apply_window_theme(&self) {
        if let Some(w) = &self.win {
            w.window.set_theme(Some(if self.dark() { WinTheme::Dark } else { WinTheme::Light }));
            #[cfg(windows)]
            rt_sys::win::darkmode::refresh_caption(w.hwnd);
        }
    }

    fn sync_menu_theme(&mut self, dark: bool) {
        if self.menu_dark == Some(dark) {
            return;
        }
        self.menu_dark = Some(dark);
        #[cfg(windows)]
        if let Some(w) = &self.win {
            let colors = dark.then_some(rt_sys::win::darkmode::MenuColors { bar: 0x202020, text: 0xE4E4E4, text_disabled: 0x7A7A7A, hot: 0x3D3D3D });
            rt_sys::win::darkmode::apply(w.hwnd, colors);
        }
    }

    // ------------------------------------------------------------ input

    fn key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) {
        if self.dialog.is_some() {
            if matches!(event.logical_key, WKey::Named(NamedKey::Escape | NamedKey::Enter | NamedKey::Space)) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        #[cfg(not(windows))]
        if self.menu_key(el, event) {
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
        let name = match &logical {
            WKey::Character(c) => c.to_lowercase(),
            WKey::Named(n) => match n {
                NamedKey::F1 => "f1".into(),
                NamedKey::F5 => "f5".into(),
                NamedKey::F6 => "f6".into(),
                NamedKey::F7 => "f7".into(),
                NamedKey::F8 => "f8".into(),
                NamedKey::F9 => "f9".into(),
                NamedKey::Delete => "delete".into(),
                NamedKey::Enter => "enter".into(),
                NamedKey::Backspace => "backspace".into(),
                NamedKey::Escape => "esc".into(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        if !name.is_empty() {
            if let Some(cmd) = commands::lookup(ctrl, shift, alt, &name) {
                return self.command(el, cmd);
            }
        }
        if view::page(&self.app) != Page::Result {
            return;
        }
        let page = self.layout.page_rows() as isize;
        match logical {
            WKey::Named(NamedKey::ArrowDown) => self.app.move_sel(1),
            WKey::Named(NamedKey::ArrowUp) => self.app.move_sel(-1),
            WKey::Named(NamedKey::PageDown) => self.app.move_sel(page),
            WKey::Named(NamedKey::PageUp) => self.app.move_sel(-page),
            WKey::Named(NamedKey::Home) => self.app.move_sel(isize::MIN / 2),
            WKey::Named(NamedKey::End) => self.app.move_sel(isize::MAX / 2),
            WKey::Named(NamedKey::ArrowLeft) => self.app.left(),
            WKey::Named(NamedKey::ArrowRight) => self.app.right(),
            WKey::Named(NamedKey::Space) => {
                if let Some(id) = self.app.sel {
                    self.app.toggle(id);
                }
            }
            _ => return,
        }
        self.menu_dirty = true;
        self.redraw();
    }

    /// Updates what the pointer is on.
    fn pointer(&mut self, x: f32, y: f32) {
        let l = &self.layout;
        let mut seg = None;
        let (hot, hover) = match view::page(&self.app) {
            Page::Start if !l.body.contains(x, y) => (Hot::None, None),
            Page::Start => {
                let card = l.cards.iter().position(|r| r.contains(x, y));
                (card.map_or(if l.choose.contains(x, y) { Hot::Choose } else { Hot::None }, Hot::Card), None)
            }
            Page::Scanning => (if l.stop.contains(x, y) { Hot::Stop } else { Hot::None }, None),
            Page::Result => {
                if l.splitter.contains(x, y) {
                    (Hot::Splitter, None)
                } else if let Some(&(kind, _)) = l.tabs.iter().find(|(_, r)| r.contains(x, y)) {
                    (Hot::Tab(kind), None)
                } else if let Some(i) = l.row_at(&self.app, x, y) {
                    (Hot::Row(i), Some(self.app.rows[i].0))
                } else if let (Some(c), true) = (&self.chart, self.settings.chart != ChartType::Treemap && l.map.contains(x, y)) {
                    seg = c.hit(x - l.map.x, y - l.map.y);
                    let node = match seg {
                        Some(Hit::Segment(i)) => c.segments[i].node,
                        Some(Hit::Center) => Some(c.root),
                        None => None,
                    };
                    (Hot::None, node)
                } else if let Some((mx, my)) = self.map_point(x, y) {
                    (Hot::None, self.map.as_ref().and_then(|m| m.hit(mx, my)))
                } else {
                    (Hot::None, None)
                }
            }
        };
        let hot = if self.dialog.is_some() { Hot::None } else { hot };
        if hot != self.hot || hover != self.hover || seg != self.hover_seg {
            self.hot = hot;
            self.hover = hover;
            self.hover_seg = seg;
            self.redraw();
        }
        let icon = match (self.drag, hot) {
            (Some(Drag::Splitter), _) | (_, Hot::Splitter) => CursorIcon::NsResize,
            (_, Hot::Card(_) | Hot::Choose | Hot::Stop | Hot::Tab(_)) => CursorIcon::Pointer,
            _ if seg == Some(Hit::Center) => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        };
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(w) = &self.win {
                w.window.set_cursor(icon);
            }
        }
    }

    fn press(&mut self, el: &ActiveEventLoop, x: f32, y: f32) {
        if let Some(d) = &self.dialog {
            if d.ok.contains(x, y) || !d.panel.contains(x, y) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        #[cfg(not(windows))]
        if self.menu_press(el, x, y) {
            return;
        }
        let clicks = self.count_click(x, y);
        match self.hot {
            Hot::Card(i) => {
                if let Some(p) = self.app.volumes.as_ref().and_then(|v| v.get(i)).map(|v| v.path.clone()) {
                    self.start_scan(&p);
                }
            }
            Hot::Choose => self.command(el, Command::ChooseFolder),
            Hot::Stop => self.command(el, Command::Stop),
            Hot::Splitter => self.drag = Some(Drag::Splitter),
            Hot::Tab(kind) => self.command(el, Command::Chart(kind)),
            Hot::Row(i) => {
                let (id, depth) = self.app.rows[i];
                if self.layout.on_arrow(x, depth) || clicks == 2 {
                    self.app.toggle(id);
                }
                self.app.select(id);
            }
            Hot::None => {
                let l = &self.layout;
                if l.track.contains(x, y) && self.app.rows.len() > l.page_rows() {
                    self.drag = Some(Drag::Thumb);
                    self.drag_to(y);
                } else if self.settings.chart != ChartType::Treemap && l.map.contains(x, y) {
                    self.chart_press(clicks);
                } else if let Some((mx, my)) = self.map_point(x, y) {
                    if let Some(id) = self.map.as_ref().and_then(|m| m.hit(mx, my)) {
                        self.app.select(id);
                        if clicks == 2 {
                            self.zoom_toward(id);
                        }
                    }
                }
            }
        }
        self.menu_dirty = true;
        self.redraw();
    }

    /// A click on the pie or the rings selects; a double click on a folder
    /// shows it; a click in the middle of the rings goes up.
    fn chart_press(&mut self, clicks: u32) {
        match self.hover_seg {
            Some(Hit::Center) => self.app.zoom_out(),
            Some(Hit::Segment(i)) => {
                let Some(id) = self.chart.as_ref().and_then(|c| c.segments.get(i)).and_then(|s| s.node) else { return };
                self.app.select(id);
                let t = &self.app.scan.as_ref().expect("a chart has a tree").tree;
                if clicks == 2 && t.node(id).is_dir() && !t.children(id).is_empty() {
                    self.app.zoom = id;
                }
            }
            None => {}
        }
    }

    /// A double click in the treemap shows the folder one level down
    /// toward `id`.
    fn zoom_toward(&mut self, id: Id) {
        let Some(t) = self.app.scan.as_ref().map(|s| &s.tree) else { return };
        let mut cur = id;
        while let Some(p) = t.parent(cur) {
            if p == self.app.zoom {
                if t.node(cur).is_dir() && !t.children(cur).is_empty() {
                    self.app.zoom = cur;
                    self.app.select(cur);
                }
                return;
            }
            cur = p;
        }
    }

    fn drag_to(&mut self, y: f32) {
        match self.drag {
            Some(Drag::Splitter) => {
                let body = self.layout.body;
                if body.h > 0.0 {
                    self.settings.split = ((y - body.y) / body.h).clamp(SPLIT_MIN, SPLIT_MAX);
                }
            }
            Some(Drag::Thumb) => {
                let page = self.layout.page_rows();
                self.app.scroll = view::scroll_for(&self.layout, self.app.rows.len(), page, y);
            }
            None => return,
        }
        self.redraw();
    }

    // ------------------------------------------------------------ frame

    fn render(&mut self) {
        let dark = self.dark();
        let theme = Theme::new(dark, self.accent);
        let s = self.scale();
        let Some(win) = &mut self.win else { return };
        let size = win.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if win.surface.resize(w, h).is_err() {
            return;
        }
        self.gfx.resize(w.get(), h.get());
        let full = Rect::new(0.0, 0.0, w.get() as f32, h.get() as f32);
        self.layout = view::layout(&self.app, self.settings.split, full, s, cfg!(not(windows)));
        self.app.page = self.layout.page_rows();
        self.ensure_map(theme.bg);
        let frame = view::Frame {
            mode: self.settings.size_mode,
            kind: self.settings.chart,
            map: self.map.as_ref(),
            chart: self.chart.as_ref(),
            hover_seg: self.hover_seg,
            types: self.types.as_deref(),
            hot: self.hot,
            hover: self.hover,
        };
        view::draw(&mut self.gfx, &self.app, &self.layout, &theme, s, &frame);
        #[cfg(not(windows))]
        {
            let g = &mut self.gfx;
            self.menu.draw_bar(g, self.layout.bar, &theme.menu_style(), s);
            self.menu.draw_popups(g, full, &theme.menu_style(), s);
        }
        if let Some(d) = &mut self.dialog {
            let (panel, ok) = rt_gfx::dialog::draw_message(&mut self.gfx, full, &theme.dialog_style(), s, &d.title, &d.text);
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

    /// Makes the treemap again when the folder, the size or the order
    /// changed. While the window is being resized, the old one is stretched
    /// and the new one waits until the size has stayed the same for a moment:
    /// a large tree takes a fraction of a second to lay out.
    fn ensure_map(&mut self, bg: Color) {
        if view::page(&self.app) != Page::Result {
            return;
        }
        let Some(scan) = &self.app.scan else { return };
        let r = self.layout.map;
        let (w, h) = (r.w.max(1.0) as u32, r.h.max(1.0) as u32);
        let key = (self.app.zoom, w, h, self.settings.size_mode, self.tree_gen);
        if self.settings.chart == ChartType::Types {
            let tkey = (self.app.zoom, 0, 0, self.settings.size_mode, self.tree_gen);
            if self.types_key != Some(tkey) || self.types.is_none() {
                self.types = Some(scan.tree.ext_totals(self.app.zoom));
                self.types_key = Some(tkey);
                self.app.types_scroll = 0;
            }
            return;
        }
        if self.settings.chart != ChartType::Treemap {
            // A few thousand segments at most: quick enough to redo at once.
            let s = self.scale();
            let bg = (bg.r as u32) << 16 | (bg.g as u32) << 8 | bg.b as u32;
            let ckey = (self.settings.chart, key, bg, s.to_bits());
            if self.chart_key != Some(ckey) || self.chart.is_none() {
                let t = &scan.tree;
                self.chart = Some(match self.settings.chart {
                    ChartType::Pie => Chart::pie(t, self.app.zoom, w, h, s, bg),
                    _ => Chart::rings(t, self.app.zoom, w, h, s, bg),
                });
                self.chart_key = Some(ckey);
                self.hover_seg = None;
            }
            return;
        }
        if self.map_key == Some(key) && self.map.is_some() {
            self.map_due = None;
            return;
        }
        let only_size = self.map_key.is_some_and(|k| (k.0, k.3, k.4) == (key.0, key.3, key.4)) && self.map.is_some();
        if only_size && self.resized.elapsed() < RESIZE_PAUSE {
            self.map_due = Some(self.resized + RESIZE_PAUSE);
            return;
        }
        self.map = Some(Map::new(&scan.tree, self.app.zoom, w, h));
        self.map_key = Some(key);
        self.map_due = None;
    }

    /// A point of the window in the treemap's own pixels (it may be
    /// stretched while the window is resized).
    fn map_point(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let (m, r) = (self.map.as_ref()?, self.layout.map);
        (r.contains(x, y) && r.w > 0.0 && r.h > 0.0).then(|| ((x - r.x) * m.w as f32 / r.w, (y - r.y) * m.h as f32 / r.h))
    }

    fn tree(&self) -> accesskit::TreeUpdate {
        use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
        let root = NodeId(0);
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        let mut focus = root;
        if let (Page::Result, Some(scan)) = (view::page(&self.app), &self.app.scan) {
            let t = &scan.tree;
            let mut list = Node::new(Role::Tree);
            list.set_label(tr("Folders"));
            let mut rows = Vec::new();
            let end = (self.app.scroll + self.layout.page_rows()).min(self.app.rows.len());
            for (i, &(id, depth)) in self.app.rows.iter().enumerate().take(end).skip(self.app.scroll) {
                let nid = NodeId(100 + i as u64);
                let mut n = Node::new(Role::TreeItem);
                n.set_label(format!("{}, {}", t.name(id), rsize::format::size(t.size(id))));
                n.set_level(depth as usize + 1);
                if !t.children(id).is_empty() {
                    n.set_expanded(self.app.expanded.contains(&id));
                }
                n.set_selected(self.app.sel == Some(id));
                n.add_action(accesskit::Action::Click);
                if self.app.sel == Some(id) {
                    focus = nid;
                }
                rows.push(nid);
                nodes.push((nid, n));
            }
            list.set_children(rows);
            nodes.push((NodeId(1), list));
            let mut map = Node::new(Role::Image);
            map.set_label(view::chart_name(self.settings.chart));
            nodes.push((NodeId(2), map));
            children.extend([NodeId(1), NodeId(2)]);
        }
        let (left, right) = view::status_text(&self.app, None);
        let mut st = Node::new(Role::Label);
        st.set_value(format!("{left} {right}"));
        nodes.push((NodeId(3), st));
        children.push(NodeId(3));
        if let Some(dl) = &self.dialog {
            let mut dn = Node::new(Role::Dialog);
            dn.set_label(dl.title.clone());
            dn.set_children(vec![NodeId(5), NodeId(6)]);
            let mut tn = Node::new(Role::Label);
            tn.set_value(dl.text.replace('\t', "  "));
            let mut ok = Node::new(Role::Button);
            ok.set_label("OK");
            ok.add_action(accesskit::Action::Click);
            nodes.extend([(NodeId(4), dn), (NodeId(5), tn), (NodeId(6), ok)]);
            children.push(NodeId(4));
            focus = NodeId(6);
        }
        let mut r = Node::new(Role::Window);
        r.set_label("rsize");
        r.set_children(children);
        nodes.push((root, r));
        TreeUpdate { nodes, tree: Some(TreeInfo::new(root)), tree_id: TreeId::ROOT, focus }
    }

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let (w, h, maximized) = self.settings.window.unwrap_or((1100.0, 780.0, false));
        let attrs = Window::default_attributes()
            .with_title("rsize")
            .with_inner_size(LogicalSize::new(w.max(480.0), h.max(360.0)))
            .with_min_inner_size(LogicalSize::new(480.0, 360.0))
            // Maximizing shows the window on Windows: a resident start, which
            // stays hidden, maximizes when it is first shown.
            .with_maximized(maximized && !self.tray)
            .with_window_icon(app_icon())
            .with_theme(Some(if self.dark() { WinTheme::Dark } else { WinTheme::Light }))
            .with_visible(false);
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("rsize: cannot create window: {e}");
                el.exit();
                return false;
            }
        };
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(el, &window, self.proxy.clone());
        let surface = match softbuffer::Context::new(window.clone()).and_then(|ctx| softbuffer::Surface::new(&ctx, window.clone())) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rsize: cannot create drawing surface: {e}");
                el.exit();
                return false;
            }
        };
        #[cfg(windows)]
        let hwnd = hwnd_of(&window);
        #[cfg(windows)]
        {
            let proxy = self.proxy.clone();
            rt_sys::win::subclass(
                hwnd,
                &IDENT,
                &mnemonics_of(&commands::menu_tree()),
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
}

/// Opens a file or folder with its default program (an explicit command).
fn open_item(p: &Path) {
    #[cfg(windows)]
    rt_sys::win::shell_open(&p.to_string_lossy());
    #[cfg(not(windows))]
    rt_sys::linux::xdg_open(p.as_os_str().to_os_string());
}

fn shortcuts_text() -> String {
    let rows = [
        ("Ctrl+O", tr("Choose a folder to analyze")),
        ("Ctrl+D", tr("Show the drives")),
        ("F5", tr("Read the same folder again")),
        ("Esc", tr("Stop reading")),
        ("↑ ↓  PgUp PgDn  Home End", tr("Move in the list")),
        ("← →  Space", tr("Close or open a folder")),
        ("Enter", tr("Show the selected folder in the treemap")),
        ("Backspace", tr("Treemap: one folder up")),
        ("Ctrl+Backspace", tr("Treemap: the whole scan")),
        ("Double click", tr("Treemap: one folder down")),
        ("F6 / F7 / F8", tr("Treemap / pie / rings")),
        ("Ctrl+Enter", tr("Open the selected file or folder")),
        ("Ctrl+E", tr("Show it in the file manager")),
        ("Ctrl+C", tr("Copy its path")),
        ("Ctrl+1 / Ctrl+2", tr("Show file sizes / sizes on disk")),
        ("F9", tr("File types")),
        ("Ctrl+S", tr("Export the folder shown to CSV")),
        ("Delete", tr("Move it to the Recycle Bin or trash, after asking")),
    ];
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
}

fn app_icon() -> Option<WinIcon> {
    let n = 32;
    WinIcon::from_rgba(rsize::ui::appicon::render(n), n, n).ok()
}

#[cfg(windows)]
fn mnemonics_of(tree: &[(&'static str, Vec<commands::M>)]) -> String {
    tree.iter().filter_map(|(t, _)| tr(t).split('&').nth(1).and_then(|s| s.chars().next())).collect()
}

#[cfg(windows)]
fn hwnd_of(window: &Window) -> rt_sys::win::HWND {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get() as _,
        _ => std::ptr::null_mut(),
    }
}

/// Starts resident mode on Linux: without an icon, a hidden window could not
/// be reached or closed, so rsize then runs as a normal window.
#[cfg(not(windows))]
fn start_resident(shell: &mut Shell) {
    use rt_sys::linux::shell as ls;
    let proxy = std::sync::Mutex::new(shell.proxy.clone());
    let labels = ls::TrayLabels { tip: "rsize".into(), open: tr("Open rsize"), exit: tr("Exit") };
    // Only a resident rsize listens for later launches, as on Windows.
    let cfg = ls::Config { app: "rsize", icon: rsize::ui::appicon::render, tray: Some(labels), listen_without_icon: false };
    let resident = ls::Shell::new(
        cfg,
        Box::new(move |m| {
            if let Ok(p) = proxy.lock() {
                let _ = p.send_event(UserEvent::Shell(m));
            }
        }),
    );
    shell.tray = resident.has_icon();
    shell.resident = shell.tray.then_some(resident);
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
            self.start_scan(&p);
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
            UserEvent::Scan(gen, ev) => self.on_scan(gen, ev),
            UserEvent::Volumes(v) => {
                self.app.volumes = Some(v);
                self.redraw();
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
            UserEvent::A11y(ev) => match ev.window_event {
                accesskit_winit::WindowEvent::ActionRequested(req) if req.action == accesskit::Action::Click => {
                    let id = req.target_node.0;
                    if id == 6 {
                        self.dialog = None;
                    } else if let Some(&(node, _)) = id.checked_sub(100).and_then(|i| self.app.rows.get(i as usize)) {
                        self.app.select(node);
                    }
                    let _ = el;
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
                self.resized = Instant::now();
                self.redraw();
            }
            WindowEvent::ThemeChanged(t) => {
                self.system_dark = t == WinTheme::Dark;
                self.redraw();
            }
            WindowEvent::Focused(false) => self.save_settings(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::DroppedFile(p) => {
                let dir = if p.is_dir() { p } else { p.parent().map(Path::to_path_buf).unwrap_or(p) };
                self.start_scan(&dir);
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
                if self.menu_pointer(x, y) {
                    return;
                }
                if self.drag.is_some() {
                    self.drag_to(y);
                }
                self.pointer(x, y);
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                if self.drag.is_none() && (self.hot != Hot::None || self.hover.is_some()) {
                    self.hot = Hot::None;
                    self.hover = None;
                    self.redraw();
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => {
                    if let Some((x, y)) = self.cursor {
                        self.press(el, x, y);
                    }
                }
                ElementState::Released => {
                    if self.drag.take().is_some() {
                        self.save_settings();
                    }
                }
            },
            #[cfg(not(windows))]
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } if self.menu.is_open() => {
                self.menu.close();
                self.redraw();
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Right, .. } if self.dialog.is_none() => {
                if view::page(&self.app) == Page::Result && self.hover.is_some() {
                    if let Some(id) = self.hover {
                        self.app.select(id);
                    }
                    self.menu_dirty = true;
                    self.redraw();
                    self.context_menu(el);
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.dialog.is_none() => {
                #[cfg(not(windows))]
                if self.menu.is_open() {
                    if let Some((l, _)) = self.cursor.and_then(|(x, y)| self.menu.row_at(x, y)) {
                        let down = match delta {
                            MouseScrollDelta::LineDelta(_, y) => y < 0.0,
                            MouseScrollDelta::PixelDelta(p) => p.y < 0.0,
                        };
                        self.menu.wheel(l, if down { 2 } else { -2 });
                        self.redraw();
                    }
                    return;
                }
                let Some((x, y)) = self.cursor else { return };
                let rows = match delta {
                    MouseScrollDelta::LineDelta(_, l) => (-l * 3.0).round() as isize,
                    MouseScrollDelta::PixelDelta(p) => (-p.y as f32 / self.layout.row_h.max(1.0)).round() as isize,
                };
                let l = &self.layout;
                match view::page(&self.app) {
                    Page::Result if l.list.contains(x, y) || l.track.contains(x, y) || l.header.contains(x, y) => self.app.scroll_by(rows),
                    Page::Result if self.settings.chart == ChartType::Types && l.map.contains(x, y) => {
                        let n = self.types.as_ref().map_or(0, Vec::len);
                        let fit = view::types_layout(l).2;
                        let max = n.saturating_sub(fit) as isize;
                        self.app.types_scroll = (self.app.types_scroll as isize + rows).clamp(0, max.max(0)) as usize;
                    }
                    Page::Start if l.start_max > 0.0 => {
                        let step = 40.0 * self.scale();
                        self.app.start_scroll = (self.app.start_scroll + rows as f32 * step / 3.0).clamp(0.0, l.start_max);
                    }
                    _ => return,
                }
                self.redraw();
                self.pointer(x, y);
            }
            WindowEvent::RedrawRequested if !self.hidden => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // The elapsed time on the scan page moves even between reports.
        if self.app.scanning.is_some() {
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(500)));
            self.redraw();
        } else if let Some(due) = self.map_due {
            // A stretched treemap is made again once the resizing pauses.
            if Instant::now() >= due {
                self.redraw();
                el.set_control_flow(ControlFlow::Wait);
            } else {
                el.set_control_flow(ControlFlow::WaitUntil(due));
            }
        } else {
            el.set_control_flow(ControlFlow::Wait);
        }
    }
}

/// Whether "Analyze with rsize" is on the Explorer menu of folders, for this rsize.exe.
#[cfg(windows)]
fn integrated_here() -> bool {
    let exe = rt_sys::win::assoc::current_exe().unwrap_or_default();
    rt_sys::win::assoc::folder_status("rsize", &rt_sys::win::assoc::Keys::user("rsize"), &exe) == rt_sys::win::assoc::Status::Here
}

/// Adds "Analyze with rsize" to the Explorer menu of folders and drives
/// (current user only), or takes it out; returns the message to show.
#[cfg(windows)]
fn integrate(on: bool) -> Result<String, String> {
    use rt_sys::win::assoc;
    let keys = assoc::Keys::user("rsize");
    let failed = |e: assoc::Error| tf("Cannot change the Windows registry (error {}).", &[&e.to_string()]);
    if on {
        let exe = assoc::current_exe()?;
        assoc::register_folders("rsize", &keys, &exe, &tr("Analyze with rsize")).map_err(failed)?;
        Ok(tr("“Analyze with rsize” was added to the Explorer context menu of folders and drives."))
    } else {
        assoc::unregister_folders("rsize", &keys).map_err(failed)?;
        Ok(tr("“Analyze with rsize” was removed from the Explorer context menu."))
    }
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
    let args: Vec<PathBuf> = raw.iter().filter(|a| !a.starts_with("--")).map(|a| PathBuf::from(rt_sys::win::assoc::folder_arg(a))).collect();
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
    let mut shell = Shell {
        app: App::new(),
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
        layout: Layout::default(),
        map: None,
        map_key: None,
        map_due: None,
        chart: None,
        chart_key: None,
        hover_seg: None,
        types: None,
        types_key: None,
        resized: Instant::now(),
        hot: Hot::None,
        hover: None,
        drag: None,
        cursor_icon: CursorIcon::Default,
        handle: None,
        generation: 0,
        tree_gen: 0,
        dialog: help.then(|| Dialog { title: "rsize".into(), text: USAGE.into(), panel: Rect::default(), ok: Rect::default() }),
        last_click: None,
        args,
        #[cfg(windows)]
        elevated: rt_sys::win::elevated(),
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
    shell.load_volumes();
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rsize: {e}");
    }
}
