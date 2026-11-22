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
}

impl Effects {
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
