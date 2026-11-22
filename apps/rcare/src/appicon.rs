//! The application icon (original artwork): a white cross-shaped plus on a
//! teal shield, with a sparkle, on a dark rounded tile. Drawn with
//! tiny-skia at any size; used at runtime for the window icon and by
//! `build.rs` for the `.ico` resource.
//!
//! Must stay self-contained (only `tiny_skia`) because `build.rs` includes it.

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

fn solid(r: u8, g: u8, b: u8, a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, a);
    p.anti_alias = true;
    p
}

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

/// Renders the icon at `n`×`n` pixels as straight (non-premultiplied) RGBA.
pub fn render(n: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(n, n).expect("icon size");
    let s = n as f32 / 256.0;
    let id = Transform::identity();
    if let Some(p) = rounded(10.0 * s, 10.0 * s, 246.0 * s, 246.0 * s, 34.0 * s) {
        pm.fill_path(&p, &solid(0x22, 0x2A, 0x36, 255), FillRule::Winding, id, None);
    }
    // The shield.
    let mut pb = PathBuilder::new();
    pb.move_to(128.0 * s, 36.0 * s);
    pb.line_to(206.0 * s, 62.0 * s);
    pb.quad_to(206.0 * s, 150.0 * s, 128.0 * s, 222.0 * s);
    pb.quad_to(50.0 * s, 150.0 * s, 50.0 * s, 62.0 * s);
    pb.close();
    if let Some(p) = pb.finish() {
        pm.fill_path(&p, &solid(0x1F, 0xA8, 0x9A, 255), FillRule::Winding, id, None);
    }
    // The plus.
    for (x0, y0, x1, y1) in [(116.0, 82.0, 140.0, 174.0), (82.0, 116.0, 174.0, 140.0)] {
        if let Some(p) = rounded(x0 * s, y0 * s, x1 * s, y1 * s, 6.0 * s) {
            pm.fill_path(&p, &solid(0xFF, 0xFF, 0xFF, 255), FillRule::Winding, id, None);
        }
    }
    // A sparkle, top right.
    let (cx, cy, r) = (198.0 * s, 52.0 * s, 26.0 * s);
    let k = r * 0.22;
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy - r);
    pb.quad_to(cx + k, cy - k, cx + r, cy);
    pb.quad_to(cx + k, cy + k, cx, cy + r);
    pb.quad_to(cx - k, cy + k, cx - r, cy);
    pb.quad_to(cx - k, cy - k, cx, cy - r);
    pb.close();
    if let Some(p) = pb.finish() {
        pm.fill_path(&p, &solid(0xF2, 0xC0, 0x4C, 255), FillRule::Winding, id, None);
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

    /// The icon as a picture to look at: `apps/rcare/target/screens/icon.png`.
    #[test]
    #[ignore]
    fn picture() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/screens");
        std::fs::create_dir_all(&dir).unwrap();
        let mut pm = tiny_skia::Pixmap::new(256, 256).unwrap();
        for (d, c) in pm.data_mut().chunks_mut(4).zip(super::render(256).chunks(4)) {
            let a = c[3] as u32;
            d.copy_from_slice(&[(c[0] as u32 * a / 255) as u8, (c[1] as u32 * a / 255) as u8, (c[2] as u32 * a / 255) as u8, c[3]]);
        }
        pm.save_png(dir.join("icon.png")).unwrap();
    }
}
