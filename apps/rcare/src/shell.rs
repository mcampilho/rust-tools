//! The window of rcare: winit, drawn on the CPU (tiny-skia + ab_glyph) into
//! a softbuffer surface. The sidebar and the menus are the window's; what is
//! to the right of the sidebar is the page the sidebar chose: the cleaner,
//! the duplicates or the disk space, each the page of a small app.

use rcare::commands::{self, Cmd};
use rcare::custom::{self, CustomPage, Part, Status};
use rcare::extensions_page::ExtensionsPage;
use rcare::i18n::{self, tf, tr};
use rcare::maint::{self, Check, MaintPage};
use rcare::pages::{self, Kind, PAGES};
use rcare::protect_page::ProtectPage;
use rcare::registry_page::RegistryPage;
use rcare::services_page::ServicesPage;
use rcare::settings::Settings;
use rcare::sidebar::{self, Hit};
use rcare::speed_page::SpeedPage;
use rcare::startup_page::StartupPage;
use rcare::storage;
use rclean::app::App;
use rclean::commands::Command as CleanCmd;
use rclean::page::CleanPage;
use rclean::ui::gfx::{Color, Gfx, Rect};
use rt_ui::page::{Dialogs, Effects, Mods, SystemDialogs};
use rt_ui::Message;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Theme as WinTheme, Window, WindowId};

enum UserEvent {
    /// A step of Check for Updates.
    Update(rt_ui::update::Step),
    A11y(accesskit_winit::Event),
    /// What a thread of a page reports.
    Clean(rclean::page::Event),
    Dups(rdup::page::Event),
    Size(rsize::page::Event),
    Inst(rinst::page::Event),
    /// What Windows says about the drives (None: it could not be asked).
    Disks(Option<Vec<rcare::disks::Volume>>),
    Win(rt_sys::win::WinMsg),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(e: accesskit_winit::Event) -> Self {
        UserEvent::A11y(e)
    }
}

const IDENT: rt_sys::win::Ident = rt_sys::win::Ident { name: "rcare", copydata: 0x5243_4152 }; // "RCAR"

/// Runs `$e` with `$p` the page now shown (each is the page of a small app).
macro_rules! page {
    ($s:expr, $p:ident => $e:expr) => {
        match PAGES[$s.selected].kind {
            Kind::Custom => {
                let $p = &mut $s.custom;
                $e
            }
            Kind::Maint => {
                let $p = &mut $s.maint;
                $e
            }
            Kind::Startup => {
                let $p = &mut $s.startup;
                $e
            }
            Kind::Speed => {
                let $p = &mut $s.speed;
                $e
            }
            Kind::Services => {
                let $p = &mut $s.services;
                $e
            }
            Kind::Extensions => {
                let $p = &mut $s.extensions;
                $e
            }
            Kind::Registry => {
                let $p = &mut $s.registry;
                $e
            }
            Kind::Protect => {
                let $p = &mut $s.protect;
                $e
            }
            Kind::Clean => {
                let $p = &mut $s.clean;
                $e
            }
            Kind::Dups => {
                let $p = &mut $s.dups;
                $e
            }
            Kind::Size => {
                let $p = &mut $s.size;
                $e
            }
            Kind::Inst => {
                let $p = &mut $s.inst;
                $e
            }
        }
    };
}

struct Win {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    adapter: accesskit_winit::Adapter,
    hwnd: rt_sys::win::HWND,
    menu: Option<rt_sys::win::menu::MenuBar<Cmd>>,
}

struct Shell {
    /// Custom Clean: rcare's own page, an overview of three of the others.
    custom: CustomPage,
    /// System Maintenance: rcare's own page, the checks in one go.
    maint: MaintPage,
    /// Startup Optimizer: rcare's own page, what starts with Windows.
    startup: StartupPage,
    /// Speed Up: rcare's own page, Game Mode, memory, processor, programs open.
    speed: SpeedPage,
    /// Program Deactivator: rcare's own page, the services of other companies.
    services: ServicesPage,
    /// Extensions: rcare's own page, what is added to the browsers.
    extensions: ExtensionsPage,
    /// Registry Cleaner: rcare's own page, invalid registry entries.
    registry: RegistryPage,
    /// Protect System: rcare's own page, the antivirus, the firewall and Defender.
    protect: ProtectPage,
    /// What Windows said about the drives, the last time it was asked.
    volumes: Option<Vec<rcare::disks::Volume>>,
    /// Asking Windows about the drives now; it could not be asked the last time.
    disks_running: bool,
    disks_failed: bool,
    /// The large files of the last tree read by the disk space page: for which tree, and (bytes, count).
    large_cache: Option<((usize, u64), (u64, u64))>,
    clean: CleanPage,
    dups: rdup::page::DupPage,
    size: rsize::page::SizePage,
    inst: rinst::page::InstPage,
    /// The settings the duplicates page keeps (its own file, `rdup.ini`).
    dups_settings: rdup::settings::Settings,
    /// The settings the uninstaller page keeps (its own file, `rinst.ini`).
    inst_settings: rinst::settings::Settings,
    /// Which pages did their first work (reading the drives, the first look).
    started: [bool; PAGES.len()],
    /// The index of the page shown.
    selected: usize,
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
    cursor_icon: CursorIcon,
    side: sidebar::Layout,
    side_hot: Option<Hit>,
    dialog: Option<Message>,
    /// A folder given on the command line or by the Explorer menu: Custom
    /// Clean looks there this time (the folder chosen in the settings stays).
    custom_once: Option<std::path::PathBuf>,
    /// Resident mode (`--tray`): closing the window hides it to the
    /// notification area; the icon or another launch shows it.
    tray: bool,
    hidden: bool,
    /// The window was last maximized: done when it is first shown.
    maximize_on_show: bool,
    /// Starts with the session (as `rcare --tray`).
    autostart: bool,
    /// "Analyze with rcare" is on the Explorer menu of folders, for this rcare.exe.
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
        self.settings.ticks = self.clean.app.ticks();
        self.settings.page = PAGES[self.selected].id.to_string();
        let store = self.settings.to_store();
        self.location.save(&store);
        // The pages of rdup, rsize and rinst keep their own settings, each in its file.
        self.dups.store_into(&mut self.dups_settings);
        self.inst.store_into(&mut self.inst_settings);
        let _ = self.location.write_text("rinst.ini", &self.inst_settings.to_store().serialize("rinst"));
        let _ = self.location.write_text("rdup.ini", &self.dups_settings.to_store().serialize("rdup"));
        let _ = self.location.write_text("rsize.ini", &self.size.settings.to_store().serialize("rsize"));
    }

    /// What the pages asked of the window, all together.
    fn take_fx(&mut self) -> Effects {
        let mut fx = Effects::default();
        for e in [
            self.speed.fx.take(),
            self.startup.fx.take(),
            self.services.fx.take(),
            self.extensions.fx.take(),
            self.registry.fx.take(),
            self.protect.fx.take(),
            self.maint.fx.take(),
            self.custom.fx.take(),
            self.clean.fx.take(),
            self.dups.fx.take(),
            self.size.fx.take(),
            self.inst.fx.take(),
        ] {
            fx.redraw |= e.redraw;
            fx.menu_dirty |= e.menu_dirty;
            fx.save |= e.save;
            if fx.info.is_none() {
                fx.info = e.info;
            }
        }
        fx
    }

    /// Takes what the pages asked of the window, and the pointer's shape.
    fn after_page(&mut self) {
        let fx = self.take_fx();
        if fx.menu_dirty {
            self.menu_dirty = true;
        }
        if fx.save {
            self.save_settings();
        }
        if let Some((title, text)) = fx.info {
            self.clean.dragging = false;
            self.dups.dragging = false;
            self.inst.dragging = false;
            self.dialog = Some(Message::new(&title, &text));
        }
        if fx.redraw {
            self.redraw();
        }
        self.sync_cursor();
    }

    fn sync_cursor(&mut self) {
        let icon = if self.side_hot.is_some() { CursorIcon::Pointer } else { page!(self, p => p.cursor_icon) };
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(w) = &self.win {
                w.window.set_cursor(icon);
            }
        }
    }

    /// The system's dialogs, over this window.
    fn dialogs(&self) -> SystemDialogs {
        SystemDialogs { hwnd: self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd) }
    }

    fn info(&mut self, title: &str, text: &str) {
        self.dialog = Some(Message::new(title, text));
        self.clean.dragging = false;
        self.dups.dragging = false;
        self.inst.dragging = false;
        self.redraw();
    }

    fn admin(&self) -> bool {
        self.clean.admin
    }

    fn update_title(&self) {
        let Some(w) = &self.win else { return };
        let title = if self.admin() { format!("rcare ({})", tr("Administrator")) } else { "rcare".to_string() };
        if w.window.title() != title {
            w.window.set_title(&title);
        }
    }

    // ------------------------------------------------------------ pages

    /// The first work of a page, the first time it is shown: the cleaner
    /// looks at what there is to clean, the others read the drives.
    fn ensure_started(&mut self, i: usize) {
        if std::mem::replace(&mut self.started[i], true) {
            return;
        }
        match PAGES[i].kind {
            // Custom Clean does nothing before it is asked: the three analyses take a while.
            Kind::Custom | Kind::Maint => {}
            Kind::Startup => self.startup.refresh(),
            Kind::Services => self.services.refresh(),
            Kind::Extensions => self.extensions.refresh(),
            Kind::Registry => self.registry.scan(),
            Kind::Protect => self.protect.refresh(),
            Kind::Speed => self.speed.refresh(),
            Kind::Clean => self.clean.analyse(),
            Kind::Dups => self.dups.load_volumes(),
            Kind::Size => self.size.load_volumes(),
            Kind::Inst => self.inst.refresh(),
        }
        self.after_page();
    }

    fn select_page(&mut self, i: usize) {
        if i < PAGES.len() && i != self.selected {
            // Speed Up reads every two seconds only while it is shown.
            if PAGES[self.selected].kind == Kind::Speed {
                self.speed.pause();
            }
            if PAGES[i].kind == Kind::Speed && self.started[i] {
                self.speed.refresh();
            }
            self.selected = i;
            self.menu_rebuild = true;
            self.menu_dirty = true;
            self.ensure_started(i);
            self.size.resized();
            self.redraw();
        }
    }

    // ------------------------------------------------------------ System Maintenance

    /// Nothing that System Maintenance starts is running.
    fn maint_idle(&self) -> bool {
        self.clean.app.busy.is_none() && !self.inst.app.loading && self.inst.idle() && !self.disks_running
    }

    /// How far each check got, read from the pages that do the work.
    fn maint_status(&self) -> [Status; 5] {
        use rclean::targets::Group;
        let clean_done = self.clean.app.busy.is_none() && self.started[pages::index_of("clean")];
        let of_group = |want: &dyn Fn(Group) -> bool, ticked_only: bool| {
            if self.clean.app.busy.is_some() {
                return Status::Working;
            }
            if !clean_done {
                return Status::NotYet;
            }
            let (mut bytes, mut count) = (0, 0);
            for it in &self.clean.app.items {
                if want(it.target.group) && (it.ticked || !ticked_only) {
                    if let Some(f) = it.found() {
                        bytes += f.bytes;
                        count += f.count();
                    }
                }
            }
            Status::Done { bytes, count }
        };
        let junk = of_group(&|g| !matches!(g, Group::Shortcuts | Group::Registry), true);
        let shortcuts = of_group(&|g| g == Group::Shortcuts, false);
        let registry = of_group(&|g| g == Group::Registry, false);
        let software = if self.inst.app.loading {
            Status::Working
        } else if let Some(found) = &self.inst.app.found {
            use rinst::inventory::Problem;
            let count = rinst::app::Tab::LISTS
                .iter()
                .flat_map(|&t| found.list(t))
                .filter(|p| p.problems.iter().any(|q| *q != Problem::ConfigOnly))
                .count();
            Status::Done { bytes: 0, count: count as u64 }
        } else {
            Status::NotYet
        };
        let disks = if self.disks_running {
            Status::Working
        } else if let Some(v) = &self.volumes {
            Status::Done { bytes: 0, count: v.iter().filter(|v| v.needs_repair()).count() as u64 }
        } else if self.disks_failed {
            Status::Failed(tr("Could not ask Windows."))
        } else {
            Status::NotYet
        };
        [junk, shortcuts, registry, software, disks]
    }

    /// Runs every check: the cleaner analyses, the uninstaller reads the
    /// programs, Windows is asked about the drives.
    fn maint_scan(&mut self) {
        if !self.maint_idle() {
            return;
        }
        self.started[pages::index_of("clean")] = true;
        self.clean.analyse();
        self.started[pages::index_of("inst")] = true;
        self.inst.refresh();
        self.disks_running = true;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let _ = proxy.send_event(UserEvent::Disks(rcare::disks::read()));
        });
        self.after_page();
        self.menu_dirty = true;
        self.redraw();
    }

    /// A row's button: the page that fixes it, or (the drives) its details.
    fn maint_review(&mut self, i: usize) {
        match Check::ALL.get(i).map(|c| c.page_id()) {
            Some(Some(id)) => self.select_page(pages::index_of(id)),
            Some(None) => {
                let text = maint::disks_text(self.volumes.as_deref().unwrap_or(&[]));
                self.info(&tr("Drive Health"), &text);
            }
            None => {}
        }
    }

    // ------------------------------------------------------------ Custom Clean

    /// Where Custom Clean looks for duplicates and large files: the folder chosen, or the user's.
    fn custom_folder(&self) -> std::path::PathBuf {
        // A folder given by the Explorer menu counts for this time only.
        if let Some(p) = &self.custom_once {
            return p.clone();
        }
        if !self.settings.custom_folder.is_empty() {
            return std::path::PathBuf::from(&self.settings.custom_folder);
        }
        std::env::var_os("USERPROFILE").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("C:\\"))
    }

    /// Nothing that Custom Clean starts is running.
    fn custom_idle(&self) -> bool {
        self.clean.app.busy.is_none() && !self.dups.searching() && !self.size.scanning()
    }

    /// How far each part got, read from the pages that do the work.
    fn custom_status(&mut self) -> [Status; 3] {
        let junk = if self.clean.app.busy.is_some() {
            Status::Working
        } else if self.started[pages::index_of("clean")] {
            let (count, bytes) = self.clean.app.ticked_total();
            Status::Done { bytes, count }
        } else {
            Status::NotYet
        };
        let dups = if self.dups.searching() {
            Status::Working
        } else if let Some(f) = &self.dups.app.found {
            Status::Done { bytes: f.wasted(), count: f.groups.len() as u64 }
        } else if let Some(e) = &self.dups.app.error {
            Status::Failed(e.clone())
        } else {
            Status::NotYet
        };
        let large = if self.size.scanning() {
            Status::Working
        } else if let Some(scan) = &self.size.app.scan {
            // The tree can hold millions of nodes: counted once per tree, not each frame.
            let t = &scan.tree;
            let key = (t.len(), t.size(t.root()));
            let (bytes, count) = match self.large_cache {
                Some((k, v)) if k == key => v,
                _ => {
                    let v = custom::large_files(t);
                    self.large_cache = Some((key, v));
                    v
                }
            };
            Status::Done { bytes, count }
        } else if let Some(e) = &self.size.app.error {
            Status::Failed(e.clone())
        } else {
            Status::NotYet
        };
        [junk, dups, large]
    }

    /// Starts the three analyses: the cleaner's, a search for duplicates in
    /// the folder, and a reading of the folder for its large files.
    fn custom_analyze(&mut self) {
        if !self.custom_idle() {
            return;
        }
        let folder = self.custom_folder();
        let (c, d, z) = (pages::index_of("clean"), pages::index_of("dups"), pages::index_of("size"));
        self.started[c] = true;
        self.clean.analyse();
        if !std::mem::replace(&mut self.started[d], true) {
            self.dups.load_volumes();
        }
        self.dups.open_paths(vec![folder.clone()]);
        if !std::mem::replace(&mut self.started[z], true) {
            self.size.load_volumes();
        }
        self.size.start_scan(&folder);
        self.after_page();
        self.menu_dirty = true;
        self.redraw();
    }

    /// Opens the page that shows one part in full.
    fn custom_review(&mut self, i: usize) {
        if let Some(part) = Part::ALL.get(i) {
            self.select_page(pages::index_of(part.page_id()));
        }
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self, el: &ActiveEventLoop, cmd: Cmd) {
        if !self.enabled(cmd) {
            return;
        }
        match cmd {
            Cmd::Clean(c) => {
                let mut d = self.dialogs();
                self.clean.command(c, &mut d);
                self.after_page();
            }
            Cmd::Dups(c) => {
                let mut d = self.dialogs();
                self.dups.command(c, &mut d);
                self.after_page();
            }
            Cmd::Size(c) => {
                let mut d = self.dialogs();
                self.size.command(c, &mut d);
                self.after_page();
            }
            Cmd::Inst(c) => {
                let mut d = self.dialogs();
                self.inst.command(c, &mut d);
                self.after_page();
            }
            Cmd::MaintScan => self.maint_scan(),
            Cmd::Startup(a) => self.startup.act(a),
            Cmd::Extensions(a) => self.extensions.act(a),
            Cmd::Protect(a) => {
                let mut d = self.dialogs();
                self.protect.act(a, &mut d);
                self.after_page();
            }
            Cmd::Registry(a) => {
                let mut d = self.dialogs();
                self.registry.act(a, &mut d);
                self.after_page();
            }
            Cmd::Services(a) => {
                let mut d = self.dialogs();
                self.services.act(a, &mut d);
                self.after_page();
            }
            Cmd::Speed(a) => {
                let mut d = self.dialogs();
                self.speed.act(a, &mut d);
                self.after_page();
            }
            Cmd::CustomAnalyze => self.custom_analyze(),
            Cmd::CustomFolder => {
                let mut d = self.dialogs();
                if let Some(p) = d.pick_folder(&tr("Choose the folder to look in")) {
                    self.settings.custom_folder = p.display().to_string();
                    self.custom_once = None;
                    self.save_settings();
                }
            }
            Cmd::ExplorerMenu => {
                let text = match integrate(!self.explorer_menu) {
                    Ok(t) | Err(t) => t,
                };
                self.explorer_menu = integrated_here();
                self.info(&tr("Windows Integration"), &text);
            }
            Cmd::Autostart => match rt_sys::autostart::set("rcare", !self.autostart) {
                Ok(()) => self.autostart = rt_sys::autostart::enabled("rcare"),
                Err(e) => self.info("rcare", &tf("Cannot change how it starts: {}", &[&e])),
            },
            Cmd::Sidebar => {
                self.settings.sidebar_collapsed = !self.settings.sidebar_collapsed;
                self.size.resized();
            }
            Cmd::RestartAdmin => {
                self.save_settings();
                let args = match PAGES[self.selected].kind {
                    Kind::Clean | Kind::Custom | Kind::Maint | Kind::Startup | Kind::Speed | Kind::Services | Kind::Extensions => {
                        String::new()
                    }
                    Kind::Dups => self.dups.restart_args(),
                    Kind::Size => self.size.restart_args(),
                    Kind::Inst | Kind::Registry | Kind::Protect => String::new(),
                };
                if rt_sys::win::restart_elevated(&args) {
                    self.quit(el);
                }
            }
            Cmd::Exit => self.quit(el),
            Cmd::Theme(t) => {
                self.settings.theme = t;
                self.apply_window_theme();
            }
            Cmd::UiLanguage(i) => {
                self.settings.language = (i as usize).checked_sub(1).and_then(|k| i18n::Lang::ALL.get(k)).copied();
                self.apply_language();
            }
            Cmd::Shortcuts => {
                let keys = match PAGES[self.selected].kind {
                    Kind::Custom => format!("F5\t{}", rt_core::i18n::strip_mnemonic(&tr("&Analyze"))),
                    Kind::Maint => format!("F5\t{}", rt_core::i18n::strip_mnemonic(&tr("&Scan"))),
                    Kind::Protect => format!("F5\t{}", rt_core::i18n::strip_mnemonic(&tr("&Refresh"))),
                    Kind::Registry => format!(
                        "F5\t{}\n{}\t{}\n↑ ↓  PgUp PgDn  Home End\t{}",
                        rt_core::i18n::strip_mnemonic(&tr("&Scan")),
                        tr("Space"),
                        tr("Tick or untick the selected category"),
                        tr("Move in the list")
                    ),
                    Kind::Extensions => format!(
                        "F5\t{}\n{}\t{}\n↑ ↓  PgUp PgDn  Home End\t{}",
                        rt_core::i18n::strip_mnemonic(&tr("&Refresh")),
                        tr("Enter"),
                        rt_core::i18n::strip_mnemonic(&tr("Open in &Browser")),
                        tr("Move in the list")
                    ),
                    Kind::Speed => format!(
                        "F5\t{}\n{}\t{}",
                        rt_core::i18n::strip_mnemonic(&tr("&Refresh")),
                        tr("Space"),
                        rt_core::i18n::strip_mnemonic(&tr("&Close the Ticked Programs...")).trim_end_matches("...")
                    ),
                    Kind::Startup | Kind::Services => format!(
                        "F5\t{}\n{}\t{}\n↑ ↓  PgUp PgDn  Home End\t{}",
                        rt_core::i18n::strip_mnemonic(&tr("&Refresh")),
                        tr("Space"),
                        tr("Turn the selected item on or off"),
                        tr("Move in the list")
                    ),
                    Kind::Clean => rclean::page::shortcuts_text(),
                    Kind::Dups => rdup::page::shortcuts_text(),
                    Kind::Size => rsize::page::shortcuts_text(),
                    Kind::Inst => rinst::page::shortcuts_text(),
                };
                let text = format!("Ctrl+B\t{}\n{keys}", tr("Sidebar"));
                self.info(&tr("Keyboard shortcuts"), &text);
            }
            Cmd::CheckUpdates => rt_ui::update::check("rcare", env!("CARGO_PKG_VERSION"), self.update_sender()),
            Cmd::About => {
                let text = tf(
                    "rcare {}\n\nWindows maintenance in one window: it brings together the cleaner and, step by step, the other tools of rust-tools.\nNo telemetry; the network only when you check for updates.\n\nMIT License.",
                    &[env!("CARGO_PKG_VERSION")],
                );
                self.info(&tr("About rcare"), &text);
            }
        }
        self.menu_dirty = true;
        self.redraw();
    }

    fn checked(&self, c: Cmd) -> bool {
        match c {
            Cmd::Dups(c) => self.dups.checked(c),
            Cmd::Size(c) => self.size.checked(c),
            Cmd::Inst(c) => self.inst.checked(c),
            Cmd::Sidebar => self.settings.sidebar_collapsed,
            Cmd::Autostart => self.autostart,
            Cmd::ExplorerMenu => self.explorer_menu,
            Cmd::Theme(t) => self.settings.theme == t,
            Cmd::UiLanguage(0) => self.settings.language.is_none(),
            Cmd::UiLanguage(i) => self.settings.language == i18n::Lang::ALL.get(i as usize - 1).copied(),
            _ => false,
        }
    }

    fn enabled(&self, c: Cmd) -> bool {
        match c {
            Cmd::Clean(c) => self.clean.enabled(c),
            Cmd::Dups(c) => self.dups.enabled(c),
            Cmd::Size(c) => self.size.enabled(c),
            Cmd::Inst(c) => self.inst.enabled(c),
            Cmd::CustomAnalyze | Cmd::CustomFolder => self.custom_idle(),
            Cmd::MaintScan => self.maint_idle(),
            Cmd::Startup(a) => self.startup.enabled(a),
            Cmd::Services(a) => self.services.enabled(a),
            Cmd::Extensions(a) => self.extensions.enabled(a),
            Cmd::Registry(a) => self.registry.enabled(a),
            Cmd::Protect(a) => self.protect.enabled(a),
            Cmd::Speed(a) => self.speed.enabled(a),
            Cmd::RestartAdmin => match PAGES[self.selected].kind {
                Kind::Clean
                | Kind::Protect
                | Kind::Custom
                | Kind::Maint
                | Kind::Startup
                | Kind::Speed
                | Kind::Services
                | Kind::Extensions
                | Kind::Registry => self.clean.enabled(CleanCmd::RestartAdmin),
                Kind::Dups => self.dups.enabled(rdup::commands::Command::RestartAdmin),
                Kind::Size => self.size.enabled(rsize::commands::Command::RestartAdmin),
                Kind::Inst => self.inst.enabled(rinst::commands::Command::RestartAdmin),
            },
            _ => true,
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
        self.clean.shutdown();
        self.dups.cancel();
        self.size.cancel();
        self.save_settings();
        if let Some(w) = &self.win {
            rt_sys::win::tray::remove(w.hwnd);
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

    /// Hides the window to the notification area; the work under way goes on.
    fn hide(&mut self) {
        self.save_settings();
        if let Some(w) = &self.win {
            w.window.set_visible(false);
        }
        self.hidden = true;
        self.clean.release();
        self.dups.release();
        self.size.release();
        self.inst.release();
        self.size.release_drawings();
        self.speed.pause();
        // Release the frame buffer while hidden.
        self.gfx.resize(1, 1);
        self.gfx.trim_cache();
    }

    /// Shows the window (from the tray icon or another launch).
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
        self.autostart = rt_sys::autostart::enabled("rcare");
        self.ensure_started(self.selected);
        self.menu_dirty = true;
        self.redraw();
    }

    /// Folders from a later launch (the Explorer menu): the window shows,
    /// and Custom Clean analyses the first one.
    fn open_paths(&mut self, el: &ActiveEventLoop, paths: Vec<std::path::PathBuf>) {
        self.show(el);
        if let Some(p) = paths.into_iter().next() {
            self.custom_once = Some(p);
            self.select_page(pages::index_of("custom"));
            self.custom_analyze();
        }
    }

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let (w, h, maximized) = self.settings.window.unwrap_or((1180.0, 720.0, false));
        let attrs = Window::default_attributes()
            .with_title("rcare")
            .with_inner_size(LogicalSize::new(w.max(760.0), h.max(480.0)))
            .with_min_inner_size(LogicalSize::new(760.0, 480.0))
            // Maximizing shows the window: a resident start, which stays
            // hidden, maximizes when it is first shown.
            .with_maximized(maximized && !self.tray)
            .with_window_icon(rt_ui::window::icon(rcare::appicon::render))
            .with_theme(Some(rt_ui::window::theme(self.dark())))
            .with_visible(false);
        let Some((window, surface)) = rt_ui::window::create(el, attrs, "rcare") else { return false };
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(el, &window, self.proxy.clone());
        let hwnd = rt_ui::window::hwnd_of(&window);
        let proxy = self.proxy.clone();
        rt_sys::win::subclass(
            hwnd,
            &IDENT,
            &rt_ui::window::mnemonics_of(&commands::menu_tree(self.selected)),
            Box::new(move |m| {
                let _ = proxy.send_event(UserEvent::Win(m));
            }),
        );
        if self.tray {
            rt_sys::win::set_resident(hwnd, &IDENT);
            rt_sys::win::tray::show(hwnd, "rcare");
        }
        self.maximize_on_show = maximized;
        self.win = Some(Win { window, surface, adapter, hwnd, menu: None });
        true
    }

    /// A menu at the pointer, asked for by the duplicates page.
    fn dups_popup(&mut self, el: &ActiveEventLoop, which: rdup::page::Popup) {
        use rdup::page::Popup;
        let items = match which {
            Popup::Rules => rdup::commands::rule_menu(),
            Popup::Item => rdup::commands::item_menu(),
        };
        let Some(hwnd) = self.win.as_ref().map(|w| w.hwnd) else { return };
        let items: Vec<_> = items
            .into_iter()
            .map(|m| match m {
                rt_core::menu::M::Item(c) => Some((c, self.dups.enabled(c))),
                _ => None,
            })
            .collect();
        if let Some(c) = rt_sys::win::menu::popup_at_cursor(hwnd, &items) {
            self.command(el, Cmd::Dups(c));
        }
    }

    /// A menu at the pointer, asked for by the uninstaller page.
    fn inst_popup(&mut self, el: &ActiveEventLoop, which: rinst::page::Popup) {
        let Some(hwnd) = self.win.as_ref().map(|w| w.hwnd) else { return };
        let page = &self.inst;
        if let Some(c) = rt_sys::win::menu::popup_at_pointer(hwnd, which.items(), |c| page.checked(c), |c| page.enabled(c)) {
            self.command(el, Cmd::from_inst(c));
        }
    }

    /// A menu at the pointer, asked for by the disk space page.
    fn size_popup(&mut self, el: &ActiveEventLoop, which: rsize::page::Popup) {
        let rsize::page::Popup::Item = which;
        let Some(hwnd) = self.win.as_ref().map(|w| w.hwnd) else { return };
        let items: Vec<_> = rsize::commands::item_menu()
            .into_iter()
            .map(|m| match m {
                rt_core::menu::M::Item(c) => Some((c, self.size.enabled(c))),
                _ => None,
            })
            .collect();
        if let Some(c) = rt_sys::win::menu::popup_at_cursor(hwnd, &items) {
            self.command(el, Cmd::Size(c));
        }
    }

    // ------------------------------------------------------------ menus

    fn sync_menu(&mut self) {
        let Some(win) = &mut self.win else { return };
        if !self.menu_dirty && win.menu.is_some() {
            return;
        }
        self.menu_dirty = false;
        if win.menu.is_none() || self.menu_rebuild {
            self.menu_rebuild = false;
            let tree = commands::menu_tree(self.selected);
            rt_sys::win::set_mnemonics(&rt_ui::window::mnemonics_of(&tree));
            win.menu = Some(rt_sys::win::menu::MenuBar::install(win.hwnd, tree));
        }
        let me = &*self;
        if let Some(m) = me.win.as_ref().and_then(|w| w.menu.as_ref()) {
            m.sync(|c| me.checked(c), |c| me.enabled(c));
        }
    }

    fn apply_window_theme(&self) {
        if let Some(w) = &self.win {
            rt_ui::window::apply_theme(&w.window, self.dark());
        }
    }

    fn sync_menu_theme(&mut self, dark: bool) {
        if self.menu_dark == Some(dark) {
            return;
        }
        self.menu_dark = Some(dark);
        if let Some(w) = &self.win {
            rt_ui::window::menu_theme(w.hwnd, dark);
        }
    }

    // ------------------------------------------------------------ drawing

    fn render(&mut self) {
        let dark = self.dark();
        let theme = rt_ui::theme::Theme::new(dark, self.accent);
        let s = self.scale();
        let Some(win) = &mut self.win else { return };
        let size = win.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if win.surface.resize(w, h).is_err() {
            return;
        }
        self.gfx.resize(w.get(), h.get());
        let full = Rect::new(0.0, 0.0, w.get() as f32, h.get() as f32);
        let mut rest = full;
        let side = rest.take_left(sidebar::width(self.settings.sidebar_collapsed, s));
        self.side = sidebar::layout(side, PAGES.len(), s);
        if PAGES[self.selected].kind == Kind::Custom {
            self.custom.view.status = self.custom_status();
            self.custom.view.folder = self.custom_folder().display().to_string();
            self.custom.view.can_analyze = self.custom_idle();
        }
        if PAGES[self.selected].kind == Kind::Maint {
            self.maint.view.status = self.maint_status();
            self.maint.view.can_scan = self.maint_idle();
        }
        match PAGES[self.selected].kind {
            Kind::Maint => self.maint.render(&mut self.gfx, rest, &theme, s),
            Kind::Startup => self.startup.render(&mut self.gfx, rest, &theme, s),
            Kind::Services => self.services.render(&mut self.gfx, rest, &theme, s),
            Kind::Extensions => self.extensions.render(&mut self.gfx, rest, &theme, s),
            Kind::Registry => self.registry.render(&mut self.gfx, rest, &theme, s),
            Kind::Protect => self.protect.render(&mut self.gfx, rest, &theme, s),
            Kind::Speed => self.speed.render(&mut self.gfx, rest, &theme, s),
            Kind::Custom => self.custom.render(&mut self.gfx, rest, &theme, s),
            Kind::Clean => self.clean.render(&mut self.gfx, rest, &theme, s, false),
            Kind::Dups => self.dups.render(&mut self.gfx, rest, &theme, s, false),
            Kind::Size => self.size.render(&mut self.gfx, rest, &theme, s, false),
            Kind::Inst => self.inst.render(&mut self.gfx, rest, &theme, s, false),
        }
        let state = sidebar::State { collapsed: self.settings.sidebar_collapsed, selected: self.selected, hot: self.side_hot };
        sidebar::draw(&mut self.gfx, &self.side, &theme, s, state, PAGES);
        if let Some(d) = &mut self.dialog {
            d.draw(&mut self.gfx, full, &theme.dialog_style(), s);
        }
        let Some(win) = &mut self.win else { return };
        rt_ui::window::present(&mut win.surface, &self.gfx);
        self.gfx.trim_cache();
        self.sync_menu();
        self.sync_menu_theme(dark);
        self.update_title();
        let tree = self.tree();
        if let Some(win) = &mut self.win {
            win.adapter.update_if_active(|| tree);
        }
    }

    /// The tree for screen readers: the sidebar (10, 11…), the page, the message.
    fn tree(&self) -> accesskit::TreeUpdate {
        use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
        let root = NodeId(0);
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        let mut toggle = Node::new(Role::Button);
        toggle.set_label(tr("Sidebar"));
        toggle.add_action(accesskit::Action::Click);
        nodes.push((NodeId(10), toggle));
        children.push(NodeId(10));
        for (i, p) in PAGES.iter().enumerate() {
            let mut n = Node::new(Role::Button);
            n.set_label(tr(p.label));
            n.set_selected(i == self.selected);
            n.add_action(accesskit::Action::Click);
            nodes.push((NodeId(11 + i as u64), n));
            children.push(NodeId(11 + i as u64));
        }
        let focus = match PAGES[self.selected].kind {
            Kind::Maint => self.maint.accessibility(&mut nodes, &mut children),
            Kind::Startup => self.startup.accessibility(&mut nodes, &mut children),
            Kind::Services => self.services.accessibility(&mut nodes, &mut children),
            Kind::Extensions => self.extensions.accessibility(&mut nodes, &mut children),
            Kind::Registry => self.registry.accessibility(&mut nodes, &mut children),
            Kind::Protect => self.protect.accessibility(&mut nodes, &mut children),
            Kind::Speed => self.speed.accessibility(&mut nodes, &mut children),
            Kind::Custom => self.custom.accessibility(&mut nodes, &mut children),
            Kind::Clean => self.clean.accessibility(&mut nodes, &mut children),
            Kind::Dups => self.dups.accessibility(&mut nodes, &mut children),
            Kind::Size => self.size.accessibility(&mut nodes, &mut children),
            Kind::Inst => self.inst.accessibility(&mut nodes, &mut children),
        };
        let mut focus = focus.unwrap_or(root);
        if let Some(dl) = &self.dialog {
            let mut dn = Node::new(Role::Dialog);
            dn.set_label(dl.title.clone());
            dn.set_children(vec![NodeId(5), NodeId(6)]);
            let mut tn = Node::new(Role::Label);
            tn.set_value(dl.spoken_text());
            let mut ok = Node::new(Role::Button);
            ok.set_label("OK");
            ok.add_action(accesskit::Action::Click);
            nodes.extend([(NodeId(4), dn), (NodeId(5), tn), (NodeId(6), ok)]);
            children.push(NodeId(4));
            focus = NodeId(6);
        }
        let mut r = Node::new(Role::Window);
        r.set_label("rcare");
        r.set_children(children);
        nodes.push((root, r));
        TreeUpdate { nodes, tree: Some(TreeInfo::new(root)), tree_id: TreeId::ROOT, focus }
    }

    // ------------------------------------------------------------ input

    fn key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) {
        if self.dialog.is_some() {
            if Message::closes_on(&event.logical_key) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        let mods: Mods = self.mods.into();
        let Mods { ctrl, shift, alt } = mods;
        let logical = if ctrl || alt {
            use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
            event.key_without_modifiers()
        } else {
            event.logical_key.clone()
        };
        // The window's own chords first, then the page's.
        let name = match &logical {
            WKey::Character(c) if ctrl || alt => c.to_lowercase(),
            WKey::Named(NamedKey::F1) => "f1".into(),
            _ => String::new(),
        };
        if !name.is_empty() {
            if let Some(cmd) = commands::lookup(ctrl, shift, alt, &name) {
                return self.command(el, cmd);
            }
        }
        let cmd = match PAGES[self.selected].kind {
            // Custom Clean has one key of its own: F5 analyses.
            Kind::Custom => matches!(logical, WKey::Named(NamedKey::F5)).then_some(Cmd::CustomAnalyze),
            Kind::Maint => matches!(logical, WKey::Named(NamedKey::F5)).then_some(Cmd::MaintScan),
            Kind::Startup => {
                self.startup.key(&logical);
                None
            }
            Kind::Speed => {
                self.speed.key(&logical);
                None
            }
            Kind::Services => self.services.key(&logical).map(Cmd::Services),
            Kind::Extensions => {
                self.extensions.key(&logical);
                None
            }
            Kind::Registry => {
                self.registry.key(&logical);
                None
            }
            Kind::Protect => self.protect.key(&logical).map(Cmd::Protect),
            Kind::Clean => self.clean.key(ctrl, shift, alt, logical).map(Cmd::from_clean),
            Kind::Dups => self.dups.key(event, mods).map(Cmd::from_dups),
            Kind::Size => self.size.key(event, mods).map(Cmd::from_size),
            Kind::Inst => self.inst.key(event, mods).map(Cmd::from_inst),
        };
        self.after_page();
        if let Some(c) = cmd {
            self.command(el, c);
        }
    }

    fn pointer(&mut self, x: f32, y: f32) {
        let blocked = self.dialog.is_some();
        let hot = if blocked { None } else { sidebar::hit(&self.side, x, y) };
        if hot != self.side_hot {
            self.side_hot = hot;
            self.redraw();
        }
        page!(self, p => p.pointer(x, y, blocked));
        self.after_page();
    }

    fn press(&mut self, el: &ActiveEventLoop, x: f32, y: f32) {
        if let Some(d) = &self.dialog {
            if d.closes_at(x, y) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        match sidebar::hit(&self.side, x, y) {
            Some(Hit::Toggle) => return self.command(el, Cmd::Sidebar),
            Some(Hit::Page(i)) => return self.select_page(i),
            None => {}
        }
        let mods: Mods = self.mods.into();
        match PAGES[self.selected].kind {
            Kind::Startup => {
                self.startup.press();
                self.after_page();
            }
            Kind::Extensions => {
                self.extensions.press();
                self.after_page();
            }
            Kind::Protect => {
                let a = self.protect.press();
                self.after_page();
                if let Some(a) = a {
                    self.command(el, Cmd::Protect(a));
                }
            }
            Kind::Registry => {
                let a = self.registry.press();
                self.after_page();
                if let Some(a) = a {
                    self.command(el, Cmd::Registry(a));
                }
            }
            Kind::Services => {
                let a = self.services.press();
                self.after_page();
                if let Some(a) = a {
                    self.command(el, Cmd::Services(a));
                }
            }
            Kind::Speed => {
                let a = self.speed.press();
                self.after_page();
                if let Some(a) = a {
                    self.command(el, Cmd::Speed(a));
                }
            }
            Kind::Maint => match self.maint.press() {
                Some(maint::Hit::Scan) => self.command(el, Cmd::MaintScan),
                Some(maint::Hit::Row(i)) => self.maint_review(i),
                None => {}
            },
            Kind::Custom => match self.custom.press() {
                Some(custom::Hit::Analyze) => self.command(el, Cmd::CustomAnalyze),
                Some(custom::Hit::Change) => self.command(el, Cmd::CustomFolder),
                Some(custom::Hit::Review(i)) => self.custom_review(i),
                None => {}
            },
            Kind::Clean => {
                let c = self.clean.press(x, y);
                self.after_page();
                if let Some(c) = c {
                    self.command(el, Cmd::from_clean(c));
                }
            }
            Kind::Dups => {
                let out = self.dups.press(&mut self.gfx, x, y, mods);
                self.after_page();
                match out {
                    Some(rdup::page::Out::Command(c)) => self.command(el, Cmd::from_dups(c)),
                    Some(rdup::page::Out::Popup(p)) => self.dups_popup(el, p),
                    None => {}
                }
            }
            Kind::Size => {
                let c = self.size.press(x, y);
                self.after_page();
                if let Some(c) = c {
                    self.command(el, Cmd::from_size(c));
                }
            }
            Kind::Inst => {
                let c = self.inst.press(&mut self.gfx, x, y, mods);
                self.after_page();
                if let Some(c) = c {
                    self.command(el, Cmd::from_inst(c));
                }
            }
        }
    }

    /// The pointer moved with the left button down.
    fn drag(&mut self, x: f32, y: f32) {
        match PAGES[self.selected].kind {
            Kind::Custom
            | Kind::Maint
            | Kind::Startup
            | Kind::Speed
            | Kind::Services
            | Kind::Extensions
            | Kind::Registry
            | Kind::Protect => {}
            Kind::Clean => {
                if self.clean.dragging {
                    self.clean.drag_to(y);
                }
            }
            Kind::Dups => self.dups.drag(&mut self.gfx, x, y),
            Kind::Size => self.size.drag(x, y),
            Kind::Inst => self.inst.drag(&mut self.gfx, x, y),
        }
    }

    fn right_click(&mut self, el: &ActiveEventLoop) {
        let mods: Mods = self.mods.into();
        match PAGES[self.selected].kind {
            Kind::Custom
            | Kind::Maint
            | Kind::Startup
            | Kind::Speed
            | Kind::Services
            | Kind::Extensions
            | Kind::Registry
            | Kind::Protect
            | Kind::Clean => {}
            Kind::Dups => {
                let popup = self.dups.right_click();
                self.after_page();
                if let Some(p) = popup {
                    self.dups_popup(el, p);
                }
            }
            Kind::Size => {
                let popup = self.size.right_click(self.cursor, mods);
                self.after_page();
                if let Some(p) = popup {
                    self.size_popup(el, p);
                }
            }
            Kind::Inst => {
                let popup = self.inst.right_click();
                self.after_page();
                if let Some(p) = popup {
                    self.inst_popup(el, p);
                }
            }
        }
    }

    fn wheel(&mut self, delta: winit::event::MouseScrollDelta) {
        let s = self.scale();
        match PAGES[self.selected].kind {
            Kind::Custom | Kind::Maint => {}
            Kind::Startup => self.startup.wheel(delta),
            Kind::Services => self.services.wheel(delta),
            Kind::Extensions => self.extensions.wheel(delta),
            Kind::Registry => self.registry.wheel(delta, self.cursor),
            Kind::Protect => {}
            Kind::Speed => self.speed.wheel(delta),
            Kind::Clean => self.clean.wheel(&mut self.gfx, s, delta, self.cursor),
            Kind::Dups => self.dups.wheel(delta, self.cursor),
            Kind::Size => self.size.wheel(delta, self.cursor),
            Kind::Inst => self.inst.wheel(delta, self.cursor),
        }
        self.after_page();
    }

    /// When the window should wake up for the page shown, if it should.
    fn animate(&mut self) -> Option<Instant> {
        let wake = match PAGES[self.selected].kind {
            // What the other pages report redraws it: nothing turns here.
            Kind::Custom | Kind::Maint | Kind::Startup | Kind::Services | Kind::Extensions => None,
            Kind::Registry => self.registry.animate(),
            Kind::Protect => self.protect.animate(),
            Kind::Speed => self.speed.animate(),
            Kind::Clean => self.clean.animate().then(|| Instant::now() + Duration::from_millis(40)),
            Kind::Dups => self.dups.animate().map(|d| Instant::now() + d),
            Kind::Size => self.size.animate(),
            Kind::Inst => self.inst.animate().then(|| Instant::now() + Duration::from_millis(40)),
        };
        self.after_page();
        wake
    }
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() || !self.open_window(el) {
            return;
        }
        self.sync_menu();
        // A folder from the command line (or the Explorer menu): Custom Clean analyses it.
        if self.custom_once.is_some() {
            self.selected = pages::index_of("custom");
            self.menu_rebuild = true;
            self.custom_analyze();
        }
        // A resident start stays in the notification area until it is shown.
        if self.tray && self.custom_once.is_none() {
            self.hidden = true;
            return;
        }
        self.ensure_started(self.selected);
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
            UserEvent::Clean(e) => {
                self.clean.event(e);
                self.after_page();
            }
            UserEvent::Dups(e) => {
                self.dups.event(e);
                self.after_page();
            }
            UserEvent::Size(e) => {
                self.size.event(e);
                self.after_page();
            }
            UserEvent::Inst(e) => {
                self.inst.event(e);
                self.after_page();
            }
            UserEvent::Disks(v) => {
                self.disks_running = false;
                self.disks_failed = v.is_none();
                self.volumes = v;
                self.menu_dirty = true;
                self.redraw();
            }
            UserEvent::Win(rt_sys::win::WinMsg::Command(id)) => {
                let cmd = self.win.as_ref().and_then(|w| w.menu.as_ref()).and_then(|m| m.command(id));
                if let Some(c) = cmd {
                    self.command(el, c);
                }
            }
            UserEvent::Win(rt_sys::win::WinMsg::SystemTheme) => {
                let dark = rt_sys::system_dark().unwrap_or(false);
                self.accent = rt_sys::accent().map(Color::rgb);
                if dark != self.system_dark {
                    self.system_dark = dark;
                    self.apply_window_theme();
                }
                self.redraw();
            }
            // Another launch (with folders: from the Explorer menu), or the tray icon: the window shows.
            UserEvent::Win(rt_sys::win::WinMsg::OpenFiles(paths)) => self.open_paths(el, paths),
            UserEvent::Win(rt_sys::win::WinMsg::Tray(rt_sys::win::tray::TrayEvent::Activate)) => self.show(el),
            UserEvent::Win(rt_sys::win::WinMsg::Tray(rt_sys::win::tray::TrayEvent::Menu)) => {
                if let Some(hwnd) = self.win.as_ref().map(|w| w.hwnd) {
                    match rt_sys::win::tray::menu(hwnd, &tr("Open rcare"), &tr("Exit")) {
                        Some(1) => self.show(el),
                        Some(2) => self.quit(el),
                        _ => {}
                    }
                }
            }
            UserEvent::Update(s) => {
                let who = rt_ui::update::Who {
                    app: "rcare",
                    version: env!("CARGO_PKG_VERSION"),
                    hwnd: self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd),
                };
                if rt_ui::update::step(&who, s, self.update_sender()) == rt_ui::update::Next::Quit {
                    self.quit(el);
                }
            }
            UserEvent::A11y(ev) => match ev.window_event {
                accesskit_winit::WindowEvent::ActionRequested(req) if req.action == accesskit::Action::Click => {
                    match req.target_node.0 {
                        6 => self.dialog = None,
                        10 => self.command(el, Cmd::Sidebar),
                        30 if PAGES[self.selected].kind == Kind::Maint => self.command(el, Cmd::MaintScan),
                        id @ 32..=36 if PAGES[self.selected].kind == Kind::Maint => self.maint_review(id as usize - 32),
                        30 if PAGES[self.selected].kind == Kind::Custom => self.command(el, Cmd::CustomAnalyze),
                        31 if PAGES[self.selected].kind == Kind::Custom => self.command(el, Cmd::CustomFolder),
                        id @ 32..=34 if PAGES[self.selected].kind == Kind::Custom => self.custom_review(id as usize - 32),
                        id @ 11..=99 => self.select_page(id as usize - 11),
                        id => {
                            if let Some(i) = id.checked_sub(100) {
                                page!(self, p => p.accessibility_click(i as usize));
                                self.after_page();
                            }
                        }
                    }
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
                self.size.resized();
                self.redraw();
            }
            WindowEvent::ThemeChanged(t) => {
                self.system_dark = t == WinTheme::Dark;
                self.redraw();
            }
            WindowEvent::Focused(false) => self.save_settings(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::DroppedFile(p) => {
                match PAGES[self.selected].kind {
                    Kind::Dups => self.dups.drop_file(p),
                    Kind::Size => self.size.drop_file(p),
                    Kind::Custom
                    | Kind::Maint
                    | Kind::Startup
                    | Kind::Speed
                    | Kind::Services
                    | Kind::Extensions
                    | Kind::Registry
                    | Kind::Protect
                    | Kind::Clean
                    | Kind::Inst => {}
                }
                self.after_page();
            }
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if event.state == ElementState::Pressed {
                    self.key(el, &event);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
                self.cursor = Some((x, y));
                self.drag(x, y);
                self.pointer(x, y);
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                if self.side_hot.take().is_some() {
                    self.redraw();
                }
                page!(self, p => p.pointer_left());
                self.after_page();
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => {
                    if let Some((x, y)) = self.cursor {
                        self.press(el, x, y);
                    }
                }
                ElementState::Released => {
                    page!(self, p => p.release());
                    self.after_page();
                }
            },
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Right, .. } if self.dialog.is_none() => {
                self.right_click(el)
            }
            WindowEvent::MouseWheel { delta, .. } if self.dialog.is_none() => self.wheel(delta),
            WindowEvent::RedrawRequested if !self.hidden => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        match self.animate() {
            Some(at) => el.set_control_flow(ControlFlow::WaitUntil(at)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }
}

/// Whether "Analyze with rcare" is on the Explorer menu of folders, for this rcare.exe.
fn integrated_here() -> bool {
    rt_ui::explorer::folders_here("rcare")
}

/// Adds "Analyze with rcare" to the Explorer menu of folders and drives
/// (current user only), or takes it out; returns the message to show.
/// The folder chosen there opens in Custom Clean, analysed.
fn integrate(on: bool) -> Result<String, String> {
    let texts = rt_ui::explorer::FolderTexts {
        verb: tr("Analyze with rcare"),
        added: tr("“Analyze with rcare” was added to the Explorer context menu of folders and drives."),
        removed: tr("“Analyze with rcare” was removed from the Explorer context menu."),
        failed: tr("Cannot change the Windows registry (error {})."),
    };
    rt_ui::explorer::set_folders("rcare", on, &texts)
}

const USAGE: &str = "rcare [--tray] [--new-window] [folder]

  Windows maintenance in one window. The sidebar chooses the page: System
  Maintenance (the checks in one go), Custom Clean (junk, duplicates and
  large files in one look), the cleaner of
  rclean, the duplicates of rdup, the disk space of rsize and the
  uninstaller of rinst, Speed Up, the Startup Optimizer, and more as the
  other tools join them.

  folder        Open Custom Clean on this folder (or drive) and analyse it.
  --tray        Stay resident in the notification area: closing the window
                hides it, and the icon (or starting rcare again) shows it.
                Exit from the icon's menu or File > Exit. File > Windows
                Integration > Start with Windows starts it this way.
  --new-window  Open a separate window even when rcare is resident.
  --register    Add \"Analyze with rcare\" to the Explorer context menu of
                folders and drives (current user only).
  --unregister  Remove it.

Settings are stored in %APPDATA%\\rcare, or next to the program when a file
named rcare.portable exists there (portable mode).";

pub fn run() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // What an update left behind.
    rt_sys::update::clean_old();
    let help = raw.iter().any(|a| a == "--help" || a == "-h" || a == "/?");
    if help && rt_sys::win::print_to_console(USAGE) {
        return;
    }
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
    // A drive from the Explorer menu comes as `C:"` (see `folder_arg`).
    let folders: Vec<std::path::PathBuf> =
        raw.iter().filter(|a| !a.starts_with("--")).map(|a| std::path::PathBuf::from(rt_sys::win::assoc::folder_arg(a))).collect();
    let tray = raw.iter().any(|a| a == "--tray" || a == "/tray") && !help;
    let new_window = raw.iter().any(|a| a == "--new-window") || help;
    // A resident rcare shows itself (and analyses the folder) instantly.
    if !new_window && rt_sys::win::hand_off(&IDENT, &folders, false, false) {
        return;
    }
    // SAFETY: only changes how this process reports a missing disc or card:
    // with an error code instead of a dialog.
    unsafe {
        use windows_sys::Win32::System::Diagnostics::Debug::{SetErrorMode, SEM_FAILCRITICALERRORS};
        SetErrorMode(SEM_FAILCRITICALERRORS);
    }
    let mut location = storage::detect();
    let store = location.load();
    let settings = Settings::load(&store);
    i18n::set_lang(settings.language.unwrap_or_else(i18n::system_lang));
    let gfx = match Gfx::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rcare: {e}");
            std::process::exit(1);
        }
    };
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("rcare: cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    let admin = rt_sys::win::elevated();
    let mut app = App::new(rclean::targets::all(&rclean::targets::Env::here()), admin, &settings.ticks);
    // Not as administrator: what needs it may still have been measured by the privileged service.
    if !admin {
        if let Some((list, age)) = rt_clean::snapshot::fetch(6 * 3600) {
            app.set_known(list, age);
        }
    }
    let data_dir = location
        .dir
        .clone()
        .or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())))
        .unwrap_or_else(std::env::temp_dir);
    // One sender for each page: what its threads say comes back as a `UserEvent`.
    macro_rules! sender {
        ($variant:ident) => {{
            let proxy = std::sync::Mutex::new(event_loop.create_proxy());
            Arc::new(move |e| {
                if let Ok(p) = proxy.lock() {
                    let _ = p.send_event(UserEvent::$variant(e));
                }
            })
        }};
    }
    let (clean_send, dups_send, size_send, inst_send) = (sender!(Clean), sender!(Dups), sender!(Size), sender!(Inst));
    // The settings of the pages of rdup, rsize and rinst are in their own files.
    let load = |name: &str| rt_core::store::Store::parse(&location.read_text(name).unwrap_or_default());
    let dups_settings = rdup::settings::Settings::load(&load("rdup.ini"));
    let size_settings = rsize::settings::Settings::load(&load("rsize.ini"));
    let inst_settings = rinst::settings::Settings::load(&load("rinst.ini"));
    let selected = pages::index_of(&settings.page);
    let mut shell = Shell {
        maint: MaintPage::default(),
        startup: StartupPage::new(admin),
        services: ServicesPage::new(admin, data_dir.clone()),
        extensions: ExtensionsPage::default(),
        registry: RegistryPage::new(admin, data_dir.clone()),
        protect: ProtectPage::new(admin),
        speed: SpeedPage::new(admin),
        volumes: None,
        disks_running: false,
        disks_failed: false,
        custom: CustomPage::default(),
        large_cache: None,
        clean: CleanPage::new(app, admin, data_dir.clone(), clean_send),
        dups: rdup::page::DupPage::new(&dups_settings, &[], location.file("hashes.bin"), admin, dups_send),
        size: rsize::page::SizePage::new(size_settings, admin, size_send),
        inst: rinst::page::InstPage::new(&inst_settings, admin, data_dir, inst_send),
        dups_settings,
        inst_settings,
        started: [false; PAGES.len()],
        selected,
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
        cursor_icon: CursorIcon::Default,
        side: sidebar::Layout::default(),
        side_hot: None,
        dialog: help.then(|| Message::new("rcare", USAGE)),
        custom_once: folders.into_iter().next(),
        tray,
        hidden: false,
        maximize_on_show: false,
        autostart: rt_sys::autostart::enabled("rcare"),
        explorer_menu: integrated_here(),
    };
    shell.apply_language();
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rcare: {e}");
    }
}
