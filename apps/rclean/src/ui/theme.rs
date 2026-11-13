//! Colours of the window: list, panels, status bar.

use super::gfx::Color;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub dark: bool,
    pub bg: Color,
    pub panel: Color,
    pub text: Color,
    pub dim: Color,
    pub border: Color,
    pub status: Color,
    pub header: Color,
    pub hover: Color,
    pub selection: Color,
    pub accent: Color,
    pub bar_track: Color,
    pub scrollbar: Color,
    pub warn: Color,
    /// What was freed.
    pub good: Color,
}

impl Theme {
    pub fn new(dark: bool, accent: Option<Color>) -> Theme {
        let c = Color::rgb;
        if dark {
            let accent = accent.unwrap_or(c(0x4CC2FF));
            Theme {
                dark,
                bg: c(0x1E1E1E),
                panel: c(0x262626),
                text: c(0xE4E4E4),
                dim: c(0x9A9A9A),
                border: c(0x3A3A3A),
                status: c(0x252526),
                header: c(0x2A2A2D),
                hover: Color::rgba(0xFFFFFF, 0x12),
                selection: accent.with_alpha(0x50),
                accent,
                bar_track: c(0x333333),
                scrollbar: Color::rgba(0xFFFFFF, 0x40),
                warn: c(0xF2B84B),
                good: c(0x6CCB5F),
            }
        } else {
            let accent = accent.unwrap_or(c(0x0067C0));
            Theme {
                dark,
                bg: c(0xFFFFFF),
                panel: c(0xF6F7F9),
                text: c(0x1B1B1B),
                dim: c(0x616161),
                border: c(0xDADADA),
                status: c(0xF0F0F0),
                header: c(0xEEF1F5),
                hover: Color::rgba(0x000000, 0x0C),
                selection: accent.with_alpha(0x38),
                accent,
                bar_track: c(0xE6E8EC),
                scrollbar: Color::rgba(0x000000, 0x40),
                warn: c(0xB26A00),
                good: c(0x0F7B0F),
            }
        }
    }

    /// Colours of the message panel (About, Keyboard shortcuts).
    pub fn dialog_style(&self) -> rt_gfx::dialog::DialogStyle {
        rt_gfx::dialog::DialogStyle { dark: self.dark, text: self.text, dim: self.dim, border: self.border }
    }

    /// Colours of the menus drawn in the window.
    #[cfg(not(windows))]
    pub fn menu_style(&self) -> rt_gfx::menu::MenuStyle {
        rt_gfx::menu::MenuStyle { dark: self.dark, bar: self.status, border: self.border, text: self.text, dim: self.dim }
    }
}
