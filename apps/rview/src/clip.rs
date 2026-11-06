//! What goes to the clipboard. Each document type describes its selection
//! as one neutral stream of events; the same stream is then written as plain
//! text, HTML and RTF, so the three always hold the same content. Nothing
//! here touches Windows (see `win::clipboard`).

use crate::media::Bitmap;
use crate::rich::Align;
use crate::ui::gfx::Color;

/// Size of body text in copied documents; `Fmt::size` is relative to it.
const BODY_PT: f32 = 11.0;
const SANS: &str = "Segoe UI";
const MONO: &str = "Consolas";
/// Width given to tables in RTF, in twips (6.25 in).
const RTF_TABLE_WIDTH: usize = 9000;
const RTF_INDENT: usize = 360;

/// Character formatting of a run. Colors are the document's own, never the
/// ones adapted for a dark page: pasted text lands on a light page.
#[derive(Clone, Debug, PartialEq)]
pub struct Fmt {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub mono: bool,
    /// Relative to the body size.
    pub size: f32,
    pub color: Option<Color>,
    pub bg: Option<Color>,
    pub link: Option<String>,
    /// -1 subscript, +1 superscript.
    pub valign: i8,
}

impl Default for Fmt {
    fn default() -> Fmt {
        Fmt { bold: false, italic: false, underline: false, strike: false, mono: false, size: 1.0, color: None, bg: None, link: None, valign: 0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    /// `heading` is 1 to 6, or 0 for body text.
    Para { align: Align, pre: bool, heading: u8, bg: Option<Color> },
    Quote,
    List { ordered: bool, start: u32 },
    Item,
    Table,
    Row,
    Cell { header: bool, colspan: usize, align: Align, bg: Option<Color> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ev {
    Enter(Tag),
    /// Closes the latest open `Enter`.
    Exit,
    Run(String, Fmt),
    /// A list marker: text for plain text and RTF; HTML lists draw their own.
    Marker(String),
    /// A hard line break.
    Br,
    /// A line break that only exists in the source layout: kept in plain
    /// text, a space in formatted text.
    Soft,
    Image { src: String, alt: String },
}

/// Clipboard contents, as ready-to-write formats.
#[derive(Default)]
pub struct Payload {
    /// Plain text with `\n` line breaks.
    pub text: String,
    /// A complete `HTML Format` clipboard document.
    pub html: Option<String>,
    pub rtf: Option<String>,
    pub image: Option<Bitmap>,
    /// Formats were dropped because the selection is very large.
    pub too_large: bool,
}

// ------------------------------------------------------------ plain text

/// The text of `evs`. Paragraphs are separated by a blank line, list items
/// and table rows by a line break, table cells by a tab.
pub fn plain(evs: &[Ev]) -> String {
    #[derive(Clone, Copy, PartialEq, PartialOrd)]
    enum Gap {
        None,
        Line,
        Blank,
        Tab,
    }
    let mut out = String::new();
    let mut stack: Vec<&Tag> = Vec::new();
    let mut gap = Gap::None;
    let flush = |out: &mut String, gap: &mut Gap| {
        if !out.is_empty() {
            out.push_str(match *gap {
                Gap::None => "",
                Gap::Line => "\n",
                Gap::Blank => "\n\n",
                Gap::Tab => "\t",
            });
        }
        *gap = Gap::None;
    };
    for ev in evs {
        match ev {
            Ev::Enter(t) => stack.push(t),
            Ev::Exit => {
                let Some(t) = stack.pop() else { continue };
                let nested = stack.iter().any(|t| matches!(t, Tag::Item | Tag::Cell { .. }));
                gap = match t {
                    Tag::Cell { .. } => Gap::Tab,
                    Tag::Row => Gap::Line,
                    Tag::Item => Gap::Line.max_with(gap),
                    Tag::Para { .. } | Tag::Quote | Tag::List { .. } | Tag::Table if nested => Gap::Line.max_with(gap),
                    _ => Gap::Blank.max_with(gap),
                };
            }
            Ev::Run(t, _) => {
                flush(&mut out, &mut gap);
                out.push_str(t);
            }
            Ev::Marker(m) => {
                flush(&mut out, &mut gap);
                let depth = stack.iter().filter(|t| matches!(t, Tag::List { .. })).count().saturating_sub(1);
                out.push_str(&"  ".repeat(depth));
                out.push_str(m);
                out.push(' ');
            }
            Ev::Br | Ev::Soft => {
                flush(&mut out, &mut gap);
                out.push('\n');
            }
            Ev::Image { alt, .. } => {
                if !alt.is_empty() {
                    flush(&mut out, &mut gap);
                    out.push('[');
                    out.push_str(alt);
                    out.push(']');
                }
            }
        }
    }
    trait MaxWith {
        fn max_with(self, o: Self) -> Self;
    }
    impl MaxWith for Gap {
        /// The stronger gap; a pending cell tab stays until its row ends.
        fn max_with(self, o: Gap) -> Gap {
            if o > self {
                o
            } else {
                self
            }
        }
    }
    out
}

// ------------------------------------------------------------ HTML

fn hex(c: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

fn esc_html(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '\n' => out.push_str("<br>"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn align_css(a: Align) -> &'static str {
    match a {
        Align::Left => "",
        Align::Center => "text-align:center;",
        Align::Right => "text-align:right;",
    }
}

fn pt(size: f32) -> f32 {
    (size * BODY_PT * 2.0).round() / 2.0
}

fn run_html(text: &str, f: &Fmt, out: &mut String) {
    let mut css = String::new();
    if f.bold {
        css.push_str("font-weight:bold;");
    }
    if f.italic {
        css.push_str("font-style:italic;");
    }
    if f.mono {
        css.push_str(&format!("font-family:{MONO},monospace;"));
    }
    match (f.underline, f.strike) {
        (true, true) => css.push_str("text-decoration:underline line-through;"),
        (true, false) => css.push_str("text-decoration:underline;"),
        (false, true) => css.push_str("text-decoration:line-through;"),
        _ => {}
    }
    if (f.size - 1.0).abs() > 0.01 {
        css.push_str(&format!("font-size:{}pt;", pt(f.size)));
    }
    if let Some(c) = f.color {
        css.push_str(&format!("color:{};", hex(c)));
    }
    if let Some(c) = f.bg {
        css.push_str(&format!("background-color:{};", hex(c)));
    }
    if let Some(l) = &f.link {
        out.push_str("<a href=\"");
        esc_html(l, out);
        out.push_str("\">");
    }
    match f.valign {
        1 => out.push_str("<sup>"),
        -1 => out.push_str("<sub>"),
        _ => {}
    }
    if css.is_empty() {
        esc_html(text, out);
    } else {
        out.push_str("<span style=\"");
        out.push_str(&css);
        out.push_str("\">");
        esc_html(text, out);
        out.push_str("</span>");
    }
    match f.valign {
        1 => out.push_str("</sup>"),
        -1 => out.push_str("</sub>"),
        _ => {}
    }
    if f.link.is_some() {
        out.push_str("</a>");
    }
}

/// Adds a run, joining it to the previous one when the format is the same.
pub fn push_run(evs: &mut Vec<Ev>, text: &str, fmt: Fmt) {
    if let Some(Ev::Run(t, f)) = evs.last_mut() {
        if *f == fmt {
            t.push_str(text);
            return;
        }
    }
    evs.push(Ev::Run(text.to_string(), fmt));
}

/// The selection as an HTML fragment. Plain paragraphs in list items and
/// table cells get no `<p>`: word processors would give them margins.
pub fn html(evs: &[Ev]) -> String {
    let mut out = String::new();
    let mut close: Vec<String> = Vec::new();
    // A bare paragraph just ended here; the next one needs a line break.
    let mut after_bare = false;
    for ev in evs {
        match ev {
            Ev::Enter(t) => {
                let bare = matches!(t, Tag::Para { align: Align::Left, pre: false, heading: 0, bg: None })
                    || matches!(t, Tag::Para { pre: false, heading: 0, bg: None, .. }) && close.last().is_some_and(|n| n == "td" || n == "th");
                if bare && close.last().is_some_and(|n| matches!(n.as_str(), "li" | "td" | "th")) {
                    if std::mem::take(&mut after_bare) {
                        out.push_str("<br>");
                    }
                    close.push(String::new());
                    continue;
                }
                after_bare = false;
                let (name, attrs) = match t {
                    Tag::Para { pre: true, bg, align, .. } => {
                        let bg = bg.map_or(String::new(), |c| format!("background-color:{};padding:6pt;", hex(c)));
                        ("pre".to_string(), format!(" style=\"font-family:{MONO},monospace;{}{bg}\"", align_css(*align)))
                    }
                    Tag::Para { align, heading, bg, .. } => {
                        let name = if *heading > 0 { format!("h{}", heading.min(&6)) } else { "p".to_string() };
                        let mut css = align_css(*align).to_string();
                        if let Some(c) = bg {
                            css.push_str(&format!("background-color:{};", hex(*c)));
                        }
                        (name, if css.is_empty() { String::new() } else { format!(" style=\"{css}\"") })
                    }
                    Tag::Quote => ("blockquote".to_string(), String::new()),
                    Tag::List { ordered: true, start } if *start != 1 => ("ol".to_string(), format!(" start=\"{start}\"")),
                    Tag::List { ordered: true, .. } => ("ol".to_string(), String::new()),
                    Tag::List { .. } => ("ul".to_string(), String::new()),
                    Tag::Item => ("li".to_string(), String::new()),
                    Tag::Table => ("table".to_string(), " border=\"1\" cellspacing=\"0\" cellpadding=\"4\" style=\"border-collapse:collapse\"".to_string()),
                    Tag::Row => ("tr".to_string(), String::new()),
                    Tag::Cell { header, colspan, align, bg } => {
                        let mut css = align_css(*align).to_string();
                        if let Some(c) = bg {
                            css.push_str(&format!("background-color:{};", hex(*c)));
                        }
                        let span = if *colspan > 1 { format!(" colspan=\"{colspan}\"") } else { String::new() };
                        let style = if css.is_empty() { String::new() } else { format!(" style=\"{css}\"") };
                        (if *header { "th" } else { "td" }.to_string(), format!("{span}{style}"))
                    }
                };
                out.push('<');
                out.push_str(&name);
                out.push_str(&attrs);
                out.push('>');
                close.push(name);
            }
            Ev::Exit => {
                if let Some(name) = close.pop() {
                    after_bare = name.is_empty();
                    if !after_bare {
                        out.push_str("</");
                        out.push_str(&name);
                        out.push('>');
                    }
                }
            }
            Ev::Run(t, f) => run_html(t, f, &mut out),
            Ev::Marker(_) => {}
            Ev::Br => out.push_str("<br>"),
            Ev::Soft => out.push(' '),
            Ev::Image { src, alt } => {
                out.push_str("<img src=\"");
                esc_html(src, &mut out);
                out.push_str("\" alt=\"");
                esc_html(alt, &mut out);
                out.push_str("\">");
            }
        }
    }
    while let Some(name) = close.pop() {
        if !name.is_empty() {
            out.push_str("</");
            out.push_str(&name);
            out.push('>');
        }
    }
    out
}

/// Wraps a fragment in the `HTML Format` clipboard document (a header with
/// byte offsets into the UTF-8 text).
pub fn html_document(fragment: &str) -> String {
    const HEADER: usize = 105; // length of the header below, with 10-digit offsets
    const BEFORE: &str = "<html><head><meta charset=\"utf-8\"></head><body><!--StartFragment-->";
    const AFTER: &str = "<!--EndFragment--></body></html>";
    let start = HEADER + BEFORE.len();
    let end = start + fragment.len();
    let total = end + AFTER.len();
    format!("Version:0.9\r\nStartHTML:{HEADER:010}\r\nEndHTML:{total:010}\r\nStartFragment:{start:010}\r\nEndFragment:{end:010}\r\n{BEFORE}{fragment}{AFTER}")
}

// ------------------------------------------------------------ RTF

fn esc_rtf(s: &str, out: &mut String) {
    for u in s.encode_utf16() {
        match u {
            0x5C => out.push_str("\\\\"),
            0x7B => out.push_str("\\{"),
            0x7D => out.push_str("\\}"),
            0x09 => out.push_str("\\tab "),
            0x0A => out.push_str("\\line "),
            0x20..=0x7E => out.push(u as u8 as char),
            // Latin-1 letters as code page bytes: every RTF reader takes
            // these, including inside hyperlink fields.
            0xA0..=0xFF => out.push_str(&format!("\\'{u:02x}")),
            _ => out.push_str(&format!("\\u{}?", u as i16)),
        }
    }
}

struct Rtf {
    out: String,
    colors: Vec<Color>,
    stack: Vec<Tag>,
    /// Cells of the open table row, as finished RTF.
    cells: Vec<String>,
    cell: String,
}

impl Rtf {
    fn sink(&mut self) -> &mut String {
        if self.stack.iter().any(|t| matches!(t, Tag::Cell { .. })) {
            &mut self.cell
        } else {
            &mut self.out
        }
    }

    fn color(&self, c: Color) -> usize {
        self.colors.iter().position(|x| *x == c).map_or(0, |i| i + 1)
    }

    fn indent(&self) -> usize {
        self.stack.iter().filter(|t| matches!(t, Tag::Quote | Tag::List { .. })).count() * RTF_INDENT
    }

    fn enter_para(&mut self, align: Align, bg: Option<Color>) {
        let mut p = String::from("\\pard");
        if self.stack.iter().any(|t| matches!(t, Tag::Cell { .. })) {
            p.push_str("\\intbl");
        }
        p.push_str(match align {
            Align::Left => "\\ql",
            Align::Center => "\\qc",
            Align::Right => "\\qr",
        });
        let indent = self.indent();
        let list = self.stack.iter().any(|t| matches!(t, Tag::Item));
        if indent > 0 {
            p.push_str(&format!("\\li{indent}"));
        }
        if list {
            p.push_str(&format!("\\fi-{}", RTF_INDENT / 2));
        }
        p.push_str("\\sa120");
        if let Some(c) = bg {
            let n = self.color(c);
            p.push_str(&format!("\\cbpat{n}"));
        }
        p.push(' ');
        self.sink().push_str(&p);
    }

    fn run(&mut self, text: &str, f: &Fmt) {
        let mut r = String::from("{");
        if f.mono {
            r.push_str("\\f1");
        }
        for (on, word) in [(f.bold, "\\b"), (f.italic, "\\i"), (f.strike, "\\strike")] {
            if on {
                r.push_str(word);
            }
        }
        if f.underline || f.link.is_some() {
            r.push_str("\\ul");
        }
        if (f.size - 1.0).abs() > 0.01 {
            r.push_str(&format!("\\fs{}", (pt(f.size) * 2.0) as i32));
        }
        match f.valign {
            1 => r.push_str("\\super"),
            -1 => r.push_str("\\sub"),
            _ => {}
        }
        let color = f.color.or(f.link.as_ref().map(|_| Color::rgb(0x0B5CC1)));
        if let Some(c) = color {
            r.push_str(&format!("\\cf{}", self.color(c)));
        }
        if let Some(c) = f.bg {
            let n = self.color(c);
            r.push_str(&format!("\\cb{n}\\chcbpat{n}"));
        }
        // A space ends a control word; after a bare "{" it would be text.
        if r.len() > 1 {
            r.push(' ');
        }
        esc_rtf(text, &mut r);
        r.push('}');
        if let Some(l) = &f.link {
            // Hyperlink fields take ASCII only.
            let mut url = String::new();
            for b in l.bytes() {
                match b {
                    b'"' | 0x80..=0xFF => url.push_str(&format!("%{b:02X}")),
                    b => esc_rtf(&(b as char).to_string(), &mut url),
                }
            }
            r = format!("{{\\field{{\\*\\fldinst{{HYPERLINK \"{url}\"}}}}{{\\fldrslt{r}}}}}");
        }
        self.sink().push_str(&r);
    }

    fn end_row(&mut self) {
        let n = self.cells.len();
        if n == 0 {
            return;
        }
        let w = RTF_TABLE_WIDTH / n;
        let mut row = String::from("\\trowd\\trgaph80");
        for i in 1..=n {
            for side in ["t", "l", "b", "r"] {
                row.push_str(&format!("\\clbrdr{side}\\brdrs\\brdrw10"));
            }
            row.push_str(&format!("\\cellx{}", w * i));
        }
        row.push('\n');
        for c in self.cells.drain(..) {
            row.push_str(&c);
            row.push_str("\\cell\n");
        }
        row.push_str("\\row\n");
        self.out.push_str(&row);
    }
}

/// The selection as an RTF document.
pub fn rtf(evs: &[Ev]) -> String {
    let mut colors: Vec<Color> = Vec::new();
    let mut add = |c: Option<Color>| {
        if let Some(c) = c {
            if !colors.contains(&c) {
                colors.push(c);
            }
        }
    };
    for ev in evs {
        match ev {
            Ev::Run(_, f) => {
                add(f.color);
                add(f.bg);
                add(f.link.as_ref().map(|_| Color::rgb(0x0B5CC1)));
            }
            Ev::Enter(Tag::Para { bg, .. } | Tag::Cell { bg, .. }) => add(*bg),
            _ => {}
        }
    }
    let table: String = colors.iter().map(|c| format!("\\red{}\\green{}\\blue{};", c.r, c.g, c.b)).collect();
    let mut r = Rtf {
        out: format!("{{\\rtf1\\ansi\\ansicpg1252\\deff0\\uc1{{\\fonttbl{{\\f0\\fswiss\\fcharset0 {SANS};}}{{\\f1\\fmodern\\fcharset0 {MONO};}}}}{{\\colortbl;{table}}}\\fs{}\n", (BODY_PT * 2.0) as i32),
        colors,
        stack: Vec::new(),
        cells: Vec::new(),
        cell: String::new(),
    };
    for ev in evs {
        match ev {
            Ev::Enter(t) => {
                r.stack.push(t.clone());
                if let Tag::Para { align, bg, .. } = t {
                    r.enter_para(*align, *bg);
                }
            }
            Ev::Exit => match r.stack.pop() {
                Some(Tag::Para { .. }) => {
                    if r.stack.iter().any(|t| matches!(t, Tag::Cell { .. })) {
                        r.cell.push_str("\\par ");
                    } else {
                        r.out.push_str("\\par\n");
                    }
                }
                Some(Tag::Cell { .. }) => {
                    let mut c = std::mem::take(&mut r.cell);
                    if let Some(s) = c.strip_suffix("\\par ") {
                        c.truncate(s.len());
                    }
                    r.cells.push(c);
                }
                Some(Tag::Row) => r.end_row(),
                _ => {}
            },
            Ev::Run(t, f) => r.run(t, f),
            Ev::Marker(m) => {
                let mut s = String::new();
                esc_rtf(m, &mut s);
                s.push_str("\\tab ");
                r.sink().push_str(&s);
            }
            Ev::Br => r.sink().push_str("\\line "),
            Ev::Soft => r.sink().push(' '),
            Ev::Image { alt, .. } => {
                if !alt.is_empty() {
                    let mut s = String::from("[");
                    esc_rtf(alt, &mut s);
                    s.push(']');
                    r.sink().push_str(&s);
                }
            }
        }
    }
    r.out.push('}');
    r.out
}

// ------------------------------------------------------------ images

/// An image as a `CF_DIB` block: a BITMAPINFOHEADER and bottom-up 24-bit
/// rows, composited on white (premultiplied input).
pub fn dib(b: &Bitmap) -> Vec<u8> {
    let (w, h) = (b.w as usize, b.h as usize);
    let stride = (w * 3 + 3) & !3;
    let size = stride * h;
    let mut out = Vec::with_capacity(40 + size);
    out.extend(40u32.to_le_bytes());
    out.extend((w as i32).to_le_bytes());
    out.extend((h as i32).to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(24u16.to_le_bytes());
    out.extend(0u32.to_le_bytes()); // BI_RGB
    out.extend((size as u32).to_le_bytes());
    out.extend([0u8; 16]); // resolution and palette: unused
    for y in (0..h).rev() {
        let row = &b.rgba[y * w * 4..(y + 1) * w * 4];
        let start = out.len();
        for p in row.as_chunks::<4>().0 {
            let over = 255 - p[3] as u32;
            let mix = |c: u8| (c as u32 + over).min(255) as u8;
            out.extend([mix(p[2]), mix(p[1]), mix(p[0])]);
        }
        out.resize(start + stride, 0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para() -> Ev {
        Ev::Enter(Tag::Para { align: Align::Left, pre: false, heading: 0, bg: None })
    }

    fn run(s: &str) -> Ev {
        Ev::Run(s.to_string(), Fmt::default())
    }

    #[test]
    fn plain_text_structure() {
        let evs = vec![
            para(),
            run("one"),
            Ev::Br,
            run("two"),
            Ev::Exit,
            Ev::Enter(Tag::List { ordered: false, start: 1 }),
            Ev::Enter(Tag::Item),
            para(),
            Ev::Marker("•".into()),
            run("a"),
            Ev::Exit,
            Ev::Exit,
            Ev::Enter(Tag::Item),
            para(),
            Ev::Marker("•".into()),
            run("b"),
            Ev::Exit,
            Ev::Exit,
            Ev::Exit,
            Ev::Enter(Tag::Table),
            Ev::Enter(Tag::Row),
            Ev::Enter(Tag::Cell { header: true, colspan: 1, align: Align::Left, bg: None }),
            para(),
            run("x"),
            Ev::Exit,
            Ev::Exit,
            Ev::Enter(Tag::Cell { header: true, colspan: 1, align: Align::Left, bg: None }),
            para(),
            run("y"),
            Ev::Exit,
            Ev::Exit,
            Ev::Exit,
            Ev::Enter(Tag::Row),
            Ev::Enter(Tag::Cell { header: false, colspan: 1, align: Align::Left, bg: None }),
            para(),
            run("1"),
            Ev::Exit,
            Ev::Exit,
            Ev::Exit,
            Ev::Exit,
            para(),
            run("end"),
            Ev::Exit,
        ];
        assert_eq!(plain(&evs), "one\ntwo\n\n• a\n• b\n\nx\ty\n1\n\nend");
    }

    #[test]
    fn html_formats_and_escapes() {
        let bold = Fmt { bold: true, size: 2.0, color: Some(Color::rgb(0xFF0000)), ..Fmt::default() };
        let link = Fmt { link: Some("https://x.org/?a=1&b=2".into()), ..Fmt::default() };
        let evs = vec![para(), Ev::Run("a<b".into(), bold), run(" & "), Ev::Run("go".into(), link), Ev::Exit];
        let h = html(&evs);
        assert_eq!(h, "<p><span style=\"font-weight:bold;font-size:22pt;color:#FF0000;\">a&lt;b</span> &amp; <a href=\"https://x.org/?a=1&amp;b=2\">go</a></p>");
    }

    #[test]
    fn html_cells_and_items_hold_bare_text() {
        let cell = Ev::Enter(Tag::Cell { header: false, colspan: 1, align: Align::Right, bg: None });
        let right = Ev::Enter(Tag::Para { align: Align::Right, pre: false, heading: 0, bg: None });
        let evs = vec![Ev::Enter(Tag::Table), Ev::Enter(Tag::Row), cell, right, run("1"), Ev::Exit, para(), run("two\nlines"), Ev::Exit, Ev::Exit, Ev::Exit, Ev::Exit];
        assert!(html(&evs).ends_with("<tr><td style=\"text-align:right;\">1<br>two<br>lines</td></tr></table>"), "{}", html(&evs));
        let evs = vec![Ev::Enter(Tag::List { ordered: true, start: 3 }), Ev::Enter(Tag::Item), para(), run("a"), Ev::Exit, Ev::Exit, Ev::Exit];
        assert_eq!(html(&evs), "<ol start=\"3\"><li>a</li></ol>");
    }

    #[test]
    fn runs_of_one_format_are_joined() {
        let mut evs = Vec::new();
        push_run(&mut evs, "a", Fmt::default());
        push_run(&mut evs, "b", Fmt::default());
        push_run(&mut evs, "c", Fmt { bold: true, ..Fmt::default() });
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0], run("ab"));
    }

    #[test]
    fn html_closes_open_tags_and_marks_the_fragment() {
        let h = html(&[Ev::Enter(Tag::Quote), para(), run("é")]);
        assert_eq!(h, "<blockquote><p>é</p></blockquote>");
        let doc = html_document(&h);
        let offset = |k: &str| -> usize {
            let at = doc.find(k).unwrap() + k.len();
            doc[at..at + 10].parse().unwrap()
        };
        assert!(doc[offset("StartHTML:")..].starts_with("<html>"));
        assert_eq!(&doc[offset("StartFragment:")..offset("EndFragment:")], h);
        assert_eq!(offset("EndHTML:"), doc.len());
    }

    #[test]
    fn rtf_is_ascii_and_escapes_unicode() {
        let evs = vec![para(), Ev::Run("é{}\\😀".into(), Fmt { bold: true, color: Some(Color::rgb(0x102030)), ..Fmt::default() }), Ev::Exit];
        let r = rtf(&evs);
        assert!(r.is_ascii());
        assert!(r.starts_with("{\\rtf1"), "{r}");
        assert!(r.ends_with('}'));
        assert!(r.contains("\\red16\\green32\\blue48;"));
        assert!(r.contains("\\b\\cf1 \\'e9\\{\\}\\\\\\u-10179?\\u-8704?}"), "{r}");
        assert_eq!(r.matches('{').count() - r.matches("\\{").count(), r.matches('}').count() - r.matches("\\}").count());
    }

    #[test]
    fn rtf_plain_runs_and_links() {
        let link = Fmt { link: Some("https://x.org/página".into()), ..Fmt::default() };
        let r = rtf(&[para(), run("a"), Ev::Run("ação".into(), link), Ev::Exit]);
        assert!(r.contains("{a}"), "no stray space: {r}");
        assert!(r.contains("HYPERLINK \"https://x.org/p%C3%A1gina\""), "{r}");
        assert!(r.contains("a\\'e7\\'e3o}"), "{r}");
    }

    #[test]
    fn rtf_tables_have_a_cell_per_column() {
        let cell = || Ev::Enter(Tag::Cell { header: false, colspan: 1, align: Align::Left, bg: None });
        let evs = vec![Ev::Enter(Tag::Table), Ev::Enter(Tag::Row), cell(), para(), run("a"), Ev::Exit, Ev::Exit, cell(), para(), run("b"), Ev::Exit, Ev::Exit, Ev::Exit, Ev::Exit];
        let r = rtf(&evs);
        assert_eq!(r.matches("\\cellx").count(), 2);
        assert_eq!(r.matches("\\cell\n").count(), 2);
        assert_eq!(r.matches("\\row").count(), 1);
        assert!(r.contains("\\intbl"), "{r}");
    }

    #[test]
    fn dib_is_bottom_up_bgr_on_white() {
        // 1×2: an opaque red pixel over a transparent one.
        let b = Bitmap { w: 1, h: 2, rgba: vec![255, 0, 0, 255, 0, 0, 0, 0] };
        let d = dib(&b);
        assert_eq!(d.len(), 40 + 8);
        assert_eq!(&d[40..44], &[255, 255, 255, 0], "bottom row first, padded");
        assert_eq!(&d[44..48], &[0, 0, 255, 0], "BGR");
    }
}
