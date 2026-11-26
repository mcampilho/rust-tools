//! The colours of the menu: a deep blue frame around a light (or dark) list,
//! as the menus of portable suites.

use super::gfx::Color;

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

impl Theme {
    pub fn new(dark: bool, accent: Option<Color>) -> Theme {
        let accent = accent.unwrap_or(Color::rgb(0x2F7FD6));
        // Text over the accent: black on a light one.
        let light = 299 * accent.r as u32 + 587 * accent.g as u32 + 114 * accent.b as u32 > 150_000;
        let on_accent = if light { Color::rgb(0x101010) } else { Color::rgb(0xFFFFFF) };
        let (list, text, dim, border, hover) = if dark {
            (Color::rgb(0x23262C), Color::rgb(0xE8E8E8), Color::rgb(0x9AA0A8), Color::rgb(0x3A3F47), Color::rgb(0x333843))
        } else {
            (Color::rgb(0xF7F9FC), Color::rgb(0x1B1F24), Color::rgb(0x66707C), Color::rgb(0xD5DBE3), Color::rgb(0xE3EBF6))
        };
        Theme {
            dark,
            frame: if dark { Color::rgb(0x101720) } else { Color::rgb(0x17325A) },
            frame_text: Color::rgb(0xF0F4FA),
            frame_dim: Color::rgb(0xA6B6CC),
            frame_hover: if dark { Color::rgb(0x243246) } else { Color::rgb(0x2B4F85) },
            list,
            text,
            dim,
            border,
            hover,
            accent,
            on_accent,
            warn: if dark { Color::rgb(0xF2B84B) } else { Color::rgb(0xA05A00) },
        }
    }
}
