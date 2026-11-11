//! Colours of the settings window.

use super::gfx::Color;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub dark: bool,
    pub bg: Color,
    pub panel: Color,
    pub text: Color,
    pub dim: Color,
    pub border: Color,
    pub hover: Color,
    pub accent: Color,
    /// Text on the accent colour.
    pub on_accent: Color,
    pub field: Color,
    pub warn: Color,
}

impl Theme {
    pub fn new(dark: bool, accent: Option<Color>) -> Theme {
        let c = Color::rgb;
        if dark {
            let accent = accent.unwrap_or(c(0x4CC2FF));
            Theme {
                dark,
                bg: c(0x1E1E1E),
                panel: c(0x2A2A2A),
                text: c(0xE4E4E4),
                dim: c(0x9A9A9A),
                border: c(0x404040),
                hover: Color::rgba(0xFFFFFF, 0x14),
                accent,
                on_accent: c(0x101010),
                field: c(0x161616),
                warn: c(0xF2B84B),
            }
        } else {
            let accent = accent.unwrap_or(c(0x0067C0));
            Theme {
                dark,
                bg: c(0xF3F3F3),
                panel: c(0xFFFFFF),
                text: c(0x1B1B1B),
                dim: c(0x616161),
                border: c(0xD6D6D6),
                hover: Color::rgba(0x000000, 0x0C),
                accent,
                on_accent: c(0xFFFFFF),
                field: c(0xFFFFFF),
                warn: c(0xB26A00),
            }
        }
    }
}
