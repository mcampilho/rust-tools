//! Keyboard and mouse: typing finds programs, the arrows move in the list,
//! clicks choose what is shown and sort.

use super::*;

impl Shell {
    pub(super) fn key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) {
        if self.dialog.is_some() {
            if Message::closes_on(&event.logical_key) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        #[cfg(not(windows))]
        if self.menu_key(el, event) {
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
            self.menu_dirty = true;
            self.redraw();
            return;
        }
        if !name.is_empty() {
            if let Some(cmd) = commands::lookup(ctrl, shift, alt, &name) {
                return self.command(el, cmd);
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
                _ => return,
            },
        }
        self.menu_dirty = true;
        self.redraw();
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
    fn press_search(&mut self, x: f32) {
        let s = self.scale();
        let at = view::search_offset(&mut self.gfx, &self.app, &self.layout, x, s);
        let now = std::time::Instant::now();
        let double = self.last_click.is_some_and(|(t, x0)| now.duration_since(t).as_millis() < 450 && (x - x0).abs() < 5.0);
        self.last_click = Some((now, x));
        let shift = self.mods.shift_key();
        self.app.edit_search(|t| {
            if double {
                t.select_word(at);
            } else {
                t.set_caret(at, shift);
            }
        });
        self.selecting = !double;
    }

    pub(super) fn pointer(&mut self, x: f32, y: f32) {
        let l = &self.layout;
        let hot = if self.dialog.is_some() {
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
            self.redraw();
        }
        let icon = match hot {
            Hot::Search => CursorIcon::Text,
            h if h.clickable() => CursorIcon::Pointer,
            // Working, but the window still answers.
            _ if self.app.busy.is_some() => CursorIcon::Progress,
            _ => CursorIcon::Default,
        };
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(w) = &self.win {
                w.window.set_cursor(icon);
            }
        }
    }

    pub(super) fn press(&mut self, el: &ActiveEventLoop, x: f32, y: f32) {
        if let Some(d) = &self.dialog {
            if d.closes_at(x, y) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        #[cfg(not(windows))]
        if self.menu_press(el, x, y) {
            return;
        }
        let focus = self.hot == Hot::Search;
        if focus != self.app.search_focus {
            self.app.search_focus = focus;
            self.redraw();
        }
        match self.hot {
            Hot::Search => self.press_search(x),
            Hot::Tab(i) => {
                if let Some(&t) = self.app.tabs().get(i) {
                    self.app.set_tab(t);
                }
            }
            Hot::Action(i) => return self.command(el, commands::actions(self.app.tab)[i]),
            Hot::Stop => return self.stop_job(),
            Hot::Whose(i) => return self.command(el, Command::Whose(i as u8)),
            Hot::System => return self.command(el, Command::ShowSystem),
            Hot::Updates => return self.command(el, Command::ShowUpdates),
            #[cfg(windows)]
            Hot::Restart => return self.command(el, Command::RestartAdmin),
            Hot::Header(c) => self.app.sort_by(c),
            Hot::CheckAll => self.app.toggle_tick_all(),
            Hot::Check(i) => self.app.toggle_tick(i),
            Hot::Row(i) => self.app.click(i, self.mods.control_key(), self.mods.shift_key()),
            Hot::None => {
                let l = &self.layout;
                if l.track.contains(x, y) && self.app.rows.len() > l.page_rows() {
                    self.dragging = true;
                    self.drag_to(y);
                }
            }
            _ => {}
        }
        self.menu_dirty = true;
        self.redraw();
    }

    pub(super) fn drag_to(&mut self, y: f32) {
        let page = self.layout.page_rows();
        self.app.scroll = view::scroll_for(&self.layout, self.app.rows.len(), page, y);
        self.redraw();
    }
}
