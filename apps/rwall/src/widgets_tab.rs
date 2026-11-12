//! The widgets, seen from the settings window: starting their thread,
//! what they report, choosing a folder, removing one.

use super::*;

impl Shell {
    /// Shows the widgets as the settings say (starting or ending their thread).
    pub(super) fn apply_widgets(&mut self) {
        #[cfg(windows)]
        {
            use rwall::widgets::host::{Host, Style};
            if !self.settings.widgets.showing() {
                self.widgets = None;
                return;
            }
            if self.widgets.is_none() {
                let proxy = std::sync::Mutex::new(self.proxy.clone());
                self.widgets = Host::start(Box::new(move |e| {
                    if let Ok(p) = proxy.lock() {
                        let _ = p.send_event(UserEvent::Widget(e));
                    }
                }));
            }
            let style = Style { dark: self.dark(), accent: Theme::new(self.dark(), self.accent).accent, lang: i18n::lang() };
            if let Some(h) = &self.widgets {
                h.set(self.settings.widgets.list.clone(), style);
            }
        }
    }

    #[cfg(windows)]
    pub(super) fn widget_event(&mut self, e: rwall::widgets::host::Event) {
        use rwall::widgets::host::Event;
        match e {
            Event::Changed(w) => {
                // Only what the widget changed; the city may be being typed here.
                let typing = self.focus == Some(Field::City(w.id));
                if let Some(old) = self.settings.widgets.get_mut(w.id) {
                    let city = old.city.clone();
                    *old = w;
                    if typing {
                        old.city = city;
                    }
                }
                self.save();
            }
            Event::Remove(id) => {
                self.settings.widgets.remove(id);
                self.widget_status.retain(|(i, _)| *i != id);
                self.save();
                self.apply_widgets();
            }
            Event::NewNote { x, y, w, h } => {
                if let Some(n) = self.settings.widgets.add(WKind::Note) {
                    (n.x, n.y, n.w, n.h) = (x, y, w, h);
                }
                self.save();
                self.apply_widgets();
            }
            Event::Settings => {
                self.tab = Tab::Widgets;
                self.scroll = 0.0;
                self.show();
            }
            Event::Status(id, s) => {
                self.widget_status.retain(|(i, _)| *i != id);
                if let Some(s) = s {
                    self.widget_status.push((id, s));
                }
            }
        }
        self.redraw();
    }

    /// A widget's option was changed here: remember it and show it.
    pub(super) fn widget_changed(&mut self) {
        self.save();
        self.apply_widgets();
    }

    pub(super) fn choose_widget_folder(&mut self, id: u32) {
        let Some(kind) = self.settings.widgets.list.iter().find(|w| w.id == id).map(|w| w.kind) else { return };
        let title = if kind == WKind::Fence { tr("Choose a folder for the fence") } else { tr("Choose a folder of photos") };
        #[cfg(windows)]
        let f = rt_sys::win::dialogs::pick_folder(self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd), &title);
        #[cfg(not(windows))]
        let f = rt_sys::linux::dialogs::pick_folder(&title);
        if let (Some(dir), Some(w)) = (f, self.settings.widgets.get_mut(id)) {
            w.folder = dir;
            self.widget_changed();
        }
    }

    /// Removes a widget, asking first when text would be lost.
    pub(super) fn remove_widget(&mut self, id: u32) {
        let Some(w) = self.settings.widgets.list.iter().find(|w| w.id == id) else { return };
        if w.kind.editable() && !w.text.trim().is_empty() {
            let text = tr("Remove this widget? What is written in it is lost.");
            #[cfg(windows)]
            let yes = rwall::widgets::host::confirm(self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd), &text);
            #[cfg(not(windows))]
            let yes = !text.is_empty();
            if !yes {
                return;
            }
        }
        self.settings.widgets.remove(id);
        self.widget_status.retain(|(i, _)| *i != id);
        self.widget_changed();
    }
}
