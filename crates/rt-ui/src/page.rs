//! What a page of an app does outside its own drawing, so the same page can
//! sit in the window of a small app and in the window of a suite (rcare).
//!
//! A page owns its state, its work threads and its drawing in a rectangle of
//! the window. It does not own the window: when it needs the window to do
//! something (draw again, show a message, rebuild the menus) it notes it in
//! its [`Effects`], and the window takes them after each call.

/// What a page asks of the window.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Effects {
    /// The page changed: draw it again.
    pub redraw: bool,
    /// What the menus show (enabled, ticked) may have changed.
    pub menu_dirty: bool,
    /// A message to show: title and text.
    pub info: Option<(String, String)>,
    /// Something the settings keep changed (the folders chosen…): save them.
    pub save: bool,
}

/// The modifier keys down.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl From<winit::keyboard::ModifiersState> for Mods {
    fn from(m: winit::keyboard::ModifiersState) -> Mods {
        Mods { ctrl: m.control_key(), shift: m.shift_key(), alt: m.alt_key() }
    }
}

/// What a page asks of the system and the user, answered at once: the window
/// passes the implementation (`SystemDialogs`, or another in tests).
pub trait Dialogs {
    /// Asks before doing something that cannot be undone from here; true for yes.
    fn confirm(&mut self, title: &str, text: &str) -> bool;
    /// Asks for a folder.
    fn pick_folder(&mut self, title: &str) -> Option<std::path::PathBuf>;
    /// Asks where to save a file: its suggested name, and the kind of file
    /// (a name such as "CSV files" and a pattern such as `*.csv`).
    fn save_file(&mut self, name: &str, kind: (&str, &str), dir: Option<&std::path::Path>) -> Option<std::path::PathBuf>;
    /// Moves a file or folder to the Recycle Bin (the trash on Linux).
    fn recycle(&mut self, path: &std::path::Path) -> Result<(), String>;
}

/// The dialogs of the system, over the window `hwnd` (Windows) or alone (Linux).
pub struct SystemDialogs {
    #[cfg(windows)]
    pub hwnd: rt_sys::win::HWND,
}

#[cfg(windows)]
impl Dialogs for SystemDialogs {
    fn confirm(&mut self, title: &str, text: &str) -> bool {
        use rt_sys::win::dialogs::{message, Answer, Buttons, Icon};
        message(self.hwnd, title, text, Buttons::OkCancel, Icon::Warning) == Answer::Ok
    }
    fn pick_folder(&mut self, title: &str) -> Option<std::path::PathBuf> {
        rt_sys::win::dialogs::pick_folder(self.hwnd, title)
    }
    fn save_file(&mut self, name: &str, kind: (&str, &str), dir: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
        let filter = rt_sys::win::dialogs::filter(&[(kind.0.to_string(), kind.1)]);
        rt_sys::win::dialogs::save_file(self.hwnd, name, dir, &filter)
    }
    fn recycle(&mut self, path: &std::path::Path) -> Result<(), String> {
        rt_sys::win::recycle(self.hwnd, path)
    }
}

#[cfg(not(windows))]
impl Dialogs for SystemDialogs {
    fn confirm(&mut self, title: &str, text: &str) -> bool {
        rt_sys::linux::dialogs::confirm(title, text)
    }
    fn pick_folder(&mut self, title: &str) -> Option<std::path::PathBuf> {
        rt_sys::linux::dialogs::pick_folder(title)
    }
    fn save_file(&mut self, name: &str, _kind: (&str, &str), dir: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
        rt_sys::linux::dialogs::save_file(name, dir)
    }
    fn recycle(&mut self, path: &std::path::Path) -> Result<(), String> {
        rt_sys::recycle(path)
    }
}

impl Effects {
    pub fn save(&mut self) {
        self.save = true;
    }

    pub fn redraw(&mut self) {
        self.redraw = true;
    }

    pub fn menus(&mut self) {
        self.menu_dirty = true;
    }

    pub fn info(&mut self, title: &str, text: &str) {
        self.info = Some((title.to_string(), text.to_string()));
        self.redraw = true;
    }

    /// What was noted so far; the page starts again from nothing.
    pub fn take(&mut self) -> Effects {
        std::mem::take(self)
    }

    pub fn is_empty(&self) -> bool {
        *self == Effects::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taking_empties_the_effects() {
        let mut e = Effects::default();
        assert!(e.is_empty());
        e.info("a", "b");
        e.menus();
        let t = e.take();
        assert!(t.redraw && t.menu_dirty && t.info == Some(("a".into(), "b".into())));
        assert!(e.is_empty());
    }
}
