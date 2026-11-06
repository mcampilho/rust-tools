//! The application icon (original artwork), drawn with tiny-skia at any
//! size. Used at runtime for the window and tray icons and by `build.rs`
//! to produce the `.ico` resource embedded in the executable.
//!
//! This file must stay self-contained (only `tiny_skia`) because the build
//! script includes it with `#[path]`.

use tiny_skia::{FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, Rect, Shader, SpreadMode, Transform};

fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let k = r * 0.447_715;
    let (x1, y1) = (x + w, y + h);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x1 - r, y);
    pb.cubic_to(x1 - k, y, x1, y + k, x1, y + r);
    pb.line_to(x1, y1 - r);
    pb.cubic_to(x1, y1 - k, x1 - k, y1, x1 - r, y1);
    pb.line_to(x + r, y1);
    pb.cubic_to(x + k, y1, x, y1 - k, x, y1 - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + k, x + k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn solid(r: u8, g: u8, b: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, 255);
    p.anti_alias = true;
    p
}

fn gradient(y0: f32, y1: f32, top: (u8, u8, u8), bottom: (u8, u8, u8)) -> Paint<'static> {
    let c = |c: (u8, u8, u8)| tiny_skia::Color::from_rgba8(c.0, c.1, c.2, 255);
    let shader = LinearGradient::new(
        Point::from_xy(0.0, y0),
        Point::from_xy(0.0, y1),
        vec![GradientStop::new(0.0, c(top)), GradientStop::new(1.0, c(bottom))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or(Shader::SolidColor(c(top)));
    Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    }
}

/// Renders the icon at `n`×`n` pixels as straight (non-premultiplied) RGBA.
pub fn render(n: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(n, n).expect("icon size");
    let s = n as f32 / 256.0;
    let id = Transform::identity();
    let fill = |pm: &mut Pixmap, path: Option<tiny_skia::Path>, paint: &Paint| {
        if let Some(p) = path {
            pm.fill_path(&p, paint, FillRule::Winding, id, None);
        }
    };
    // Body
    fill(
        &mut pm,
        rrect(30.0 * s, 8.0 * s, 196.0 * s, 240.0 * s, 34.0 * s),
        &gradient(8.0 * s, 248.0 * s, (0x4A, 0x4F, 0x57), (0x24, 0x27, 0x2C)),
    );
    // Display
    fill(
        &mut pm,
        rrect(52.0 * s, 32.0 * s, 152.0 * s, 58.0 * s, 12.0 * s),
        &gradient(32.0 * s, 90.0 * s, (0xE9, 0xF3, 0xFC), (0xC8, 0xDD, 0xF0)),
    );
    if n >= 32 {
        // "123" hint on the display as three bars
        let bar = solid(0x2B, 0x3A, 0x4A);
        for i in 0..3 {
            let x = 150.0 - i as f32 * 26.0;
            fill(&mut pm, rrect(x * s, 46.0 * s, 16.0 * s, 30.0 * s, 4.0 * s), &bar);
        }
    }
    // Keys: 3x3 grid; the bottom-right key is the accent "=".
    let (cols, rows) = (3, 3);
    let (gx, gy, gw, gh) = (52.0, 108.0, 152.0, 116.0);
    let gap = if n >= 32 { 10.0 } else { 16.0 };
    let kw = (gw - gap * (cols as f32 - 1.0)) / cols as f32;
    let kh = (gh - gap * (rows as f32 - 1.0)) / rows as f32;
    for r in 0..rows {
        for c in 0..cols {
            let x = gx + c as f32 * (kw + gap);
            let y = gy + r as f32 * (kh + gap);
            let accent = r == rows - 1 && c == cols - 1;
            let paint = if accent {
                gradient(y * s, (y + kh) * s, (0x5C, 0xC8, 0xFF), (0x1E, 0x8C, 0xE8))
            } else {
                solid(0x6B, 0x71, 0x7A)
            };
            fill(&mut pm, rrect(x * s, y * s, kw * s, kh * s, 7.0 * s), &paint);
            if accent && n >= 32 {
                let white = solid(0xFF, 0xFF, 0xFF);
                for dy in [-7.0, 5.0] {
                    if let Some(rect) = Rect::from_xywh((x + kw * 0.25) * s, (y + kh / 2.0 + dy - 2.0) * s, kw * 0.5 * s, 5.0 * s) {
                        pm.fill_rect(rect, &white, id, None);
                    }
                }
            }
        }
    }
    // Un-premultiply for consumers that expect straight alpha.
    let mut data = pm.take();
    for px in data.chunks_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    data
}
