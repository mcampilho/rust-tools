//! Light and dark palettes modelled on Windows 11 Fluent colors.

use super::gfx::Color;

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub dark: bool,
    pub bg: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub text_disabled: Color,
    pub num: Color,
    pub num_hover: Color,
    pub num_pressed: Color,
    pub op: Color,
    pub op_hover: Color,
    pub op_pressed: Color,
    pub subtle_hover: Color,
    pub subtle_pressed: Color,
    pub border: Color,
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_pressed: Color,
    pub accent_text: Color,
    /// Accent used for text/indicators on the window background.
    pub accent_fg: Color,
    pub flyout: Color,
    pub flyout_border: Color,
    pub scrim: Color,
    pub selected: Color,
    pub focus: Color,
    pub divider: Color,
}

/// Windows 11 default accent (blue) used when the system accent is unknown.
pub const DEFAULT_ACCENT: Color = Color::rgb(0x0078D4);

fn lighten(c: Color, t: f32) -> Color {
    c.mix(Color::rgb(0xFFFFFF), t)
}

fn darken(c: Color, t: f32) -> Color {
    c.mix(Color::rgb(0x000000), t)
}

fn luminance(c: Color) -> f32 {
    let f = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
}

impl Theme {
    pub fn new(dark: bool, accent: Color) -> Theme {
        if dark {
            // Windows uses the lighter accent shades on dark backgrounds.
            let a = lighten(accent, 0.45);
            let text_on = if luminance(a) > 0.35 {
                Color::rgb(0x000000)
            } else {
                Color::rgb(0xFFFFFF)
            };
            Theme {
                dark,
                bg: Color::rgb(0x202020),
                text: Color::rgb(0xFFFFFF),
                text_secondary: Color::rgb(0xC5C5C5),
                text_disabled: Color::rgb(0x6E6E6E),
                num: Color::rgb(0x3B3B3B),
                num_hover: Color::rgb(0x454545),
                num_pressed: Color::rgb(0x323232),
                op: Color::rgb(0x323232),
                op_hover: Color::rgb(0x3C3C3C),
                op_pressed: Color::rgb(0x2B2B2B),
                subtle_hover: Color::rgba(0xFFFFFF, 0x10),
                subtle_pressed: Color::rgba(0xFFFFFF, 0x0A),
                border: Color::rgba(0xFFFFFF, 0x0C),
                accent: a,
                accent_hover: a.mix(Color::rgb(0x202020), 0.1),
                accent_pressed: a.mix(Color::rgb(0x202020), 0.2),
                accent_text: text_on,
                accent_fg: a,
                flyout: Color::rgb(0x2C2C2C),
                flyout_border: Color::rgba(0x000000, 0x60),
                scrim: Color::rgba(0x000000, 0x50),
                selected: Color::rgba(0xFFFFFF, 0x0F),
                focus: Color::rgb(0xFFFFFF),
                divider: Color::rgba(0xFFFFFF, 0x15),
            }
        } else {
            let a = darken(accent, 0.15);
            let text_on = if luminance(a) > 0.35 {
                Color::rgb(0x000000)
            } else {
                Color::rgb(0xFFFFFF)
            };
            Theme {
                dark,
                bg: Color::rgb(0xF3F3F3),
                text: Color::rgb(0x1A1A1A),
                text_secondary: Color::rgb(0x5D5D5D),
                text_disabled: Color::rgb(0xA0A0A0),
                num: Color::rgb(0xFFFFFF),
                num_hover: Color::rgb(0xF7F7F7),
                num_pressed: Color::rgb(0xF0F0F0),
                op: Color::rgb(0xF9F9F9),
                op_hover: Color::rgb(0xF0F0F0),
                op_pressed: Color::rgb(0xEAEAEA),
                subtle_hover: Color::rgba(0x000000, 0x0A),
                subtle_pressed: Color::rgba(0x000000, 0x06),
                border: Color::rgba(0x000000, 0x10),
                accent: a,
                accent_hover: a.mix(Color::rgb(0xF3F3F3), 0.1),
                accent_pressed: a.mix(Color::rgb(0xF3F3F3), 0.2),
                accent_text: text_on,
                accent_fg: a,
                flyout: Color::rgb(0xF9F9F9),
                flyout_border: Color::rgba(0x000000, 0x30),
                scrim: Color::rgba(0x000000, 0x20),
                selected: Color::rgba(0x000000, 0x08),
                focus: Color::rgb(0x000000),
                divider: Color::rgba(0x000000, 0x14),
            }
        }
    }
}
