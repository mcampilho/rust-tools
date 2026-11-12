//! Menus: the native menu bar and pop-up menus (Windows) or the menus drawn
//! in the window (Linux); the status bar's, the editor's and the tabs' menus;
//! their theme.

use super::*;

impl Shell {
    /// Pops up a native menu at the mouse position; runs the chosen command.
    #[cfg(windows)]
    pub(super) fn popup(&mut self, el: &ActiveEventLoop, items: Vec<M>) {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut pt = POINT { x: 0, y: 0 };
        // SAFETY: writes the cursor position into a local POINT.
        unsafe { GetCursorPos(&mut pt) };
        let app = &self.app;
        let chosen = win::menu::popup(self.host.hwnd, items, pt.x, pt.y, |c| app.checked(c), |c| app.enabled(c));
        self.input.active = None;
        self.pressed = None;
        if let Some(c) = chosen {
            self.command(el, c);
        } else {
            self.redraw();
        }
    }

    /// Opens a menu at the mouse position; the choice arrives as input.
    #[cfg(not(windows))]
    pub(super) fn popup(&mut self, _el: &ActiveEventLoop, items: Vec<M>) {
        let app = &self.app;
        let items = ui::menu::build(items, &|c| app.checked(c), &|c| app.enabled(c));
        let at = self.cursor.unwrap_or((0.0, 0.0));
        self.app.menu.open(None, items, at, false);
        self.input.active = None;
        self.pressed = None;
        self.redraw();
    }

    /// Opens the top-level menu `i` under its title.
    #[cfg(not(windows))]
    pub(super) fn open_menu(&mut self, i: usize, keyboard: bool) {
        let tree = app::commands::menu_tree(&self.app.settings.recent);
        let Some((_, items)) = tree.into_iter().nth(i) else { return };
        let app = &self.app;
        let items = ui::menu::build(items, &|c| app.checked(c), &|c| app.enabled(c));
        let Some(rect) = self.app.menu.titles.get(i).map(|t| t.rect) else { return };
        self.app.menu.open(Some(i), items, (rect.x, rect.bottom() + 2.0), keyboard);
        self.redraw();
    }

    #[cfg(not(windows))]
    pub(super) fn menu_act(&mut self, el: &ActiveEventLoop, act: ui::menu::Act) {
        use ui::menu::Act;
        match act {
            Act::Run(c) => {
                self.app.menu.close();
                self.command(el, c);
            }
            Act::Switch(dir) => {
                let n = self.app.menu.titles.len() as isize;
                if let Some(top) = self.app.menu.top {
                    self.open_menu((top as isize + dir).rem_euclid(n) as usize, true);
                }
            }
            Act::None => self.redraw(),
        }
    }

    /// Keyboard for the menus. Returns true when it used the key.
    #[cfg(not(windows))]
    pub(super) fn menu_key(&mut self, el: &ActiveEventLoop, key: &Key) -> bool {
        if !self.app.menu.is_open() {
            let found = match key {
                Key::F(10) => Some(0),
                Key::Char(c) if self.mods.alt_key() && !self.mods.control_key() => {
                    let k = c.to_lowercase().next();
                    self.app.menu.titles.iter().position(|t| t.mnemonic == k)
                }
                _ => None,
            };
            if let Some(i) = found {
                self.open_menu(i, true);
            }
            return found.is_some();
        }
        let act = match key {
            Key::Up => {
                self.app.menu.up_down(-1);
                ui::menu::Act::None
            }
            Key::Down => {
                self.app.menu.up_down(1);
                ui::menu::Act::None
            }
            Key::Left => self.app.menu.left(),
            Key::Right => self.app.menu.right(),
            Key::Enter | Key::Char(' ') => self.app.menu.enter(),
            Key::Escape => {
                self.app.menu.back();
                ui::menu::Act::None
            }
            Key::Char(c) => self.app.menu.mnemonic(*c),
            _ => ui::menu::Act::None,
        };
        self.menu_act(el, act);
        true
    }

    /// The pointer follows the open menus.
    #[cfg(not(windows))]
    pub(super) fn menu_pointer(&mut self) {
        if !self.app.menu.is_open() {
            return;
        }
        match self.input.hot.and_then(|h| self.widget(h)).map(|w| w.action.clone()) {
            Some(UiAction::MenuRow(l, i)) => {
                self.app.menu.hover(l, i);
                self.redraw();
            }
            Some(UiAction::MenuTitle(i)) if self.app.menu.top.is_some_and(|t| t != i) => self.open_menu(i, false),
            _ => {}
        }
    }

    pub(super) fn status_menu(&mut self, el: &ActiveEventLoop, k: u8) {
        use rt_core::menu::M::*;
        let items = match k {
            0 => {
                let mut v = vec![Item(Command::LangAuto), Sep];
                v.extend((0..syntax::lang::DEFS.len()).map(|i| Item(Command::SetLang(i as u8))));
                v
            }
            1 => {
                let all = io::encoding::Enc::all();
                let mut save: Vec<M> = (0..all.len()).map(|i| Item(Command::SetEncoding(i as u8))).collect();
                save.insert(4, Sep);
                let mut reopen: Vec<M> = (0..all.len()).map(|i| Item(Command::ReopenWithEncoding(i as u8))).collect();
                reopen.insert(4, Sep);
                vec![Sub("&Save with encoding", save), Sub("&Reopen with encoding", reopen)]
            }
            2 => vec![Item(Command::SetEol(0)), Item(Command::SetEol(1)), Item(Command::SetEol(2))],
            3 => vec![
                Item(Command::UseSpaces),
                Item(Command::UseTabs),
                Sep,
                Item(Command::TabWidth(2)),
                Item(Command::TabWidth(4)),
                Item(Command::TabWidth(8)),
                Sep,
                Item(Command::IndentToSpaces),
                Item(Command::IndentToTabs),
            ],
            _ => return self.command(el, Command::GotoLine),
        };
        self.popup(el, items);
    }

    pub(super) fn editor_menu(&mut self, el: &ActiveEventLoop) {
        self.popup(el, app::commands::editor_menu());
    }

    pub(super) fn tab_menu(&mut self, el: &ActiveEventLoop, i: usize) {
        self.app.activate(i);
        self.redraw();
        self.popup(el, app::commands::tab_menu());
    }

    #[cfg(windows)]
    pub(super) fn sync_menu(&mut self) {
        let Some(win) = &mut self.win else { return };
        if !self.app.menu_dirty && win.menu.is_some() {
            return;
        }
        self.app.menu_dirty = false;
        if win.menu.is_none() || self.menu_recent != self.app.settings.recent || self.app.menu_rebuild {
            self.app.menu_rebuild = false;
            self.menu_recent = self.app.settings.recent.clone();
            let hwnd = self.host.hwnd;
            let tree = app::commands::menu_tree(&self.menu_recent);
            win::set_mnemonics(&rt_ui::window::mnemonics_of(&tree));
            win.menu = Some(win::menu::MenuBar::install(hwnd, tree));
        }
        if let Some(m) = &win.menu {
            let app = &self.app;
            m.sync(|c| app.checked(c), |c| app.enabled(c));
        }
    }

    /// The titles of the drawn menu bar, in the current language.
    #[cfg(not(windows))]
    pub(super) fn sync_menu(&mut self) {
        if !self.app.menu.titles.is_empty() && !self.app.menu_rebuild {
            return;
        }
        self.app.menu_rebuild = false;
        self.app.menu.titles = app::commands::menu_tree(&[])
            .iter()
            .map(|(title, _)| {
                let (label, mnemonic, mark) = ui::menu::parse_label(&i18n::tr(title));
                ui::menu::Title { label, mnemonic, mark, rect: Rect::new(0.0, 0.0, 0.0, 0.0) }
            })
            .collect();
        self.redraw();
    }

    /// Dark menu bar and popup menus in the dark theme.
    pub(super) fn sync_menu_theme(&mut self, dark: bool) {
        if self.menu_dark == Some(dark) {
            return;
        }
        self.menu_dark = Some(dark);
        #[cfg(windows)]
        {
            let colors = dark.then_some(win::darkmode::MenuColors { bar: 0x202020, text: 0xE4E4E4, text_disabled: 0x7A7A7A, hot: 0x3D3D3D });
            win::darkmode::apply(self.host.hwnd, colors);
        }
    }
}
