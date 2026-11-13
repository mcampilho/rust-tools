//! The application icon (original artwork): a grid of four program tiles,
//! one of them lifting out of it, with a red minus badge, on a dark rounded
//! tile. Drawn with tiny-skia at any size; used at runtime for the window
//! icon and by `build.rs` for the `.ico` resource.
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

fn solid(r: u8, g: u8, b: u8, a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, a);
    p.anti_alias = true;
    p
}

/// Renders the icon at `n`×`n` pixels as straight (non-premultiplied) RGBA.
pub fn render(n: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(n, n).expect("icon size");
    let s = n as f32 / 256.0;
    let id = Transform::identity();
    let fill = |pm: &mut Pixmap, r: (f32, f32, f32, f32, f32), paint: &Paint| {
        if let Some(p) = rounded(r.0 * s, r.1 * s, r.2 * s, r.3 * s, r.4 * s) {
            pm.fill_path(&p, paint, FillRule::Winding, id, None);
        }
    };
    fill(&mut pm, (10.0, 10.0, 246.0, 246.0, 34.0), &solid(0x22, 0x2A, 0x36, 255));
    // Three tiles in place, and the place of the fourth, empty.
    let (a, b, c, d) = (44.0, 124.0, 132.0, 212.0);
    fill(&mut pm, (a, c, b, d, 14.0), &solid(0x4C, 0xA3, 0xF2, 255));
    fill(&mut pm, (c, c, d, d, 14.0), &solid(0x5C, 0xD1, 0x8A, 255));
    fill(&mut pm, (a, a + 8.0, b, b + 8.0, 14.0), &solid(0xF2, 0xC0, 0x4C, 255));
    fill(&mut pm, (c, a + 8.0, d, b + 8.0, 14.0), &solid(0xFF, 0xFF, 0xFF, 40));
    // The fourth, lifted up and out.
    fill(&mut pm, (c + 18.0, a - 14.0, d + 18.0, b - 14.0, 14.0), &solid(0xE8, 0x6A, 0x8A, 255));
    // The red badge with a minus.
    let (cx, cy, r) = (196.0 * s, 74.0 * s, 34.0 * s);
    if let Some(p) = PathBuilder::from_circle(cx, cy, r) {
        pm.fill_path(&p, &solid(0xE5, 0x39, 0x35, 255), FillRule::Winding, id, None);
    }
    if let Some(p) = PathBuilder::from_circle(cx, cy, r) {
        let stroke = tiny_skia::Stroke { width: 8.0 * s, ..Default::default() };
        pm.stroke_path(&p, &solid(0x22, 0x2A, 0x36, 255), &stroke, id, None);
    }
    fill(&mut pm, (196.0 - 18.0, 74.0 - 6.0, 196.0 + 18.0, 74.0 + 6.0, 4.0), &solid(0xFF, 0xFF, 0xFF, 255));
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
