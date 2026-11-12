//! Controls drawn in the window, for settings pages and dialogs: headings,
//! notes, buttons, check boxes and radio buttons, text boxes, steppers and
//! rows of buttons. [`Painter`] draws them and remembers where each one is
//! with the app's own click type `H`, so a click is tested against exactly
//! what was drawn ([`Painter::hit_at`]).
//!
//! Sizes are given at 100 % and multiplied by the scale.

use rt_gfx::dialog::{ellipsize, wrap_words};
use rt_gfx::{Align, Color, FontId, Gfx, Rect};

/// The colours the controls use.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color,
    /// Cards, buttons.
    pub panel: Color,
    pub text: Color,
    pub dim: Color,
    pub border: Color,
    /// Laid over what the pointer is on.
    pub hover: Color,
    pub accent: Color,
    /// Text on the accent colour.
    pub on_accent: Color,
    /// Text boxes, unticked boxes.
    pub field: Color,
    pub warn: Color,
}

impl Palette {
    /// The Windows 11 look, light or dark, with the system's accent colour.
    pub fn new(dark: bool, accent: Option<Color>) -> Palette {
        let c = Color::rgb;
        if dark {
            Palette {
                bg: c(0x1E1E1E),
                panel: c(0x2A2A2A),
                text: c(0xE4E4E4),
                dim: c(0x9A9A9A),
                border: c(0x404040),
                hover: Color::rgba(0xFFFFFF, 0x14),
                accent: accent.unwrap_or(c(0x4CC2FF)),
                on_accent: c(0x101010),
                field: c(0x161616),
                warn: c(0xF2B84B),
            }
        } else {
            Palette {
                bg: c(0xF3F3F3),
                panel: c(0xFFFFFF),
                text: c(0x1B1B1B),
                dim: c(0x616161),
                border: c(0xD6D6D6),
                hover: Color::rgba(0x000000, 0x0C),
                accent: accent.unwrap_or(c(0x0067C0)),
                on_accent: c(0xFFFFFF),
                field: c(0xFFFFFF),
                warn: c(0xB26A00),
            }
        }
    }
}

/// A button of a row: text, what a click does, enabled, primary (filled
/// with the accent colour, or "on" in a row of choices).
pub type Button<H> = (String, H, bool, bool);

pub struct Painter<'a, H> {
    pub g: &'a mut Gfx,
    pub t: &'a Palette,
    /// The scale (1.0 at 96 dpi).
    pub s: f32,
    /// What the pointer is on.
    pub hot: Option<H>,
    pub hits: Vec<(Rect, H)>,
    /// Clicks count only inside this (the part of the window that scrolls).
    pub full: Rect,
}

impl<'a, H: Copy + PartialEq> Painter<'a, H> {
    pub fn new(g: &'a mut Gfx, t: &'a Palette, s: f32, hot: Option<H>, full: Rect) -> Self {
        Painter { g, t, s, hot, hits: Vec::new(), full }
    }

    pub fn px(&self, v: f32) -> f32 {
        v * self.s
    }

    /// Remembers that a click in `r` (the part inside `full`) means `h`.
    pub fn hit(&mut self, r: Rect, h: H) {
        let vis = r.intersect(&self.full);
        if vis.w > 0.0 && vis.h > 0.0 {
            self.hits.push((vis, h));
        }
    }

    /// What a click at (x, y) lands on: the last control drawn there.
    pub fn hit_at(hits: &[(Rect, H)], x: f32, y: f32) -> Option<H> {
        hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h)
    }

    /// One line of text, cut with "…" to fit; returns its width.
    pub fn label(&mut self, text: &str, r: Rect, px: f32, font: FontId, c: Color) -> f32 {
        let s = ellipsize(self.g, text, r.w, self.px(px), font);
        self.g.text_in(&s, r, self.px(px), font, c, Align::Left)
    }

    pub fn heading(&mut self, text: &str, x: f32, y: f32, w: f32) {
        let r = Rect::new(x, y, w, self.px(24.0));
        let c = self.t.text;
        self.label(text, r, 13.5, FontId::Semibold, c);
    }

    /// Up to three lines of dim (or warning) text, wrapped; returns the new `y`.
    pub fn note(&mut self, text: &str, x: f32, mut y: f32, w: f32, warn: bool) -> f32 {
        let c = if warn { self.t.warn } else { self.t.dim };
        let px = self.px(11.5);
        for l in wrap_words(self.g, text, w, px, FontId::Regular).iter().take(3) {
            self.label(l, Rect::new(x, y, w, self.px(18.0)), 11.5, FontId::Regular, c);
            y += self.px(18.0);
        }
        y
    }

    pub fn button(&mut self, r: Rect, text: &str, h: H, enabled: bool, primary: bool) {
        let t = *self.t;
        let rad = self.px(4.0);
        let hot = enabled && self.hot == Some(h);
        if primary && enabled {
            let bg = if hot { t.accent.mix(t.bg, 0.15) } else { t.accent };
            self.g.fill_rrect(r, rad, bg);
        } else {
            self.g.fill_rrect(r, rad, t.panel);
            if hot {
                self.g.fill_rrect(r, rad, t.hover);
            }
            self.g.stroke_rrect(r.inset(0.5), rad, 1.0, t.border);
        }
        let c = if !enabled {
            t.dim
        } else if primary {
            t.on_accent
        } else {
            t.text
        };
        let px = self.px(12.5);
        let label = ellipsize(self.g, text, r.w - self.px(12.0), px, FontId::Regular);
        self.g.text_in(&label, r, px, FontId::Regular, c, Align::Center);
        if enabled {
            self.hit(r, h);
        }
    }

    /// The width of a button for `text` (at least `min`).
    pub fn text_width(&mut self, text: &str, min: f32) -> f32 {
        (self.g.measure(text, self.px(12.5), FontId::Regular) + self.px(24.0)).max(self.px(min))
    }

    /// A check box (or a radio button) with its text; returns the row's width.
    #[allow(clippy::too_many_arguments)]
    pub fn choice(&mut self, x: f32, y: f32, w: f32, text: &str, on: bool, radio: bool, h: H) -> f32 {
        let t = *self.t;
        let row = Rect::new(x, y, w, self.px(26.0));
        let b = self.px(16.0);
        let bx = Rect::new(x + self.px(2.0), row.center_y() - b / 2.0, b, b);
        if self.hot == Some(h) {
            self.g.fill_rrect(Rect::new(x - self.px(4.0), y, w + self.px(8.0), row.h), self.px(4.0), t.hover);
        }
        if radio {
            let (cx, cy) = (bx.x + b / 2.0, bx.y + b / 2.0);
            self.g.fill_circle(cx, cy, b / 2.0, if on { t.accent } else { t.border });
            self.g.fill_circle(cx, cy, b / 2.0 - self.px(1.2), t.field);
            if on {
                self.g.fill_circle(cx, cy, b / 2.0 - self.px(4.5), t.accent);
            }
        } else if on {
            self.g.fill_rrect(bx, self.px(3.0), t.accent);
            let (x0, y0) = (bx.x, bx.y);
            let pts = [(x0 + b * 0.22, y0 + b * 0.52), (x0 + b * 0.42, y0 + b * 0.72), (x0 + b * 0.78, y0 + b * 0.3)];
            self.g.polyline(&pts, self.px(1.8), t.on_accent, false);
        } else {
            self.g.fill_rrect(bx, self.px(3.0), t.field);
            self.g.stroke_rrect(bx.inset(0.5), self.px(3.0), 1.0, t.dim);
        }
        let tx = bx.right() + self.px(8.0);
        let tw = self.label(text, Rect::new(tx, y, w - (tx - x), row.h), 12.5, FontId::Regular, t.text);
        let used = tx - x + tw + self.px(6.0);
        self.hit(Rect::new(x, y, used.min(w), row.h), h);
        used
    }

    /// A one-line text box; while `focus`, the end of the text stays in
    /// view, with the caret after it.
    pub fn field(&mut self, r: Rect, value: &str, placeholder: &str, h: H, focus: bool) {
        let t = *self.t;
        let rad = self.px(4.0);
        self.g.fill_rrect(r, rad, t.field);
        self.g.stroke_rrect(r.inset(0.5), rad, if focus { 1.5 } else { 1.0 }, if focus { t.accent } else { t.border });
        let inner = Rect::new(r.x + self.px(7.0), r.y, r.w - self.px(14.0), r.h);
        let px = self.px(12.5);
        if value.is_empty() && !focus {
            self.label(placeholder, inner, 12.5, FontId::Italic, t.dim);
        } else {
            let mut shown = value.to_string();
            while !shown.is_empty() && self.g.measure(&shown, px, FontId::Regular) > inner.w - self.px(4.0) {
                shown.remove(0);
            }
            let w = self.g.text_in(&shown, inner, px, FontId::Regular, t.text, Align::Left);
            if focus {
                let cx = (inner.x + w + self.px(1.0)).round();
                self.g.fill_rect(Rect::new(cx, r.y + self.px(5.0), self.px(1.0).max(1.0), r.h - self.px(10.0)), t.text);
            }
        }
        self.hit(r, h);
    }

    /// "[−] value [+]"; returns its width.
    pub fn stepper(&mut self, x: f32, y: f32, value: &str, less: H, more: H) -> f32 {
        let b = self.px(26.0);
        self.button(Rect::new(x, y, b, b), "−", less, true, false);
        let vw = self.g.measure(value, self.px(12.5), FontId::Semibold).max(self.px(28.0)) + self.px(12.0);
        let t = self.t.text;
        self.g.text_in(value, Rect::new(x + b, y, vw, b), self.px(12.5), FontId::Semibold, t, Align::Center);
        self.button(Rect::new(x + b + vw, y, b, b), "+", more, true, false);
        2.0 * b + vw
    }

    /// Buttons side by side in equal shares of `w`, each at most `max_each`
    /// wide.
    pub fn segments(&mut self, x: f32, y: f32, w: f32, buttons: &[Button<H>], max_each: f32) {
        let n = buttons.len() as f32;
        let gap = self.px(6.0);
        let seg_w = ((w - (n - 1.0) * gap) / n).min(self.px(max_each));
        for (i, (text, h, enabled, primary)) in buttons.iter().enumerate() {
            let r = Rect::new(x + i as f32 * (seg_w + gap), y, seg_w, self.px(28.0));
            self.button(r, text, *h, *enabled, *primary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_land_on_what_was_drawn() {
        let Ok(mut g) = Gfx::new() else { return };
        g.resize(400, 300);
        let pal = Palette::new(false, None);
        let mut p = Painter::new(&mut g, &pal, 1.0, Some(2u8), Rect::new(0.0, 0.0, 400.0, 200.0));
        p.button(Rect::new(10.0, 10.0, 100.0, 28.0), "OK", 1u8, true, true);
        p.choice(10.0, 50.0, 200.0, "Seconds", true, false, 2);
        p.field(Rect::new(10.0, 90.0, 200.0, 26.0), "Lisboa", "", 3, true);
        p.stepper(10.0, 130.0, "5 min", 4, 5);
        p.button(Rect::new(10.0, 170.0, 100.0, 28.0), "Off", 6, false, false);
        // Below `full`: drawn, but not clickable.
        p.button(Rect::new(10.0, 250.0, 100.0, 28.0), "Hidden", 7, true, false);
        let hits = std::mem::take(&mut p.hits);
        assert_eq!(Painter::hit_at(&hits, 50.0, 20.0), Some(1));
        assert_eq!(Painter::hit_at(&hits, 30.0, 60.0), Some(2));
        assert_eq!(Painter::hit_at(&hits, 100.0, 100.0), Some(3));
        assert_eq!(Painter::hit_at(&hits, 20.0, 140.0), Some(4));
        assert_eq!(Painter::hit_at(&hits, 50.0, 180.0), None, "disabled");
        assert_eq!(Painter::hit_at(&hits, 50.0, 260.0), None, "outside");
    }
}
