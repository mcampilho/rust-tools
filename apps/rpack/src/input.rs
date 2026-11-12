//! Keyboard and mouse in the window: the list, the path, the buttons and
//! the scroll bar.

use super::*;

impl Shell {
    pub(super) fn key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) {
        if let Some(d) = &self.dialog {
            // Ctrl+C copies the message (all the hashes, when it shows some).
            if self.mods.control_key() && matches!(&event.logical_key, WKey::Character(c) if c.eq_ignore_ascii_case("c")) {
                let text = self.dialog_copy.clone().unwrap_or_else(|| format!("{}\n\n{}", d.title, d.text));
                rt_sys::set_clipboard_text(&text);
                self.app.note = Some(tr("Copied to the clipboard."));
                return;
            }
            if Message::closes_on(&event.logical_key) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        if self.app.add.is_some() {
            self.form_key(event);
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
            WKey::Character(c) => c.to_lowercase(),
            WKey::Named(n) => match n {
                NamedKey::F1 => "f1".into(),
                NamedKey::F5 => "f5".into(),
                NamedKey::Enter => "enter".into(),
                NamedKey::Escape => "esc".into(),
                NamedKey::Backspace => "backspace".into(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        if !name.is_empty() {
            if let Some(cmd) = commands::lookup(ctrl, shift, alt, &name) {
                return self.command(el, cmd);
            }
        }
        if view::page(&self.app) != Page::Browse {
            return;
        }
        let page = self.layout.page_rows() as isize;
        match logical {
            WKey::Named(NamedKey::ArrowDown) => self.app.move_cursor(1, shift),
            WKey::Named(NamedKey::ArrowUp) => self.app.move_cursor(-1, shift),
            WKey::Named(NamedKey::PageDown) => self.app.move_cursor(page, shift),
            WKey::Named(NamedKey::PageUp) => self.app.move_cursor(-page, shift),
            WKey::Named(NamedKey::Home) => self.app.move_cursor(isize::MIN / 2, shift),
            WKey::Named(NamedKey::End) => self.app.move_cursor(isize::MAX / 2, shift),
            _ => return,
        }
        self.menu_dirty = true;
        self.redraw();
    }

    /// Keys in the "Add to archive" form: the name, Enter, Esc.
    fn form_key(&mut self, event: &winit::event::KeyEvent) {
        use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
        let (ctrl, alt) = (self.mods.control_key(), self.mods.alt_key());
        let key = if ctrl { event.key_without_modifiers() } else { event.logical_key.clone() };
        match &key {
            WKey::Named(NamedKey::Escape) => self.app.add = None,
            WKey::Named(NamedKey::Enter) => return self.submit_form(),
            _ => {
                let Some(f) = self.app.add.as_mut() else { return };
                match &key {
                    WKey::Named(NamedKey::Backspace) => f.delete(false),
                    WKey::Named(NamedKey::Delete) => f.delete(true),
                    WKey::Named(NamedKey::ArrowLeft) => f.move_caret(-1, false),
                    WKey::Named(NamedKey::ArrowRight) => f.move_caret(1, false),
                    WKey::Named(NamedKey::Home) => f.move_caret(-1, true),
                    WKey::Named(NamedKey::End) => f.move_caret(1, true),
                    WKey::Character(c) if ctrl && !alt => match c.to_lowercase().as_str() {
                        "a" => f.all = true,
                        "v" => {
                            if let Some(t) = rt_sys::clipboard_text() {
                                f.type_text(t.lines().next().unwrap_or(""));
                            }
                        }
                        "c" => {
                            rt_sys::set_clipboard_text(&f.name);
                        }
                        _ => return,
                    },
                    _ => match &event.text {
                        Some(t) if !(alt && !ctrl) && !t.chars().all(char::is_control) => f.type_text(t),
                        _ => return,
                    },
                }
            }
        }
        self.redraw();
    }

    /// A click in the "Add to archive" form.
    fn form_press(&mut self) {
        match self.hot {
            Hot::FormFolder => {
                if let Some(dir) = self.pick_folder(&tr("Choose where to put the archive")) {
                    if let Some(f) = self.app.add.as_mut() {
                        f.dir = dir;
                    }
                }
            }
            Hot::FormCancel => self.app.add = None,
            Hot::FormOk => return self.submit_form(),
            _ => {
                let Some(f) = self.app.add.as_mut() else { return };
                match self.hot {
                    Hot::FormName => f.all = false,
                    Hot::FormKind(i) => {
                        if let Some(&k) = rpack::create::Kind::ALL.get(i) {
                            f.set_kind(k);
                        }
                    }
                    Hot::FormLevel(i) if f.spec.kind.has_levels() => {
                        if let Some(&l) = rpack::create::Level::ALL.get(i) {
                            f.spec.level = l;
                        }
                    }
                    Hot::FormMethod(i) => {
                        if let Some(&m) = rpack::create::ZipMethod::ALL.get(i) {
                            f.spec.zip_method = m;
                        }
                    }
                    Hot::FormSolid => f.spec.solid = !f.spec.solid,
                    Hot::FormEach => f.each = !f.each,
                    _ => {}
                }
            }
        }
        self.redraw();
    }

    /// Updates what the pointer is on.
    pub(super) fn pointer(&mut self, x: f32, y: f32) {
        let l = &self.layout;
        let hot = match view::page(&self.app) {
            _ if self.dialog.is_some() => Hot::None,
            _ if self.app.add.is_some() => l.form.as_ref().map_or(Hot::None, |f| f.hot_at(x, y)),
            Page::Busy => {
                if l.stop.contains(x, y) {
                    Hot::Stop
                } else {
                    Hot::None
                }
            }
            Page::Browse => {
                if l.up.contains(x, y) && self.app.up_target().is_some() {
                    Hot::Up
                } else if let Some(i) = l.crumb.iter().position(|r| r.contains(x, y) && r.x >= l.up.right()) {
                    Hot::Crumb(i)
                } else if l.header.contains(x, y) {
                    l.cols.iter().find(|c| x >= c.1 && x < c.1 + c.2).map_or(Hot::None, |c| Hot::Header(c.0))
                } else if l.add.contains(x, y) && view::can_add(&self.app) {
                    Hot::Add
                } else if l.hashes.contains(x, y) && view::can_hash(&self.app) {
                    Hot::Hashes
                } else if l.test.contains(x, y) && view::can_extract(&self.app) {
                    Hot::Test
                } else if l.change.contains(x, y) && self.app.in_archive() {
                    Hot::Change
                } else if l.extract.contains(x, y) && view::can_extract(&self.app) {
                    Hot::Extract
                } else {
                    l.row_at(&self.app, x, y).map_or(Hot::None, Hot::Row)
                }
            }
        };
        if hot != self.hot {
            self.hot = hot;
            self.redraw();
        }
        let icon = if hot.clickable() { CursorIcon::Pointer } else { CursorIcon::Default };
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
        if self.app.add.is_some() {
            return self.form_press();
        }
        match self.hot {
            Hot::Stop => self.command(el, Command::Stop),
            Hot::Add => self.command(el, Command::AddDialog),
            Hot::Up => self.command(el, Command::Up),
            Hot::Crumb(i) => {
                let trail = self.app.trail(&view::drives_label());
                // The last place is where the browser is.
                if let Some((to, _)) = trail.get(i).filter(|_| i + 1 < trail.len()).cloned() {
                    // Going up: the place left is selected.
                    let select = match trail.get(i + 1).map(|t| &t.0) {
                        Some(Crumb::Folder(p)) => Some(p.clone()),
                        _ if self.app.in_archive() && !matches!(to, Crumb::Node(_)) => self.app.opened.as_ref().map(|o| o.path.clone()),
                        _ => None,
                    };
                    self.go(to, select);
                }
            }
            Hot::Header(c) => self.app.sort_by(c),
            Hot::Hashes => self.hash_menu(el),
            Hot::Test => self.command(el, Command::Test),
            Hot::Change => self.command(el, Command::ChooseFolder),
            Hot::Extract => self.command(el, Command::Extract),
            Hot::Row(i) => {
                let double = self.last_click.is_some_and(|(t, r)| r == i && t.elapsed() < Duration::from_millis(500));
                self.app.click(i, self.mods.control_key(), self.mods.shift_key());
                if double && !self.mods.control_key() && !self.mods.shift_key() {
                    self.last_click = None;
                    self.command(el, Command::OpenItem);
                    return;
                }
                self.last_click = Some((Instant::now(), i));
            }
            Hot::FormName | Hot::FormFolder | Hot::FormKind(_) | Hot::FormLevel(_) | Hot::FormMethod(_) | Hot::FormSolid | Hot::FormEach | Hot::FormCancel | Hot::FormOk | Hot::FormPanel => {}
            Hot::None => {
                let l = &self.layout;
                if view::page(&self.app) == Page::Browse && l.track.contains(x, y) && self.app.rows.len() > l.page_rows() {
                    self.dragging = true;
                    self.drag_to(y);
                }
            }
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
