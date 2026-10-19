//! Software rendering: shapes via tiny-skia, text via ab_glyph with a glyph
//! cache. All coordinates are physical pixels.

use ab_glyph::{Font, FontVec, GlyphId, Point};
use std::collections::HashMap;
use tiny_skia::{FillRule, Mask, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
    pub fn center_y(&self) -> f32 {
        self.y + self.h / 2.0
    }
    pub fn take_bottom(&mut self, h: f32) -> Rect {
        let h = h.min(self.h);
        self.h -= h;
        Rect::new(self.x, self.y + self.h, self.w, h)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(hex: u32) -> Color {
        Color {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
            a: 255,
        }
    }
    pub const fn rgba(hex: u32, a: u8) -> Color {
        Color {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
            a,
        }
    }

    /// Linear mix toward `o` by `t` (0..1).
    pub fn mix(self, o: Color, t: f32) -> Color {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Color {
            r: m(self.r, o.r),
            g: m(self.g, o.g),
            b: m(self.b, o.b),
            a: m(self.a, o.a),
        }
    }

    fn paint(self) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color_rgba8(self.r, self.g, self.b, self.a);
        p.anti_alias = true;
        p
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontId {
    Regular,
    Bold,
    Italic,
    BoldItalic,
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
}

struct Fonts {
    regular: FontVec,
    semibold: Option<FontVec>,
    mono: Option<FontVec>,
    bold: Option<FontVec>,
    italic: Option<FontVec>,
    bold_italic: Option<FontVec>,
    /// Fallback for symbols missing from the main font; loaded on demand.
    symbol: Option<Option<FontVec>>,
    /// CJK fallback (Japanese or Chinese glyph style), loaded on demand.
    cjk: Option<(bool, Option<FontVec>)>,
}

fn font_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Some(w) = std::env::var_os("WINDIR") {
        dirs.push(std::path::PathBuf::from(w).join("Fonts"));
    }
    dirs.push("C:\\Windows\\Fonts".into());
    for d in [
        "/usr/share/fonts/truetype/dejavu",
        "/usr/share/fonts/TTF",
        "/usr/share/fonts/dejavu",
        "/System/Library/Fonts/Supplemental",
        "/Library/Fonts",
    ] {
        dirs.push(d.into());
    }
    dirs
}

fn load_font(names: &[&str]) -> Option<FontVec> {
    for dir in font_dirs() {
        for n in names {
            if let Ok(bytes) = std::fs::read(dir.join(n)) {
                if let Ok(f) = FontVec::try_from_vec_and_index(bytes, 0) {
                    return Some(f);
                }
            }
        }
    }
    None
}

impl Fonts {
    fn load() -> Result<Fonts, String> {
        let regular = load_font(&["segoeui.ttf", "DejaVuSans.ttf", "Arial.ttf", "arial.ttf"])
            .ok_or("No usable system font found (expected Segoe UI in the Windows Fonts folder).")?;
        Ok(Fonts {
            regular,
            semibold: load_font(&["seguisb.ttf", "DejaVuSans-Bold.ttf"]),
            mono: load_font(&["consola.ttf", "CascadiaMono.ttf", "DejaVuSansMono.ttf", "cour.ttf"]),
            bold: load_font(&["segoeuib.ttf", "DejaVuSans-Bold.ttf"]),
            italic: load_font(&["segoeuii.ttf", "DejaVuSans-Oblique.ttf"]),
            bold_italic: load_font(&["segoeuiz.ttf", "DejaVuSans-BoldOblique.ttf"]),
            symbol: None,
            cjk: None,
        })
    }

    fn get(&self, id: u8) -> &FontVec {
        match id {
            1 => self.semibold.as_ref().unwrap_or(&self.regular),
            2 => self.mono.as_ref().unwrap_or(&self.regular),
            3 => self.bold.as_ref().or(self.semibold.as_ref()).unwrap_or(&self.regular),
            6 => self.italic.as_ref().unwrap_or(&self.regular),
            7 => self.bold_italic.as_ref().or(self.italic.as_ref()).unwrap_or(&self.regular),
            4 => self.symbol.as_ref().and_then(|s| s.as_ref()).unwrap_or(&self.regular),
            5 => self.cjk.as_ref().and_then(|(_, f)| f.as_ref()).unwrap_or(&self.regular),
            _ => &self.regular,
        }
    }

    /// Resolves a char to (font index, glyph), falling back to the symbol
    /// font and then the regular font.
    fn resolve(&mut self, primary: u8, c: char) -> (u8, GlyphId) {
        let g = self.get(primary).glyph_id(c);
        if g.0 != 0 {
            return (primary, g);
        }
        let g = self.regular.glyph_id(c);
        if g.0 != 0 {
            return (0, g);
        }
        if self.symbol.is_none() {
            self.symbol = Some(load_font(&["seguisym.ttf", "DejaVuSans.ttf"]));
        }
        if let Some(Some(s)) = &self.symbol {
            let g = s.glyph_id(c);
            if g.0 != 0 {
                return (4, g);
            }
        }
        // Han characters look different in Japanese and Chinese fonts, so the
        // fallback follows the UI language.
        let zh = false;
        if self.cjk.as_ref().is_none_or(|(z, _)| *z != zh) {
            let names: &[&str] = if zh {
                &["msyh.ttc", "simsun.ttc", "YuGothM.ttc", "NotoSansCJK-Regular.ttc"]
            } else {
                &["YuGothM.ttc", "meiryo.ttc", "msgothic.ttc", "msyh.ttc", "NotoSansCJK-Regular.ttc"]
            };
            self.cjk = Some((zh, load_font(names)));
        }
        if let Some((_, Some(f))) = &self.cjk {
            let g = f.glyph_id(c);
            if g.0 != 0 {
                return (5, g);
            }
        }
        (primary, GlyphId(0))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    font: u8,
    glyph: u16,
    size_q: u16,
    sub_x: u8,
}

struct GlyphBmp {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    cov: Vec<u8>,
}

/// Coverage-to-alpha curve; slightly boosts mid tones so grayscale text
/// looks closer to ClearType weight.
fn contrast_lut() -> [u8; 256] {
    let mut lut = [0u8; 256];
    for (i, v) in lut.iter_mut().enumerate() {
        *v = ((i as f32 / 255.0).powf(0.85) * 255.0).round() as u8;
    }
    lut
}

pub struct Gfx {
    pub pix: Pixmap,
    fonts: Fonts,
    cache: HashMap<GlyphKey, GlyphBmp>,
    clip: Option<(Rect, Mask)>,
    lut: [u8; 256],
}

impl Gfx {
    pub fn new() -> Result<Gfx, String> {
        Ok(Gfx {
            pix: Pixmap::new(1, 1).expect("pixmap"),
            fonts: Fonts::load()?,
            cache: HashMap::new(),
            clip: None,
            lut: contrast_lut(),
        })
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if self.pix.width() != w || self.pix.height() != h {
            self.pix = Pixmap::new(w.max(1), h.max(1)).expect("pixmap");
            self.clip = None;
        }
    }

    /// Horizontal advance of `c` in font `f` at `px`.
    pub fn advance(&mut self, c: char, px: f32, f: FontId) -> f32 {
        let (fi, g) = self.fonts.resolve(Self::font_index(f), c);
        let font = self.fonts.get(fi);
        font.h_advance_unscaled(g) * Self::units_scale(font, px)
    }

    pub fn trim_cache(&mut self) {
        if self.cache.len() > 3000 {
            self.cache.clear();
        }
    }

    pub fn set_clip(&mut self, r: Option<Rect>) {
        self.clip = r.map(|r| {
            let mut m = Mask::new(self.pix.width(), self.pix.height()).expect("mask");
            if let Some(rect) = tiny_skia::Rect::from_xywh(r.x, r.y, r.w.max(0.5), r.h.max(0.5)) {
                let path = PathBuilder::from_rect(rect);
                m.fill_path(&path, FillRule::Winding, false, Transform::identity());
            }
            (r, m)
        });
    }

    pub fn fill_rect(&mut self, r: Rect, c: Color) {
        if c.a == 0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        if let Some(rect) = tiny_skia::Rect::from_xywh(r.x, r.y, r.w, r.h) {
            let mut p = c.paint();
            p.anti_alias = false;
            let mask = self.clip.as_ref().map(|(_, m)| m);
            self.pix.fill_rect(rect, &p, Transform::identity(), mask);
        }
    }

    fn rrect_path(r: Rect, rad: f32) -> Option<tiny_skia::Path> {
        let rad = rad.min(r.w / 2.0).min(r.h / 2.0).max(0.0);
        let mut pb = PathBuilder::new();
        let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
        // Cubic approximation of quarter circles.
        let k = rad * 0.447_715;
        pb.move_to(x0 + rad, y0);
        pb.line_to(x1 - rad, y0);
        pb.cubic_to(x1 - k, y0, x1, y0 + k, x1, y0 + rad);
        pb.line_to(x1, y1 - rad);
        pb.cubic_to(x1, y1 - k, x1 - k, y1, x1 - rad, y1);
        pb.line_to(x0 + rad, y1);
        pb.cubic_to(x0 + k, y1, x0, y1 - k, x0, y1 - rad);
        pb.line_to(x0, y0 + rad);
        pb.cubic_to(x0, y0 + k, x0 + k, y0, x0 + rad, y0);
        pb.close();
        pb.finish()
    }

    pub fn fill_rrect(&mut self, r: Rect, rad: f32, c: Color) {
        if c.a == 0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        if rad < 0.5 {
            return self.fill_rect(r, c);
        }
        if let Some(path) = Self::rrect_path(r, rad) {
            let mask = self.clip.as_ref().map(|(_, m)| m);
            self.pix
                .fill_path(&path, &c.paint(), FillRule::Winding, Transform::identity(), mask);
        }
    }

    pub fn stroke_rrect(&mut self, r: Rect, rad: f32, width: f32, c: Color) {
        if c.a == 0 || r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let half = width / 2.0;
        let r = Rect::new(r.x + half, r.y + half, r.w - width, r.h - width);
        if let Some(path) = Self::rrect_path(r, (rad - half).max(0.0)) {
            let stroke = Stroke {
                width,
                ..Stroke::default()
            };
            let mask = self.clip.as_ref().map(|(_, m)| m);
            self.pix.stroke_path(&path, &c.paint(), &stroke, Transform::identity(), mask);
        }
    }

    // ------------------------------------------------------------ text

    fn font_index(f: FontId) -> u8 {
        match f {
            FontId::Regular => 0,
            FontId::Mono => 2,
            FontId::Bold => 3,
            FontId::Italic => 6,
            FontId::BoldItalic => 7,
        }
    }

    /// Scale converting font units to pixels for an em size of `px`.
    fn units_scale(font: &FontVec, px: f32) -> f32 {
        px / font.units_per_em().unwrap_or(2048.0)
    }

    /// Ascent and descent (descent negative) of the regular font at `px`.
    pub fn metrics(&self, px: f32, f: FontId) -> (f32, f32) {
        let font = self.fonts.get(Self::font_index(f));
        let k = Self::units_scale(font, px);
        (font.ascent_unscaled() * k, font.descent_unscaled() * k)
    }

    pub fn measure(&mut self, s: &str, px: f32, f: FontId) -> f32 {
        let primary = Self::font_index(f);
        let mut x = 0.0;
        let mut prev: Option<(u8, GlyphId)> = None;
        for c in s.chars() {
            let (fi, g) = self.fonts.resolve(primary, c);
            let font = self.fonts.get(fi);
            let k = Self::units_scale(font, px);
            if let Some((pf, pg)) = prev {
                if pf == fi {
                    x += font.kern_unscaled(pg, g) * k;
                }
            }
            x += font.h_advance_unscaled(g) * k;
            prev = Some((fi, g));
        }
        x
    }

    /// Draws text with its baseline at `y`. Returns the advance width.
    pub fn text(&mut self, s: &str, x: f32, y: f32, px: f32, f: FontId, c: Color) -> f32 {
        let primary = Self::font_index(f);
        let mut pen = x;
        let mut prev: Option<(u8, GlyphId)> = None;
        let size_q = (px * 4.0).round().clamp(1.0, 65535.0) as u16;
        let baseline = y.round() as i32;
        for ch in s.chars() {
            let (fi, g) = self.fonts.resolve(primary, ch);
            let k = Self::units_scale(self.fonts.get(fi), px);
            if let Some((pf, pg)) = prev {
                if pf == fi {
                    pen += self.fonts.get(fi).kern_unscaled(pg, g) * k;
                }
            }
            let ix = pen.floor();
            let sub = ((pen - ix) * 4.0).floor() as u8 % 4;
            let key = GlyphKey {
                font: fi,
                glyph: g.0,
                size_q,
                sub_x: sub,
            };
            if !self.cache.contains_key(&key) {
                let bmp = self.raster(fi, g, px, sub as f32 / 4.0);
                self.cache.insert(key, bmp);
            }
            let bmp = &self.cache[&key];
            if bmp.w > 0 {
                blit(
                    &mut self.pix,
                    bmp,
                    ix as i32 + bmp.x,
                    baseline + bmp.y,
                    c,
                    self.clip.as_ref().map(|(r, _)| *r),
                    &self.lut,
                );
            }
            pen += self.fonts.get(fi).h_advance_unscaled(g) * k;
            prev = Some((fi, g));
        }
        pen - x
    }

    fn raster(&self, fi: u8, g: GlyphId, px: f32, sub_x: f32) -> GlyphBmp {
        let font = self.fonts.get(fi);
        let height = font.height_unscaled();
        let upem = font.units_per_em().unwrap_or(2048.0);
        // ab_glyph scales so that `height_unscaled` maps to the PxScale value.
        let scale = ab_glyph::PxScale::from(px * height / upem);
        let glyph = g.with_scale_and_position(scale, Point { x: sub_x, y: 0.0 });
        let Some(outlined) = font.outline_glyph(glyph) else {
            return GlyphBmp {
                x: 0,
                y: 0,
                w: 0,
                h: 0,
                cov: Vec::new(),
            };
        };
        let b = outlined.px_bounds();
        let (w, h) = (b.width() as u32, b.height() as u32);
        let mut cov = vec![0u8; (w * h) as usize];
        outlined.draw(|x, y, v| {
            let i = (y * w + x) as usize;
            if i < cov.len() {
                cov[i] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
        GlyphBmp {
            x: b.min.x as i32,
            y: b.min.y as i32,
            w,
            h,
            cov,
        }
    }

    /// Draws text aligned inside `r`, vertically centered on the font's
    /// ascent/descent box (or with an explicit baseline offset).
    pub fn text_in(&mut self, s: &str, r: Rect, px: f32, f: FontId, c: Color, align: Align) -> f32 {
        let w = self.measure(s, px, f);
        let (asc, desc) = self.metrics(px, f);
        let baseline = r.center_y() + (asc + desc) / 2.0;
        let x = match align {
            Align::Left => r.x,
            Align::Center => r.x + (r.w - w) / 2.0,
        };
        self.text(s, x.round(), baseline, px, f, c)
    }

    /// Draws a premultiplied RGBA image scaled into `dest` (bilinear).
    pub fn draw_image(&mut self, img: &Pixmap, dest: Rect) {
        if dest.w <= 0.0 || dest.h <= 0.0 || img.width() == 0 || img.height() == 0 {
            return;
        }
        let sx = dest.w / img.width() as f32;
        let sy = dest.h / img.height() as f32;
        let exact = (sx - 1.0).abs() < 0.001 && (sy - 1.0).abs() < 0.001;
        let paint = PixmapPaint {
            quality: if exact { tiny_skia::FilterQuality::Nearest } else { tiny_skia::FilterQuality::Bilinear },
            ..PixmapPaint::default()
        };
        let (x, y) = if exact { (dest.x.round(), dest.y.round()) } else { (dest.x, dest.y) };
        let t = Transform::from_row(sx, 0.0, 0.0, sy, x, y);
        let mask = self.clip.as_ref().map(|(_, m)| m);
        self.pix.draw_pixmap(0, 0, img.as_ref(), &paint, t, mask);
    }

    /// Copies the frame into a 0RGB buffer (softbuffer format).
    pub fn copy_to(&self, out: &mut [u32]) {
        let data = self.pix.data();
        for (o, px) in out.iter_mut().zip(data.as_chunks::<4>().0) {
            *o = (px[0] as u32) << 16 | (px[1] as u32) << 8 | px[2] as u32;
        }
    }
}

fn blit(pix: &mut Pixmap, g: &GlyphBmp, x0: i32, y0: i32, c: Color, clip: Option<Rect>, lut: &[u8; 256]) {
    let (pw, ph) = (pix.width() as i32, pix.height() as i32);
    let (cx0, cy0, cx1, cy1) = match clip {
        Some(r) => (
            r.x.max(0.0) as i32,
            r.y.max(0.0) as i32,
            (r.right() as i32).min(pw),
            (r.bottom() as i32).min(ph),
        ),
        None => (0, 0, pw, ph),
    };
    let data = pix.data_mut();
    let ca = c.a as u32;
    for gy in 0..g.h as i32 {
        let y = y0 + gy;
        if y < cy0 || y >= cy1 {
            continue;
        }
        for gx in 0..g.w as i32 {
            let x = x0 + gx;
            if x < cx0 || x >= cx1 {
                continue;
            }
            let cov = lut[g.cov[(gy * g.w as i32 + gx) as usize] as usize] as u32;
            if cov == 0 {
                continue;
            }
            let a = cov * ca / 255;
            let i = ((y * pw + x) * 4) as usize;
            let inv = 255 - a;
            data[i] = ((c.r as u32 * a + data[i] as u32 * inv) / 255) as u8;
            data[i + 1] = ((c.g as u32 * a + data[i + 1] as u32 * inv) / 255) as u8;
            data[i + 2] = ((c.b as u32 * a + data[i + 2] as u32 * inv) / 255) as u8;
            data[i + 3] = (a + data[i + 3] as u32 * inv / 255) as u8;
        }
    }
}
