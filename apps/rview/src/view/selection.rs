//! Selecting what is shown and turning the selection into clipboard
//! contents. Each document type has its own idea of a place in the document
//! ([`Sel`]): a byte in text, a character in laid-out Markdown or HTML, a
//! cell in CSV, a glyph on a PDF page, or a rectangle of a PDF page.

use super::*;
use crate::clip::{self, Ev, Fmt, Payload, Tag};
use crate::doc::pdf::PageText;
use crate::media::Bitmap;
use crate::rich::layout::Pos;
use crate::rich::{Align, Style};

/// Largest selection copied with formatting, in bytes of text.
const MAX_RICH_TEXT: usize = 8 << 20;
/// Largest selection copied with formatting, in table cells.
const MAX_RICH_CELLS: usize = 200_000;
/// Fastest scrolling while dragging past the edge, in pixels per step.
const MAX_AUTOSCROLL: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sel {
    /// Byte offsets in the text.
    Text { anchor: usize, caret: usize },
    Rich { anchor: Pos, caret: Pos },
    /// (row, column), row 0 being the header. `rows` selects whole rows,
    /// `cols` whole columns.
    Csv { anchor: (usize, usize), caret: (usize, usize), rows: bool, cols: bool },
    /// (page, glyph boundary).
    Pdf { anchor: (u32, u32), caret: (u32, u32) },
    /// A rectangle of one PDF page, in page units.
    Region { page: u32, a: (f32, f32), b: (f32, f32) },
}

/// Pointer shape over the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hover {
    Arrow,
    Text,
    Cell,
    Cross,
}

/// Text view metrics from the last draw (for hit testing).
#[derive(Clone, Copy, Default)]
pub(super) struct TextGeom {
    pub cw: f32,
    pub lh: f32,
    pub pad: f32,
    pub gutter: f32,
}

/// CSV view metrics from the last draw (for hit testing).
#[derive(Clone, Default)]
pub(super) struct CsvGeom {
    pub rh: f32,
    pub num_w: f32,
    pub widths: Vec<f32>,
}

fn ordered<T: PartialOrd + Copy>(a: T, b: T) -> (T, T) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// The word (or run of spaces, or of punctuation) around byte `p`.
fn word_at(s: &str, p: usize) -> (usize, usize) {
    fn class(c: char) -> u8 {
        match c {
            '\n' | '\r' => 3,
            c if c.is_alphanumeric() || c == '_' => 0,
            c if c.is_whitespace() => 1,
            _ => 2,
        }
    }
    let (before, after) = (s[..p].chars().next_back(), s[p..].chars().next());
    let Some(pick) = (match (before, after) {
        (Some(b), Some(a)) if class(a) != 0 && class(b) == 0 => Some(b),
        (b, a) => a.or(b),
    }) else {
        return (p, p);
    };
    let k = class(pick);
    if k == 3 {
        return (p, p);
    }
    let start = s[..p].char_indices().rev().take_while(|(_, c)| class(*c) == k).last().map_or(p, |(i, _)| i);
    let end = s[p..].char_indices().find(|(_, c)| class(*c) != k).map_or(s.len(), |(i, _)| p + i);
    (start, end)
}

/// Extent of the selected part of a text row (`a..b` chars of `line`, whose
/// first byte is at `base`): (left, right) in pixels. A selected line break
/// shows as half a cell after the line's end.
#[allow(clippy::too_many_arguments)]
pub(super) fn sel_span(line: &str, base: usize, a: usize, b: usize, s: usize, e: usize, x0: f32, cw: f32, max_x: f32) -> Option<(f32, f32)> {
    let (mut col, mut base_col) = (0usize, None);
    let (mut from, mut to) = (None, x0);
    let mut end_x = x0;
    let mut whole = true;
    for (i, (byte, ch)) in line.char_indices().enumerate() {
        if i >= b {
            whole = false;
            break;
        }
        let next = cells::advance(col, ch, TAB);
        if i < a {
            col = next;
            continue;
        }
        let first = *base_col.get_or_insert(col);
        let x = x0 + (col - first) as f32 * cw;
        let w = (next - col) as f32 * cw;
        col = next;
        end_x = x + w;
        let at = base + byte;
        if at >= e {
            whole = false;
            break;
        }
        if at >= s {
            from.get_or_insert(x);
            to = x + w;
        }
        if x > max_x {
            whole = false;
            break;
        }
    }
    let line_end = base + line.len();
    if whole && s <= line_end && e > line_end {
        from.get_or_insert(end_x);
        to = end_x + cw / 2.0;
    }
    from.map(|f| (f, to))
}

/// The selected part of `page`, as (first glyph, past-the-end glyph).
fn page_range(sel: ((u32, u32), (u32, u32)), page: u32, count: usize) -> Option<(usize, usize)> {
    let (a, b) = sel;
    if page < a.0 || page > b.0 {
        return None;
    }
    let from = if page == a.0 { a.1 as usize } else { 0 };
    let to = if page == b.0 { b.1 as usize } else { count };
    (from.min(count) < to.min(count)).then(|| (from.min(count), to.min(count)))
}

/// Paints the selected glyphs of a PDF page, joined line by line. `k` is
/// pixels per page unit.
pub(super) fn paint_glyphs(g: &mut Gfx, page: &PageText, range: (usize, usize), dest: Rect, k: f32, color: Color) {
    let mut span: Option<Rect> = None;
    let flush = |g: &mut Gfx, r: Rect| g.fill_rect(Rect::new(dest.x + r.x * k, dest.y + r.y * k, r.w * k, r.h * k), color);
    for gl in &page.glyphs[range.0..range.1] {
        if gl.w <= 0.0 {
            continue;
        }
        let r = Rect::new(gl.x, gl.y, gl.w, gl.h);
        span = match span {
            Some(s) if (r.y - s.y).abs() < s.h * 0.5 && r.x >= s.x && r.x <= s.right() + s.h => {
                let top = s.y.min(r.y);
                let bottom = s.bottom().max(r.bottom());
                Some(Rect::new(s.x, top, r.right().max(s.right()) - s.x, bottom - top))
            }
            Some(s) => {
                flush(g, s);
                Some(r)
            }
            None => Some(r),
        };
    }
    if let Some(s) = span {
        flush(g, s);
    }
}

fn fmt_of(s: &Style, doc: &RichDoc) -> Fmt {
    let token = crate::syntax::tok_from(s.tok.wrapping_sub(1)).filter(|_| s.tok != 0);
    Fmt {
        bold: s.bold,
        italic: s.italic,
        underline: s.underline,
        strike: s.strike,
        mono: s.mono,
        size: s.size,
        color: s.color.or(token.and_then(|t| crate::syntax::color(t, false))),
        bg: s.bg.or(s.code.then(|| layout::Palette::paper().code_bg)),
        link: (s.link != 0).then(|| doc.links.get(s.link as usize - 1).cloned()).flatten(),
        valign: s.valign,
    }
}

/// An image address another program can open.
fn image_url(key: &str) -> String {
    if key.starts_with("data:") || key.contains("://") || key.starts_with("//") {
        return key.to_string();
    }
    let mut url = String::from("file:///");
    for b in key.replace('\\', "/").bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => url.push(b as char),
            _ => url.push_str(&format!("%{b:02X}")),
        }
    }
    url
}

/// The selected text of a laid-out document as clipboard events.
fn rich_events(l: &Layout, doc: &RichDoc, a: Pos, b: Pos) -> Vec<Ev> {
    let mut evs = Vec::new();
    let mut open: Vec<u32> = Vec::new();
    for i in a.item as usize..=(b.item as usize).min(l.items.len().saturating_sub(1)) {
        let m = &l.meta[i];
        let from = if i == a.item as usize { a.byte as usize } else { 0 };
        let to = if i == b.item as usize { b.byte as usize } else { l.item_len(i) };
        if from >= to {
            continue;
        }
        let mut path = Vec::new();
        let mut n = m.node;
        while n != 0 {
            path.push(n);
            n = l.nodes[n as usize].parent;
        }
        path.reverse();
        let common = open.iter().zip(&path).take_while(|(x, y)| x == y).count();
        evs.extend(std::iter::repeat_n(Ev::Exit, open.len() - common));
        open.truncate(common);
        let continues = common == path.len() && !path.is_empty();
        for &n in &path[common..] {
            evs.push(Ev::Enter(l.nodes[n as usize].tag.clone()));
            open.push(n);
        }
        if continues {
            evs.extend(std::iter::repeat_n(Ev::Br, m.breaks as usize));
            if m.space && m.breaks == 0 {
                // The space belongs to neither side: no bold, no link.
                clip::push_run(&mut evs, " ", Fmt { mono: m.style.mono, size: m.style.size, ..Fmt::default() });
            }
        }
        match &l.items[i] {
            Item::Text { s, .. } if m.marker => evs.push(Ev::Marker(s.clone())),
            Item::Text { s, .. } => clip::push_run(&mut evs, &s[from..to], fmt_of(&m.style, doc)),
            Item::Image { key, .. } => evs.push(Ev::Image { src: image_url(key), alt: m.alt.as_deref().unwrap_or("").to_string() }),
            Item::Fill { .. } => {}
        }
    }
    evs.extend(std::iter::repeat_n(Ev::Exit, open.len()));
    evs
}

/// The selected glyphs of PDF pages as clipboard events. Lines close
/// together become one paragraph; short lines keep their break.
fn pdf_events(pages: &[PageText], a: (u32, u32), b: (u32, u32)) -> Vec<Ev> {
    let para = || Ev::Enter(Tag::Para { align: Align::Left, pre: false, heading: 0, bg: None });
    let mut evs = Vec::new();
    let mut in_para = false;
    for p in a.0..=b.0 {
        let Some(page) = pages.get(p as usize) else { continue };
        let Some((from, to)) = page_range((a, b), p, page.glyphs.len()) else { continue };
        // Widest line reach, to tell a full line from a short one.
        let right = page.glyphs[from..to].iter().map(|g| g.x + g.w).fold(0.0f32, f32::max);
        let left = page.glyphs[from..to].iter().filter(|g| g.w > 0.0).map(|g| g.x).fold(f32::MAX, f32::min);
        if in_para {
            evs.push(Ev::Exit);
            in_para = false;
        }
        let mut run = String::new();
        let mut run_style = None;
        let flush = |run: &mut String, style: Option<u16>, evs: &mut Vec<Ev>| {
            if run.is_empty() {
                return;
            }
            let s = style.and_then(|i| page.styles.get(i as usize));
            let fmt = s.map_or(Fmt::default(), |s| Fmt { bold: s.bold, italic: s.italic, mono: s.mono, size: pdf_size(s.size), color: s.color, ..Fmt::default() });
            evs.push(Ev::Run(std::mem::take(run), fmt));
        };
        for i in from..to {
            let g = page.glyphs[i];
            if page.is_break(i) {
                flush(&mut run, run_style, &mut evs);
                let next = page.glyphs[i + 1..to].iter().find(|g| g.w > 0.0);
                let same_para = next.is_some_and(|n| n.y >= g.y && n.y - g.y < g.h * 1.5 && n.x < left + g.h * 3.0);
                let full = g.x > left + (right - left) * 0.8;
                match (same_para, full) {
                    (true, true) => evs.push(Ev::Soft),
                    (true, false) => evs.push(Ev::Br),
                    (false, _) => {
                        if in_para {
                            evs.push(Ev::Exit);
                            in_para = false;
                        }
                    }
                }
                continue;
            }
            if !in_para {
                evs.push(para());
                in_para = true;
            }
            if run_style != Some(g.style) {
                flush(&mut run, run_style, &mut evs);
                run_style = Some(g.style);
            }
            run.push_str(page.glyph_text(i));
        }
        flush(&mut run, run_style, &mut evs);
    }
    if in_para {
        evs.push(Ev::Exit);
    }
    evs
}

/// A PDF font size (page units, 96 per inch) relative to body text.
fn pdf_size(size: f32) -> f32 {
    let pt = (size * 0.75 * 2.0).round() / 2.0;
    pt / 11.0
}

/// Quotes a field for tab-separated text, the way spreadsheets expect it.
fn tsv_field(f: &str, out: &mut String) {
    if f.contains(['\t', '\n', '\r', '"']) {
        out.push('"');
        out.push_str(&f.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(f);
    }
}

fn formatted(evs: &[Ev]) -> (Option<String>, Option<String>) {
    (Some(clip::html_document(&clip::html(evs))), Some(clip::rtf(evs)))
}

impl Viewer {
    pub fn has_selection(&self) -> bool {
        match self.sel {
            None => false,
            Some(Sel::Text { anchor, caret }) => anchor != caret,
            Some(Sel::Rich { anchor, caret }) => anchor != caret,
            Some(Sel::Csv { .. }) => true,
            Some(Sel::Pdf { anchor, caret }) => anchor != caret,
            Some(Sel::Region { a, b, .. }) => (a.0 - b.0).abs() > 1.0 && (a.1 - b.1).abs() > 1.0,
        }
    }

    pub fn clear_selection(&mut self) {
        self.sel = None;
        self.pdf.all_pending = false;
    }

    /// Whether the text of a PDF is still being read.
    pub fn pdf_text_pending(&self) -> bool {
        self.kind == Kind::Pdf && self.pdf.text.is_none()
    }

    /// Selects everything shown. A PDF with no text at all selects the
    /// current page as a picture.
    pub fn select_all(&mut self) {
        self.sel = match (&self.state, self.kind) {
            (State::Ready(Content::Text(d)), _) => Some(Sel::Text { anchor: 0, caret: d.text.len() }),
            (State::Ready(Content::Rich(_)), _) => self.rich.as_ref().and_then(Layout::bounds).map(|(anchor, caret)| Sel::Rich { anchor, caret }),
            (State::Ready(Content::Csv(d)), _) => (d.rows() > 0 && d.columns > 0).then(|| Sel::Csv { anchor: (0, 0), caret: (d.rows() - 1, d.columns - 1), rows: true, cols: true }),
            (State::Ready(Content::Pdf), _) => match &self.pdf.text {
                None => {
                    self.pdf.all_pending = true;
                    None
                }
                Some(pages) => match pages.iter().rposition(|p| !p.glyphs.is_empty()) {
                    Some(last) => Some(Sel::Pdf { anchor: (0, 0), caret: (last as u32, pages[last].glyphs.len() as u32) }),
                    None => self.pdf.sizes.get(self.pdf.current).map(|s| Sel::Region { page: self.pdf.current as u32, a: (0.0, 0.0), b: *s }),
                },
            },
            _ => None,
        };
    }

    fn text_hit(&self, x: f32, y: f32) -> Option<usize> {
        let State::Ready(Content::Text(doc)) = &self.state else { return None };
        let t = self.text_geom;
        if t.lh <= 0.0 {
            return None;
        }
        let rows = self.wrap_idx.as_ref().map_or(doc.line_count(), |w| w.rows);
        let row = (y - self.view.y - t.pad + self.scroll.1) / t.lh;
        if row < 0.0 {
            return Some(0);
        }
        let row = row as usize;
        if row >= rows {
            return Some(doc.text.len());
        }
        let (line, sub) = self.wrap_idx.as_ref().map_or((row, 0), |w| w.locate(row));
        let text = doc.line(line);
        let base = doc.lines[line];
        let (a, b) = match &self.wrap_idx {
            Some(w) => {
                let starts = cells::wrap(text, w.cells, TAB);
                (starts.get(sub).copied().unwrap_or(0), starts.get(sub + 1).copied().unwrap_or(usize::MAX))
            }
            None => (0, usize::MAX),
        };
        let x0 = self.view.x + t.gutter - if self.wrap_idx.is_some() { 0.0 } else { self.scroll.0 };
        let target = (x - x0) / t.cw;
        let (mut col, mut first) = (0usize, None);
        for (i, (byte, ch)) in text.char_indices().enumerate() {
            if i >= b {
                return Some(base + byte);
            }
            let next = cells::advance(col, ch, TAB);
            if i >= a {
                let f = *first.get_or_insert(col);
                if target < (col + next) as f32 / 2.0 - f as f32 {
                    return Some(base + byte);
                }
            }
            col = next;
        }
        Some(base + text.len())
    }

    /// Cell under a point: (row, column, in the header row, in the row numbers).
    fn csv_hit(&self, x: f32, y: f32) -> Option<(usize, usize, bool, bool)> {
        let State::Ready(Content::Csv(doc)) = &self.state else { return None };
        let c = &self.csv_geom;
        if doc.rows() == 0 || c.widths.is_empty() || c.rh <= 0.0 {
            return None;
        }
        let v = self.view;
        let in_header = y < v.y + c.rh;
        let row = if in_header && self.scroll.1 <= 0.0 { 0 } else { ((y - v.y - c.rh + self.scroll.1) / c.rh).max(0.0) as usize + 1 };
        let in_numbers = x < v.x + c.num_w;
        let mut cx = v.x + c.num_w - self.scroll.0;
        let mut col = c.widths.len() - 1;
        for (i, w) in c.widths.iter().enumerate() {
            if x < cx + w {
                col = i;
                break;
            }
            cx += w;
        }
        Some((row.min(doc.rows() - 1), col, in_header, in_numbers))
    }

    /// Page under a point (the nearest one), and the point in page units.
    fn pdf_point(&self, x: f32, y: f32) -> Option<(u32, f32, f32)> {
        let (dx, dy) = (x - self.view.x + self.scroll.0, y - self.view.y + self.scroll.1);
        let dist = |r: &Rect| (r.y - dy).max(dy - r.bottom()).max(0.0);
        let page = (0..self.pdf.rects.len()).min_by(|a, b| dist(&self.pdf.rects[*a]).total_cmp(&dist(&self.pdf.rects[*b])))?;
        let r = self.pdf.rects[page];
        let size = *self.pdf.sizes.get(page)?;
        let k = r.w / size.0;
        Some((page as u32, (dx - r.x) / k, (dy - r.y) / k))
    }

    fn rich_hit(&self, g: &mut Gfx, x: f32, y: f32) -> Option<Pos> {
        self.rich.as_ref()?.hit(g, x - self.view.x + self.scroll.0, y - self.view.y + self.scroll.1)
    }

    /// Starts a selection at a window point. `extend` keeps the existing
    /// start (Shift), `region` asks for a rectangle of a PDF page (Alt), and
    /// `clicks` is 2 for a word and 3 for a line or paragraph.
    pub fn select_press(&mut self, g: &mut Gfx, x: f32, y: f32, extend: bool, region: bool, clicks: u32) {
        self.pdf.all_pending = false;
        self.sel = match self.kind {
            Kind::Text => self.press_text(x, y, extend, clicks),
            Kind::Markdown | Kind::Html => self.press_rich(g, x, y, extend, clicks),
            Kind::Csv => self.press_csv(x, y, extend),
            Kind::Pdf => self.press_pdf(x, y, extend, region, clicks),
        };
    }

    fn press_text(&self, x: f32, y: f32, extend: bool, clicks: u32) -> Option<Sel> {
        let State::Ready(Content::Text(doc)) = &self.state else { return None };
        let p = self.text_hit(x, y)?;
        Some(match (self.sel, extend, clicks) {
            (Some(Sel::Text { anchor, .. }), true, _) => Sel::Text { anchor, caret: p },
            (_, _, 2) => {
                let (anchor, caret) = word_at(&doc.text, p);
                Sel::Text { anchor, caret }
            }
            (_, _, 3..) => {
                let line = doc.lines.partition_point(|&l| l <= p).saturating_sub(1);
                Sel::Text { anchor: doc.lines[line], caret: doc.lines.get(line + 1).copied().unwrap_or(doc.text.len()) }
            }
            _ => Sel::Text { anchor: p, caret: p },
        })
    }

    fn press_rich(&self, g: &mut Gfx, x: f32, y: f32, extend: bool, clicks: u32) -> Option<Sel> {
        let p = self.rich_hit(g, x, y)?;
        let l = self.rich.as_ref()?;
        let i = p.item as usize;
        Some(match (self.sel, extend, clicks) {
            (Some(Sel::Rich { anchor, .. }), true, _) => Sel::Rich { anchor, caret: p },
            (_, _, 2) => match &l.items[i] {
                Item::Text { s, .. } => {
                    let (a, b) = word_at(s, (p.byte as usize).min(s.len()));
                    Sel::Rich { anchor: Pos { item: p.item, byte: a as u32 }, caret: Pos { item: p.item, byte: b as u32 } }
                }
                _ => Sel::Rich { anchor: Pos { item: p.item, byte: 0 }, caret: Pos { item: p.item, byte: 1 } },
            },
            (_, _, 3..) => {
                let (anchor, caret) = l.paragraph(i);
                Sel::Rich { anchor, caret }
            }
            _ => Sel::Rich { anchor: p, caret: p },
        })
    }

    fn press_csv(&self, x: f32, y: f32, extend: bool) -> Option<Sel> {
        let State::Ready(Content::Csv(doc)) = &self.state else { return None };
        let (row, col, in_header, in_numbers) = self.csv_hit(x, y)?;
        let last = (doc.rows() - 1, doc.columns.saturating_sub(1));
        Some(match (self.sel, extend) {
            (Some(Sel::Csv { anchor, rows, cols, .. }), true) => Sel::Csv { anchor, caret: (row, col), rows, cols },
            _ if in_header && in_numbers => Sel::Csv { anchor: (0, 0), caret: last, rows: true, cols: true },
            _ if in_header => Sel::Csv { anchor: (0, col), caret: (last.0, col), rows: false, cols: true },
            _ if in_numbers => Sel::Csv { anchor: (row, 0), caret: (row, last.1), rows: true, cols: false },
            _ => Sel::Csv { anchor: (row, col), caret: (row, col), rows: false, cols: false },
        })
    }

    fn press_pdf(&self, x: f32, y: f32, extend: bool, region: bool, clicks: u32) -> Option<Sel> {
        let (page, px, py) = self.pdf_point(x, y)?;
        let text = self.pdf.text.as_ref().and_then(|t| t.get(page as usize));
        let has_text = text.is_some_and(|t| !t.glyphs.is_empty());
        if region || !has_text {
            return Some(match (self.sel, extend) {
                (Some(Sel::Region { page: p, a, .. }), true) if p == page => Sel::Region { page, a, b: (px, py) },
                _ => Sel::Region { page, a: (px, py), b: (px, py) },
            });
        }
        let text = text?;
        let at = text.hit(px, py)?;
        let i = (at as usize).min(text.glyphs.len() - 1);
        Some(match (self.sel, extend, clicks) {
            (Some(Sel::Pdf { anchor, .. }), true, _) => Sel::Pdf { anchor, caret: (page, at) },
            (_, _, 2) => {
                let (a, b) = word_at(&text.text, text.glyphs[i].byte as usize);
                let end = if b >= text.text.len() { text.glyphs.len() as u32 } else { text.glyph_at_byte(b) };
                Sel::Pdf { anchor: (page, text.glyph_at_byte(a)), caret: (page, end) }
            }
            (_, _, 3..) => {
                let first = (0..i).rev().find(|k| text.is_break(*k)).map_or(0, |k| k + 1);
                let last = (i..text.glyphs.len()).find(|k| text.is_break(*k)).unwrap_or(text.glyphs.len());
                Sel::Pdf { anchor: (page, first as u32), caret: (page, last as u32) }
            }
            _ => Sel::Pdf { anchor: (page, at), caret: (page, at) },
        })
    }

    /// Moves the end of the selection to a window point.
    pub fn select_drag(&mut self, g: &mut Gfx, x: f32, y: f32) {
        self.sel = match self.sel {
            Some(Sel::Text { anchor, caret }) => Some(Sel::Text { anchor, caret: self.text_hit(x, y).unwrap_or(caret) }),
            Some(Sel::Rich { anchor, caret }) => Some(Sel::Rich { anchor, caret: self.rich_hit(g, x, y).unwrap_or(caret) }),
            Some(Sel::Csv { anchor, caret, rows, cols }) => {
                let c = self.csv_hit(x, y).map_or(caret, |(r, c, _, _)| (r, c));
                Some(Sel::Csv { anchor, caret: c, rows, cols })
            }
            Some(Sel::Pdf { anchor, caret }) => {
                let hit = self.pdf_point(x, y).and_then(|(p, px, py)| {
                    let t = self.pdf.text.as_ref()?.get(p as usize)?;
                    if t.glyphs.is_empty() {
                        return None;
                    }
                    Some((p, t.hit(px, py)?))
                });
                Some(Sel::Pdf { anchor, caret: hit.unwrap_or(caret) })
            }
            Some(Sel::Region { page, a, b }) => {
                let size = self.pdf.sizes.get(page as usize).copied().unwrap_or((0.0, 0.0));
                let end = self.pdf.rects.get(page as usize).map_or(b, |r| {
                    let k = r.w / size.0.max(1.0);
                    let (dx, dy) = (x - self.view.x + self.scroll.0, y - self.view.y + self.scroll.1);
                    (((dx - r.x) / k).clamp(0.0, size.0), ((dy - r.y) / k).clamp(0.0, size.1))
                });
                Some(Sel::Region { page, a, b: end })
            }
            None => None,
        };
    }

    /// Scrolls towards a pointer dragged past the view's edge; returns true
    /// if the view moved.
    pub fn autoscroll(&mut self, x: f32, y: f32) -> bool {
        let v = self.view;
        let top = if self.kind == Kind::Csv { v.y + self.csv_geom.rh } else { v.y };
        let dy = if y < top { y - top } else { (y - v.bottom()).max(0.0) };
        let dx = if x < v.x { x - v.x } else { (x - v.right()).max(0.0) };
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        let before = self.scroll;
        self.scroll_by(dx.clamp(-MAX_AUTOSCROLL, MAX_AUTOSCROLL), dy.clamp(-MAX_AUTOSCROLL, MAX_AUTOSCROLL));
        self.scroll != before
    }

    /// What a pointer at a window point can do.
    pub fn hover(&self, x: f32, y: f32, region: bool) -> Hover {
        if !self.view.contains(x, y) || !matches!(self.state, State::Ready(_)) {
            return Hover::Arrow;
        }
        match self.kind {
            Kind::Text => {
                let gutter = self.view.x + self.text_geom.gutter;
                if x >= gutter - self.text_geom.pad {
                    Hover::Text
                } else {
                    Hover::Arrow
                }
            }
            Kind::Csv => Hover::Cell,
            Kind::Markdown | Kind::Html => match &self.rich {
                Some(l) if l.over_text(x - self.view.x + self.scroll.0, y - self.view.y + self.scroll.1) => Hover::Text,
                _ => Hover::Arrow,
            },
            Kind::Pdf => {
                let Some((page, px, py)) = self.pdf_point(x, y) else { return Hover::Arrow };
                let size = self.pdf.sizes.get(page as usize).copied().unwrap_or((0.0, 0.0));
                if px < 0.0 || py < 0.0 || px > size.0 || py > size.1 {
                    return Hover::Arrow;
                }
                match self.pdf.text.as_ref().and_then(|t| t.get(page as usize)) {
                    _ if region => Hover::Cross,
                    Some(t) if t.glyphs.is_empty() => Hover::Cross,
                    Some(t) if t.over(px, py) => Hover::Text,
                    _ => Hover::Arrow,
                }
            }
        }
    }

    /// Keeps a Markdown or HTML selection across a new layout.
    pub(super) fn sel_ordinals(&self) -> Option<(usize, usize)> {
        match (self.sel, &self.rich) {
            (Some(Sel::Rich { anchor, caret }), Some(l)) => Some((l.ordinal(anchor), l.ordinal(caret))),
            _ => None,
        }
    }

    pub(super) fn restore_ordinals(&mut self, ordinals: Option<(usize, usize)>) {
        if let (Some((a, c)), Some(l)) = (ordinals, &self.rich) {
            self.sel = Some(Sel::Rich { anchor: l.at_ordinal(a), caret: l.at_ordinal(c) });
        }
    }

    pub(super) fn text_range(&self) -> Option<(usize, usize)> {
        match self.sel {
            Some(Sel::Text { anchor, caret }) if anchor != caret => Some(ordered(anchor, caret)),
            _ => None,
        }
    }

    pub(super) fn rich_range(&self) -> Option<(Pos, Pos)> {
        match self.sel {
            Some(Sel::Rich { anchor, caret }) if anchor != caret => Some(ordered(anchor, caret)),
            _ => None,
        }
    }

    /// Selected cells: (first row, last row, first column, last column).
    pub(super) fn csv_range(&self) -> Option<(usize, usize, usize, usize)> {
        let State::Ready(Content::Csv(d)) = &self.state else { return None };
        let Some(Sel::Csv { anchor, caret, rows, cols }) = self.sel else { return None };
        let last = d.columns.saturating_sub(1);
        let (r0, r1) = ordered(anchor.0, caret.0);
        let (c0, c1) = ordered(anchor.1, caret.1);
        let (r0, r1) = if cols && !rows { (0, d.rows().saturating_sub(1)) } else { (r0, r1) };
        let (c0, c1) = if rows { (0, last) } else { (c0, c1) };
        Some((r0, r1, c0, c1))
    }

    pub(super) fn pdf_range(&self, page: u32, count: usize) -> Option<(usize, usize)> {
        match self.sel {
            Some(Sel::Pdf { anchor, caret }) => page_range(ordered(anchor, caret), page, count),
            _ => None,
        }
    }

    /// A selected rectangle on `page`, in page units.
    pub(super) fn pdf_region(&self, page: u32) -> Option<Rect> {
        match self.sel {
            Some(Sel::Region { page: p, a, b }) if p == page => {
                let (x0, x1) = ordered(a.0, b.0);
                let (y0, y1) = ordered(a.1, b.1);
                Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
            }
            _ => None,
        }
    }

    // ------------------------------------------------------------ copying

    /// The selection as clipboard contents: all formats, or text alone.
    /// None when nothing is selected (or nothing of the wanted kind).
    pub fn payload(&mut self, plain_only: bool) -> Option<Payload> {
        if !self.has_selection() {
            return None;
        }
        let out = match self.sel? {
            Sel::Text { anchor, caret } => {
                let (s, e) = ordered(anchor, caret);
                self.text_payload(s, e, plain_only)
            }
            Sel::Rich { anchor, caret } => {
                let (a, b) = ordered(anchor, caret);
                self.rich_payload(a, b, plain_only)
            }
            Sel::Csv { .. } => self.csv_payload(plain_only),
            Sel::Pdf { anchor, caret } => {
                let (a, b) = ordered(anchor, caret);
                let evs = pdf_events(self.pdf.text.as_ref()?, a, b);
                let text = clip::plain(&evs);
                let too_large = !plain_only && text.len() > MAX_RICH_TEXT;
                let (html, rtf) = if plain_only || too_large { (None, None) } else { formatted(&evs) };
                Some(Payload { text, html, rtf, image: None, too_large })
            }
            Sel::Region { page, .. } => self.region_payload(page, plain_only),
        }?;
        (!out.text.is_empty() || out.image.is_some()).then_some(out)
    }

    fn text_payload(&mut self, s: usize, e: usize, plain_only: bool) -> Option<Payload> {
        let Viewer { state, hl, .. } = self;
        let State::Ready(Content::Text(doc)) = state else { return None };
        let text = doc.text[s..e].to_string();
        let too_large = !plain_only && text.len() > MAX_RICH_TEXT;
        if plain_only || too_large {
            return Some(Payload { text, too_large, ..Payload::default() });
        }
        let base = Fmt { mono: true, size: 0.9, ..Fmt::default() };
        let mut evs = vec![Ev::Enter(Tag::Para { align: Align::Left, pre: true, heading: 0, bg: None })];
        let first = doc.lines.partition_point(|&l| l <= s).saturating_sub(1);
        let mut spans = Vec::new();
        for line in first..doc.line_count() {
            let start = doc.lines[line];
            if start >= e && line > first {
                break;
            }
            if line > first {
                evs.push(Ev::Br);
            }
            let t = doc.line(line);
            let (from, to) = (s.saturating_sub(start).min(t.len()), e.saturating_sub(start).min(t.len()));
            spans.clear();
            if let Some(h) = hl {
                h.spans(&|i| doc.line(i), line, &mut spans);
            }
            let mut at = from;
            for sp in &spans {
                let (a, b) = (sp.start.max(from), sp.end.min(to));
                if a >= b {
                    continue;
                }
                if at < a {
                    clip::push_run(&mut evs, &t[at..a], base.clone());
                }
                clip::push_run(&mut evs, &t[a..b], Fmt { color: crate::syntax::color(sp.tok, false), ..base.clone() });
                at = b;
            }
            if at < to {
                clip::push_run(&mut evs, &t[at..to], base.clone());
            }
        }
        evs.push(Ev::Exit);
        let (html, rtf) = formatted(&evs);
        Some(Payload { text, html, rtf, image: None, too_large: false })
    }

    fn rich_payload(&self, a: Pos, b: Pos, plain_only: bool) -> Option<Payload> {
        let (l, doc) = (self.rich.as_ref()?, self.rich_doc()?);
        let evs = rich_events(l, doc, a, b);
        let text = clip::plain(&evs);
        let too_large = !plain_only && text.len() > MAX_RICH_TEXT;
        let (html, rtf) = if plain_only || too_large { (None, None) } else { formatted(&evs) };
        // A lone image also goes as a picture.
        let image = if plain_only {
            None
        } else {
            let mut shown = evs.iter().filter(|e| matches!(e, Ev::Run(..) | Ev::Image { .. }));
            match (shown.next(), shown.next(), l.items.get(a.item as usize)) {
                (Some(Ev::Image { .. }), None, _) => {
                    let key = (a.item..=b.item).find_map(|i| match l.items.get(i as usize) {
                        Some(Item::Image { key, .. }) => Some(key),
                        _ => None,
                    });
                    key.and_then(|k| self.images.get(k)).and_then(|i| i.pix.as_ref()).map(|p| Bitmap { w: p.width(), h: p.height(), rgba: p.data().to_vec() })
                }
                _ => None,
            }
        };
        Some(Payload { text, html, rtf, image, too_large })
    }

    fn csv_payload(&self, plain_only: bool) -> Option<Payload> {
        let State::Ready(Content::Csv(doc)) = &self.state else { return None };
        let (r0, r1, c0, c1) = self.csv_range()?;
        let n = (r1 - r0 + 1).saturating_mul(c1 - c0 + 1);
        let too_large = !plain_only && n > MAX_RICH_CELLS;
        let rich = !plain_only && !too_large;
        let mut text = String::new();
        let mut evs = vec![Ev::Enter(Tag::Table)];
        for r in r0..=r1 {
            let rec = doc.record(r);
            if rich {
                evs.push(Ev::Enter(Tag::Row));
            }
            for c in c0..=c1 {
                let f = rec.get(c).map_or("", String::as_str);
                if c > c0 {
                    text.push('\t');
                }
                tsv_field(f, &mut text);
                if rich {
                    let align = if r > 0 && csv::is_number(f) { Align::Right } else { Align::Left };
                    evs.push(Ev::Enter(Tag::Cell { header: r == 0, colspan: 1, align, bg: None }));
                    evs.push(Ev::Enter(Tag::Para { align, pre: false, heading: 0, bg: None }));
                    evs.push(Ev::Run(f.replace("\r\n", "\n"), Fmt { bold: r == 0, ..Fmt::default() }));
                    evs.push(Ev::Exit);
                    evs.push(Ev::Exit);
                }
            }
            if rich {
                evs.push(Ev::Exit);
            }
            if r < r1 {
                text.push('\n');
            }
        }
        evs.push(Ev::Exit);
        let (html, rtf) = if rich { formatted(&evs) } else { (None, None) };
        Some(Payload { text, html, rtf, image: None, too_large })
    }

    /// A rectangle of a PDF page: the picture as shown, plus any text inside.
    fn region_payload(&self, page: u32, plain_only: bool) -> Option<Payload> {
        let r = self.pdf_region(page)?;
        let text = self.pdf.text.as_ref().and_then(|t| t.get(page as usize)).map_or(String::new(), |t| {
            let inside: Vec<usize> = (0..t.glyphs.len()).filter(|i| r.contains(t.glyphs[*i].x + t.glyphs[*i].w / 2.0, t.glyphs[*i].y + t.glyphs[*i].h / 2.0)).collect();
            let mut out = String::new();
            for (n, &i) in inside.iter().enumerate() {
                if n > 0 && inside[n - 1] + 1 != i && t.glyphs[inside[n - 1]].y != t.glyphs[i].y {
                    out.push('\n');
                }
                out.push_str(t.glyph_text(i));
            }
            out.trim().to_string()
        });
        let image = if plain_only { None } else { self.crop_page(page, r) };
        Some(Payload { text, html: None, rtf: None, image, too_large: false })
    }

    /// Cuts a rectangle (page units) out of a rendered page.
    fn crop_page(&self, page: u32, r: Rect) -> Option<Bitmap> {
        let (_, pix) = self.pdf.pages.get(&page)?;
        let size = self.pdf.sizes.get(page as usize)?;
        let k = pix.width() as f32 / size.0;
        let (pw, ph) = (pix.width() as usize, pix.height() as usize);
        let x0 = ((r.x * k).round().max(0.0) as usize).min(pw);
        let y0 = ((r.y * k).round().max(0.0) as usize).min(ph);
        let x1 = ((r.right() * k).round().max(0.0) as usize).min(pw);
        let y1 = ((r.bottom() * k).round().max(0.0) as usize).min(ph);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let mut rgba = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
        for y in y0..y1 {
            rgba.extend_from_slice(&pix.data()[(y * pw + x0) * 4..(y * pw + x1) * 4]);
        }
        Some(Bitmap { w: (x1 - x0) as u32, h: (y1 - y0) as u32, rgba })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc;
    use crate::ui::theme::Theme;

    const W: f32 = 800.0;
    const H: f32 = 600.0;

    struct Rig {
        v: Viewer,
        g: Gfx,
        media: crate::media::Media,
    }

    impl Rig {
        fn open(name: &str, body: &[u8]) -> Option<Rig> {
            let g = Gfx::new().ok()?;
            let dir = std::env::temp_dir().join(format!("rview-sel-{}-{name}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let p = dir.join(name);
            std::fs::write(&p, body).ok()?;
            let mut v = Viewer::new(true);
            let gen = v.begin(&p, false, None);
            v.finish(gen, doc::load(&p, Kind::of(&p)));
            let _ = std::fs::remove_dir_all(&dir);
            let media = crate::media::Media::start(std::sync::Arc::new(|| {}));
            let mut r = Rig { v, g, media };
            r.draw();
            Some(r)
        }

        fn draw(&mut self) {
            self.g.resize(W as u32, H as u32);
            self.v.draw(&mut self.g, Rect::new(0.0, 0.0, W, H), &Theme::new(false), 1.0, &self.media);
        }

        fn drag(&mut self, from: (f32, f32), to: (f32, f32)) {
            self.v.select_press(&mut self.g, from.0, from.1, false, false, 1);
            self.v.select_drag(&mut self.g, to.0, to.1);
        }

        /// Window position of the first text item containing `what`.
        fn find(&self, what: &str) -> (f32, f32) {
            let l = self.v.rich.as_ref().expect("laid out");
            for it in &l.items {
                if let Item::Text { x, y, s, px, .. } = it {
                    if s.contains(what) {
                        return (x + 1.0, y - px * 0.4);
                    }
                }
            }
            panic!("{what} not found");
        }

        fn text(&mut self) -> String {
            self.v.payload(true).expect("selection").text
        }

        fn html(&mut self) -> String {
            self.v.payload(false).expect("selection").html.expect("html")
        }
    }

    #[test]
    fn words() {
        assert_eq!(word_at("hello big world", 7), (6, 9));
        assert_eq!(word_at("hello big world", 9), (6, 9));
        assert_eq!(word_at("hello  world", 6), (5, 7));
        assert_eq!(word_at("a.b", 1), (0, 1));
        assert_eq!(word_at("olá mundo", 2), (0, 4));
        assert_eq!(word_at("", 0), (0, 0));
    }

    #[test]
    fn text_selection_by_mouse() {
        let Some(mut r) = Rig::open("a.txt", b"first line\nsecond line\nthird") else { return };
        let t = r.v.text_geom;
        let row = |n: f32| t.pad + t.lh * n + t.lh / 2.0;
        let col = |n: f32| t.gutter + t.cw * n;
        r.drag((col(6.0), row(0.0)), (col(6.0), row(1.0)));
        assert_eq!(r.text(), "line\nsecond");
        r.v.select_press(&mut r.g, col(1.0), row(1.0), false, false, 2);
        assert_eq!(r.text(), "second");
        r.v.select_press(&mut r.g, col(1.0), row(1.0), false, false, 3);
        assert_eq!(r.text(), "second line\n");
        r.v.select_all();
        assert_eq!(r.text(), "first line\nsecond line\nthird");
        // A click alone selects nothing; Shift+click extends from it.
        r.v.select_press(&mut r.g, col(0.0), row(2.0), false, false, 1);
        assert!(!r.v.has_selection());
        r.v.select_press(&mut r.g, col(5.0), row(2.0), true, false, 1);
        assert_eq!(r.text(), "third");
        r.v.clear_selection();
        assert!(r.v.payload(false).is_none());
    }

    #[test]
    fn wrapped_text_keeps_every_character() {
        let line = "abcd efgh ".repeat(40);
        let Some(mut r) = Rig::open("w.txt", line.as_bytes()) else { return };
        let t = r.v.text_geom;
        r.drag((t.gutter, t.pad + t.lh * 0.5), (t.gutter, t.pad + t.lh * 1.5));
        let got = r.text();
        assert!(!got.is_empty() && line.starts_with(&got), "{got:?}");
    }

    #[test]
    fn code_is_copied_with_light_syntax_colors() {
        let Some(mut r) = Rig::open("a.rs", b"fn main() {}\n// done\n") else { return };
        r.v.select_all();
        let p = r.v.payload(false).unwrap();
        let h = p.html.unwrap();
        assert!(h.contains("<pre"), "{h}");
        assert!(h.contains("color:#0000FF"), "keyword color: {h}");
        assert!(h.contains("<br>"));
        assert!(p.rtf.unwrap().contains("\\cf"));
        assert_eq!(p.text, "fn main() {}\n// done\n");
    }

    #[test]
    fn large_selections_fall_back_to_plain_text() {
        let body = vec![b'x'; MAX_RICH_TEXT + 10];
        let Some(mut r) = Rig::open("big.txt", &body) else { return };
        r.v.select_all();
        let p = r.v.payload(false).unwrap();
        assert!(p.too_large && p.html.is_none() && p.rtf.is_none());
        assert_eq!(p.text.len(), body.len());
    }

    #[test]
    fn markdown_keeps_structure_and_formats() {
        let md = "# Title\n\nSome **bold** and *italic* with [a link](https://x.org) here.\n\n- one\n- two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> quoted\n\n```rust\nfn f() {}\n```\n";
        let Some(mut r) = Rig::open("a.md", md.as_bytes()) else { return };
        r.v.select_all();
        let t = r.text();
        assert_eq!(t, "Title\n\nSome bold and italic with a link here.\n\n• one\n• two\n\na\tb\n1\t2\n\nquoted\n\nfn f() {}");
        let h = r.html();
        for want in ["<h1>", "font-weight:bold", "font-style:italic", "<a href=\"https://x.org\">", "<ul>", "<li>", "<table", "<th>", "<td>", "<blockquote>", "<pre"] {
            assert!(h.contains(want), "{want} missing in {h}");
        }
        assert!(!h.contains('•'), "lists are real lists: {h}");
    }

    #[test]
    fn markdown_partial_selection() {
        let Some(mut r) = Rig::open("b.md", b"Before **bold** after.\n\nNext paragraph.\n") else { return };
        let from = r.find("bold");
        let to = r.find("Next");
        r.drag(from, (to.0 + 10.0, to.1));
        assert_eq!(r.text(), "bold after.\n\nN");
        let h = r.html();
        assert!(h.contains("<p><span style=\"font-weight:bold;\">bold</span> after.</p><p>N</p>"), "{h}");
    }

    #[test]
    fn wrapped_paragraphs_join_with_spaces() {
        let text = "word ".repeat(300);
        let Some(mut r) = Rig::open("c.md", text.as_bytes()) else { return };
        r.v.select_all();
        assert_eq!(r.text(), text.trim());
    }

    #[test]
    fn selection_survives_a_new_layout() {
        let text = "alpha beta gamma delta ".repeat(60);
        let Some(mut r) = Rig::open("d.md", text.as_bytes()) else { return };
        let from = r.find("alpha");
        r.drag(from, (from.0 + 300.0, from.1 + 60.0));
        let before = r.text();
        r.v.set_zoom(1.5, None);
        r.draw();
        assert_eq!(r.text().replace(['\n', ' '], ""), before.replace(['\n', ' '], ""));
    }

    #[test]
    fn html_colors_are_the_documents_even_in_the_dark_theme() {
        let html = "<p style='color:#123456'>dark blue</p>";
        let Some(mut r) = Rig::open("e.html", html.as_bytes()) else { return };
        r.v.draw(&mut r.g, Rect::new(0.0, 0.0, W, H), &Theme::new(true), 1.0, &r.media);
        r.v.select_all();
        assert!(r.html().contains("color:#123456"));
    }

    #[test]
    fn csv_cells_rows_and_columns() {
        let Some(mut r) = Rig::open("a.csv", b"name,qty\napple,3\n\"pear, big\",12\nkiwi,\"x\ty\"\n") else { return };
        let c = r.v.csv_geom.clone();
        let x = |col: usize| c.num_w + c.widths[..col].iter().sum::<f32>() + 5.0;
        let y = |row: usize| c.rh * row as f32 + c.rh / 2.0;
        r.drag((x(0), y(1)), (x(1), y(2)));
        assert_eq!(r.text(), "apple\t3\npear, big\t12");
        let h = r.html();
        assert!(h.contains("<table") && h.contains("text-align:right") && !h.contains("<th>"), "{h}");
        // Column header: the whole column, with its title.
        r.v.select_press(&mut r.g, x(1), y(0), false, false, 1);
        assert_eq!(r.text(), "qty\n3\n12\n\"x\ty\"");
        // Row number: the whole row.
        r.v.select_press(&mut r.g, 2.0, y(1), false, false, 1);
        assert_eq!(r.text(), "apple\t3");
        r.v.select_all();
        assert!(r.html().contains("<th>"));
        assert!(r.text().starts_with("name\tqty\n"));
    }

    fn sample_pdf() -> Vec<u8> {
        let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Italic /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 122 /Widths [250 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 600 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 500 500 500 500 500 500 500 500 500 500 500 500 500 500 500 500 500 500 500 500 500 500 500 500 500 500] >>";
        doc::pdf::tests::pdf(
            &[("/MediaBox [0 0 400 300]", "BT /F1 12 Tf 40 250 Td (hello world) Tj 0 -40 Td (second part) Tj ET"), ("/MediaBox [0 0 400 300]", "BT /F1 12 Tf 40 250 Td (next page) Tj ET")],
            font,
            "",
        )
    }

    /// A viewer showing a PDF, without Windows' renderer: sizes as reported
    /// for it and the text read directly.
    fn pdf_rig() -> Option<Rig> {
        let g = Gfx::new().ok()?;
        let sizes = vec![(533.0, 400.0), (533.0, 400.0)];
        let mut v = Viewer::new(true);
        v.kind = Kind::Pdf;
        v.state = State::Ready(Content::Pdf);
        v.pdf.text = doc::pdf::extract_bytes(&sample_pdf(), &sizes, &|| true);
        v.pdf.text_asked = true;
        v.pdf.sizes = sizes;
        let media = crate::media::Media::start(std::sync::Arc::new(|| {}));
        let mut r = Rig { v, g, media };
        r.draw();
        Some(r)
    }

    fn glyph_point(r: &Rig, page: usize, glyph: usize) -> (f32, f32) {
        let g = r.v.pdf.text.as_ref().unwrap()[page].glyphs[glyph];
        let rect = r.v.pdf.rects[page];
        let k = rect.w / r.v.pdf.sizes[page].0;
        (rect.x - r.v.scroll.0 + (g.x + 0.5) * k, rect.y - r.v.scroll.1 + (g.y + g.h / 2.0) * k)
    }

    #[test]
    fn pdf_text_selection_and_formats() {
        let Some(mut r) = pdf_rig() else { return };
        assert!(!r.v.pdf_text_pending());
        let (a, b) = (glyph_point(&r, 0, 6), glyph_point(&r, 0, 18));
        r.drag(a, b);
        assert_eq!(r.text(), "world\n\nsecond");
        let h = r.html();
        assert!(h.contains("font-style:italic"), "{h}");
        assert_eq!(h.matches("<p>").count(), 2, "{h}");
        r.v.select_all();
        assert_eq!(r.text(), "hello world\n\nsecond part\n\nnext page");
    }

    #[test]
    fn pdf_words_lines_and_region() {
        let Some(mut r) = pdf_rig() else { return };
        let p = glyph_point(&r, 0, 7);
        r.v.select_press(&mut r.g, p.0, p.1, false, false, 2);
        assert_eq!(r.text(), "world");
        r.v.select_press(&mut r.g, p.0, p.1, false, false, 3);
        assert_eq!(r.text(), "hello world");
        // Alt+drag: a rectangle; the text inside it comes along.
        let (a, b) = (glyph_point(&r, 0, 0), glyph_point(&r, 0, 4));
        r.v.select_press(&mut r.g, a.0 - 3.0, a.1 - 12.0, false, true, 1);
        r.v.select_drag(&mut r.g, b.0 + 8.0, b.1 + 12.0);
        assert!(matches!(r.v.sel, Some(Sel::Region { .. })));
        assert_eq!(r.text(), "hello");
    }

    #[test]
    fn pdf_page_without_text_selects_a_region_and_select_all_takes_the_page() {
        let Some(mut r) = pdf_rig() else { return };
        r.v.pdf.text = Some(vec![PageText::default(), PageText::default()]);
        r.v.select_press(&mut r.g, 200.0, 100.0, false, false, 1);
        r.v.select_drag(&mut r.g, 300.0, 200.0);
        assert!(matches!(r.v.sel, Some(Sel::Region { page: 0, .. })));
        assert!(r.v.has_selection());
        // Nothing to copy as text; the picture needs a rendered page.
        assert!(r.v.payload(true).is_none());
        r.v.select_all();
        assert!(matches!(r.v.sel, Some(Sel::Region { page: 0, b: (533.0, 400.0), .. })));
        // With a rendered page the region is cut out of it.
        r.v.pdf.pages.insert(0, (533, tiny_skia::Pixmap::new(533, 400).unwrap()));
        r.v.sel = Some(Sel::Region { page: 0, a: (10.0, 20.0), b: (30.0, 60.0) });
        let img = r.v.payload(false).unwrap().image.unwrap();
        assert_eq!((img.w, img.h), (20, 40));
    }

    #[test]
    fn select_all_waits_for_pdf_text() {
        let Some(mut r) = pdf_rig() else { return };
        let pages = r.v.pdf.text.take();
        r.v.select_all();
        assert!(!r.v.has_selection() && r.v.pdf.all_pending);
        r.v.on_media(crate::media::Done::PdfText { gen: r.v.gen, pages: pages.unwrap() });
        assert!(r.v.has_selection());
    }

    #[test]
    fn new_documents_clear_the_selection() {
        let Some(mut r) = Rig::open("f.txt", b"text") else { return };
        r.v.select_all();
        r.v.begin(Path::new("other.txt"), false, None);
        assert!(!r.v.has_selection());
    }

    #[test]
    fn autoscroll_follows_the_pointer() {
        let body = "line\n".repeat(500);
        let Some(mut r) = Rig::open("g.txt", body.as_bytes()) else { return };
        assert!(!r.v.autoscroll(100.0, 100.0));
        assert!(r.v.autoscroll(100.0, H + 20.0));
        assert!(r.v.scroll.1 > 0.0);
    }
}
