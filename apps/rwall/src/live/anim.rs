//! Animated pictures (GIF and WebP), decoded once into frames kept in
//! memory. Large animations are made smaller so they never take more than
//! [`BUDGET`] bytes.

use image::AnimationDecoder;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

/// Memory for all the frames, at most.
pub const BUDGET: usize = 384 << 20;
/// Frames kept, at most.
const MAX_FRAMES: usize = 3000;
/// Browsers show frames of 0 or 10 ms for 100 ms; so does rwall.
const SHORTEST: Duration = Duration::from_millis(20);
const DEFAULT: Duration = Duration::from_millis(100);

pub struct Frames {
    pub w: u32,
    pub h: u32,
    /// Pixels as `0x00RRGGBB` (over black), and how long each shows.
    pub frames: Vec<(Vec<u32>, Duration)>,
}

fn delay(d: image::Delay) -> Duration {
    let (n, den) = d.numer_denom_ms();
    let ms = n.checked_div(den).unwrap_or(0);
    let dur = Duration::from_millis(ms.into());
    if dur < SHORTEST {
        DEFAULT
    } else {
        dur
    }
}

/// Reads the frames of a GIF or a WebP (a still WebP gives one frame).
pub fn load(path: &Path) -> Result<Frames, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let reader = BufReader::new(file);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let (w, h, frames): (u32, u32, image::Frames) = if ext == "webp" {
        let d = image::codecs::webp::WebPDecoder::new(reader).map_err(|e| e.to_string())?;
        let (w, h) = image::ImageDecoder::dimensions(&d);
        if !d.has_animation() {
            let img = image::DynamicImage::from_decoder(d).map_err(|e| e.to_string())?.to_rgba8();
            let frame = image::Frame::new(img);
            return collect(w, h, image::Frames::new(Box::new(std::iter::once(Ok(frame)))));
        }
        (w, h, d.into_frames())
    } else {
        let d = image::codecs::gif::GifDecoder::new(reader).map_err(|e| e.to_string())?;
        let (w, h) = image::ImageDecoder::dimensions(&d);
        (w, h, d.into_frames())
    };
    collect(w, h, frames)
}

fn collect(w: u32, h: u32, frames: image::Frames) -> Result<Frames, String> {
    if w == 0 || h == 0 {
        return Err("the picture is empty".into());
    }
    let mut out = Frames { w, h, frames: Vec::new() };
    let mut scaled: Option<(u32, u32)> = None;
    for f in frames {
        let f = f.map_err(|e| e.to_string())?;
        let d = delay(f.delay());
        let mut buf = f.into_buffer();
        if scaled.is_none() && out.frames.len() == 16 {
            // Guess the whole from the first frames: make them smaller if
            // the animation would not fit.
            scaled = Some(fit_budget(w, h));
        }
        if let Some((sw, sh)) = scaled.filter(|s| *s != (w, h)) {
            buf = image::imageops::resize(&buf, sw, sh, image::imageops::FilterType::Triangle);
        }
        out.frames.push((to_rgb(&buf), d));
        if out.frames.len() >= MAX_FRAMES || out.frames.iter().map(|f| f.0.len() * 4).sum::<usize>() > BUDGET {
            break;
        }
    }
    if out.frames.is_empty() {
        return Err("the picture has no frames".into());
    }
    // Every frame the same size as the last ones.
    if let Some((sw, sh)) = scaled.filter(|s| *s != (w, h)) {
        for f in out.frames.iter_mut().take(16) {
            f.0 = scale_rgb(&f.0, w, h, sw, sh);
        }
        out.w = sw;
        out.h = sh;
    }
    Ok(out)
}

/// The size to keep frames at, for an animation of `w`×`h` that may have
/// many frames: smaller when 300 of them would pass the budget.
fn fit_budget(w: u32, h: u32) -> (u32, u32) {
    let per = BUDGET / 300;
    let bytes = w as usize * h as usize * 4;
    if bytes <= per {
        return (w, h);
    }
    let k = (per as f64 / bytes as f64).sqrt();
    (((w as f64 * k) as u32).max(1), ((h as f64 * k) as u32).max(1))
}

fn to_rgb(img: &image::RgbaImage) -> Vec<u32> {
    img.pixels()
        .map(|p| {
            let a = p[3] as u32;
            let ch = |v: u8| v as u32 * a / 255;
            (ch(p[0]) << 16) | (ch(p[1]) << 8) | ch(p[2])
        })
        .collect()
}

fn scale_rgb(px: &[u32], w: u32, h: u32, nw: u32, nh: u32) -> Vec<u32> {
    let mut img = image::RgbaImage::new(w, h);
    for (d, s) in img.pixels_mut().zip(px) {
        *d = image::Rgba([(s >> 16) as u8, (s >> 8) as u8, *s as u8, 255]);
    }
    to_rgb(&image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_gif() {
        let d = std::env::temp_dir().join(format!("rwall-anim-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let file = d.join("a.gif");
        {
            let f = std::fs::File::create(&file).unwrap();
            let mut enc = image::codecs::gif::GifEncoder::new(f);
            for (i, delay) in [(0u8, 50u32), (1, 0), (2, 200)] {
                let img = image::RgbaImage::from_pixel(8, 6, image::Rgba([i * 100, 10, 20, 255]));
                let frame = image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(delay, 1));
                enc.encode_frame(frame).unwrap();
            }
        }
        let fr = load(&file).unwrap();
        assert_eq!((fr.w, fr.h, fr.frames.len()), (8, 6, 3));
        assert_eq!(fr.frames[1].1, DEFAULT);
        assert_eq!(fr.frames[2].1, Duration::from_millis(200));
        assert_eq!(fr.frames[1].0[0] >> 16, 100);
        assert!(load(&d.join("none.gif")).is_err());
        std::fs::write(d.join("bad.gif"), b"GIF89a nonsense").unwrap();
        assert!(load(&d.join("bad.gif")).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn large_animations_are_made_smaller() {
        assert_eq!(fit_budget(640, 480), (640, 480));
        let (w, h) = fit_budget(3840, 2160);
        assert!(w < 3840 && (w as f32 / h as f32 - 16.0 / 9.0).abs() < 0.01);
        assert!(w as usize * h as usize * 4 * 300 <= BUDGET);
    }
}
