//! HTML → rich document. A tolerant tokenizer and tree builder (implied
//! end tags for `p`, `li`, table parts…, raw text for `script`/`style`)
//! followed by a conversion that applies built-in tag styles, legacy
//! attributes, `<style>`/local `<link>` style sheets and `style=""`.
//! Scripts never run and nothing is fetched from the network.

use super::css::{self, Elem, Sheet};
use super::{Align, Block, Cell, Inline, Kind, Length, List, ListItem, RichDoc, Row, Style, Table};
use crate::ui::gfx::Color;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct Node {
    /// Lowercase tag name; empty for text nodes.
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<usize>,
}

impl Node {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
}

pub struct Dom {
    pub nodes: Vec<Node>,
}

const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr", "basefont", "frame"];
const RAW: &[&str] = &["script", "style", "textarea", "title", "xmp", "noembed", "iframe", "noframes", "plaintext"];
const CLOSES_P: &[&str] = &[
    "address", "article", "aside", "blockquote", "center", "details", "dialog", "dir", "div", "dl", "fieldset", "figcaption", "figure", "footer", "form",
    "h1", "h2", "h3", "h4", "h5", "h6", "header", "hgroup", "hr", "main", "menu", "nav", "ol", "p", "pre", "section", "table", "ul", "li", "dd", "dt",
];
const BLOCK: &[&str] = &[
    "html", "body", "address", "article", "aside", "blockquote", "center", "details", "dialog", "dir", "div", "dl", "dd", "dt", "fieldset", "figcaption",
    "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hgroup", "hr", "main", "menu", "nav", "ol", "p", "pre", "section", "table",
    "ul", "li", "summary", "caption", "legend", "noscript", "listing", "xmp", "plaintext", "search",
];
const HIDDEN: &[&str] = &[
    "head", "script", "style", "title", "meta", "link", "template", "noembed", "noframes", "iframe", "object", "embed", "svg", "math", "canvas", "video",
    "audio", "select", "datalist", "map", "area", "param", "source", "track", "base", "dialog", "frame", "frameset",
];

// ---------------------------------------------------------------- entities

const ENTITIES: &[(&str, &str)] = &[
    ("amp", "&"), ("lt", "<"), ("gt", ">"), ("quot", "\""), ("apos", "'"), ("nbsp", "\u{A0}"), ("copy", "©"), ("reg", "®"), ("trade", "™"),
    ("hellip", "…"), ("mdash", "—"), ("ndash", "–"), ("lsquo", "‘"), ("rsquo", "’"), ("sbquo", "‚"), ("ldquo", "“"), ("rdquo", "”"),
    ("bdquo", "„"), ("bull", "•"), ("middot", "·"), ("laquo", "«"), ("raquo", "»"), ("lsaquo", "‹"), ("rsaquo", "›"), ("deg", "°"),
    ("plusmn", "±"), ("times", "×"), ("divide", "÷"), ("euro", "€"), ("pound", "£"), ("yen", "¥"), ("cent", "¢"), ("curren", "¤"),
    ("sect", "§"), ("para", "¶"), ("frac12", "½"), ("frac14", "¼"), ("frac34", "¾"), ("sup1", "¹"), ("sup2", "²"), ("sup3", "³"),
    ("iexcl", "¡"), ("iquest", "¿"), ("acute", "´"), ("cedil", "¸"), ("micro", "µ"), ("ordf", "ª"), ("ordm", "º"), ("not", "¬"),
    ("shy", "\u{AD}"), ("macr", "¯"), ("uml", "¨"), ("brvbar", "¦"), ("dagger", "†"), ("Dagger", "‡"), ("permil", "‰"), ("prime", "′"),
    ("Prime", "″"), ("larr", "←"), ("rarr", "→"), ("uarr", "↑"), ("darr", "↓"), ("harr", "↔"), ("lArr", "⇐"), ("rArr", "⇒"), ("hArr", "⇔"),
    ("le", "≤"), ("ge", "≥"), ("ne", "≠"), ("asymp", "≈"), ("equiv", "≡"), ("infin", "∞"), ("minus", "−"), ("radic", "√"), ("sum", "∑"),
    ("prod", "∏"), ("part", "∂"), ("int", "∫"), ("isin", "∈"), ("cap", "∩"), ("cup", "∪"), ("and", "∧"), ("or", "∨"), ("forall", "∀"),
    ("exist", "∃"), ("empty", "∅"), ("nabla", "∇"), ("loz", "◊"), ("spades", "♠"), ("clubs", "♣"), ("hearts", "♥"), ("diams", "♦"),
    ("check", "✓"), ("cross", "✗"), ("star", "☆"), ("starf", "★"), ("ensp", "\u{2002}"), ("emsp", "\u{2003}"), ("thinsp", "\u{2009}"),
    ("zwnj", "\u{200C}"), ("zwj", "\u{200D}"), ("lrm", "\u{200E}"), ("rlm", "\u{200F}"), ("alpha", "α"), ("beta", "β"), ("gamma", "γ"),
    ("delta", "δ"), ("epsilon", "ε"), ("theta", "θ"), ("lambda", "λ"), ("mu", "μ"), ("pi", "π"), ("sigma", "σ"), ("tau", "τ"), ("phi", "φ"),
    ("omega", "ω"), ("Delta", "Δ"), ("Sigma", "Σ"), ("Omega", "Ω"), ("Pi", "Π"), ("szlig", "ß"), ("aelig", "æ"), ("AElig", "Æ"),
    ("oslash", "ø"), ("Oslash", "Ø"), ("aring", "å"), ("Aring", "Å"), ("ccedil", "ç"), ("Ccedil", "Ç"), ("ntilde", "ñ"), ("Ntilde", "Ñ"),
    ("otilde", "õ"), ("Otilde", "Õ"), ("atilde", "ã"), ("Atilde", "Ã"), ("eth", "ð"), ("thorn", "þ"), ("yuml", "ÿ"), ("oelig", "œ"),
    ("OElig", "Œ"), ("scaron", "š"), ("Scaron", "Š"), ("fnof", "ƒ"), ("circ", "ˆ"), ("tilde", "˜"),
];

fn accented(name: &str) -> Option<char> {
    // e.g. eacute, Uuml, agrave, ocirc
    let marks: [(&str, &str, &str); 5] = [
        ("acute", "aeiouyAEIOUY", "áéíóúýÁÉÍÓÚÝ"),
        ("grave", "aeiouAEIOU", "àèìòùÀÈÌÒÙ"),
        ("circ", "aeiouAEIOU", "âêîôûÂÊÎÔÛ"),
        ("uml", "aeiouyAEIOU", "äëïöüÿÄËÏÖÜ"),
        ("tilde", "anoANO", "ãñõÃÑÕ"),
    ];
    for (suffix, base, out) in marks {
        if let Some(b) = name.strip_suffix(suffix) {
            let mut ch = b.chars();
            let (Some(c), None) = (ch.next(), ch.next()) else { continue };
            if let Some(i) = base.chars().position(|x| x == c) {
                return out.chars().nth(i);
            }
        }
    }
    None
}

/// Decodes character references (`&amp;`, `&#233;`, `&#xE9;`).
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let end = after.char_indices().take(32).find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '#')).map(|(j, _)| j).unwrap_or(after.len().min(32));
        let name = &after[..end];
        let semi = after[end..].starts_with(';');
        let decoded = if let Some(num) = name.strip_prefix('#') {
            let v = if let Some(h) = num.strip_prefix(['x', 'X']) { u32::from_str_radix(h, 16).ok() } else { num.parse().ok() };
            v.map(|v| match v {
                0 => '\u{FFFD}',
                0x80..=0x9F => encoding_rs::WINDOWS_1252.decode_without_bom_handling(&[v as u8]).0.chars().next().unwrap_or('\u{FFFD}'),
                _ => char::from_u32(v).unwrap_or('\u{FFFD}'),
            })
            .map(String::from)
        } else {
            ENTITIES.iter().find(|(n, _)| *n == name).map(|(_, v)| v.to_string()).or_else(|| accented(name).map(String::from))
        };
        match decoded {
            Some(d) if !name.is_empty() => {
                out.push_str(&d);
                rest = &after[end + usize::from(semi)..];
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------- parsing

/// Parses the attributes part of a tag (`a="1" b='2' c=3 d`).
pub fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' && b[i] != b'>' {
            i += 1;
        }
        if start == i {
            i += 1;
            continue;
        }
        let name = s[start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let vs = i + 1;
                let ve = s[vs..].bytes().position(|c| c == q).map_or(s.len(), |p| vs + p);
                value = decode_entities(&s[vs..ve]);
                i = ve + 1;
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                value = decode_entities(&s[vs..i]);
            }
        }
        if !out.iter().any(|(n, _): &(String, String)| *n == name) {
            out.push((name, value));
        }
    }
    out
}

struct TreeBuilder {
    nodes: Vec<Node>,
    stack: Vec<usize>,
}

impl TreeBuilder {
    fn top(&self) -> usize {
        *self.stack.last().unwrap_or(&0)
    }

    fn open(&self, tag: &str) -> Option<usize> {
        self.stack.iter().rposition(|&n| self.nodes[n].tag == tag)
    }

    /// Position of `tag` in the stack if found before any of `stop`.
    fn open_in_scope(&self, tag: &str, stop: &[&str]) -> Option<usize> {
        for (pos, &n) in self.stack.iter().enumerate().rev() {
            let t = self.nodes[n].tag.as_str();
            if t == tag {
                return Some(pos);
            }
            if stop.contains(&t) {
                return None;
            }
        }
        None
    }

    fn close_to(&mut self, pos: usize) {
        self.stack.truncate(pos.max(1));
    }

    fn text(&mut self, t: String) {
        if t.is_empty() {
            return;
        }
        let parent = self.top();
        if let Some(&last) = self.nodes[parent].children.last() {
            if self.nodes[last].tag.is_empty() {
                self.nodes[last].text.push_str(&t);
                return;
            }
        }
        self.nodes.push(Node { text: t, ..Node::default() });
        let id = self.nodes.len() - 1;
        self.nodes[parent].children.push(id);
    }

    fn start(&mut self, tag: String, attrs: Vec<(String, String)>, self_closing: bool) -> usize {
        let t = tag.as_str();
        if matches!(t, "html" | "body" | "head") {
            if let Some(pos) = self.open(t) {
                // Merge attributes into the existing element.
                let n = self.stack[pos];
                for a in attrs {
                    if self.nodes[n].attr(&a.0).is_none() {
                        self.nodes[n].attrs.push(a);
                    }
                }
                return n;
            }
        }
        if CLOSES_P.contains(&t) {
            if let Some(pos) = self.open_in_scope("p", &["button", "table", "td", "th", "caption", "html", "body"]) {
                self.close_to(pos);
            }
        }
        match t {
            "li" => {
                if let Some(pos) = self.open_in_scope("li", &["ul", "ol", "menu", "table"]) {
                    self.close_to(pos);
                }
            }
            "dt" | "dd" => {
                if let Some(pos) = self.open_in_scope("dd", &["dl"]).or_else(|| self.open_in_scope("dt", &["dl"])) {
                    self.close_to(pos);
                }
            }
            "tr" => {
                if let Some(pos) = self.open_in_scope("tr", &["table"]) {
                    self.close_to(pos);
                }
            }
            "td" | "th" => {
                if let Some(pos) = self.open_in_scope("td", &["tr", "table"]).or_else(|| self.open_in_scope("th", &["tr", "table"])) {
                    self.close_to(pos);
                }
            }
            "thead" | "tbody" | "tfoot" => {
                for g in ["thead", "tbody", "tfoot"] {
                    if let Some(pos) = self.open_in_scope(g, &["table"]) {
                        self.close_to(pos);
                    }
                }
            }
            "option" => {
                if let Some(pos) = self.open_in_scope("option", &["select"]) {
                    self.close_to(pos);
                }
            }
            _ => {}
        }
        let parent = self.top();
        let void = VOID.contains(&t) || self_closing && !matches!(t, "div" | "p" | "span" | "a" | "table" | "td" | "li");
        self.nodes.push(Node { tag, attrs, ..Node::default() });
        let id = self.nodes.len() - 1;
        self.nodes[parent].children.push(id);
        if !void {
            self.stack.push(id);
        }
        id
    }

    fn end(&mut self, tag: &str) {
        if tag == "br" {
            self.start("br".into(), Vec::new(), true);
            return;
        }
        if matches!(tag, "html" | "body") {
            return;
        }
        if let Some(pos) = self.open(tag) {
            self.close_to(pos);
        }
    }
}

/// Parses an HTML document or fragment into a tree (node 0 is the root).
pub fn parse_dom(src: &str) -> Dom {
    let mut tb = TreeBuilder { nodes: vec![Node { tag: "#root".into(), ..Node::default() }], stack: vec![0] };
    let b = src.as_bytes();
    let mut i = 0;
    let mut text_start = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let next = b.get(i + 1).copied().unwrap_or(0);
        let is_tag = next.is_ascii_alphabetic() || next == b'/' || next == b'!' || next == b'?';
        if !is_tag {
            i += 1;
            continue;
        }
        if text_start < i {
            tb.text(decode_entities(&src[text_start..i]));
        }
        if src[i..].starts_with("<!--") {
            i = src[i + 4..].find("-->").map_or(b.len(), |p| i + 4 + p + 3);
            text_start = i;
            continue;
        }
        if next == b'!' || next == b'?' {
            if src[i..].starts_with("<![CDATA[") {
                let end = src[i + 9..].find("]]>").map_or(b.len(), |p| i + 9 + p);
                tb.text(src[i + 9..end].to_string());
                i = (end + 3).min(b.len());
            } else {
                i = src[i..].find('>').map_or(b.len(), |p| i + p + 1);
            }
            text_start = i;
            continue;
        }
        // Tag: find the closing '>' outside quotes.
        let mut j = i + 1;
        let mut quote = 0u8;
        while j < b.len() {
            let c = b[j];
            if quote != 0 {
                if c == quote {
                    quote = 0;
                }
            } else if c == b'"' || c == b'\'' {
                quote = c;
            } else if c == b'>' {
                break;
            }
            j += 1;
        }
        let inner = &src[i + 1..j.min(b.len())];
        i = (j + 1).min(b.len());
        text_start = i;
        if let Some(name) = inner.strip_prefix('/') {
            let name: String = name.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>().to_ascii_lowercase();
            tb.end(&name);
            continue;
        }
        let name_len = inner.find(|c: char| c.is_whitespace() || c == '/' || c == '>').unwrap_or(inner.len());
        let name = inner[..name_len].to_ascii_lowercase();
        let self_closing = inner.trim_end().ends_with('/');
        let attrs = parse_attrs(&inner[name_len..]);
        let id = tb.start(name.clone(), attrs, self_closing);
        if RAW.contains(&name.as_str()) && !self_closing {
            // Raw text up to the matching end tag.
            let lower = src[i..].to_ascii_lowercase();
            let close = format!("</{name}");
            let end = lower.find(&close).map_or(b.len(), |p| i + p);
            let content = &src[i..end];
            let text = if matches!(name.as_str(), "textarea" | "title") { decode_entities(content) } else { content.to_string() };
            if !text.is_empty() {
                tb.nodes.push(Node { text, ..Node::default() });
                let t = tb.nodes.len() - 1;
                tb.nodes[id].children.push(t);
            }
            tb.end(&name);
            i = src[end..].find('>').map_or(b.len(), |p| end + p + 1);
            text_start = i;
        }
    }
    if text_start < b.len() {
        tb.text(decode_entities(&src[text_start..]));
    }
    Dom { nodes: tb.nodes }
}

/// Decodes HTML bytes: BOM, then `<meta charset>`, then UTF-8, then Windows-1252.
pub fn decode(bytes: &[u8]) -> (String, String) {
    if let Some((enc, len)) = encoding_rs::Encoding::for_bom(bytes) {
        return (enc.decode_without_bom_handling(&bytes[len..]).0.into_owned(), enc.name().to_string());
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
    if let Some(p) = head.find("charset=") {
        let label: String = head[p + 8..].trim_start_matches(['"', '\'']).chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
        if let Some(enc) = encoding_rs::Encoding::for_label(label.as_bytes()) {
            let enc = if enc == encoding_rs::UTF_16LE || enc == encoding_rs::UTF_16BE { encoding_rs::UTF_8 } else { enc };
            return (enc.decode_without_bom_handling(bytes).0.into_owned(), enc.name().to_string());
        }
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), "UTF-8".into()),
        Err(_) => (encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes).0.into_owned(), "windows-1252".into()),
    }
}

// ---------------------------------------------------------------- conversion

#[derive(Clone, Copy, PartialEq, Eq)]
enum Display {
    Inline,
    Block,
    ListItem,
    None,
}

#[derive(Clone, Copy)]
struct Computed {
    style: Style,
    align: Align,
    pre: bool,
    display: Display,
    bg: Option<Color>,
}

pub fn length(v: &str) -> Option<Length> {
    let v = v.trim();
    if let Some(p) = v.strip_suffix('%') {
        return p.trim().parse().ok().map(Length::Percent);
    }
    v.trim_end_matches("px").trim().parse::<f32>().ok().filter(|x| *x > 0.0).map(Length::Px)
}

struct Converter<'a> {
    dom: &'a Dom,
    sheet: Sheet,
    links: Vec<String>,
    title: Option<String>,
    page_bg: Option<Color>,
    page_color: Option<Color>,
    /// Element path for selector matching (indices into nodes).
    path: Vec<usize>,
}

fn heading_level(tag: &str) -> Option<usize> {
    match tag {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

const H_SIZES: [f32; 6] = [2.0, 1.5, 1.17, 1.0, 0.83, 0.67];

impl Converter<'_> {
    fn compute(&self, n: usize, parent: &Computed) -> Computed {
        let node = &self.dom.nodes[n];
        let tag = node.tag.as_str();
        let mut c = Computed { bg: None, display: Display::Inline, ..*parent };
        c.style.bg = if parent.display == Display::Inline { parent.style.bg } else { None };
        c.display = if HIDDEN.contains(&tag) {
            Display::None
        } else if tag == "li" {
            Display::ListItem
        } else if BLOCK.contains(&tag) || matches!(tag, "tr" | "td" | "th" | "thead" | "tbody" | "tfoot") {
            Display::Block
        } else {
            Display::Inline
        };
        if tag == "input" && node.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("hidden")) {
            c.display = Display::None;
        }
        let s = &mut c.style;
        match tag {
            "b" | "strong" | "th" => s.bold = true,
            "i" | "em" | "cite" | "var" | "dfn" | "address" => s.italic = true,
            "code" | "kbd" | "samp" | "tt" | "pre" | "listing" | "xmp" | "plaintext" => s.mono = true,
            "u" | "ins" => s.underline = true,
            "s" | "strike" | "del" => s.strike = true,
            "small" => s.size *= 0.83,
            "big" => s.size *= 1.2,
            "sub" => {
                s.valign = -1;
                s.size *= 0.8;
            }
            "sup" => {
                s.valign = 1;
                s.size *= 0.8;
            }
            "mark" => s.bg = Some(Color::rgb(0xFFF3A3)),
            "center" => c.align = Align::Center,
            "caption" => c.align = Align::Center,
            "font" => {
                if let Some(col) = node.attr("color").and_then(css::color) {
                    s.color = Some(col);
                }
                if let Some(sz) = node.attr("size").and_then(|v| v.trim().parse::<i32>().ok()) {
                    s.size = [0.63, 0.82, 1.0, 1.13, 1.5, 2.0, 3.0][(sz.clamp(1, 7) - 1) as usize];
                }
                if node.attr("face").is_some_and(css::is_mono_family) {
                    s.mono = true;
                }
            }
            "a" => {
                if let Some(href) = node.attr("href") {
                    self.link_placeholder(s, href);
                    // Like a browser's default link style, links do not
                    // inherit the text color (author CSS can still set one).
                    s.color = None;
                }
            }
            _ => {}
        }
        if let Some(l) = heading_level(tag) {
            s.bold = true;
            s.size = H_SIZES[l - 1];
        }
        if matches!(tag, "pre" | "listing" | "xmp" | "plaintext" | "textarea") {
            c.pre = true;
        }
        if tag == "th" {
            c.align = Align::Center;
        }
        if let Some(a) = node.attr("align") {
            c.align = match a.to_ascii_lowercase().as_str() {
                "center" | "middle" => Align::Center,
                "right" => Align::Right,
                "left" => Align::Left,
                _ => c.align,
            };
        }
        if let Some(bg) = node.attr("bgcolor").and_then(css::color) {
            c.bg = Some(bg);
        }
        if tag == "body" {
            if let Some(t) = node.attr("text").and_then(css::color) {
                c.style.color = Some(t);
            }
        }
        // Style sheets, then the style attribute.
        let mut decls = if self.sheet.rules.is_empty() { Vec::new() } else { self.sheet.cascade(&self.elem_path()) };
        if let Some(inline) = node.attr("style") {
            decls.extend(css::declarations(inline));
        }
        for (k, v) in decls {
            let lv = v.to_ascii_lowercase();
            match k.as_str() {
                "color" => {
                    if let Some(col) = css::color(&v) {
                        c.style.color = Some(col);
                    }
                }
                "background-color" | "background" => {
                    let col = lv.split_whitespace().find_map(css::color).or_else(|| css::color(&lv));
                    if col.is_some() || lv.contains("none") || lv.contains("transparent") {
                        if c.display == Display::Inline {
                            c.style.bg = col;
                        } else {
                            c.bg = col;
                        }
                    }
                }
                "font-weight" => c.style.bold = lv.starts_with("bold") || lv.parse::<u32>().is_ok_and(|w| w >= 600),
                "font-style" => c.style.italic = lv.contains("italic") || lv.contains("oblique"),
                "font-size" => {
                    if let Some(sz) = css::font_size(&lv, parent.style.size) {
                        c.style.size = sz;
                    }
                }
                "font-family" => c.style.mono = css::is_mono_family(&lv),
                "font" => {
                    if css::is_mono_family(&lv) {
                        c.style.mono = true;
                    }
                    if lv.contains("bold") {
                        c.style.bold = true;
                    }
                    if lv.contains("italic") {
                        c.style.italic = true;
                    }
                }
                "text-align" => {
                    c.align = match lv.as_str() {
                        "center" => Align::Center,
                        "right" | "end" => Align::Right,
                        "left" | "start" | "justify" => Align::Left,
                        _ => c.align,
                    }
                }
                "text-decoration" | "text-decoration-line" => {
                    c.style.underline = lv.contains("underline");
                    c.style.strike = lv.contains("line-through");
                }
                "display" => {
                    c.display = match lv.as_str() {
                        "none" => Display::None,
                        "inline" | "inline-block" | "inline-flex" | "contents" => Display::Inline,
                        "list-item" => Display::ListItem,
                        _ => Display::Block,
                    }
                }
                "visibility" if lv == "hidden" || lv == "collapse" => c.display = Display::None,
                "white-space" => c.pre = lv.starts_with("pre"),
                _ => {}
            }
        }
        c
    }

    fn link_placeholder(&self, s: &mut Style, _href: &str) {
        // The real index is assigned when the element is visited (see `add_link`).
        s.link = u32::MAX;
    }

    fn add_link(&mut self, href: &str) -> u32 {
        self.links.push(href.to_string());
        self.links.len() as u32
    }

    fn elem_path(&self) -> Vec<Elem<'_>> {
        self.path
            .iter()
            .map(|&i| {
                let n = &self.dom.nodes[i];
                Elem {
                    tag: n.tag.as_str(),
                    id: n.attr("id"),
                    classes: n.attr("class").map(|c| c.split_whitespace().collect()).unwrap_or_default(),
                    attrs: n.attrs.iter().map(|(k, _)| k.as_str()).collect(),
                }
            })
            .collect()
    }

    /// Converts the children of `n` in block context.
    fn flow(&mut self, n: usize, comp: &Computed, out: &mut Vec<Block>) {
        let mut inl: Vec<Inline> = Vec::new();
        let children = self.dom.nodes[n].children.clone();
        for ch in children {
            let node = &self.dom.nodes[ch];
            if node.tag.is_empty() {
                self.text(&node.text.clone(), comp, &mut inl);
                continue;
            }
            self.path.push(ch);
            let mut c = self.compute(ch, comp);
            match c.display {
                Display::None => {}
                Display::Inline => self.inline(ch, &mut c, &mut inl),
                Display::Block | Display::ListItem => {
                    flush(&mut inl, comp, out);
                    self.block(ch, &c, out);
                }
            }
            self.path.pop();
        }
        flush(&mut inl, comp, out);
    }

    fn text(&self, t: &str, comp: &Computed, inl: &mut Vec<Inline>) {
        if comp.pre {
            inl.push(Inline::Text(t.replace("\r\n", "\n").replace('\t', "    "), comp.style));
        } else {
            let mut s = String::with_capacity(t.len());
            let mut ws = false;
            for ch in t.chars() {
                if matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{C}') {
                    ws = true;
                } else {
                    if ws {
                        s.push(' ');
                        ws = false;
                    }
                    s.push(ch);
                }
            }
            if ws {
                s.push(' ');
            }
            if !s.is_empty() {
                inl.push(Inline::Text(s, comp.style));
            }
        }
    }

    /// Converts an inline element (and its subtree) into inline content.
    fn inline(&mut self, n: usize, comp: &mut Computed, inl: &mut Vec<Inline>) {
        let node = &self.dom.nodes[n];
        let tag = node.tag.clone();
        if let Some(id) = node.attr("id").or_else(|| if tag == "a" { node.attr("name") } else { None }) {
            inl.push(Inline::Anchor(id.to_string()));
        }
        if comp.style.link == u32::MAX {
            let href = node.attr("href").unwrap_or("").to_string();
            comp.style.link = self.add_link(&href);
        }
        match tag.as_str() {
            "br" => return inl.push(Inline::Break),
            "img" => {
                let node = &self.dom.nodes[n];
                let style_w = node.attr("style").and_then(|s| css::declarations(s).into_iter().rev().find(|(k, _)| k == "width").map(|(_, v)| v));
                inl.push(Inline::Image {
                    src: node.attr("src").unwrap_or("").to_string(),
                    alt: node.attr("alt").unwrap_or("").to_string(),
                    width: style_w.as_deref().or(node.attr("width")).and_then(length),
                    height: node.attr("height").and_then(length),
                    style: comp.style,
                });
                return;
            }
            "input" => {
                let ty = node.attr("type").unwrap_or("text").to_ascii_lowercase();
                let text = match ty.as_str() {
                    "checkbox" | "radio" => {
                        let on = node.attr("checked").is_some();
                        match (ty.as_str(), on) {
                            ("checkbox", true) => "☑ ",
                            ("checkbox", false) => "☐ ",
                            (_, true) => "◉ ",
                            _ => "○ ",
                        }
                        .to_string()
                    }
                    "submit" | "button" | "reset" => format!("[{}] ", node.attr("value").unwrap_or("")),
                    _ => match node.attr("value") {
                        Some(v) if !v.is_empty() => format!("[{v}] "),
                        _ => String::new(),
                    },
                };
                if !text.is_empty() {
                    inl.push(Inline::Text(text, comp.style));
                }
                return;
            }
            "q" => inl.push(Inline::Text("“".into(), comp.style)),
            _ => {}
        }
        let children = self.dom.nodes[n].children.clone();
        for ch in children {
            let node = &self.dom.nodes[ch];
            if node.tag.is_empty() {
                self.text(&node.text.clone(), comp, inl);
                continue;
            }
            self.path.push(ch);
            let mut c = self.compute(ch, comp);
            match c.display {
                Display::None => {}
                Display::Inline => self.inline(ch, &mut c, inl),
                // A block inside inline content: put it on its own line.
                _ => {
                    inl.push(Inline::Break);
                    let mut blocks = Vec::new();
                    self.block(ch, &c, &mut blocks);
                    for b in blocks {
                        flatten(b, inl);
                    }
                    inl.push(Inline::Break);
                }
            }
            self.path.pop();
        }
        if tag == "q" {
            inl.push(Inline::Text("”".into(), comp.style));
        }
    }

    fn block(&mut self, n: usize, comp: &Computed, out: &mut Vec<Block>) {
        let mut own = *comp;
        if own.style.link == u32::MAX {
            let href = self.dom.nodes[n].attr("href").unwrap_or("").to_string();
            own.style.link = self.add_link(&href);
        }
        let comp = &own;
        let node = &self.dom.nodes[n];
        let tag = node.tag.clone();
        let anchor = node.attr("id").map(str::to_string);
        if tag == "body" || tag == "html" {
            if comp.bg.is_some() {
                self.page_bg = comp.bg;
            }
            if comp.style.color.is_some() {
                self.page_color = comp.style.color;
            }
        }
        let mut b = Block { align: comp.align, bg: comp.bg, anchor, ..Block::default() };
        match tag.as_str() {
            "hr" => {
                b.kind = Kind::Rule;
                b.margin_top = 0.5;
                b.margin_bottom = 0.5;
            }
            "ul" | "ol" | "menu" | "dir" => {
                let start = self.dom.nodes[n].attr("start").and_then(|s| s.trim().parse().ok()).unwrap_or(1);
                let mut list = List { ordered: tag == "ol", start, items: Vec::new() };
                let children = self.dom.nodes[n].children.clone();
                for ch in children {
                    let node = &self.dom.nodes[ch];
                    if node.tag.is_empty() {
                        if node.text.trim().is_empty() {
                            continue;
                        }
                        let mut inl = Vec::new();
                        self.text(&node.text.clone(), comp, &mut inl);
                        list.items.push(ListItem { blocks: vec![Block::para(inl)], checked: None });
                        continue;
                    }
                    self.path.push(ch);
                    let c = self.compute(ch, comp);
                    if c.display != Display::None {
                        let mut blocks = Vec::new();
                        if node.tag == "li" {
                            self.flow(ch, &c, &mut blocks);
                            for bl in &mut blocks {
                                bl.margin_bottom = bl.margin_bottom.min(0.3);
                            }
                            list.items.push(ListItem { blocks, checked: None });
                        } else {
                            self.block(ch, &c, &mut blocks);
                            match list.items.last_mut() {
                                Some(item) => item.blocks.extend(blocks),
                                None => list.items.push(ListItem { blocks, checked: None }),
                            }
                        }
                    }
                    self.path.pop();
                }
                b.kind = Kind::List(list);
                b.margin_top = 0.6;
                b.margin_bottom = 0.6;
            }
            "table" => {
                let mut table = Table::default();
                let mut caption = Vec::new();
                self.table_rows(n, comp, &mut table, &mut caption);
                for c in caption {
                    out.push(c);
                }
                b.kind = Kind::Table(table);
                b.margin_bottom = 0.6;
            }
            "pre" | "listing" | "xmp" | "plaintext" | "textarea" => {
                let mut inl = Vec::new();
                let mut c = *comp;
                c.pre = true;
                self.inline(n, &mut c, &mut inl);
                // A leading newline right after <pre> is ignored.
                if let Some(Inline::Text(t, _)) = inl.iter_mut().find(|i| matches!(i, Inline::Text(..))) {
                    if let Some(rest) = t.strip_prefix('\n') {
                        *t = rest.to_string();
                    }
                }
                b.kind = Kind::Pre(inl);
                b.margin_top = 1.0;
                b.margin_bottom = 1.0;
                b.padding = if comp.bg.is_some() { 0.5 } else { 0.0 };
            }
            _ => {
                let mut blocks = Vec::new();
                self.flow(n, comp, &mut blocks);
                let (mt, mb) = match tag.as_str() {
                    "p" | "dl" | "figure" => (1.0, 1.0),
                    "blockquote" => (1.0, 1.0),
                    "li" => (0.0, 0.3),
                    _ if heading_level(&tag).is_some() => (0.8, 0.5),
                    _ => (0.0, 0.0),
                };
                b.margin_top = mt;
                b.margin_bottom = mb;
                if matches!(tag.as_str(), "body" | "html") {
                    // The page background is drawn separately.
                    out.extend(blocks);
                    return;
                }
                if matches!(tag.as_str(), "blockquote" | "figure" | "dd") {
                    b.indent = 2.0;
                }
                if tag == "li" {
                    // A list item outside a list: show it with a bullet.
                    let list = List { ordered: false, start: 1, items: vec![ListItem { blocks, checked: None }] };
                    b.kind = Kind::List(list);
                } else if blocks.len() == 1 && matches!(blocks[0].kind, Kind::Para(_)) && b.bg.is_none() {
                    let only = blocks.pop().expect("one block");
                    b.kind = only.kind;
                    if b.anchor.is_none() {
                        b.anchor = only.anchor;
                    }
                } else {
                    if blocks.is_empty() {
                        if b.anchor.is_some() {
                            b.kind = Kind::Empty;
                            out.push(b);
                        }
                        return;
                    }
                    if b.bg.is_some() {
                        b.padding = 0.5;
                    }
                    b.kind = Kind::Group(blocks);
                }
            }
        }
        out.push(b);
    }

    fn table_rows(&mut self, n: usize, comp: &Computed, table: &mut Table, caption: &mut Vec<Block>) {
        let children = self.dom.nodes[n].children.clone();
        for ch in children {
            let tag = self.dom.nodes[ch].tag.clone();
            if tag.is_empty() {
                continue;
            }
            self.path.push(ch);
            let c = self.compute(ch, comp);
            if c.display != Display::None {
                match tag.as_str() {
                    "thead" | "tbody" | "tfoot" => self.table_rows(ch, &c, table, caption),
                    "tr" => {
                        let mut row = Row::default();
                        let cells = self.dom.nodes[ch].children.clone();
                        for cell in cells {
                            let ctag = self.dom.nodes[cell].tag.clone();
                            if ctag != "td" && ctag != "th" {
                                continue;
                            }
                            self.path.push(cell);
                            let cc = self.compute(cell, &c);
                            let mut blocks = Vec::new();
                            self.flow(cell, &cc, &mut blocks);
                            for bl in &mut blocks {
                                bl.margin_bottom = bl.margin_bottom.min(0.4);
                                bl.margin_top = bl.margin_top.min(0.4);
                            }
                            let colspan = self.dom.nodes[cell].attr("colspan").and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, 64);
                            row.cells.push(Cell { blocks, header: ctag == "th", align: cc.align, colspan, bg: cc.bg.or(c.bg) });
                            self.path.pop();
                        }
                        table.rows.push(row);
                    }
                    "caption" => self.flow(ch, &c, caption),
                    _ => {}
                }
            }
            self.path.pop();
        }
    }
}

fn flush(inl: &mut Vec<Inline>, comp: &Computed, out: &mut Vec<Block>) {
    if inl.is_empty() {
        return;
    }
    let v = std::mem::take(inl);
    let only_space = v.iter().all(|i| matches!(i, Inline::Text(t, _) if t.trim().is_empty()));
    if only_space {
        return;
    }
    out.push(Block { kind: Kind::Para(v), align: comp.align, ..Block::default() });
}

fn flatten(b: Block, inl: &mut Vec<Inline>) {
    match b.kind {
        Kind::Para(v) | Kind::Pre(v) => inl.extend(v),
        Kind::Group(g) => g.into_iter().for_each(|b| flatten(b, inl)),
        Kind::List(l) => l.items.into_iter().flat_map(|i| i.blocks).for_each(|b| flatten(b, inl)),
        _ => {}
    }
}

fn find_tag(dom: &Dom, n: usize, tag: &str, out: &mut Vec<usize>) {
    for &c in &dom.nodes[n].children {
        if dom.nodes[c].tag == tag {
            out.push(c);
        }
        find_tag(dom, c, tag, out);
    }
}

/// Converts a full HTML document. `base` resolves local style sheets.
pub fn parse(src: &str, base: Option<&Path>) -> RichDoc {
    let dom = parse_dom(src);
    let mut sheet = Sheet::default();
    let mut found = Vec::new();
    find_tag(&dom, 0, "style", &mut found);
    find_tag(&dom, 0, "link", &mut found);
    found.sort_unstable();
    for n in found {
        let node = &dom.nodes[n];
        if node.tag == "style" {
            if node.attr("media").is_none_or(|m| m.contains("screen") || m.contains("all")) {
                for &c in &node.children {
                    sheet.add(&dom.nodes[c].text);
                }
            }
        } else if node.attr("rel").is_some_and(|r| r.eq_ignore_ascii_case("stylesheet")) {
            // Only local style sheets next to the document; never the network.
            if let (Some(href), Some(base)) = (node.attr("href"), base) {
                if !href.contains("://") && !href.starts_with("//") {
                    let path = base.join(href.split(['?', '#']).next().unwrap_or(href));
                    if std::fs::metadata(&path).is_ok_and(|m| m.len() < 2 << 20) {
                        if let Ok(css) = std::fs::read_to_string(&path) {
                            sheet.add(&css);
                        }
                    }
                }
            }
        }
    }
    let mut titles = Vec::new();
    find_tag(&dom, 0, "title", &mut titles);
    let title = titles.first().and_then(|&t| dom.nodes[t].children.first()).map(|&c| dom.nodes[c].text.split_whitespace().collect::<Vec<_>>().join(" "));
    let mut conv = Converter { dom: &dom, sheet, links: Vec::new(), title, page_bg: None, page_color: None, path: Vec::new() };
    let root = Computed { style: Style::default(), align: Align::Left, pre: false, display: Display::Block, bg: None };
    let mut blocks = Vec::new();
    conv.flow(0, &root, &mut blocks);
    RichDoc { blocks, links: conv.links, title: conv.title, page_bg: conv.page_bg, page_color: conv.page_color, ..RichDoc::default() }
}

/// Converts an HTML fragment (embedded in Markdown); links are appended to `links`.
pub fn fragment(src: &str, links: &mut Vec<String>) -> Vec<Block> {
    let dom = parse_dom(src);
    let mut conv = Converter { dom: &dom, sheet: Sheet::default(), links: std::mem::take(links), title: None, page_bg: None, page_color: None, path: Vec::new() };
    let root = Computed { style: Style::default(), align: Align::Left, pre: false, display: Display::Block, bg: None };
    let mut blocks = Vec::new();
    conv.flow(0, &root, &mut blocks);
    *links = conv.links;
    for b in &mut blocks {
        if b.margin_bottom == 0.0 {
            b.margin_bottom = 0.8;
        }
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich::plain_text;

    #[test]
    fn entities() {
        assert_eq!(decode_entities("a &amp; b &lt;c&gt; &eacute;&#233;&#xE9; &copy; &bogus; & x"), "a & b <c> ééé © &bogus; & x");
        assert_eq!(decode_entities("&#150;"), "–");
    }

    #[test]
    fn tolerant_tree() {
        let d = parse_dom("<p>one<p>two<ul><li>a<li>b</ul><table><tr><td>1<td>2<tr><td>3</table><br><img src=x.png>");
        let tags: Vec<&str> = d.nodes[0].children.iter().map(|&c| d.nodes[c].tag.as_str()).collect();
        assert_eq!(tags, ["p", "p", "ul", "table", "br", "img"]);
        let ul = d.nodes[0].children[2];
        assert_eq!(d.nodes[ul].children.len(), 2);
        let table = d.nodes[0].children[3];
        assert_eq!(d.nodes[table].children.len(), 2, "two rows");
    }

    #[test]
    fn script_and_style_are_raw() {
        let doc = parse("<html><head><title>T &amp; t</title><style>p{color:#00f} .x{display:none}</style><script>if (a<b) { document.write('<p>no</p>') }</script></head><body><p>yes</p><p class=x>hidden</p></body></html>", None);
        assert_eq!(doc.title.as_deref(), Some("T & t"));
        let text = plain_text(&doc.blocks);
        assert!(text.contains("yes"));
        assert!(!text.contains("no") && !text.contains("hidden") && !text.contains("document"), "{text}");
        let Kind::Para(v) = &doc.blocks[0].kind else { panic!("{:?}", doc.blocks[0]) };
        let Inline::Text(_, s) = &v[0] else { panic!() };
        assert_eq!(s.color, Some(Color::rgb(0x0000FF)));
    }

    #[test]
    fn blocks_lists_tables_links() {
        let doc = parse(
            "<body bgcolor=#eeeeee><h1 id=top>Title</h1><p>Hello <b>bold</b> <a href='#top'>up</a></p>\
             <ol start=3><li>three</li><li>four</li></ol><table><tr><th>A</th><th colspan=2>B</th></tr><tr><td>1</td><td>2</td><td>3</td></tr></table>\
             <pre>\n  x  y\n</pre><div style='text-align:center'>c</div></body>",
            None,
        );
        assert_eq!(doc.page_bg, Some(Color::rgb(0xEEEEEE)));
        assert_eq!(doc.links, vec!["#top".to_string()]);
        assert_eq!(doc.blocks[0].anchor.as_deref(), Some("top"));
        let Kind::List(l) = &doc.blocks[2].kind else { panic!("{:?}", doc.blocks[2].kind) };
        assert!(l.ordered && l.start == 3 && l.items.len() == 2);
        let Kind::Table(t) = &doc.blocks[3].kind else { panic!() };
        assert_eq!(t.rows[0].cells[1].colspan, 2);
        assert!(t.rows[0].cells[0].header);
        let Kind::Pre(v) = &doc.blocks[4].kind else { panic!() };
        assert_eq!(v, &vec![Inline::Text("  x  y\n".into(), Style { mono: true, ..Style::default() })]);
        assert_eq!(doc.blocks[5].align, Align::Center);
    }

    #[test]
    fn whitespace_collapses_between_inline_elements() {
        let doc = parse("<p>a\n   <i>b</i>c  d</p>", None);
        assert_eq!(plain_text(&doc.blocks).trim(), "a bc d");
    }

    #[test]
    fn charset_detection() {
        let (t, e) = decode(b"<meta charset=\"windows-1252\"><p>caf\xE9</p>");
        assert!(t.contains("café"));
        assert_eq!(e, "windows-1252");
        let (t, _) = decode("<p>ok ü</p>".as_bytes());
        assert!(t.contains('ü'));
    }
}
