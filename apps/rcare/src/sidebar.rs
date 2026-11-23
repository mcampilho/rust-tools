//! The sidebar: a button that collapses it, and one entry for each page.
//! Open it shows the names; collapsed only the icons. Drawn with `rt-gfx`,
//! like everything in the window; `layout` decides where things go, and the
//! window uses the same [`Layout`] to find what the pointer is on.

use crate::i18n::tr;
use crate::pages::{Glyph, PageInfo};
use rt_gfx::gfx::{Align, FontId, Gfx, Rect};
use rt_ui::theme::Theme;

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The button that collapses or opens the sidebar.
    Toggle,
    /// The entry of a page (index into `PAGES`).
    Page(usize),
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub toggle: Rect,
    pub items: Vec<Rect>,
}

/// How wide the sidebar is.
pub fn width(collapsed: bool, s: f32) -> f32 {
    ((if collapsed { 52.0 } else { 224.0 }) * s).round()
}

/// Where the button and the entries go in `area`, the sidebar's rectangle.
pub fn layout(area: Rect, pages: usize, s: f32) -> Layout {
    let pad = (6.0 * s).round();
    let row = (40.0 * s).round();
    let toggle = Rect::new(area.x + pad, area.y + (8.0 * s).round(), area.w - 2.0 * pad, row);
    let mut y = toggle.bottom() + (10.0 * s).round();
    // Many pages in a low window: the entries get shorter (down to 22 px, the icon's height) so all of them show.
    let gap = (2.0 * s).round();
    let room = area.bottom() - y - pad;
    let item = if pages == 0 { row } else { ((room / pages as f32) - gap).floor().clamp((22.0 * s).round(), row) };
    let items = (0..pages)
        .map(|_| {
            let r = Rect::new(area.x + pad, y, area.w - 2.0 * pad, item);
            y += item + gap;
            r
        })
        .collect();
    Layout { area, toggle, items }
}

pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Hit> {
    if l.toggle.contains(x, y) {
        Some(Hit::Toggle)
    } else {
        l.items.iter().position(|r| r.contains(x, y)).map(Hit::Page)
    }
}

/// What the sidebar shows besides the pages.
#[derive(Clone, Copy, Debug)]
pub struct State {
    /// Only the icons.
    pub collapsed: bool,
    /// The index of the page shown.
    pub selected: usize,
    /// What the pointer is on.
    pub hot: Option<Hit>,
}

/// Draws the sidebar.
pub fn draw(g: &mut Gfx, l: &Layout, th: &Theme, s: f32, state: State, pages: &[PageInfo]) {
    let State { collapsed, selected, hot } = state;
    g.fill_rect(l.area, th.panel);
    g.fill_rect(Rect::new(l.area.right() - 1.0, l.area.y, 1.0, l.area.h), th.border);
    let px = 13.5 * s;
    // The button: three bars.
    if hot == Some(Hit::Toggle) {
        g.fill_rrect(l.toggle, 6.0 * s, th.hover);
    }
    let icon = icon_box(l.toggle, s);
    for k in 0..3 {
        let y = icon.y + icon.h * (0.2 + 0.3 * k as f32) - 1.0 * s;
        g.fill_rrect(Rect::new(icon.x, y, icon.w, (2.0 * s).max(1.0)), 1.0 * s, th.text);
    }
    if !collapsed {
        let r = Rect::new(icon.right() + 14.0 * s, l.toggle.y, l.toggle.right() - icon.right() - 18.0 * s, l.toggle.h);
        g.text_in(&tr("Sidebar"), r, px, FontId::Regular, th.dim, Align::Left);
    }
    for (i, (r, p)) in l.items.iter().zip(pages).enumerate() {
        let on = i == selected;
        if on {
            g.fill_rrect(*r, 6.0 * s, th.selection);
            g.fill_rrect(Rect::new(r.x, r.y + 9.0 * s, 3.0 * s, r.h - 18.0 * s), 1.5 * s, th.accent);
        } else if hot == Some(Hit::Page(i)) {
            g.fill_rrect(*r, 6.0 * s, th.hover);
        }
        let icon = icon_box(*r, s);
        draw_glyph(g, p.glyph, icon, if on { th.accent } else { th.text });
        if !collapsed {
            let t = Rect::new(icon.right() + 14.0 * s, r.y, r.right() - icon.right() - 18.0 * s, r.h);
            let font = if on { FontId::Semibold } else { FontId::Regular };
            g.text_in(&tr(p.label), t, px, font, th.text, Align::Left);
        }
    }
}

/// Where the icon goes in a button: always at the same distance from the
/// left edge, so it does not move when the sidebar collapses.
fn icon_box(r: Rect, s: f32) -> Rect {
    let size = (20.0 * s).round();
    Rect::new(r.x + (10.0 * s).round(), (r.y + (r.h - size) / 2.0).round(), size, size)
}

fn draw_glyph(g: &mut Gfx, glyph: Glyph, r: Rect, c: rt_gfx::gfx::Color) {
    match glyph {
        Glyph::Sparkle => {
            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            let (a, b) = (r.w / 2.0, r.w * 0.12);
            let pts = [
                (cx, cy - a),
                (cx + b, cy - b),
                (cx + a, cy),
                (cx + b, cy + b),
                (cx, cy + a),
                (cx - b, cy + b),
                (cx - a, cy),
                (cx - b, cy - b),
            ];
            g.fill_polygon(&pts, c);
        }
        Glyph::Copies => {
            let w = r.w * 0.62;
            let back = Rect::new(r.x + r.w - w, r.y, w, w * 1.15);
            let front = Rect::new(r.x, r.y + r.h - w * 1.15, w, w * 1.15);
            g.stroke_rrect(back, 2.0, 1.6, c);
            g.fill_rrect(front, 2.0, c);
        }
        Glyph::Remove => {
            // An open box, and an arrow leaving it upwards.
            let (bx, by, bw, bh) = (r.x + r.w * 0.1, r.y + r.h * 0.5, r.w * 0.8, r.h * 0.5);
            g.fill_rrect(Rect::new(bx, by, bw, 2.0), 1.0, c);
            g.fill_rrect(Rect::new(bx, by, 2.0, bh), 1.0, c);
            g.fill_rrect(Rect::new(bx + bw - 2.0, by, 2.0, bh), 1.0, c);
            g.fill_rrect(Rect::new(bx, by + bh - 2.0, bw, 2.0), 1.0, c);
            let cx = r.x + r.w / 2.0;
            g.fill_rrect(Rect::new(cx - 1.0, r.y + r.h * 0.12, 2.0, r.h * 0.6), 1.0, c);
            let a = r.w * 0.22;
            g.fill_polygon(&[(cx, r.y), (cx + a, r.y + a), (cx - a, r.y + a)], c);
        }
        Glyph::Gauge => {
            // A half dial and its needle.
            let (cx, cy, rad) = (r.x + r.w / 2.0, r.y + r.h * 0.78, r.w * 0.48);
            let pts: Vec<(f32, f32)> = (0..=20)
                .map(|k| {
                    let a = (180.0 + 180.0 * k as f32 / 20.0).to_radians();
                    (cx + rad * a.cos(), cy + rad * a.sin())
                })
                .collect();
            g.polyline(&pts, 2.0, c, false);
            let a = (-50.0f32).to_radians();
            g.polyline(&[(cx, cy), (cx + rad * 0.8 * a.cos(), cy + rad * 0.8 * a.sin())], 2.0, c, false);
            g.fill_circle(cx, cy, 2.0, c);
        }
        Glyph::Cogs => {
            // A big cog and a small one: an outline with teeth, a dot in the middle.
            let cog = |g: &mut Gfx, cx: f32, cy: f32, rad: f32, teeth: usize| {
                let pts: Vec<(f32, f32)> = (0..teeth * 4)
                    .map(|k| {
                        let a = (k as f32 + 0.5) * std::f32::consts::TAU / (teeth * 4) as f32;
                        let out = if (k / 2) % 2 == 0 { rad } else { rad * 0.74 };
                        (cx + out * a.cos(), cy + out * a.sin())
                    })
                    .collect();
                g.polyline(&pts, 1.6, c, true);
                g.fill_circle(cx, cy, rad * 0.28, c);
            };
            cog(g, r.x + r.w * 0.38, r.y + r.h * 0.6, r.w * 0.4, 8);
            cog(g, r.x + r.w * 0.8, r.y + r.h * 0.22, r.w * 0.24, 6);
        }
        Glyph::Wrench => {
            // A handle across, and an open jaw at its top end.
            let (x0, y0, x1, y1) = (r.x + r.w * 0.16, r.y + r.h * 0.84, r.x + r.w * 0.66, r.y + r.h * 0.34);
            g.polyline(&[(x0, y0), (x1, y1)], 3.0, c, false);
            let (cx, cy, rad) = (r.x + r.w * 0.72, r.y + r.h * 0.28, r.w * 0.24);
            let pts: Vec<(f32, f32)> = (0..=20)
                .map(|k| {
                    // The ring open towards the top right.
                    let a = (80.0 + 280.0 * k as f32 / 20.0).to_radians();
                    (cx + rad * a.cos(), cy - rad * a.sin())
                })
                .collect();
            g.polyline(&pts, 2.4, c, false);
        }
        Glyph::Monitor => {
            // A screen on its stand.
            let screen = Rect::new(r.x, r.y + r.h * 0.08, r.w, r.h * 0.62);
            g.stroke_rrect(screen, 2.0, 1.6, c);
            let cx = r.x + r.w / 2.0;
            g.fill_rrect(Rect::new(cx - 1.0, screen.bottom(), 2.0, r.h * 0.16), 1.0, c);
            g.fill_rrect(Rect::new(cx - r.w * 0.25, r.y + r.h * 0.88, r.w * 0.5, 2.0), 1.0, c);
        }
        Glyph::Eye => {
            // An eye's outline and its pupil.
            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            let (a, b) = (r.w * 0.5, r.h * 0.32);
            let pts: Vec<(f32, f32)> = (0..=32)
                .map(|k| {
                    let t = std::f32::consts::TAU * k as f32 / 32.0;
                    // A lens: pointed at both ends.
                    let y = b * t.sin() * (1.0 - 0.35 * t.cos().abs());
                    (cx + a * t.cos(), cy + y)
                })
                .collect();
            g.polyline(&pts, 1.6, c, false);
            g.fill_circle(cx, cy, r.w * 0.16, c);
        }
        Glyph::Globe => {
            // A circle, its equator and a meridian's two halves.
            let (cx, cy, rad) = (r.x + r.w / 2.0, r.y + r.h / 2.0, r.w * 0.46);
            let ring = |rx: f32| -> Vec<(f32, f32)> {
                (0..=32)
                    .map(|k| {
                        let a = std::f32::consts::TAU * k as f32 / 32.0;
                        (cx + rx * a.cos(), cy + rad * a.sin())
                    })
                    .collect()
            };
            g.polyline(&ring(rad), 1.6, c, false);
            g.polyline(&ring(rad * 0.42), 1.4, c, false);
            g.polyline(&[(cx - rad, cy), (cx + rad, cy)], 1.4, c, false);
        }
        Glyph::Shield => {
            // A shield's outline, and a tick in it.
            let (x, y, w, h) = (r.x + r.w * 0.12, r.y, r.w * 0.76, r.h);
            let pts = [
                (x + w / 2.0, y),
                (x + w, y + h * 0.16),
                (x + w, y + h * 0.5),
                (x + w * 0.72, y + h * 0.84),
                (x + w / 2.0, y + h),
                (x + w * 0.28, y + h * 0.84),
                (x, y + h * 0.5),
                (x, y + h * 0.16),
            ];
            g.polyline(&pts, 1.8, c, true);
            g.polyline(&[(x + w * 0.27, y + h * 0.5), (x + w * 0.45, y + h * 0.66), (x + w * 0.74, y + h * 0.34)], 2.0, c, false);
        }
        Glyph::Blocks => {
            // Three blocks stacked like a hive's cells: two below, one above.
            let b = r.w * 0.42;
            let gap = r.w * 0.1;
            let y2 = r.y + r.h - b;
            g.stroke_rrect(Rect::new(r.x, y2, b, b), 2.0, 1.6, c);
            g.stroke_rrect(Rect::new(r.x + b + gap, y2, b, b), 2.0, 1.6, c);
            g.fill_rrect(Rect::new(r.x + (b + gap) / 2.0, y2 - gap - b, b, b), 2.0, c);
        }
        Glyph::Puzzle => {
            // A square piece with a knob on top and one on the right.
            let q = r.w * 0.7;
            let body = Rect::new(r.x, r.y + r.h - q, q, q);
            g.fill_rrect(body, 2.0, c);
            let k = r.w * 0.14;
            g.fill_circle(body.x + q / 2.0, body.y - k * 0.4, k, c);
            g.fill_circle(body.right() + k * 0.4, body.y + q / 2.0, k, c);
        }
        Glyph::Power => {
            // A ring open at the top, and a stroke down into it.
            let (cx, cy, rad) = (r.x + r.w / 2.0, r.y + r.h * 0.56, r.w * 0.4);
            let pts: Vec<(f32, f32)> = (0..=24)
                .map(|k| {
                    let a = (-60.0 + 300.0 * k as f32 / 24.0 - 90.0).to_radians();
                    (cx + rad * a.cos(), cy + rad * a.sin())
                })
                .collect();
            g.polyline(&pts, 2.0, c, false);
            g.fill_rrect(Rect::new(cx - 1.0, r.y, 2.0, r.h * 0.5), 1.0, c);
        }
        Glyph::Pulse => {
            let (x, y, w, h) = (r.x, r.y + r.h / 2.0, r.w, r.h);
            let pts = [(x, y), (x + w * 0.28, y), (x + w * 0.4, y - h * 0.38), (x + w * 0.55, y + h * 0.4), (x + w * 0.68, y), (x + w, y)];
            g.polyline(&pts, 2.0, c, false);
        }
        Glyph::Grid => {
            let q = (r.w - 3.0) / 2.0;
            for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                g.fill_rrect(Rect::new(r.x + dx * (q + 3.0), r.y + dy * (q + 3.0), q, q), 2.0, c);
            }
        }
        Glyph::Bars => {
            let bw = r.w * 0.22;
            for (i, h) in [0.55, 1.0, 0.75].iter().enumerate() {
                let bh = r.h * h;
                g.fill_rrect(Rect::new(r.x + i as f32 * (bw + r.w * 0.085), r.y + r.h - bh, bw, bh), 1.5, c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages::PAGES;

    #[test]
    fn collapsed_is_narrower_and_scales() {
        assert!(width(true, 1.0) < width(false, 1.0));
        assert_eq!(width(false, 2.0), 2.0 * width(false, 1.0));
    }

    #[test]
    fn entries_are_found_under_the_pointer() {
        let l = layout(Rect::new(0.0, 0.0, width(false, 1.0), 600.0), PAGES.len() + 2, 1.0);
        assert_eq!(hit(&l, 20.0, l.toggle.y + 5.0), Some(Hit::Toggle));
        for (i, r) in l.items.iter().enumerate() {
            assert_eq!(hit(&l, r.x + 5.0, r.y + 5.0), Some(Hit::Page(i)));
        }
        assert_eq!(hit(&l, 20.0, 590.0), None);
        // The entries stay inside the sidebar and do not overlap.
        for w in l.items.windows(2) {
            assert!(w[0].bottom() <= w[1].y);
        }
        assert!(l.items.iter().all(|r| r.right() <= l.area.right()));
    }

    #[test]
    fn the_icon_does_not_move_when_collapsing() {
        let open = layout(Rect::new(0.0, 0.0, width(false, 1.0), 600.0), 1, 1.0);
        let shut = layout(Rect::new(0.0, 0.0, width(true, 1.0), 600.0), 1, 1.0);
        assert_eq!(icon_box(open.items[0], 1.0).x, icon_box(shut.items[0], 1.0).x);
    }
}
