//! The application icon (original artwork): two equal sheets of paper, one
//! over the other, with the same lines of text, on a dark rounded tile.
//! Drawn with tiny-skia at any size; used at runtime for the window icon and
//! by `build.rs` for the `.ico` resource.
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

/// A sheet with its top right corner folded, as (x0, y0, x1, y1).
fn sheet(x0: f32, y0: f32, x1: f32, y1: f32, fold: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(x0, y0);
    pb.line_to(x1 - fold, y0);
    pb.line_to(x1, y0 + fold);
    pb.line_to(x1, y1);
    pb.line_to(x0, y1);
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
    if let Some(frame) = rounded(10.0 * s, 10.0 * s, 246.0 * s, 246.0 * s, 34.0 * s) {
        pm.fill_path(&frame, &solid(0x22, 0x2A, 0x36, 255), FillRule::Winding, id, None);
    }
    let small = n < 32;
    // The sheet behind, then the one in front, each with the same lines.
    /// (left, top, top colour, bottom colour) in 256ths.
    type Sheet = (f32, f32, (u8, u8, u8), (u8, u8, u8));
    let sheets: [Sheet; 2] =
        [(42.0, 34.0, (0x8F, 0xB8, 0xF5), (0x5E, 0x93, 0xE6)), (92.0, 84.0, (0xFF, 0xFF, 0xFF), (0xDC, 0xE6, 0xF5))];
    for (i, (x0, y0, top, bottom)) in sheets.into_iter().enumerate() {
        let (x1, y1) = (x0 + 122.0, y0 + 140.0);
        let fold = 34.0;
        if let Some(p) = sheet(x0 * s, y0 * s, x1 * s, y1 * s, fold * s) {
            pm.fill_path(&p, &gradient(y0 * s, y1 * s, top, bottom), FillRule::Winding, id, None);
        }
        // The folded corner.
        let mut pb = PathBuilder::new();
        pb.move_to((x1 - fold) * s, y0 * s);
        pb.line_to((x1 - fold) * s, (y0 + fold) * s);
        pb.line_to(x1 * s, (y0 + fold) * s);
        pb.close();
        if let Some(p) = pb.finish() {
            pm.fill_path(&p, &solid(0x22, 0x2A, 0x36, 70), FillRule::Winding, id, None);
        }
        // Lines of text, the same on both sheets.
        let lines: &[(f32, f32)] = if small { &[(48.0, 70.0), (88.0, 90.0)] } else { &[(54.0, 62.0), (78.0, 92.0), (102.0, 80.0), (126.0, 92.0)] };
        let color = if i == 0 { (0x22, 0x4E, 0x9A) } else { (0x2F, 0x80, 0xED) };
        for &(dy, w) in lines {
            let h = if small { 14.0 } else { 9.0 };
            if let Some(r) = rounded((x0 + 16.0) * s, (y0 + dy) * s, (x0 + 16.0 + w) * s, (y0 + dy + h) * s, h / 2.0 * s) {
                pm.fill_path(&r, &solid(color.0, color.1, color.2, 255), FillRule::Winding, id, None);
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
