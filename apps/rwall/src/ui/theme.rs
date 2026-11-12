//! Colours of the settings window: the shared controls' palette
//! (`rt_ui::controls::Palette`), and whether it is dark.

use super::gfx::Color;
use rt_ui::controls::Palette;

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
        let p = Palette::new(dark, accent);
        Theme {
            dark,
            bg: p.bg,
            panel: p.panel,
            text: p.text,
            dim: p.dim,
            border: p.border,
            hover: p.hover,
            accent: p.accent,
            on_accent: p.on_accent,
            field: p.field,
            warn: p.warn,
        }
    }

    /// The colours of the shared controls.
    pub fn palette(&self) -> Palette {
        Palette {
            bg: self.bg,
            panel: self.panel,
            text: self.text,
            dim: self.dim,
            border: self.border,
            hover: self.hover,
            accent: self.accent,
            on_accent: self.on_accent,
            field: self.field,
            warn: self.warn,
        }
    }
}
