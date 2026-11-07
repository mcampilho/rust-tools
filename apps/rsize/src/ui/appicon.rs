//! The application icon (original artwork): a rounded tile split into
//! coloured blocks, like a small treemap, each with a soft highlight. Drawn
//! with tiny-skia at any size; used at runtime for the window icon and by
//! `build.rs` for the `.ico` resource.
//!
//! Must stay self-contained (only `tiny_skia`) because `build.rs` includes it.

use tiny_skia::{FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, Shader, SpreadMode, Stroke, Transform};

fn rounded(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0).max(0.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x0 + r, y0);
    pb.line_to(x1 - r, y0);
    pb.quad_to(x1, y0, x1, y0 + r);
    pb.line_to(x1, y1 - r);
    pb.quad_to(x1, y1, x1 - r, y1);
    pb.line_to(x0 + r, y1);
    pb.quad_to(x0, y1, x0, y1 - r);
    pb.line_to(x0, y0 + r);
    pb.quad_to(x0, y0, x0 + r, y0);
    pb.close();
    pb.finish()
}

/// A diagonal gradient from a lighter to a darker shade of `c`.
fn shaded(x0: f32, y0: f32, x1: f32, y1: f32, c: (u8, u8, u8)) -> Paint<'static> {
    let shade = |k: f32| {
        let f = |v: u8| (v as f32 * k).clamp(0.0, 255.0) as u8;
        tiny_skia::Color::from_rgba8(f(c.0), f(c.1), f(c.2), 255)
    };
    let shader = LinearGradient::new(
        Point::from_xy(x0, y0),
        Point::from_xy(x1, y1),
        vec![GradientStop::new(0.0, shade(1.25)), GradientStop::new(1.0, shade(0.72))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or(Shader::SolidColor(shade(1.0)));
    Paint { shader, anti_alias: true, ..Paint::default() }
}

/// Renders the icon at `n`×`n` pixels as straight (non-premultiplied) RGBA.
pub fn render(n: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(n, n).expect("icon size");
    let s = n as f32 / 256.0;
    let id = Transform::identity();
    // Frame.
    if let Some(frame) = rounded(10.0 * s, 10.0 * s, 246.0 * s, 246.0 * s, 34.0 * s) {
        let mut p = Paint::default();
        p.set_color_rgba8(0x22, 0x2A, 0x36, 255);
        p.anti_alias = true;
        pm.fill_path(&frame, &p, FillRule::Winding, id, None);
    }
    // Blocks, laid out like a squarified treemap (x0, y0, x1, y1 in 256ths).
    let gap = if n >= 32 { 8.0 } else { 14.0 };
    type Block = (f32, f32, f32, f32, (u8, u8, u8));
    let blocks: [Block; 5] = [
        (26.0, 26.0, 150.0, 230.0, (0x2F, 0x80, 0xED)),
        (150.0, 26.0, 230.0, 124.0, (0xE8, 0x5D, 0x3F)),
        (150.0, 124.0, 230.0, 184.0, (0x3C, 0xB3, 0x71)),
        (150.0, 184.0, 192.0, 230.0, (0xF2, 0xB7, 0x2E)),
        (192.0, 184.0, 230.0, 230.0, (0x9B, 0x6C, 0xE0)),
    ];
    for (x0, y0, x1, y1, c) in blocks {
        let (x0, y0) = ((x0 + gap / 2.0) * s, (y0 + gap / 2.0) * s);
        let (x1, y1) = ((x1 - gap / 2.0) * s, (y1 - gap / 2.0) * s);
        if let Some(b) = rounded(x0, y0, x1, y1, 10.0 * s) {
            pm.fill_path(&b, &shaded(x0, y0, x1, y1, c), FillRule::Winding, id, None);
            if n >= 48 {
                let mut p = Paint::default();
                p.set_color_rgba8(255, 255, 255, 60);
                p.anti_alias = true;
                let st = Stroke { width: 3.0 * s, ..Default::default() };
                pm.stroke_path(&b, &p, &st, id, None);
            }
        }
    }
    // Un-premultiply for straight RGBA output.
    pm.pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_sizes_render() {
        for n in [16, 20, 24, 32, 48, 256] {
            let px = super::render(n);
            assert_eq!(px.len(), (n * n * 4) as usize);
            assert!(px.chunks(4).any(|p| p[3] == 255), "opaque pixels at {n}");
        }
    }
}
