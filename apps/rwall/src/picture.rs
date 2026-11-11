//! Picture files: what kind they are, making them ready for the desktop, the
//! preview, and the folder of downloaded pictures.

use std::path::{Path, PathBuf};

/// Extensions of the pictures rwall takes from a folder.
pub const EXTENSIONS: &[&str] = &["jpg", "jpeg", "jfif", "jpe", "png", "bmp", "dib", "webp", "gif", "tif", "tiff"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Jpeg,
    Png,
    Bmp,
    Gif,
    Tiff,
    Webp,
}

impl Kind {
    pub fn ext(self) -> &'static str {
        match self {
            Kind::Jpeg => "jpg",
            Kind::Png => "png",
            Kind::Bmp => "bmp",
            Kind::Gif => "gif",
            Kind::Tiff => "tif",
            Kind::Webp => "webp",
        }
    }

    /// Whether the desktop shows it without help (WebP needs a codec that
    /// Windows may not have, and not every Linux desktop reads it).
    pub fn native(self) -> bool {
        self != Kind::Webp
    }
}

/// The kind of picture by its first bytes (not by its name).
pub fn kind(b: &[u8]) -> Option<Kind> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Kind::Jpeg)
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Kind::Png)
    } else if b.starts_with(b"BM") && b.len() > 26 {
        Some(Kind::Bmp)
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some(Kind::Gif)
    } else if b.starts_with(b"II*\0") || b.starts_with(b"MM\0*") {
        Some(Kind::Tiff)
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(Kind::Webp)
    } else {
        None
    }
}

/// Whether `path` has one of the picture extensions.
pub fn has_picture_ext(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Width and height, read from the header only.
pub fn size_of(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
}

/// Smallest picture accepted from a service, so an error page or an icon
/// never becomes the wallpaper.
pub const MIN_SIDE: u32 = 320;

/// Checks downloaded bytes: a picture rwall can use, large enough.
pub fn check(bytes: &[u8]) -> Result<Kind, String> {
    let k = kind(bytes).ok_or("the answer is not a picture")?;
    let (w, h) = size_of(bytes).ok_or("the picture cannot be read")?;
    if w < MIN_SIDE || h < MIN_SIDE {
        return Err(format!("the picture is too small ({w}×{h})"));
    }
    Ok(k)
}

/// A file name made from an id: letters, digits, '-', '_' and '.' only.
pub fn safe_name(id: &str) -> String {
    let s: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).take(120).collect();
    s.trim_matches('.').to_string()
}

/// Saves downloaded bytes as `<dir>/<name>.<ext>` and returns the path.
pub fn save(dir: &Path, name: &str, bytes: &[u8], k: Kind) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{}.{}", safe_name(name), k.ext()));
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })?;
    Ok(path)
}

/// A file the desktop can show: the picture itself, or a PNG copy in `dir`
/// for kinds it may not read.
pub fn ready(path: &Path, dir: &Path) -> Result<PathBuf, String> {
    let mut head = [0u8; 32];
    let n = {
        use std::io::Read;
        let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        f.read(&mut head).map_err(|e| format!("{}: {e}", path.display()))?
    };
    let k = kind(&head[..n]).ok_or_else(|| format!("{}: not a picture", path.display()))?;
    if k.native() {
        return Ok(path.to_path_buf());
    }
    let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stem = path.file_stem().map_or_else(|| "picture".into(), |s| s.to_string_lossy().to_string());
    let out = dir.join(format!("{}-{:08x}.png", safe_name(&stem), hash(&path.to_string_lossy())));
    img.to_rgb8().save_with_format(&out, image::ImageFormat::Png).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(out)
}

/// FNV-1a, for short stable names.
pub fn hash(s: &str) -> u32 {
    s.bytes().fold(0x811C_9DC5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

/// A small copy of the picture for the settings window, at most `w`×`h`.
pub fn thumbnail(path: &Path, w: u32, h: u32) -> Option<tiny_skia::Pixmap> {
    let img = image::open(path).ok()?.thumbnail(w, h).to_rgba8();
    let (tw, th) = img.dimensions();
    let mut pm = tiny_skia::Pixmap::new(tw, th)?;
    for (dst, src) in pm.pixels_mut().iter_mut().zip(img.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    Some(pm)
}

/// Deletes the oldest downloaded pictures beyond `keep`, never `except`.
/// Only files rwall wrote (picture extensions) in `dir` are touched.
pub fn trim(dir: &Path, keep: usize, except: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| has_picture_ext(p) && p != except)
        .map(|p| (std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    // The picture on the desktop counts as one kept.
    let keep = keep.saturating_sub(1);
    files.iter().skip(keep).filter(|(_, p)| std::fs::remove_file(p).is_ok()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rwall-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbImage::from_pixel(w, h, image::Rgb([10, 120, 200]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn kinds_by_content() {
        assert_eq!(kind(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(Kind::Jpeg));
        assert_eq!(kind(b"RIFF\0\0\0\0WEBPVP8 "), Some(Kind::Webp));
        assert_eq!(kind(b"<!DOCTYPE html>"), None);
        assert_eq!(kind(&png(1, 1)), Some(Kind::Png));
        assert!(has_picture_ext(Path::new("a/B.JPEG")));
        assert!(!has_picture_ext(Path::new("a/b.txt")));
        assert_eq!(safe_name("bing:20261111/OHR.Tower?x"), "bing_20261111_OHR.Tower_x");
    }

    #[test]
    fn check_refuses_small_or_foreign_answers() {
        assert_eq!(check(&png(400, 400)), Ok(Kind::Png));
        assert!(check(&png(100, 600)).is_err());
        assert!(check(b"{\"error\":\"rate limit\"}").is_err());
    }

    #[test]
    fn webp_becomes_png_and_thumbnails_shrink() {
        let d = dir("ready");
        let mut webp = Vec::new();
        image::RgbaImage::from_pixel(64, 48, image::Rgba([200, 30, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut webp), image::ImageFormat::WebP)
            .unwrap();
        let src = save(&d, "pic", &webp, Kind::Webp).unwrap();
        let out = ready(&src, &d.join("converted")).unwrap();
        assert_eq!(out.extension().unwrap(), "png");
        assert_eq!(kind(&std::fs::read(&out).unwrap()), Some(Kind::Png));
        let jpg_like = save(&d, "plain", &png(64, 48), Kind::Png).unwrap();
        assert_eq!(ready(&jpg_like, &d).unwrap(), jpg_like);
        let t = thumbnail(&out, 32, 32).unwrap();
        assert_eq!((t.width(), t.height()), (32, 24));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn trim_keeps_the_newest() {
        let d = dir("trim");
        let mut paths = Vec::new();
        for i in 0..5 {
            let p = d.join(format!("{i}.png"));
            std::fs::write(&p, b"x").unwrap();
            let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000 + i * 100);
            std::fs::File::options().write(true).open(&p).unwrap().set_modified(t).unwrap();
            paths.push(p);
        }
        std::fs::write(d.join("notes.txt"), b"keep me").unwrap();
        // Keep 3: the current one (the oldest here) and the two newest.
        assert_eq!(trim(&d, 3, &paths[0]), 2);
        assert!(paths[0].exists() && !paths[1].exists() && !paths[2].exists());
        assert!(paths[3].exists() && paths[4].exists() && d.join("notes.txt").exists());
        let _ = std::fs::remove_dir_all(d);
    }
}
