//! Simple line icons drawn as vector paths (original artwork, no icon font
//! dependency). Each icon is designed on a 16x16 grid and scaled to `size`.

use super::gfx::{Color, Gfx, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Menu,
    History,
    Trash,
    Backspace,
    ChevronDown,
    ChevronUp,
    KeepOnTop,
    BackToFull,
    Calendar,
    FullKeypad,
    BitKeypad,
    Triangle,
    Function,
    Settings,
}

pub fn draw(g: &mut Gfx, icon: Icon, r: Rect, size: f32, c: Color) {
    let k = size / 16.0;
    let ox = (r.x + (r.w - size) / 2.0).round();
    let oy = (r.y + (r.h - size) / 2.0).round();
    let p = |x: f32, y: f32| (ox + x * k, oy + y * k);
    let w = (1.1 * k).max(1.0);
    match icon {
        Icon::Menu => {
            for y in [3.5, 8.0, 12.5] {
                g.polyline(&[p(1.0, y), p(15.0, y)], w, c, false);
            }
        }
        Icon::History => {
            let pts: Vec<_> = (0..=28)
                .map(|i| {
                    let a = -2.4 + i as f32 * (5.2 / 28.0);
                    p(8.0 + 6.5 * a.cos(), 8.0 + 6.5 * a.sin())
                })
                .collect();
            g.polyline(&pts, w, c, false);
            // arrow head at the start of the arc
            let s = pts[0];
            g.polyline(&[(s.0 - 2.5 * k, s.1 - 0.5 * k), s, (s.0 + 0.3 * k, s.1 - 2.8 * k)], w, c, false);
            g.polyline(&[p(8.0, 4.5), p(8.0, 8.0), p(10.5, 10.0)], w, c, false);
        }
        Icon::Trash => {
            g.polyline(&[p(2.0, 3.5), p(14.0, 3.5)], w, c, false);
            g.polyline(&[p(6.0, 3.5), p(6.0, 1.5), p(10.0, 1.5), p(10.0, 3.5)], w, c, false);
            g.polyline(&[p(3.5, 3.5), p(4.5, 15.0), p(11.5, 15.0), p(12.5, 3.5)], w, c, false);
            g.polyline(&[p(6.5, 6.5), p(6.7, 12.5)], w, c, false);
            g.polyline(&[p(9.5, 6.5), p(9.3, 12.5)], w, c, false);
        }
        Icon::Backspace => {
            g.polyline(&[p(5.0, 3.0), p(15.0, 3.0), p(15.0, 13.0), p(5.0, 13.0), p(0.8, 8.0)], w, c, true);
            g.polyline(&[p(7.5, 5.5), p(12.5, 10.5)], w, c, false);
            g.polyline(&[p(12.5, 5.5), p(7.5, 10.5)], w, c, false);
        }
        Icon::ChevronDown => g.polyline(&[p(2.0, 5.5), p(8.0, 11.0), p(14.0, 5.5)], w * 1.2, c, false),
        Icon::ChevronUp => g.polyline(&[p(2.0, 10.5), p(8.0, 5.0), p(14.0, 10.5)], w * 1.2, c, false),
        Icon::KeepOnTop => {
            g.polyline(&[p(9.0, 1.5), p(1.5, 1.5), p(1.5, 14.5), p(14.5, 14.5), p(14.5, 7.0)], w, c, false);
            g.polyline(&[p(6.5, 9.5), p(14.0, 2.0)], w, c, false);
            g.polyline(&[p(9.5, 2.0), p(14.0, 2.0), p(14.0, 6.5)], w, c, false);
        }
        Icon::BackToFull => {
            g.polyline(&[p(9.0, 1.5), p(1.5, 1.5), p(1.5, 14.5), p(14.5, 14.5), p(14.5, 7.0)], w, c, false);
            g.polyline(&[p(14.0, 2.0), p(6.5, 9.5)], w, c, false);
            g.polyline(&[p(6.5, 5.0), p(6.5, 9.5), p(11.0, 9.5)], w, c, false);
        }
        Icon::Calendar => {
            g.polyline(&[p(1.5, 2.5), p(14.5, 2.5), p(14.5, 14.5), p(1.5, 14.5)], w, c, true);
            g.polyline(&[p(1.5, 5.5), p(14.5, 5.5)], w, c, false);
            for (x, y) in [(5.0, 8.5), (8.0, 8.5), (11.0, 8.5), (5.0, 11.5), (8.0, 11.5)] {
                let (cx, cy) = p(x, y);
                g.fill_circle(cx, cy, 0.9 * k, c);
            }
        }
        Icon::FullKeypad => {
            for row in 0..3 {
                for col in 0..3 {
                    let (cx, cy) = p(3.0 + col as f32 * 5.0, 3.0 + row as f32 * 5.0);
                    g.fill_circle(cx, cy, 1.3 * k, c);
                }
            }
        }
        Icon::BitKeypad => {
            for row in 0..4 {
                for col in 0..4 {
                    let (cx, cy) = p(2.0 + col as f32 * 4.0, 2.0 + row as f32 * 4.0);
                    let rad = if (row + col) % 3 == 0 { 1.3 } else { 0.7 };
                    g.fill_circle(cx, cy, rad * k, c);
                }
            }
        }
        Icon::Triangle => {
            g.polyline(&[p(1.5, 14.0), p(14.5, 14.0), p(1.5, 3.0)], w, c, true);
            g.polyline(&[p(1.5, 11.0), p(4.5, 11.0), p(4.5, 14.0)], w * 0.8, c, false);
        }
        Icon::Function => {
            // stylised "f"
            g.polyline(
                &[
                    p(12.0, 2.5),
                    p(10.0, 1.5),
                    p(8.0, 2.5),
                    p(7.0, 6.0),
                    p(5.5, 13.5),
                    p(4.0, 14.5),
                    p(2.0, 13.8),
                ],
                w,
                c,
                false,
            );
            g.polyline(&[p(4.5, 6.0), p(11.0, 6.0)], w, c, false);
        }
        Icon::Settings => {
            let (cx, cy) = p(8.0, 8.0);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                g.polyline(
                    &[
                        (cx + 5.0 * k * a.cos(), cy + 5.0 * k * a.sin()),
                        (cx + 7.0 * k * a.cos(), cy + 7.0 * k * a.sin()),
                    ],
                    w * 1.6,
                    c,
                    false,
                );
            }
            let ring: Vec<_> = (0..=24)
                .map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 24.0;
                    (cx + 5.0 * k * a.cos(), cy + 5.0 * k * a.sin())
                })
                .collect();
            g.polyline(&ring, w, c, true);
            let inner: Vec<_> = (0..=16)
                .map(|i| {
                    let a = i as f32 * std::f32::consts::TAU / 16.0;
                    (cx + 2.0 * k * a.cos(), cy + 2.0 * k * a.sin())
                })
                .collect();
            g.polyline(&inner, w, c, true);
        }
    }
}
