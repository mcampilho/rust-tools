//! The application icon (original artwork): a parcel with its lid and a
//! zipped band down the middle, on a dark rounded tile. Drawn with tiny-skia
//! at any size; used at runtime for the window icon and by `build.rs` for the
//! `.ico` resource.
//!
//! Must stay self-contained (only `tiny_skia`) because `build.rs` includes it.

use tiny_skia::{FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, Shader, SpreadMode, Transform};

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

/// A vertical gradient between two colours.
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

/// Renders the icon at `n`×`n` pixels as straight (non-premultiplied) RGBA.
pub fn render(n: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(n, n).expect("icon size");
    let s = n as f32 / 256.0;
    let id = Transform::identity();
    let small = n < 32;
    let fill = |pm: &mut Pixmap, r: (f32, f32, f32, f32, f32), paint: &Paint| {
        if let Some(p) = rounded(r.0 * s, r.1 * s, r.2 * s, r.3 * s, r.4 * s) {
            pm.fill_path(&p, paint, FillRule::Winding, id, None);
        }
    };
    fill(&mut pm, (10.0, 10.0, 246.0, 246.0, 34.0), &solid(0x22, 0x2A, 0x36, 255));
    // The box, then its lid, a little wider.
    fill(&mut pm, (48.0, 92.0, 208.0, 214.0, 12.0), &gradient(92.0 * s, 214.0 * s, (0xF2, 0xB0, 0x5A), (0xD9, 0x84, 0x2B)));
    fill(&mut pm, (40.0, 60.0, 216.0, 100.0, 10.0), &gradient(60.0 * s, 100.0 * s, (0xFF, 0xD0, 0x86), (0xF0, 0xA9, 0x4E)));
    // The shadow of the lid on the box.
    fill(&mut pm, (48.0, 100.0, 208.0, 108.0, 0.0), &solid(0x8A, 0x4B, 0x10, 90));
    // The band down the middle, with the teeth of a zip.
    let (bx0, bx1) = (114.0, 142.0);
    fill(&mut pm, (bx0, 60.0, bx1, 214.0, 3.0), &solid(0x2B, 0x35, 0x45, 255));
    let teeth = if small { 3 } else { 8 };
    let step = 140.0 / teeth as f32;
    for i in 0..teeth {
        let y = 68.0 + i as f32 * step;
        let (x0, x1) = if i % 2 == 0 { (bx0 + 4.0, 130.0) } else { (126.0, bx1 - 4.0) };
        fill(&mut pm, (x0, y, x1, y + step * 0.55, 2.0), &solid(0xE6, 0xEA, 0xF0, 255));
    }
    // The pull of the zip, at the top.
    fill(&mut pm, (118.0, 40.0, 138.0, 74.0, 6.0), &solid(0xE6, 0xEA, 0xF0, 255));
    fill(&mut pm, (124.0, 48.0, 132.0, 62.0, 3.0), &solid(0x2B, 0x35, 0x45, 255));
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
