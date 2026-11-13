//! Keyboard and mouse: the arrows move in the list, Space and the boxes
//! tick, the buttons act.

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
                _ => String::new(),
            },
            _ => String::new(),
        };
        if !name.is_empty() {
            if let Some(cmd) = commands::lookup(ctrl, shift, alt, &name) {
                return self.command(el, cmd);
            }
        }
        let page = self.layout.page_rows() as isize;
        match logical {
            WKey::Named(NamedKey::Space) if !ctrl && !alt => {
                if let Some(r) = self.app.sel {
                    self.app.toggle_row(r);
                }
            }
            WKey::Named(NamedKey::ArrowDown) => self.app.move_sel(1),
            WKey::Named(NamedKey::ArrowUp) => self.app.move_sel(-1),
            WKey::Named(NamedKey::PageDown) => self.app.move_sel(page),
            WKey::Named(NamedKey::PageUp) => self.app.move_sel(-page),
            WKey::Named(NamedKey::Home) => self.app.move_sel(isize::MIN / 2),
            WKey::Named(NamedKey::End) => self.app.move_sel(isize::MAX / 2),
            _ => return,
        }
        self.app.note = None;
        self.menu_dirty = true;
        self.redraw();
    }

    pub(super) fn pointer(&mut self, x: f32, y: f32) {
        let l = &self.layout;
        let hot = if self.dialog.is_some() {
            Hot::None
        } else if let Some(i) = l.actions.iter().position(|r| r.w > 0.0 && r.contains(x, y)) {
            Hot::Action(i)
        } else if l.stop.w > 0.0 && l.stop.contains(x, y) && self.app.busy.as_ref().is_some_and(|b| !b.stopping) {
            Hot::Stop
        } else if l.banner.h > 0.0 && l.restart.contains(x, y) {
            Hot::Restart
        } else if l.details.contains(x, y) {
            Hot::Details
        } else if let Some(r) = l.row_at(&self.app, x, y) {
            let (x0, x1) = l.check_area(&self.app, r);
            if x >= x0 && x < x1 && self.app.busy.is_none() {
                Hot::Check(r)
            } else {
                Hot::Row(r)
            }
        } else {
            Hot::None
        };
        if hot != self.hot {
            self.hot = hot;
            self.redraw();
        }
        let icon = match hot {
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

    /// The pointer's shape follows whether it is working.
    pub(super) fn update_pointer(&mut self) {
        if let Some((x, y)) = self.cursor {
            self.pointer(x, y);
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
        self.app.note = None;
        match self.hot {
            Hot::Action(i) => return self.command(el, commands::ACTIONS[i]),
            Hot::Stop => return self.command(el, Command::Stop),
            #[cfg(windows)]
            Hot::Restart => return self.command(el, Command::RestartAdmin),
            Hot::Check(r) => {
                self.app.select(r);
                self.app.toggle_row(r);
            }
            Hot::Row(r) => self.app.select(r),
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
