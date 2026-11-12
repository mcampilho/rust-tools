//! A message over the window (About, Keyboard shortcuts, the help): a panel
//! with a title, text and OK, drawn by `rt_gfx::dialog::draw_message`.

use rt_gfx::dialog::{draw_message, DialogStyle};
use rt_gfx::{Gfx, Rect};
use winit::keyboard::{Key, NamedKey};

pub struct Message {
    pub title: String,
    pub text: String,
    /// Where the panel and its OK button were last drawn.
    panel: Rect,
    ok: Rect,
}

impl Message {
    pub fn new(title: &str, text: &str) -> Message {
        Message { title: title.to_string(), text: text.to_string(), panel: Rect::default(), ok: Rect::default() }
    }

    /// Draws it over what is in `g`, centred in `full`.
    pub fn draw(&mut self, g: &mut Gfx, full: Rect, st: &DialogStyle, s: f32) {
        (self.panel, self.ok) = draw_message(g, full, st, s, &self.title, &self.text);
    }

    /// A click there closes it: OK, or anywhere outside the panel.
    pub fn closes_at(&self, x: f32, y: f32) -> bool {
        self.ok.contains(x, y) || !self.panel.contains(x, y)
    }

    /// A key that closes it: Esc, Enter or Space.
    pub fn closes_on(key: &Key) -> bool {
        matches!(key, Key::Named(NamedKey::Escape | NamedKey::Enter | NamedKey::Space))
    }

    /// The panel's text for screen readers (tabs as spaces).
    pub fn spoken_text(&self) -> String {
        self.text.replace('\t', "  ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing() {
        let mut m = Message::new("About", "x\ty");
        m.panel = Rect::new(100.0, 100.0, 200.0, 100.0);
        m.ok = Rect::new(240.0, 160.0, 50.0, 30.0);
        assert!(m.closes_at(250.0, 170.0), "OK");
        assert!(m.closes_at(10.0, 10.0), "outside");
        assert!(!m.closes_at(150.0, 120.0), "on the text");
        assert!(Message::closes_on(&Key::Named(NamedKey::Escape)));
        assert!(!Message::closes_on(&Key::Named(NamedKey::Tab)));
        assert_eq!(m.spoken_text(), "x  y");
    }
}
