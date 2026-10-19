//! Rich documents (Markdown and HTML) share one model: a tree of blocks
//! holding styled inline content. Parsers build it; `layout` turns it into
//! positioned drawing items for a given width and zoom.

pub mod css;
pub mod html;
pub mod layout;
pub mod markdown;

use crate::ui::gfx::Color;
use std::path::PathBuf;

/// Inline text style. Sizes are relative to the document's base size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    pub size: f32,
    pub color: Option<Color>,
    pub bg: Option<Color>,
    pub underline: bool,
    pub strike: bool,
    /// Index + 1 into `RichDoc::links` (0 = not a link).
    pub link: u32,
    /// -1 subscript, +1 superscript.
    pub valign: i8,
    /// Inline code: drawn on a subtle background.
    pub code: bool,
    /// Syntax token of highlighted code: `Tok` index + 1 (0 = none).
    pub tok: u8,
}

impl Default for Style {
    fn default() -> Style {
        Style { bold: false, italic: false, mono: false, size: 1.0, color: None, bg: None, underline: false, strike: false, link: 0, valign: 0, code: false, tok: 0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String, Style),
    Break,
    Image { src: String, alt: String, width: Option<Length>, height: Option<Length>, style: Style },
    /// Target of `#name` links.
    Anchor(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Percent(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Block {
    pub kind: Kind,
    pub align: Align,
    /// Vertical margins in em.
    pub margin_top: f32,
    pub margin_bottom: f32,
    /// Left indent in em.
    pub indent: f32,
    /// Inner padding in em (for blocks with a background).
    pub padding: f32,
    pub bg: Option<Color>,
    /// A bar on the left (block quotes).
    pub bar: bool,
    /// A rule below (Markdown h1/h2).
    pub underline: bool,
    pub anchor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Kind {
    #[default]
    Empty,
    Para(Vec<Inline>),
    /// Preformatted: line breaks and spaces kept, no wrapping.
    Pre(Vec<Inline>),
    List(List),
    Group(Vec<Block>),
    Rule,
    Table(Table),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct List {
    pub ordered: bool,
    pub start: u32,
    pub items: Vec<ListItem>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ListItem {
    pub blocks: Vec<Block>,
    /// Task list checkbox state.
    pub checked: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Table {
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Row {
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Cell {
    pub blocks: Vec<Block>,
    pub header: bool,
    pub align: Align,
    pub colspan: usize,
    pub bg: Option<Color>,
}

#[derive(Clone, Debug, Default)]
pub struct RichDoc {
    pub blocks: Vec<Block>,
    pub links: Vec<String>,
    /// Folder for resolving relative links and images.
    pub base: PathBuf,
    pub title: Option<String>,
    pub page_bg: Option<Color>,
    pub page_color: Option<Color>,
    /// Maximum text column width in em (Markdown reads better narrower).
    pub max_width: Option<f32>,
    pub encoding: String,
}

impl Block {
    pub fn para(inlines: Vec<Inline>) -> Block {
        Block { kind: Kind::Para(inlines), margin_bottom: 0.8, ..Block::default() }
    }

    pub fn is_empty(&self) -> bool {
        match &self.kind {
            Kind::Empty => true,
            Kind::Para(v) => v.iter().all(|i| matches!(i, Inline::Text(t, _) if t.trim().is_empty())),
            Kind::Group(b) => b.iter().all(Block::is_empty) && self.bg.is_none(),
            _ => false,
        }
    }
}

/// Code as inline runs, syntax highlighted when the language is known.
pub fn code_inlines(lang: crate::syntax::Lang, code: String, base: Style) -> Vec<Inline> {
    if lang == crate::syntax::Lang::Plain || code.len() > 1 << 20 {
        return vec![Inline::Text(code, base)];
    }
    crate::syntax::highlight(lang, &code)
        .into_iter()
        .map(|(t, tok)| {
            let tok = if matches!(tok, crate::syntax::Tok::Text) { 0 } else { tok as u8 + 1 };
            Inline::Text(t, Style { tok, ..base })
        })
        .collect()
}

/// All text of a block tree (tests, accessibility).
pub fn plain_text(blocks: &[Block]) -> String {
    fn inl(v: &[Inline], out: &mut String) {
        for i in v {
            match i {
                Inline::Text(t, _) => out.push_str(t),
                Inline::Break => out.push('\n'),
                Inline::Image { alt, .. } => out.push_str(alt),
                Inline::Anchor(_) => {}
            }
        }
    }
    fn walk(b: &[Block], out: &mut String) {
        for b in b {
            match &b.kind {
                Kind::Para(v) | Kind::Pre(v) => {
                    inl(v, out);
                    out.push('\n');
                }
                Kind::List(l) => l.items.iter().for_each(|i| walk(&i.blocks, out)),
                Kind::Group(g) => walk(g, out),
                Kind::Table(t) => {
                    for r in &t.rows {
                        for c in &r.cells {
                            walk(&c.blocks, out);
                        }
                    }
                }
                Kind::Rule | Kind::Empty => {}
            }
        }
    }
    let mut s = String::new();
    walk(blocks, &mut s);
    s
}
