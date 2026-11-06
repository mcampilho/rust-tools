//! The application icon (original artwork): a page with a folded corner,
//! text lines and a caret. Drawn with tiny-skia at any size; used at runtime
//! for the window icon and by `build.rs` for the `.ico` resource.
//!
//! Must stay self-contained (only `tiny_skia`) because `build.rs` includes it.

use tiny_skia::{FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, Rect, Shader, SpreadMode, Transform};

fn solid(r: u8, g: u8, b: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(r, g, b, 255);
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

/// Renders the icon at `n`×`n` pixels as straight (non-premultiplied) RGBA.
pub fn render(n: u32) -> Vec<u8> {
    let mut pm = Pixmap::new(n, n).expect("icon size");
    let s = n as f32 / 256.0;
    let id = Transform::identity();
    let p = |x: f32, y: f32| (x * s, y * s);
    // Page outline with a folded top-right corner.
    let (x0, y0, x1, y1, fold, r) = (40.0, 12.0, 216.0, 244.0, 56.0, 14.0);
    let mut pb = PathBuilder::new();
    let m = p(x0 + r, y0);
    pb.move_to(m.0, m.1);
    let a = p(x1 - fold, y0);
    pb.line_to(a.0, a.1);
    let b = p(x1, y0 + fold);
    pb.line_to(b.0, b.1);
    let c = p(x1, y1 - r);
    pb.line_to(c.0, c.1);
    let (c1, c2) = (p(x1, y1), p(x1 - r, y1));
    pb.quad_to(c1.0, c1.1, c2.0, c2.1);
    let d = p(x0 + r, y1);
    pb.line_to(d.0, d.1);
    let (d1, d2) = (p(x0, y1), p(x0, y1 - r));
    pb.quad_to(d1.0, d1.1, d2.0, d2.1);
    let e = p(x0, y0 + r);
    pb.line_to(e.0, e.1);
    let (e1, e2) = (p(x0, y0), p(x0 + r, y0));
    pb.quad_to(e1.0, e1.1, e2.0, e2.1);
    pb.close();
    if let Some(path) = pb.finish() {
        // Border then body.
        pm.fill_path(&path, &gradient(y0 * s, y1 * s, (0xFF, 0xFF, 0xFF), (0xE4, 0xEA, 0xF2)), FillRule::Winding, id, None);
        let stroke = tiny_skia::Stroke { width: (6.0 * s).max(1.5), ..Default::default() };
        pm.stroke_path(&path, &solid(0x5A, 0x6B, 0x82), &stroke, id, None);
    }
    // Folded corner.
    let mut pb = PathBuilder::new();
    let f0 = p(x1 - fold, y0);
    pb.move_to(f0.0, f0.1);
    let f1 = p(x1 - fold, y0 + fold - 8.0);
    pb.line_to(f1.0, f1.1);
    let f2 = p(x1 - fold + 8.0, y0 + fold);
    pb.quad_to(f1.0, f0.1 + fold * s, f2.0, f2.1);
    let f3 = p(x1, y0 + fold);
    pb.line_to(f3.0, f3.1);
    pb.close();
    if let Some(path) = pb.finish() {
        pm.fill_path(&path, &solid(0xB9, 0xC6, 0xD6), FillRule::Winding, id, None);
    }
    // Text lines (fewer and thicker at small sizes).
    let small = n < 32;
    let lines: &[(f32, f32, f32)] = if small {
        &[(72.0, 96.0, 150.0), (72.0, 144.0, 180.0), (72.0, 192.0, 120.0)]
    } else {
        &[(72.0, 92.0, 120.0), (72.0, 124.0, 176.0), (96.0, 156.0, 150.0), (96.0, 188.0, 112.0), (72.0, 220.0, 70.0)]
    };
    let lh = if small { 22.0 } else { 14.0 };
    for (i, (x, y, w)) in lines.iter().enumerate() {
        let color = if i == 0 && !small { (0x1F, 0x7A, 0xE0) } else { (0x6B, 0x7A, 0x90) };
        if let Some(rect) = Rect::from_xywh(x * s, (y - lh / 2.0) * s, w * s, lh * s) {
            pm.fill_rect(rect, &Paint { anti_alias: false, ..solid(color.0, color.1, color.2) }, id, None);
        }
    }
    // Caret.
    if !small {
        if let Some(rect) = Rect::from_xywh(150.0 * s, 206.0 * s, 8.0 * s, 28.0 * s) {
            pm.fill_rect(rect, &Paint { anti_alias: false, ..solid(0x1F, 0x7A, 0xE0) }, id, None);
        }
    }
    let mut data = pm.take();
    for px in data.chunks_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    data
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_sizes_render() {
        for n in [16u32, 20, 24, 32, 40, 48, 64, 128, 256] {
            assert_eq!(super::render(n).len(), (n * n * 4) as usize);
        }
    }
}

