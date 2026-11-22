//! The window of rcare: winit, drawn on the CPU (tiny-skia + ab_glyph) into
//! a softbuffer surface. The sidebar and the menus are the window's; what is
//! to the right of the sidebar is the page the sidebar chose.

use rcare::commands::{self, Cmd};
use rcare::i18n::{self, tf, tr};
use rcare::pages::{self, PAGES};
use rcare::settings::Settings;
use rcare::sidebar::{self, Hit};
use rcare::storage;
use rclean::app::App;
use rclean::commands::Command as CleanCmd;
use rclean::page::{shortcuts_text, CleanPage};
use rclean::ui::gfx::{Color, Gfx, Rect};
use rclean::ui::theme::Theme;
use rt_ui::Message;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
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
    /// What a thread of the cleaning page reports.
    Clean(rclean::page::Event),
    Win(rt_sys::win::WinMsg),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(e: accesskit_winit::Event) -> Self {
        UserEvent::A11y(e)
    }
}

const IDENT: rt_sys::win::Ident = rt_sys::win::Ident { name: "rcare", copydata: 0x5243_4152 }; // "RCAR"

struct Win {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    adapter: accesskit_winit::Adapter,
    hwnd: rt_sys::win::HWND,
    menu: Option<rt_sys::win::menu::MenuBar<Cmd>>,
}

struct Shell {
    /// The cleaning page (the first of the sidebar).
    clean: CleanPage,
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
    }

    /// Takes what the page asked of the window, and the pointer's shape.
    fn after_page(&mut self) {
        let fx = self.clean.fx.take();
        if fx.menu_dirty {
            self.menu_dirty = true;
        }
        if let Some((title, text)) = fx.info {
            self.clean.dragging = false;
            self.dialog = Some(Message::new(&title, &text));
        }
        if fx.redraw {
            self.redraw();
        }
        self.sync_cursor();
    }

    fn sync_cursor(&mut self) {
        let icon = if self.side_hot.is_some() { CursorIcon::Pointer } else { self.clean.cursor_icon };
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(w) = &self.win {
                w.window.set_cursor(icon);
            }
        }
    }

    /// Asks before doing something, through the Windows dialog.
    fn confirm_with(win: &Option<Win>, title: &str, text: &str) -> bool {
        use rt_sys::win::dialogs::{message, Answer, Buttons, Icon};
        let hwnd = win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd);
        message(hwnd, title, text, Buttons::OkCancel, Icon::Warning) == Answer::Ok
    }

    fn info(&mut self, title: &str, text: &str) {
        self.dialog = Some(Message::new(title, text));
        self.clean.dragging = false;
        self.redraw();
    }

    fn update_title(&self) {
        let Some(w) = &self.win else { return };
        let title = if self.clean.admin { format!("rcare ({})", tr("Administrator")) } else { "rcare".to_string() };
        if w.window.title() != title {
            w.window.set_title(&title);
        }
    }

    // ------------------------------------------------------------ pages

    fn select_page(&mut self, i: usize) {
        if i < PAGES.len() && i != self.selected {
            self.selected = i;
            self.menu_rebuild = true;
            self.menu_dirty = true;
            self.redraw();
        }
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self, el: &ActiveEventLoop, cmd: Cmd) {
        if !self.enabled(cmd) {
            return;
        }
        match cmd {
            Cmd::Clean(c) => {
                let win = &self.win;
                self.clean.command(c, &mut |t, x| Shell::confirm_with(win, t, x));
                self.after_page();
            }
            Cmd::Sidebar => self.settings.sidebar_collapsed = !self.settings.sidebar_collapsed,
            Cmd::RestartAdmin => {
                self.save_settings();
                if rt_sys::win::restart_elevated("") {
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
                let text = format!("Ctrl+B\t{}\n{}", tr("Sidebar"), shortcuts_text());
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
            Cmd::RestartAdmin => self.clean.enabled(CleanCmd::RestartAdmin),
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
        let mut rest = full;
        let side = rest.take_left(sidebar::width(self.settings.sidebar_collapsed, s));
        self.side = sidebar::layout(side, PAGES.len(), s);
        self.clean.render(&mut self.gfx, rest, &theme, s, false);
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
        let mut focus = self.clean.accessibility(&mut nodes, &mut children).unwrap_or(root);
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
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        let alt = self.mods.alt_key();
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
        let cmd = self.clean.key(ctrl, shift, alt, logical);
        self.after_page();
        if let Some(c) = cmd {
            self.command(el, Cmd::from_clean(c));
        }
    }

    fn pointer(&mut self, x: f32, y: f32) {
        let hot = if self.dialog.is_some() { None } else { sidebar::hit(&self.side, x, y) };
        if hot != self.side_hot {
            self.side_hot = hot;
            self.redraw();
        }
        self.clean.pointer(x, y, self.dialog.is_some());
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
        let cmd = self.clean.press(x, y);
        self.after_page();
        if let Some(c) = cmd {
            self.command(el, Cmd::from_clean(c));
        }
    }
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() || !self.open_window(el) {
            return;
        }
        self.sync_menu();
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
                        id @ 11..=99 => self.select_page(id as usize - 11),
                        id => {
                            if let Some(i) = id.checked_sub(100) {
                                self.clean.accessibility_click(i as usize);
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
                if self.clean.dragging {
                    self.clean.drag_to(y);
                }
                self.pointer(x, y);
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                if self.side_hot.take().is_some() {
                    self.redraw();
                }
                self.clean.pointer_left();
                self.after_page();
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => {
                    if let Some((x, y)) = self.cursor {
                        self.press(el, x, y);
                    }
                }
                ElementState::Released => self.clean.release(),
            },
            WindowEvent::MouseWheel { delta, .. } if self.dialog.is_none() => {
                let s = self.scale();
                self.clean.wheel(&mut self.gfx, s, delta, self.cursor);
                self.after_page();
            }
            WindowEvent::RedrawRequested => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // While working, the arc turns, so it shows it is busy.
        if self.clean.animate() {
            const FRAME: std::time::Duration = std::time::Duration::from_millis(40);
            self.after_page();
            el.set_control_flow(ControlFlow::WaitUntil(std::time::Instant::now() + FRAME));
        } else {
            el.set_control_flow(ControlFlow::Wait);
        }
    }
}

const USAGE: &str = "rcare

  Windows maintenance in one window. The sidebar chooses the page: the
  cleaner of rclean, and more as the other tools join it.

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
    let sender = {
        let proxy = std::sync::Mutex::new(event_loop.create_proxy());
        Arc::new(move |e| {
            if let Ok(p) = proxy.lock() {
                let _ = p.send_event(UserEvent::Clean(e));
            }
        })
    };
    let selected = pages::index_of(&settings.page);
    let mut shell = Shell {
        clean: CleanPage::new(app, admin, data_dir, sender),
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
    // A first look at once: how much each holds.
    shell.clean.analyse();
    shell.after_page();
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rcare: {e}");
    }
}
