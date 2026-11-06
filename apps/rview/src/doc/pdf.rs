//! Text of PDF pages, for selection and copying. `Windows.Data.Pdf` only
//! draws pages, so the file is read a second time here (by lopdf) and the
//! page content is interpreted just far enough to know which character sits
//! where. Glyph positions use the same units as the sizes reported by the
//! renderer, so they line up with the page images at any zoom.

use crate::ui::gfx::Color;
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Encoding, Object, ObjectId};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

/// Limit for one decompressed stream (a crafted file must not exhaust memory).
const MAX_STREAM: usize = 256 << 20;
/// Limit of content operators on one page, form XObjects included.
const MAX_OPS: usize = 20_000_000;
const MAX_FORM_DEPTH: u32 = 12;
/// Gap between glyphs, in font sizes, that counts as a space.
const WORD_GAP: f32 = 0.18;
/// Gap, in font sizes, that separates columns rather than words.
const COLUMN_GAP: f32 = 2.0;
/// Fonts this heavy are bold even when their name does not say so.
const BOLD_STEM: f32 = 110.0;

/// One character (or ligature) on a page. Spaces and line breaks that the
/// file only implies by positioning are added as glyphs too; a line break
/// has zero width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    /// Offset of its text in `PageText::text`.
    pub byte: u32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub style: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    /// Font size in page units.
    pub size: f32,
    pub color: Option<Color>,
}

/// Where a glyph sits along its line of text, in page units.
#[derive(Clone, Copy, Debug)]
struct Pen {
    origin: (f32, f32),
    end: (f32, f32),
    /// Unit vector along the line.
    dir: (f32, f32),
    size: f32,
}

#[derive(Default, Debug)]
pub struct PageText {
    pub text: String,
    pub glyphs: Vec<Glyph>,
    pub styles: Vec<TextStyle>,
    last: Option<Pen>,
}

impl PageText {
    /// Text of glyph `i`.
    pub fn glyph_text(&self, i: usize) -> &str {
        let start = self.glyphs[i].byte as usize;
        let end = self.glyphs.get(i + 1).map_or(self.text.len(), |g| g.byte as usize);
        &self.text[start..end]
    }

    pub fn is_break(&self, i: usize) -> bool {
        self.glyphs[i].w == 0.0 && self.glyph_text(i) == "\n"
    }

    /// Boundary (glyph index, up to `glyphs.len()`) nearest to a page point.
    pub fn hit(&self, x: f32, y: f32) -> Option<u32> {
        let mut best: Option<(f32, usize)> = None;
        for (i, g) in self.glyphs.iter().enumerate() {
            if g.w <= 0.0 {
                continue;
            }
            let dy = (g.y - y).max(y - (g.y + g.h)).max(0.0);
            let dx = (g.x - x).max(x - (g.x + g.w)).max(0.0);
            // Reaching along a line is more likely than reaching across lines.
            let score = dy * 4.0 + dx;
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, i));
            }
        }
        let (_, i) = best?;
        let g = &self.glyphs[i];
        Some(if x > g.x + g.w / 2.0 { i as u32 + 1 } else { i as u32 })
    }

    /// Whether a page point is on a glyph.
    pub fn over(&self, x: f32, y: f32) -> bool {
        self.glyphs.iter().any(|g| g.w > 0.0 && x >= g.x && x < g.x + g.w && y >= g.y && y < g.y + g.h)
    }

    /// Glyph index of the byte offset `byte`.
    pub fn glyph_at_byte(&self, byte: usize) -> u32 {
        self.glyphs.partition_point(|g| (g.byte as usize) <= byte).saturating_sub(1) as u32
    }

    fn style_id(&mut self, s: TextStyle) -> u16 {
        if let Some(i) = self.styles.iter().rposition(|x| *x == s) {
            return i as u16;
        }
        if self.styles.len() >= u16::MAX as usize {
            return 0;
        }
        self.styles.push(s);
        (self.styles.len() - 1) as u16
    }

    fn raw_push(&mut self, s: &str, x: f32, y: f32, w: f32, h: f32, style: u16) {
        self.glyphs.push(Glyph { byte: self.text.len() as u32, x, y, w, h, style });
        self.text.push_str(s);
    }

    /// Appends a glyph, adding the space or line break that separates it
    /// from the previous one. Lines run along the text's own direction, so
    /// this holds for rotated pages too.
    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, s: &str, x: f32, y: f32, w: f32, h: f32, style: u16, pen: Pen) {
        if let (Some(p), Some(g)) = (self.last, self.glyphs.last().copied()) {
            let (rx, ry) = (pen.origin.0 - p.end.0, pen.origin.1 - p.end.1);
            let along = rx * p.dir.0 + ry * p.dir.1;
            let across = (ry * p.dir.0 - rx * p.dir.1).abs();
            let size = p.size.max(pen.size);
            if across >= 0.5 * size || along <= -size {
                self.raw_push("\n", g.x + g.w, g.y, 0.0, g.h, g.style);
            } else if along > WORD_GAP * size && !s.starts_with(' ') && !self.text.ends_with(' ') {
                let sep = if along > COLUMN_GAP * size { "\t" } else { " " };
                let (ex, ey) = (p.end.0.min(pen.origin.0), p.end.1.min(pen.origin.1));
                if p.dir.0.abs() >= p.dir.1.abs() {
                    self.raw_push(sep, ex, g.y, rx.abs(), g.h, g.style);
                } else {
                    self.raw_push(sep, g.x, ey, g.w, ry.abs(), g.style);
                }
            }
        }
        self.raw_push(s, x, y, w, h, style);
        self.last = Some(pen);
    }
}

type Matrix = [f32; 6];
const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `a` applied first, then `b`.
fn mul(a: &Matrix, b: &Matrix) -> Matrix {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn apply(m: &Matrix, x: f32, y: f32) -> (f32, f32) {
    (x * m[0] + y * m[2] + m[4], x * m[1] + y * m[3] + m[5])
}

/// From PDF user space to rendered page units.
#[derive(Clone, Copy)]
struct PageMap {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    rotate: i64,
    sx: f32,
    sy: f32,
}

impl PageMap {
    fn new(cropbox: [f32; 4], rotate: i64, size: (f32, f32)) -> PageMap {
        let [x0, y0, x1, y1] = cropbox;
        let (w, h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
        let (shown_w, shown_h) = if rotate % 180 == 0 { (w, h) } else { (h, w) };
        PageMap { x0, y0, x1, y1, rotate, sx: size.0 / shown_w, sy: size.1 / shown_h }
    }

    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        let (px, py) = match self.rotate {
            90 => (y - self.y0, x - self.x0),
            180 => (self.x1 - x, y - self.y0),
            270 => (self.y1 - y, self.x1 - x),
            _ => (x - self.x0, self.y1 - y),
        };
        (px * self.sx, py * self.sy)
    }
}

/// Reads a number through references.
fn number(doc: &Document, o: &Object) -> Option<f32> {
    doc.dereference(o).ok()?.1.as_float().ok()
}

/// A value of a page, or of the nearest ancestor that has it.
fn inherited<'a>(doc: &'a Document, page: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut dict = doc.get_dictionary(page).ok()?;
    for _ in 0..64 {
        if let Ok(v) = dict.get(key) {
            return Some(v);
        }
        dict = doc.get_dictionary(dict.get(b"Parent").ok()?.as_reference().ok()?).ok()?;
    }
    None
}

fn page_box(doc: &Document, page: ObjectId) -> Option<[f32; 4]> {
    let v = inherited(doc, page, b"CropBox").or_else(|| inherited(doc, page, b"MediaBox"))?;
    let a = doc.dereference(v).ok()?.1.as_array().ok()?;
    let n: Vec<f32> = a.iter().filter_map(|o| number(doc, o)).collect();
    (n.len() == 4).then(|| [n[0].min(n[2]), n[1].min(n[3]), n[0].max(n[2]), n[1].max(n[3])])
}

struct Font<'a> {
    /// Codes are two bytes (composite fonts).
    wide: bool,
    /// UTF-16 codes (predefined `Uni…-UCS2/UTF16` CMaps).
    utf16: bool,
    to_unicode: Option<Encoding<'static>>,
    base: Option<Encoding<'a>>,
    /// Text of the glyph names in `/Differences` that lopdf does not know.
    names: HashMap<u32, String>,
    first: u32,
    widths: Vec<f32>,
    cid_widths: HashMap<u32, f32>,
    default_width: f32,
    /// From font units to thousandths of the font size (Type 3 fonts have
    /// their own units).
    width_scale: f32,
    /// Type 3 glyphs have made-up names; TeX's still follow its encodings.
    tex_codes: bool,
    bold: bool,
    italic: bool,
    mono: bool,
    decoded: RefCell<HashMap<u32, Rc<str>>>,
}

fn name_of(dict: &Dictionary, doc: &Document, key: &[u8]) -> String {
    dict.get_deref(key, doc).and_then(Object::as_name).map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_default()
}

/// Text of a glyph name: ligatures and `uniXXXX` names.
fn glyph_name_text(name: &[u8]) -> Option<String> {
    Some(match name {
        b"ff" => "ff".to_string(),
        b"fi" => "fi".to_string(),
        b"fl" => "fl".to_string(),
        b"ffi" => "ffi".to_string(),
        b"ffl" => "ffl".to_string(),
        b"dotlessi" => "ı".to_string(),
        _ => {
            let hex = std::str::from_utf8(name.strip_prefix(b"uni")?).ok()?;
            char::from_u32(u32::from_str_radix(hex.get(..4)?, 16).ok()?)?.to_string()
        }
    })
}

/// Named glyphs of a simple font's `/Encoding` dictionary.
fn difference_names(doc: &Document, dict: &Dictionary) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    let Ok(enc) = dict.get_deref(b"Encoding", doc).and_then(Object::as_dict) else { return out };
    let Ok(list) = enc.get_deref(b"Differences", doc).and_then(Object::as_array) else { return out };
    let mut code = 0u32;
    for o in list {
        if let Ok(n) = o.as_i64() {
            code = n.max(0) as u32;
        } else if let Ok(name) = o.as_name() {
            if let Some(t) = glyph_name_text(name) {
                out.insert(code, t);
            }
            code += 1;
        }
    }
    out
}

/// The `/W` array of a CID font: `c [w1 w2 …]` or `first last w`.
fn cid_widths(doc: &Document, cid: &Dictionary) -> HashMap<u32, f32> {
    let mut out = HashMap::new();
    let Ok(a) = cid.get_deref(b"W", doc).and_then(Object::as_array) else { return out };
    let mut i = 0;
    while i + 1 < a.len() {
        let Some(first) = number(doc, &a[i]) else { break };
        let first = first as u32;
        if let Ok(list) = doc.dereference(&a[i + 1]).and_then(|(_, o)| o.as_array()) {
            for (k, w) in list.iter().enumerate() {
                if let Some(w) = number(doc, w) {
                    out.insert(first + k as u32, w);
                }
            }
            i += 2;
        } else {
            let (Some(last), Some(w)) = (number(doc, &a[i + 1]), a.get(i + 2).and_then(|o| number(doc, o))) else { break };
            for c in first..=(last as u32).min(first.saturating_add(0xFFFF)) {
                out.insert(c, w);
            }
            i += 3;
        }
    }
    out
}

impl<'a> Font<'a> {
    fn load(doc: &'a Document, dict: &'a Dictionary) -> Font<'a> {
        let subtype = dict.get_deref(b"Subtype", doc).and_then(Object::as_name).unwrap_or(b"");
        let wide = subtype == b"Type0";
        let type3 = subtype == b"Type3";
        let font_matrix = dict.get_deref(b"FontMatrix", doc).and_then(Object::as_array).ok().and_then(|a| a.first()).and_then(|o| number(doc, o));
        let encoding = name_of(dict, doc, b"Encoding");
        // The CID font holds the metrics of a composite font.
        let cid = dict
            .get_deref(b"DescendantFonts", doc)
            .and_then(Object::as_array)
            .ok()
            .and_then(|a| a.first())
            .and_then(|o| doc.dereference(o).ok())
            .and_then(|(_, o)| o.as_dict().ok());
        let metrics = cid.unwrap_or(dict);
        let descriptor = metrics.get_deref(b"FontDescriptor", doc).and_then(Object::as_dict).ok();
        let flags = descriptor.and_then(|d| d.get(b"Flags").ok()).and_then(|o| o.as_i64().ok()).unwrap_or(0);
        let described = |key: &[u8]| descriptor.and_then(|d| d.get(key).ok()).and_then(|o| number(doc, o)).unwrap_or(0.0);
        let name = name_of(dict, doc, b"BaseFont").to_ascii_lowercase();
        let mono = flags & 1 != 0 || ["courier", "consolas", "mono", "typewriter"].iter().any(|n| name.contains(n));
        // Without a ToUnicode map, `/Encoding` decides; with one, the map wins.
        let to_unicode = {
            let mut d = dict.clone();
            d.remove(b"Encoding");
            match d.get_font_encoding_with_limit(doc, MAX_STREAM) {
                Ok(Encoding::UnicodeMapEncoding(m)) => Some(Encoding::UnicodeMapEncoding(m)),
                _ => None,
            }
        };
        let base = if wide { None } else { dict.get_font_encoding_with_limit(doc, MAX_STREAM).ok() };
        let widths = if wide {
            Vec::new()
        } else {
            metrics.get_deref(b"Widths", doc).and_then(Object::as_array).map_or(Vec::new(), |a| a.iter().map(|o| number(doc, o).unwrap_or(0.0)).collect())
        };
        let missing = descriptor.and_then(|d| d.get(b"MissingWidth").ok()).and_then(|o| number(doc, o));
        let default_width = if wide {
            metrics.get_deref(b"DW", doc).ok().and_then(|o| number(doc, o)).unwrap_or(1000.0)
        } else {
            // Fonts without widths are the standard 14: a typical advance.
            missing.unwrap_or(if mono { 600.0 } else { 500.0 })
        };
        Font {
            wide,
            utf16: encoding.starts_with("Uni") && (encoding.contains("UCS2") || encoding.contains("UTF16")),
            to_unicode,
            base,
            names: if wide { HashMap::new() } else { difference_names(doc, dict) },
            first: metrics.get(b"FirstChar").ok().and_then(|o| number(doc, o)).unwrap_or(0.0) as u32,
            widths,
            cid_widths: if wide { cid_widths(doc, metrics) } else { HashMap::new() },
            default_width,
            width_scale: if type3 { font_matrix.unwrap_or(0.001) * 1000.0 } else { 1.0 },
            tex_codes: type3,
            bold: flags & (1 << 18) != 0 || name.contains("bold") || name.contains("black") || name.contains("heavy") || described(b"StemV") >= BOLD_STEM || described(b"FontWeight") >= 600.0,
            italic: flags & 64 != 0 || name.contains("italic") || name.contains("oblique") || described(b"ItalicAngle") != 0.0,
            mono,
            decoded: RefCell::new(HashMap::new()),
        }
    }

    /// Glyph width in thousandths of the font size.
    fn width(&self, code: u32) -> f32 {
        if self.wide {
            return self.cid_widths.get(&code).copied().unwrap_or(self.default_width);
        }
        code.checked_sub(self.first).and_then(|i| self.widths.get(i as usize)).copied().unwrap_or(self.default_width) * self.width_scale
    }

    fn decode_raw(&self, code: u32, bytes: &[u8]) -> String {
        let usable = |s: String| Some(s).filter(|s| !s.is_empty() && !s.contains('\u{FFFD}'));
        if let Some(s) = self.to_unicode.as_ref().and_then(|m| m.bytes_to_string(bytes).ok()).and_then(usable) {
            return s;
        }
        if let Some(s) = self.names.get(&code) {
            return s.clone();
        }
        if self.tex_codes {
            // The ligatures of the T1 and OT1 encodings.
            const LIGATURES: [&str; 5] = ["ff", "fi", "fl", "ffi", "ffl"];
            if let Some(s) = code.checked_sub(27).or(code.checked_sub(11)).and_then(|i| LIGATURES.get(i as usize)) {
                return s.to_string();
            }
        }
        if let Some(s) = self.base.as_ref().and_then(|e| e.bytes_to_string(bytes).ok()).and_then(usable) {
            return s;
        }
        if self.utf16 {
            let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|b| u16::from_be_bytes(*b)).collect();
            return String::from_utf16_lossy(&units);
        }
        "\u{FFFD}".to_string()
    }

    fn decode(&self, code: u32, bytes: &[u8]) -> Rc<str> {
        if let Some(s) = self.decoded.borrow().get(&code) {
            return s.clone();
        }
        // Text for people: no control characters, plain spaces and hyphens.
        let s: String = self.decode_raw(code, bytes).chars().filter(|c| !c.is_control()).map(|c| match c {
            '\u{A0}' => ' ',
            '\u{AD}' => '-',
            c => c,
        }).collect();
        let s: Rc<str> = if s.is_empty() { " ".into() } else { s.into() };
        self.decoded.borrow_mut().insert(code, s.clone());
        s
    }
}

/// The graphics state that matters for text.
#[derive(Clone)]
struct State<'a> {
    ctm: Matrix,
    char_space: f32,
    word_space: f32,
    scale: f32,
    leading: f32,
    size: f32,
    rise: f32,
    font: Option<Rc<Font<'a>>>,
    color: Option<Color>,
}

struct Reader<'a> {
    doc: &'a Document,
    map: PageMap,
    fonts: &'a RefCell<FontCache<'a>>,
    out: PageText,
    ops: usize,
}

type FontCache<'a> = HashMap<*const Dictionary, Rc<Font<'a>>>;

/// A text color worth keeping when pasted on a white page.
fn ink(r: f32, g: f32, b: f32) -> Option<Color> {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let color = Color { r: c(r), g: c(g), b: c(b), a: 255 };
    let lum = crate::rich::layout::luminance(color);
    (lum > 16.0 && lum < 235.0).then_some(color)
}

fn floats(ops: &[Object]) -> Vec<f32> {
    ops.iter().filter_map(|o| o.as_float().ok()).collect()
}

impl<'a> Reader<'a> {
    fn font(&self, name: &[u8], resources: &[&'a Dictionary]) -> Option<Rc<Font<'a>>> {
        let doc = self.doc;
        let dict = resources.iter().find_map(|r| {
            let fonts = r.get_deref(b"Font", doc).and_then(Object::as_dict).ok()?;
            fonts.get_deref(name, doc).and_then(Object::as_dict).ok()
        })?;
        let key = dict as *const Dictionary;
        if let Some(f) = self.fonts.borrow().get(&key) {
            return Some(f.clone());
        }
        let f = Rc::new(Font::load(doc, dict));
        self.fonts.borrow_mut().insert(key, f.clone());
        Some(f)
    }

    fn show(&mut self, gs: &State<'a>, tm: &mut Matrix, bytes: &[u8]) {
        let Some(font) = gs.font.clone() else { return };
        let step = if font.wide { 2 } else { 1 };
        for raw in bytes.chunks(step) {
            let code = raw.iter().fold(0u32, |a, b| a * 256 + *b as u32);
            let advance = font.width(code) / 1000.0 * gs.size;
            let text = font.decode(code, raw);
            let m = mul(&mul(&[gs.scale, 0.0, 0.0, 1.0, 0.0, gs.rise], tm), &gs.ctm);
            // The glyph's box: from below the baseline to above it.
            let corners = [(0.0, -0.22 * gs.size), (advance, -0.22 * gs.size), (0.0, 0.82 * gs.size), (advance, 0.82 * gs.size)];
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for (cx, cy) in corners {
                let (ux, uy) = apply(&m, cx, cy);
                let (px, py) = self.map.point(ux, uy);
                x0 = x0.min(px);
                y0 = y0.min(py);
                x1 = x1.max(px);
                y1 = y1.max(py);
            }
            let (w, h) = (x1 - x0, y1 - y0);
            let at = |map: &PageMap, x: f32, y: f32| {
                let (ux, uy) = apply(&m, x, y);
                map.point(ux, uy)
            };
            let origin = at(&self.map, 0.0, 0.0);
            let end = at(&self.map, advance, 0.0);
            let up = at(&self.map, 0.0, gs.size);
            let along = at(&self.map, gs.size, 0.0);
            let px_size = (up.0 - origin.0).hypot(up.1 - origin.1);
            let reach = (along.0 - origin.0).hypot(along.1 - origin.1).max(f32::MIN_POSITIVE);
            if w.is_finite() && h.is_finite() && px_size > 0.0 {
                let dir = ((along.0 - origin.0) / reach, (along.1 - origin.1) / reach);
                let style = self.out.style_id(TextStyle { bold: font.bold, italic: font.italic, mono: font.mono, size: px_size, color: gs.color });
                self.out.push(&text, x0, y0, w.max(0.01), h, style, Pen { origin, end, dir, size: px_size });
            }
            let spacing = gs.char_space + if !font.wide && code == 32 { gs.word_space } else { 0.0 };
            *tm = mul(&[1.0, 0.0, 0.0, 1.0, (advance + spacing) * gs.scale, 0.0], tm);
        }
    }

    fn form(&mut self, name: &[u8], gs: &State<'a>, resources: &[&'a Dictionary], depth: u32) {
        if depth >= MAX_FORM_DEPTH {
            return;
        }
        let doc = self.doc;
        let Some(stream) = resources.iter().find_map(|r| {
            let x = r.get_deref(b"XObject", doc).and_then(Object::as_dict).ok()?;
            x.get_deref(name, doc).and_then(Object::as_stream).ok()
        }) else {
            return;
        };
        if stream.dict.get_deref(b"Subtype", doc).and_then(Object::as_name).map_or(true, |s| s != b"Form") {
            return;
        }
        let Ok(data) = stream.decompressed_content_with_limit(MAX_STREAM).or_else(|_| Ok::<_, lopdf::Error>(stream.content.clone())) else { return };
        let Ok(content) = Content::decode(&data) else { return };
        let mut inner = gs.clone();
        if let Ok(a) = stream.dict.get_deref(b"Matrix", doc).and_then(Object::as_array) {
            let n: Vec<f32> = a.iter().filter_map(|o| number(doc, o)).collect();
            if let [a, b, c, d, e, f] = n[..] {
                inner.ctm = mul(&[a, b, c, d, e, f], &inner.ctm);
            }
        }
        let own = stream.dict.get_deref(b"Resources", doc).and_then(Object::as_dict).ok();
        let mut res: Vec<&'a Dictionary> = own.into_iter().collect();
        res.extend_from_slice(resources);
        self.run(&content, inner, &res, depth + 1);
    }

    fn run(&mut self, content: &Content<Vec<lopdf::content::Operation>>, start: State<'a>, resources: &[&'a Dictionary], depth: u32) {
        let mut gs = start;
        let mut stack: Vec<State<'a>> = Vec::new();
        let mut tm = IDENTITY;
        let mut tlm = IDENTITY;
        for op in &content.operations {
            self.ops += 1;
            if self.ops > MAX_OPS {
                return;
            }
            let args = &op.operands;
            match op.operator.as_str() {
                "q" => stack.push(gs.clone()),
                "Q" => {
                    if let Some(s) = stack.pop() {
                        gs = s;
                    }
                }
                "cm" => {
                    if let [a, b, c, d, e, f] = floats(args)[..] {
                        gs.ctm = mul(&[a, b, c, d, e, f], &gs.ctm);
                    }
                }
                "BT" => {
                    tm = IDENTITY;
                    tlm = IDENTITY;
                }
                "Tc" => gs.char_space = args.first().and_then(|o| o.as_float().ok()).unwrap_or(0.0),
                "Tw" => gs.word_space = args.first().and_then(|o| o.as_float().ok()).unwrap_or(0.0),
                "Tz" => gs.scale = args.first().and_then(|o| o.as_float().ok()).unwrap_or(100.0) / 100.0,
                "TL" => gs.leading = args.first().and_then(|o| o.as_float().ok()).unwrap_or(0.0),
                "Ts" => gs.rise = args.first().and_then(|o| o.as_float().ok()).unwrap_or(0.0),
                "Tf" => {
                    if let (Some(name), Some(size)) = (args.first().and_then(|o| o.as_name().ok()), args.get(1).and_then(|o| o.as_float().ok())) {
                        gs.font = self.font(name, resources);
                        gs.size = size;
                    }
                }
                "Td" | "TD" => {
                    if let [x, y] = floats(args)[..] {
                        if op.operator == "TD" {
                            gs.leading = -y;
                        }
                        tlm = mul(&[1.0, 0.0, 0.0, 1.0, x, y], &tlm);
                        tm = tlm;
                    }
                }
                "Tm" => {
                    if let [a, b, c, d, e, f] = floats(args)[..] {
                        tlm = [a, b, c, d, e, f];
                        tm = tlm;
                    }
                }
                "T*" => {
                    tlm = mul(&[1.0, 0.0, 0.0, 1.0, 0.0, -gs.leading], &tlm);
                    tm = tlm;
                }
                "Tj" | "'" | "\"" => {
                    if op.operator != "Tj" {
                        if op.operator == "\"" {
                            if let [w, c, ..] = floats(args)[..] {
                                gs.word_space = w;
                                gs.char_space = c;
                            }
                        }
                        tlm = mul(&[1.0, 0.0, 0.0, 1.0, 0.0, -gs.leading], &tlm);
                        tm = tlm;
                    }
                    if let Some(Ok(s)) = args.last().map(Object::as_str) {
                        self.show(&gs, &mut tm, s);
                    }
                }
                "TJ" => {
                    let Some(Ok(items)) = args.first().map(Object::as_array) else { continue };
                    for item in items {
                        if let Ok(s) = item.as_str() {
                            self.show(&gs, &mut tm, s);
                        } else if let Ok(n) = item.as_float() {
                            tm = mul(&[1.0, 0.0, 0.0, 1.0, -n / 1000.0 * gs.size * gs.scale, 0.0], &tm);
                        }
                    }
                }
                "g" => {
                    if let [v] = floats(args)[..] {
                        gs.color = ink(v, v, v);
                    }
                }
                "rg" => {
                    if let [r, g, b] = floats(args)[..] {
                        gs.color = ink(r, g, b);
                    }
                }
                "k" => {
                    if let [c, m, y, k] = floats(args)[..] {
                        gs.color = ink((1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k));
                    }
                }
                "sc" | "scn" => match floats(args)[..] {
                    [v] => gs.color = ink(v, v, v),
                    [r, g, b] => gs.color = ink(r, g, b),
                    _ => {}
                },
                "Do" => {
                    if let Some(Ok(name)) = args.first().map(Object::as_name) {
                        let name = name.to_vec();
                        self.form(&name, &gs, resources, depth);
                    }
                }
                _ => {}
            }
        }
    }
}

fn page_resources(doc: &Document, page: ObjectId) -> Vec<&Dictionary> {
    let mut out = Vec::new();
    if let Ok((direct, ids)) = doc.get_page_resources(page) {
        out.extend(direct);
        out.extend(ids.into_iter().filter_map(|id| doc.get_dictionary(id).ok()));
    }
    out
}

fn read_page<'a>(doc: &'a Document, fonts: &'a RefCell<FontCache<'a>>, id: ObjectId, size: (f32, f32)) -> PageText {
    let Some(cropbox) = page_box(doc, id) else { return PageText::default() };
    let rotate = inherited(doc, id, b"Rotate").and_then(|o| number(doc, o)).unwrap_or(0.0) as i64;
    let map = PageMap::new(cropbox, rotate.rem_euclid(360), size);
    let Ok(data) = doc.get_page_content_with_limit(id, MAX_STREAM) else { return PageText::default() };
    let Ok(content) = Content::decode(&data) else { return PageText::default() };
    let mut r = Reader { doc, map, fonts, out: PageText::default(), ops: 0 };
    let start = State { ctm: IDENTITY, char_space: 0.0, word_space: 0.0, scale: 1.0, leading: 0.0, size: 0.0, rise: 0.0, font: None, color: None };
    r.run(&content, start, &page_resources(doc, id), 0);
    r.out
}

/// Reads the text of every page. `sizes` are the rendered page sizes;
/// `keep_going` is asked between pages so a closed document stops the work.
pub fn extract_bytes(bytes: &[u8], sizes: &[(f32, f32)], keep_going: &dyn Fn() -> bool) -> Option<Vec<PageText>> {
    let doc = Document::load_mem(bytes).ok()?;
    let fonts = RefCell::new(FontCache::new());
    let mut pages = Vec::with_capacity(sizes.len());
    let ids = doc.get_pages();
    for (i, size) in sizes.iter().enumerate() {
        if !keep_going() {
            return None;
        }
        pages.push(match ids.get(&(i as u32 + 1)) {
            Some(id) => read_page(&doc, &fonts, *id, *size),
            None => PageText::default(),
        });
    }
    Some(pages)
}

pub fn extract(path: &Path, sizes: &[(f32, f32)], keep_going: &dyn Fn() -> bool) -> Option<Vec<PageText>> {
    extract_bytes(&std::fs::read(path).ok()?, sizes, keep_going)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Builds a PDF from page objects: each page is (MediaBox and extras,
    /// content); object 3 is a font and 4 a form XObject.
    pub fn pdf(pages: &[(&str, &str)], font: &str, form: &str) -> Vec<u8> {
        let mut objects: Vec<String> = Vec::new();
        let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", 5 + i * 2)).collect();
        objects.push("<< /Type /Catalog /Pages 2 0 R >>".into());
        objects.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()));
        objects.push(font.to_string());
        objects.push(format!("<< /Type /XObject /Subtype /Form /BBox [0 0 600 800] /Resources << /Font << /F1 3 0 R >> >> /Length {} >>\nstream\n{form}\nendstream", form.len()));
        for (i, (extra, content)) in pages.iter().enumerate() {
            objects.push(format!("<< /Type /Page /Parent 2 0 R {extra} /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> /XObject << /X1 4 0 R >> >> >>", 6 + i * 2));
            objects.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
        }
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
        out
    }

    const HELVETICA: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths [278 333 474 556 556 889 722 238 333 333 389 584 278 333 278 278 556 556 556 556 556 556 556 556 556 556 333 333 584 584 584 611 975 722 722 722 722 667 611 778 722 278 556 722 611 833 722 778 667 778 722 667 611 722 667 944 667 667 611 333 278 333 584 556 333 556 611 556 611 556 333 611 611 278 278 556 278 889 611 611 611 611 389 556 333 611 556 778 556 556 500 389 280 389 584] >>";

    fn pages(data: &[u8], sizes: &[(f32, f32)]) -> Vec<PageText> {
        extract_bytes(data, sizes, &|| true).expect("readable PDF")
    }

    #[test]
    fn text_with_positions_and_spacing() {
        // One page of 200 × 100 pt, shown at 2 units per point.
        let content = "1 0 0 rg BT /F1 10 Tf 20 80 Td (Hello) Tj 40 0 Td (World) Tj 0 -12 Td [(A) -120 (B)] TJ ET";
        let data = pdf(&[("/MediaBox [0 0 200 100]", content)], HELVETICA, "");
        let p = &pages(&data, &[(400.0, 200.0)])[0];
        assert_eq!(p.text, "Hello World\nAB");
        let h = &p.glyphs[0];
        assert!((h.x - 40.0).abs() < 0.5, "x {}", h.x);
        // Baseline at y = 80 pt, so 40 units below the top of the page.
        assert!(h.y < 40.0 && h.y + h.h > 40.0, "{h:?}");
        assert!((h.w - 14.44).abs() < 0.1, "width of H: {}", h.w);
        let s = &p.styles[h.style as usize];
        assert!(s.bold && !s.italic);
        assert!((s.size - 20.0).abs() < 0.1, "size {}", s.size);
        assert_eq!(s.color, Some(Color::rgb(0xFF0000)));
    }

    #[test]
    fn hit_testing_finds_the_nearest_boundary() {
        let data = pdf(&[("/MediaBox [0 0 200 100]", "BT /F1 10 Tf 20 80 Td (Hello) Tj ET")], HELVETICA, "");
        let p = &pages(&data, &[(200.0, 100.0)])[0];
        let g = p.glyphs[1];
        assert_eq!(p.hit(g.x + 1.0, g.y + 2.0), Some(1));
        assert_eq!(p.hit(g.x + g.w - 1.0, g.y + 2.0), Some(2));
        assert_eq!(p.hit(0.0, 0.0), Some(0));
        assert_eq!(p.hit(199.0, 99.0), Some(5));
        assert!(p.over(g.x + 1.0, g.y + 2.0));
        assert!(!p.over(190.0, 90.0));
    }

    #[test]
    fn rotation_crop_box_and_forms() {
        let content = "BT /F1 10 Tf 20 80 Td (RR R) Tj ET";
        let data = pdf(
            &[
                ("/MediaBox [0 0 200 100] /Rotate 90", content),
                ("/MediaBox [0 0 200 100] /CropBox [10 10 110 90]", content),
                ("/MediaBox [0 0 200 100]", "q 1 0 0 1 50 0 cm /X1 Do Q"),
            ],
            HELVETICA,
            "BT /F1 10 Tf 0 50 Td (Form) Tj ET",
        );
        let p = pages(&data, &[(100.0, 200.0), (100.0, 80.0), (200.0, 100.0)]);
        // Rotated 90° clockwise: user (20, 80) lands at (80, 20), and the
        // line runs down the page.
        assert_eq!(p[0].text, "RR R");
        let r = p[0].glyphs[0];
        assert!(r.x < 82.0 && r.x + r.w > 72.0, "{r:?}");
        assert!(r.y > 18.0 && r.y < 22.0, "{r:?}");
        // The crop box moves the origin.
        let c = p[1].glyphs[0];
        assert!((c.x - 10.0).abs() < 0.5, "{c:?}");
        // Text inside a form XObject, moved by the outer matrix.
        assert_eq!(p[2].text, "Form");
        assert!((p[2].glyphs[0].x - 50.0).abs() < 0.5);
    }

    #[test]
    fn columns_ligatures_and_font_descriptors() {
        let font = "<< /Type /Font /Subtype /Type1 /BaseFont /ABCDEF+SFTI1000 /FirstChar 27 /LastChar 122 /Widths [600 500 500 500 500] /FontDescriptor << /Type /FontDescriptor /FontName /X /Flags 4 /ItalicAngle -14 /StemV 114 >> /Encoding << /Type /Encoding /Differences [27 /ff /fi /fl /ffi /ffl] >> >>";
        let content = "BT /F1 10 Tf 20 80 Td (o\\036ce) Tj 100 0 Td (\\034ne) Tj ET";
        let data = pdf(&[("/MediaBox [0 0 300 100]", content)], font, "");
        let p = &pages(&data, &[(300.0, 100.0)])[0];
        assert_eq!(p.text, "office\tfine");
        assert!(p.styles[0].italic && p.styles[0].bold, "{:?}", p.styles[0]);
    }

    #[test]
    fn type3_fonts_use_their_own_units() {
        // Bitmap fonts from TeX: widths scaled by the font matrix.
        let font = "<< /Type /Font /Subtype /Type3 /FontMatrix [0.01 0 0 0.01 0 0] /FontBBox [0 0 60 60] /CharProcs << >> /FirstChar 28 /LastChar 111 /Widths [50 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 50] /Encoding << /Differences [28 /a28 111 /a111] >> >>";
        let data = pdf(&[("/MediaBox [0 0 200 100]", "BT /F1 10 Tf 20 80 Td (o\\034o) Tj ET")], font, "");
        let p = &pages(&data, &[(200.0, 100.0)])[0];
        assert_eq!(p.text, "ofio");
        assert!((p.glyphs[0].w - 5.0).abs() < 0.01, "{:?}", p.glyphs[0]);
    }

    #[test]
    fn broken_or_cancelled_reads_give_nothing() {
        assert!(extract_bytes(b"not a pdf", &[(10.0, 10.0)], &|| true).is_none());
        let data = pdf(&[("/MediaBox [0 0 200 100]", "BT /F1 10 Tf (x) Tj ET")], HELVETICA, "");
        assert!(extract_bytes(&data, &[(200.0, 100.0)], &|| false).is_none());
    }

    #[test]
    fn composite_fonts_use_to_unicode() {
        // Two-byte codes through a ToUnicode CMap, with /W widths.
        let cmap = "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n2 beginbfchar\n<0001> <0041>\n<0002> <00E9>\nendbfchar\nendcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            "<< /Length 38 >>\nstream\nBT /F1 10 Tf 20 80 Td <00010002> Tj ET\nendstream".to_string(),
            "<< /Type /Font /Subtype /Type0 /BaseFont /Arial-ItalicMT /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>".to_string(),
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Arial-ItalicMT /DW 1000 /W [1 [700] 2 2 400] >>".to_string(),
            format!("<< /Length {} >>\nstream\n{cmap}\nendstream", cmap.len()),
        ];
        let mut out = b"%PDF-1.5\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for off in offsets {
            out.extend(format!("{off:010} 00000 n \n").bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
        let p = &pages(&out, &[(200.0, 100.0)])[0];
        assert_eq!(p.text, "Aé");
        assert!((p.glyphs[0].w - 7.0).abs() < 0.1 && (p.glyphs[1].w - 4.0).abs() < 0.1, "{:?}", p.glyphs);
        assert!(p.styles[0].italic);
    }
}
