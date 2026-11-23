//! A message over the window (About, Keyboard shortcuts, the help): a panel
//! with a title, text and OK, drawn by `rt_gfx::dialog::draw_panel` — or a
//! question with Yes and No (drawn in the window's theme, unlike Windows' own
//! message box, which has no dark mode).

use rt_gfx::dialog::{draw_panel, DialogStyle};
use rt_gfx::{Gfx, Rect};
use winit::keyboard::{Key, NamedKey};

pub struct Message {
    pub title: String,
    pub text: String,
    /// The buttons: (label, whether it is the main one). A message has OK; a question Yes and No.
    buttons: Vec<(String, bool)>,
    /// Where the panel and its buttons were last drawn.
    panel: Rect,
    rects: Vec<Rect>,
}

impl Message {
    pub fn new(title: &str, text: &str) -> Message {
        Message {
            title: title.to_string(),
            text: text.to_string(),
            buttons: vec![("OK".into(), true)],
            panel: Rect::default(),
            rects: Vec::new(),
        }
    }

    /// A question: `yes` (the main button) and `no`.
    pub fn question(title: &str, text: &str, yes: &str, no: &str) -> Message {
        Message { buttons: vec![(yes.to_string(), true), (no.to_string(), false)], ..Message::new(title, text) }
    }

    pub fn is_question(&self) -> bool {
        self.buttons.len() == 2
    }

    /// The buttons' labels, in order (for screen readers).
    pub fn labels(&self) -> Vec<&str> {
        self.buttons.iter().map(|(l, _)| l.as_str()).collect()
    }

    /// Draws it over what is in `g`, centred in `full`.
    pub fn draw(&mut self, g: &mut Gfx, full: Rect, st: &DialogStyle, s: f32) {
        let buttons: Vec<(&str, bool)> = self.buttons.iter().map(|(l, m)| (l.as_str(), *m)).collect();
        (self.panel, self.rects) = draw_panel(g, full, st, s, &self.title, &self.text, &buttons);
    }

    /// A message: a click there closes it (OK, or anywhere outside the panel).
    pub fn closes_at(&self, x: f32, y: f32) -> bool {
        self.rects.first().is_some_and(|r| r.contains(x, y)) || !self.panel.contains(x, y)
    }

    /// A question: the answer a click gives (Some(true) Yes, Some(false) No); None elsewhere.
    pub fn answer_at(&self, x: f32, y: f32) -> Option<bool> {
        self.rects.iter().position(|r| r.contains(x, y)).map(|i| i == 0)
    }

    /// A key that closes a message: Esc, Enter or Space.
    pub fn closes_on(key: &Key) -> bool {
        matches!(key, Key::Named(NamedKey::Escape | NamedKey::Enter | NamedKey::Space))
    }

    /// A question's answer by key: Enter is Yes, Esc is No.
    pub fn answer_on(key: &Key) -> Option<bool> {
        match key {
            Key::Named(NamedKey::Enter) => Some(true),
            Key::Named(NamedKey::Escape) => Some(false),
            _ => None,
        }
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
        m.rects = vec![Rect::new(240.0, 160.0, 50.0, 30.0)];
        assert!(m.closes_at(250.0, 170.0), "OK");
        assert!(m.closes_at(10.0, 10.0), "outside");
        assert!(!m.closes_at(150.0, 120.0), "on the text");
        assert!(Message::closes_on(&Key::Named(NamedKey::Escape)));
        assert!(!Message::closes_on(&Key::Named(NamedKey::Tab)));
        assert_eq!(m.spoken_text(), "x  y");
        assert!(!m.is_question());
    }

    #[test]
    fn answering() {
        let mut q = Message::question("Clean", "Delete?", "Yes", "No");
        assert!(q.is_question() && q.labels() == ["Yes", "No"]);
        q.panel = Rect::new(100.0, 100.0, 300.0, 120.0);
        q.rects = vec![Rect::new(200.0, 180.0, 88.0, 30.0), Rect::new(296.0, 180.0, 88.0, 30.0)];
        assert_eq!(q.answer_at(210.0, 190.0), Some(true));
        assert_eq!(q.answer_at(300.0, 190.0), Some(false));
        assert_eq!(q.answer_at(10.0, 10.0), None, "outside answers nothing");
        assert_eq!(Message::answer_on(&Key::Named(NamedKey::Enter)), Some(true));
        assert_eq!(Message::answer_on(&Key::Named(NamedKey::Escape)), Some(false));
        assert_eq!(Message::answer_on(&Key::Named(NamedKey::Space)), None);
    }
}
