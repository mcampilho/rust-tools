//! rclean: a portable cleaner.
//!
//! The window is driven by winit and drawn on the CPU (tiny-skia + ab_glyph)
//! into a softbuffer surface. Analysing and cleaning run on a thread of
//! their own, one target after another; each step is sent to the window.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rclean::app::{App, Busy, State};
use rclean::clean::{self, Cleaned, Found};
use rclean::commands::{self, Command};
use rclean::format;
use rclean::i18n::{self, tf, tr};
use rclean::settings::Settings;
use rclean::storage;
use rclean::ui::gfx::{Color, Gfx, Rect};
use rclean::ui::theme::Theme;
use rclean::ui::view::{self, Hot, Layout};
use rt_ui::Message;

/// How many targets are looked at at the same time.
const ANALYSE_THREADS: usize = 4;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Theme as WinTheme, Window, WindowId};

mod input;
mod menus;
mod render;

enum UserEvent {
    /// A step of Check for Updates.
    Update(rt_ui::update::Step),
    A11y(accesskit_winit::Event),
    /// The work of this generation starts on an item.
    Starting(u64, usize),
    /// An item looked at, and whether its program is open.
    Analysed(u64, usize, Box<Found>, bool),
    Cleaned(u64, usize, Cleaned),
    /// The work of this generation is done (true: it was cleaning).
    Finished(u64, bool),
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
const IDENT: rt_sys::win::Ident = rt_sys::win::Ident { name: "rclean", copydata: 0x524C_4E43 }; // "RLNC"

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
    cursor_icon: CursorIcon,
    generation: u64,
    /// Running with administrator rights: Windows' own folders can be cleaned.
    admin: bool,
    dialog: Option<Message>,
    /// Asks the work under way to stop after the item now.
    stop: Arc<AtomicBool>,
    /// When the work started, for what turns while it runs.
    started: Option<std::time::Instant>,
    /// Where this cleaning saved registry entries.
    backups: Option<std::path::PathBuf>,
    /// Resident mode (`--tray`): closing the window hides it to the
    /// notification area; the next launch or the icon shows it.
    tray: bool,
    hidden: bool,
    maximize_on_show: bool,
    /// Starts with the session (as `rclean --tray`).
    autostart: bool,
    /// "Clean with rclean" is on the Explorer menu of folders, for this rclean.exe.
    #[cfg(windows)]
    explorer_menu: bool,
    #[cfg(not(windows))]
    resident: Option<rt_sys::linux::shell::Shell>,
    #[cfg(not(windows))]
    menu: rclean::ui::menu::Menu,
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
        self.settings.ticks = self.app.ticks();
        let store = self.settings.to_store();
        self.location.save(&store);
    }

    // ------------------------------------------------------------ work

    /// Looks at what every target holds, on a thread.
    fn analyse(&mut self) {
        if self.app.busy.is_some() {
            return;
        }
        self.generation += 1;
        let gen = self.generation;
        self.stop = Arc::new(AtomicBool::new(false));
        let work: Vec<(usize, rclean::targets::Target)> =
            self.app.items.iter().enumerate().filter(|(_, it)| it.usable()).map(|(i, it)| (i, it.target.clone())).collect();
        for &(i, _) in &work {
            self.app.items[i].state = State::Analysing;
        }
        self.start_busy(false, work.len());
        let (proxy, stop) = (self.proxy.clone(), self.stop.clone());
        std::thread::spawn(move || {
            let running = Arc::new(rclean::procs::running());
            // The targets are independent (their own folders, or their own program): a few at a
            // time, because most of the time goes to waiting for the disk and for commands.
            let workers = work.len().clamp(1, ANALYSE_THREADS);
            let queue = Arc::new(std::sync::Mutex::new(work.into_iter()));
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    let (queue, running, proxy, stop) = (queue.clone(), running.clone(), proxy.clone(), stop.clone());
                    std::thread::spawn(move || loop {
                        let next = queue.lock().ok().and_then(|mut q| q.next());
                        let Some((i, t)) = next else { break };
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        let _ = proxy.send_event(UserEvent::Starting(gen, i));
                        let found = clean::analyse(&t, &stop);
                        let open = rclean::procs::any_running(&running, t.process);
                        let _ = proxy.send_event(UserEvent::Analysed(gen, i, Box::new(found), open));
                    })
                })
                .collect();
            for h in handles {
                let _ = h.join();
            }
            let _ = proxy.send_event(UserEvent::Finished(gen, false));
        });
    }

    /// Asks, then deletes what the ticked targets hold, on a thread.
    fn clean(&mut self) {
        if self.app.busy.is_some() {
            return;
        }
        let work: Vec<(usize, rclean::targets::Target, Found)> = self
            .app
            .chosen()
            .into_iter()
            .filter_map(|i| {
                self.app.items[i].found().filter(|f| f.count() > 0 || f.unknown).map(|f| (i, self.app.items[i].target.clone(), f.clone()))
            })
            .collect();
        if work.is_empty() {
            return;
        }
        let (n, bytes) = work.iter().fold((0, 0), |(n, b), (_, _, f)| (n + f.count(), b + f.bytes));
        let shortcuts = work.iter().any(|(_, t, _)| t.kind == rclean::targets::Kind::Shortcuts);
        let mut text = if shortcuts && work.iter().all(|(_, t, _)| t.kind == rclean::targets::Kind::Shortcuts) {
            tf("{} broken shortcuts will go to the Recycle Bin:", &[&format::count(n)])
        } else if shortcuts {
            tf(
                "{} in {} files will be cleaned (broken shortcuts to the Recycle Bin, the rest for good):",
                &[&format::size(bytes), &format::count(n)],
            )
        } else {
            tf("{} in {} files will be deleted for good:", &[&format::size(bytes), &format::count(n)])
        };
        text += "\n\n";
        text += &work
            .iter()
            .map(|(i, t, f)| {
                let size = if f.unknown { "?".to_string() } else { format::size(f.bytes) };
                format!("  {} — {}: {size}", view::group_label(t.group), tr(t.name)) + if self.app.items[*i].open { " *" } else { "" }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if work.iter().any(|(i, _, _)| self.app.items[*i].open) {
            text += "\n\n";
            text += &tr("* Open now: close it first to clean everything; files in use are left as they are.");
        }
        if work.iter().any(|(_, t, _)| t.kind == rclean::targets::Kind::Registry) {
            text += "\n\n";
            text += &tr("Registry entries are saved to a .reg file first, to put them back if need be.");
        }
        if !self.confirm(&tr("Clean"), &text) {
            return;
        }
        self.generation += 1;
        let gen = self.generation;
        self.stop = Arc::new(AtomicBool::new(false));
        self.app.freed = None;
        for (i, _, f) in &work {
            self.app.items[*i].state = State::Cleaning(f.clone());
        }
        self.start_busy(true, work.len());
        // Registry entries are saved first: `backups\registry <date>\<id>.reg`
        // beside the settings.
        let stamp = {
            let t = rt_sys::local_time();
            format!("{:04}-{:02}-{:02} {:02}{:02}{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
        };
        let backups = self
            .location
            .dir
            .clone()
            .or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())))
            .unwrap_or_else(std::env::temp_dir)
            .join("backups")
            .join(format!("registry {stamp}"));
        self.backups = work.iter().any(|(_, t, _)| t.kind == rclean::targets::Kind::Registry).then(|| backups.clone());
        let (proxy, stop) = (self.proxy.clone(), self.stop.clone());
        std::thread::spawn(move || {
            for (i, t, f) in work {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let _ = proxy.send_event(UserEvent::Starting(gen, i));
                let c = clean::clean(&t, &f, &stop, &backups);
                let _ = proxy.send_event(UserEvent::Cleaned(gen, i, c));
            }
            let _ = proxy.send_event(UserEvent::Finished(gen, true));
        });
    }

    fn start_busy(&mut self, cleaning: bool, total: usize) {
        self.app.busy = Some(Busy { cleaning, total, ..Busy::default() });
        self.started = Some(std::time::Instant::now());
        self.menu_dirty = true;
        self.redraw();
    }

    fn stop_work(&mut self) {
        if let Some(b) = &mut self.app.busy {
            b.stopping = true;
            self.stop.store(true, Ordering::Relaxed);
            self.redraw();
        }
    }

    /// The work is done: what cleaning freed, then a new look.
    fn finished(&mut self, cleaning: bool) {
        self.app.busy = None;
        self.started = None;
        // What was not reached goes back to how it was.
        for it in &mut self.app.items {
            match &it.state {
                State::Analysing => it.state = State::Waiting,
                State::Cleaning(f) => it.state = State::Found(f.clone()),
                _ => {}
            }
        }
        if cleaning {
            let c = self.app.freed.clone().unwrap_or_default();
            let mut text = tf("{} freed: {} files deleted.", &[&format::size(c.bytes), &format::count(c.files)]);
            if c.failed > 0 {
                text += "\n\n";
                text += &tf("{} files were in use or could not be deleted, and stayed.", &[&format::count(c.failed)]);
            }
            if let Some(dir) = self.backups.take().filter(|d| d.is_dir()) {
                text += "\n\n";
                text += &tf(
                    "The registry entries were saved in {} first: open the .reg files there to put them back.",
                    &[&dir.display().to_string()],
                );
            }
            // What a package manager's command said when it did not work.
            let errors: Vec<String> = self
                .app
                .items
                .iter()
                .filter_map(|it| match &it.state {
                    State::Cleaned(Cleaned { error: Some(e), .. }) => {
                        Some(format!("  {} — {}: {e}", view::group_label(it.target.group), tr(it.target.name)))
                    }
                    _ => None,
                })
                .collect();
            if !errors.is_empty() {
                text += "\n\n";
                text += &tf("Did not work ({}):", &[&errors.len().to_string()]);
                text += "\n";
                text += &errors.join("\n");
            }
            self.info(&tr("Clean"), &text);
            self.analyse();
        }
        self.menu_dirty = true;
        self.update_pointer();
        self.redraw();
    }

    fn update_title(&self) {
        let Some(w) = &self.win else { return };
        let title = if self.admin { format!("rclean ({})", tr("Administrator")) } else { "rclean".to_string() };
        if w.window.title() != title {
            w.window.set_title(&title);
        }
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self, el: &ActiveEventLoop, cmd: Command) {
        if !self.enabled(cmd) {
            return;
        }
        match cmd {
            Command::Analyse => self.analyse(),
            Command::Clean => self.clean(),
            Command::Stop => self.stop_work(),
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
            Command::Autostart => match rt_sys::autostart::set("rclean", !self.autostart) {
                Ok(()) => self.autostart = rt_sys::autostart::enabled("rclean"),
                Err(e) => self.info("rclean", &tf("Cannot change how it starts: {}", &[&e])),
            },
            Command::TickAll => self.app.tick_all(true),
            Command::UntickAll => self.app.tick_all(false),
            Command::TickDefaults => self.app.tick_defaults(),
            Command::OpenFolder => {
                if let Some(dir) = self.folder() {
                    open_path(&dir);
                }
            }
            Command::CopyDetails => {
                rt_sys::set_clipboard_text(&view::details_text(&self.app));
                self.app.note = Some(tr("Copied to the clipboard."));
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
            Command::CheckUpdates => rt_ui::update::check("rclean", env!("CARGO_PKG_VERSION"), self.update_sender()),
            Command::About => {
                let text = tf(
                    "rclean {}\n\nCleans the temporary files of the system and the caches of the browsers and package managers, written in Rust.\nNo telemetry, no network access.\n\nMIT License.",
                    &[env!("CARGO_PKG_VERSION")],
                );
                self.info(&tr("About rclean"), &text);
            }
        }
        self.menu_dirty = true;
        self.redraw();
    }

    /// The first folder of the target selected that is there.
    fn folder(&self) -> Option<std::path::PathBuf> {
        self.app.selected_item()?.target.places.iter().map(|p| p.dir.clone()).find(|d| d.is_dir())
    }

    fn checked(&self, c: Command) -> bool {
        match c {
            Command::Theme(t) => self.settings.theme == t,
            Command::UiLanguage(0) => self.settings.language.is_none(),
            Command::UiLanguage(i) => self.settings.language == i18n::Lang::ALL.get(i as usize - 1).copied(),
            Command::Autostart => self.autostart,
            #[cfg(windows)]
            Command::ExplorerMenu => self.explorer_menu,
            _ => false,
        }
    }

    fn enabled(&self, c: Command) -> bool {
        let idle = self.app.busy.is_none();
        match c {
            Command::Analyse => idle,
            Command::Clean => idle && self.app.anything_to_clean(),
            Command::Stop => self.app.busy.as_ref().is_some_and(|b| !b.stopping),
            #[cfg(windows)]
            Command::RestartAdmin => !self.admin && idle,
            Command::TickAll | Command::UntickAll | Command::TickDefaults => idle,
            Command::OpenFolder => self.folder().is_some(),
            Command::CopyDetails => self.app.sel.is_some(),
            _ => true,
        }
    }

    fn info(&mut self, title: &str, text: &str) {
        self.dialog = Some(Message::new(title, text));
        self.dragging = false;
        self.redraw();
    }

    /// Asks before doing something; true for yes.
    fn confirm(&mut self, title: &str, text: &str) -> bool {
        #[cfg(windows)]
        {
            use rt_sys::win::dialogs::{message, Answer, Buttons, Icon};
            let hwnd = self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd);
            message(hwnd, title, text, Buttons::OkCancel, Icon::Warning) == Answer::Ok
        }
        #[cfg(not(windows))]
        {
            rt_sys::linux::dialogs::confirm(title, text)
        }
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
        self.stop.store(true, Ordering::Relaxed);
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

    /// Hides the window to the notification area; a cleaning goes on.
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
        // Release the frame buffer while hidden.
        self.gfx.resize(1, 1);
        self.gfx.trim_cache();
    }

    /// Shows the window (from the icon or another launch), and looks again
    /// at what there is to clean.
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
        self.autostart = rt_sys::autostart::enabled("rclean");
        self.menu_dirty = true;
        if was_hidden && self.app.busy.is_none() {
            self.analyse();
        }
        self.redraw();
    }

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let (w, h, maximized) = self.settings.window.unwrap_or((1060.0, 640.0, false));
        let attrs = Window::default_attributes()
            .with_title("rclean")
            .with_inner_size(LogicalSize::new(w.max(560.0), h.max(400.0)))
            .with_min_inner_size(LogicalSize::new(560.0, 400.0))
            // Maximizing shows the window on Windows: a resident start, which
            // stays hidden, maximizes when it is first shown.
            .with_maximized(maximized && !self.tray)
            .with_window_icon(rt_ui::window::icon(rclean::ui::appicon::render))
            .with_theme(Some(rt_ui::window::theme(self.dark())))
            .with_visible(false);
        let Some((window, surface)) = rt_ui::window::create(el, attrs, "rclean") else { return false };
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
                rt_sys::win::tray::show(hwnd, "rclean");
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

/// Opens a folder in the file manager.
fn open_path(p: &std::path::Path) {
    #[cfg(windows)]
    rt_sys::win::shell_open(&p.to_string_lossy());
    #[cfg(not(windows))]
    rt_sys::linux::xdg_open(p.as_os_str().to_os_string());
}

fn shortcuts_text() -> String {
    let rows = [
        ("F5".to_string(), tr("Analyze again")),
        ("Ctrl+L".to_string(), tr("Clean what is ticked")),
        ("Esc".to_string(), tr("Stop")),
        ("↑ ↓  PgUp PgDn  Home End".to_string(), tr("Move in the list")),
        (tr("Space"), tr("Tick or untick the row selected")),
        ("Ctrl+A".to_string(), tr("Tick everything")),
        ("Ctrl+E".to_string(), tr("Open the folder")),
        ("Ctrl+Shift+C".to_string(), tr("Copy the details")),
    ];
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
}

/// Starts resident mode on Linux: without an icon, a hidden window could not
/// be reached or closed, so rclean then runs as a normal window.
#[cfg(not(windows))]
fn start_resident(shell: &mut Shell) {
    let proxy = std::sync::Mutex::new(shell.proxy.clone());
    let send = Box::new(move |m| {
        if let Ok(p) = proxy.lock() {
            let _ = p.send_event(UserEvent::Shell(m));
        }
    });
    shell.resident = rt_ui::resident::start("rclean", rclean::ui::appicon::render, tr("Open rclean"), tr("Exit"), send);
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
        // A resident start stays in the notification area (and looks at
        // nothing until shown); Linux has no hidden window, so it is made
        // when first shown.
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
            UserEvent::Starting(gen, i) if gen == self.generation => {
                let name = self
                    .app
                    .items
                    .get(i)
                    .map(|it| format!("{} — {}", view::group_label(it.target.group), tr(it.target.name)))
                    .unwrap_or_default();
                if let Some(b) = &mut self.app.busy {
                    b.name = name;
                }
                self.redraw();
            }
            UserEvent::Analysed(gen, i, found, open) if gen == self.generation => {
                if let Some(it) = self.app.items.get_mut(i) {
                    it.open = open;
                }
                self.app.set_found(i, *found);
                self.menu_dirty = true;
                self.redraw();
            }
            UserEvent::Cleaned(gen, i, c) if gen == self.generation => {
                self.app.set_cleaned(i, c);
                self.redraw();
            }
            UserEvent::Finished(gen, cleaning) if gen == self.generation => self.finished(cleaning),
            UserEvent::Starting(..) | UserEvent::Analysed(..) | UserEvent::Cleaned(..) | UserEvent::Finished(..) => {}
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
                    match rt_sys::win::tray::menu(hwnd, &tr("Open rclean"), &tr("Exit")) {
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
                    app: "rclean",
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
                        self.app.toggle_row(i as usize);
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
                ElementState::Released => self.dragging = false,
            },
            #[cfg(not(windows))]
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } if self.menu.is_open() => {
                self.menu.close();
                self.redraw();
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
                    let max = (view::details_height(&mut self.gfx, &self.app, &self.layout, s) - self.layout.details.h).max(0.0);
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
        // While working, the arc turns, so it shows it is busy.
        if let (Some(t0), Some(b)) = (self.started, &mut self.app.busy) {
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

/// Whether "Clean with rclean" is on the Explorer menu of folders, for this rclean.exe.
#[cfg(windows)]
fn integrated_here() -> bool {
    rt_ui::explorer::folders_here("rclean")
}

/// Adds "Clean with rclean" to the Explorer menu of folders and drives
/// (current user only), or takes it out; returns the message to show.
#[cfg(windows)]
fn integrate(on: bool) -> Result<String, String> {
    let texts = rt_ui::explorer::FolderTexts {
        verb: tr("Clean with rclean"),
        added: tr("“Clean with rclean” was added to the Explorer context menu of folders and drives."),
        removed: tr("“Clean with rclean” was removed from the Explorer context menu."),
        failed: tr("Cannot change the Windows registry (error {})."),
    };
    rt_ui::explorer::set_folders("rclean", on, &texts)
}

const USAGE: &str = "rclean [--tray] [--new-window]

  Cleans the temporary files of the system and the caches of the browsers
  and package managers:
  it shows how much each holds, and deletes for good what you tick, after
  asking. With administrator rights, also Windows' own folders.

  --tray        Stay resident in the notification area: closing the window
                hides it, and the icon (or starting rclean again) shows it,
                looking again at what there is to clean. Exit from the
                icon's menu or File > Exit. File > Start with Windows starts
                it this way with the session.
  --new-window  Open a separate window even when rclean is resident.

Windows only:
  --register    Add \"Clean with rclean\" to the Explorer context menu of
                folders and drives (current user only).
  --unregister  Remove it.

Settings are stored in the user's settings folder (%APPDATA%\\rclean on
Windows, ~/.config/rclean on Linux), or next to the program when a file
named rclean.portable exists there (portable mode).";

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
    // A resident rclean shows itself instantly.
    #[cfg(windows)]
    if !new_window && rt_sys::win::hand_off(&IDENT, &[], false, false) {
        return;
    }
    #[cfg(not(windows))]
    if !new_window && rt_sys::linux::shell::hand_off("rclean", &[], false) {
        return;
    }
    let mut location = storage::detect();
    let store = location.load();
    let settings = Settings::load(&store);
    let gfx = match Gfx::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rclean: {e}");
            std::process::exit(1);
        }
    };
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("rclean: cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    #[cfg(windows)]
    let admin = rt_sys::win::elevated();
    #[cfg(not(windows))]
    let admin = std::fs::read_to_string("/proc/self/status")
        .is_ok_and(|s| s.lines().any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(2) == Some("0")));
    let mut app = App::new(rclean::targets::all(&rclean::targets::Env::here()), admin, &settings.ticks);
    // Not as administrator: what needs it may still have been measured by the privileged service.
    if !admin {
        if let Some((list, age)) = rt_clean::snapshot::fetch(6 * 3600) {
            app.set_known(list, age);
        }
    }
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
        cursor_icon: CursorIcon::Default,
        generation: 0,
        admin,
        dialog: help.then(|| Message::new("rclean", USAGE)),
        stop: Arc::new(AtomicBool::new(false)),
        started: None,
        backups: None,
        tray,
        hidden: false,
        maximize_on_show: false,
        autostart: rt_sys::autostart::enabled("rclean"),
        #[cfg(windows)]
        explorer_menu: integrated_here(),
        #[cfg(not(windows))]
        resident: None,
        #[cfg(not(windows))]
        menu: rclean::ui::menu::Menu::default(),
    };
    shell.apply_language();
    // A first look at once: how much each holds.
    // A first look at once, unless it starts hidden (it looks when shown).
    if !tray {
        shell.analyse();
    }
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rclean: {e}");
    }
}
