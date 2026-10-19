//! The document view: holds the loaded document, zoom and scroll state,
//! and draws whatever part of it is visible.

use crate::doc::csv::{self, CsvDoc};
use crate::doc::text::{WrapIndex, TAB, WRAP_LIMIT};
use crate::doc::{Content, Kind};
use crate::i18n::{tf, tr};
use crate::media::{Done, Job, Media, MAX_EDGE};
use crate::rich::layout::{self, ImgState, Item, Layout, Palette};
use crate::rich::RichDoc;
use crate::ui::gfx::{Color, FontId, Gfx, Rect};
use crate::ui::layout as cells;
use crate::ui::theme::Theme;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tiny_skia::Pixmap;

pub const ZOOMS: [f32; 17] = [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0];
/// Memory budget for rendered PDF pages.
const PDF_CACHE_BYTES: usize = 256 << 20;
const SCROLLBAR: f32 = 12.0;

/// A scroll bar's track and thumb, when shown.
type Bar = Option<(Rect, Rect)>;

pub enum State {
    Empty,
    Loading,
    Error(String),
    Ready(Content),
}

/// What a click on a link asks the window to do.
pub enum LinkAction {
    Scrolled,
    OpenUrl(String),
    OpenFile(PathBuf, Option<String>),
    Unsupported(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

struct Img {
    state: ImgState,
    pix: Option<Pixmap>,
}

#[derive(Default)]
struct Pdf {
    sizes: Vec<(f32, f32)>,
    pages: HashMap<u32, (u32, Pixmap)>,
    requested: Vec<(u32, u32)>,
    /// Pages are shown with inverted lightness (dark mode).
    dark: bool,
    /// Page rectangles (document coordinates) from the last draw.
    rects: Vec<Rect>,
    current: usize,
}

pub struct Viewer {
    pub path: Option<PathBuf>,
    pub kind: Kind,
    pub state: State,
    pub zoom: f32,
    /// PDF: zoom follows the window width.
    pub fit: bool,
    pub wrap: bool,
    pub scroll: (f32, f32),
    pub gen: u64,
    pub hover_link: Option<String>,
    content: (f32, f32),
    view: Rect,
    pending_anchor: Option<String>,
    /// Scroll position to restore after (re)loading, as a fraction.
    restore: Option<f32>,
    rich: Option<Layout>,
    rich_key: (i32, i32, u64, bool),
    rich_pal: Option<Palette>,
    wrap_idx: Option<WrapIndex>,
    csv_cols: Option<(f32, Vec<f32>)>,
    images: HashMap<String, Img>,
    img_gen: u64,
    pdf: Pdf,
    /// Show HTML and PDF pages dark in the dark theme.
    pub dark_pages: bool,
    /// Syntax highlighting of the text document.
    hl: Option<crate::syntax::Highlighter>,
}

fn pixmap(b: crate::media::Bitmap) -> Option<Pixmap> {
    Pixmap::from_vec(b.rgba, tiny_skia::IntSize::from_wh(b.w, b.h)?)
}

/// Decodes `%xx` escapes in a URL path.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (b.get(i + 1).and_then(|c| (*c as char).to_digit(16)), b.get(i + 2).and_then(|c| (*c as char).to_digit(16))) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// Resolves an image `src` to its cache key: an absolute file path, a data
/// URI, or the remote URL itself (never fetched).
pub fn image_key(base: &Path, src: &str) -> String {
    let src = src.trim();
    if src.starts_with("data:") || src.contains("://") && !src.starts_with("file:") || src.starts_with("//") {
        return src.to_string();
    }
    let p = src.strip_prefix("file:///").or_else(|| src.strip_prefix("file://")).unwrap_or(src);
    let p = percent_decode(p.split(['?', '#']).next().unwrap_or(p));
    let path = Path::new(&p);
    let full = if path.is_absolute() { path.to_path_buf() } else { base.join(path) };
    full.to_string_lossy().into_owned()
}

/// Truncates `s` with an ellipsis to fit `max` pixels.
fn fit_text(g: &mut Gfx, s: &str, max: f32, px: f32, font: FontId) -> String {
    if g.measure(s, px, font) <= max {
        return s.to_string();
    }
    let ell = g.measure("…", px, font);
    let mut out = String::new();
    let mut w = 0.0;
    for c in s.chars() {
        let cw = g.advance(c, px, font);
        if w + cw + ell > max {
            break;
        }
        w += cw;
        out.push(c);
    }
    out.push('…');
    out
}

impl Viewer {
    pub fn new(wrap: bool) -> Viewer {
        Viewer {
            path: None,
            kind: Kind::Text,
            state: State::Empty,
            zoom: 1.0,
            fit: true,
            wrap,
            scroll: (0.0, 0.0),
            gen: 0,
            hover_link: None,
            content: (0.0, 0.0),
            view: Rect::default(),
            pending_anchor: None,
            restore: None,
            rich: None,
            rich_key: (0, 0, 0, false),
            rich_pal: None,
            wrap_idx: None,
            csv_cols: None,
            images: HashMap::new(),
            img_gen: 0,
            pdf: Pdf::default(),
            dark_pages: true,
            hl: None,
        }
    }

    /// Starts loading `path`; returns the load generation. `keep` keeps the
    /// zoom and scroll position (reload).
    pub fn begin(&mut self, path: &Path, keep: bool, anchor: Option<String>) -> u64 {
        self.gen += 1;
        self.restore = keep.then(|| if self.content.1 > 0.0 { self.scroll.1 / self.content.1 } else { 0.0 });
        let kind = crate::doc::Kind::of(path);
        if !keep || kind != self.kind {
            self.zoom = 1.0;
            self.fit = true;
        }
        self.path = Some(path.to_path_buf());
        self.kind = kind;
        self.state = State::Loading;
        self.scroll = (0.0, 0.0);
        self.pending_anchor = anchor;
        self.rich = None;
        self.rich_key = (0, 0, 0, false);
        self.wrap_idx = None;
        self.csv_cols = None;
        self.images.clear();
        self.pdf = Pdf::default();
        self.hl = None;
        self.hover_link = None;
        self.gen
    }

    pub fn finish(&mut self, gen: u64, result: Result<Content, String>) {
        if gen != self.gen {
            return;
        }
        self.state = match result {
            Ok(c) => State::Ready(c),
            Err(e) => State::Error(e),
        };
        if let State::Ready(Content::Text(d)) = &self.state {
            let first = if d.line_count() > 0 { d.line(0) } else { "" };
            let lang = crate::syntax::detect(self.path.as_deref(), first);
            if lang != crate::syntax::Lang::Plain && d.text.len() <= crate::syntax::MAX_HIGHLIGHT_SIZE {
                self.hl = Some(crate::syntax::Highlighter::new(lang));
            }
        }
    }

    /// Language of a highlighted text document.
    pub fn language(&self) -> Option<crate::syntax::Lang> {
        self.hl.as_ref().map(|h| h.lang)
    }

    /// Applies a result from the media worker. Returns true to redraw.
    pub fn on_media(&mut self, done: Done) -> bool {
        match done {
            Done::PdfOpened { gen, sizes } if gen == self.gen => {
                self.pdf.sizes = sizes;
                self.state = State::Ready(Content::Pdf);
                true
            }
            Done::PdfFailed { gen, message } if gen == self.gen => {
                self.state = State::Error(tf("This PDF cannot be opened: {}", &[&message]));
                true
            }
            Done::Page { gen, page, width, mut bmp } if gen == self.gen => {
                if self.pdf.dark {
                    darken(&mut bmp.rgba);
                }
                if let Some(p) = pixmap(bmp) {
                    self.pdf.pages.insert(page, (width, p));
                    self.evict_pages();
                }
                true
            }
            Done::Image { key, bmp } => {
                if let Some(img) = self.images.get_mut(&key) {
                    let pix = bmp.and_then(pixmap);
                    img.state = match &pix {
                        Some(p) => ImgState::Ready(p.width(), p.height()),
                        None => ImgState::Failed,
                    };
                    img.pix = pix;
                    self.img_gen += 1;
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn evict_pages(&mut self) {
        let mut total: usize = self.pdf.pages.values().map(|(_, p)| p.data().len()).sum();
        if total <= PDF_CACHE_BYTES {
            return;
        }
        let cur = self.pdf.current as i64;
        let mut keys: Vec<u32> = self.pdf.pages.keys().copied().collect();
        keys.sort_by_key(|k| std::cmp::Reverse((*k as i64 - cur).abs()));
        for k in keys {
            if total <= PDF_CACHE_BYTES || (k as i64 - cur).abs() <= 2 {
                break;
            }
            if let Some((_, p)) = self.pdf.pages.remove(&k) {
                total -= p.data().len();
            }
        }
    }

    // ------------------------------------------------------------ zoom and scroll

    pub fn set_zoom(&mut self, z: f32, anchor: Option<(f32, f32)>) {
        let z = z.clamp(0.1, 8.0);
        if (z - self.zoom).abs() < 1e-4 {
            self.fit = false;
            return;
        }
        let k = z / self.zoom;
        let (ax, ay) = anchor.unwrap_or((self.view.w / 2.0, self.view.h / 2.0));
        // Keep the point under the anchor in place (content scales ~linearly).
        self.scroll.0 = (self.scroll.0 + ax) * k - ax;
        self.scroll.1 = (self.scroll.1 + ay) * k - ay;
        self.zoom = z;
        self.fit = false;
    }

    pub fn zoom_step(&mut self, dir: i32, anchor: Option<(f32, f32)>) {
        let z = self.zoom;
        let next = if dir > 0 { ZOOMS.iter().copied().find(|&s| s > z * 1.01).unwrap_or(8.0) } else { ZOOMS.iter().rev().copied().find(|&s| s < z * 0.99).unwrap_or(0.1) };
        self.set_zoom(next, anchor);
    }

    pub fn zoom_reset(&mut self) {
        self.set_zoom(1.0, None);
    }

    pub fn fit_width(&mut self) {
        self.fit = true;
    }

    pub fn scroll_by(&mut self, dx: f32, dy: f32) {
        self.scroll.0 += dx;
        self.scroll.1 += dy;
        self.clamp();
    }

    pub fn scroll_to(&mut self, y: f32) {
        self.scroll.1 = y;
        self.clamp();
    }

    pub fn page_height(&self) -> f32 {
        (self.view.h * 0.9).max(20.0)
    }

    fn clamp(&mut self) {
        let max_x = (self.content.0 - self.view.w).max(0.0);
        let max_y = (self.content.1 - self.view.h).max(0.0);
        self.scroll.0 = self.scroll.0.clamp(0.0, max_x);
        self.scroll.1 = self.scroll.1.clamp(0.0, max_y);
    }

    /// Scroll bar track and thumb rectangles (window coordinates).
    fn bars(&self) -> (Bar, Bar) {
        let v = self.view;
        let sb = SCROLLBAR * (v.h / v.h.max(1.0));
        let vert = (self.content.1 > v.h + 0.5).then(|| {
            let track = Rect::new(v.right() - sb, v.y, sb, v.h);
            let th = (v.h / self.content.1 * v.h).clamp(24.0, v.h);
            let t = (self.scroll.1 / (self.content.1 - v.h).max(1.0)) * (v.h - th);
            (track, Rect::new(track.x + 3.0, v.y + t, sb - 6.0, th))
        });
        let horiz = (self.content.0 > v.w + 0.5).then(|| {
            let track = Rect::new(v.x, v.bottom() - sb, v.w - sb, sb);
            let tw = (v.w / self.content.0 * track.w).clamp(24.0, track.w);
            let t = (self.scroll.0 / (self.content.0 - v.w).max(1.0)) * (track.w - tw);
            (track, Rect::new(v.x + t, track.y + 3.0, tw, sb - 6.0))
        });
        (vert, horiz)
    }

    pub fn scrollbar_at(&self, x: f32, y: f32) -> Option<Axis> {
        let (v, h) = self.bars();
        if v.is_some_and(|(t, _)| t.contains(x, y)) {
            return Some(Axis::Vertical);
        }
        if h.is_some_and(|(t, _)| t.contains(x, y)) {
            return Some(Axis::Horizontal);
        }
        None
    }

    /// Moves the scroll position so the thumb follows the pointer.
    pub fn drag_scrollbar(&mut self, axis: Axis, x: f32, y: f32) {
        let (v, h) = self.bars();
        match axis {
            Axis::Vertical => {
                if let Some((t, th)) = v {
                    let f = ((y - t.y - th.h / 2.0) / (t.h - th.h).max(1.0)).clamp(0.0, 1.0);
                    self.scroll.1 = f * (self.content.1 - self.view.h).max(0.0);
                }
            }
            Axis::Horizontal => {
                if let Some((t, tw)) = h {
                    let f = ((x - t.x - tw.w / 2.0) / (t.w - tw.w).max(1.0)).clamp(0.0, 1.0);
                    self.scroll.0 = f * (self.content.0 - self.view.w).max(0.0);
                }
            }
        }
    }

    pub fn in_view(&self, x: f32, y: f32) -> bool {
        self.view.contains(x, y)
    }

    // ------------------------------------------------------------ links

    fn rich_doc(&self) -> Option<&RichDoc> {
        match &self.state {
            State::Ready(Content::Rich(d)) => Some(d),
            _ => None,
        }
    }

    /// Link target under a window point.
    pub fn link_at(&self, x: f32, y: f32) -> Option<String> {
        if !self.view.contains(x, y) {
            return None;
        }
        let (doc, l) = (self.rich_doc()?, self.rich.as_ref()?);
        let n = l.link_at(x - self.view.x + self.scroll.0, y - self.view.y + self.scroll.1)?;
        doc.links.get(n as usize - 1).cloned()
    }

    pub fn follow(&mut self, href: &str) -> LinkAction {
        let href = href.trim();
        if let Some(frag) = href.strip_prefix('#') {
            self.jump_to(&percent_decode(frag));
            return LinkAction::Scrolled;
        }
        let lower = href.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") {
            return LinkAction::OpenUrl(href.to_string());
        }
        if lower.contains("://") && !lower.starts_with("file:") || lower.starts_with("javascript:") || lower.starts_with("data:") {
            return LinkAction::Unsupported(href.to_string());
        }
        let (target, frag) = match href.split_once('#') {
            Some((t, f)) => (t, Some(percent_decode(f))),
            None => (href, None),
        };
        let base = self.rich_doc().map(|d| d.base.clone()).unwrap_or_default();
        let path = PathBuf::from(image_key(&base, target));
        let supported = path.extension().is_some_and(|e| {
            let e = e.to_string_lossy().to_ascii_lowercase();
            matches!(e.as_str(), "pdf" | "md" | "markdown" | "html" | "htm" | "xhtml" | "txt" | "csv" | "tsv" | "log")
        });
        if supported && path.is_file() {
            LinkAction::OpenFile(path, frag)
        } else {
            LinkAction::Unsupported(href.to_string())
        }
    }

    fn jump_to(&mut self, anchor: &str) {
        if let Some(y) = self.rich.as_ref().and_then(|l| l.anchors.get(anchor).copied()) {
            self.scroll_to(y - 8.0);
        } else {
            self.pending_anchor = Some(anchor.to_string());
        }
    }

    // ------------------------------------------------------------ status

    pub fn status(&self) -> Vec<String> {
        let mut v = Vec::new();
        match &self.state {
            State::Ready(Content::Text(d)) => {
                if let Some(l) = self.language() {
                    v.push(tr(l.name()));
                }
                v.push(tf("{} lines", &[&d.line_count().to_string()]));
                v.push(d.eol.to_string());
                v.push(d.encoding.clone());
            }
            State::Ready(Content::Csv(d)) => {
                v.push(tf("{} rows × {} columns", &[&d.rows().saturating_sub(1).to_string(), &d.columns.to_string()]));
                let delim = match d.delim {
                    ';' => tr("Semicolon"),
                    '\t' => tr("Tab"),
                    '|' => "|".to_string(),
                    _ => tr("Comma"),
                };
                v.push(delim);
                v.push(d.encoding.clone());
            }
            State::Ready(Content::Rich(d)) => {
                if !d.encoding.is_empty() {
                    v.push(d.encoding.clone());
                }
            }
            State::Ready(Content::Pdf) => {
                v.push(tf("Page {} of {}", &[&(self.pdf.current + 1).to_string(), &self.pdf.sizes.len().to_string()]));
            }
            _ => {}
        }
        if matches!(self.state, State::Ready(_)) {
            v.push(self.kind.name());
            v.push(format!("{}%", (self.zoom * 100.0).round()));
        }
        v
    }

    /// Visible text for screen readers.
    pub fn accessible_text(&self) -> String {
        match &self.state {
            State::Ready(Content::Text(d)) => {
                let first = (self.scroll.1 / 20.0) as usize;
                (first..(first + 60).min(d.line_count())).map(|i| d.line(i)).collect::<Vec<_>>().join("\n")
            }
            State::Ready(Content::Rich(d)) => crate::rich::plain_text(&d.blocks).chars().take(100_000).collect(),
            State::Ready(Content::Csv(d)) => (0..d.rows().min(50)).map(|i| d.record(i).join(", ")).collect::<Vec<_>>().join("\n"),
            State::Ready(Content::Pdf) => tf("Page {} of {}", &[&(self.pdf.current + 1).to_string(), &self.pdf.sizes.len().to_string()]),
            State::Error(e) => e.clone(),
            _ => String::new(),
        }
    }

    pub fn title(&self) -> Option<String> {
        self.rich_doc().and_then(|d| d.title.clone()).filter(|t| !t.is_empty())
    }

    // ------------------------------------------------------------ drawing

    /// Draws the document into `area`. `s` is the DPI scale.
    pub fn draw(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32, media: &Media) {
        self.view = area;
        match &self.state {
            State::Empty => {
                g.fill_rect(area, th.bg);
                let px = 15.0 * s;
                let lines = [tr("Drop a document here, or press Ctrl+O to open one."), tr("PDF · Markdown · HTML · Text · CSV")];
                for (i, l) in lines.iter().enumerate() {
                    let r = Rect::new(area.x, area.center_y() - px * 1.6 + i as f32 * px * 2.0, area.w, px * 2.0);
                    g.text_in(l, r, if i == 0 { px } else { px * 0.9 }, FontId::Regular, if i == 0 { th.text } else { th.dim }, crate::ui::gfx::Align::Center);
                }
                self.content = (0.0, 0.0);
            }
            State::Loading => {
                g.fill_rect(area, th.bg);
                g.text_in(&tr("Opening…"), area, 15.0 * s, FontId::Regular, th.dim, crate::ui::gfx::Align::Center);
                self.content = (0.0, 0.0);
            }
            State::Error(e) => {
                g.fill_rect(area, th.bg);
                let e = e.clone();
                let px = 14.0 * s;
                let w = (area.w - 48.0 * s).max(100.0);
                let mut y = area.center_y() - px * 2.0;
                for line in e.lines() {
                    let t = fit_text(g, line, w, px, FontId::Regular);
                    g.text_in(&t, Rect::new(area.x, y, area.w, px * 1.8), px, FontId::Regular, th.text, crate::ui::gfx::Align::Center);
                    y += px * 1.8;
                }
                self.content = (0.0, 0.0);
            }
            State::Ready(_) => {
                g.set_clip(Some(area));
                match self.kind {
                    Kind::Text => self.draw_text(g, area, th, s),
                    Kind::Csv => self.draw_csv(g, area, th, s),
                    Kind::Markdown | Kind::Html => self.draw_rich(g, area, th, s, media),
                    Kind::Pdf => self.draw_pdf(g, area, th, s, media),
                }
                g.set_clip(None);
                self.draw_bars(g, th);
            }
        }
    }


    fn draw_bars(&self, g: &mut Gfx, th: &Theme) {
        let (v, h) = self.bars();
        for (_, thumb) in [v, h].into_iter().flatten() {
            g.fill_rrect(thumb, 3.0, th.scrollbar);
        }
    }

    fn draw_text(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        let State::Ready(Content::Text(doc)) = &self.state else { return };
        let px = (14.0 * s * self.zoom).max(4.0);
        let cw = g.advance('0', px, FontId::Mono).max(1.0);
        let lh = (px * 1.45).round().max(2.0);
        let pad = (8.0 * s).round();
        let digits = doc.line_count().to_string().len();
        let gutter = digits as f32 * cw + 2.0 * pad;
        let text_w = area.w - gutter - pad - SCROLLBAR;
        let wrap = self.wrap && doc.text.len() <= WRAP_LIMIT;
        let ncells = (text_w / cw).floor().max(8.0) as usize;
        if wrap && self.wrap_idx.as_ref().is_none_or(|w| w.cells != ncells) {
            let frac = if self.content.1 > 0.0 { Some(self.scroll.1 / self.content.1) } else { None };
            self.wrap_idx = Some(WrapIndex::build(doc, ncells));
            if self.restore.is_none() {
                self.restore = frac;
            }
        }
        if !wrap {
            self.wrap_idx = None;
        }
        let rows = self.wrap_idx.as_ref().map_or(doc.line_count(), |w| w.rows);
        let content_w = if wrap { area.w } else { gutter + doc.longest as f32 * cw + 2.0 * pad + SCROLLBAR };
        self.content = (content_w, rows as f32 * lh + 2.0 * pad);
        settle(&mut self.restore, &mut self.scroll, self.content, self.view);
        g.fill_rect(area, th.bg);
        let (asc, desc) = g.metrics(px, FontId::Mono);
        let base_off = ((lh + asc + desc) / 2.0).round();
        let first = ((self.scroll.1 - pad) / lh).floor().max(0.0) as usize;
        let x0 = area.x + gutter - if wrap { 0.0 } else { self.scroll.0 };
        let mut cached: Option<(usize, Vec<usize>)> = None;
        let mut spans: Vec<crate::syntax::Span> = Vec::new();
        let mut spans_line: Option<usize> = None;
        let mut row = first;
        loop {
            let y = area.y + pad + row as f32 * lh - self.scroll.1;
            if y > area.bottom() || row >= rows {
                break;
            }
            let (line, sub) = match &self.wrap_idx {
                Some(w) => w.locate(row),
                None => (row, 0),
            };
            let text = doc.line(line);
            let (a, b) = match &self.wrap_idx {
                Some(w) => {
                    if cached.as_ref().is_none_or(|(l, _)| *l != line) {
                        cached = Some((line, cells::wrap(text, w.cells, TAB)));
                    }
                    let starts = &cached.as_ref().expect("wrap").1;
                    let a = starts.get(sub).copied().unwrap_or(0);
                    let b = starts.get(sub + 1).copied().unwrap_or(usize::MAX);
                    (a, b)
                }
                None => (0, usize::MAX),
            };
            let baseline = y + base_off;
            if sub == 0 {
                let n = (line + 1).to_string();
                let nw = g.measure(&n, px * 0.9, FontId::Mono);
                g.text(&n, area.x + gutter - pad - nw, baseline, px * 0.9, FontId::Mono, th.gutter_text);
            }
            if let Some(hl) = &mut self.hl {
                if spans_line != Some(line) {
                    hl.spans(&|i| doc.line(i), line, &mut spans);
                    spans_line = Some(line);
                }
            }
            let dark = th.dark;
            let color_at = |byte: usize| -> Color {
                spans.iter().find(|sp| byte >= sp.start && byte < sp.end).and_then(|sp| crate::syntax::color(sp.tok, dark)).unwrap_or(th.text)
            };
            draw_cells(g, text, a, b, x0, baseline, px, cw, area.x + gutter - cw, area.right(), &color_at);
            row += 1;
        }
        // Gutter background over horizontally scrolled text.
        if !wrap && self.scroll.0 > 0.0 {
            g.fill_rect(Rect::new(area.x, area.y, gutter - pad / 2.0, area.h), th.bg);
            let mut row = first;
            loop {
                let y = area.y + pad + row as f32 * lh - self.scroll.1;
                if y > area.bottom() || row >= rows {
                    break;
                }
                let n = (row + 1).to_string();
                let nw = g.measure(&n, px * 0.9, FontId::Mono);
                g.text(&n, area.x + gutter - pad - nw, y + base_off, px * 0.9, FontId::Mono, th.gutter_text);
                row += 1;
            }
        }
    }

    fn draw_csv(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        let State::Ready(Content::Csv(doc)) = &self.state else { return };
        let px = (13.0 * s * self.zoom).max(4.0);
        let rh = (px * 1.9).round().max(4.0);
        let pad = (px * 0.6).round();
        if self.csv_cols.is_none() {
            self.csv_cols = Some((px, column_widths(g, doc, px, pad)));
        }
        let (px0, ref base) = *self.csv_cols.as_ref().expect("widths");
        let k = px / px0;
        let widths: Vec<f32> = base.iter().map(|w| (w * k).round()).collect();
        let rows = doc.rows();
        let body = rows.saturating_sub(1);
        let digits = body.max(1).to_string().len();
        let num_w = digits as f32 * g.advance('0', px, FontId::Regular) + 2.0 * pad;
        let total_w: f32 = num_w + widths.iter().sum::<f32>() + SCROLLBAR;
        self.content = (total_w, (body as f32 + 1.0) * rh + SCROLLBAR);
        settle(&mut self.restore, &mut self.scroll, self.content, self.view);
        g.fill_rect(area, th.bg);
        let (asc, desc) = g.metrics(px, FontId::Regular);
        let base_off = ((rh + asc + desc) / 2.0).round();
        let x0 = area.x + num_w - self.scroll.0;
        let first = (self.scroll.1 / rh).floor().max(0.0) as usize;
        let visible = (area.h / rh).ceil() as usize + 1;
        let line = |g: &mut Gfx, r: Rect| g.fill_rect(r, th.border);
        for i in first..(first + visible).min(body) {
            let y = (area.y + rh + i as f32 * rh - self.scroll.1).round();
            if i % 2 == 1 {
                g.fill_rect(Rect::new(area.x, y, area.w, rh), th.stripe);
            }
            let fields = doc.record(i + 1);
            let mut x = x0;
            for (c, w) in widths.iter().enumerate() {
                if x > area.right() {
                    break;
                }
                if x + w >= area.x + num_w {
                    if let Some(f) = fields.get(c) {
                        let f = f.replace("\r\n", "\n").replace('\n', " ↵ ");
                        let t = fit_text(g, &f, w - 2.0 * pad, px, FontId::Regular);
                        let tw = g.measure(&t, px, FontId::Regular);
                        let tx = if csv::is_number(&f) { x + w - pad - tw } else { x + pad };
                        g.text(&t, tx.round(), y + base_off, px, FontId::Regular, th.text);
                    }
                }
                x += w;
            }
            line(g, Rect::new(area.x, (y + rh - 1.0).round(), area.w, 1.0));
        }
        // Column lines.
        let mut x = x0;
        for w in &widths {
            x += w;
            if x > area.x + num_w && x < area.right() {
                line(g, Rect::new(x.round() - 1.0, area.y, 1.0, area.h));
            }
        }
        // Sticky header row.
        g.fill_rect(Rect::new(area.x, area.y, area.w, rh), th.header);
        if let Some(&r0) = doc.records.first() {
            let (header, _) = csv::parse_record(&doc.text, r0, doc.delim);
            let mut x = x0;
            for (c, w) in widths.iter().enumerate() {
                if x > area.right() {
                    break;
                }
                if x + w >= area.x + num_w {
                    let h = header.get(c).map(String::as_str).unwrap_or("");
                    let t = fit_text(g, h, w - 2.0 * pad, px, FontId::Bold);
                    g.text(&t, (x + pad).round(), area.y + base_off, px, FontId::Bold, th.text);
                    line(g, Rect::new((x + w).round() - 1.0, area.y, 1.0, rh));
                }
                x += w;
            }
        }
        line(g, Rect::new(area.x, area.y + rh - 1.0, area.w, 1.0));
        // Sticky row numbers.
        g.fill_rect(Rect::new(area.x, area.y + rh, num_w, area.h - rh), th.header);
        for i in first..(first + visible).min(body) {
            let y = (area.y + rh + i as f32 * rh - self.scroll.1).round();
            if y + rh < area.y + rh {
                continue;
            }
            let n = (i + 1).to_string();
            let nw = g.measure(&n, px * 0.9, FontId::Regular);
            g.text(&n, area.x + num_w - pad - nw, y + base_off, px * 0.9, FontId::Regular, th.gutter_text);
        }
        g.fill_rect(Rect::new(area.x, area.y, num_w, rh), th.header);
        line(g, Rect::new(area.x + num_w - 1.0, area.y, 1.0, area.h));
    }

    fn draw_rich(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32, media: &Media) {
        let em = ((layout::BASE_PX * s * self.zoom) * 4.0).round() / 4.0;
        let width = area.w - SCROLLBAR;
        let html = self.kind == Kind::Html;
        // Markdown follows the theme; HTML too unless "Dark Pages" is off.
        let dark = th.dark && (!html || self.dark_pages);
        let key = (width.round() as i32, (em * 100.0) as i32, self.img_gen, dark);
        if self.rich.is_none() || self.rich_key != key {
            let State::Ready(Content::Rich(doc)) = &self.state else { return };
            let mut pal = if dark { Palette::dark() } else { Palette::paper() };
            pal.adapt = dark;
            if let Some(bg) = doc.page_bg {
                pal.bg = pal.bgc(bg);
                // Keep default text readable on a dark page background.
                let lum = 0.299 * bg.r as f32 + 0.587 * bg.g as f32 + 0.114 * bg.b as f32;
                if lum < 110.0 && doc.page_color.is_none() {
                    let d = Palette::dark();
                    pal.text = d.text;
                    pal.link = d.link;
                    pal.muted = d.muted;
                }
            }
            if self.kind == Kind::Markdown && dark {
                pal.bg = th.bg;
            }
            let base = doc.base.clone();
            let images = &self.images;
            let resolve = |src: &str| {
                let k = image_key(&base, src);
                let state = images.get(&k).map_or(ImgState::Pending, |i| i.state);
                (k, state)
            };
            let old_h = self.content.1;
            let frac = if old_h > 0.0 && self.rich.is_some() { Some(self.scroll.1 / old_h) } else { None };
            let l = layout::layout(doc, g, width, em, pal, &resolve);
            self.rich = Some(l);
            self.rich_key = key;
            self.rich_pal = Some(pal);
            if self.restore.is_none() {
                self.restore = frac;
            }
            self.request_images(media);
        }
        let Some(l) = &self.rich else { return };
        self.content = (l.width, l.height);
        if let Some(y) = self.pending_anchor.take().and_then(|a| l.anchors.get(&a).copied()) {
            self.restore = None;
            self.scroll.1 = y - 8.0;
        }
        settle(&mut self.restore, &mut self.scroll, self.content, self.view);
        let Some(l) = &self.rich else { return };
        let pal = self.rich_pal.unwrap_or(Palette::paper());
        g.fill_rect(area, pal.bg);
        let (ox, oy) = (area.x - self.scroll.0, area.y - self.scroll.1);
        let (top, bottom) = (self.scroll.1, self.scroll.1 + area.h);
        for (item, &(a, b)) in l.items.iter().zip(&l.spans) {
            if b < top || a > bottom {
                continue;
            }
            match item {
                Item::Text { x, y, s, px, font, color } => {
                    if x + ox > area.right() {
                        continue;
                    }
                    g.text(s, (x + ox).round(), y + oy, *px, *font, *color);
                }
                Item::Fill { r, color } => g.fill_rect(Rect::new(r.x + ox, r.y + oy, r.w, r.h), *color),
                Item::Image { r, key } => {
                    let dest = Rect::new(r.x + ox, r.y + oy, r.w, r.h);
                    match self.images.get(key).and_then(|i| i.pix.as_ref()) {
                        Some(p) => g.draw_image(p, dest),
                        None => g.stroke_rrect(dest, 2.0, 1.0, pal.border),
                    }
                }
            }
        }
    }

    fn request_images(&mut self, media: &Media) {
        let Some(l) = &self.rich else { return };
        let keys: Vec<String> = l.images.iter().filter(|k| !self.images.contains_key(*k)).cloned().collect();
        for k in keys {
            let bytes = if let Some(data) = k.strip_prefix("data:") {
                // data:[mime][;base64],payload
                data.split_once(',').and_then(|(meta, payload)| if meta.ends_with(";base64") { base64(payload) } else { Some(percent_decode(payload).into_bytes()) })
            } else if k.contains("://") || k.starts_with("//") {
                None // remote images are never fetched
            } else {
                std::fs::metadata(&k).ok().filter(|m| m.is_file() && m.len() < 64 << 20).and_then(|_| std::fs::read(&k).ok())
            };
            match bytes {
                Some(b) if !b.is_empty() => {
                    self.images.insert(k.clone(), Img { state: ImgState::Pending, pix: None });
                    media.send(Job::Image { key: k, bytes: b });
                }
                _ => {
                    self.images.insert(k, Img { state: ImgState::Failed, pix: None });
                    self.img_gen += 1;
                }
            }
        }
    }

    fn draw_pdf(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32, media: &Media) {
        let dark = th.dark && self.dark_pages;
        if dark != self.pdf.dark {
            // Re-render: cached pages were made for the other mode.
            self.pdf.dark = dark;
            self.pdf.pages.clear();
            self.pdf.requested.clear();
        }
        let sizes = &self.pdf.sizes;
        if sizes.is_empty() {
            g.fill_rect(area, th.canvas);
            return;
        }
        let margin = (16.0 * s).round();
        let gap = (12.0 * s).round();
        let max_w = sizes.iter().map(|p| p.0).fold(1.0f32, f32::max);
        if self.fit {
            let z = ((area.w - SCROLLBAR - 2.0 * margin) / (max_w * s)).clamp(0.1, 8.0);
            if (z - self.zoom).abs() > 1e-3 {
                let f = if self.content.1 > 0.0 { self.scroll.1 / self.content.1 } else { 0.0 };
                self.zoom = z;
                if self.restore.is_none() && self.content.1 > 0.0 {
                    self.restore = Some(f);
                }
            }
        }
        let k = s * self.zoom;
        let content_w = max_w * k + 2.0 * margin;
        let mut rects = Vec::with_capacity(sizes.len());
        let mut y = margin;
        for &(w, h) in sizes {
            let (pw, ph) = ((w * k).round(), (h * k).round());
            let x = ((content_w.max(area.w - SCROLLBAR) - pw) / 2.0).round();
            rects.push(Rect::new(x, y, pw, ph));
            y += ph + gap;
        }
        self.content = (content_w, y - gap + margin);
        settle(&mut self.restore, &mut self.scroll, self.content, self.view);
        g.fill_rect(area, th.canvas);
        let (ox, oy) = (area.x - self.scroll.0, area.y - self.scroll.1);
        let (top, bottom) = (self.scroll.1, self.scroll.1 + area.h);
        let first = rects.partition_point(|r| r.bottom() < top);
        let mut wanted = Vec::new();
        let mid = top + area.h / 2.0;
        let mut current = first;
        for (i, r) in rects.iter().enumerate().skip(first) {
            if r.y > bottom {
                break;
            }
            if r.y <= mid {
                current = i;
            }
            let dest = Rect::new(r.x + ox, r.y + oy, r.w, r.h);
            g.fill_rect(Rect::new(dest.x + 2.0, dest.y + 2.0, dest.w, dest.h), Color::rgba(0x000000, 0x30));
            g.fill_rect(dest, if dark { Color::rgb(0x1E1E1E) } else { Color::rgb(0xFFFFFF) });
            let target = (r.w as u32).clamp(16, MAX_EDGE);
            match self.pdf.pages.get(&(i as u32)) {
                Some((w, p)) => {
                    g.draw_image(p, dest);
                    if *w != target {
                        wanted.push((i as u32, target));
                    }
                }
                None => wanted.push((i as u32, target)),
            }
        }
        // Prefetch the next and previous page.
        let last_visible = rects.partition_point(|r| r.y <= bottom).saturating_sub(1);
        for i in [last_visible + 1, first.wrapping_sub(1)] {
            if let Some(r) = rects.get(i) {
                let target = (r.w as u32).clamp(16, MAX_EDGE);
                if self.pdf.pages.get(&(i as u32)).is_none_or(|(w, _)| *w != target) {
                    wanted.push((i as u32, target));
                }
            }
        }
        self.pdf.current = current;
        self.pdf.rects = rects;
        if wanted != self.pdf.requested {
            media.request_pages(self.gen, &wanted);
            self.pdf.requested = wanted;
        }
    }

    /// Scrolls a PDF to the start of the previous/next page.
    pub fn pdf_page_step(&mut self, dir: i32) -> bool {
        if self.kind != Kind::Pdf || self.pdf.rects.is_empty() {
            return false;
        }
        let n = self.pdf.rects.len() as i64;
        let i = (self.pdf.current as i64 + dir as i64).clamp(0, n - 1) as usize;
        let y = self.pdf.rects[i].y - (12.0f32).min(self.pdf.rects[i].y);
        self.scroll_to(y);
        true
    }
}

/// Applies a pending scroll restore (a fraction of the height) and clamps.
fn settle(restore: &mut Option<f32>, scroll: &mut (f32, f32), content: (f32, f32), view: Rect) {
    if let Some(f) = restore.take() {
        scroll.1 = f * content.1;
    }
    scroll.0 = scroll.0.clamp(0.0, (content.0 - view.w).max(0.0));
    scroll.1 = scroll.1.clamp(0.0, (content.1 - view.h).max(0.0));
}

/// Draws chars `a..b` of a monospace line at cell positions starting at `x0`.
#[allow(clippy::too_many_arguments)]
fn draw_cells(g: &mut Gfx, line: &str, a: usize, b: usize, x0: f32, baseline: f32, px: f32, cw: f32, min_x: f32, max_x: f32, color_at: &dyn Fn(usize) -> Color) {
    let mut col = 0usize;
    let mut base_col = None;
    let mut run = String::new();
    let mut run_x = 0.0;
    let mut run_color = color_at(0);
    let flush = |g: &mut Gfx, run: &mut String, x: f32, color: Color| {
        if !run.is_empty() {
            g.text(run, x.round(), baseline, px, FontId::Mono, color);
            run.clear();
        }
    };
    for (i, (byte, ch)) in line.char_indices().enumerate() {
        if i >= b {
            break;
        }
        let next = cells::advance(col, ch, TAB);
        if i < a {
            col = next;
            continue;
        }
        let base = *base_col.get_or_insert(col);
        let x = x0 + (col - base) as f32 * cw;
        let cell_w = (next - col).max(1) as f32 * cw;
        col = next;
        if x + cell_w < min_x {
            continue;
        }
        if x > max_x {
            break;
        }
        let color = color_at(byte);
        if ch.is_ascii_graphic() {
            if !run.is_empty() && color != run_color {
                flush(g, &mut run, run_x, run_color);
            }
            if run.is_empty() {
                run_x = x;
                run_color = color;
            }
            run.push(ch);
            continue;
        }
        flush(g, &mut run, run_x, run_color);
        if ch == ' ' || ch == '\t' || cells::cells(ch) == 0 && ch as u32 >= 0x20 {
            continue;
        }
        let shown = if (ch as u32) < 0x20 || ch as u32 == 0x7F { char::from_u32(0x2400 + (ch as u32 & 0x1F)).unwrap_or('?') } else { ch };
        let w = g.advance(shown, px, FontId::Mono);
        g.text(&shown.to_string(), (x + ((cell_w - w) / 2.0).max(0.0)).round(), baseline, px, FontId::Mono, color);
    }
    flush(g, &mut run, run_x, run_color);
}

/// Inverts the lightness of a rendered PDF page in place (dark mode),
/// keeping hues: white paper becomes #1E1E1E and black text white. Pages
/// that are already mostly dark (full-page photos, dark slides) are left
/// as they are.
fn darken(rgba: &mut [u8]) {
    let px = rgba.as_chunks::<4>().0;
    let step = (px.len() / 4096).max(1);
    let (mut sum, mut n) = (0.0f64, 0u32);
    for p in px.iter().step_by(step) {
        sum += layout::luminance(Color { r: p[0], g: p[1], b: p[2], a: 255 }) as f64;
        n += 1;
    }
    if n == 0 || sum / (n as f64) < 150.0 {
        return;
    }
    for p in rgba.as_chunks_mut::<4>().0 {
        let c = layout::invert_lightness(Color { r: p[0], g: p[1], b: p[2], a: p[3] });
        p[0] = c.r;
        p[1] = c.g;
        p[2] = c.b;
    }
}

/// Column widths from a sample of rows, at font size `px`.
fn column_widths(g: &mut Gfx, doc: &CsvDoc, px: f32, pad: f32) -> Vec<f32> {
    let mut w = vec![px * 3.0; doc.columns];
    // The first rows plus rows spread over the whole file (values often
    // grow towards the end, e.g. ids).
    let n = doc.records.len();
    let step = (n / 500).max(1);
    let sample = (0..n.min(300)).chain((300..n).step_by(step)).chain(n.saturating_sub(50)..n);
    for i in sample {
        let start = doc.records[i];
        let (fields, _) = csv::parse_record(&doc.text, start, doc.delim);
        for (c, f) in fields.iter().enumerate().take(doc.columns) {
            let first = f.lines().next().unwrap_or("");
            let first: String = first.chars().take(80).collect();
            let font = if i == 0 { FontId::Bold } else { FontId::Regular };
            let fw = g.measure(&first, px, font) + 2.0 * pad + 2.0;
            w[c] = w[c].max(fw);
        }
    }
    w.iter().map(|x| x.min(px * 28.0).ceil()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_keys() {
        let base = Path::new("C:\\docs");
        assert_eq!(image_key(base, "img/a%20b.png"), "C:\\docs\\img/a b.png");
        assert_eq!(image_key(base, "https://x.org/a.png"), "https://x.org/a.png");
        assert!(image_key(base, "data:image/png;base64,AAAA").starts_with("data:"));
        assert_eq!(image_key(base, "file:///C:/x/y.png"), "C:/x/y.png");
    }

    #[test]
    fn base64_and_percent() {
        assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(percent_decode("a%20b%C3%A9"), "a bé");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn dark_pages_invert_light_pages_only() {
        // A text page: mostly white paper with some black ink.
        let mut light = [255u8, 255, 255, 255].repeat(9);
        light.extend([0, 0, 0, 255]);
        let mut light = light.repeat(20);
        darken(&mut light);
        assert_eq!(&light[..4], &[30, 30, 30, 255]);
        assert_eq!(&light[36..40], &[255, 255, 255, 255]);
        let mut dark = [20, 20, 20, 255].repeat(100);
        darken(&mut dark);
        assert_eq!(&dark[..4], &[20, 20, 20, 255]);
    }

    #[test]
    fn zoom_steps() {
        let mut v = Viewer::new(true);
        v.zoom_step(1, None);
        assert_eq!(v.zoom, 1.1);
        v.zoom_step(-1, None);
        v.zoom_step(-1, None);
        assert_eq!(v.zoom, 0.9);
        v.set_zoom(100.0, None);
        assert_eq!(v.zoom, 8.0);
    }

    #[test]
    fn links_are_safe() {
        let mut v = Viewer::new(true);
        assert!(matches!(v.follow("https://example.com"), LinkAction::OpenUrl(_)));
        assert!(matches!(v.follow("javascript:alert(1)"), LinkAction::Unsupported(_)));
        assert!(matches!(v.follow("C:\\Windows\\notepad.exe"), LinkAction::Unsupported(_)));
        assert!(matches!(v.follow("#top"), LinkAction::Scrolled));
    }
}
