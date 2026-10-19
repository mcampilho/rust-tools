//! Offline Markdown preview: parses with pulldown-cmark and lays the result
//! out with the built-in renderer. Nothing is fetched: images are shown as
//! their alt text and links are displayed, not followed.

use super::gfx::{Color, FontId, Gfx, Rect};
use super::theme::Theme;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Text,
    Code,
    Link,
    Heading,
    Quote,
}

#[derive(Clone, Debug)]
struct Run {
    x: f32,
    text: String,
    font: FontId,
    size: f32,
    kind: Kind,
}

#[derive(Clone, Debug, Default)]
struct Line {
    y: f32,
    h: f32,
    runs: Vec<Run>,
    code_block: bool,
    quote_depth: usize,
    rule: bool,
    left: f32,
}

#[derive(Default)]
pub struct Preview {
    pub scroll: f32,
    key: Option<(u64, u64, u32, u32)>,
    lines: Vec<Line>,
    pub height: f32,
}

/// Documents larger than this are not previewed (keeps typing responsive).
pub const MAX_PREVIEW: usize = 2 * 1024 * 1024;

struct Builder<'a> {
    g: &'a mut Gfx,
    width: f32,
    base: f32,
    lines: Vec<Line>,
    cur: Line,
    x: f32,
    y: f32,
    indent: f32,
    font: FontId,
    size: f32,
    kind: Kind,
    quote: usize,
    space_after: f32,
}

impl Builder<'_> {
    fn line_h(&self, size: f32) -> f32 {
        (size * 1.45).round()
    }

    fn newline(&mut self) {
        let h = self.cur.runs.iter().map(|r| self.line_h(r.size)).fold(self.line_h(self.base), f32::max);
        self.cur.y = self.y;
        self.cur.h = h;
        self.cur.quote_depth = self.quote;
        self.cur.left = self.indent;
        self.y += h;
        let l = std::mem::take(&mut self.cur);
        self.lines.push(l);
        self.x = self.indent;
    }

    fn flush_block(&mut self, gap: f32) {
        if !self.cur.runs.is_empty() || self.cur.code_block {
            self.newline();
        }
        self.y += gap;
        self.x = self.indent;
    }

    fn push_word(&mut self, word: &str) {
        let w = self.g.measure(word, self.size, self.font);
        if self.x + w > self.width && self.x > self.indent && !word.trim().is_empty() {
            self.newline();
        }
        if self.x == self.indent && word.trim().is_empty() {
            return;
        }
        self.cur.runs.push(Run { x: self.x, text: word.to_string(), font: self.font, size: self.size, kind: self.kind });
        self.x += w;
    }

    fn text(&mut self, t: &str) {
        let mut word = String::new();
        for c in t.chars() {
            word.push(c);
            if c == ' ' {
                let w = std::mem::take(&mut word);
                self.push_word(&w);
            }
        }
        if !word.is_empty() {
            self.push_word(&word);
        }
    }
}

impl Preview {
    /// Lays out `text` for `width` pixels (cached by document version).
    pub fn layout(&mut self, g: &mut Gfx, text: &str, key: (u64, u64), width: f32, base: f32) {
        let k = (key.0, key.1, width as u32, base as u32);
        if self.key == Some(k) {
            return;
        }
        self.key = Some(k);
        let mut b = Builder {
            g,
            width,
            base,
            lines: Vec::new(),
            cur: Line::default(),
            x: 0.0,
            y: 0.0,
            indent: 0.0,
            font: FontId::Regular,
            size: base,
            kind: Kind::Text,
            quote: 0,
            space_after: 0.0,
        };
        let mut list_stack: Vec<Option<u64>> = Vec::new();
        let mut in_code = false;
        let mut emph = 0;
        let mut strong = 0;
        let mut table_row: Vec<String> = Vec::new();
        let mut cell = String::new();
        let mut in_table = false;
        let set_font = |b: &mut Builder, emph: i32, strong: i32| {
            if b.kind == Kind::Code {
                return;
            }
            b.font = if strong > 0 || b.kind == Kind::Heading {
                FontId::Semibold
            } else if emph > 0 {
                FontId::Italic
            } else {
                FontId::Regular
            };
        };
        let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
        for ev in Parser::new_ext(text, opts) {
            match ev {
                Event::Start(Tag::Heading { level, .. }) => {
                    b.flush_block(0.0);
                    b.y += base * 0.4;
                    b.size = base
                        * match level {
                            HeadingLevel::H1 => 1.9,
                            HeadingLevel::H2 => 1.5,
                            HeadingLevel::H3 => 1.25,
                            HeadingLevel::H4 => 1.1,
                            _ => 1.0,
                        };
                    b.kind = Kind::Heading;
                    b.font = FontId::Semibold;
                }
                Event::End(TagEnd::Heading(_)) => {
                    b.flush_block(base * 0.5);
                    b.size = base;
                    b.kind = Kind::Text;
                    set_font(&mut b, emph, strong);
                }
                Event::Start(Tag::Paragraph) => {}
                Event::End(TagEnd::Paragraph) => {
                    let gap = if list_stack.is_empty() { base * 0.7 } else { base * 0.2 };
                    b.flush_block(gap);
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    b.flush_block(0.0);
                    b.quote += 1;
                    b.indent += base * 1.2;
                    b.x = b.indent;
                    b.kind = Kind::Quote;
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    b.flush_block(0.0);
                    b.quote -= 1;
                    b.indent -= base * 1.2;
                    b.x = b.indent;
                    b.kind = Kind::Text;
                }
                Event::Start(Tag::CodeBlock(_)) => {
                    b.flush_block(0.0);
                    in_code = true;
                    b.kind = Kind::Code;
                    b.font = FontId::Mono;
                    b.size = base * 0.92;
                }
                Event::End(TagEnd::CodeBlock) => {
                    in_code = false;
                    b.kind = Kind::Text;
                    b.size = base;
                    set_font(&mut b, emph, strong);
                    b.y += base * 0.7;
                }
                Event::Start(Tag::List(start)) => {
                    if list_stack.is_empty() {
                        b.flush_block(0.0);
                    } else if !b.cur.runs.is_empty() {
                        b.newline();
                    }
                    list_stack.push(start);
                    b.indent += base * 1.5;
                }
                Event::End(TagEnd::List(_)) => {
                    list_stack.pop();
                    b.indent -= base * 1.5;
                    if list_stack.is_empty() {
                        b.flush_block(base * 0.5);
                    }
                }
                Event::Start(Tag::Item) => {
                    if !b.cur.runs.is_empty() {
                        b.newline();
                    }
                    b.x = b.indent;
                    let marker = match list_stack.last_mut() {
                        Some(Some(n)) => {
                            let m = format!("{n}.");
                            *n += 1;
                            m
                        }
                        _ => "•".to_string(),
                    };
                    let w = b.g.measure(&marker, base, FontId::Regular);
                    b.cur.runs.push(Run { x: b.indent - w - base * 0.4, text: marker, font: FontId::Regular, size: base, kind: Kind::Text });
                }
                Event::End(TagEnd::Item) => {
                    if !b.cur.runs.is_empty() {
                        b.newline();
                    }
                }
                Event::TaskListMarker(done) => b.text(if done { "☑ " } else { "☐ " }),
                Event::Start(Tag::Emphasis) => {
                    emph += 1;
                    set_font(&mut b, emph, strong);
                }
                Event::End(TagEnd::Emphasis) => {
                    emph -= 1;
                    set_font(&mut b, emph, strong);
                }
                Event::Start(Tag::Strong) => {
                    strong += 1;
                    set_font(&mut b, emph, strong);
                }
                Event::End(TagEnd::Strong) => {
                    strong -= 1;
                    set_font(&mut b, emph, strong);
                }
                Event::Start(Tag::Link { .. }) => b.kind = Kind::Link,
                Event::End(TagEnd::Link) => b.kind = if b.quote > 0 { Kind::Quote } else { Kind::Text },
                Event::Start(Tag::Image { .. }) => b.text(&format!("[{}: ", crate::i18n::tr("image"))),
                Event::End(TagEnd::Image) => b.text("]"),
                Event::Start(Tag::Table(_)) => {
                    b.flush_block(0.0);
                    in_table = true;
                }
                Event::End(TagEnd::Table) => {
                    in_table = false;
                    b.flush_block(base * 0.7);
                }
                Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => table_row.clear(),
                Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => {
                    let line = format!("| {} |", table_row.join(" | "));
                    let (pf, ps, pk) = (b.font, b.size, b.kind);
                    b.font = FontId::Mono;
                    b.size = base * 0.92;
                    b.kind = Kind::Code;
                    b.cur.code_block = false;
                    b.push_word(&line);
                    b.newline();
                    b.font = pf;
                    b.size = ps;
                    b.kind = pk;
                }
                Event::Start(Tag::TableCell) => cell.clear(),
                Event::End(TagEnd::TableCell) => table_row.push(std::mem::take(&mut cell)),
                Event::Text(t) => {
                    if in_table {
                        cell.push_str(&t);
                    } else if in_code {
                        for (i, l) in t.split('\n').enumerate() {
                            if i > 0 {
                                b.cur.code_block = true;
                                b.newline();
                            }
                            if !l.is_empty() {
                                b.cur.code_block = true;
                                let x = b.x;
                                b.cur.runs.push(Run { x: x + base * 0.5, text: l.replace('\t', "    "), font: FontId::Mono, size: b.size, kind: Kind::Code });
                            }
                        }
                    } else {
                        b.text(&t);
                    }
                }
                Event::Code(t) => {
                    if in_table {
                        cell.push_str(&t);
                    } else {
                        let (pf, pk) = (b.font, b.kind);
                        b.font = FontId::Mono;
                        b.kind = Kind::Code;
                        b.text(&t);
                        b.font = pf;
                        b.kind = pk;
                    }
                }
                Event::Html(t) | Event::InlineHtml(t) => {
                    let (pf, pk) = (b.font, b.kind);
                    b.font = FontId::Mono;
                    b.kind = Kind::Code;
                    b.text(t.trim_end());
                    b.font = pf;
                    b.kind = pk;
                    if t.ends_with('\n') {
                        b.newline();
                    }
                }
                Event::SoftBreak => b.text(" "),
                Event::HardBreak => b.newline(),
                Event::Rule => {
                    b.flush_block(0.0);
                    b.cur.rule = true;
                    b.newline();
                    b.y += base * 0.5;
                }
                _ => {}
            }
        }
        b.flush_block(0.0);
        let _ = b.space_after;
        self.height = b.y;
        self.lines = b.lines;
    }

    pub fn draw(&self, g: &mut Gfx, t: &Theme, r: Rect, pad: f32) {
        let y0 = r.y + pad - self.scroll;
        for l in &self.lines {
            let y = y0 + l.y;
            if y + l.h < r.y || y > r.bottom() {
                continue;
            }
            if l.rule {
                g.fill_rect(Rect::new(r.x + pad, (y + l.h / 2.0).round(), r.w - 2.0 * pad, 1.0), t.border);
                continue;
            }
            if l.code_block {
                g.fill_rect(Rect::new(r.x + pad + l.left, y, r.w - 2.0 * pad - l.left, l.h), if t.dark { Color::rgba(0xFFFFFF, 0x0C) } else { Color::rgba(0x000000, 0x08) });
            }
            for q in 0..l.quote_depth {
                let x = r.x + pad + q as f32 * 1.2 * 14.0 * (l.h / (14.0 * 1.45)).max(0.5);
                g.fill_rect(Rect::new(x.round(), y, 3.0, l.h), t.accent.with_alpha(0x80));
            }
            for run in &l.runs {
                let (asc, desc) = g.metrics(run.size, run.font);
                let base = (y + l.h / 2.0 + (asc + desc) / 2.0).round();
                let color = match run.kind {
                    Kind::Code => t.tok(crate::syntax::Tok::Code),
                    Kind::Link => t.tok(crate::syntax::Tok::Link),
                    Kind::Heading => t.text,
                    Kind::Quote => t.text_dim,
                    Kind::Text => t.text,
                };
                let x = r.x + pad + run.x;
                if run.kind == Kind::Code && !l.code_block {
                    let w = g.measure(&run.text, run.size, run.font);
                    g.fill_rrect(Rect::new(x - 1.0, y + 2.0, w + 2.0, l.h - 4.0), 3.0, if t.dark { Color::rgba(0xFFFFFF, 0x12) } else { Color::rgba(0x000000, 0x0A) });
                }
                let w = g.text(&run.text, x, base, run.size, run.font, color);
                if run.kind == Kind::Link {
                    g.fill_rect(Rect::new(x, base + 2.0, w, 1.0), color);
                }
            }
        }
    }
}
