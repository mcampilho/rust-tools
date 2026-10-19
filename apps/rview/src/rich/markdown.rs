//! Markdown → rich document, via pulldown-cmark (CommonMark + GitHub
//! tables, task lists, strikethrough and footnotes). HTML embedded in
//! Markdown is handed to the HTML parser.

use super::{html, Align, Block, Inline, Kind, List, ListItem, RichDoc, Row, Style, Table};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;

pub const HEADING_SIZES: [f32; 6] = [2.0, 1.5, 1.25, 1.0, 0.875, 0.85];

enum Frame {
    Root(Vec<Block>),
    Quote(Vec<Block>),
    Footnote(String, Vec<Block>),
    List(List),
    Table { table: Table, aligns: Vec<Alignment>, head: bool },
}

struct Builder {
    stack: Vec<Frame>,
    inlines: Vec<Inline>,
    styles: Vec<Style>,
    links: Vec<String>,
    heading: Option<(usize, Option<String>)>,
    code: Option<String>,
    code_lang: crate::syntax::Lang,
    image: Option<(String, String)>,
    html: String,
    html_tags: Vec<String>,
    slugs: HashMap<String, usize>,
}

/// GitHub-style heading anchor: lowercase, punctuation dropped, spaces to `-`.
pub fn slug(text: &str) -> String {
    text.trim()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c.to_lowercase().next().unwrap_or(c))
            } else if c == ' ' {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

impl Builder {
    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, f: impl FnOnce(&mut Style)) {
        let mut s = self.style();
        f(&mut s);
        self.styles.push(s);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn push_block(&mut self, b: Block) {
        match self.stack.last_mut() {
            Some(Frame::Root(v) | Frame::Quote(v) | Frame::Footnote(_, v)) => v.push(b),
            Some(Frame::List(l)) => {
                if l.items.is_empty() {
                    l.items.push(ListItem::default());
                }
                l.items.last_mut().expect("item").blocks.push(b);
            }
            Some(Frame::Table { .. }) | None => {}
        }
    }

    /// Ends the current run of inline content as a paragraph.
    fn flush(&mut self) {
        self.flush_html();
        if self.inlines.is_empty() {
            return;
        }
        let inlines = std::mem::take(&mut self.inlines);
        if inlines.iter().all(|i| matches!(i, Inline::Text(t, _) if t.trim().is_empty())) {
            return;
        }
        self.push_block(Block::para(inlines));
    }

    fn flush_html(&mut self) {
        if self.html.is_empty() {
            return;
        }
        let src = std::mem::take(&mut self.html);
        for b in html::fragment(&src, &mut self.links) {
            self.push_block(b);
        }
    }

    fn text(&mut self, t: &str) {
        if let Some(code) = &mut self.code {
            code.push_str(t);
        } else if let Some((_, alt)) = &mut self.image {
            alt.push_str(t);
        } else {
            let s = self.style();
            self.inlines.push(Inline::Text(t.to_string(), s));
        }
    }

    fn inline_html(&mut self, raw: &str) {
        let t = raw.trim();
        let Some(inner) = t.strip_prefix('<').and_then(|t| t.strip_suffix('>')) else {
            return self.text(raw);
        };
        let closing = inner.starts_with('/');
        let inner = inner.trim_start_matches('/').trim_end_matches('/');
        let name: String = inner.chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        if closing {
            if let Some(pos) = self.html_tags.iter().rposition(|t| *t == name) {
                self.html_tags.truncate(pos);
                if name != "br" && self.styles.len() > 1 {
                    self.pop_style();
                }
            }
            return;
        }
        match name.as_str() {
            "br" => self.inlines.push(Inline::Break),
            "img" => {
                let attrs = html::parse_attrs(&inner[3..]);
                let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
                let src = get("src").unwrap_or_default();
                let style = self.style();
                self.inlines.push(Inline::Image {
                    src,
                    alt: get("alt").unwrap_or_default(),
                    width: get("width").and_then(|v| html::length(&v)),
                    height: get("height").and_then(|v| html::length(&v)),
                    style,
                });
            }
            "b" | "strong" | "i" | "em" | "u" | "ins" | "s" | "del" | "strike" | "code" | "kbd" | "sub" | "sup" | "mark" | "small" | "a" => {
                let link = if name == "a" {
                    let attrs = html::parse_attrs(&inner[1..]);
                    attrs.iter().find(|(n, _)| n == "href").map(|(_, v)| {
                        self.links.push(v.clone());
                        self.links.len() as u32
                    })
                } else {
                    None
                };
                self.push_style(|s| match name.as_str() {
                    "b" | "strong" => s.bold = true,
                    "i" | "em" => s.italic = true,
                    "u" | "ins" => s.underline = true,
                    "s" | "del" | "strike" => s.strike = true,
                    "code" | "kbd" => {
                        s.mono = true;
                        s.code = true;
                    }
                    "sub" => {
                        s.valign = -1;
                        s.size *= 0.8;
                    }
                    "sup" => {
                        s.valign = 1;
                        s.size *= 0.8;
                    }
                    "mark" => s.bg = Some(crate::ui::gfx::Color::rgb(0xFFF3A3)),
                    "small" => s.size *= 0.85,
                    _ => {
                        if let Some(l) = link {
                            s.link = l;
                        }
                    }
                });
                self.html_tags.push(name);
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => self.flush(),
            Tag::Heading { level, id, .. } => {
                self.flush();
                let n = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                self.heading = Some((n, id.map(|s| s.to_string())));
                self.push_style(|s| {
                    s.bold = true;
                    s.size = HEADING_SIZES[n - 1];
                });
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.stack.push(Frame::Quote(Vec::new()));
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                self.code_lang = match kind {
                    CodeBlockKind::Fenced(info) => crate::syntax::from_tag(&info),
                    CodeBlockKind::Indented => crate::syntax::Lang::Plain,
                };
                self.code = Some(String::new());
            }
            Tag::List(start) => {
                self.flush();
                self.stack.push(Frame::List(List { ordered: start.is_some(), start: start.unwrap_or(1) as u32, items: Vec::new() }));
            }
            Tag::Item => {
                self.flush();
                if let Some(Frame::List(l)) = self.stack.last_mut() {
                    l.items.push(ListItem::default());
                }
            }
            Tag::FootnoteDefinition(label) => {
                self.flush();
                self.stack.push(Frame::Footnote(label.to_string(), Vec::new()));
            }
            Tag::Table(aligns) => {
                self.flush();
                self.stack.push(Frame::Table { table: Table::default(), aligns, head: false });
            }
            Tag::TableHead => {
                if let Some(Frame::Table { table, head, .. }) = self.stack.last_mut() {
                    *head = true;
                    table.rows.push(Row::default());
                }
            }
            Tag::TableRow => {
                if let Some(Frame::Table { table, head, .. }) = self.stack.last_mut() {
                    *head = false;
                    table.rows.push(Row::default());
                }
            }
            Tag::TableCell => self.inlines.clear(),
            Tag::Emphasis => self.push_style(|s| s.italic = true),
            Tag::Strong => self.push_style(|s| s.bold = true),
            Tag::Strikethrough => self.push_style(|s| s.strike = true),
            Tag::Link { dest_url, .. } => {
                self.links.push(dest_url.to_string());
                let n = self.links.len() as u32;
                self.push_style(|s| s.link = n);
            }
            Tag::Image { dest_url, .. } => self.image = Some((dest_url.to_string(), String::new())),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush(),
            TagEnd::Heading(_) => {
                self.flush_html();
                self.pop_style();
                let inlines = std::mem::take(&mut self.inlines);
                let Some((n, id)) = self.heading.take() else { return };
                let text = super::plain_text(&[Block::para(inlines.clone())]);
                let mut anchor = id.unwrap_or_else(|| slug(&text));
                let count = self.slugs.entry(anchor.clone()).or_insert(0);
                if *count > 0 {
                    anchor = format!("{anchor}-{count}");
                }
                *count += 1;
                self.push_block(Block {
                    kind: Kind::Para(inlines),
                    margin_top: if n <= 2 { 1.2 } else { 1.0 },
                    margin_bottom: if n <= 2 { 0.6 } else { 0.5 },
                    underline: n <= 2,
                    anchor: Some(anchor),
                    ..Block::default()
                });
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                if let Some(Frame::Quote(blocks)) = self.stack.pop() {
                    self.push_block(Block { kind: Kind::Group(blocks), indent: 1.0, bar: true, margin_bottom: 0.8, ..Block::default() });
                }
            }
            TagEnd::CodeBlock => {
                let mut code = self.code.take().unwrap_or_default();
                if code.ends_with('\n') {
                    code.pop();
                }
                let style = Style { mono: true, size: 0.9, ..Style::default() };
                let inlines = super::code_inlines(self.code_lang, code, style);
                self.push_block(Block { kind: Kind::Pre(inlines), margin_bottom: 0.8, padding: 0.8, ..Block::default() });
            }
            TagEnd::List(_) => {
                self.flush();
                if let Some(Frame::List(l)) = self.stack.pop() {
                    self.push_block(Block { kind: Kind::List(l), margin_bottom: 0.8, ..Block::default() });
                }
            }
            TagEnd::Item => self.flush(),
            TagEnd::FootnoteDefinition => {
                self.flush();
                if let Some(Frame::Footnote(label, mut blocks)) = self.stack.pop() {
                    let mark = Inline::Text(format!("{label}. "), Style { bold: true, size: 0.85, ..Style::default() });
                    match blocks.first_mut().map(|b| &mut b.kind) {
                        Some(Kind::Para(v)) => v.insert(0, mark),
                        _ => blocks.insert(0, Block::para(vec![mark])),
                    }
                    self.push_block(Block { kind: Kind::Group(blocks), anchor: Some(format!("fn-{label}")), margin_bottom: 0.4, ..Block::default() });
                }
            }
            TagEnd::TableCell => {
                self.flush_html();
                let inlines = std::mem::take(&mut self.inlines);
                if let Some(Frame::Table { table, aligns, head }) = self.stack.last_mut() {
                    if let Some(row) = table.rows.last_mut() {
                        let align = match aligns.get(row.cells.len()) {
                            Some(Alignment::Center) => Align::Center,
                            Some(Alignment::Right) => Align::Right,
                            _ => Align::Left,
                        };
                        let inlines = if *head {
                            inlines
                                .into_iter()
                                .map(|i| match i {
                                    Inline::Text(t, s) => Inline::Text(t, Style { bold: true, ..s }),
                                    other => other,
                                })
                                .collect()
                        } else {
                            inlines
                        };
                        row.cells.push(super::Cell {
                            blocks: vec![Block { kind: Kind::Para(inlines), ..Block::default() }],
                            header: *head,
                            align,
                            colspan: 1,
                            bg: None,
                        });
                    }
                }
            }
            TagEnd::Table => {
                if let Some(Frame::Table { table, .. }) = self.stack.pop() {
                    self.push_block(Block { kind: Kind::Table(table), margin_bottom: 0.8, ..Block::default() });
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => self.pop_style(),
            TagEnd::Image => {
                if let Some((src, alt)) = self.image.take() {
                    let style = self.style();
                    self.inlines.push(Inline::Image { src, alt, width: None, height: None, style });
                }
            }
            _ => {}
        }
    }
}

/// Parses Markdown into a rich document.
pub fn parse(src: &str) -> RichDoc {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_FOOTNOTES | Options::ENABLE_HEADING_ATTRIBUTES;
    let mut b = Builder {
        stack: vec![Frame::Root(Vec::new())],
        inlines: Vec::new(),
        styles: vec![Style::default()],
        links: Vec::new(),
        heading: None,
        code: None,
        code_lang: crate::syntax::Lang::Plain,
        image: None,
        html: String::new(),
        html_tags: Vec::new(),
        slugs: HashMap::new(),
    };
    for ev in Parser::new_ext(src, opts) {
        if !matches!(ev, Event::Html(_)) {
            b.flush_html();
        }
        match ev {
            Event::Start(t) => b.start(t),
            Event::End(t) => b.end(t),
            Event::Text(t) => b.text(&t),
            Event::Code(t) => {
                let s = Style { mono: true, code: true, ..b.style() };
                b.inlines.push(Inline::Text(t.to_string(), s));
            }
            Event::Html(t) => b.html.push_str(&t),
            Event::InlineHtml(t) => b.inline_html(&t),
            Event::SoftBreak => b.text(" "),
            Event::HardBreak => b.inlines.push(Inline::Break),
            Event::Rule => {
                b.flush();
                b.push_block(Block { kind: Kind::Rule, margin_top: 0.6, margin_bottom: 1.0, ..Block::default() });
            }
            Event::TaskListMarker(done) => {
                if let Some(Frame::List(l)) = b.stack.last_mut() {
                    if let Some(item) = l.items.last_mut() {
                        item.checked = Some(done);
                    }
                }
            }
            Event::FootnoteReference(label) => {
                b.links.push(format!("#fn-{label}"));
                let n = b.links.len() as u32;
                let s = Style { valign: 1, size: b.style().size * 0.8, link: n, ..b.style() };
                b.inlines.push(Inline::Text(format!("[{label}]"), s));
            }
            _ => {}
        }
    }
    b.flush();
    // Close anything left open (pulldown-cmark always balances events, but
    // stay safe).
    while b.stack.len() > 1 {
        let kind = match b.stack.pop() {
            Some(Frame::Quote(v) | Frame::Footnote(_, v) | Frame::Root(v)) => Kind::Group(v),
            Some(Frame::List(l)) => Kind::List(l),
            Some(Frame::Table { table, .. }) => Kind::Table(table),
            None => Kind::Empty,
        };
        b.push_block(Block { kind, ..Block::default() });
    }
    let blocks = match b.stack.pop() {
        Some(Frame::Root(v)) => v,
        _ => Vec::new(),
    };
    let title = blocks.iter().find_map(|bl| match (&bl.kind, bl.underline) {
        (Kind::Para(v), true) => Some(super::plain_text(&[Block::para(v.clone())]).trim().to_string()),
        _ => None,
    });
    RichDoc { blocks, links: b.links, title, max_width: Some(52.0), ..RichDoc::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(d: &RichDoc) -> Vec<&'static str> {
        d.blocks
            .iter()
            .map(|b| match b.kind {
                Kind::Para(_) => "p",
                Kind::Pre(_) => "pre",
                Kind::List(_) => "list",
                Kind::Group(_) => "group",
                Kind::Rule => "hr",
                Kind::Table(_) => "table",
                Kind::Empty => "empty",
            })
            .collect()
    }

    #[test]
    fn structure() {
        let d = parse("# Title\n\nSome *em* and **strong** `code`.\n\n- a\n- [x] b\n\n> quote\n\n```\nfn x() {}\n```\n\n---\n\n| A | B |\n|---|--:|\n| 1 | 2 |\n");
        assert_eq!(kinds(&d), ["p", "p", "list", "group", "pre", "hr", "table"]);
        assert_eq!(d.title.as_deref(), Some("Title"));
        assert_eq!(d.blocks[0].anchor.as_deref(), Some("title"));
        let Kind::List(l) = &d.blocks[2].kind else { panic!() };
        assert_eq!(l.items.len(), 2);
        assert_eq!(l.items[1].checked, Some(true));
        let Kind::Table(t) = &d.blocks[6].kind else { panic!() };
        assert_eq!(t.rows.len(), 2);
        assert!(t.rows[0].cells[0].header);
        assert_eq!(t.rows[1].cells[1].align, Align::Right);
        let text = super::super::plain_text(&d.blocks);
        assert!(text.contains("Some em and strong code."), "{text}");
    }

    #[test]
    fn links_images_and_html() {
        let d = parse("See [docs](other.md#x) ![logo](img/a.png)\n\n<p align=\"center\"><b>Hi</b><br>there</p>\n\nA<br>B");
        assert_eq!(d.links, vec!["other.md#x".to_string()]);
        let Kind::Para(v) = &d.blocks[0].kind else { panic!() };
        assert!(v.iter().any(|i| matches!(i, Inline::Image { src, alt, .. } if src == "img/a.png" && alt == "logo")));
        assert_eq!(d.blocks[1].align, Align::Center);
        let Kind::Para(v) = &d.blocks[2].kind else { panic!() };
        assert!(v.contains(&Inline::Break));
    }

    #[test]
    fn code_blocks_are_highlighted() {
        let d = parse("```rust
fn main() {} // x
```

```
plain
```
");
        let Kind::Pre(v) = &d.blocks[0].kind else { panic!() };
        let toks: Vec<u8> = v.iter().filter_map(|i| if let Inline::Text(_, s) = i { Some(s.tok) } else { None }).collect();
        assert!(toks.contains(&(crate::syntax::Tok::Keyword as u8 + 1)), "{v:?}");
        assert!(toks.contains(&(crate::syntax::Tok::Comment as u8 + 1)));
        let Kind::Pre(v) = &d.blocks[1].kind else { panic!() };
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Hello, World!"), "hello-world");
        assert_eq!(slug("Ünïcode Title 2"), "ünïcode-title-2");
        let d = parse("# A\n# A\n");
        assert_eq!(d.blocks[1].anchor.as_deref(), Some("a-1"));
    }
}
