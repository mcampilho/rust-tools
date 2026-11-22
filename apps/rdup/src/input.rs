//! Keyboard and mouse: the page takes the list, the filter boxes, the
//! splitters and the scroll bars; the window runs the commands of the chords
//! and the buttons, and shows the menus that pop up.

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
        let cmd = self.page.key(event, self.mods.into());
        self.after_page();
        if let Some(cmd) = cmd {
            self.command(el, cmd);
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
        let out = self.page.press(&mut self.gfx, x, y, self.mods.into());
        self.after_page();
        match out {
            Some(Out::Command(c)) => self.command(el, c),
            Some(Out::Popup(p)) => self.popup(el, p),
            None => {}
        }
    }
}
