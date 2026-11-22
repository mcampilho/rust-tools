//! The window of rcare: winit, drawn on the CPU (tiny-skia + ab_glyph) into
//! a softbuffer surface. The sidebar and the menus are the window's; what is
//! to the right of the sidebar is the page the sidebar chose: the cleaner,
//! the duplicates or the disk space, each the page of a small app.

use rcare::commands::{self, Cmd};
use rcare::custom::{self, CustomPage, Part, Status};
use rcare::i18n::{self, tf, tr};
use rcare::pages::{self, Kind, PAGES};
use rcare::settings::Settings;
use rcare::sidebar::{self, Hit};
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
    started: [bool; 5],
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
        for e in [self.custom.fx.take(), self.clean.fx.take(), self.dups.fx.take(), self.size.fx.take(), self.inst.fx.take()] {
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
            Kind::Custom => {}
            Kind::Clean => self.clean.analyse(),
            Kind::Dups => self.dups.load_volumes(),
            Kind::Size => self.size.load_volumes(),
            Kind::Inst => self.inst.refresh(),
        }
        self.after_page();
    }

    fn select_page(&mut self, i: usize) {
        if i < PAGES.len() && i != self.selected {
            self.selected = i;
            self.menu_rebuild = true;
            self.menu_dirty = true;
            self.ensure_started(i);
            self.size.resized();
            self.redraw();
        }
    }

    // ------------------------------------------------------------ Custom Clean

    /// Where Custom Clean looks for duplicates and large files: the folder chosen, or the user's.
    fn custom_folder(&self) -> std::path::PathBuf {
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
                self.clean.command(c, &mut |t, x| d.confirm(t, x));
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
            Cmd::CustomAnalyze => self.custom_analyze(),
            Cmd::CustomFolder => {
                let mut d = self.dialogs();
                if let Some(p) = d.pick_folder(&tr("Choose the folder to look in")) {
                    self.settings.custom_folder = p.display().to_string();
                    self.save_settings();
                }
            }
            Cmd::Sidebar => {
                self.settings.sidebar_collapsed = !self.settings.sidebar_collapsed;
                self.size.resized();
            }
            Cmd::RestartAdmin => {
                self.save_settings();
                let args = match PAGES[self.selected].kind {
                    Kind::Clean | Kind::Custom => String::new(),
                    Kind::Dups => self.dups.restart_args(),
                    Kind::Size => self.size.restart_args(),
                    Kind::Inst => String::new(),
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
            Cmd::RestartAdmin => match PAGES[self.selected].kind {
                Kind::Clean | Kind::Custom => self.clean.enabled(CleanCmd::RestartAdmin),
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
        el.exit();
    }

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let (w, h, maximized) = self.settings.window.unwrap_or((1180.0, 720.0, false));
        let attrs = Window::default_attributes()
            .with_title("rcare")
            .with_inner_size(LogicalSize::new(w.max(760.0), h.max(480.0)))
            .with_min_inner_size(LogicalSize::new(760.0, 480.0))
            .with_maximized(maximized)
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
        let theme = rclean::ui::theme::Theme::new(dark, self.accent);
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
        match PAGES[self.selected].kind {
            Kind::Custom => self.custom.render(&mut self.gfx, rest, &theme, s),
            Kind::Clean => self.clean.render(&mut self.gfx, rest, &theme, s, false),
            Kind::Dups => self.dups.render(&mut self.gfx, rest, &rdup::ui::theme::Theme::new(dark, self.accent), s, false),
            Kind::Size => self.size.render(&mut self.gfx, rest, &rsize::ui::theme::Theme::new(dark, self.accent), s, false),
            Kind::Inst => self.inst.render(&mut self.gfx, rest, &rinst::ui::theme::Theme::new(dark, self.accent), s, false),
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
            Kind::Custom => {}
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
            Kind::Custom | Kind::Clean => {}
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
            Kind::Custom => {}
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
            Kind::Custom => None,
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
        self.ensure_started(self.selected);
        if let Some(w) = &self.win {
            if self.settings.window.is_some_and(|(_, _, max)| max) {
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
            UserEvent::Win(_) => {}
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
            WindowEvent::CloseRequested => self.quit(el),
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
                    Kind::Custom | Kind::Clean | Kind::Inst => {}
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
            WindowEvent::RedrawRequested => self.render(),
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

const USAGE: &str = "rcare

  Windows maintenance in one window. The sidebar chooses the page: Custom
  Clean (junk, duplicates and large files in one look), the cleaner of rclean, the duplicates of rdup, the disk space of rsize and
  the uninstaller of rinst, and more as the other tools join them.

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
        custom: CustomPage::default(),
        large_cache: None,
        clean: CleanPage::new(app, admin, data_dir.clone(), clean_send),
        dups: rdup::page::DupPage::new(&dups_settings, &[], location.file("hashes.bin"), admin, dups_send),
        size: rsize::page::SizePage::new(size_settings, admin, size_send),
        inst: rinst::page::InstPage::new(&inst_settings, admin, data_dir, inst_send),
        dups_settings,
        inst_settings,
        started: [false; 5],
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
    };
    shell.apply_language();
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rcare: {e}");
    }
}
