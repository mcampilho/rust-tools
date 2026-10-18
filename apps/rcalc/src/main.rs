//! rcalc: a portable desktop calculator inspired by Windows Calculator.
//!
//! The window is driven by winit; every frame is rendered on the CPU
//! (tiny-skia + ab_glyph) into a softbuffer surface, so the program needs no
//! GPU driver, graphics runtime or bundled assets. Frames are only produced
//! in response to input or window events: the app is idle otherwise.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rcalc::app::{Action, App, KeyIn, Mods, Overlay, ThemePref};
use rcalc::{platform, storage, ui};
use std::num::NonZeroU32;
use std::rc::Rc;
use ui::gfx::{Color, Gfx};
use ui::theme::{Theme, DEFAULT_ACCENT};
use ui::{Input, Role, ScrollArea, WId, Widget};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{Icon as WinIcon, Theme as WinTheme, Window, WindowId, WindowLevel};

enum UserEvent {
    A11y(accesskit_winit::Event),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(e: accesskit_winit::Event) -> Self {
        UserEvent::A11y(e)
    }
}

struct Win {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    adapter: accesskit_winit::Adapter,
}

struct Shell {
    app: App,
    gfx: Gfx,
    input: Input,
    widgets: Vec<Widget>,
    scrolls: Vec<ScrollArea>,
    win: Option<Win>,
    proxy: EventLoopProxy<UserEvent>,
    mods: ModifiersState,
    cursor: Option<(f32, f32)>,
    system_dark: bool,
    accent: Color,
    on_top: bool,
    restore_size: Option<LogicalSize<f64>>,
    /// Move keyboard focus into a newly opened overlay after the next frame.
    focus_overlay: bool,
    focus_before_overlay: Option<WId>,
    theme_pref: ThemePref,
}

const DEFAULT_SIZE: (f64, f64) = (340.0, 560.0);
const COMPACT_SIZE: (f64, f64) = (320.0, 400.0);

impl Shell {
    fn theme(&self) -> Theme {
        let dark = match self.app.theme_pref {
            ThemePref::Dark => true,
            ThemePref::Light => false,
            ThemePref::System => self.system_dark,
        };
        Theme::new(dark, self.accent)
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.win {
            w.window.request_redraw();
        }
    }

    fn top_layer(&self) -> u8 {
        self.widgets.iter().map(|w| w.layer).max().unwrap_or(0)
    }

    fn hit(&self, x: f32, y: f32) -> Option<WId> {
        let layer = self.top_layer();
        self.widgets
            .iter()
            .rev()
            .filter(|w| w.layer == layer && !matches!(w.role, Role::Label | Role::Display))
            .find(|w| w.rect.contains(x, y))
            .filter(|w| w.clickable())
            .map(|w| w.id)
    }

    fn widget(&self, id: WId) -> Option<&Widget> {
        self.widgets.iter().find(|w| w.id == id)
    }

    fn update_hot(&mut self) -> bool {
        let hot = self.cursor.and_then(|(x, y)| self.hit(x, y));
        let changed = hot != self.input.hot;
        self.input.hot = hot;
        changed
    }

    fn dispatch(&mut self, action: Action) {
        let before = self.app.overlay;
        self.app.apply(&action);
        let after = self.app.overlay;
        if before == Overlay::None && after != Overlay::None {
            self.focus_before_overlay = self.input.focus;
            self.focus_overlay = self.input.focus_visible;
        } else if before != Overlay::None && after == Overlay::None {
            if let Some(f) = self.focus_before_overlay.take() {
                self.input.focus = Some(f);
            }
        } else if before != after {
            self.focus_overlay = self.input.focus_visible;
        }
        if matches!(action, Action::Navigate(_)) {
            self.input.focus = None;
        }
        self.sync_window();
        self.request_redraw();
    }

    /// Applies window-level side effects of app state (keep on top, theme).
    fn sync_window(&mut self) {
        let Some(w) = &self.win else { return };
        if self.app.keep_on_top != self.on_top {
            self.on_top = self.app.keep_on_top;
            if self.on_top {
                let scale = w.window.scale_factor();
                self.restore_size = Some(w.window.inner_size().to_logical(scale));
                w.window.set_window_level(WindowLevel::AlwaysOnTop);
                w.window.set_min_inner_size(Some(LogicalSize::new(COMPACT_SIZE.0, 300.0)));
                let _ = w.window.request_inner_size(LogicalSize::new(COMPACT_SIZE.0, COMPACT_SIZE.1));
            } else {
                w.window.set_window_level(WindowLevel::Normal);
                w.window
                    .set_min_inner_size(Some(LogicalSize::new(ui::view::MIN_WIDTH, ui::view::MIN_HEIGHT)));
                if let Some(s) = self.restore_size.take() {
                    let _ = w.window.request_inner_size(s);
                }
            }
        }
        if self.app.theme_pref != self.theme_pref {
            self.theme_pref = self.app.theme_pref;
            w.window.set_theme(match self.theme_pref {
                ThemePref::Dark => Some(WinTheme::Dark),
                ThemePref::Light => Some(WinTheme::Light),
                ThemePref::System => None,
            });
        }
    }

    fn focusable_ids(&self) -> Vec<WId> {
        let layer = self.top_layer();
        self.widgets
            .iter()
            .filter(|w| w.layer == layer && w.focusable())
            .map(|w| w.id)
            .collect()
    }

    fn move_focus(&mut self, delta: i32) {
        let ids = self.focusable_ids();
        if ids.is_empty() {
            return;
        }
        let cur = self.input.focus.and_then(|f| ids.iter().position(|i| *i == f));
        let n = ids.len() as i32;
        let next = match cur {
            Some(i) if self.input.focus_visible => (i as i32 + delta).rem_euclid(n),
            Some(i) => i as i32,
            None if delta > 0 => 0,
            None => n - 1,
        };
        self.input.focus = Some(ids[next as usize]);
        self.input.focus_visible = true;
        self.request_redraw();
    }

    fn key(&mut self, event: &winit::event::KeyEvent) {
        let ctrl = self.mods.control_key();
        let key = if ctrl {
            use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
            event.key_without_modifiers()
        } else {
            event.logical_key.clone()
        };
        let kin = match &key {
            WKey::Named(n) => match n {
                NamedKey::Enter => KeyIn::Enter,
                NamedKey::Escape => KeyIn::Escape,
                NamedKey::Backspace => KeyIn::Backspace,
                NamedKey::Delete => KeyIn::Delete,
                NamedKey::Tab => KeyIn::Tab,
                NamedKey::Space => KeyIn::Space,
                NamedKey::ArrowUp => KeyIn::Up,
                NamedKey::ArrowDown => KeyIn::Down,
                NamedKey::ArrowLeft => KeyIn::Left,
                NamedKey::ArrowRight => KeyIn::Right,
                NamedKey::PageUp => KeyIn::PageUp,
                NamedKey::PageDown => KeyIn::PageDown,
                NamedKey::F1 => KeyIn::F(1),
                NamedKey::F2 => KeyIn::F(2),
                NamedKey::F3 => KeyIn::F(3),
                NamedKey::F4 => KeyIn::F(4),
                NamedKey::F5 => KeyIn::F(5),
                NamedKey::F6 => KeyIn::F(6),
                NamedKey::F7 => KeyIn::F(7),
                NamedKey::F8 => KeyIn::F(8),
                NamedKey::F9 => KeyIn::F(9),
                NamedKey::F12 => KeyIn::F(12),
                _ => return,
            },
            WKey::Character(s) => match s.chars().next() {
                Some(' ') => KeyIn::Space,
                Some(c) => KeyIn::Char(c),
                None => return,
            },
            _ => return,
        };
        let mods = Mods {
            ctrl,
            shift: self.mods.shift_key(),
            alt: self.mods.alt_key(),
        };
        let focused = self
            .input
            .focus
            .filter(|_| self.input.focus_visible)
            .and_then(|f| self.widget(f).cloned());
        match kin {
            KeyIn::Tab => return self.move_focus(if mods.shift { -1 } else { 1 }),
            KeyIn::Escape if self.app.overlay != Overlay::None => return self.dispatch(Action::CloseOverlay),
            KeyIn::Space => {
                if let Some(w) = focused {
                    self.dispatch(w.action);
                }
                return;
            }
            KeyIn::Enter => {
                if let Some(w) = &focused {
                    if !matches!(w.action, Action::Key(_)) || self.app.overlay != Overlay::None {
                        return self.dispatch(w.action.clone());
                    }
                }
            }
            KeyIn::Up | KeyIn::Down | KeyIn::Left | KeyIn::Right => {
                let delta = if matches!(kin, KeyIn::Up | KeyIn::Left) { -1 } else { 1 };
                if let Some(Widget {
                    role: Role::ComboBox,
                    action: Action::Open(Overlay::Combo(id)),
                    ..
                }) = &focused
                {
                    if matches!(kin, KeyIn::Up | KeyIn::Down) {
                        self.app.combo_step(*id, delta as i64);
                        return self.request_redraw();
                    }
                }
                if let Overlay::Calendar(_) = self.app.overlay {
                    let step = match kin {
                        KeyIn::Up => -7,
                        KeyIn::Down => 7,
                        _ => delta,
                    };
                    return self.move_focus(step);
                }
                if self.app.overlay != Overlay::None || focused.is_some() {
                    return self.move_focus(delta);
                }
                return;
            }
            KeyIn::PageUp | KeyIn::PageDown => {
                if let Overlay::Calendar(_) = self.app.overlay {
                    return self.dispatch(Action::CalMonth(if kin == KeyIn::PageUp { -1 } else { 1 }));
                }
                return;
            }
            _ => {}
        }
        if let Some(action) = self.app.shortcut(&kin, mods) {
            self.input.focus_visible = false;
            self.dispatch(action);
        }
    }

    fn render(&mut self) {
        let theme = self.theme();
        let Some(win) = &mut self.win else { return };
        let size = win.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if win.surface.resize(w, h).is_err() {
            return;
        }
        let scale = win.window.scale_factor() as f32;
        self.gfx.resize(w.get(), h.get());
        let mut u = ui::Ui::new(&mut self.gfx, theme, scale, &self.input);
        ui::view::build(&self.app, &mut u);
        let ui::Ui { widgets, scrolls, .. } = u;
        self.widgets = widgets;
        self.scrolls = scrolls;
        if let Ok(mut buf) = win.surface.buffer_mut() {
            self.gfx.copy_to(&mut buf);
            let _ = buf.present();
        }
        self.gfx.trim_cache();
        self.after_frame();
    }

    /// Bookkeeping that depends on the freshly laid-out widgets.
    fn after_frame(&mut self) {
        for s in &self.scrolls {
            if self.app.scroll(s.id) > s.max {
                self.app.set_scroll(s.id, s.max);
            }
        }
        if self.focus_overlay {
            self.focus_overlay = false;
            let layer = self.top_layer();
            let candidates = || self.widgets.iter().filter(|w| w.layer == layer && w.focusable());
            let pick = candidates()
                .find(|w| w.checked == Some(true))
                .or_else(|| candidates().next())
                .map(|w| w.id);
            if pick.is_some() {
                self.input.focus = pick;
                self.input.focus_visible = true;
                self.request_redraw();
            }
        }
        if self.input.focus.is_some_and(|f| self.widget(f).is_none()) {
            self.input.focus = None;
        }
        if self.update_hot() {
            self.request_redraw();
        }
        let tree = self.tree();
        if let Some(win) = &mut self.win {
            win.adapter.update_if_active(|| tree);
        }
    }

    fn tree(&self) -> accesskit::TreeUpdate {
        use accesskit::{Node, NodeId, Rect as ARect, Role as ARole, Toggled, TreeId, TreeInfo, TreeUpdate};
        let root = NodeId(0);
        let layer = self.top_layer();
        let mut nodes = Vec::with_capacity(self.widgets.len() + 1);
        let mut children = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for w in &self.widgets {
            if w.layer != layer || w.role == Role::Scrim || w.label.is_empty() || !seen.insert(w.id) {
                continue;
            }
            let role = match w.role {
                Role::Button | Role::Toggle => ARole::Button,
                Role::Radio => ARole::RadioButton,
                Role::Tab => ARole::Tab,
                Role::ListItem => ARole::ListItem,
                Role::MenuItem => ARole::MenuItem,
                Role::ComboBox => ARole::ComboBox,
                Role::Field => ARole::TextInput,
                Role::Label | Role::Display | Role::Scrim => ARole::Label,
            };
            let mut n = Node::new(role);
            n.set_label(w.label.clone());
            n.set_bounds(ARect {
                x0: w.rect.x as f64,
                y0: w.rect.y as f64,
                x1: w.rect.right() as f64,
                y1: w.rect.bottom() as f64,
            });
            if let Some(v) = &w.value {
                n.set_value(v.clone());
            } else if matches!(w.role, Role::Label | Role::Display) {
                // Static text exposes its content as the value.
                n.set_value(w.label.clone());
            }
            if w.focusable() {
                n.add_action(accesskit::Action::Focus);
                n.add_action(accesskit::Action::Click);
            }
            if !w.enabled {
                n.set_disabled();
            }
            match (w.role, w.checked) {
                (Role::Toggle, Some(c)) => n.set_toggled(if c { Toggled::True } else { Toggled::False }),
                (Role::Radio | Role::Tab | Role::ListItem | Role::MenuItem | Role::Field, Some(c)) => n.set_selected(c),
                _ => {}
            }
            if w.role == Role::Display {
                n.set_live(accesskit::Live::Polite);
            }
            children.push(NodeId(w.id));
            nodes.push((NodeId(w.id), n));
        }
        let mut r = Node::new(ARole::Window);
        r.set_label("Calculator");
        r.set_children(children);
        nodes.push((root, r));
        let focus = self.input.focus.filter(|f| seen.contains(f)).map(NodeId).unwrap_or(root);
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus,
        }
    }

    fn save(&mut self) {
        if let Some(w) = &self.win {
            if !self.on_top && !w.window.is_minimized().unwrap_or(false) {
                let s: LogicalSize<f64> = w.window.inner_size().to_logical(w.window.scale_factor());
                self.app.window_size = Some((s.width, s.height));
            }
        }
        self.app.save();
    }
}

/// Window icon drawn at startup (original artwork).
fn app_icon(accent: Color) -> Option<WinIcon> {
    let n = 32u32;
    let mut pm = tiny_skia::Pixmap::new(n, n)?;
    let mut paint = tiny_skia::Paint {
        anti_alias: true,
        ..Default::default()
    };
    let mut fill = |pm: &mut tiny_skia::Pixmap, x: f32, y: f32, w: f32, h: f32, c: (u8, u8, u8)| {
        paint.set_color_rgba8(c.0, c.1, c.2, 255);
        if let Some(r) = tiny_skia::Rect::from_xywh(x, y, w, h) {
            pm.fill_rect(r, &paint, tiny_skia::Transform::identity(), None);
        }
    };
    fill(&mut pm, 4.0, 1.0, 24.0, 30.0, (0x2B, 0x2B, 0x2B));
    fill(&mut pm, 7.0, 4.0, 18.0, 7.0, (0xE8, 0xE8, 0xE8));
    for row in 0..3 {
        for col in 0..3 {
            let c = if row == 2 && col == 2 {
                (accent.r, accent.g, accent.b)
            } else {
                (0x9A, 0x9A, 0x9A)
            };
            fill(&mut pm, 7.0 + col as f32 * 6.5, 14.0 + row as f32 * 5.5, 5.0, 4.0, c);
        }
    }
    // All pixels are opaque or fully transparent, so premultiplied == straight alpha.
    WinIcon::from_rgba(pm.data().to_vec(), n, n).ok()
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() {
            return;
        }
        let (w, h) = self.app.window_size.unwrap_or(DEFAULT_SIZE);
        let attrs = Window::default_attributes()
            .with_title("Calculator")
            .with_inner_size(LogicalSize::new(w.max(ui::view::MIN_WIDTH), h.max(ui::view::MIN_HEIGHT)))
            .with_min_inner_size(LogicalSize::new(ui::view::MIN_WIDTH, ui::view::MIN_HEIGHT))
            .with_window_icon(app_icon(self.accent))
            .with_theme(match self.app.theme_pref {
                ThemePref::Dark => Some(WinTheme::Dark),
                ThemePref::Light => Some(WinTheme::Light),
                ThemePref::System => None,
            })
            .with_visible(false);
        let window = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("rcalc: cannot create window: {e}");
                el.exit();
                return;
            }
        };
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(el, &window, self.proxy.clone());
        if let Some(t) = window.theme() {
            self.system_dark = t == WinTheme::Dark;
        }
        let surface = softbuffer::Context::new(window.clone()).and_then(|ctx| softbuffer::Surface::new(&ctx, window.clone()));
        let surface = match surface {
            Ok(s) => s,
            Err(e) => {
                eprintln!("rcalc: cannot create drawing surface: {e}");
                el.exit();
                return;
            }
        };
        window.set_visible(true);
        self.win = Some(Win { window, surface, adapter });
        self.theme_pref = self.app.theme_pref;
        self.request_redraw();
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, event: UserEvent) {
        let UserEvent::A11y(ev) = event;
        match ev.window_event {
            accesskit_winit::WindowEvent::InitialTreeRequested => {
                let tree = self.tree();
                if let Some(w) = &mut self.win {
                    w.adapter.update_if_active(|| tree);
                }
                self.request_redraw();
            }
            accesskit_winit::WindowEvent::ActionRequested(req) => {
                let id = req.target_node.0;
                let Some(w) = self.widget(id).cloned() else { return };
                match req.action {
                    accesskit::Action::Click => self.dispatch(w.action),
                    accesskit::Action::Focus => {
                        self.input.focus = Some(id);
                        self.input.focus_visible = true;
                        self.request_redraw();
                    }
                    _ => {}
                }
            }
            accesskit_winit::WindowEvent::AccessibilityDeactivated => {}
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(w) = &mut self.win {
            w.adapter.process_event(&w.window, &event);
        }
        match event {
            WindowEvent::CloseRequested => {
                self.save();
                el.exit();
            }
            WindowEvent::Focused(false) => self.save(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.request_redraw(),
            WindowEvent::ThemeChanged(t) => {
                self.system_dark = t == WinTheme::Dark;
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::CursorMoved {
                position: PhysicalPosition { x, y },
                ..
            } => {
                self.cursor = Some((x as f32, y as f32));
                if self.update_hot() {
                    self.request_redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                if self.update_hot() {
                    self.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                self.update_hot();
                match state {
                    ElementState::Pressed => {
                        self.input.active = self.input.hot;
                        self.input.focus_visible = false;
                        if let Some(h) = self.input.hot {
                            if self.widget(h).is_some_and(|w| w.focusable()) {
                                self.input.focus = Some(h);
                            }
                        }
                        self.request_redraw();
                    }
                    ElementState::Released => {
                        let active = self.input.active.take();
                        if let (Some(a), Some(h)) = (active, self.input.hot) {
                            if a == h {
                                if let Some(w) = self.widget(a).cloned() {
                                    self.dispatch(w.action);
                                }
                            }
                        }
                        self.request_redraw();
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let Some((x, y)) = self.cursor else { return };
                let scale = self.win.as_ref().map_or(1.0, |w| w.window.scale_factor() as f32);
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, l) => -l * 48.0 * scale,
                    MouseScrollDelta::PixelDelta(p) => -p.y as f32,
                };
                if let Some(area) = self.scrolls.iter().rev().find(|s| s.rect.contains(x, y)) {
                    let v = (self.app.scroll(area.id) + dy).clamp(0.0, area.max);
                    self.app.set_scroll(area.id, v);
                    self.request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => {
                if event.state == ElementState::Pressed {
                    self.key(&event);
                }
            }
            WindowEvent::RedrawRequested => self.render(),
            _ => {}
        }
    }
}

fn main() {
    let mut location = storage::Location::detect();
    let store = location.load();
    let mut app = App::new(location);
    app.load(&store);

    let gfx = match Gfx::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rcalc: {e}");
            std::process::exit(1);
        }
    };
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("rcalc: cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let accent = platform::accent().map(Color::rgb).unwrap_or(DEFAULT_ACCENT);
    let mut shell = Shell {
        theme_pref: app.theme_pref,
        app,
        gfx,
        input: Input::default(),
        widgets: Vec::new(),
        scrolls: Vec::new(),
        win: None,
        proxy: event_loop.create_proxy(),
        mods: ModifiersState::empty(),
        cursor: None,
        system_dark: platform::system_dark().unwrap_or(false),
        accent,
        on_top: false,
        restore_size: None,
        focus_overlay: false,
        focus_before_overlay: None,
    };
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rcalc: {e}");
    }
}
