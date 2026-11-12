//! Menus: the native menu bar (Windows) or the menus drawn in the window
//! (Linux, through `rt_ui::menus`), the context menu, and their theme.

use super::*;

impl Shell {
    /// The context menu of the selection, at the pointer.
    #[cfg(windows)]
    pub(super) fn context_menu(&mut self, el: &ActiveEventLoop) {
        let Some(hwnd) = self.win.as_ref().map(|w| w.hwnd) else { return };
        let mut items = Vec::new();
        for m in commands::item_menu() {
            items.push(match m {
                rt_core::menu::M::Item(c) => Some((c, self.enabled(c))),
                _ => None,
            });
        }
        if let Some(c) = rt_sys::win::menu::popup_at_cursor(hwnd, &items) {
            self.command(el, c);
        }
    }

    #[cfg(not(windows))]
    pub(super) fn context_menu(&mut self, _el: &ActiveEventLoop) {
        let me = &*self;
        let items = rsize::ui::menu::build(commands::item_menu(), &|c| me.checked(c), &|c| me.enabled(c));
        let at = self.cursor.unwrap_or((0.0, 0.0));
        self.menu.open(None, items, at, false);
        self.redraw();
    }

    /// Does what the drawn menus asked; true when they took the input.
    #[cfg(not(windows))]
    pub(super) fn menu_step(&mut self, el: &ActiveEventLoop, step: rt_ui::menus::Step<Command>) -> bool {
        use rt_ui::menus::Step;
        match step {
            Step::NotMine => return false,
            Step::Used => {}
            Step::Redraw => self.redraw(),
            Step::Open { index, keyboard } => {
                let Some((_, tree)) = commands::menu_tree().into_iter().nth(index) else { return true };
                let me = &*self;
                let items = rsize::ui::menu::build(tree, &|c| me.checked(c), &|c| me.enabled(c));
                rt_ui::menus::open(&mut self.menu, index, items, keyboard);
                self.redraw();
            }
            Step::Run(c) => self.command(el, c),
        }
        true
    }

    /// Keyboard for the menus. Returns true when it used the key.
    #[cfg(not(windows))]
    pub(super) fn menu_key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) -> bool {
        use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
        let step = rt_ui::menus::key(&mut self.menu, self.mods, &event.key_without_modifiers());
        self.menu_step(el, step)
    }

    #[cfg(not(windows))]
    pub(super) fn menu_pointer(&mut self, el: &ActiveEventLoop, x: f32, y: f32) -> bool {
        let (redraw, step) = rt_ui::menus::pointer(&mut self.menu, x, y);
        if redraw {
            self.redraw();
        }
        self.menu_step(el, step)
    }

    #[cfg(not(windows))]
    pub(super) fn menu_press(&mut self, el: &ActiveEventLoop, x: f32, y: f32) -> bool {
        let step = rt_ui::menus::press(&mut self.menu, x, y);
        self.menu_step(el, step)
    }

    #[cfg(windows)]
    pub(super) fn sync_menu(&mut self) {
        let Some(win) = &mut self.win else { return };
        if !self.menu_dirty && win.menu.is_some() {
            return;
        }
        self.menu_dirty = false;
        if win.menu.is_none() || self.menu_rebuild {
            self.menu_rebuild = false;
            let tree = commands::menu_tree();
            rt_sys::win::set_mnemonics(&rt_ui::window::mnemonics_of(&tree));
            win.menu = Some(rt_sys::win::menu::MenuBar::install(win.hwnd, tree));
        }
        let me = &*self;
        if let Some(m) = me.win.as_ref().and_then(|w| w.menu.as_ref()) {
            m.sync(|c| me.checked(c), |c| me.enabled(c));
        }
    }

    #[cfg(not(windows))]
    pub(super) fn sync_menu(&mut self) {
        self.menu_dirty = false;
        if !self.menu.titles.is_empty() && !self.menu_rebuild {
            return;
        }
        self.menu_rebuild = false;
        self.menu.titles = commands::menu_tree().iter().map(|(title, _)| rsize::ui::menu::Title::new(&tr(title))).collect();
        self.redraw();
    }

    pub(super) fn apply_window_theme(&self) {
        if let Some(w) = &self.win {
            rt_ui::window::apply_theme(&w.window, self.dark());
        }
    }

    pub(super) fn sync_menu_theme(&mut self, dark: bool) {
        if self.menu_dark == Some(dark) {
            return;
        }
        self.menu_dark = Some(dark);
        #[cfg(windows)]
        if let Some(w) = &self.win {
            rt_ui::window::menu_theme(w.hwnd, dark);
        }
    }
}
