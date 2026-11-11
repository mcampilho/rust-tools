//! The application icon (original artwork): a framed landscape (sky, sun,
//! two hills) with a round arrow in the corner, for a picture that changes.
//! Drawn with tiny-skia at any size; used at runtime for the window icon and
//! by `build.rs` for the `.ico` resource.
//!
//! Must stay self-contained (only `tiny_skia`) because `build.rs` includes it.

use tiny_skia::{FillRule, GradientStop, LinearGradient, LineCap, Paint, PathBuilder, Pixmap, Point, Shader, SpreadMode, Stroke, Transform};

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

fn solid(r: u8, g: u8, b: u8, a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, a);
    p.anti_alias = true;
    p
}

fn vertical(y0: f32, y1: f32, top: (u8, u8, u8), bottom: (u8, u8, u8)) -> Paint<'static> {
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

/// Renders the icon at `n`×`n` pixels as straight (non-premultiplied) RGBA.
pub fn render(n: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(n, n).expect("icon size");
    let s = n as f32 / 256.0;
    let id = Transform::identity();
    let (x0, y0, x1, y1) = (12.0 * s, 30.0 * s, 244.0 * s, 226.0 * s);
    // Frame, then the picture inside it.
    if let Some(frame) = rounded(x0, y0, x1, y1, 30.0 * s) {
        pm.fill_path(&frame, &solid(0x22, 0x2A, 0x36, 255), FillRule::Winding, id, None);
    }
    let inset = 16.0 * s;
    let (px0, py0, px1, py1) = (x0 + inset, y0 + inset, x1 - inset, y1 - inset);
    let Some(pic) = rounded(px0, py0, px1, py1, 16.0 * s) else { return Vec::new() };
    let mask = {
        let mut m = tiny_skia::Mask::new(n, n).expect("mask");
        m.fill_path(&pic, FillRule::Winding, true, id);
        m
    };
    pm.fill_path(&pic, &vertical(py0, py1, (0x3E, 0x8E, 0xF0), (0xF6, 0xB2, 0x6B)), FillRule::Winding, id, None);
    // Sun.
    let mut pb = PathBuilder::new();
    pb.push_circle(170.0 * s, 92.0 * s, 24.0 * s);
    if let Some(p) = pb.finish() {
        pm.fill_path(&p, &solid(0xFF, 0xE2, 0x7A, 255), FillRule::Winding, id, Some(&mask));
    }
    // Two hills.
    let hill = |pts: &[(f32, f32)], c: (u8, u8, u8), pm: &mut Pixmap| {
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0].0 * s, pts[0].1 * s);
        for w in pts[1..].chunks(2) {
            pb.quad_to(w[0].0 * s, w[0].1 * s, w[1].0 * s, w[1].1 * s);
        }
        pb.line_to(px1, py1);
        pb.line_to(px0, py1);
        pb.close();
        if let Some(p) = pb.finish() {
            pm.fill_path(&p, &solid(c.0, c.1, c.2, 255), FillRule::Winding, id, Some(&mask));
        }
    };
    hill(&[(28.0, 170.0), (90.0, 100.0), (160.0, 168.0), (200.0, 200.0), (240.0, 190.0)], (0x2E, 0x8B, 0x57), &mut pm);
    hill(&[(28.0, 196.0), (130.0, 150.0), (240.0, 200.0)], (0x1F, 0x6B, 0x45), &mut pm);
    // The round arrow, on a dark disc at the bottom right.
    if n >= 20 {
        let (cx, cy, r) = (204.0 * s, 196.0 * s, 46.0 * s);
        let mut pb = PathBuilder::new();
        pb.push_circle(cx, cy, r);
        if let Some(p) = pb.finish() {
            pm.fill_path(&p, &solid(0x22, 0x2A, 0x36, 255), FillRule::Winding, id, None);
        }
        let a = 26.0 * s;
        let mut pb = PathBuilder::new();
        // Three quarters of a circle, from the top clockwise to the left.
        pb.move_to(cx, cy - a);
        pb.cubic_to(cx + a * 0.55, cy - a, cx + a, cy - a * 0.55, cx + a, cy);
        pb.cubic_to(cx + a, cy + a * 0.55, cx + a * 0.55, cy + a, cx, cy + a);
        pb.cubic_to(cx - a * 0.55, cy + a, cx - a, cy + a * 0.55, cx - a, cy);
        if let Some(p) = pb.finish() {
            let st = Stroke { width: 11.0 * s, line_cap: LineCap::Round, ..Default::default() };
            pm.stroke_path(&p, &solid(0xFF, 0xFF, 0xFF, 255), &st, id, None);
        }
        // Arrow head at the left end, pointing up.
        let mut pb = PathBuilder::new();
        pb.move_to(cx - a - 15.0 * s, cy + 2.0 * s);
        pb.line_to(cx - a, cy - 17.0 * s);
        pb.line_to(cx - a + 15.0 * s, cy + 2.0 * s);
        pb.close();
        if let Some(p) = pb.finish() {
            pm.fill_path(&p, &solid(0xFF, 0xFF, 0xFF, 255), FillRule::Winding, id, None);
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
