//! The application icon (original artwork): a page with a coloured header,
//! text lines and a magnifying glass. Drawn with tiny-skia at any size; used
//! at runtime for the window icon and by `build.rs` for the `.ico` resource.
//!
//! Must stay self-contained (only `tiny_skia`) because `build.rs` includes it.

use tiny_skia::{FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, Rect, Shader, SpreadMode, Stroke, Transform};

fn solid(r: u8, g: u8, b: u8, a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, a);
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
    Paint { shader, anti_alias: true, ..Paint::default() }
}

fn rounded(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> Option<tiny_skia::Path> {
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

/// Renders the icon at `n`×`n` pixels as straight (non-premultiplied) RGBA.
pub fn render(n: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(n, n).expect("icon size");
    let s = n as f32 / 256.0;
    let id = Transform::identity();
    // Page.
    let (x0, y0, x1, y1) = (36.0 * s, 14.0 * s, 204.0 * s, 236.0 * s);
    if let Some(page) = rounded(x0, y0, x1, y1, 16.0 * s) {
        pm.fill_path(&page, &gradient(y0, y1, (0xFF, 0xFF, 0xFF), (0xE6, 0xEC, 0xF4)), FillRule::Winding, id, None);
        // Header band.
        let mut clip = tiny_skia::Mask::new(n, n).expect("mask");
        clip.fill_path(&page, FillRule::Winding, true, id);
        if let Some(band) = Rect::from_xywh(x0, y0, x1 - x0, 46.0 * s) {
            pm.fill_rect(band, &gradient(y0, y0 + 46.0 * s, (0x3B, 0x8E, 0xEA), (0x1F, 0x6F, 0xD1)), id, Some(&clip));
        }
        let stroke = Stroke { width: (5.0 * s).max(1.2), ..Default::default() };
        pm.stroke_path(&page, &solid(0x5A, 0x6B, 0x80, 255), &stroke, id, None);
    }
    // Text lines.
    if n >= 24 {
        let line = solid(0x9A, 0xA8, 0xBA, 255);
        for (i, w) in [118.0, 132.0, 96.0, 124.0].iter().enumerate() {
            let y = (84.0 + i as f32 * 26.0) * s;
            if let Some(r) = Rect::from_xywh(62.0 * s, y, w * s, (10.0 * s).max(1.0)) {
                pm.fill_rect(r, &line, id, None);
            }
        }
    }
    // Magnifying glass.
    let (cx, cy, r) = (162.0 * s, 170.0 * s, 46.0 * s);
    if let Some(lens) = PathBuilder::from_circle(cx, cy, r) {
        pm.fill_path(&lens, &solid(0xD6, 0xEA, 0xFF, 230), FillRule::Winding, id, None);
        let ring = Stroke { width: (16.0 * s).max(2.0), ..Default::default() };
        pm.stroke_path(&lens, &solid(0x1F, 0x3A, 0x5F, 255), &ring, id, None);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(cx + r * 0.72, cy + r * 0.72);
    pb.line_to(cx + r * 1.55, cy + r * 1.55);
    if let Some(handle) = pb.finish() {
        let st = Stroke { width: (24.0 * s).max(3.0), line_cap: tiny_skia::LineCap::Round, ..Default::default() };
        pm.stroke_path(&handle, &solid(0x1F, 0x3A, 0x5F, 255), &st, id, None);
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
