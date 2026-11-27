//! The colours of the menu: a coloured frame around a light (or dark) list, as
//! the menus of portable suites. The frame's colour is chosen in Options, and
//! any colour can be replaced in a `theme.ini` file beside the settings.

use super::gfx::Color;
use crate::storage::Store;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    /// The header, the right column and the footer.
    pub frame: Color,
    pub frame_text: Color,
    pub frame_dim: Color,
    pub frame_hover: Color,
    /// The list of programs.
    pub list: Color,
    pub text: Color,
    pub dim: Color,
    pub border: Color,
    pub hover: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub warn: Color,
}

/// The frame colours offered: name (a translation key), light and dark. After
/// them comes the one made from the system's accent.
pub const COLOURS: [(&str, u32, u32); 7] = [
    ("Blue", 0x17325A, 0x101720),
    ("Graphite", 0x3A3F47, 0x17191D),
    ("Green", 0x1E4D36, 0x0F1B15),
    ("Teal", 0x12505A, 0x0D1B1E),
    ("Purple", 0x3E2A63, 0x17121F),
    ("Red", 0x5E2129, 0x1D1012),
    ("Orange", 0x6B3A12, 0x1F160E),
];

/// How many choices of colour there are, the system's included.
pub const CHOICES: u8 = COLOURS.len() as u8 + 1;

/// The name of choice `colour`, untranslated.
pub fn colour_name(colour: u8) -> &'static str {
    COLOURS.get(usize::from(colour)).map_or("System accent", |c| c.0)
}

/// The frame of choice `colour`.
pub fn frame_of(colour: u8, dark: bool, accent: Option<Color>) -> Color {
    match COLOURS.get(usize::from(colour)) {
        Some(&(_, light, night)) => Color::rgb(if dark { night } else { light }),
        // The accent, darkened until white text reads on it.
        None => accent.unwrap_or(Color::rgb(0x2F7FD6)).mix(Color::rgb(0x000000), if dark { 0.82 } else { 0.55 }),
    }
}

/// Black or white, whichever reads on `c`.
fn on(c: Color) -> Color {
    if 299 * u32::from(c.r) + 587 * u32::from(c.g) + 114 * u32::from(c.b) > 150_000 {
        Color::rgb(0x101010)
    } else {
        Color::rgb(0xFFFFFF)
    }
}

/// `#RRGGBB`.
fn parse(text: &str) -> Option<Color> {
    let hex = text.trim().strip_prefix('#')?;
    (hex.len() == 6).then(|| u32::from_str_radix(hex, 16).ok()).flatten().map(Color::rgb)
}

impl Theme {
    pub fn new(dark: bool, accent: Option<Color>, colour: u8) -> Theme {
        let frame = frame_of(colour, dark, accent);
        let accent = accent.unwrap_or(Color::rgb(0x2F7FD6));
        let (list, text, dim, border, hover) = if dark {
            (Color::rgb(0x23262C), Color::rgb(0xE8E8E8), Color::rgb(0x9AA0A8), Color::rgb(0x3A3F47), Color::rgb(0x333843))
        } else {
            (Color::rgb(0xF7F9FC), Color::rgb(0x1B1F24), Color::rgb(0x66707C), Color::rgb(0xD5DBE3), Color::rgb(0xE3EBF6))
        };
        let frame_text = Color::rgb(0xF0F4FA);
        Theme {
            dark,
            frame,
            frame_text,
            frame_dim: frame_text.mix(frame, 0.3),
            frame_hover: frame.mix(Color::rgb(0xFFFFFF), 0.13),
            list,
            text,
            dim,
            border,
            hover,
            accent,
            on_accent: on(accent),
            warn: if dark { Color::rgb(0xF2B84B) } else { Color::rgb(0xA05A00) },
        }
    }

    /// Replaces the colours that `file` (`name=#RRGGBB` lines) gives. A new
    /// frame or accent also brings the colours that go with it.
    pub fn with(mut self, file: &Store) -> Theme {
        let get = |name: &str| file.get(name).and_then(parse);
        if let Some(frame) = get("frame") {
            self.frame = frame;
            self.frame_text = on(frame);
            self.frame_dim = self.frame_text.mix(frame, 0.3);
            self.frame_hover = frame.mix(self.frame_text, 0.13);
        }
        if let Some(accent) = get("accent") {
            self.accent = accent;
            self.on_accent = on(accent);
        }
        for (name, colour) in [
            ("frame_text", &mut self.frame_text),
            ("frame_dim", &mut self.frame_dim),
            ("frame_hover", &mut self.frame_hover),
            ("list", &mut self.list),
            ("text", &mut self.text),
            ("dim", &mut self.dim),
            ("border", &mut self.border),
            ("hover", &mut self.hover),
            ("on_accent", &mut self.on_accent),
            ("warn", &mut self.warn),
        ] {
            if let Some(c) = get(name) {
                *colour = c;
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_replaces_the_colours_it_names() {
        let base = Theme::new(false, None, 0);
        let t = base.with(&Store::parse("# mine\nframe=#FFFFFF\nlist = #102030\ntext=red\nwarn=#12345\n"));
        assert_eq!((t.frame, t.list), (Color::rgb(0xFFFFFF), Color::rgb(0x102030)));
        // On a white frame the text turns dark; what is not a colour is ignored.
        assert_eq!(t.frame_text, Color::rgb(0x101010));
        assert_eq!((t.text, t.warn), (base.text, base.warn));
        assert_eq!(colour_name(CHOICES - 1), "System accent");
    }
}
