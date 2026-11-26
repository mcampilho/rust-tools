//! The application icon (original artwork): a rocket-like arrow leaving a
//! grid of tiles, on a dark blue rounded square. Drawn with tiny-skia at any
//! size; used at runtime and by `build.rs` for the `.ico` resource.
//!
//! Must stay self-contained (only `tiny_skia`) because `build.rs` includes it.

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

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

fn solid(r: u8, g: u8, b: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, 255);
    p.anti_alias = true;
    p
}

/// Renders the icon at `n`×`n` pixels as straight (non-premultiplied) RGBA.
pub fn render(n: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(n, n).expect("icon size");
    let s = n as f32 / 256.0;
    let id = Transform::identity();
    let mut fill = |p: Option<tiny_skia::Path>, c: Paint<'static>| {
        if let Some(p) = p {
            pm.fill_path(&p, &c, FillRule::Winding, id, None);
        }
    };
    fill(rounded(10.0 * s, 10.0 * s, 246.0 * s, 246.0 * s, 40.0 * s), solid(0x14, 0x2E, 0x55));
    // Three tiles of a menu; the fourth corner is where the arrow starts.
    for (x, y, c) in [(46.0, 46.0, (0x4F, 0xA8, 0xF0)), (46.0, 140.0, (0x7B, 0xC6, 0x7E)), (140.0, 140.0, (0xF2, 0xB8, 0x4B))] {
        fill(rounded(x * s, y * s, (x + 70.0) * s, (y + 70.0) * s, 14.0 * s), solid(c.0, c.1, c.2));
    }
    // The arrow, up and to the right.
    let mut pb = PathBuilder::new();
    let pts = [(216.0, 40.0), (216.0, 122.0), (190.0, 96.0), (152.0, 134.0), (122.0, 104.0), (160.0, 66.0), (134.0, 40.0)];
    pb.move_to(pts[0].0 * s, pts[0].1 * s);
    for (x, y) in &pts[1..] {
        pb.line_to(x * s, y * s);
    }
    pb.close();
    fill(pb.finish(), solid(0xFF, 0xFF, 0xFF));
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
