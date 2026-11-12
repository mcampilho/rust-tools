//! Keyboard and mouse in the document: keys, links under the pointer,
//! panning.

use super::*;

impl Shell {
    pub(super) fn start_pan(&mut self, x: f32, y: f32) {
        let (sx, sy) = self.viewer.scroll;
        self.drag = Some(Drag::Pan { x, y, sx, sy });
    }

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
        let s = self.scale();
        let line = 48.0 * s;
        let page = self.viewer.page_height();
        let name = match &logical {
            WKey::Character(c) => c.to_lowercase(),
            WKey::Named(n) => match n {
                NamedKey::F1 => "f1".into(),
                NamedKey::F5 => "f5".into(),
                NamedKey::F11 => "f11".into(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        if !name.is_empty() {
            if let Some(cmd) = commands::lookup(ctrl, shift, alt, &name) {
                return self.command(el, cmd);
            }
        }
        match logical {
            WKey::Named(NamedKey::ArrowDown) => self.viewer.scroll_by(0.0, line),
            WKey::Named(NamedKey::ArrowUp) => self.viewer.scroll_by(0.0, -line),
            WKey::Named(NamedKey::ArrowRight) => self.viewer.scroll_by(line, 0.0),
            WKey::Named(NamedKey::ArrowLeft) => self.viewer.scroll_by(-line, 0.0),
            WKey::Named(NamedKey::PageDown) => {
                if !(ctrl && self.viewer.pdf_page_step(1)) {
                    self.viewer.scroll_by(0.0, page)
                }
            }
            WKey::Named(NamedKey::PageUp) => {
                if !(ctrl && self.viewer.pdf_page_step(-1)) {
                    self.viewer.scroll_by(0.0, -page)
                }
            }
            WKey::Named(NamedKey::Space) => self.viewer.scroll_by(0.0, if shift { -page } else { page }),
            WKey::Named(NamedKey::Home) => self.viewer.scroll_to(0.0),
            WKey::Named(NamedKey::End) => self.viewer.scroll_to(f32::MAX),
            WKey::Named(NamedKey::Escape) => {
                if self.viewer.has_selection() {
                    self.viewer.clear_selection();
                    self.menu_dirty = true;
                } else if let Some(w) = &self.win {
                    if w.window.fullscreen().is_some() {
                        w.window.set_fullscreen(None);
                    }
                }
            }
            _ => return,
        }
        self.redraw();
    }

    pub(super) fn update_hover(&mut self) {
        let (link, icon) = match (self.cursor.filter(|_| self.dialog.is_none()), &self.drag) {
            (_, Some(Drag::Pan { .. })) => (None, CursorIcon::Grabbing),
            (_, Some(Drag::Select)) => (None, self.cursor_icon),
            (Some((x, y)), _) => match self.viewer.link_at(x, y) {
                Some(l) => (Some(l), CursorIcon::Pointer),
                None => (
                    None,
                    match self.viewer.hover(x, y, self.mods.alt_key()) {
                        Hover::Arrow => CursorIcon::Default,
                        Hover::Text => CursorIcon::Text,
                        Hover::Cell => CursorIcon::Cell,
                        Hover::Cross => CursorIcon::Crosshair,
                    },
                ),
            },
            _ => (None, CursorIcon::Default),
        };
        if link != self.viewer.hover_link {
            self.viewer.hover_link = link;
            self.redraw();
        }
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(w) = &self.win {
                w.window.set_cursor(icon);
            }
        }
    }

    pub(super) fn follow(&mut self, href: &str) {
        match self.viewer.follow(href) {
            LinkAction::Scrolled => {}
            LinkAction::OpenUrl(u) => {
                #[cfg(windows)]
                win::open_url(&u);
                #[cfg(not(windows))]
                linux::open_url(&u);
            }
            LinkAction::OpenFile(p, anchor) => self.open(&p, false, anchor),
            LinkAction::Unsupported(h) => self.status_note = Some(tf("This link cannot be opened here: {}", &[&h])),
        }
        self.redraw();
    }
}
