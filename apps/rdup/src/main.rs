//! rdup: a portable duplicate file finder.
//!
//! The window is driven by winit and drawn on the CPU (tiny-skia + ab_glyph)
//! into a softbuffer surface. Searches run on background threads and report
//! to the window through the event loop.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rdup::actions::{Op, Rule};
use rdup::format;
use rdup::app::{App, Row, Searching, View};
use rdup::commands::{self, Command};
use rdup::i18n::{self, tf, tr};
use rdup::search::{self, Event as SearchEvent, Failure, Options};
use rdup::settings::Settings;
use rdup::storage;
use rdup::ui::gfx::{Color, Gfx, Rect};
use rdup::ui::theme::Theme;
use rdup::ui::view::{self, Hot, Layout, Page};
use rdup::volumes::{self, Volume};
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
    /// Moving the marked copies: how many done, of how many; then what was
    /// moved and what was not (with the reason).
    Acting(usize, usize),
    Acted(Vec<PathBuf>, Vec<(PathBuf, String)>),
    Search(u64, SearchEvent),
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
const IDENT: rt_sys::win::Ident = rt_sys::win::Ident { name: "rdup", copydata: 0x5244_5550 }; // "RDUP"

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
    /// Dragging the scroll bar's thumb.
    dragging: bool,
    /// Selecting text in a filter box with the pointer.
    selecting: bool,
    /// The last click in a filter box, to see a double click.
    last_click: Option<(Instant, f32, f32)>,
    cursor_icon: CursorIcon,
    handle: Option<search::Handle>,
    generation: u64,
    /// An in-app message (About, Keyboard shortcuts, the help).
    dialog: Option<Message>,
    /// Marked copies being moved: (done, of how many).
    acting: Option<(usize, usize)>,
    /// Running with administrator rights (NTFS can be read directly).
    #[cfg(windows)]
    elevated: bool,
    #[cfg(not(windows))]
    menu: rdup::ui::menu::Menu,
    /// Resident mode (`--tray`): closing the window hides it to the
    /// notification area.
    tray: bool,
    hidden: bool,
    /// The window was last maximized: done when it is first shown.
    maximize_on_show: bool,
    /// The notification-area icon and the socket for later launches (Linux).
    #[cfg(not(windows))]
    resident: Option<rt_sys::linux::shell::Shell>,
    /// "Find duplicates with rdup" is on the Explorer menu of folders, for this rdup.exe.
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
        self.settings.roots = self.app.chosen.clone();
        if self.app.volumes.is_some() {
            self.settings.folders = self.app.folders();
        }
        self.settings.choice = self.app.choice.clone();
        self.settings.share = self.app.share;
        self.settings.compare = self.app.compare;
        let store = self.settings.to_store();
        self.location.save(&store);
    }

    // ------------------------------------------------------------ searches

    fn load_volumes(&self) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let _ = proxy.send_event(UserEvent::Volumes(volumes::list()));
        });
    }

    fn start_search(&mut self) {
        if self.app.chosen.is_empty() {
            return;
        }
        let Ok(filter) = self.app.choice.filter() else { return };
        self.app.focus = None;
        self.app.calendar = None;
        if let Some(h) = self.handle.take() {
            h.cancel();
        }
        self.generation += 1;
        let gen = self.generation;
        let roots = search::distinct_roots(&self.app.chosen);
        self.app.searching = Some(Searching { roots: roots.clone(), progress: Default::default(), started: Instant::now(), generation: gen });
        self.app.error = None;
        let proxy = self.proxy.clone();
        let lang = i18n::lang();
        self.handle = Some(search::start(
            roots,
            Options { filter, files: self.app.compare[0], folders: self.app.compare[1], cache: self.location.file("hashes.bin") },
            Box::new(move |e| {
                i18n::set_lang(lang);
                let _ = proxy.send_event(UserEvent::Search(gen, e));
            }),
        ));
        self.dragging = false;
        self.menu_dirty = true;
        self.save_settings();
        self.redraw();
    }

    fn on_search(&mut self, gen: u64, ev: SearchEvent) {
        if gen != self.generation {
            return;
        }
        match ev {
            SearchEvent::Progress(p) => {
                if let Some(s) = &mut self.app.searching {
                    s.progress = p;
                }
            }
            SearchEvent::Done(result) => {
                self.app.searching = None;
                self.handle = None;
                match result {
                    Ok(found) => self.app.set_found(*found),
                    Err(Failure::Stopped) => {
                        self.app.show_start = true;
                        self.app.error = Some(tr("The search was stopped."));
                    }
                    Err(Failure::Unreadable(why)) => {
                        self.app.show_start = true;
                        self.app.error = Some(tf("Could not read: {}", &[&why]));
                    }
                }
                self.menu_dirty = true;
            }
        }
        self.redraw();
    }

    fn update_title(&self) {
        let Some(w) = &self.win else { return };
        let title = match view::page(&self.app) {
            Page::Searching => format!("{} - rdup", tr("Searching")),
            _ => "rdup".to_string(),
        };
        if w.window.title() != title {
            w.window.set_title(&title);
        }
    }

    fn add_folder(&mut self) {
        let title = tr("Choose a folder to search");
        #[cfg(windows)]
        let chosen = self.win.as_ref().and_then(|w| rt_sys::win::dialogs::pick_folder(w.hwnd, &title));
        #[cfg(not(windows))]
        let chosen = rt_sys::linux::dialogs::pick_folder(&title);
        if let Some(p) = chosen {
            self.settings.last_dir = p.display().to_string();
            self.add_root(&p);
        }
    }

    /// Adds a folder to the search and shows the start page with it.
    fn add_root(&mut self, p: &Path) {
        self.app.add_folder(p);
        self.app.show_start = true;
        self.app.error = None;
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self, el: &ActiveEventLoop, cmd: Command) {
        if !self.enabled(cmd) {
            return;
        }
        match cmd {
            Command::AddFolder => self.add_folder(),
            Command::Search => self.start_search(),
            Command::NewSearch => {
                self.app.show_start = true;
                self.app.volumes = None;
                self.load_volumes();
            }
            Command::Stop => {
                if let Some(h) = &self.handle {
                    h.cancel();
                }
            }
            #[cfg(windows)]
            Command::RestartAdmin => {
                self.save_settings();
                // The new instance searches the same drives and folders, without
                // their last backslash, which would escape the closing quote.
                let args: Vec<String> = self.app.chosen.iter().map(|r| format!("\"{}\"", r.display().to_string().trim_end_matches('\\'))).collect();
                if rt_sys::win::restart_elevated(&args.join(" ")) {
                    self.quit(el);
                }
            }
            #[cfg(windows)]
            Command::ExplorerMenu => self.integration(!self.explorer_menu),
            Command::Exit => self.quit(el),
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
            Command::ToggleMark => {
                if let Some(i) = self.app.sel {
                    self.toggle_mark(i);
                }
            }
            Command::KeepNewest => self.apply_rule(Rule::Newest),
            Command::KeepOldest => self.apply_rule(Rule::Oldest),
            Command::KeepShortest => self.apply_rule(Rule::Shortest),
            Command::KeepInFolder => {
                if let Some(dir) = self.pick_folder(&tr("Choose the folder whose copies to keep")) {
                    self.apply_rule(Rule::InFolder(dir));
                }
            }
            Command::UnmarkAll => self.app.unmark_all(),
            Command::Recycle => self.act(Op::Recycle),
            Command::MoveTo => {
                if let Some(dir) = self.pick_folder(&tr("Choose where to move the marked copies")) {
                    self.act(Op::MoveTo(dir));
                }
            }
            Command::Export => self.export(),
            Command::ShowFiles => self.app.set_view(View::Files),
            Command::ShowFolders => self.app.set_view(View::Folders),
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
                    "rdup {}\n\nFinds files with the same contents on drives and folders, written in Rust.\nNo telemetry, no network access.\n\nMIT License.",
                    &[env!("CARGO_PKG_VERSION")],
                );
                self.info(&tr("About rdup"), &text);
            }
        }
        self.menu_dirty = true;
        self.redraw();
    }

    fn checked(&self, c: Command) -> bool {
        match c {
            Command::Theme(t) => self.settings.theme == t,
            Command::ShowFiles => self.app.view == View::Files,
            Command::ShowFolders => self.app.view == View::Folders,
            Command::UiLanguage(0) => self.settings.language.is_none(),
            Command::UiLanguage(i) => self.settings.language == i18n::Lang::ALL.get(i as usize - 1).copied(),
            #[cfg(windows)]
            Command::ExplorerMenu => self.explorer_menu,
            _ => false,
        }
    }

    fn enabled(&self, c: Command) -> bool {
        let result = view::page(&self.app) == Page::Result;
        let idle = self.app.searching.is_none();
        match c {
            Command::Search => idle && !self.app.chosen.is_empty() && self.app.choice.filter().is_ok() && self.app.compare.contains(&true),
            Command::AddFolder => idle,
            Command::NewSearch => idle && self.app.has_result() && !self.app.show_start,
            Command::Stop => !idle,
            #[cfg(windows)]
            Command::RestartAdmin => !self.elevated,
            Command::CopyPath | Command::OpenItem | Command::Reveal => result && self.app.selected_path().is_some(),
            Command::ToggleMark => result && self.app.sel.is_some_and(|i| self.app.markable(i)) && self.acting.is_none(),
            Command::KeepNewest | Command::KeepOldest | Command::KeepShortest | Command::KeepInFolder => {
                result && self.acting.is_none() && (0..self.app.rows.len()).any(|i| self.app.markable(i))
            }
            Command::UnmarkAll | Command::Recycle | Command::MoveTo => result && self.acting.is_none() && !self.app.marked().is_empty(),
            Command::Export => result && !self.app.rows.is_empty(),
            Command::ShowFiles => result && self.app.found.as_ref().is_some_and(|f| !f.groups.is_empty() && f.look == [true, true]),
            Command::ShowFolders => result && self.app.found.as_ref().is_some_and(|f| !f.groups.is_empty() && f.look[1]),
            _ => true,
        }
    }

    fn info(&mut self, title: &str, text: &str) {
        self.dialog = Some(Message::new(title, text));
        self.dragging = false;
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

    /// Hides the window to the notification area; a search goes on.
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
    /// and searches them.
    fn open_paths(&mut self, el: &ActiveEventLoop, paths: Vec<PathBuf>) {
        self.show(el);
        if !paths.is_empty() && self.acting.is_none() {
            self.app.chosen = paths.iter().map(|p| rt_scan::normalize(p)).collect();
            self.start_search();
        }
    }

    /// Adds "Find duplicates with rdup" in Explorer, or takes it out, and
    /// says how it went.
    #[cfg(windows)]
    fn integration(&mut self, on: bool) {
        let text = match integrate(on) {
            Ok(t) | Err(t) => t,
        };
        self.explorer_menu = integrated_here();
        self.menu_dirty = true;
        self.info(&tr("Windows Integration"), &text);
    }

    // ------------------------------------------------------------ marks and actions

    fn toggle_mark(&mut self, row: usize) {
        match self.app.toggle_mark(row) {
            Ok(()) => self.app.note = None,
            Err(why) => self.app.note = Some(why),
        }
        self.menu_dirty = true;
        self.redraw();
    }

    fn apply_rule(&mut self, rule: Rule) {
        let n = self.app.apply_rule(&rule);
        self.app.note = Some(if n == 0 { tr("No copy was marked: the rule keeps them all.") } else { tf("{} marked", &[&format::count(n as u64)]) });
    }

    fn pick_folder(&mut self, title: &str) -> Option<PathBuf> {
        #[cfg(windows)]
        return self.win.as_ref().and_then(|w| rt_sys::win::dialogs::pick_folder(w.hwnd, title));
        #[cfg(not(windows))]
        rt_sys::linux::dialogs::pick_folder(title)
    }

    /// Asks before doing something that cannot be undone from here.
    fn confirm(&self, title: &str, question: &str) -> bool {
        #[cfg(windows)]
        {
            use rt_sys::win::dialogs::{message, Answer, Buttons, Icon};
            let hwnd = self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd);
            message(hwnd, title, question, Buttons::OkCancel, Icon::Warning) == Answer::Ok
        }
        #[cfg(not(windows))]
        rt_sys::linux::dialogs::confirm(title, question)
    }

    /// Moves the marked copies of the view to the Recycle Bin or a folder,
    /// after asking, in the background.
    fn act(&mut self, op: Op) {
        let marked = self.app.marked();
        if marked.is_empty() || self.acting.is_some() {
            return;
        }
        let n = format::count(marked.len() as u64);
        let what = if self.app.view == View::Folders { tf("{} folders", &[&n]) } else { tf("{} files", &[&n]) };
        let size = format::size(marked.iter().map(|(_, b)| b).sum());
        let (title, question) = match &op {
            Op::Recycle => (
                view::recycle_label(),
                if cfg!(windows) {
                    tf("Move {} to the Recycle Bin?\n\n{} in all. One copy of each group stays where it is.", &[&what, &size])
                } else {
                    tf("Move {} to the trash?\n\n{} in all. One copy of each group stays where it is.", &[&what, &size])
                },
            ),
            Op::MoveTo(dir) => (
                rt_core::i18n::strip_mnemonic(&tr("Mo&ve To...")),
                tf("Move {} to {}?\n\n{} in all. One copy of each group stays where it is.", &[&what, &dir.display().to_string(), &size]),
            ),
        };
        if !self.confirm(&title, &question) {
            return;
        }
        let paths: Vec<PathBuf> = marked.into_iter().map(|(p, _)| p).collect();
        self.acting = Some((0, paths.len()));
        self.dragging = false;
        let proxy = self.proxy.clone();
        let lang = i18n::lang();
        std::thread::spawn(move || {
            i18n::set_lang(lang);
            let total = paths.len();
            let (done, failed) = rdup::actions::run(&op, &paths, &|i, _| {
                let _ = proxy.send_event(UserEvent::Acting(i, total));
            });
            let _ = proxy.send_event(UserEvent::Acted(done, failed));
        });
        self.menu_dirty = true;
        self.redraw();
    }

    fn acted(&mut self, done: Vec<PathBuf>, failed: Vec<(PathBuf, String)>) {
        self.acting = None;
        self.app.removed(&done);
        let moved = format::count(done.len() as u64);
        let mut note = tf("{} moved.", &[&moved]);
        if let Some((p, why)) = failed.first() {
            note = format!("{note} {}", tf("{} could not be moved, such as {}: {}", &[&format::count(failed.len() as u64), &p.display().to_string(), why]));
        }
        self.app.note = Some(note);
        self.menu_dirty = true;
        self.redraw();
    }

    /// Writes the list shown to a CSV file the user chooses.
    fn export(&mut self) {
        let name = if self.app.view == View::Folders { "rdup-folders.csv" } else { "rdup-files.csv" };
        #[cfg(windows)]
        let chosen = self.win.as_ref().and_then(|w| {
            let filter = rt_sys::win::dialogs::filter(&[(tr("CSV files"), "*.csv")]);
            rt_sys::win::dialogs::save_file(w.hwnd, name, None, &filter)
        });
        #[cfg(not(windows))]
        let chosen = rt_sys::linux::dialogs::save_file(name, None);
        let Some(path) = chosen else { return };
        self.app.note = Some(match std::fs::write(&path, self.app.csv()) {
            Ok(()) => tf("The list was saved to {}.", &[&path.display().to_string()]),
            Err(e) => tf("Could not save {}: {}", &[&path.display().to_string(), &e.to_string()]),
        });
        self.redraw();
    }

    // ------------------------------------------------------------ menus

    // ------------------------------------------------------------ input

    // ------------------------------------------------------------ frame

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let (w, h, maximized) = self.settings.window.unwrap_or((1100.0, 780.0, false));
        let attrs = Window::default_attributes()
            .with_title("rdup")
            .with_inner_size(LogicalSize::new(w.max(480.0), h.max(360.0)))
            .with_min_inner_size(LogicalSize::new(480.0, 360.0))
            // Maximizing shows the window on Windows: a resident start, which
            // stays hidden, maximizes when it is first shown.
            .with_maximized(maximized && !self.tray)
            .with_window_icon(rt_ui::window::icon(rdup::ui::appicon::render))
            .with_theme(Some(rt_ui::window::theme(self.dark())))
            .with_visible(false);
        let Some((window, surface)) = rt_ui::window::create(el, attrs, "rdup") else { return false };
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
                rt_sys::win::tray::show(hwnd, "rdup");
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

/// Opens a file with its default program (an explicit command).
fn open_item(p: &Path) {
    #[cfg(windows)]
    rt_sys::win::shell_open(&p.to_string_lossy());
    #[cfg(not(windows))]
    rt_sys::linux::xdg_open(p.as_os_str().to_os_string());
}

fn shortcuts_text() -> String {
    let rows = [
        ("Ctrl+O", tr("Add a folder to the search")),
        ("F5", tr("Find the duplicates")),
        ("Ctrl+N", tr("Change the drives and folders")),
        ("Esc", tr("Stop the search")),
        ("Tab", tr("Go to the next filter box")),
        ("Ctrl+1  Ctrl+2", tr("Show the files or the folders")),
        ("Space", tr("Mark or unmark the selected copy")),
        ("Delete", if cfg!(windows) { tr("Move the marked copies to the Recycle Bin") } else { tr("Move the marked copies to the trash") }),
        ("Ctrl+M", tr("Move the marked copies to a folder")),
        ("Ctrl+S", tr("Save the list as CSV")),
        ("↑ ↓  PgUp PgDn  Home End", tr("Move in the list")),
        ("Ctrl+Enter", tr("Open the selected file")),
        ("Ctrl+E", tr("Show it in the file manager")),
        ("Ctrl+C", tr("Copy its path")),
    ];
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
}

/// Starts resident mode on Linux: without an icon, a hidden window could not
/// be reached or closed, so rdup then runs as a normal window.
#[cfg(not(windows))]
fn start_resident(shell: &mut Shell) {
    let proxy = std::sync::Mutex::new(shell.proxy.clone());
    let send = Box::new(move |m| {
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(UserEvent::Shell(m));
        }
    });
    shell.resident = rt_ui::resident::start("rdup", rdup::ui::appicon::render, tr("Open rdup"), tr("Exit"), send);
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
        // A resident start stays in the notification area unless a search
        // was asked for; Linux has no hidden window, so it is made when first shown.
        let start_hidden = self.tray && self.app.searching.is_none();
        if !(cfg!(not(windows)) && start_hidden) && !self.open_window(el) {
            return;
        }
        self.sync_menu();
        if start_hidden {
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
            UserEvent::Search(gen, ev) => self.on_search(gen, ev),
            UserEvent::Acting(done, total) => {
                self.acting = Some((done, total));
                self.app.note = Some(tf("Moving {} of {}…", &[&format::count(done as u64 + 1), &format::count(total as u64)]));
                self.redraw();
            }
            UserEvent::Acted(done, failed) => self.acted(done, failed),
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
                    match rt_sys::win::tray::menu(hwnd, &tr("Open rdup"), &tr("Exit")) {
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
            WindowEvent::DroppedFile(p) if self.app.searching.is_none() => {
                let dir = if p.is_dir() { p } else { p.parent().map(Path::to_path_buf).unwrap_or(p) };
                self.add_root(&dir);
                self.redraw();
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
                if self.dragging {
                    self.drag_to(y);
                }
                if self.selecting {
                    if let Some(f) = self.app.focus {
                        let s = self.scale();
                        let caret = view::caret_at(&mut self.gfx, &self.app, &self.layout, f, x, s);
                        self.app.drag_caret(caret);
                        self.redraw();
                    }
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
                    self.app.select(i);
                    self.menu_dirty = true;
                    self.redraw();
                    if self.app.selected_path().is_some() {
                        self.context_menu(el);
                    }
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
                    MouseScrollDelta::PixelDelta(p) => (-p.y as f32 / self.layout.row_h.max(20.0)).round() as isize,
                };
                let l = &self.layout;
                if let Some(c) = &mut self.app.calendar {
                    // Over the calendar the wheel turns the months; elsewhere it closes it.
                    if l.cal.as_ref().is_some_and(|cl| cl.panel.contains(x, y)) {
                        c.shift(rows.signum() as i32);
                    } else {
                        self.app.calendar = None;
                    }
                    self.redraw();
                    return;
                }
                match view::page(&self.app) {
                    Page::Result => self.app.scroll_by(rows),
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
        // The elapsed time on the search page moves even between reports.
        if self.app.searching.is_some() {
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(500)));
            self.redraw();
        } else {
            el.set_control_flow(ControlFlow::Wait);
        }
    }
}

/// Whether "Find duplicates with rdup" is on the Explorer menu of folders, for this rdup.exe.
#[cfg(windows)]
fn integrated_here() -> bool {
    rt_ui::explorer::folders_here("rdup")
}

/// Adds "Find duplicates with rdup" to the Explorer menu of folders and
/// drives (current user only), or takes it out; returns the message to show.
#[cfg(windows)]
fn integrate(on: bool) -> Result<String, String> {
    let texts = rt_ui::explorer::FolderTexts {
        verb: tr("Find duplicates with rdup"),
        added: tr("“Find duplicates with rdup” was added to the Explorer context menu of folders and drives."),
        removed: tr("“Find duplicates with rdup” was removed from the Explorer context menu."),
        failed: tr("Cannot change the Windows registry (error {})."),
    };
    rt_ui::explorer::set_folders("rdup", on, &texts)
}

const USAGE: &str = "rdup [--tray] [--new-window] [folder...]

  Finds files and folders with the same contents on drives and in folders:
  files of the same size are compared by the first and last bytes, then by
  a hash of their whole contents (BLAKE3).

  folder        Search these folders (or drives, or network shares) at
                once. Without them, rdup shows the drives to choose from.
  --tray        Stay resident in the notification area: closing the window
                hides it, and launching rdup again (with or without
                folders) or clicking the tray icon shows it instantly. Exit
                from the tray icon's menu or File > Exit.
  --new-window  Open a separate window even when rdup is resident.

Windows only:
  --register    Add \"Find duplicates with rdup\" to the Explorer context
                menu of folders and drives (current user only).
  --unregister  Remove it.

Settings are stored in the user's settings folder (%APPDATA%\\rdup on
Windows, ~/.config/rdup on Linux), or next to the program when a file named
rdup.portable exists there (portable mode).";

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // The help goes to the console it was asked from; without one, rdup
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
    // A resident rdup shows itself (and searches the folders) instantly.
    #[cfg(windows)]
    if !new_window && rt_sys::win::hand_off(&IDENT, &args, false, false) {
        return;
    }
    #[cfg(not(windows))]
    if !new_window && rt_sys::linux::shell::hand_off("rdup", &args, false) {
        return;
    }
    let mut location = storage::detect();
    let store = location.load();
    let settings = Settings::load(&store);
    let gfx = match Gfx::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rdup: {e}");
            std::process::exit(1);
        }
    };
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("rdup: cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    let mut app = App::new();
    app.choice = settings.choice.clone();
    app.share = settings.share;
    app.compare = settings.compare;
    app.chosen = if args.is_empty() { settings.roots.clone() } else { args.iter().map(|a| rt_scan::normalize(a)).collect() };
    app.saved = settings.folders.clone();
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
        acting: None,
        handle: None,
        generation: 0,
        dialog: help.then(|| Message::new("rdup", USAGE)),
        #[cfg(windows)]
        elevated: rt_sys::win::elevated(),
        #[cfg(not(windows))]
        menu: rdup::ui::menu::Menu::default(),
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
    if !args.is_empty() {
        shell.start_search();
    }
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rdup: {e}");
    }
}
