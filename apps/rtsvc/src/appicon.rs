//! The service's icon (original artwork): a white cog on a blue shield, on a
//! dark rounded tile — what Windows shows for `rtsvc.exe` in Explorer and in
//! the file's properties. Drawn with tiny-skia at any size by `build.rs` for
//! the `.ico` resource.
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
    pb.move_to(128.0 * s, 32.0 * s);
    pb.line_to(210.0 * s, 60.0 * s);
    pb.quad_to(210.0 * s, 152.0 * s, 128.0 * s, 226.0 * s);
    pb.quad_to(46.0 * s, 152.0 * s, 46.0 * s, 60.0 * s);
    pb.close();
    if let Some(p) = pb.finish() {
        pm.fill_path(&p, &solid(0x2B, 0x7C, 0xD6, 255), FillRule::Winding, id, None);
    }
    // The cog: a ring of teeth around a hole.
    let (cx, cy) = (128.0 * s, 122.0 * s);
    let (outer, inner, hole) = (58.0 * s, 44.0 * s, 18.0 * s);
    let teeth = 8;
    let mut pb = PathBuilder::new();
    for k in 0..teeth * 4 {
        let a = (k as f32 + 0.5) * std::f32::consts::TAU / (teeth * 4) as f32;
        let r = if (k / 2) % 2 == 0 { outer } else { inner };
        let (x, y) = (cx + r * a.cos(), cy + r * a.sin());
        if k == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.close();
    // The hole, drawn the other way round so EvenOdd leaves it empty.
    pb.push_circle(cx, cy, hole);
    if let Some(p) = pb.finish() {
        pm.fill_path(&p, &solid(0xFF, 0xFF, 0xFF, 255), FillRule::EvenOdd, id, None);
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

    /// The icon as a picture to look at: `apps/rtsvc/target/icon.png`.
    #[test]
    #[ignore]
    fn picture() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target");
        std::fs::create_dir_all(&dir).unwrap();
        let mut pm = tiny_skia::Pixmap::new(256, 256).unwrap();
        for (d, c) in pm.data_mut().chunks_mut(4).zip(super::render(256).chunks(4)) {
            let a = c[3] as u32;
            d.copy_from_slice(&[(c[0] as u32 * a / 255) as u8, (c[1] as u32 * a / 255) as u8, (c[2] as u32 * a / 255) as u8, c[3]]);
        }
        pm.save_png(dir.join("icon.png")).unwrap();
    }
}
