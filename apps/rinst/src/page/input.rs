//! Keyboard and mouse of the page: typing finds programs, the arrows move in
//! the list, clicks choose what is shown and sort.

use super::*;
use winit::keyboard::{Key as WKey, NamedKey};

impl InstPage {
    /// A key: the command of a chord (for the window to run), or the search,
    /// the arrows, Space and typing.
    pub fn key(&mut self, event: &winit::event::KeyEvent, mods: Mods) -> Option<Command> {
        self.mods = mods;
        let Mods { ctrl, shift, alt } = mods;
        let logical = if ctrl || alt {
            use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
            event.key_without_modifiers()
        } else {
            event.logical_key.clone()
        };
        let name = match &logical {
            WKey::Character(c) if ctrl || alt => c.to_lowercase(),
            WKey::Named(n) => match n {
                NamedKey::F1 => "f1".into(),
                NamedKey::F5 => "f5".into(),
                NamedKey::Escape => "esc".into(),
                NamedKey::Delete => "delete".into(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        if self.search_key(&logical, ctrl, shift, alt) {
            self.fx.menus();
            self.fx.redraw();
            return None;
        }
        if !name.is_empty() {
            if let Some(cmd) = commands::lookup(ctrl, shift, alt, &name) {
                return Some(cmd);
            }
        }
        let page = self.layout.page_rows() as isize;
        // Moving in the list leaves the search box; there, Space ticks.
        let moves = matches!(
            logical,
            WKey::Named(NamedKey::ArrowDown | NamedKey::ArrowUp | NamedKey::PageDown | NamedKey::PageUp | NamedKey::Home | NamedKey::End)
        );
        if moves {
            self.app.search_focus = false;
        }
        match logical {
            WKey::Named(NamedKey::Space) if !self.app.search_focus && !ctrl && !alt => self.app.toggle_ticks_selected(),
            WKey::Named(NamedKey::ArrowDown) => self.app.move_sel(1, shift),
            WKey::Named(NamedKey::ArrowUp) => self.app.move_sel(-1, shift),
            WKey::Named(NamedKey::PageDown) => self.app.move_sel(page, shift),
            WKey::Named(NamedKey::PageUp) => self.app.move_sel(-page, shift),
            WKey::Named(NamedKey::Home) => self.app.move_sel(isize::MIN / 2, shift),
            WKey::Named(NamedKey::End) => self.app.move_sel(isize::MAX / 2, shift),
            WKey::Named(NamedKey::Backspace) => self.app.backspace(),
            // Letters and digits find programs (Ctrl alone is for commands;
            // Ctrl with Alt is AltGr, which types).
            _ => match &event.text {
                Some(t) if !(ctrl && !alt) && !(alt && !ctrl) && !t.chars().all(char::is_control) => self.app.type_text(t),
                _ => return None,
            },
        }
        self.fx.menus();
        self.fx.redraw();
        None
    }

    /// The keys of the search box: Ctrl+V anywhere; with the box in use, the
    /// arrows, Home, End, Delete, Ctrl+A, Ctrl+C and Ctrl+X edit it. True when
    /// the key was used.
    fn search_key(&mut self, key: &WKey, ctrl: bool, shift: bool, alt: bool) -> bool {
        if alt {
            return false;
        }
        let ch = |c: &str| matches!(key, WKey::Character(k) if k.eq_ignore_ascii_case(c));
        // Pasting goes to the search wherever the focus is, as typing does.
        if ctrl && !shift && ch("v") {
            self.paste();
            return true;
        }
        if !self.app.search_focus {
            return false;
        }
        let e = &self.app.search;
        match key {
            WKey::Character(_) if ctrl && !shift && ch("a") => self.app.edit_search(|t| t.select_all()),
            WKey::Character(_) if ctrl && !shift && ch("c") && e.selection().is_some() => {
                rt_sys::set_clipboard_text(e.selected_text());
            }
            WKey::Character(_) if ctrl && !shift && ch("x") && e.selection().is_some() => {
                let mut cut = String::new();
                self.app.edit_search(|t| cut = t.cut());
                rt_sys::set_clipboard_text(&cut);
            }
            WKey::Named(NamedKey::ArrowLeft) => self.app.edit_search(|t| t.move_caret(true, false, ctrl, shift)),
            WKey::Named(NamedKey::ArrowRight) => self.app.edit_search(|t| t.move_caret(false, false, ctrl, shift)),
            WKey::Named(NamedKey::Home) => self.app.edit_search(|t| t.move_caret(true, true, false, shift)),
            WKey::Named(NamedKey::End) => self.app.edit_search(|t| t.move_caret(false, true, false, shift)),
            // In the box, Delete deletes text, never programs.
            WKey::Named(NamedKey::Delete) => self.app.edit_search(|t| t.delete(true, ctrl)),
            WKey::Named(NamedKey::Backspace) => self.app.edit_search(|t| t.delete(false, ctrl)),
            _ => return false,
        }
        true
    }

    /// The clipboard's text into the search, at its caret.
    pub(super) fn paste(&mut self) {
        if let Some(t) = rt_sys::clipboard_text() {
            self.app.type_text(&t);
        }
    }

    /// A press in the search box: the caret there (with Shift: selecting up
    /// to there; twice: the word), and dragging selects.
    fn press_search(&mut self, g: &mut Gfx, x: f32) {
        let at = view::search_offset(g, &self.app, &self.layout, x, self.scale);
        let now = std::time::Instant::now();
        let double = self.last_click.is_some_and(|(t, x0)| now.duration_since(t).as_millis() < 450 && (x - x0).abs() < 5.0);
        self.last_click = Some((now, x));
        let shift = self.mods.shift;
        self.app.edit_search(|t| {
            if double {
                t.select_word(at);
            } else {
                t.set_caret(at, shift);
            }
        });
        self.selecting = !double;
    }

    /// What the pointer is on; `blocked` while a message is open.
    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        self.pointer_at = Some((x, y));
        let l = &self.layout;
        let hot = if blocked {
            Hot::None
        } else if let Some(i) = l.actions.iter().position(|r| r.w > 0.0 && r.contains(x, y)) {
            Hot::Action(i)
        } else if l.stop.w > 0.0 && l.stop.contains(x, y) && self.app.busy.as_ref().is_some_and(|b| !b.stopping) {
            Hot::Stop
        } else if let Some(i) = l.tabs.iter().position(|r| r.contains(x, y)) {
            Hot::Tab(i)
        } else if l.search.contains(x, y) {
            Hot::Search
        } else if let Some(i) = l.whose.iter().position(|r| r.contains(x, y)) {
            Hot::Whose(i)
        } else if l.system.contains(x, y) {
            Hot::System
        } else if l.updates.contains(x, y) {
            Hot::Updates
        } else if l.restart.contains(x, y) && l.banner.h > 0.0 {
            Hot::Restart
        } else if l.header.contains(x, y) && x < l.check_area().1 && x >= l.check_area().0 {
            Hot::CheckAll
        } else if l.list.contains(x, y) && x < l.check_area().1 && x >= l.check_area().0 && l.row_at(&self.app, x, y).is_some() {
            l.row_at(&self.app, x, y).map_or(Hot::None, Hot::Check)
        } else if l.header.contains(x, y) {
            l.cols.iter().find(|c| x >= c.1 && x < c.1 + c.2).map_or(Hot::None, |c| Hot::Header(c.0))
        } else if l.details.contains(x, y) {
            Hot::Details
        } else {
            l.row_at(&self.app, x, y).map_or(Hot::None, Hot::Row)
        };
        if hot != self.hot {
            self.hot = hot;
            self.fx.redraw();
        }
        self.cursor_icon = match hot {
            Hot::Search => CursorIcon::Text,
            h if h.clickable() => CursorIcon::Pointer,
            // Working, but the window still answers.
            _ if self.app.busy.is_some() => CursorIcon::Progress,
            _ => CursorIcon::Default,
        };
    }

    /// The pointer left the page.
    pub fn pointer_left(&mut self) {
        self.pointer_at = None;
        if !self.dragging && self.hot != Hot::None {
            self.hot = Hot::None;
            self.fx.redraw();
        }
    }

    /// The pointer's shape follows whether it is working.
    pub(super) fn update_pointer(&mut self) {
        if let Some((x, y)) = self.pointer_at {
            self.pointer(x, y, false);
        }
    }

    /// A click at (x, y): the command of a button, for the window to run.
    pub fn press(&mut self, g: &mut Gfx, x: f32, y: f32, mods: Mods) -> Option<Command> {
        self.mods = mods;
        let focus = self.hot == Hot::Search;
        if focus != self.app.search_focus {
            self.app.search_focus = focus;
            self.fx.redraw();
        }
        let mut out = None;
        match self.hot {
            Hot::Search => self.press_search(g, x),
            Hot::Tab(i) => {
                if let Some(&t) = self.app.tabs().get(i) {
                    self.app.set_tab(t);
                }
            }
            Hot::Action(i) => out = commands::actions(self.app.tab).get(i).copied(),
            Hot::Stop => self.stop_job(),
            Hot::Whose(i) => out = Some(Command::Whose(i as u8)),
            Hot::System => out = Some(Command::ShowSystem),
            Hot::Updates => out = Some(Command::ShowUpdates),
            #[cfg(windows)]
            Hot::Restart => out = Some(Command::RestartAdmin),
            Hot::Header(c) => self.app.sort_by(c),
            Hot::CheckAll => self.app.toggle_tick_all(),
            Hot::Check(i) => self.app.toggle_tick(i),
            Hot::Row(i) => self.app.click(i, mods.ctrl, mods.shift),
            Hot::None => {
                let l = &self.layout;
                if l.track.contains(x, y) && self.app.rows.len() > l.page_rows() {
                    self.dragging = true;
                    self.drag_to(y);
                }
            }
            _ => {}
        }
        self.fx.menus();
        self.fx.redraw();
        out
    }

    /// The pointer moved while the left button is down: the scroll bar's
    /// thumb, or the search's selection, follows it.
    pub fn drag(&mut self, g: &mut Gfx, x: f32, y: f32) {
        if self.dragging {
            self.drag_to(y);
        }
        if self.selecting {
            // Dragging over the search's text selects it.
            let at = view::search_offset(g, &self.app, &self.layout, x, self.scale);
            self.app.search.set_caret(at, true);
            self.fx.redraw();
        }
    }

    pub fn drag_to(&mut self, y: f32) {
        let page = self.layout.page_rows();
        self.app.scroll = view::scroll_for(&self.layout, self.app.rows.len(), page, y);
        self.fx.redraw();
    }

    /// The left button was released.
    pub fn release(&mut self) {
        self.dragging = false;
        self.selecting = false;
    }

    /// A click with the right button: the menu of the programs (a row outside
    /// the selection is selected alone first), or the search box's.
    pub fn right_click(&mut self) -> Option<Popup> {
        if let Hot::Row(i) = self.hot {
            if self.app.tab == crate::app::Tab::Leftovers {
                return None;
            }
            // A row outside the selection: it alone; inside: the selection stays.
            if !self.app.is_marked(i) {
                self.app.select(i);
            }
            self.fx.menus();
            self.fx.redraw();
            Some(Popup::Program)
        } else if self.hot == Hot::Search {
            self.app.search_focus = true;
            self.fx.redraw();
            Some(Popup::Search)
        } else {
            None
        }
    }
}
