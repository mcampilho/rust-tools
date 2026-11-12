//! Clicks, typing in the text boxes and the keyboard of the settings window.

use super::*;

impl Shell {
    pub(super) fn click(&mut self, h: Hit) {
        let st = &mut self.settings;
        let now = rt_sys::local_time();
        match h {
            Hit::Tab(t) => {
                if self.tab != t {
                    self.tab = t;
                    self.scroll = 0.0;
                    self.focus = None;
                }
            }
            Hit::LiveOn => {
                st.live.on = !st.live.on;
                self.live_error = None;
                self.apply_live();
                self.save();
            }
            Hit::LiveKind(k) => {
                let empty = k != Kind::Scene && self.settings.live.file(k).as_os_str().is_empty();
                if empty {
                    if !self.choose_live(k) {
                        self.redraw();
                        return;
                    }
                } else {
                    self.settings.live.kind = k;
                }
                self.live_changed();
            }
            Hit::LiveChoose(k) => {
                if self.choose_live(k) {
                    self.live_changed();
                }
            }
            Hit::LiveNext => self.next_video(),
            Hit::LiveShowFile => {
                if let Some(f) = self.settings.live.show().and_then(|s| s.file().map(|f| f.to_path_buf())) {
                    rt_sys::reveal(&f);
                }
            }
            Hit::LiveScene(s) => {
                st.live.scene = s;
                st.live.kind = Kind::Scene;
                self.live_changed();
            }
            Hit::LiveFit(f) => {
                st.live.fit = f;
                self.live_changed();
            }
            Hit::LivePause(p) => {
                st.live.toggle_pause(p);
                self.live_changed();
            }
            Hit::LiveSubfolders => {
                st.live.subfolders = !st.live.subfolders;
                self.save();
            }
            Hit::WidgetsOn => {
                st.widgets.on = !st.widgets.on;
                self.widget_changed();
            }
            Hit::AddWidget(k) => {
                let Some(id) = st.widgets.add(k).map(|w| w.id) else { return };
                st.widgets.on = true;
                match k {
                    WKind::Fence | WKind::Photos => self.choose_widget_folder(id),
                    // The city of the regional settings is there already;
                    // without one, type it.
                    WKind::Weather if st.widgets.list.iter().any(|w| w.id == id && w.city.is_empty()) => {
                        self.focus = Some(Field::City(id));
                        self.focus_value.clear();
                    }
                    _ => {}
                }
                self.widget_changed();
            }
            Hit::WidgetTop(id)
            | Hit::WidgetSeconds(id)
            | Hit::WidgetH24(id)
            | Hit::WidgetHolidays(id)
            | Hit::WidgetLess(id)
            | Hit::WidgetMore(id)
            | Hit::WidgetUnits(id, _) => {
                if let Some(w) = st.widgets.get_mut(id) {
                    match h {
                        Hit::WidgetTop(_) => w.on_top = !w.on_top,
                        Hit::WidgetSeconds(_) => w.seconds = !w.seconds,
                        Hit::WidgetH24(_) => w.h24 = !w.h24,
                        Hit::WidgetHolidays(_) => w.holidays = !w.holidays,
                        Hit::WidgetUnits(_, f) => w.fahrenheit = f,
                        _ => w.minutes = rwall::widgets::photo_step(w.minutes, h == Hit::WidgetMore(id)),
                    }
                }
                self.widget_changed();
            }
            Hit::WidgetRemove(id) => self.remove_widget(id),
            Hit::WidgetFolder(id) => self.choose_widget_folder(id),
            Hit::Next => self.change(),
            Hit::About => self.about(),
            Hit::OpenFolder => self.show_file(),
            Hit::Frequency(f) => {
                st.frequency = f;
                // The new frequency starts counting now: no change at once.
                st.last_period = schedule::period(f, &now, st.custom_minutes);
                self.save();
            }
            Hit::CustomLess | Hit::CustomMore => {
                st.custom_minutes = schedule::custom_step(st.custom_minutes, h == Hit::CustomMore);
                st.frequency = schedule::Frequency::Custom;
                st.last_period = schedule::period(st.frequency, &now, st.custom_minutes);
                self.save();
            }
            Hit::Source(s) => {
                st.toggle_source(s);
                if s == Source::Folders && st.source_on(s) && st.folders.is_empty() {
                    self.add_folder();
                }
                self.save();
            }
            Hit::Field(f) => {
                if self.focus != Some(f) {
                    self.unfocus();
                    self.focus = Some(f);
                    self.focus_value = self.field_text(f).map(|s| s.clone()).unwrap_or_default();
                }
            }
            Hit::AddFolder => {
                self.add_folder();
                self.save();
            }
            Hit::RemoveFolder(i) => {
                if i < st.folders.len() {
                    st.folders.remove(i);
                }
                self.save();
            }
            Hit::Subfolders => {
                st.subfolders = !st.subfolders;
                self.save();
            }
            Hit::Fit(f) => {
                if st.fit != f {
                    st.fit = f;
                    self.save();
                    self.reapply();
                }
            }
            Hit::KeepLess | Hit::KeepMore => {
                let k = st.keep;
                st.keep = if h == Hit::KeepMore {
                    KEEP_STEPS.into_iter().find(|s| *s > k).unwrap_or(KEEP_MAX)
                } else {
                    KEEP_STEPS.into_iter().rev().find(|s| *s < k).unwrap_or(KEEP_MIN)
                };
                self.save();
            }
            Hit::Autostart => match rwall::autostart::set(!self.autostart) {
                Ok(()) => self.autostart = rwall::autostart::enabled(),
                Err(e) => self.error = Some(tf("Cannot change how rwall starts: {}", &[&e])),
            },
        }
        self.redraw();
    }

    pub(super) fn add_folder(&mut self) {
        if let Some(dir) = self.pick_folder() {
            let st = &mut self.settings;
            if !st.folders.contains(&dir) {
                st.folders.push(dir);
            }
            if !st.source_on(Source::Folders) {
                st.toggle_source(Source::Folders);
            }
        }
    }

    /// Ends typing in a text box, keeping what was typed.
    pub(super) fn unfocus(&mut self) {
        let Some(f) = self.focus.take() else { return };
        let st = &mut self.settings;
        st.bing_market = st.bing_market.trim().to_string();
        if let Field::City(id) = f {
            if let Some(w) = st.widgets.get_mut(id) {
                w.city = w.city.trim().to_string();
                // Another city: look for it again.
                if w.city != self.focus_value.trim() {
                    w.place.clear();
                    self.widget_status.retain(|(i, _)| *i != id);
                }
            }
            self.apply_widgets();
        }
        self.save();
        self.redraw();
    }

    pub(super) fn field_text(&mut self, f: Field) -> Option<&mut String> {
        match f {
            Field::Market => Some(&mut self.settings.bing_market),
            Field::Wallhaven => Some(&mut self.settings.wallhaven_query),
            Field::City(id) => self.settings.widgets.get_mut(id).map(|w| &mut w.city),
        }
    }

    pub(super) fn key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) {
        if let Some(f) = self.focus {
            match &event.logical_key {
                WKey::Named(NamedKey::Enter | NamedKey::Escape | NamedKey::Tab) => self.unfocus(),
                WKey::Named(NamedKey::Backspace) => {
                    if let Some(s) = self.field_text(f) {
                        s.pop();
                    }
                }
                WKey::Character(c) if self.mods.control_key() && c.eq_ignore_ascii_case("v") => {
                    if let Some(t) = rt_sys::clipboard_text() {
                        let line = t.lines().next().unwrap_or("").to_string();
                        if let Some(s) = self.field_text(f) {
                            s.push_str(&line);
                        }
                    }
                }
                _ => {
                    if let Some(t) = event.text.as_ref().filter(|_| !self.mods.control_key() && !self.mods.alt_key()) {
                        let typed: String = t.chars().filter(|c| !c.is_control()).collect();
                        if let Some(s) = self.field_text(f) {
                            if s.chars().count() + typed.chars().count() <= 200 {
                                s.push_str(&typed);
                            }
                        }
                    }
                }
            }
            self.redraw();
            return;
        }
        match &event.logical_key {
            WKey::Named(NamedKey::Escape) => self.close(el),
            WKey::Named(NamedKey::F5) => self.change(),
            _ => {}
        }
    }

    pub(super) fn pointer(&mut self, x: f32, y: f32) {
        let hot = self.drawn.hit(x, y);
        if hot != self.hot {
            self.hot = hot;
            self.redraw();
        }
        let icon = match hot {
            Some(Hit::Field(_)) => CursorIcon::Text,
            Some(_) => CursorIcon::Pointer,
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
