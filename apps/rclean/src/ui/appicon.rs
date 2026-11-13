//! The application icon (original artwork): a broom sweeping, with three
//! sparkles, on a dark rounded tile. Drawn with tiny-skia at any size; used
//! at runtime for the window icon and by `build.rs` for the `.ico` resource.
//!
//! Must stay self-contained (only `tiny_skia`) because `build.rs` includes it.

use tiny_skia::{FillRule, LineCap, Paint, PathBuilder, Pixmap, Stroke, Transform};

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

/// A four-pointed sparkle at (`cx`, `cy`) of radius `r`.
fn sparkle(cx: f32, cy: f32, r: f32) -> Option<tiny_skia::Path> {
    let k = r * 0.22;
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy - r);
    pb.quad_to(cx + k, cy - k, cx + r, cy);
    pb.quad_to(cx + k, cy + k, cx, cy + r);
    pb.quad_to(cx - k, cy + k, cx - r, cy);
    pb.quad_to(cx - k, cy - k, cx, cy - r);
    pb.close();
    pb.finish()
}

/// Renders the icon at `n`×`n` pixels as straight (non-premultiplied) RGBA.
pub fn render(n: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(n, n).expect("icon size");
    let s = n as f32 / 256.0;
    let id = Transform::identity();
    if let Some(p) = rounded(10.0 * s, 10.0 * s, 246.0 * s, 246.0 * s, 34.0 * s) {
        pm.fill_path(&p, &solid(0x22, 0x2A, 0x36, 255), FillRule::Winding, id, None);
    }
    // The handle, from the top right down to the head.
    let mut pb = PathBuilder::new();
    pb.move_to(206.0 * s, 42.0 * s);
    pb.line_to(118.0 * s, 142.0 * s);
    if let Some(p) = pb.finish() {
        let stroke = Stroke { width: 18.0 * s, line_cap: LineCap::Round, ..Default::default() };
        pm.stroke_path(&p, &solid(0xC8, 0x8B, 0x52, 255), &stroke, id, None);
    }
    // The band holding the bristles.
    let mut pb = PathBuilder::new();
    pb.move_to(100.0 * s, 128.0 * s);
    pb.line_to(132.0 * s, 156.0 * s);
    pb.line_to(122.0 * s, 168.0 * s);
    pb.line_to(88.0 * s, 140.0 * s);
    pb.close();
    if let Some(p) = pb.finish() {
        pm.fill_path(&p, &solid(0xE5, 0x39, 0x35, 255), FillRule::Winding, id, None);
    }
    // The bristles, fanning out to the bottom left.
    let mut pb = PathBuilder::new();
    pb.move_to(88.0 * s, 142.0 * s);
    pb.line_to(122.0 * s, 170.0 * s);
    pb.quad_to(110.0 * s, 212.0 * s, 80.0 * s, 226.0 * s);
    pb.line_to(34.0 * s, 186.0 * s);
    pb.quad_to(52.0 * s, 160.0 * s, 88.0 * s, 142.0 * s);
    pb.close();
    if let Some(p) = pb.finish() {
        pm.fill_path(&p, &solid(0xF2, 0xC0, 0x4C, 255), FillRule::Winding, id, None);
    }
    // The bristles' lines.
    for (a, b) in [((100.0, 158.0), (60.0, 200.0)), ((110.0, 166.0), (78.0, 212.0))] {
        let mut pb = PathBuilder::new();
        pb.move_to(a.0 * s, a.1 * s);
        pb.line_to(b.0 * s, b.1 * s);
        if let Some(p) = pb.finish() {
            let stroke = Stroke { width: 5.0 * s, line_cap: LineCap::Round, ..Default::default() };
            pm.stroke_path(&p, &solid(0xC9, 0x93, 0x2A, 255), &stroke, id, None);
        }
    }
    // Sparkles where it swept.
    for (x, y, r) in [(176.0, 172.0, 30.0), (210.0, 120.0, 16.0), (148.0, 214.0, 14.0)] {
        if let Some(p) = sparkle(x * s, y * s, r * s) {
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
