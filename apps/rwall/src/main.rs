//! rwall: changes the desktop wallpaper.
//!
//! Three ways to run it:
//! - `rwall`: the settings window; closing it ends rwall. The picture changes
//!   only when asked (Next picture).
//! - `rwall --tray`: resident, with an icon in the notification area and the
//!   settings window hidden when closed. A timer checks once every half
//!   minute whether the period of the chosen frequency changed.
//! - `rwall --auto`: one change with the saved settings, without a window
//!   (handed to the rwall already running, if there is one).
//!
//! A change (download, conversion, setting the wallpaper) runs on a background
//! thread and reports to the window through the event loop.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rwall::changer::{self, Done};
use rwall::commands::{tray_menu, TrayCmd, M};
use rwall::i18n::{self, tf, tr};
use rwall::schedule;
use rwall::settings::{Settings, KEEP_MAX, KEEP_MIN};
use rwall::sources::Source;
use rwall::storage;
use rwall::ui::gfx::{Color, Gfx, Rect};
use rwall::ui::theme::Theme;
use rwall::ui::view::{self, Drawn, Field, Hit, Model};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Icon as WinIcon, Theme as WinTheme, Window, WindowId};

enum UserEvent {
    /// A change of wallpaper ended.
    Changed(Result<Box<Done>, String>),
    /// The current picture was set again (a new position).
    Applied(Result<(), String>),
    Thumb(Option<tiny_skia::Pixmap>),
    #[cfg(windows)]
    Win(rt_sys::win::WinMsg),
    /// A later launch, or the notification-area icon (Linux).
    #[cfg(not(windows))]
    Shell(rt_sys::linux::shell::Msg),
    #[cfg(not(windows))]
    TrayItem(u32),
}

#[cfg(windows)]
const IDENT: rt_sys::win::Ident = rt_sys::win::Ident { name: "rwall", copydata: 0x5257_414C }; // "RWAL"

/// What `rwall --auto` hands to the running one (never a real file: '?'
/// cannot be in a Windows file name).
const NEXT_MARK: &str = "?next";

/// How often the time is checked.
const TICK: Duration = Duration::from_secs(30);
/// How long to wait after a failed change before trying again on its own.
const RETRY: Duration = Duration::from_secs(10 * 60);
/// Size of the preview, in pixels.
const THUMB: (u32, u32) = (480, 270);
/// Steps of "Downloaded pictures to keep".
const KEEP_STEPS: [u32; 10] = [1, 5, 10, 20, 30, 50, 100, 200, 300, 500];

struct Win {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    #[cfg(windows)]
    hwnd: rt_sys::win::HWND,
}

struct Shell {
    settings: Settings,
    location: storage::Location,
    gfx: Gfx,
    win: Option<Win>,
    proxy: EventLoopProxy<UserEvent>,
    visible: bool,
    system_dark: bool,
    accent: Option<Color>,
    thumb: Option<tiny_skia::Pixmap>,
    busy: bool,
    error: Option<String>,
    retry_at: Option<Instant>,
    next_tick: Instant,
    autostart: bool,
    focus: Option<Field>,
    hot: Option<Hit>,
    scroll: f32,
    drawn: Drawn,
    mods: ModifiersState,
    cursor_icon: CursorIcon,
    /// Resident in the notification area, changing on schedule (`--tray`).
    tray: bool,
    /// Show the settings window once started.
    show_on_start: bool,
    /// Size of the main screen, in pixels.
    screen: (u32, u32),
    /// The notification-area icon and the socket for later launches (Linux).
    #[cfg(not(windows))]
    resident: Option<rt_sys::linux::shell::Shell>,
    /// Without `--tray`: the socket for later launches only (Linux).
    #[cfg(not(windows))]
    listener: Option<rt_sys::linux::shell::Shell>,
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
            if self.visible {
                w.window.request_redraw();
            }
        }
    }

    fn save(&mut self) {
        let store = self.settings.to_store();
        if !self.location.save(&store) {
            self.error = self.location.error.clone();
        }
    }

    fn apply_language(&mut self) {
        i18n::set_lang(self.settings.language.unwrap_or_else(i18n::system_lang));
        self.gfx.clear_cache();
    }

    // ------------------------------------------------------------ changes

    /// The tooltip of the tray icon.
    fn tip(&self) -> String {
        match &self.settings.current {
            Some(c) if !c.title.is_empty() => format!("rwall – {}", c.title),
            _ => "rwall".to_string(),
        }
    }

    fn update_tip(&self) {
        #[cfg(windows)]
        if let Some(w) = self.win.as_ref().filter(|_| self.tray) {
            rt_sys::win::tray::show(w.hwnd, &self.tip());
        }
        #[cfg(not(windows))]
        if let Some(r) = &self.resident {
            r.set_labels(tray_labels(&self.tip()));
        }
    }

    /// Changes the wallpaper when a new period started (resident only).
    fn tick(&mut self) {
        if !self.tray || self.busy || self.settings.sources.is_empty() || self.retry_at.is_some_and(|r| Instant::now() < r) {
            return;
        }
        let now = rt_sys::local_time();
        if schedule::period(self.settings.frequency, &now, self.settings.custom_minutes) != self.settings.last_period {
            self.change();
        }
    }

    fn change(&mut self) {
        if self.busy {
            return;
        }
        let Some(images) = storage::images_dir(&self.location) else {
            self.error = Some(tr("No user data folder is available."));
            self.redraw();
            return;
        };
        let Some(job) = changer::job(&self.settings, images, self.screen, THUMB) else {
            self.error = Some(tr("Turn on at least one source of pictures."));
            self.redraw();
            return;
        };
        self.busy = true;
        self.error = None;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            // A malformed picture must not leave rwall waiting for ever.
            let result = std::panic::catch_unwind(|| changer::run(&job)).unwrap_or_else(|_| Err("the picture could not be read".into()));
            let _ = proxy.send_event(UserEvent::Changed(result.map(Box::new)));
        });
        self.redraw();
    }

    fn changed(&mut self, result: Result<Box<Done>, String>) {
        self.busy = false;
        let now = rt_sys::local_time();
        match result {
            Ok(done) => {
                let Done { pick, file, thumb } = *done;
                changer::record(&mut self.settings, pick, file, &now);
                self.thumb = thumb;
                self.error = None;
                self.retry_at = None;
                self.save();
                self.update_tip();
            }
            Err(e) => {
                self.error = Some(tf("The picture could not be changed: {}", &[&e]));
                self.retry_at = Some(Instant::now() + RETRY);
            }
        }
        self.redraw();
    }

    /// Sets the current picture again, after the position changed.
    fn reapply(&mut self) {
        let Some(file) = self.settings.current.as_ref().map(|c| c.file.clone()) else { return };
        let (fit, proxy) = (self.settings.fit, self.proxy.clone());
        std::thread::spawn(move || {
            let _ = proxy.send_event(UserEvent::Applied(rwall::wallpaper::set(&file, fit)));
        });
    }

    fn load_thumb(&self) {
        let Some(file) = self.settings.current.as_ref().map(|c| c.file.clone()) else { return };
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let thumb = std::panic::catch_unwind(|| rwall::picture::thumbnail(&file, THUMB.0, THUMB.1)).ok().flatten();
            let _ = proxy.send_event(UserEvent::Thumb(thumb));
        });
    }

    fn status(&self) -> String {
        if self.busy {
            return tr("Looking for a picture…");
        }
        if self.settings.sources.is_empty() {
            return tr("Turn on at least one source of pictures.");
        }
        if !self.tray {
            return tr("The picture changes on its own only while rwall runs in the notification area (rwall --tray).");
        }
        let next = schedule::next_change(self.settings.frequency, &rt_sys::local_time(), self.settings.custom_minutes);
        tf("Next change: {}", &[&schedule::format(&next)])
    }

    // ------------------------------------------------------------ commands

    fn about(&self) {
        if let Some(c) = &self.settings.current {
            if !c.link.is_empty() {
                rt_sys::open_url(&c.link);
            }
        }
    }

    /// Shows the picture on the desktop in the file manager, or the folder
    /// of downloads when there is none.
    fn show_file(&self) {
        if let Some(c) = self.settings.current.as_ref().filter(|c| c.file.exists()) {
            rt_sys::reveal(&c.file);
            return;
        }
        if let Some(dir) = storage::images_dir(&self.location) {
            let _ = std::fs::create_dir_all(&dir);
            #[cfg(windows)]
            rt_sys::win::shell_open(&dir.to_string_lossy());
            #[cfg(not(windows))]
            rt_sys::linux::xdg_open(dir.into_os_string());
        }
    }

    fn tray_command(&mut self, el: &ActiveEventLoop, c: TrayCmd) {
        match c {
            TrayCmd::Next => self.change(),
            TrayCmd::About => self.about(),
            TrayCmd::Settings => self.show(),
            TrayCmd::ShowFile => self.show_file(),
            TrayCmd::Exit => self.quit(el),
        }
    }

    fn pick_folder(&self) -> Option<PathBuf> {
        let title = tr("Choose a folder of pictures");
        #[cfg(windows)]
        return rt_sys::win::dialogs::pick_folder(self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd), &title);
        #[cfg(not(windows))]
        return rt_sys::linux::dialogs::pick_folder(&title);
    }

    fn click(&mut self, h: Hit) {
        let st = &mut self.settings;
        let now = rt_sys::local_time();
        match h {
            Hit::Next => self.change(),
            Hit::About => self.about(),
            Hit::OpenFolder => self.show_file(),
            Hit::Frequency(f) => {
                st.frequency = f;
                // The new frequency starts counting now: no change at once.
                st.last_period = schedule::period(f, &now, st.custom_minutes);
                self.save();
            }
            Hit::CustomLess | Hit::CustomMore => {
                st.custom_minutes = schedule::custom_step(st.custom_minutes, h == Hit::CustomMore);
                st.frequency = schedule::Frequency::Custom;
                st.last_period = schedule::period(st.frequency, &now, st.custom_minutes);
                self.save();
            }
            Hit::Source(s) => {
                st.toggle_source(s);
                if s == Source::Folders && st.source_on(s) && st.folders.is_empty() {
                    self.add_folder();
                }
                self.save();
            }
            Hit::Field(f) => self.focus = Some(f),
            Hit::AddFolder => {
                self.add_folder();
                self.save();
            }
            Hit::RemoveFolder(i) => {
                if i < st.folders.len() {
                    st.folders.remove(i);
                }
                self.save();
            }
            Hit::Subfolders => {
                st.subfolders = !st.subfolders;
                self.save();
            }
            Hit::Fit(f) => {
                if st.fit != f {
                    st.fit = f;
                    self.save();
                    self.reapply();
                }
            }
            Hit::KeepLess | Hit::KeepMore => {
                let k = st.keep;
                st.keep = if h == Hit::KeepMore {
                    KEEP_STEPS.into_iter().find(|s| *s > k).unwrap_or(KEEP_MAX)
                } else {
                    KEEP_STEPS.into_iter().rev().find(|s| *s < k).unwrap_or(KEEP_MIN)
                };
                self.save();
            }
            Hit::Autostart => match rwall::autostart::set(!self.autostart) {
                Ok(()) => self.autostart = rwall::autostart::enabled(),
                Err(e) => self.error = Some(tf("Cannot change how rwall starts: {}", &[&e])),
            },
        }
        self.redraw();
    }

    fn add_folder(&mut self) {
        if let Some(dir) = self.pick_folder() {
            let st = &mut self.settings;
            if !st.folders.contains(&dir) {
                st.folders.push(dir);
            }
            if !st.source_on(Source::Folders) {
                st.toggle_source(Source::Folders);
            }
        }
    }

    /// Ends typing in a text box, keeping what was typed.
    fn unfocus(&mut self) {
        if self.focus.take().is_some() {
            let st = &mut self.settings;
            st.bing_market = st.bing_market.trim().to_string();
            self.save();
            self.redraw();
        }
    }

    fn field_text(&mut self, f: Field) -> &mut String {
        match f {
            Field::Market => &mut self.settings.bing_market,
            Field::Wallhaven => &mut self.settings.wallhaven_query,
        }
    }

    fn key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) {
        if let Some(f) = self.focus {
            match &event.logical_key {
                WKey::Named(NamedKey::Enter | NamedKey::Escape | NamedKey::Tab) => self.unfocus(),
                WKey::Named(NamedKey::Backspace) => {
                    self.field_text(f).pop();
                }
                WKey::Character(c) if self.mods.control_key() && c.eq_ignore_ascii_case("v") => {
                    if let Some(t) = rt_sys::clipboard_text() {
                        let line = t.lines().next().unwrap_or("").to_string();
                        self.field_text(f).push_str(&line);
                    }
                }
                _ => {
                    if let Some(t) = event.text.as_ref().filter(|_| !self.mods.control_key() && !self.mods.alt_key()) {
                        let typed: String = t.chars().filter(|c| !c.is_control()).collect();
                        let s = self.field_text(f);
                        if s.chars().count() + typed.chars().count() <= 200 {
                            s.push_str(&typed);
                        }
                    }
                }
            }
            self.redraw();
            return;
        }
        match &event.logical_key {
            WKey::Named(NamedKey::Escape) => self.close(el),
            WKey::Named(NamedKey::F5) => self.change(),
            _ => {}
        }
    }

    // ------------------------------------------------------------ window

    fn show(&mut self) {
        if let Some(w) = &self.win {
            self.autostart = rwall::autostart::enabled();
            w.window.set_visible(true);
            w.window.set_minimized(false);
            w.window.focus_window();
            self.visible = true;
            self.redraw();
        }
    }

    /// The close button and Esc: hide when resident, else end rwall.
    fn close(&mut self, el: &ActiveEventLoop) {
        if self.tray {
            self.hide();
        } else {
            self.quit(el);
        }
    }

    fn hide(&mut self) {
        self.unfocus();
        #[cfg(not(windows))]
        if self.resident.is_none() {
            // Without an icon the window is the only way back: keep it.
            if let Some(w) = &self.win {
                w.window.set_minimized(true);
            }
            return;
        }
        if let Some(w) = &self.win {
            w.window.set_visible(false);
        }
        self.visible = false;
        self.hot = None;
    }

    fn quit(&mut self, el: &ActiveEventLoop) {
        self.unfocus();
        self.save();
        #[cfg(windows)]
        if let Some(w) = &self.win {
            rt_sys::win::tray::remove(w.hwnd);
        }
        #[cfg(not(windows))]
        {
            self.resident = None;
            self.listener = None;
        }
        el.exit();
    }

    fn paint(&mut self) {
        let theme = Theme::new(self.dark(), self.accent);
        let s = self.scale();
        let status = self.status();
        let Some(w) = &mut self.win else { return };
        let size = w.window.inner_size();
        let (Some(wd), Some(ht)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if w.surface.resize(wd, ht).is_err() {
            return;
        }
        self.gfx.resize(size.width, size.height);
        let full = Rect::new(0.0, 0.0, size.width as f32, size.height as f32);
        let model = Model {
            settings: &self.settings,
            thumb: self.thumb.as_ref(),
            busy: self.busy,
            status,
            error: self.error.as_deref(),
            autostart: self.autostart,
            focus: self.focus,
            hot: self.hot,
            scroll: self.scroll,
        };
        self.drawn = view::draw(&mut self.gfx, &model, &theme, s, full);
        // Keep the scroll inside the content after a resize.
        let max = (self.drawn.content_h - full.h).max(0.0);
        if self.scroll > max {
            self.scroll = max;
            w.window.request_redraw();
        }
        if let Ok(mut buf) = w.surface.buffer_mut() {
            self.gfx.copy_to(&mut buf);
            let _ = buf.present();
        }
    }

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let attrs = Window::default_attributes()
            .with_title("rwall")
            .with_inner_size(LogicalSize::new(640.0, 760.0))
            .with_min_inner_size(LogicalSize::new(500.0, 360.0))
            .with_window_icon(app_icon())
            .with_theme(Some(if self.dark() { WinTheme::Dark } else { WinTheme::Light }))
            .with_visible(false);
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("rwall: cannot create window: {e}");
                el.exit();
                return false;
            }
        };
        let surface = match softbuffer::Context::new(window.clone()).and_then(|ctx| softbuffer::Surface::new(&ctx, window.clone())) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rwall: cannot create drawing surface: {e}");
                el.exit();
                return false;
            }
        };
        if let Some(m) = el.primary_monitor() {
            let sz = m.size();
            self.screen = (sz.width.max(640), sz.height.max(480));
        }
        #[cfg(windows)]
        let hwnd = hwnd_of(&window);
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
            if self.tray {
                rt_sys::win::set_resident(hwnd, &IDENT);
                rt_sys::win::tray::show(hwnd, &self.tip());
            }
        }
        self.win = Some(Win {
            window,
            surface,
            #[cfg(windows)]
            hwnd,
        });
        true
    }

    fn pointer(&mut self, x: f32, y: f32) {
        let hot = self.drawn.hit(x, y);
        if hot != self.hot {
            self.hot = hot;
            self.redraw();
        }
        let icon = match hot {
            Some(Hit::Field(_)) => CursorIcon::Text,
            Some(_) => CursorIcon::Pointer,
            None => CursorIcon::Default,
        };
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(w) = &self.win {
                w.window.set_cursor(icon);
            }
        }
    }
}

fn app_icon() -> Option<WinIcon> {
    let n = 32;
    WinIcon::from_rgba(rwall::ui::appicon::render(n), n, n).ok()
}

#[cfg(windows)]
fn hwnd_of(window: &Window) -> rt_sys::win::HWND {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get() as _,
        _ => std::ptr::null_mut(),
    }
}

#[cfg(windows)]
fn show_tray_menu(shell: &mut Shell, el: &ActiveEventLoop) {
    use rt_sys::win::tray::{popup, Item};
    let Some(hwnd) = shell.win.as_ref().map(|w| w.hwnd) else { return };
    let link = shell.settings.current.as_ref().is_some_and(|c| !c.link.is_empty());
    let menu = tray_menu();
    let texts: Vec<String> = menu.iter().map(|m| if let M::Item(c) = m { tr(c.key()) } else { String::new() }).collect();
    let items: Vec<Item> = menu
        .iter()
        .zip(&texts)
        .map(|(m, t)| match m {
            M::Item(c) => Item {
                id: c.id(),
                text: t,
                enabled: match c {
                    TrayCmd::Next => !shell.busy && !shell.settings.sources.is_empty(),
                    TrayCmd::About => link,
                    _ => true,
                },
            },
            _ => Item::SEPARATOR,
        })
        .collect();
    if let Some(c) = popup(hwnd, &items, TrayCmd::Settings.id()).and_then(TrayCmd::from_id) {
        shell.tray_command(el, c);
    }
}

#[cfg(not(windows))]
fn tray_labels(tip: &str) -> rt_sys::linux::shell::TrayLabels {
    rt_sys::linux::shell::TrayLabels { tip: tip.to_string(), open: tr("&Settings…").replace('&', ""), exit: tr("E&xit").replace('&', "") }
}

#[cfg(not(windows))]
fn tray_items() -> Vec<(u32, String)> {
    tray_menu()
        .iter()
        .map(|m| match m {
            // StatusNotifierItem menus mark access keys with '_'.
            M::Item(c) => (c.id(), tr(c.key()).replace('_', "__").replace('&', "_")),
            _ => (0, String::new()),
        })
        .collect()
}
/// Without `--tray`: listens for later launches, which show this window.
#[cfg(not(windows))]
fn start_listener(shell: &mut Shell) {
    use rt_sys::linux::shell as ls;
    let proxy = std::sync::Mutex::new(shell.proxy.clone());
    let cfg = ls::Config { app: "rwall", icon: rwall::ui::appicon::render, tray: None, listen_without_icon: true };
    shell.listener = Some(ls::Shell::new(
        cfg,
        Box::new(move |m| {
            if let Ok(p) = proxy.lock() {
                let _ = p.send_event(UserEvent::Shell(m));
            }
        }),
    ));
}

/// Starts the notification-area icon and the socket for later launches.
#[cfg(not(windows))]
fn start_resident(shell: &mut Shell) {
    use rt_sys::linux::shell as ls;
    let proxy = std::sync::Mutex::new(shell.proxy.clone());
    let items_proxy = std::sync::Mutex::new(shell.proxy.clone());
    let cfg = ls::Config { app: "rwall", icon: rwall::ui::appicon::render, tray: Some(tray_labels(&shell.tip())), listen_without_icon: true };
    let resident = ls::Shell::with_items(
        cfg,
        tray_items(),
        Some(Box::new(move |id| {
            if let Ok(p) = items_proxy.lock() {
                let _ = p.send_event(UserEvent::TrayItem(id));
            }
        })),
        Box::new(move |m| {
            if let Ok(p) = proxy.lock() {
                let _ = p.send_event(UserEvent::Shell(m));
            }
        }),
    );
    if !resident.has_icon() {
        // Without an icon the window must stay reachable.
        shell.show_on_start = true;
    }
    shell.resident = resident.has_icon().then_some(resident);
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        #[cfg(not(windows))]
        if self.tray {
            start_resident(self);
        } else {
            start_listener(self);
        }
        if !self.open_window(el) {
            return;
        }
        if self.thumb.is_none() {
            self.load_thumb();
        }
        self.tick();
        if self.show_on_start {
            self.show();
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Changed(r) => self.changed(r),
            UserEvent::Applied(r) => {
                if let Err(e) = r {
                    self.error = Some(tf("The picture could not be changed: {}", &[&e]));
                }
                self.redraw();
            }
            UserEvent::Thumb(t) => {
                if self.thumb.is_none() {
                    self.thumb = t;
                }
                self.redraw();
            }
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::OpenFiles(paths)) => {
                if paths.iter().any(|p| p.file_name().is_some_and(|n| n == NEXT_MARK)) {
                    self.change();
                } else {
                    self.show();
                }
            }
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::Tray(rt_sys::win::tray::TrayEvent::Activate)) => self.show(),
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::Tray(rt_sys::win::tray::TrayEvent::Menu)) => show_tray_menu(self, el),
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::SystemTheme) => {
                self.system_dark = rt_sys::system_dark().unwrap_or(false);
                self.accent = rt_sys::accent().map(Color::rgb);
                if let Some(w) = &self.win {
                    w.window.set_theme(Some(if self.dark() { WinTheme::Dark } else { WinTheme::Light }));
                }
                self.redraw();
            }
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::Command(_)) => {}
            #[cfg(not(windows))]
            UserEvent::Shell(rt_sys::linux::shell::Msg::Open(paths)) => {
                if paths.iter().any(|p| p.file_name().is_some_and(|n| n == NEXT_MARK)) {
                    self.change();
                } else {
                    self.show();
                }
            }
            #[cfg(not(windows))]
            UserEvent::Shell(rt_sys::linux::shell::Msg::Quit) => self.quit(el),
            #[cfg(not(windows))]
            UserEvent::TrayItem(id) => {
                if let Some(c) = TrayCmd::from_id(id) {
                    self.tray_command(el, c);
                }
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.close(el),
            WindowEvent::RedrawRequested => self.paint(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.redraw(),
            WindowEvent::ThemeChanged(t) => {
                self.system_dark = t == WinTheme::Dark;
                self.redraw();
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } if event.state == ElementState::Pressed => self.key(el, &event),
            WindowEvent::CursorMoved { position, .. } => self.pointer(position.x as f32, position.y as f32),
            WindowEvent::CursorLeft { .. } => {
                if self.hot.take().is_some() {
                    self.redraw();
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let hit = self.hot;
                if !matches!(hit, Some(Hit::Field(_))) {
                    self.unfocus();
                }
                if let Some(h) = hit {
                    self.click(h);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let s = self.scale();
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 48.0 * s,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                let h = self.win.as_ref().map_or(0.0, |w| w.window.inner_size().height as f32);
                let max = (self.drawn.content_h - h).max(0.0);
                let new = (self.scroll - dy).clamp(0.0, max);
                if new != self.scroll {
                    self.scroll = new;
                    self.hot = None;
                    self.redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_tick {
            self.next_tick = now + TICK;
            self.tick();
            // The "next change" line follows the clock.
            self.redraw();
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.next_tick));
    }
}

const USAGE: &str = "rwall [--tray | --auto]

  Changes the desktop wallpaper with pictures from Bing, NASA's Astronomy
  Picture of the Day, Wallhaven, Picsum or your own folders: every day, twice
  or four times a day, every week, every month or every so many minutes.

  (none)   Show the settings: the picture now, Next picture, where pictures
           come from, how often and how they fill the screen. Closing the
           window ends rwall; the picture changes only when asked.
  --tray   Stay in the notification area and change the picture as often as
           chosen. Clicking the icon shows the settings; closing them only
           hides them. Exit from the icon's menu. \"Start with Windows\"
           starts rwall this way.
  --auto   Change the picture once with the saved settings, without a
           window, and end (for the Task Scheduler, a shortcut or a
           script). With rwall already running, that rwall changes it.

  A later launch shows the settings of the rwall already running.

Settings and downloaded pictures are kept in the user's settings folder
(%APPDATA%\\rwall on Windows, ~/.config/rwall on Linux), or next to the
program when a file named rwall.portable exists there (portable mode).";

/// The size of the main screen, in pixels, without a window.
fn screen_size() -> (u32, u32) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        // SAFETY: no preconditions.
        let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
        if w > 0 && h > 0 {
            return ((w as u32).max(640), (h as u32).max(480));
        }
    }
    (1920, 1080)
}

/// `--auto`: one change with the saved settings, without a window. Returns
/// the exit code; what went wrong goes to the console, if there is one.
fn auto() -> i32 {
    let mut location = storage::detect();
    let mut settings = Settings::load(&location.load());
    i18n::set_lang(settings.language.unwrap_or_else(i18n::system_lang));
    let result = (|| {
        let images = storage::images_dir(&location).ok_or_else(|| tr("No user data folder is available."))?;
        let job = changer::job(&settings, images, screen_size(), (1, 1)).ok_or_else(|| tr("Turn on at least one source of pictures."))?;
        let done = std::panic::catch_unwind(|| changer::run(&job))
            .unwrap_or_else(|_| Err("the picture could not be read".into()))
            .map_err(|e| tf("The picture could not be changed: {}", &[&e]))?;
        changer::record(&mut settings, done.pick, done.file, &rt_sys::local_time());
        if location.save(&settings.to_store()) {
            Ok(())
        } else {
            Err(location.error.clone().unwrap_or_default())
        }
    })();
    match result {
        Ok(()) => 0,
        Err(e) => {
            #[cfg(windows)]
            rt_sys::win::print_to_console(&format!("rwall: {e}"));
            #[cfg(not(windows))]
            eprintln!("rwall: {e}");
            1
        }
    }
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.iter().any(|a| a == "--help" || a == "-h" || a == "/?") {
        #[cfg(windows)]
        rt_sys::win::report("rwall", USAGE);
        #[cfg(not(windows))]
        println!("{USAGE}");
        return;
    }
    let tray = raw.iter().any(|a| a == "--tray" || a == "/tray");
    let auto_once = raw.iter().any(|a| a == "--auto" || a == "/auto");
    // One rwall at a time: a later launch shows the running one's settings;
    // --auto asks it for the next picture.
    let ask: Vec<PathBuf> = if auto_once { vec![PathBuf::from(NEXT_MARK)] } else { Vec::new() };
    #[cfg(windows)]
    if rt_sys::win::hand_off(&IDENT, &ask, true, !auto_once) {
        return;
    }
    #[cfg(not(windows))]
    if rt_sys::linux::shell::hand_off("rwall", &ask, true) {
        return;
    }
    if auto_once {
        std::process::exit(auto());
    }    let mut location = storage::detect();
    let first_run = location.settings_file().is_none_or(|f| !f.exists());
    let settings = Settings::load(&location.load());
    let gfx = match Gfx::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rwall: {e}");
            std::process::exit(1);
        }
    };
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("rwall: cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    let mut shell = Shell {
        settings,
        location,
        gfx,
        win: None,
        proxy: event_loop.create_proxy(),
        visible: false,
        system_dark: rt_sys::system_dark().unwrap_or(false),
        accent: rt_sys::accent().map(Color::rgb),
        thumb: None,
        busy: false,
        error: None,
        retry_at: None,
        next_tick: Instant::now() + TICK,
        autostart: rwall::autostart::enabled(),
        focus: None,
        hot: None,
        scroll: 0.0,
        drawn: Drawn::default(),
        mods: ModifiersState::empty(),
        cursor_icon: CursorIcon::Default,
        tray,
        show_on_start: !tray || first_run,
        screen: (1920, 1080),
        #[cfg(not(windows))]
        resident: None,
        #[cfg(not(windows))]
        listener: None,
    };
    shell.apply_language();
    if first_run {
        // The file marks that rwall has run, and holds the defaults to edit.
        shell.save();
    }
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rwall: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_mark_is_not_a_file_name() {
        assert!(NEXT_MARK.contains('?'));
    }
}
