//! rsize: a portable disk space analyzer.
//!
//! The window is driven by winit and drawn on the CPU (tiny-skia + ab_glyph)
//! into a softbuffer surface. Scans run on background threads and report to
//! the window through the event loop.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rsize::app::{App, Scanning};
use rsize::chart::{Chart, Hit};
use rsize::settings::{ChartType, ViewMode};
use rsize::commands::{self, Command};
use rsize::i18n::{self, tf, tr};
use rsize::model::{Id, SizeMode};
use rsize::scan::{self, Event as ScanEvent};
use rsize::settings::{Settings, SPLIT_MAX, SPLIT_MIN, SPLIT_X_MAX, SPLIT_X_MIN};
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
use rt_ui::Message;
use winit::window::{CursorIcon, Theme as WinTheme, Window, WindowId};

mod input;
mod menus;
mod render;

enum UserEvent {
    A11y(accesskit_winit::Event),
    Scan(u64, ScanEvent),
    Volumes(Vec<Volume>),
    /// The space of the scanned folder's volume (for the scan of that generation).
    Space(u64, Option<volumes::Space>),
    /// The system's names of the scanned file types, by extension.
    TypeNames(u64, std::collections::HashMap<String, String>),
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


#[derive(Clone, Copy)]
enum Drag {
    Splitter,
    VSplitter,
    Thumb,
    ExtThumb,
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
    /// The treemap with the files of the picked type standing out, and
    /// what it was made for: the treemap and the type.
    lit: Option<((MapKey, u32), tiny_skia::Pixmap)>,
    /// What the table of file types was made for: folder, order, tree.
    exts_key: Option<(Id, SizeMode, u64)>,
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
    /// An in-app message (About, Keyboard shortcuts, the help).
    dialog: Option<Message>,
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
            rt_ui::window::remember_size(&w.window, &mut self.settings.window);
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

    /// Asks for the space of the volume `root` is on, in the background (a
    /// network drive can be slow to answer).
    fn load_space(&self, root: &Path) {
        let (proxy, gen, root) = (self.proxy.clone(), self.generation, root.to_path_buf());
        std::thread::spawn(move || {
            let _ = proxy.send_event(UserEvent::Space(gen, volumes::space_of(&root)));
        });
    }

    /// Asks the system what the scanned file types are called, in the
    /// background: thousands of them take a moment.
    fn load_type_names(&self) {
        let Some(t) = self.app.scan.as_ref().map(|s| &s.tree) else { return };
        let exts: Vec<String> = t.ext_ranking().iter().map(|&e| t.ext_name(e).to_string()).collect();
        let (proxy, gen) = (self.proxy.clone(), self.generation);
        std::thread::spawn(move || {
            #[cfg(windows)]
            let names = exts.into_iter().filter_map(|e| Some((e.clone(), rt_sys::win::file_type_name(&e)?))).collect();
            #[cfg(not(windows))]
            let names = {
                let _ = exts;
                std::collections::HashMap::new()
            };
            let _ = proxy.send_event(UserEvent::TypeNames(gen, names));
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
                        self.app.space = None;
                        self.load_space(&root);
                        self.load_type_names();
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
                // The pie and the rings need the larger area of the map layout.
                if kind != ChartType::Treemap {
                    self.settings.view = ViewMode::Map;
                }
                self.hover = None;
                self.hover_seg = None;
            }
            Command::View(mode) => {
                self.settings.view = mode;
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
            Command::Chart(kind) => self.settings.chart_shown() == kind,
            Command::View(mode) => self.settings.view == mode,
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
        self.dialog = Some(Message::new(title, text));
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

    // ------------------------------------------------------------ menus

    // ------------------------------------------------------------ input

    // ------------------------------------------------------------ frame

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let (w, h, maximized) = self.settings.window.unwrap_or((1100.0, 780.0, false));
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
        ("Right click", tr("Treemap: one folder up")),
        ("F3 / F4", tr("Folder view / map view")),
        ("F6 / F7 / F8", tr("Treemap / pie / rings")),
        ("Ctrl+Enter", tr("Open the selected file or folder")),
        ("Ctrl+E", tr("Show it in the file manager")),
        ("Ctrl+C", tr("Copy its path")),
        ("Ctrl+1 / Ctrl+2", tr("Show file sizes / sizes on disk")),
        ("Ctrl+S", tr("Export the folder shown to CSV")),
        ("Delete", tr("Move it to the Recycle Bin or trash, after asking")),
    ];
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
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
            UserEvent::Space(gen, space) if gen == self.generation => {
                self.app.space = space;
                self.redraw();
            }
            UserEvent::TypeNames(gen, names) if gen == self.generation => {
                self.app.type_names = names;
                self.app.sort_exts();
                self.redraw();
            }
            UserEvent::Space(..) | UserEvent::TypeNames(..) => {}
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
                if self.menu_pointer(el, x, y) {
                    return;
                }
                if self.drag.is_some() {
                    self.drag_to(x, y);
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
                let in_chart = self.cursor.is_some_and(|(x, y)| self.layout.map.contains(x, y));
                if view::page(&self.app) == Page::Result && in_chart && !self.mods.shift_key() {
                    // In the chart a right click goes one folder up, as Backspace
                    // (a double click goes down, as Enter); Shift+right click
                    // opens the item's menu there.
                    self.app.zoom_out();
                    self.hover = None;
                    self.hover_seg = None;
                    self.menu_dirty = true;
                    self.redraw();
                } else if view::page(&self.app) == Page::Result && self.hover.is_some() {
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
                let Some((x, y)) = self.cursor else { return };
                let rows = match delta {
                    MouseScrollDelta::LineDelta(_, l) => (-l * 3.0).round() as isize,
                    MouseScrollDelta::PixelDelta(p) => (-p.y as f32 / self.layout.row_h.max(1.0)).round() as isize,
                };
                let l = &self.layout;
                match view::page(&self.app) {
                    Page::Result if l.list.contains(x, y) || l.track.contains(x, y) || l.header.contains(x, y) => self.app.scroll_by(rows),
                    Page::Result if l.ext_list.contains(x, y) || l.ext_track.contains(x, y) || l.ext_header.contains(x, y) => self.app.scroll_types_by(rows),
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
        exts_key: None,
        lit: None,
        resized: Instant::now(),
        hot: Hot::None,
        hover: None,
        drag: None,
        cursor_icon: CursorIcon::Default,
        handle: None,
        generation: 0,
        tree_gen: 0,
        dialog: help.then(|| Message::new("rsize", USAGE)),
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
