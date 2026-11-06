//! A small CSS subset for document HTML: colors, font weight/style/size/
//! family, text alignment and decoration, display, white-space. Selectors
//! support tags, classes, ids, attributes and descendant/child combinators
//! with specificity and source order; pseudo-classes other than `:link`
//! never match, and `@media` blocks apply only for `screen`/`all`.

use crate::ui::gfx::Color;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Compound {
    pub tag: Option<String>,
    pub ids: Vec<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<String>,
    pub never: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
}

/// Compounds left to right; `comb[i]` joins `parts[i]` and `parts[i + 1]`.
#[derive(Clone, Debug, Default)]
pub struct Selector {
    pub parts: Vec<Compound>,
    pub comb: Vec<Combinator>,
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub sel: Selector,
    pub spec: (u16, u16, u16),
    pub order: usize,
    pub decls: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub rules: Vec<Rule>,
}

/// What a selector needs to know about an element.
pub struct Elem<'a> {
    pub tag: &'a str,
    pub id: Option<&'a str>,
    pub classes: Vec<&'a str>,
    pub attrs: Vec<&'a str>,
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        rest = match rest[i + 2..].find("*/") {
            Some(j) => &rest[i + 2 + j + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// Parses `a: b; c: d` into lowercase property names and trimmed values.
pub fn declarations(s: &str) -> Vec<(String, String)> {
    s.split(';')
        .filter_map(|d| {
            let (k, v) = d.split_once(':')?;
            let v = v.trim().trim_end_matches("!important").trim();
            Some((k.trim().to_ascii_lowercase(), v.to_string()))
        })
        .filter(|(k, v)| !k.is_empty() && !v.is_empty())
        .collect()
}

fn compound(s: &str) -> Compound {
    let mut c = Compound::default();
    let mut rest = s;
    // Tag or universal selector.
    let tag_end = rest.find(['.', '#', '[', ':']).unwrap_or(rest.len());
    let tag = &rest[..tag_end];
    if !tag.is_empty() && tag != "*" {
        c.tag = Some(tag.to_ascii_lowercase());
    }
    rest = &rest[tag_end..];
    while !rest.is_empty() {
        let kind = rest.as_bytes()[0];
        let body = &rest[1..];
        let end = if kind == b'[' { body.find(']').map_or(body.len(), |e| e + 1) } else { body.find(['.', '#', '[', ':']).unwrap_or(body.len()) };
        let token = &body[..end.min(body.len())];
        match kind {
            b'.' => c.classes.push(token.to_string()),
            b'#' => c.ids.push(token.to_string()),
            b'[' => c.attrs.push(token.trim_end_matches(']').split(['=', '~', '|', '^', '$', '*']).next().unwrap_or("").trim().to_ascii_lowercase()),
            b':' => {
                let p = token.trim_start_matches(':').to_ascii_lowercase();
                if !matches!(p.as_str(), "link" | "visited" | "root") {
                    c.never = true;
                }
            }
            _ => c.never = true,
        }
        rest = &body[end.min(body.len())..];
    }
    c
}

fn selector(s: &str) -> Option<Selector> {
    let s = s.trim();
    if s.is_empty() || s.contains(['+', '~']) && !s.contains('[') {
        return None;
    }
    let mut sel = Selector::default();
    let spaced = s.replace('>', " > ");
    let mut pending = Combinator::Descendant;
    for tok in spaced.split_whitespace() {
        if tok == ">" {
            pending = Combinator::Child;
            continue;
        }
        if !sel.parts.is_empty() {
            sel.comb.push(pending);
        }
        sel.parts.push(compound(tok));
        pending = Combinator::Descendant;
    }
    (!sel.parts.is_empty()).then_some(sel)
}

fn specificity(sel: &Selector) -> (u16, u16, u16) {
    sel.parts.iter().fold((0, 0, 0), |(a, b, c), p| {
        (a + p.ids.len() as u16, b + (p.classes.len() + p.attrs.len()) as u16, c + u16::from(p.tag.is_some()))
    })
}

impl Sheet {
    /// Adds the rules of a style sheet.
    pub fn add(&mut self, css: &str) {
        let css = strip_comments(css);
        self.add_rules(&css);
    }

    fn add_rules(&mut self, css: &str) {
        let mut rest = css;
        while let Some(open) = rest.find('{') {
            let head = rest[..open].trim();
            // Find the matching close brace (nested for @media).
            let mut depth = 0;
            let mut close = None;
            for (i, ch) in rest[open..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            close = Some(open + i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let close = close.unwrap_or(rest.len());
            let body = &rest[open + 1..close.min(rest.len())];
            // Statements like `@import url(x);` before the block.
            let head = head.rsplit(';').next().unwrap_or(head).trim();
            if let Some(at) = head.strip_prefix('@') {
                let q = at.to_ascii_lowercase();
                if q.starts_with("media") {
                    let query = q.trim_start_matches("media").trim();
                    let applies = query.split(',').any(|m| {
                        let m = m.trim();
                        (m.is_empty() || m.starts_with("screen") || m.starts_with("all")) && !m.contains("width") && !m.contains("prefers-color-scheme: dark")
                    });
                    if applies {
                        self.add_rules(body);
                    }
                }
            } else {
                let decls = declarations(body);
                if !decls.is_empty() {
                    for s in head.split(',') {
                        if let Some(sel) = selector(s) {
                            let spec = specificity(&sel);
                            let order = self.rules.len();
                            self.rules.push(Rule { sel, spec, order, decls: decls.clone() });
                        }
                    }
                }
            }
            rest = if close < rest.len() { &rest[close + 1..] } else { "" };
        }
    }

    /// Declarations that apply to the last element of `path` (root first),
    /// in cascade order (later wins).
    pub fn cascade(&self, path: &[Elem]) -> Vec<(String, String)> {
        let mut hits: Vec<&Rule> = self.rules.iter().filter(|r| matches(&r.sel, path)).collect();
        hits.sort_by_key(|r| (r.spec, r.order));
        hits.into_iter().flat_map(|r| r.decls.iter().cloned()).collect()
    }
}

fn matches_compound(c: &Compound, e: &Elem) -> bool {
    !c.never
        && c.tag.as_deref().is_none_or(|t| t == e.tag)
        && c.ids.iter().all(|i| e.id == Some(i.as_str()))
        && c.classes.iter().all(|k| e.classes.contains(&k.as_str()))
        && c.attrs.iter().all(|a| e.attrs.contains(&a.as_str()))
}

fn matches(sel: &Selector, path: &[Elem]) -> bool {
    let Some((last, ancestors)) = path.split_last() else { return false };
    let n = sel.parts.len();
    if !matches_compound(&sel.parts[n - 1], last) {
        return false;
    }
    // Match the remaining compounds right to left against ancestors.
    fn rest(sel: &Selector, k: usize, anc: &[Elem]) -> bool {
        if k == 0 {
            return true;
        }
        let part = &sel.parts[k - 1];
        match sel.comb[k - 1] {
            Combinator::Child => match anc.split_last() {
                Some((p, up)) => matches_compound(part, p) && rest(sel, k - 1, up),
                None => false,
            },
            Combinator::Descendant => {
                for i in (0..anc.len()).rev() {
                    if matches_compound(part, &anc[i]) && rest(sel, k - 1, &anc[..i]) {
                        return true;
                    }
                }
                false
            }
        }
    }
    rest(sel, n - 1, ancestors)
}

const NAMED: &[(&str, u32)] = &[
    ("black", 0x000000), ("silver", 0xC0C0C0), ("gray", 0x808080), ("grey", 0x808080), ("white", 0xFFFFFF), ("maroon", 0x800000),
    ("red", 0xFF0000), ("purple", 0x800080), ("fuchsia", 0xFF00FF), ("magenta", 0xFF00FF), ("green", 0x008000), ("lime", 0x00FF00),
    ("olive", 0x808000), ("yellow", 0xFFFF00), ("navy", 0x000080), ("blue", 0x0000FF), ("teal", 0x008080), ("aqua", 0x00FFFF),
    ("cyan", 0x00FFFF), ("orange", 0xFFA500), ("darkgray", 0xA9A9A9), ("darkgrey", 0xA9A9A9), ("lightgray", 0xD3D3D3),
    ("lightgrey", 0xD3D3D3), ("gainsboro", 0xDCDCDC), ("whitesmoke", 0xF5F5F5), ("dimgray", 0x696969), ("dimgrey", 0x696969),
    ("darkred", 0x8B0000), ("darkgreen", 0x006400), ("darkblue", 0x00008B), ("darkorange", 0xFF8C00), ("brown", 0xA52A2A),
    ("pink", 0xFFC0CB), ("gold", 0xFFD700), ("indigo", 0x4B0082), ("violet", 0xEE82EE), ("crimson", 0xDC143C), ("coral", 0xFF7F50),
    ("tomato", 0xFF6347), ("salmon", 0xFA8072), ("khaki", 0xF0E68C), ("beige", 0xF5F5DC), ("ivory", 0xFFFFF0), ("lavender", 0xE6E6FA),
    ("steelblue", 0x4682B4), ("royalblue", 0x4169E1), ("skyblue", 0x87CEEB), ("lightblue", 0xADD8E6), ("dodgerblue", 0x1E90FF),
    ("seagreen", 0x2E8B57), ("forestgreen", 0x228B22), ("lightgreen", 0x90EE90), ("lightyellow", 0xFFFFE0), ("aliceblue", 0xF0F8FF),
    ("slategray", 0x708090), ("darkslategray", 0x2F4F4F), ("midnightblue", 0x191970), ("firebrick", 0xB22222), ("chocolate", 0xD2691E),
    ("tan", 0xD2B48C), ("wheat", 0xF5DEB3), ("linen", 0xFAF0E6), ("honeydew", 0xF0FFF0), ("mintcream", 0xF5FFFA), ("snow", 0xFFFAFA),
    ("orchid", 0xDA70D6), ("plum", 0xDDA0DD), ("turquoise", 0x40E0D0), ("cornflowerblue", 0x6495ED), ("darkviolet", 0x9400D3),
];

/// Parses a CSS color. Translucent colors are blended over white.
pub fn color(v: &str) -> Option<Color> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(h) = v.strip_prefix('#') {
        let hex = |s: &str| u32::from_str_radix(s, 16).ok();
        let (rgb, a) = match h.len() {
            3 | 4 => {
                let d: Vec<u32> = h.chars().filter_map(|c| c.to_digit(16)).map(|d| d * 17).collect();
                if d.len() != h.len() {
                    return None;
                }
                ((d[0] << 16) | (d[1] << 8) | d[2], d.get(3).copied().unwrap_or(255))
            }
            6 => (hex(h)?, 255),
            8 => (hex(&h[..6])?, hex(&h[6..])?),
            _ => return None,
        };
        return Some(blend(rgb, a as f32 / 255.0));
    }
    if let Some(args) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")) {
        let nums: Vec<f32> = args
            .trim_end_matches(')')
            .split([',', ' ', '/'])
            .filter(|s| !s.is_empty())
            .filter_map(|s| if let Some(p) = s.strip_suffix('%') { p.parse::<f32>().ok().map(|x| x * 2.55) } else { s.parse().ok() })
            .collect();
        if nums.len() < 3 {
            return None;
        }
        let c = |x: f32| x.clamp(0.0, 255.0) as u32;
        let a = nums.get(3).map_or(1.0, |a| if *a > 1.0 { a / 255.0 } else { *a });
        return Some(blend((c(nums[0]) << 16) | (c(nums[1]) << 8) | c(nums[2]), a));
    }
    if v == "transparent" {
        return None;
    }
    NAMED.iter().find(|(n, _)| *n == v).map(|(_, c)| Color::rgb(*c))
}

fn blend(rgb: u32, a: f32) -> Color {
    if a >= 0.999 {
        return Color::rgb(rgb);
    }
    Color::rgb(rgb).mix(Color::rgb(0xFFFFFF), 1.0 - a.clamp(0.0, 1.0))
}

/// Font size relative to the parent size (`parent`) and root (1.0), in em.
pub fn font_size(v: &str, parent: f32) -> Option<f32> {
    let v = v.trim().to_ascii_lowercase();
    let num = |s: &str| s.trim().parse::<f32>().ok();
    Some(match v.as_str() {
        "xx-small" => 0.6,
        "x-small" => 0.75,
        "small" => 0.89,
        "medium" => 1.0,
        "large" => 1.2,
        "x-large" => 1.5,
        "xx-large" => 2.0,
        "xxx-large" => 3.0,
        "smaller" => parent * 0.83,
        "larger" => parent * 1.2,
        _ => {
            if let Some(n) = v.strip_suffix("px") {
                num(n)? / 16.0
            } else if let Some(n) = v.strip_suffix("rem") {
                num(n)?
            } else if let Some(n) = v.strip_suffix("em") {
                num(n)? * parent
            } else if let Some(n) = v.strip_suffix("pt") {
                num(n)? / 12.0
            } else {
                num(v.strip_suffix('%')?)? / 100.0 * parent
            }
        }
    })
    .map(|s| s.clamp(0.4, 6.0))
}

/// Whether a font-family list asks for a monospace font.
pub fn is_mono_family(v: &str) -> bool {
    let v = v.to_ascii_lowercase();
    ["monospace", "mono", "courier", "consolas", "menlo", "monaco"].iter().any(|k| v.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el<'a>(tag: &'a str, id: Option<&'a str>, classes: &[&'a str]) -> Elem<'a> {
        Elem { tag, id, classes: classes.to_vec(), attrs: Vec::new() }
    }

    #[test]
    fn colors() {
        assert_eq!(color("#f00"), Some(Color::rgb(0xFF0000)));
        assert_eq!(color("#00ff7f"), Some(Color::rgb(0x00FF7F)));
        assert_eq!(color("rgb(10, 20, 30)"), Some(Color::rgb(0x0A141E)));
        assert_eq!(color("Navy"), Some(Color::rgb(0x000080)));
        assert_eq!(color("transparent"), None);
        assert_eq!(color("bogus"), None);
    }

    #[test]
    fn cascade_and_specificity() {
        let mut s = Sheet::default();
        s.add("/* c */ p { color: red } .note p { color: blue } #x { color: green } @media print { p { color: black } } div > p.k { font-weight: bold } a:hover { color: pink }");
        let path = [el("html", None, &[]), el("div", None, &["note"]), el("p", Some("x"), &["k"])];
        let c = s.cascade(&path);
        assert_eq!(c.iter().rev().find(|d| d.0 == "color").map(|d| d.1.as_str()), Some("green"));
        assert!(c.contains(&("font-weight".into(), "bold".into())));
        let plain = [el("body", None, &[]), el("p", None, &[])];
        let c = s.cascade(&plain);
        assert_eq!(c, vec![("color".to_string(), "red".to_string())]);
        let link = [el("a", None, &[])];
        assert!(s.cascade(&link).is_empty());
    }

    #[test]
    fn sizes() {
        assert_eq!(font_size("24px", 1.0), Some(1.5));
        assert_eq!(font_size("2em", 1.5), Some(3.0));
        assert_eq!(font_size("150%", 1.0), Some(1.5));
        assert_eq!(font_size("large", 1.0), Some(1.2));
        assert_eq!(font_size("12pt", 1.0), Some(1.0));
        assert_eq!(font_size("auto", 1.0), None);
    }
}
