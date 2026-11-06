//! Lays out a rich document for a width and scale into a flat list of
//! positioned drawing items (text runs, rectangles, images). Drawing then
//! only walks the items intersecting the viewport.

use super::{Align, Block, Inline, Kind, Length, RichDoc, Style, Table};
use crate::clip::Tag;
use crate::ui::gfx::{Color, FontId, Gfx, Rect};
use std::collections::HashMap;

/// Base text size in CSS pixels at 100 % zoom.
pub const BASE_PX: f32 = 15.0;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color,
    pub text: Color,
    pub link: Color,
    pub code_bg: Color,
    pub border: Color,
    pub header_bg: Color,
    pub muted: Color,
    /// Dark background: syntax colors for dark themes.
    pub dark: bool,
    /// Adapt author colors to a dark page (light backgrounds become dark,
    /// dark text becomes light; hues are kept).
    pub adapt: bool,
}

/// Inverts a color's lightness, keeping its hue, into a soft dark range
/// (white becomes #1E1E1E, black becomes white).
pub fn invert_lightness(c: Color) -> Color {
    let (mx, mn) = (c.r.max(c.g).max(c.b) as i32, c.r.min(c.g).min(c.b) as i32);
    let f = |v: u8| (30 + (v as i32 + 255 - mx - mn).clamp(0, 255) * 225 / 255) as u8;
    Color { r: f(c.r), g: f(c.g), b: f(c.b), a: c.a }
}

pub fn luminance(c: Color) -> f32 {
    0.299 * c.r as f32 + 0.587 * c.g as f32 + 0.114 * c.b as f32
}

impl Palette {
    /// A foreground color as shown on this palette's page.
    pub fn fg(&self, c: Color) -> Color {
        if self.adapt && luminance(c) < 140.0 {
            invert_lightness(c)
        } else {
            c
        }
    }

    /// A background color as shown on this palette's page.
    pub fn bgc(&self, c: Color) -> Color {
        if self.adapt && luminance(c) > 100.0 {
            invert_lightness(c)
        } else {
            c
        }
    }
}

impl Palette {
    pub fn paper() -> Palette {
        Palette {
            bg: Color::rgb(0xFFFFFF),
            text: Color::rgb(0x1F2328),
            link: Color::rgb(0x0B5CC1),
            code_bg: Color::rgb(0xEFF1F3),
            border: Color::rgb(0xD0D7DE),
            header_bg: Color::rgb(0xF6F8FA),
            muted: Color::rgb(0x59636E),
            dark: false,
            adapt: false,
        }
    }

    pub fn dark() -> Palette {
        Palette {
            bg: Color::rgb(0x1E1E1E),
            text: Color::rgb(0xE2E2E2),
            link: Color::rgb(0x6CB6FF),
            code_bg: Color::rgb(0x2D2D30),
            border: Color::rgb(0x44474C),
            header_bg: Color::rgb(0x262629),
            muted: Color::rgb(0x9DA3AB),
            dark: true,
            adapt: false,
        }
    }
}

/// Image state as known to the caller (decoded on a worker thread).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ImgState {
    Pending,
    Ready(u32, u32),
    Failed,
}

#[derive(Clone, Debug)]
pub enum Item {
    /// Text with its baseline at `y`.
    Text { x: f32, y: f32, s: String, px: f32, font: FontId, color: Color },
    Fill { r: Rect, color: Color },
    Image { r: Rect, key: String },
}

/// A place in the text of a layout: inside text item `item` (an index into
/// `Layout::items`) at byte `byte`, or before (0) or after (1) an image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub item: u32,
    pub byte: u32,
}

/// What selection and copying need to know about an item.
#[derive(Clone, Debug, Default)]
pub struct Meta {
    /// Innermost structure node (index into `Layout::nodes`).
    pub node: u32,
    pub style: Style,
    /// Width in pixels (text and images).
    pub w: f32,
    /// A list marker, not document text.
    pub marker: bool,
    /// A space separates this item from the previous one.
    pub space: bool,
    /// This item starts a wrapped line.
    pub wrapped: bool,
    /// Hard line breaks before this item.
    pub breaks: u8,
    pub alt: Option<Box<str>>,
}

/// The block structure around the text: a tree stored parent-first.
#[derive(Clone, Debug)]
pub struct Node {
    pub parent: u32,
    pub tag: Tag,
}

#[derive(Default)]
pub struct Layout {
    pub items: Vec<Item>,
    /// Vertical extent of each item (for culling).
    pub spans: Vec<(f32, f32)>,
    /// One entry per item.
    pub meta: Vec<Meta>,
    /// Node 0 is the document root.
    pub nodes: Vec<Node>,
    pub width: f32,
    pub height: f32,
    pub links: Vec<(Rect, u32)>,
    pub anchors: HashMap<String, f32>,
    /// Image keys referenced by the document.
    pub images: Vec<String>,
}

impl Layout {
    fn push(&mut self, it: Item) -> usize {
        let span = match &it {
            Item::Text { y, px, .. } => (y - px * 1.1, y + px * 0.4),
            Item::Fill { r, .. } | Item::Image { r, .. } => (r.y, r.bottom()),
        };
        self.items.push(it);
        self.spans.push(span);
        self.meta.push(Meta::default());
        self.items.len() - 1
    }

    fn patch(&mut self, idx: usize, r: Rect) {
        if let Item::Fill { r: old, .. } = &mut self.items[idx] {
            *old = r;
            self.spans[idx] = (r.y, r.bottom());
        }
    }

    /// Link under a document point.
    pub fn link_at(&self, x: f32, y: f32) -> Option<u32> {
        self.links.iter().find(|(r, _)| r.contains(x, y)).map(|(_, l)| *l)
    }

    /// Selectable box of an item, if it has one.
    fn item_box(&self, i: usize) -> Option<Rect> {
        match &self.items[i] {
            Item::Text { x, y, s, px, .. } if !s.is_empty() => Some(Rect::new(*x, y - px * 0.95, self.meta[i].w, px * 1.3)),
            Item::Image { r, .. } => Some(*r),
            _ => None,
        }
    }

    /// Whether a document point is on text.
    pub fn over_text(&self, x: f32, y: f32) -> bool {
        (0..self.items.len()).any(|i| matches!(self.items[i], Item::Text { .. }) && self.item_box(i).is_some_and(|b| b.contains(x, y)))
    }

    /// The place in the text nearest to a document point.
    pub fn hit(&self, g: &mut Gfx, x: f32, y: f32) -> Option<Pos> {
        let mut best: Option<(f32, usize)> = None;
        for i in 0..self.items.len() {
            let Some(b) = self.item_box(i) else { continue };
            let dy = (b.y - y).max(y - b.bottom()).max(0.0);
            let dx = (b.x - x).max(x - b.right()).max(0.0);
            // Reaching along a line is more likely than reaching across lines.
            let score = dy * 4.0 + dx;
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, i));
            }
        }
        let (_, i) = best?;
        let byte = match &self.items[i] {
            Item::Text { x: ix, s, px, font, .. } => {
                let mut pen = *ix;
                let mut at = s.len();
                for (b, c) in s.char_indices() {
                    let w = g.advance(c, *px, *font);
                    if x < pen + w / 2.0 {
                        at = b;
                        break;
                    }
                    pen += w;
                }
                at
            }
            Item::Image { r, .. } => usize::from(x > r.x + r.w / 2.0),
            Item::Fill { .. } => 0,
        };
        Some(Pos { item: i as u32, byte: byte as u32 })
    }

    /// Length of an item in `Pos::byte` units.
    pub fn item_len(&self, i: usize) -> usize {
        match &self.items[i] {
            Item::Text { s, .. } => s.len(),
            Item::Image { .. } => 1,
            Item::Fill { .. } => 0,
        }
    }

    /// First and past-the-end places of the document.
    pub fn bounds(&self) -> Option<(Pos, Pos)> {
        let sel = |i: &usize| self.item_len(*i) > 0;
        let first = (0..self.items.len()).find(sel)?;
        let last = (0..self.items.len()).rfind(sel)?;
        Some((Pos { item: first as u32, byte: 0 }, Pos { item: last as u32, byte: self.item_len(last) as u32 }))
    }

    /// Items of the paragraph around item `i`, as a selection.
    pub fn paragraph(&self, i: usize) -> (Pos, Pos) {
        let node = self.meta[i].node;
        let same = |k: &usize| self.item_len(*k) > 0 && self.meta[*k].node == node;
        let first = (0..=i).rev().take_while(|k| self.item_len(*k) == 0 || self.meta[*k].node == node).filter(same).last().unwrap_or(i);
        let last = (i..self.items.len()).take_while(|k| self.item_len(*k) == 0 || self.meta[*k].node == node).filter(same).last().unwrap_or(i);
        (Pos { item: first as u32, byte: 0 }, Pos { item: last as u32, byte: self.item_len(last) as u32 })
    }

    /// Counts the visible characters before `p`. Layouts of one document
    /// count the same however they wrap, so a selection survives zooming
    /// and resizing.
    pub fn ordinal(&self, p: Pos) -> usize {
        let visible = |s: &str| s.chars().filter(|c| !c.is_whitespace()).count();
        let mut n = 0;
        for (i, it) in self.items.iter().enumerate().take(p.item as usize + 1) {
            match it {
                Item::Text { s, .. } if i == p.item as usize => n += visible(&s[..(p.byte as usize).min(s.len())]),
                Item::Text { s, .. } => n += visible(s),
                Item::Image { .. } => n += usize::from(i != p.item as usize || p.byte > 0),
                Item::Fill { .. } => {}
            }
        }
        n
    }

    /// The place after `n` visible characters (inverse of [`Layout::ordinal`]).
    pub fn at_ordinal(&self, n: usize) -> Pos {
        let mut left = n;
        let mut end = Pos::default();
        for (i, it) in self.items.iter().enumerate() {
            match it {
                Item::Text { s, .. } => {
                    for (b, c) in s.char_indices() {
                        if !c.is_whitespace() {
                            if left == 0 {
                                return Pos { item: i as u32, byte: b as u32 };
                            }
                            left -= 1;
                        }
                    }
                    if !s.is_empty() {
                        end = Pos { item: i as u32, byte: s.len() as u32 };
                    }
                }
                Item::Image { .. } => {
                    if left == 0 {
                        return Pos { item: i as u32, byte: 0 };
                    }
                    left -= 1;
                    end = Pos { item: i as u32, byte: 1 };
                }
                Item::Fill { .. } => {}
            }
        }
        end
    }
}

enum Atom {
    Word(String, Style, f32),
    Space(f32),
    Image(String, String, Style, f32, f32),
    Break,
    Anchor(String),
}

pub struct Ctx<'a> {
    g: &'a mut Gfx,
    em: f32,
    pal: Palette,
    text: Color,
    img: &'a dyn Fn(&str) -> (String, ImgState),
    out: Layout,
    marker: Option<(String, f32)>,
    list_depth: usize,
    cell_align: Option<Align>,
    /// Current structure node.
    node: u32,
    /// Between the previous line and the next item: a wrapped space.
    lead_space: bool,
    /// Between the previous line and the next item: hard line breaks.
    lead_breaks: u8,
}

fn font_of(s: &Style) -> FontId {
    match (s.mono, s.bold, s.italic) {
        (true, _, _) => FontId::Mono,
        (false, true, true) => FontId::BoldItalic,
        (false, true, false) => FontId::Bold,
        (false, false, true) => FontId::Italic,
        _ => FontId::Regular,
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x3FFFD)
}

impl Ctx<'_> {
    fn px(&self, s: &Style) -> f32 {
        self.em * s.size * if s.mono { 0.92 } else { 1.0 }
    }

    /// Opens a structure node; returns the node to go back to.
    fn enter(&mut self, tag: Tag) -> u32 {
        let saved = self.node;
        self.out.nodes.push(Node { parent: saved, tag });
        self.node = (self.out.nodes.len() - 1) as u32;
        saved
    }

    /// Adds a list marker.
    fn push_marker(&mut self, x: f32, y: f32, s: String, px: f32, w: f32) {
        let color = self.text;
        let i = self.out.push(Item::Text { x, y, s, px, font: FontId::Regular, color });
        self.out.meta[i] = Meta { node: self.node, marker: true, w, ..Meta::default() };
    }

    fn image_size(&mut self, src: &str, w: Option<Length>, h: Option<Length>, avail: f32) -> (String, Option<(f32, f32)>) {
        let (key, state) = (self.img)(src);
        if !self.out.images.contains(&key) {
            self.out.images.push(key.clone());
        }
        let scale = self.em / BASE_PX;
        let dim = |l: Option<Length>, total: f32| match l {
            Some(Length::Px(v)) => Some(v * scale),
            Some(Length::Percent(p)) => Some(total * p / 100.0),
            None => None,
        };
        let (aw, ah) = (dim(w, avail), dim(h, avail));
        let size = match (state, aw, ah) {
            (ImgState::Failed, _, _) => None,
            (_, Some(w), Some(h)) => Some((w, h)),
            (ImgState::Ready(iw, ih), w, h) => {
                let (iw, ih) = (iw.max(1) as f32 * scale, ih.max(1) as f32 * scale);
                Some(match (w, h) {
                    (Some(w), None) => (w, w * ih / iw),
                    (None, Some(h)) => (h * iw / ih, h),
                    _ => (iw, ih),
                })
            }
            (ImgState::Pending, Some(w), None) => Some((w, w * 0.6)),
            (ImgState::Pending, None, Some(h)) => Some((h, h)),
            (ImgState::Pending, None, None) => Some((self.em, self.em)),
        };
        // Never wider than the column.
        let size = size.map(|(w, h)| if w > avail && w > 0.0 { (avail, h * avail / w) } else { (w, h) });
        (key, size)
    }

    fn atoms(&mut self, inl: &[Inline], nowrap: bool, avail: f32) -> Vec<Atom> {
        let mut out = Vec::new();
        for i in inl {
            match i {
                Inline::Text(t, s) => {
                    let (px, font) = (self.px(s), font_of(s));
                    if nowrap {
                        for (k, line) in t.split('\n').enumerate() {
                            if k > 0 {
                                out.push(Atom::Break);
                            }
                            if !line.is_empty() {
                                let w = self.g.measure(line, px, font);
                                out.push(Atom::Word(line.to_string(), *s, w));
                            }
                        }
                        continue;
                    }
                    // Code spans get a little padding on both sides (an
                    // empty word of the same style, so the background
                    // covers it).
                    let code_pad = if s.code { px * 0.3 } else { 0.0 };
                    if s.code {
                        out.push(Atom::Word(String::new(), *s, code_pad));
                    }
                    let mut word = String::new();
                    let flush = |word: &mut String, out: &mut Vec<Atom>, g: &mut Gfx| {
                        if !word.is_empty() {
                            let w = g.measure(word, px, font);
                            out.push(Atom::Word(std::mem::take(word), *s, w));
                        }
                    };
                    for c in t.chars() {
                        if c == ' ' || c == '\u{200B}' {
                            flush(&mut word, &mut out, self.g);
                            if c == ' ' && !matches!(out.last(), Some(Atom::Space(_))) {
                                let w = self.g.advance(' ', px, font);
                                out.push(Atom::Space(w));
                            }
                        } else if is_cjk(c) {
                            flush(&mut word, &mut out, self.g);
                            word.push(c);
                            flush(&mut word, &mut out, self.g);
                        } else {
                            word.push(c);
                        }
                    }
                    flush(&mut word, &mut out, self.g);
                    if s.code {
                        out.push(Atom::Word(String::new(), *s, code_pad));
                    }
                }
                Inline::Break => out.push(Atom::Break),
                Inline::Anchor(a) => out.push(Atom::Anchor(a.clone())),
                Inline::Image { src, alt, width, height, style } => match self.image_size(src, *width, *height, avail) {
                    (key, Some((w, h))) => out.push(Atom::Image(key, alt.clone(), *style, w, h)),
                    (_, None) => {
                        if !alt.is_empty() {
                            let s = Style { italic: true, ..*style };
                            let t = format!("[{alt}]");
                            let w = self.g.measure(&t, self.px(&s), font_of(&s));
                            out.push(Atom::Word(t, s, w));
                        }
                    }
                },
            }
        }
        out
    }

    fn color(&self, s: &Style) -> Color {
        match s.color {
            Some(c) => self.pal.fg(c),
            None if s.link != 0 => self.pal.link,
            None if s.tok != 0 => crate::syntax::tok_from(s.tok - 1).and_then(|t| crate::syntax::color(t, self.pal.dark)).unwrap_or(self.text),
            None => self.text,
        }
    }

    /// Lays out inline content; returns the bottom y.
    fn para(&mut self, inl: &[Inline], x: f32, w: f32, y: f32, align: Align, nowrap: bool) -> f32 {
        let raw = self.atoms(inl, nowrap, w);
        // Words wider than the column are split into pieces that fit.
        let mut all: Vec<Atom> = Vec::with_capacity(raw.len());
        for a in raw {
            match a {
                Atom::Word(t, s, aw) if !nowrap && aw > w && w > 0.0 && t.chars().count() > 1 => {
                    let (px, font) = (self.px(&s), font_of(&s));
                    let mut chunk = String::new();
                    for c in t.chars() {
                        chunk.push(c);
                        if chunk.chars().count() > 1 && self.g.measure(&chunk, px, font) > w {
                            chunk.pop();
                            let piece = std::mem::take(&mut chunk);
                            let pw = self.g.measure(&piece, px, font);
                            all.push(Atom::Word(piece, s, pw));
                            chunk.push(c);
                        }
                    }
                    let pw = self.g.measure(&chunk, px, font);
                    all.push(Atom::Word(chunk, s, pw));
                }
                other => all.push(other),
            }
        }
        let first_style = inl.iter().find_map(|i| if let Inline::Text(_, s) = i { Some(*s) } else { None }).unwrap_or_default();
        let mut y = y;
        let mut line: Vec<(usize, f32)> = Vec::new();
        let mut cur = 0.0f32;
        let mut pending_space = 0.0f32;
        for (i, atom) in all.iter().enumerate() {
            match atom {
                Atom::Break => {
                    y = self.flush_line(&all, &mut line, x, w, y, align, first_style, true);
                    self.lead_breaks = self.lead_breaks.saturating_add(1);
                    self.lead_space = false;
                    cur = 0.0;
                    pending_space = 0.0;
                }
                Atom::Space(sw) => {
                    if !line.is_empty() {
                        pending_space = *sw;
                    }
                }
                Atom::Anchor(_) => line.push((i, cur)),
                Atom::Word(_, _, aw) | Atom::Image(_, _, _, aw, _) => {
                    if !nowrap && cur + pending_space + aw > w && !line.is_empty() {
                        y = self.flush_line(&all, &mut line, x, w, y, align, first_style, false);
                        self.lead_space = pending_space > 0.0;
                        cur = 0.0;
                        pending_space = 0.0;
                    }
                    cur += pending_space;
                    line.push((i, cur));
                    cur += aw;
                    pending_space = 0.0;
                }
            }
        }
        if !line.is_empty() {
            y = self.flush_line(&all, &mut line, x, w, y, align, first_style, false);
        } else if let Some((m, right)) = self.marker.take() {
            // A list item with no text: draw its marker anyway.
            let px = self.em;
            let mw = self.g.measure(&m, px, FontId::Regular);
            self.push_marker(right - mw, y + px, m, px, mw);
            y += px * 1.5;
        }
        self.lead_space = false;
        self.lead_breaks = 0;
        y
    }

    #[allow(clippy::too_many_arguments)]
    fn flush_line(&mut self, atoms: &[Atom], line: &mut Vec<(usize, f32)>, x: f32, w: f32, y: f32, align: Align, first: Style, forced: bool) -> f32 {
        // Line metrics.
        let mut asc: f32 = 0.0;
        let mut desc: f32 = 0.0;
        let mut width: f32 = 0.0;
        let mut has_content = false;
        for &(i, ax) in line.iter() {
            match &atoms[i] {
                Atom::Word(_, s, aw) => {
                    let px = self.px(s);
                    let lead = px * 1.45;
                    let shift = s.valign as f32 * px * 0.35;
                    asc = asc.max(lead * 0.72 + shift.max(0.0));
                    desc = desc.max(lead * 0.28 - shift.min(0.0));
                    width = width.max(ax + aw);
                    has_content = true;
                }
                Atom::Image(_, _, _, iw, ih) => {
                    asc = asc.max(*ih + 2.0);
                    desc = desc.max(self.em * 0.2);
                    width = width.max(ax + iw);
                    has_content = true;
                }
                _ => {}
            }
        }
        if !has_content {
            let lead = self.px(&first) * 1.45;
            asc = lead * 0.72;
            desc = lead * 0.28;
            if !forced && line.iter().all(|(i, _)| matches!(atoms[*i], Atom::Anchor(_))) {
                for &(i, _) in line.iter() {
                    if let Atom::Anchor(a) = &atoms[i] {
                        if !a.is_empty() {
                            self.out.anchors.entry(a.clone()).or_insert(y);
                        }
                    }
                }
                line.clear();
                return y;
            }
        }
        let baseline = y + asc;
        let align = match (self.cell_align, align) {
            (Some(a), Align::Left) => a,
            _ => align,
        };
        let off = match align {
            Align::Left => 0.0,
            Align::Center => ((w - width) / 2.0).max(0.0),
            Align::Right => (w - width).max(0.0),
        };
        if let Some((m, right)) = self.marker.take() {
            let px = self.px(&first);
            let mw = self.g.measure(&m, px, FontId::Regular);
            self.push_marker(right - mw, baseline, m, px, mw);
        }
        // Emit runs: consecutive words of the same style become one item.
        let mut k = 0;
        // End of the previous item on this line.
        let mut prev_end: Option<f32> = None;
        while k < line.len() {
            let (i, ax) = line[k];
            match &atoms[i] {
                Atom::Anchor(a) => {
                    if !a.is_empty() {
                        self.out.anchors.entry(a.clone()).or_insert(y);
                    }
                    k += 1;
                }
                Atom::Image(key, alt, s, iw, ih) => {
                    let r = Rect::new(x + off + ax, baseline - ih, *iw, *ih);
                    let idx = self.out.push(Item::Image { r, key: key.clone() });
                    let wrapped = std::mem::take(&mut self.lead_space);
                    self.out.meta[idx] = Meta {
                        node: self.node,
                        style: *s,
                        w: *iw,
                        wrapped,
                        space: wrapped || prev_end.is_some_and(|e| ax - e > 0.01),
                        breaks: std::mem::take(&mut self.lead_breaks),
                        alt: (!alt.is_empty()).then(|| alt.as_str().into()),
                        marker: false,
                    };
                    prev_end = Some(ax + iw);
                    if s.link != 0 {
                        self.out.links.push((r, s.link));
                    }
                    k += 1;
                }
                Atom::Word(_, s, _) => {
                    let s = *s;
                    let mut text = String::new();
                    let start_x = ax;
                    let mut end_x = ax;
                    let mut j = k;
                    while j < line.len() {
                        let (ii, jx) = line[j];
                        match &atoms[ii] {
                            Atom::Word(t, st, aw) if *st == s => {
                                // Re-insert the collapsed spaces between words.
                                if !text.is_empty() {
                                    let gap = jx - end_x;
                                    let sw = self.g.advance(' ', self.px(&s), font_of(&s)).max(0.01);
                                    let n = (gap / sw).round().max(0.0) as usize;
                                    text.extend(std::iter::repeat_n(' ', n));
                                }
                                text.push_str(t);
                                end_x = jx + aw;
                                j += 1;
                            }
                            _ => break,
                        }
                    }
                    let px = self.px(&s);
                    let font = font_of(&s);
                    let rx = x + off + start_x;
                    let rw = end_x - start_x;
                    let shift = s.valign as f32 * px * 0.35;
                    let base = baseline - shift;
                    if s.bg.is_some() || s.code {
                        let bg = s.bg.map_or(self.pal.code_bg, |c| self.pal.bgc(c));
                        let pad = (px * 0.18).round();
                        self.out.push(Item::Fill { r: Rect::new(rx - pad, base - px * 0.95, rw + 2.0 * pad, px * 1.25), color: bg });
                    }
                    let color = self.color(&s);
                    let idx = self.out.push(Item::Text { x: rx, y: base, s: text, px, font, color });
                    let wrapped = std::mem::take(&mut self.lead_space);
                    self.out.meta[idx] = Meta {
                        node: self.node,
                        style: s,
                        w: rw,
                        wrapped,
                        space: wrapped || prev_end.is_some_and(|e| start_x - e > 0.01),
                        breaks: std::mem::take(&mut self.lead_breaks),
                        ..Meta::default()
                    };
                    prev_end = Some(end_x);
                    let thick = (px / 14.0).max(1.0).round();
                    if s.underline {
                        self.out.push(Item::Fill { r: Rect::new(rx, (base + px * 0.12).round(), rw, thick), color });
                    }
                    if s.strike {
                        self.out.push(Item::Fill { r: Rect::new(rx, (base - px * 0.3).round(), rw, thick), color });
                    }
                    if s.link != 0 {
                        self.out.links.push((Rect::new(rx, base - px, rw, px * 1.3), s.link));
                    }
                    k = j;
                }
                _ => k += 1,
            }
        }
        line.clear();
        self.out.width = self.out.width.max(x + off + width);
        baseline + desc
    }

    /// Lays out a sequence of blocks; returns the bottom y (margins collapsed
    /// with the following content are left to the caller).
    fn blocks(&mut self, blocks: &[Block], x: f32, w: f32, y: f32) -> f32 {
        let mut y = y;
        let mut pending: f32 = 0.0;
        let mut first = true;
        for b in blocks {
            if b.is_empty() && b.anchor.is_none() {
                continue;
            }
            let top = if first { 0.0 } else { pending.max(b.margin_top * self.em) };
            y += top;
            first = false;
            y = self.block(b, x, w, y);
            pending = b.margin_bottom * self.em;
        }
        y
    }

    fn block(&mut self, b: &Block, x: f32, w: f32, y: f32) -> f32 {
        if let Some(a) = &b.anchor {
            self.out.anchors.entry(a.clone()).or_insert(y);
        }
        let em = self.em;
        let indent = b.indent * em;
        let (x, w) = (x + indent, (w - indent).max(1.0));
        let pre = matches!(b.kind, Kind::Pre(_));
        let bg = b.bg.map(|c| self.pal.bgc(c)).or(if pre && b.padding > 0.0 { Some(self.pal.code_bg) } else { None });
        let pad = if bg.is_some() { b.padding.max(if pre { 0.8 } else { 0.5 }) * em } else { 0.0 };
        let bg_item = bg.map(|c| self.out.push(Item::Fill { r: Rect::default(), color: c }));
        let bar = if b.bar { (em * 0.25).max(3.0).round() } else { 0.0 };
        let cx = x + pad + if b.bar { bar + em * 0.8 } else { 0.0 };
        let cw = (w - 2.0 * pad - (cx - x - pad)).max(1.0);
        let saved_text = self.text;
        if b.bar {
            self.text = self.pal.muted;
        }
        let saved_node = self.node;
        if b.bar {
            self.enter(Tag::Quote);
        }
        let para_tag = |pre: bool| Tag::Para {
            align: b.align,
            pre,
            heading: b.heading,
            // The document's own colors: copies land on a light page.
            bg: b.bg.or(if pre && b.padding > 0.0 { Some(Palette::paper().code_bg) } else { None }),
        };
        let top = y;
        let mut y = y + pad;
        y = match &b.kind {
            Kind::Empty => y,
            Kind::Para(inl) => {
                self.enter(para_tag(false));
                self.para(inl, cx, cw, y, b.align, false)
            }
            Kind::Pre(inl) => {
                self.enter(para_tag(true));
                self.para(inl, cx, cw, y, b.align, true)
            }
            Kind::Group(children) => self.blocks(children, cx, cw, y),
            Kind::Rule => {
                let h = (em / 10.0).max(1.0).round();
                let r = Rect::new(cx, y + em * 0.4, cw, h);
                self.out.push(Item::Fill { r, color: self.pal.border });
                y + em * 0.8 + h
            }
            Kind::List(l) => {
                self.list_depth += 1;
                self.enter(Tag::List { ordered: l.ordered, start: l.start });
                let ind = em * 1.8;
                let mut yy = y;
                for (i, item) in l.items.iter().enumerate() {
                    let marker = match item.checked {
                        Some(true) => "☑".to_string(),
                        Some(false) => "☐".to_string(),
                        None if l.ordered => format!("{}.", l.start as usize + i),
                        None => ["•", "◦", "▪"][(self.list_depth - 1) % 3].to_string(),
                    };
                    if i > 0 {
                        yy += em * 0.2;
                    }
                    self.marker = Some((marker, cx + ind - em * 0.45));
                    let list_node = self.enter(Tag::Item);
                    yy = self.blocks(&item.blocks, cx + ind, cw - ind, yy);
                    if let Some((m, right)) = self.marker.take() {
                        let mw = self.g.measure(&m, em, FontId::Regular);
                        self.push_marker(right - mw, yy + em, m, em, mw);
                        yy += em * 1.45;
                    }
                    self.node = list_node;
                }
                self.list_depth -= 1;
                yy
            }
            Kind::Table(t) => {
                self.enter(Tag::Table);
                self.table(t, cx, cw, y)
            }
        };
        self.node = saved_node;
        y += pad;
        if b.bar {
            self.out.push(Item::Fill { r: Rect::new(x, top, bar, y - top), color: self.pal.border });
            self.text = saved_text;
        }
        if let Some(idx) = bg_item {
            self.out.patch(idx, Rect::new(x, top, w, y - top));
            self.out.width = self.out.width.max(x + w);
        }
        if b.underline {
            let h = (em / 14.0).max(1.0).round();
            y += em * 0.3;
            self.out.push(Item::Fill { r: Rect::new(x, y, w, h), color: self.pal.border });
            y += h;
        }
        y
    }

    // ------------------------------------------------------------ tables

    /// (minimum, preferred) width of inline content.
    fn measure_inlines(&mut self, inl: &[Inline], nowrap: bool) -> (f32, f32) {
        let atoms = self.atoms(inl, nowrap, f32::MAX);
        let (mut min, mut max, mut line) = (0.0f32, 0.0f32, 0.0f32);
        for a in &atoms {
            match a {
                Atom::Word(_, _, w) | Atom::Image(_, _, _, w, _) => {
                    min = min.max(*w);
                    line += w;
                }
                Atom::Space(w) => line += w,
                Atom::Break => {
                    max = max.max(line);
                    line = 0.0;
                }
                Atom::Anchor(_) => {}
            }
        }
        max = max.max(line);
        if nowrap {
            min = max;
        }
        (min, max)
    }

    fn measure_blocks(&mut self, blocks: &[Block]) -> (f32, f32) {
        let mut r = (0.0f32, 0.0f32);
        for b in blocks {
            let extra = b.indent * self.em + if b.bg.is_some() { 2.0 * b.padding.max(0.5) * self.em } else { 0.0 } + if b.bar { self.em * 1.2 } else { 0.0 };
            let (mi, ma) = match &b.kind {
                Kind::Para(v) => self.measure_inlines(v, false),
                Kind::Pre(v) => self.measure_inlines(v, true),
                Kind::Group(g) => self.measure_blocks(g),
                Kind::List(l) => {
                    let mut m = (0.0f32, 0.0f32);
                    for it in &l.items {
                        let x = self.measure_blocks(&it.blocks);
                        m = (m.0.max(x.0), m.1.max(x.1));
                    }
                    (m.0 + self.em * 1.8, m.1 + self.em * 1.8)
                }
                Kind::Table(t) => {
                    let mut m = (0.0f32, 0.0f32);
                    for row in &t.rows {
                        let mut rm = (0.0, 0.0);
                        for c in &row.cells {
                            let x = self.measure_blocks(&c.blocks);
                            rm = (rm.0 + x.0 + self.em, rm.1 + x.1 + self.em);
                        }
                        m = (m.0.max(rm.0), m.1.max(rm.1));
                    }
                    m
                }
                Kind::Rule | Kind::Empty => (0.0, 0.0),
            };
            r = (r.0.max(mi + extra), r.1.max(ma + extra));
        }
        r
    }

    fn table(&mut self, t: &Table, x: f32, w: f32, y: f32) -> f32 {
        let em = self.em;
        let ncols = t.rows.iter().map(|r| r.cells.iter().map(|c| c.colspan.max(1)).sum::<usize>()).max().unwrap_or(0);
        if ncols == 0 {
            return y;
        }
        let (padx, pady) = ((em * 0.6).round(), (em * 0.35).round());
        let mut mins = vec![0.0f32; ncols];
        let mut maxs = vec![0.0f32; ncols];
        let mut spans = Vec::new();
        for row in &t.rows {
            let mut col = 0;
            for c in &row.cells {
                let span = c.colspan.max(1).min(ncols - col.min(ncols - 1));
                if col >= ncols {
                    break;
                }
                let (mi, ma) = self.measure_blocks(&c.blocks);
                if span == 1 {
                    mins[col] = mins[col].max(mi);
                    maxs[col] = maxs[col].max(ma);
                } else {
                    spans.push((col, span, mi, ma));
                }
                col += span;
            }
        }
        for (col, span, mi, ma) in spans {
            let have_min: f32 = mins[col..col + span].iter().sum();
            let have_max: f32 = maxs[col..col + span].iter().sum();
            for k in col..col + span {
                if mi > have_min {
                    mins[k] += (mi - have_min) / span as f32;
                }
                if ma > have_max {
                    maxs[k] += (ma - have_max) / span as f32;
                }
            }
        }
        let pad2 = 2.0 * padx;
        let total_max: f32 = maxs.iter().map(|m| m + pad2).sum();
        let total_min: f32 = mins.iter().map(|m| m + pad2).sum();
        let widths: Vec<f32> = if total_max <= w {
            maxs.iter().map(|m| (m + pad2).ceil()).collect()
        } else if total_min >= w {
            mins.iter().map(|m| (m + pad2).ceil()).collect()
        } else {
            let extra = w - total_min;
            let flex: f32 = maxs.iter().zip(&mins).map(|(a, b)| a - b).sum::<f32>().max(0.001);
            mins.iter().zip(&maxs).map(|(mi, ma)| (mi + pad2 + extra * (ma - mi) / flex).floor()).collect()
        };
        let line = (em / 14.0).max(1.0).round();
        let mut y = y;
        for (ri, row) in t.rows.iter().enumerate() {
            let table_node = self.enter(Tag::Row);
            let top = y;
            let mut cx = x;
            let mut bottom = top;
            let mut cells = Vec::new();
            let mut col = 0;
            for c in &row.cells {
                if col >= ncols {
                    break;
                }
                let span = c.colspan.max(1).min(ncols - col);
                let cw: f32 = widths[col..col + span].iter().sum();
                let zebra = ri % 2 == 0 && ri > 0 && !c.header;
                let bg = c.bg.map(|x| self.pal.bgc(x)).or(if c.header { Some(self.pal.header_bg) } else if zebra { Some(self.pal.header_bg.mix(self.pal.bg, 0.5)) } else { None });
                let bg_item = bg.map(|color| self.out.push(Item::Fill { r: Rect::default(), color }));
                self.cell_align = Some(c.align);
                let row_node = self.enter(Tag::Cell { header: c.header, colspan: c.colspan.max(1), align: c.align, bg: c.bg });
                let b = self.blocks(&c.blocks, cx + padx, cw - pad2, top + pady);
                self.node = row_node;
                self.cell_align = None;
                bottom = bottom.max(b + pady);
                cells.push((cx, cw, bg_item));
                cx += cw;
                col += span;
            }
            let h = (bottom - top).max(em);
            for (cx, cw, bg_item) in cells {
                if let Some(i) = bg_item {
                    self.out.patch(i, Rect::new(cx, top, cw, h));
                }
                // Cell borders.
                let c = self.pal.border;
                self.out.push(Item::Fill { r: Rect::new(cx, top, cw, line), color: c });
                self.out.push(Item::Fill { r: Rect::new(cx, top, line, h), color: c });
                self.out.push(Item::Fill { r: Rect::new(cx + cw - line, top, line, h), color: c });
                self.out.push(Item::Fill { r: Rect::new(cx, top + h - line, cw, line), color: c });
            }
            y = top + h;
            self.node = table_node;
        }
        let tw: f32 = widths.iter().sum();
        self.out.width = self.out.width.max(x + tw);
        y
    }
}

/// Lays out `doc` for a content `width` at `em` pixels per base em.
pub fn layout(doc: &RichDoc, g: &mut Gfx, width: f32, em: f32, pal: Palette, img: &dyn Fn(&str) -> (String, ImgState)) -> Layout {
    let pad = (em * 2.0).round();
    let col = doc.max_width.map_or(width - 2.0 * pad, |m| (m * em).min(width - 2.0 * pad)).max(em * 8.0);
    let x = ((width - col) / 2.0).max(pad).round();
    let text = doc.page_color.map_or(pal.text, |c| pal.fg(c));
    let mut ctx = Ctx { g, em, pal, text, img, out: Layout::default(), marker: None, list_depth: 0, cell_align: None, node: 0, lead_space: false, lead_breaks: 0 };
    ctx.out.nodes.push(Node { parent: 0, tag: Tag::Quote }); // the root; never written out
    let bottom = ctx.blocks(&doc.blocks, x, col, pad);
    let mut out = ctx.out;
    out.height = bottom + pad;
    out.width = out.width.max(x + col) + pad;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich::markdown;

    fn texts(l: &Layout) -> Vec<String> {
        l.items.iter().filter_map(|i| if let Item::Text { s, .. } = i { Some(s.clone()) } else { None }).collect()
    }

    #[test]
    fn wraps_and_measures() {
        let Ok(mut g) = Gfx::new() else { return };
        let doc = markdown::parse("# Heading\n\nThe quick brown fox jumps over the lazy dog again and again and again.\n\n- one\n- two\n\n| a | bb |\n|---|---|\n| 1 | 2 |\n");
        let no_img = |_: &str| (String::new(), ImgState::Failed);
        let wide = layout(&doc, &mut g, 2000.0, 15.0, Palette::paper(), &no_img);
        let narrow = layout(&doc, &mut g, 300.0, 15.0, Palette::paper(), &no_img);
        assert!(narrow.height > wide.height, "narrow layout wraps");
        let t = texts(&wide);
        assert!(t.iter().any(|s| s.contains("quick brown fox jumps")), "{t:?}");
        assert!(t.contains(&"•".to_string()) && t.contains(&"one".to_string()));
        assert!(wide.anchors.contains_key("heading"));
        // Zoom scales the layout.
        let big = layout(&doc, &mut g, 2000.0, 30.0, Palette::paper(), &no_img);
        assert!(big.height > wide.height * 1.6);
    }

    #[test]
    fn dark_adaptation() {
        assert_eq!(invert_lightness(Color::rgb(0xFFFFFF)), Color::rgb(0x1E1E1E));
        assert_eq!(invert_lightness(Color::rgb(0x000000)), Color::rgb(0xFFFFFF));
        let mut p = Palette::dark();
        p.adapt = true;
        // Dark text becomes light, light backgrounds become dark; hue kept.
        assert!(luminance(p.fg(Color::rgb(0x222222))) > 180.0);
        assert!(luminance(p.bgc(Color::rgb(0xFFF8D6))) < 80.0);
        let red = p.fg(Color::rgb(0x800000));
        assert!(red.r > red.g && red.r > red.b, "{red:?}");
        // Already suitable colors stay.
        assert_eq!(p.fg(Color::rgb(0xEEEEEE)), Color::rgb(0xEEEEEE));
        assert_eq!(p.bgc(Color::rgb(0x202020)), Color::rgb(0x202020));
        assert_eq!(Palette::paper().fg(Color::rgb(0x222222)), Color::rgb(0x222222));
    }

    #[test]
    fn long_words_are_split() {
        let Ok(mut g) = Gfx::new() else { return };
        let doc = markdown::parse(&"x".repeat(400));
        let l = layout(&doc, &mut g, 400.0, 15.0, Palette::paper(), &|_| (String::new(), ImgState::Failed));
        assert!(l.width <= 400.0 + 1.0, "width {}", l.width);
        assert!(texts(&l).len() > 2);
    }

    #[test]
    fn links_are_hit_testable() {
        let Ok(mut g) = Gfx::new() else { return };
        let doc = markdown::parse("Go [here](#x) now.");
        let l = layout(&doc, &mut g, 800.0, 15.0, Palette::paper(), &|_| (String::new(), ImgState::Failed));
        let (r, n) = l.links[0];
        assert_eq!(n, 1);
        assert_eq!(l.link_at(r.x + 1.0, r.y + r.h / 2.0), Some(1));
    }
}
