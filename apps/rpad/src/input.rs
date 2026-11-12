//! Keyboard and mouse in the window, and the pointer's shape.

use super::*;

impl Shell {
    pub(super) fn key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) {
        if self.dialog.is_some() {
            if matches!(event.logical_key, WKey::Named(NamedKey::Escape | NamedKey::Enter | NamedKey::Space)) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        let ctrl = self.mods.control_key();
        let alt = self.mods.alt_key();
        let shift = self.mods.shift_key();
        let logical = if ctrl || alt {
            use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
            event.key_without_modifiers()
        } else {
            event.logical_key.clone()
        };
        let key = match &logical {
            WKey::Named(n) => match n {
                NamedKey::Enter => Key::Enter,
                NamedKey::Tab => Key::Tab,
                NamedKey::Backspace => Key::Backspace,
                NamedKey::Delete => Key::Delete,
                NamedKey::Escape => Key::Escape,
                NamedKey::ArrowLeft => Key::Left,
                NamedKey::ArrowRight => Key::Right,
                NamedKey::ArrowUp => Key::Up,
                NamedKey::ArrowDown => Key::Down,
                NamedKey::Home => Key::Home,
                NamedKey::End => Key::End,
                NamedKey::PageUp => Key::PageUp,
                NamedKey::PageDown => Key::PageDown,
                NamedKey::Insert => Key::Insert,
                NamedKey::Space => Key::Char(' '),
                NamedKey::F1 => Key::F(1),
                NamedKey::F2 => Key::F(2),
                NamedKey::F3 => Key::F(3),
                NamedKey::F4 => Key::F(4),
                NamedKey::F5 => Key::F(5),
                NamedKey::F6 => Key::F(6),
                NamedKey::F7 => Key::F(7),
                NamedKey::F8 => Key::F(8),
                NamedKey::F9 => Key::F(9),
                #[cfg(not(windows))]
                NamedKey::F10 => Key::F(10),
                NamedKey::F11 => Key::F(11),
                NamedKey::F12 => Key::F(12),
                _ => return,
            },
            WKey::Character(s) => match s.chars().next() {
                Some(c) => Key::Char(c),
                None => return,
            },
            _ => return,
        };
        #[cfg(not(windows))]
        if self.menu_key(el, &key) {
            return;
        }
        let text = event.text.as_ref().map(|t| t.to_string());
        let k = KeyInput { key, ctrl, shift, alt, text };
        self.input.tooltip = None;
        let consumed = self.app.key(&k, &self.host);
        if !consumed {
            if let Some(t) = &k.text {
                // Plain keys and AltGr (reported as Ctrl+Alt) type text;
                // Ctrl+key and Alt+key are shortcuts only.
                if ctrl == alt {
                    self.app.text_input(t, &self.host);
                }
            }
        }
        self.after_action(el);
    }

    pub(super) fn mouse_press(&mut self, el: &ActiveEventLoop, button: MouseButton) {
        let Some((x, y)) = self.cursor else { return };
        // The OK button, or a click outside the message, closes it.
        if let Some((_, _, panel, ok)) = &self.dialog {
            if ok.contains(x, y) || !panel.contains(x, y) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        self.input.tooltip = None;
        self.update_hot();
        let hit = self.hit(x, y).cloned();
        #[cfg(not(windows))]
        if self.app.menu.is_open() {
            let on_menu = hit.is_some() || self.app.menu.contains(x, y);
            if button != MouseButton::Left || !on_menu {
                self.app.menu.close();
                return self.redraw();
            }
            if hit.is_none() {
                return;
            }
        }
        match button {
            MouseButton::Left => {
                if self.app.palette.is_some() && hit.is_none() {
                    self.app.palette = None;
                    self.app.focus = Focus::Editor;
                    return self.after_action(el);
                }
                match hit {
                    Some(w) if w.action != UiAction::Editor => {
                        if let UiAction::Field(f) = w.action {
                            self.app.focus = f;
                            self.reset_blink();
                            self.redraw();
                            return;
                        }
                        self.pressed = Some(w.id);
                        self.input.active = Some(w.id);
                        self.redraw();
                    }
                    _ => {
                        let l = &self.app.layout;
                        if l.rect.contains(x, y) || l.hscroll.contains(x, y) {
                            let (c, s, a) = (self.mods.control_key(), self.mods.shift_key(), self.mods.alt_key());
                            self.app.mouse_down(x, y, c, s, a);
                            self.editor_drag = true;
                            self.after_action(el);
                        }
                    }
                }
            }
            MouseButton::Middle => {
                if let Some(Widget { action: UiAction::Tab(i) | UiAction::CloseTab(i), .. }) = hit {
                    self.app.close_doc(i, &self.host);
                    self.after_action(el);
                }
            }
            MouseButton::Right => match hit.map(|w| w.action) {
                Some(UiAction::Tab(i)) => self.tab_menu(el, i),
                Some(UiAction::Editor) | None if self.app.layout.rect.contains(x, y) => {
                    // Keep the selection when right-clicking inside it.
                    if let Some(pos) = self.app.hit_pos(x, y) {
                        let s = self.app.doc().buf.primary_sel();
                        if self.app.doc().buf.sels.len() == 1 && !(pos >= s.start() && pos <= s.end() && !s.is_empty()) {
                            self.app.doc_mut().buf.set_caret(pos);
                        }
                    }
                    self.app.focus = Focus::Editor;
                    self.redraw();
                    self.editor_menu(el);
                }
                _ => {}
            },
            _ => {}
        }
    }

    pub(super) fn mouse_release(&mut self, el: &ActiveEventLoop) {
        if self.editor_drag {
            self.editor_drag = false;
            self.app.mouse_up();
            self.redraw();
        }
        let pressed = self.pressed.take();
        self.input.active = None;
        self.update_hot();
        if let (Some(p), Some(h)) = (pressed, self.input.hot) {
            if p == h {
                if let Some(w) = self.widget(p).cloned() {
                    return self.action(el, w.action);
                }
            }
        }
        self.redraw();
    }

    pub(super) fn wheel(&mut self, delta: MouseScrollDelta) {
        let Some((x, y)) = self.cursor else { return };
        let line_h = self.app.layout.line_h.max(1.0);
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, l) => -l * 3.0,
            MouseScrollDelta::PixelDelta(p) => -(p.y as f32) / line_h,
        };
        let over = self.hit(x, y).map(|w| w.action.clone());
        #[cfg(not(windows))]
        if self.app.menu.is_open() {
            if let Some(UiAction::MenuRow(l, _)) = over {
                self.app.menu.wheel(l, lines.signum() as isize * 2);
                self.redraw();
            }
            return;
        }
        match over {
            Some(UiAction::Outline(_)) => {
                self.app.outline_scroll = (self.app.outline_scroll as isize + lines.round() as isize).max(0) as usize;
            }
            Some(UiAction::Result(_)) => {
                if let Some(r) = self.app.results.as_mut() {
                    let n = r.items.len().saturating_sub(1);
                    r.sel = ((r.sel as isize + lines.round() as isize).max(0) as usize).min(n);
                }
            }
            Some(UiAction::PaletteItem(_)) => {
                if let Some(p) = self.app.palette.as_mut() {
                    let n = p.shown.len().saturating_sub(1);
                    p.sel = ((p.sel as isize + lines.signum() as isize).max(0) as usize).min(n);
                }
            }
            Some(UiAction::None) => self.app.preview.scroll += lines * line_h,
            _ => {
                if self.app.layout.rect.contains(x, y) {
                    self.app.wheel(lines, self.mods.control_key(), self.mods.shift_key());
                    if self.mods.control_key() {
                        self.app.menu_dirty = true;
                    }
                }
            }
        }
        self.redraw();
    }

    pub(super) fn update_cursor_icon(&mut self) {
        let icon = match self.cursor {
            Some((x, y)) => {
                let l = &self.app.layout;
                let in_text = l.rect.contains(x, y) && x >= l.text_x - 2.0 && !l.vscroll.contains(x, y);
                let over_widget = self.hit(x, y).is_some_and(|w| w.action != UiAction::Editor);
                match self.hit(x, y).map(|w| w.role) {
                    Some(Role::TextField) => CursorIcon::Text,
                    _ if in_text && !over_widget => CursorIcon::Text,
                    _ => CursorIcon::Default,
                }
            }
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
