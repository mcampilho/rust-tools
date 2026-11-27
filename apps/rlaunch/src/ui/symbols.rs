//! The few symbols of the menu drawn as lines, for systems without the Windows
//! icon font. Each is told apart by the code of that font's glyph.

use super::gfx::{Color, Gfx, Rect};

/// Draws the symbol of `glyph` in the square `r`, with lines `w` wide.
pub fn draw(g: &mut Gfx, glyph: &str, r: Rect, w: f32, c: Color) {
    // A point of the square, from (0, 0) to (1, 1).
    let p = |x: f32, y: f32| (r.x + x * r.w, r.y + y * r.h);
    let line = |g: &mut Gfx, points: &[(f32, f32)], closed: bool| {
        let at: Vec<(f32, f32)> = points.iter().map(|&(x, y)| p(x, y)).collect();
        g.polyline(&at, w, c, closed);
    };
    let ring = |g: &mut Gfx, x: f32, y: f32, radius: f32| {
        let d = radius * r.w;
        let (cx, cy) = p(x, y);
        g.stroke_rrect(Rect::new(cx - d, cy - d, 2.0 * d, 2.0 * d), d, w, c);
    };
    match glyph {
        // Documents: a page with a folded corner.
        "\u{E8A5}" => {
            line(g, &[(0.2, 0.05), (0.62, 0.05), (0.82, 0.27), (0.82, 0.95), (0.2, 0.95)], true);
            line(g, &[(0.62, 0.05), (0.62, 0.27), (0.82, 0.27)], false);
        }
        // Downloads: an arrow into a tray.
        "\u{E896}" => {
            line(g, &[(0.5, 0.05), (0.5, 0.66)], false);
            line(g, &[(0.25, 0.43), (0.5, 0.68), (0.75, 0.43)], false);
            line(g, &[(0.15, 0.92), (0.85, 0.92)], false);
        }
        // Pictures: a frame with a hill and the sun.
        "\u{E8B9}" => {
            line(g, &[(0.05, 0.15), (0.95, 0.15), (0.95, 0.85), (0.05, 0.85)], true);
            line(g, &[(0.1, 0.8), (0.4, 0.5), (0.58, 0.68), (0.72, 0.56), (0.92, 0.76)], false);
            ring(g, 0.68, 0.35, 0.08);
        }
        // Music: a note.
        "\u{E8D6}" => {
            line(g, &[(0.4, 0.75), (0.4, 0.15), (0.86, 0.07), (0.86, 0.67)], false);
            ring(g, 0.27, 0.76, 0.13);
            ring(g, 0.73, 0.68, 0.13);
        }
        // Videos: a camera.
        "\u{E714}" => {
            line(g, &[(0.05, 0.25), (0.66, 0.25), (0.66, 0.75), (0.05, 0.75)], true);
            line(g, &[(0.66, 0.45), (0.95, 0.28), (0.95, 0.72), (0.66, 0.55)], false);
        }
        // Explore: a folder.
        "\u{E8B7}" => line(g, &[(0.05, 0.18), (0.38, 0.18), (0.48, 0.3), (0.95, 0.3), (0.95, 0.84), (0.05, 0.84)], true),
        // Options: sliders.
        "\u{E713}" => {
            for (y, x) in [(0.2, 0.68), (0.5, 0.32), (0.8, 0.6)] {
                line(g, &[(0.05, y), (0.95, y)], false);
                let (cx, cy) = p(x, y);
                g.fill_circle(cx, cy, 0.11 * r.w, c);
            }
        }
        // About: an i in a circle.
        "\u{E946}" => {
            ring(g, 0.5, 0.5, 0.45);
            line(g, &[(0.5, 0.46), (0.5, 0.74)], false);
            line(g, &[(0.5, 0.26), (0.5, 0.32)], false);
        }
        // Exit: the power sign.
        "\u{E7E8}" => {
            line(g, &[(0.32, 0.22), (0.14, 0.45), (0.18, 0.72), (0.4, 0.92), (0.6, 0.92), (0.82, 0.72), (0.86, 0.45), (0.68, 0.22)], false);
            line(g, &[(0.5, 0.05), (0.5, 0.48)], false);
        }
        // Back: an arrow to the left.
        "\u{E72B}" => {
            line(g, &[(0.08, 0.5), (0.92, 0.5)], false);
            line(g, &[(0.4, 0.18), (0.08, 0.5), (0.4, 0.82)], false);
        }
        // Close.
        "\u{E8BB}" => {
            line(g, &[(0.1, 0.1), (0.9, 0.9)], false);
            line(g, &[(0.9, 0.1), (0.1, 0.9)], false);
        }
        // Pin, off and on.
        "\u{E718}" | "\u{E840}" => {
            line(g, &[(0.3, 0.08), (0.7, 0.08), (0.64, 0.45), (0.84, 0.62), (0.16, 0.62), (0.36, 0.45)], true);
            line(g, &[(0.5, 0.62), (0.5, 0.95)], false);
        }
        // Shield.
        "\u{EA18}" => line(g, &[(0.5, 0.04), (0.9, 0.18), (0.84, 0.62), (0.5, 0.95), (0.16, 0.62), (0.1, 0.18)], true),
        _ => {
            let (cx, cy) = p(0.5, 0.5);
            g.fill_circle(cx, cy, 0.2 * r.w, c);
        }
    }
}
