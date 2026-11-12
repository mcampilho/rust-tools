//! Keyboard and mouse in the window: the list, the filter boxes, the
//! splitters and scroll bars.

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
        let start = view::page(&self.app) == Page::Start;
        if let (Some(cal), true) = (&mut self.app.calendar, start) {
            match event.logical_key {
                WKey::Named(NamedKey::Escape) => self.app.calendar = None,
                WKey::Named(NamedKey::PageUp) => cal.shift(-1),
                WKey::Named(NamedKey::PageDown) => cal.shift(1),
                _ => return,
            }
            self.redraw();
            return;
        }
        #[cfg(not(windows))]
        if self.menu_key(el, event) {
            return;
        }
        if self.field_key(el, event) {
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
                NamedKey::Space => "space".into(),
                NamedKey::Delete => "delete".into(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        if !name.is_empty() {
            if let Some(cmd) = commands::lookup(ctrl, shift, alt, &name) {
                return self.command(el, cmd);
            }
        }
        if view::page(&self.app) != Page::Result {
            return;
        }
        let page = self.layout.page_rows() as isize;
        match logical {
            WKey::Named(NamedKey::ArrowDown) => self.app.move_sel(1),
            WKey::Named(NamedKey::ArrowUp) => self.app.move_sel(-1),
            WKey::Named(NamedKey::PageDown) => self.app.move_sel(page),
            WKey::Named(NamedKey::PageUp) => self.app.move_sel(-page),
            WKey::Named(NamedKey::Home) => self.app.move_sel(isize::MIN / 2),
            WKey::Named(NamedKey::End) => self.app.move_sel(isize::MAX / 2),
            _ => return,
        }
        self.menu_dirty = true;
        self.redraw();
    }

    /// Typing in a filter box, and Tab to reach them. Returns true when it used the key.
    pub(super) fn field_key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) -> bool {
        use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
        if view::page(&self.app) != Page::Start {
            return false;
        }
        let (ctrl, alt, shift) = (self.mods.control_key(), self.mods.alt_key(), self.mods.shift_key());
        let tab = matches!(event.logical_key, WKey::Named(NamedKey::Tab)) && !ctrl && !alt;
        if self.app.focus.is_none() {
            if tab {
                self.app.focus_next(shift);
                self.redraw();
            }
            return tab;
        }
        let key = if ctrl { event.key_without_modifiers() } else { event.logical_key.clone() };
        match &key {
            WKey::Named(NamedKey::Backspace) => self.app.delete(false, ctrl),
            WKey::Named(NamedKey::Delete) => self.app.delete(true, ctrl),
            WKey::Named(NamedKey::ArrowLeft) => self.app.move_caret(-1, false, ctrl, shift),
            WKey::Named(NamedKey::ArrowRight) => self.app.move_caret(1, false, ctrl, shift),
            WKey::Named(NamedKey::Home) => self.app.move_caret(-1, true, false, shift),
            WKey::Named(NamedKey::End) => self.app.move_caret(1, true, false, shift),
            WKey::Named(NamedKey::Tab) if tab => self.app.focus_next(shift),
            WKey::Named(NamedKey::Escape) => self.app.focus = None,
            WKey::Named(NamedKey::Enter) if !ctrl && !alt => {
                self.app.focus = None;
                self.command(el, Command::Search);
            }
            // Ctrl alone: editing shortcuts (Ctrl with Alt is AltGr, which types).
            WKey::Character(c) if ctrl && !alt => {
                let text = self.app.selected_text();
                match c.to_lowercase().as_str() {
                    "a" => self.app.select_all(),
                    "c" if !text.is_empty() => {
                        rt_sys::set_clipboard_text(&text);
                    }
                    "x" if !text.is_empty() => {
                        rt_sys::set_clipboard_text(&text);
                        self.app.delete(false, false);
                    }
                    "c" | "x" => {}
                    "v" => {
                        if let Some(t) = rt_sys::clipboard_text() {
                            self.app.type_text(&t.replace(['\r', '\n', '\t'], " "));
                        }
                    }
                    _ => return false,
                }
            }
            _ => match &event.text {
                Some(t) if !(alt && !ctrl) && !t.chars().all(char::is_control) => self.app.type_text(t),
                _ => return false,
            },
        }
        self.redraw();
        true
    }

    /// Updates what the pointer is on.
    pub(super) fn pointer(&mut self, x: f32, y: f32) {
        let l = &self.layout;
        let hot = match view::page(&self.app) {
            _ if self.dialog.is_some() => Hot::None,
            Page::Start if self.app.calendar.is_some() => l.cal.as_ref().and_then(|c| c.hot_at(x, y)).unwrap_or(Hot::None),
            Page::Start if l.body.contains(x, y) && !l.actions.contains(x, y) => {
                if let Some(i) = l.cards.iter().position(|r| r.contains(x, y)) {
                    Hot::Card(i)
                } else if let Some(i) = l.removes.iter().position(|r| r.contains(x, y)) {
                    Hot::Remove(i)
                } else if let Some(i) = l.folders.iter().position(|r| r.contains(x, y)) {
                    Hot::Folder(i)
                } else if let Some(i) = l.sizes.iter().position(|r| r.contains(x, y)) {
                    Hot::MinSize(i)
                } else if let Some(i) = l.fields.iter().position(|r| r.contains(x, y)) {
                    let f = rdup::filter::Field::ALL[i];
                    if matches!(f, rdup::filter::Field::From | rdup::filter::Field::To) && view::pick_button(l.fields[i], self.scale()).contains(x, y) {
                        Hot::Pick(f)
                    } else {
                        Hot::Field(f)
                    }
                } else if let Some(i) = l.kinds.iter().position(|r| r.contains(x, y)) {
                    Hot::Kind(i)
                } else if let Some(i) = l.compare.iter().position(|r| r.contains(x, y)) {
                    Hot::Compare(i)
                } else if l.clear.contains(x, y) && self.app.choice.narrows() {
                    Hot::Clear
                } else {
                    Hot::None
                }
            }
            Page::Start => {
                if l.add.contains(x, y) {
                    Hot::Add
                } else if l.find.contains(x, y) && self.enabled(Command::Search) {
                    Hot::Find
                } else {
                    Hot::None
                }
            }
            Page::Searching => {
                if l.stop.contains(x, y) {
                    Hot::Stop
                } else {
                    Hot::None
                }
            }
            Page::Result => {
                if let Some(i) = l.tabs.iter().position(|r| r.contains(x, y)) {
                    Hot::Tab(i)
                } else if let Some(i) = l.shares.iter().position(|r| r.contains(x, y)) {
                    Hot::Share(i)
                } else if l.rules.contains(x, y) && self.enabled(Command::KeepNewest) {
                    Hot::Rules
                } else if l.unmark.contains(x, y) && self.enabled(Command::UnmarkAll) {
                    Hot::Unmark
                } else if l.move_to.contains(x, y) && self.enabled(Command::MoveTo) {
                    Hot::MoveTo
                } else if l.recycle.contains(x, y) && self.enabled(Command::Recycle) {
                    Hot::Recycle
                } else if l.actions.contains(x, y) {
                    Hot::None
                } else {
                    l.row_at(&self.app, x, y).map_or(Hot::None, Hot::Row)
                }
            }
        };
        if hot != self.hot {
            self.hot = hot;
            self.redraw();
        }
        let icon = match hot {
            Hot::Card(_)
            | Hot::Remove(_)
            | Hot::Folder(_)
            | Hot::MinSize(_)
            | Hot::Kind(_)
            | Hot::Compare(_)
            | Hot::Clear
            | Hot::Pick(_)
            | Hot::CalPrev
            | Hot::CalNext
            | Hot::CalDay(_)
            | Hot::CalToday
            | Hot::CalClear
            | Hot::Add
            | Hot::Find
            | Hot::Tab(_)
            | Hot::Share(_)
            | Hot::Rules
            | Hot::Unmark
            | Hot::MoveTo
            | Hot::Recycle
            | Hot::Stop => CursorIcon::Pointer,
            Hot::Field(_) => CursorIcon::Text,
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
        if !matches!(self.hot, Hot::Field(_)) {
            self.app.focus = None;
        }
        // A click outside an open calendar only closes it.
        if self.app.calendar.is_some() && !matches!(self.hot, Hot::CalPrev | Hot::CalNext | Hot::CalDay(_) | Hot::CalToday | Hot::CalClear | Hot::CalPanel) {
            self.app.calendar = None;
            self.redraw();
            return;
        }
        match self.hot {
            Hot::Field(f) => {
                let s = self.scale();
                let caret = view::caret_at(&mut self.gfx, &self.app, &self.layout, f, x, s);
                let double = self.last_click.is_some_and(|(t, cx, cy)| t.elapsed() < Duration::from_millis(500) && (cx - x).abs() < 4.0 * s && (cy - y).abs() < 4.0 * s);
                if double && self.app.focus == Some(f) {
                    self.app.select_word(caret);
                    self.last_click = None;
                } else {
                    if self.mods.shift_key() && self.app.focus == Some(f) {
                        self.app.drag_caret(caret);
                    } else {
                        self.app.focus_field(f, Some(caret));
                    }
                    self.selecting = true;
                    self.last_click = Some((Instant::now(), x, y));
                }
            }
            Hot::Pick(f) => self.app.calendar = Some(rdup::app::Calendar::open(f, self.app.choice.text(f))),
            Hot::CalPrev | Hot::CalNext => {
                if let Some(c) = &mut self.app.calendar {
                    c.shift(if self.hot == Hot::CalPrev { -1 } else { 1 });
                }
            }
            Hot::CalDay(i) => {
                let day = self.layout.cal.as_ref().and_then(|c| c.days.get(i)).map(|d| d.1);
                if let (Some((y, m, d)), Some(c)) = (day, self.app.calendar.take()) {
                    self.app.choice.texts[c.field.index()] = format!("{y:04}-{m:02}-{d:02}");
                }
            }
            Hot::CalToday => {
                if let Some(c) = self.app.calendar.take() {
                    let t = rt_sys::local_time();
                    self.app.choice.texts[c.field.index()] = format!("{:04}-{:02}-{:02}", t.year, t.month, t.day);
                }
            }
            Hot::CalClear => {
                if let Some(c) = self.app.calendar.take() {
                    self.app.choice.texts[c.field.index()].clear();
                }
            }
            Hot::CalPanel => {}
            Hot::Kind(i) => self.app.choice.toggle_kind(i),
            Hot::Compare(i) => self.app.compare[i] = !self.app.compare[i],
            Hot::Clear => self.app.choice.clear(),
            Hot::Card(i) => {
                if let Some(p) = self.app.volumes.as_ref().and_then(|v| v.get(i)).map(|v| v.path.clone()) {
                    self.app.toggle_root(&p);
                    self.app.error = None;
                }
            }
            Hot::Remove(i) => {
                if let Some(p) = self.app.folders().get(i) {
                    self.app.remove_folder(p);
                }
            }
            Hot::Folder(i) => {
                if let Some(p) = self.app.folders().get(i) {
                    self.app.toggle_folder(p);
                    self.app.error = None;
                }
            }
            Hot::MinSize(i) => {
                if let Some(&size) = rdup::settings::MIN_SIZES.get(i) {
                    self.app.choice.set_min(if size <= 1 { String::new() } else { view::size_label(size) });
                }
            }
            Hot::Add => self.command(el, Command::AddFolder),
            Hot::Find => self.command(el, Command::Search),
            Hot::Stop => self.command(el, Command::Stop),
            Hot::Row(i) => {
                self.app.select(i);
                // The box at the start of the row.
                if x < self.layout.cols.name.0 + 32.0 * self.scale() && self.app.markable(i) && self.acting.is_none() {
                    self.toggle_mark(i);
                }
            }
            Hot::Rules => self.rule_menu(el),
            Hot::Unmark => self.command(el, Command::UnmarkAll),
            Hot::MoveTo => self.command(el, Command::MoveTo),
            Hot::Recycle => self.command(el, Command::Recycle),
            Hot::Tab(i) => self.app.set_view(if i == 1 { View::Folders } else { View::Files }),
            Hot::Share(i) => {
                if let Some(&share) = rdup::folders::SHARES.get(i) {
                    self.app.set_share(share);
                }
            }
            Hot::None => {
                let l = &self.layout;
                if view::page(&self.app) == Page::Result && l.track.contains(x, y) && self.app.rows.len() > l.page_rows() {
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
