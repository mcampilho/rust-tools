//! rinst: a portable uninstaller.
//!
//! The window is driven by winit and drawn on the CPU (tiny-skia + ab_glyph)
//! into a softbuffer surface. The programs and their shortcuts are read on a
//! background thread, then their icons, one batch at a time.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rinst::app::{App, Col, Found, Whose};
use rinst::commands::{self, Command};
use rinst::i18n::{self, tf, tr};
use rinst::settings::Settings;
use rinst::storage;
use rinst::ui::gfx::{Color, Gfx, Rect};
use rinst::ui::theme::Theme;
use rinst::ui::view::{self, Hot, Layout};
use rt_ui::Message;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tiny_skia::Pixmap;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Theme as WinTheme, Window, WindowId};

mod input;
mod menus;
mod render;
mod uninstalling;

/// An icon read: the program's index, and its pixels (width, height, RGBA).
type Icon = (usize, u32, u32, Vec<u8>);

enum UserEvent {
    /// A step of Check for Updates.
    Update(rt_ui::update::Step),
    A11y(accesskit_winit::Event),
    /// The programs read, for the reading of this generation.
    Read(u64, Box<Found>),
    /// Icons read (only Windows reads them yet).
    #[cfg_attr(not(windows), allow(dead_code))]
    Icons(u64, Vec<Icon>),
    /// The uninstalling queue.
    Job(uninstalling::JobMsg),
    /// What the programs uninstalled left behind.
    Leftovers(Vec<rinst::leftovers::Leftover>),
    /// How removing the leftovers ticked went (their indexes), and the
    /// `.reg` file with the keys.
    Removed(Vec<(usize, rinst::leftovers::Removed)>, std::path::PathBuf),
    /// What `winget list` said (None: winget is not there, or failed).
    Winget(u64, Option<Vec<rinst::winget::Row>>),
    /// The tabs of DISM and the development tools that are there.
    Tools(u64, Vec<(rinst::app::Tab, Vec<rinst::inventory::Program>)>),
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
    hot: Hot,
    dragging: bool,
    /// Selecting the search's text with the pointer held down.
    selecting: bool,
    /// The last left click in the search (when, where), to see a double click.
    last_click: Option<(std::time::Instant, f32)>,
    cursor_icon: CursorIcon,
    generation: u64,
    /// The next reading is the first: it may come from the privileged service.
    first_load: bool,
    /// The icons read so far, by program index.
    icons: HashMap<usize, Pixmap>,
    /// winget's answer when it came before the list.
    pending_winget: Option<Option<Vec<rinst::winget::Row>>>,
    /// The tools' tabs when they came before the list.
    pending_tools: Option<Vec<(rinst::app::Tab, Vec<rinst::inventory::Program>)>>,
    /// Running with administrator rights: every user can be read.
    admin: bool,
    dialog: Option<Message>,
    /// Uninstalling now, and, once it is done, the report waiting for the
    /// list read again.
    job: Option<uninstalling::Job>,
    report: Option<uninstalling::Job>,
    /// When the queue started, for what turns while it runs.
    job_started: Option<std::time::Instant>,
    #[cfg(not(windows))]
    menu: rinst::ui::menu::Menu,
    /// Resident mode (`--tray`): closing the window hides it to the
    /// notification area.
    tray: bool,
    hidden: bool,
    maximize_on_show: bool,
    /// Starts with the session (as `rinst --tray`).
    autostart: bool,
    #[cfg(not(windows))]
    resident: Option<rt_sys::linux::shell::Shell>,
}

/// Straight RGBA to a pixmap (premultiplied).
fn pixmap(w: u32, h: u32, rgba: &[u8]) -> Option<Pixmap> {
    let mut pm = Pixmap::new(w, h)?;
    for (d, s) in pm.pixels_mut().iter_mut().zip(rgba.as_chunks::<4>().0) {
        *d = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
    }
    Some(pm)
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
        self.settings.whose = self.app.whose;
        self.settings.show_system = self.app.show_system;
        self.settings.show_updates = self.app.show_updates;
        let store = self.settings.to_store();
        self.location.save(&store);
    }

    /// Reads the programs and their shortcuts on a thread, then their icons.
    fn refresh(&mut self) {
        self.generation += 1;
        let gen = self.generation;
        self.app.loading = true;
        self.pending_winget = None;
        let proxy = self.proxy.clone();
        // winget takes seconds: on a thread of its own, joined when it answers.
        let p2 = proxy.clone();
        std::thread::spawn(move || {
            let rows = rinst::winget::run().map(|t| rinst::winget::parse(&t));
            let _ = p2.send_event(UserEvent::Winget(gen, rows));
        });
        // DISM, npm and pip take a second or more too.
        self.pending_tools = None;
        let p3 = proxy.clone();
        let admin = self.admin;
        // The first reading may come from the privileged service, if it runs and is not elevated
        // already: a user who asks again (or uninstalls something) wants what is there now.
        let use_snapshot = std::mem::take(&mut self.first_load) && !admin;
        std::thread::spawn(move || {
            let tools = rinst::tools::read_all(&rinst::inventory::me(), admin);
            let _ = p3.send_event(UserEvent::Tools(gen, tools));
        });
        std::thread::spawn(move || {
            let inv = use_snapshot
                .then(|| rt_inst::snapshot::fetch(24 * 3600))
                .flatten()
                .map(|(mut inv, _age)| {
                    inv.me = rinst::inventory::me();
                    inv
                })
                .unwrap_or_else(rinst::inventory::read);
            let shortcuts = rinst::shortcuts::read(&inv.me, &inv.other_users);
            let mut found = Found::new(inv, shortcuts);
            let scoop = rinst::managers::scoop_all(&found.inv.me);
            found.scoop = (!scoop.is_empty()).then_some(scoop);
            found.chocolatey = rinst::managers::choco_root().map(|r| rinst::managers::choco_packages(&r));
            #[cfg(not(windows))]
            {
                found.flatpak = rinst::linux_pkgs::flatpak(&found.inv.me.name);
                found.snap = rinst::linux_pkgs::snap();
            }
            let wanted: Vec<IconOf> = found
                .inv
                .programs
                .iter()
                .enumerate()
                .filter_map(|(i, p)| match (&p.icon, &p.app_id) {
                    (Some((f, n)), _) => Some((i, IconFrom::File(f.clone(), *n))),
                    (None, Some(a)) => Some((i, IconFrom::App(a.clone()))),
                    _ => None,
                })
                .collect();
            let _ = proxy.send_event(UserEvent::Read(gen, Box::new(found)));
            read_icons(gen, wanted, &proxy);
        });
        self.menu_dirty = true;
        self.redraw();
    }

    fn update_title(&self) {
        let Some(w) = &self.win else { return };
        let title = if self.admin { format!("rinst ({})", tr("Administrator")) } else { "rinst".to_string() };
        if w.window.title() != title {
            w.window.set_title(&title);
        }
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self, el: &ActiveEventLoop, cmd: Command) {
        // Delete on the leftovers removes them.
        let leftovers_tab = self.app.tab == rinst::app::Tab::Leftovers;
        let cmd = match cmd {
            Command::Uninstall | Command::UninstallQuiet if leftovers_tab => Command::RemoveLeftovers,
            Command::OpenFolder if leftovers_tab => Command::OpenLeftover,
            c => c,
        };
        if !self.enabled(cmd) {
            return;
        }
        let copy = |text: String, app: &mut App| {
            if !text.is_empty() {
                rt_sys::set_clipboard_text(&text);
                app.note = Some(tr("Copied to the clipboard."));
            }
        };
        match cmd {
            Command::Refresh => self.refresh(),
            #[cfg(windows)]
            Command::RestartAdmin => {
                self.save_settings();
                if rt_sys::win::restart_elevated("") {
                    self.quit(el);
                }
            }
            Command::Export => self.export(),
            Command::Uninstall => self.uninstall(rinst::uninstall::How::Normal),
            Command::UninstallQuiet => self.uninstall(rinst::uninstall::How::Quiet),
            Command::Modify => self.uninstall(rinst::uninstall::How::Modify),
            Command::SelectAll => self.app.select_all(),
            Command::Exit => self.quit(el),
            Command::Autostart => match rt_sys::autostart::set("rinst", !self.autostart) {
                Ok(()) => self.autostart = rt_sys::autostart::enabled("rinst"),
                Err(e) => self.info("rinst", &tf("Cannot change how it starts: {}", &[&e])),
            },
            Command::OpenLeftover => {
                if let Some(dir) = self.leftover_folder() {
                    open_path(&dir);
                }
            }
            Command::OpenFolder => {
                if let Some(dir) = self.app.selected().and_then(folder_of) {
                    open_path(&dir);
                }
            }
            Command::TextCut => {
                let mut cut = String::new();
                self.app.edit_search(|t| cut = t.cut());
                rt_sys::set_clipboard_text(&cut);
            }
            Command::TextCopy => {
                rt_sys::set_clipboard_text(self.app.search.selected_text());
            }
            Command::TextPaste => self.paste(),
            Command::TextDelete => self.app.edit_search(|t| t.delete(false, false)),
            Command::TextSelectAll => self.app.edit_search(|t| t.select_all()),
            Command::FindLeftovers => self.look_for_orphans(),
            Command::RemoveLeftovers => self.remove_leftovers(),
            Command::CloseLeftovers => self.app.set_leftovers(None, false),
            Command::CopyName => {
                let t = self.app.selected().map(|p| p.name.clone()).unwrap_or_default();
                copy(t, &mut self.app);
            }
            Command::CopyCommand => {
                let t = self.app.selected().and_then(|p| p.uninstall.clone()).unwrap_or_default();
                copy(t, &mut self.app);
            }
            Command::CopyKey => {
                let t = self.app.selected().map(|p| p.key.clone()).unwrap_or_default();
                copy(t, &mut self.app);
            }
            Command::CopyDetails => {
                let t = view::details_text(&self.app);
                copy(t, &mut self.app);
            }
            Command::ClearSearch => self.app.clear_search(),
            Command::Whose(i) => {
                self.app.whose = Whose::ALL[(i as usize).min(3)];
                self.app.rebuild();
            }
            Command::ShowSystem => {
                self.app.show_system = !self.app.show_system;
                self.app.rebuild();
            }
            Command::ShowUpdates => {
                self.app.show_updates = !self.app.show_updates;
                self.app.rebuild();
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
            Command::CheckUpdates => rt_ui::update::check("rinst", env!("CARGO_PKG_VERSION"), self.update_sender()),
            Command::About => {
                let text = tf(
                    "rinst {}\n\nLists the programs installed for every user, with their shortcuts, written in Rust.\nNo telemetry, no network access.\n\nMIT License.",
                    &[env!("CARGO_PKG_VERSION")],
                );
                self.info(&tr("About rinst"), &text);
            }
        }
        self.menu_dirty = true;
        self.redraw();
    }

    fn checked(&self, c: Command) -> bool {
        match c {
            Command::Theme(t) => self.settings.theme == t,
            Command::UiLanguage(0) => self.settings.language.is_none(),
            Command::UiLanguage(i) => self.settings.language == i18n::Lang::ALL.get(i as usize - 1).copied(),
            Command::Whose(i) => Whose::ALL.get(i as usize) == Some(&self.app.whose),
            Command::ShowSystem => self.app.show_system,
            Command::ShowUpdates => self.app.show_updates,
            Command::Autostart => self.autostart,
            _ => false,
        }
    }

    fn enabled(&self, c: Command) -> bool {
        let sel = self.app.selected();
        let leftovers_tab = self.app.tab == rinst::app::Tab::Leftovers;
        let lo_idle = self.app.leftovers.as_ref().is_some_and(|l| !l.searching && !l.removing);
        match c {
            #[cfg(windows)]
            Command::RestartAdmin => !self.admin,
            Command::Refresh => !self.app.loading && !self.busy(),
            Command::RemoveLeftovers => leftovers_tab && lo_idle && !self.app.chosen().is_empty(),
            Command::FindLeftovers => {
                !self.busy() && self.app.found.is_some() && self.app.leftovers.as_ref().is_none_or(|l| !l.searching && !l.removing)
            }
            Command::CloseLeftovers => lo_idle,
            Command::OpenLeftover | Command::OpenFolder if leftovers_tab => self.leftover_folder().is_some(),
            Command::OpenLeftover => false,
            Command::CopyDetails if leftovers_tab => !self.app.rows.is_empty(),
            Command::Uninstall | Command::UninstallQuiet if leftovers_tab => lo_idle && !self.app.chosen().is_empty(),
            Command::Modify | Command::CopyName | Command::CopyKey | Command::CopyCommand | Command::Export if leftovers_tab => false,
            Command::Uninstall | Command::UninstallQuiet => !self.busy() && !self.app.chosen().is_empty(),
            // On for one program: when it has nothing to change with, a click
            // says why (a button always off says nothing).
            Command::Modify => !self.busy() && self.app.chosen_programs().len() == 1,
            Command::TextCut | Command::TextCopy | Command::TextDelete => self.app.search.selection().is_some(),
            Command::TextPaste => rt_sys::clipboard_text().is_some_and(|t| !t.is_empty()),
            Command::TextSelectAll => !self.app.search.text.is_empty(),
            Command::SelectAll => !self.app.rows.is_empty(),
            Command::Export => !self.app.rows.is_empty(),
            Command::OpenFolder => sel.and_then(folder_of).is_some(),
            Command::CopyName | Command::CopyKey | Command::CopyDetails => sel.is_some(),
            Command::CopyCommand => sel.is_some_and(|p| p.uninstall.is_some()),
            Command::ClearSearch => !self.app.search.text.is_empty(),
            _ => true,
        }
    }

    fn info(&mut self, title: &str, text: &str) {
        self.dialog = Some(Message::new(title, text));
        self.dragging = false;
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
        self.dragging = false;
        self.selecting = false;
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
        let leftovers_busy = self.app.leftovers.as_ref().is_some_and(|l| l.searching || l.removing);
        if was_hidden && self.job.is_none() && self.app.busy.is_none() && !leftovers_busy {
            self.refresh();
        }
        self.redraw();
    }

    /// Writes the list shown to a CSV file the user chooses.
    fn export(&mut self) {
        let name = "rinst-programs.csv";
        #[cfg(windows)]
        let chosen = self.win.as_ref().and_then(|w| {
            let filter = rt_sys::win::dialogs::filter(&[(tr("CSV files"), "*.csv")]);
            rt_sys::win::dialogs::save_file(w.hwnd, name, None, &filter)
        });
        #[cfg(not(windows))]
        let chosen = rt_sys::linux::dialogs::save_file(name, None);
        let Some(path) = chosen else { return };
        let header: Vec<String> = [
            "Name",
            "Publisher",
            "Version",
            "Size",
            "Installed",
            "For",
            "Source",
            "winget id",
            "Newer version (winget)",
            "Install folder",
            "Uninstall command",
            "Registry key",
        ]
        .iter()
        .map(|h| tr(h))
        .collect();
        let csv = self.app.csv(&header, &view::scope_text, &view::sources_text);
        self.app.note = Some(match std::fs::write(&path, csv) {
            Ok(()) => tf("The list was saved to {}.", &[&path.display().to_string()]),
            Err(e) => tf("Could not save {}: {}", &[&path.display().to_string(), &e.to_string()]),
        });
        self.redraw();
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

/// Where a program's icon comes from: a file and an index, or an app of a package.
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(not(windows), allow(dead_code))]
enum IconFrom {
    File(PathBuf, i32),
    App(String),
}

type IconOf = (usize, IconFrom);

/// Reads the icons on this thread and sends them in batches.
fn read_icons(gen: u64, wanted: Vec<IconOf>, proxy: &EventLoopProxy<UserEvent>) {
    #[cfg(windows)]
    {
        type Pixels = Option<(u32, u32, Vec<u8>)>;
        let mut cache: HashMap<IconFrom, Pixels> = HashMap::new();
        let mut batch = Vec::new();
        for (i, from) in wanted {
            let got = cache
                .entry(from.clone())
                .or_insert_with(|| match &from {
                    IconFrom::File(f, n) => rt_sys::win::icon::extract(f, *n),
                    IconFrom::App(a) => rinst::appx::icon(a, 32),
                })
                .clone();
            if let Some((w, h, px)) = got {
                batch.push((i, w, h, px));
            }
            if batch.len() >= 16 {
                let _ = proxy.send_event(UserEvent::Icons(gen, std::mem::take(&mut batch)));
            }
        }
        if !batch.is_empty() {
            let _ = proxy.send_event(UserEvent::Icons(gen, batch));
        }
    }
    #[cfg(not(windows))]
    let _ = (gen, wanted, proxy);
}

/// The folder of a program: its install folder, or its uninstaller's.
fn folder_of(p: &rinst::inventory::Program) -> Option<PathBuf> {
    p.location.clone().filter(|l| l.is_dir()).or_else(|| {
        p.uninstall
            .as_deref()
            .and_then(rinst::inventory::command_file)
            .and_then(|f| f.parent().map(Path::to_path_buf))
            .filter(|d| d.is_absolute() && d.is_dir())
    })
}

/// Opens a folder in the file manager.
fn open_path(p: &Path) {
    #[cfg(windows)]
    rt_sys::win::shell_open(&p.to_string_lossy());
    #[cfg(not(windows))]
    rt_sys::linux::xdg_open(p.as_os_str().to_os_string());
}

fn shortcuts_text() -> String {
    let rows = [
        (tr("Letters"), tr("Find a program by its name, publisher or folder")),
        ("Esc".to_string(), tr("Clear the search")),
        ("↑ ↓  PgUp PgDn  Home End".to_string(), tr("Move in the list")),
        ("Delete".to_string(), tr("Uninstall the programs selected")),
        ("Shift+Delete".to_string(), tr("Uninstall them quietly")),
        ("Ctrl+A".to_string(), tr("Select every program shown")),
        (tr("Ctrl+click, Shift+click"), tr("Select several programs")),
        (tr("Ctrl+V"), tr("Paste into the search")),
        (tr("Space"), tr("Tick the programs selected: the buttons act on those ticked, whatever the search shows")),
        ("F5".to_string(), tr("Read the programs again")),
        ("Ctrl+E".to_string(), tr("Open the install folder")),
        ("Ctrl+C".to_string(), tr("Copy the name")),
        ("Ctrl+Shift+C".to_string(), tr("Copy all the details")),
        ("Ctrl+S".to_string(), tr("Save the list as CSV")),
    ];
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
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
            UserEvent::Read(gen, found) if gen == self.generation => {
                self.icons.clear();
                self.app.set_found(*found);
                if let Some(rows) = self.pending_winget.take() {
                    self.app.set_winget(rows);
                }
                if let Some(tools) = self.pending_tools.take() {
                    self.app.set_tools(tools);
                }
                // The report waits for every list (an npm package may be gone).
                if self.app.found.as_ref().is_some_and(|f| f.tools_done) {
                    self.show_report();
                }
                self.menu_dirty = true;
                self.redraw();
            }
            UserEvent::Read(..) => {}
            UserEvent::Icons(gen, icons) if gen == self.generation => {
                for (i, w, h, px) in icons {
                    if let Some(pm) = pixmap(w, h, &px) {
                        self.icons.insert(i, pm);
                    }
                }
                self.redraw();
            }
            UserEvent::Icons(..) => {}
            UserEvent::Winget(gen, rows) if gen == self.generation => {
                // The list may not be there yet: then it waits for it.
                if self.app.found.is_some() {
                    self.app.set_winget(rows);
                } else {
                    self.pending_winget = Some(rows);
                }
                self.redraw();
            }
            UserEvent::Winget(..) => {}
            UserEvent::Tools(gen, tools) if gen == self.generation => {
                if self.app.found.as_ref().is_some_and(|f| !f.tools_done) && !self.app.loading {
                    self.app.set_tools(tools);
                    self.show_report();
                } else {
                    self.pending_tools = Some(tools);
                }
                self.menu_dirty = true;
                self.redraw();
            }
            UserEvent::Tools(..) => {}
            UserEvent::Job(m) => self.job_event(m),
            UserEvent::Leftovers(items) => self.leftovers_found(items),
            UserEvent::Removed(results, backup) => self.leftovers_removed(results, &backup),
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
                        self.app.select(i as usize);
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
                if self.dragging {
                    self.drag_to(y);
                }
                if self.selecting {
                    // Dragging over the search's text selects it.
                    let s = self.scale();
                    let at = view::search_offset(&mut self.gfx, &self.app, &self.layout, x, s);
                    self.app.search.set_caret(at, true);
                    self.redraw();
                }
                self.pointer(x, y);
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                if !self.dragging && self.hot != Hot::None {
                    self.hot = Hot::None;
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
                    self.dragging = false;
                    self.selecting = false;
                }
            },
            #[cfg(not(windows))]
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } if self.menu.is_open() => {
                self.menu.close();
                self.redraw();
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Right, .. } if self.dialog.is_none() => {
                if let Hot::Row(i) = self.hot {
                    if self.app.tab == rinst::app::Tab::Leftovers {
                        return;
                    }
                    // A row outside the selection: it alone; inside: the selection stays.
                    if !self.app.is_marked(i) {
                        self.app.select(i);
                    }
                    self.menu_dirty = true;
                    self.redraw();
                    self.context_menu(el, commands::program_menu());
                } else if self.hot == Hot::Search {
                    self.app.search_focus = true;
                    self.redraw();
                    self.context_menu(el, commands::search_menu());
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
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, l) => -l * 3.0,
                    MouseScrollDelta::PixelDelta(p) => -p.y as f32 / self.layout.row_h.max(20.0),
                };
                let s = self.scale();
                let over_details = self.cursor.is_some_and(|(x, y)| self.layout.details.contains(x, y));
                if over_details {
                    let max = (view::details_height(&self.app, s) - self.layout.details.h).max(0.0);
                    self.app.details_scroll = (self.app.details_scroll + lines * 18.0 * s).clamp(0.0, max);
                } else {
                    self.app.scroll_by(lines.round() as isize);
                }
                self.redraw();
                if let Some((x, y)) = self.cursor {
                    self.pointer(x, y);
                }
            }
            WindowEvent::RedrawRequested if !self.hidden => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // While uninstalling, the bar and the marks move, so it shows it is
        // working even when the uninstaller shows nothing.
        if let (Some(t0), Some(b)) = (self.job_started, &mut self.app.busy) {
            const FRAME: std::time::Duration = std::time::Duration::from_millis(40);
            let phase = (t0.elapsed().as_secs_f32() / 1.6).fract();
            if (phase - b.phase).abs() > 0.01 {
                b.phase = phase;
                if let Some(w) = &self.win {
                    w.window.request_redraw();
                }
            }
            el.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now() + FRAME));
        } else {
            el.set_control_flow(ControlFlow::Wait);
        }
    }
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
    let mut app = App::new();
    app.whose = settings.whose;
    app.show_system = settings.show_system;
    app.show_updates = settings.show_updates;
    app.sort = Col::Name;
    let mut shell = Shell {
        app,
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
        hot: Hot::None,
        dragging: false,
        selecting: false,
        last_click: None,
        cursor_icon: CursorIcon::Default,
        generation: 0,
        first_load: true,
        icons: HashMap::new(),
        pending_winget: None,
        pending_tools: None,
        #[cfg(windows)]
        admin: rt_sys::win::elevated(),
        #[cfg(not(windows))]
        admin: rinst::linux_pkgs::is_root(),
        dialog: help.then(|| Message::new("rinst", USAGE)),
        job: None,
        report: None,
        job_started: None,
        #[cfg(not(windows))]
        menu: rinst::ui::menu::Menu::default(),
        tray,
        hidden: false,
        maximize_on_show: false,
        autostart: rt_sys::autostart::enabled("rinst"),
        #[cfg(not(windows))]
        resident: None,
    };
    shell.apply_language();
    // A first reading at once, unless it starts hidden (it reads when shown).
    if !tray {
        shell.refresh();
    }
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rinst: {e}");
    }
}
