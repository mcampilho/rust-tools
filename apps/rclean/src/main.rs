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
        let work: Vec<(usize, rclean::targets::Target)> = self.app.items.iter().enumerate().filter(|(_, it)| it.usable()).map(|(i, it)| (i, it.target.clone())).collect();
        for &(i, _) in &work {
            self.app.items[i].state = State::Analysing;
        }
        self.start_busy(false, work.len());
        let (proxy, stop) = (self.proxy.clone(), self.stop.clone());
        std::thread::spawn(move || {
            let running = rclean::procs::running();
            for (i, t) in work {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let _ = proxy.send_event(UserEvent::Starting(gen, i));
                let found = clean::analyse(&t, &stop);
                let open = rclean::procs::any_running(&running, t.process);
                let _ = proxy.send_event(UserEvent::Analysed(gen, i, Box::new(found), open));
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
            .filter_map(|i| self.app.items[i].found().filter(|f| f.count() > 0).map(|f| (i, self.app.items[i].target.clone(), f.clone())))
            .collect();
        if work.is_empty() {
            return;
        }
        let (n, bytes) = work.iter().fold((0, 0), |(n, b), (_, _, f)| (n + f.count(), b + f.bytes));
        let mut text = tf("{} in {} files will be deleted for good:", &[&format::size(bytes), &format::count(n)]);
        text += "\n\n";
        text += &work
            .iter()
            .map(|(i, t, f)| format!("  {} — {}: {}", view::group_label(t.group), tr(t.name), format::size(f.bytes)) + if self.app.items[*i].open { " *" } else { "" })
            .collect::<Vec<_>>()
            .join("\n");
        if work.iter().any(|(i, _, _)| self.app.items[*i].open) {
            text += "\n\n";
            text += &tr("* Open now: close it first to clean everything; files in use are left as they are.");
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
        let (proxy, stop) = (self.proxy.clone(), self.stop.clone());
        std::thread::spawn(move || {
            for (i, t, f) in work {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let _ = proxy.send_event(UserEvent::Starting(gen, i));
                let c = clean::clean(&t, &f, &stop);
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
            Command::About => {
                let text = tf(
                    "rclean {}\n\nCleans the temporary files of the system and the caches of the browsers, written in Rust.\nNo telemetry, no network access.\n\nMIT License.",
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

    fn quit(&mut self, el: &ActiveEventLoop) {
        self.stop.store(true, Ordering::Relaxed);
        self.save_settings();
        el.exit();
    }

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let (w, h, maximized) = self.settings.window.unwrap_or((1060.0, 640.0, false));
        let attrs = Window::default_attributes()
            .with_title("rclean")
            .with_inner_size(LogicalSize::new(w.max(560.0), h.max(400.0)))
            .with_min_inner_size(LogicalSize::new(560.0, 400.0))
            .with_maximized(maximized)
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
        }
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

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() || !self.open_window(el) {
            return;
        }
        self.sync_menu();
        if let Some(w) = &self.win {
            w.window.set_visible(true);
            w.window.focus_window();
        }
        self.redraw();
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Starting(gen, i) if gen == self.generation => {
                let name = self.app.items.get(i).map(|it| format!("{} — {}", view::group_label(it.target.group), tr(it.target.name))).unwrap_or_default();
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
            #[cfg(windows)]
            UserEvent::Win(_) => {}
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
            WindowEvent::CloseRequested => self.quit(el),
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
            WindowEvent::RedrawRequested => self.render(),
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

const USAGE: &str = "rclean

  Cleans the temporary files of the system and the caches of the browsers:
  it shows how much each holds, and deletes for good what you tick, after
  asking. With administrator rights, also Windows' own folders.

Settings are stored in the user's settings folder (%APPDATA%\\rclean on
Windows, ~/.config/rclean on Linux), or next to the program when a file
named rclean.portable exists there (portable mode).";

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
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
    let admin = std::fs::read_to_string("/proc/self/status").is_ok_and(|s| s.lines().any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(2) == Some("0")));
    let app = App::new(rclean::targets::all(&rclean::targets::Env::here()), admin, &settings.ticks);
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
        #[cfg(not(windows))]
        menu: rclean::ui::menu::Menu::default(),
    };
    shell.apply_language();
    // A first look at once: how much each holds.
    shell.analyse();
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rclean: {e}");
    }
}
