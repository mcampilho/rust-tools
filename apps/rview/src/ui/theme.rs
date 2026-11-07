//! Colors of the window chrome (status bar, scroll bars, empty state) and
//! of themed documents (text, CSV, Markdown).

use super::gfx::Color;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub dark: bool,
    pub bg: Color,
    pub text: Color,
    pub dim: Color,
    pub border: Color,
    pub status: Color,
    pub scrollbar: Color,
    /// Background around PDF pages.
    pub canvas: Color,
    pub gutter_text: Color,
    pub stripe: Color,
    pub header: Color,
    /// Selected text and cells (translucent: drawn over page images too).
    pub selection: Color,
}

impl Theme {
    pub fn new(dark: bool) -> Theme {
        let c = Color::rgb;
        if dark {
            Theme {
                dark,
                bg: c(0x1E1E1E),
                text: c(0xE2E2E2),
                dim: c(0x9A9A9A),
                border: c(0x3A3A3A),
                status: c(0x252526),
                scrollbar: Color::rgba(0xFFFFFF, 0x40),
                canvas: c(0x2B2B2B),
                gutter_text: c(0x6E7681),
                stripe: c(0x232325),
                header: c(0x2A2A2D),
                selection: Color::rgba(0x3390FF, 0x70),
            }
        } else {
            Theme {
                dark,
                bg: c(0xFFFFFF),
                text: c(0x1B1B1B),
                dim: c(0x616161),
                border: c(0xDADADA),
                status: c(0xF0F0F0),
                scrollbar: Color::rgba(0x000000, 0x40),
                canvas: c(0xE6E6E6),
                gutter_text: c(0x9A9A9A),
                stripe: c(0xF7F8FA),
                header: c(0xEEF1F5),
                selection: Color::rgba(0x3390FF, 0x55),
            }
        }
    }

    /// Colours of the menus drawn in the window.
    #[cfg(not(windows))]
    pub fn menu_style(&self) -> rt_gfx::menu::MenuStyle {
        rt_gfx::menu::MenuStyle { dark: self.dark, bar: self.status, border: self.border, text: self.text, dim: self.dim }
    }
}
