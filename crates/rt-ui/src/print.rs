//! Printing text: the pages of a document of coloured text in a fixed-width font, with the file name and the
//! date on top and "Page n of N" below. The planning (wrapping the lines, tabs, page breaks) is plain code
//! with tests; `print_text` (Windows) shows the Print dialog and sends the pages to the printer through
//! `rt_sys::win::print`. Apps that print something else (pictures of pages, a laid-out document) use
//! `rt_sys::win::print::Printer` themselves.

use rt_core::i18n::{tf, tr};
use rt_gfx::{Color, FontId, Gfx, Rect};

/// A piece of a line in one colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub color: Color,
}

impl Run {
    pub fn new(text: impl Into<String>, color: Color) -> Run {
        Run { text: text.into(), color }
    }
}

/// What to print.
pub struct TextJob {
    /// Shown on top of every page (the file name) and as the name of the print job.
    pub title: String,
    /// The lines of the document.
    pub lines: Vec<Vec<Run>>,
    pub line_numbers: bool,
    /// Width of a tab in columns.
    pub tab: usize,
    /// Wrap long lines (off: they are cut at the margin).
    pub wrap: bool,
    /// Size of the text in points.
    pub pt: f32,
}

/// The lines of `text` as runs coloured for white paper by the highlighting of `lang`. Big files are not
/// highlighted (as on screen).
pub fn highlighted_lines(lang: rt_syntax::Lang, text: &str) -> Vec<Vec<Run>> {
    let ink = Color::rgb(0x1B1B1B);
    let segments: Vec<(String, Option<Color>)> = if lang == rt_syntax::Lang::Plain || text.len() > 1 << 20 {
        vec![(text.to_string(), None)]
    } else {
        rt_syntax::highlight(lang, text).into_iter().map(|(s, t)| (s, rt_syntax::rgb(t, false).map(Color::rgb))).collect()
    };
    let mut lines: Vec<Vec<Run>> = vec![Vec::new()];
    for (seg, color) in segments {
        let color = color.unwrap_or(ink);
        for (k, part) in seg.split('\n').enumerate() {
            if k > 0 {
                lines.push(Vec::new());
            }
            let part = part.trim_end_matches('\r');
            if !part.is_empty() {
                lines.last_mut().expect("a line").push(Run::new(part, color));
            }
        }
    }
    // A final newline ends the last line; it does not start one more.
    if text.ends_with('\n') && lines.len() > 1 && lines.last().is_some_and(Vec::is_empty) {
        lines.pop();
    }
    lines
}

/// One row of a page: a line, or the continuation of one that was wrapped.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Number of the line, on the row where it starts.
    pub number: Option<usize>,
    pub runs: Vec<Run>,
}

/// The rows of the whole document, cut into pages.
#[derive(Debug, PartialEq)]
pub struct Plan {
    pub rows: Vec<Row>,
    pub per_page: usize,
}

impl Plan {
    pub fn pages(&self) -> u32 {
        self.rows.len().div_ceil(self.per_page.max(1)).max(1) as u32
    }

    pub fn page(&self, n: u32) -> &[Row] {
        let per = self.per_page.max(1);
        let a = (n as usize * per).min(self.rows.len());
        &self.rows[a..(a + per).min(self.rows.len())]
    }
}

/// Columns a character takes: two for wide (East Asian) characters, none for combining marks.
fn cells(c: char) -> usize {
    match c as u32 {
        0x0300..=0x036F | 0x200B..=0x200D | 0xFE00..=0xFE0F => 0,
        0x1100..=0x115F
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1FAFF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

/// Cuts the lines of `job` into rows of at most `cols` columns (tabs become spaces) and pages of `per_page`.
pub fn plan(job: &TextJob, cols: usize, per_page: usize) -> Plan {
    let cols = cols.max(1);
    let tab = job.tab.max(1);
    let mut rows = Vec::new();
    for (n, line) in job.lines.iter().enumerate() {
        let mut row = Row { number: job.line_numbers.then_some(n + 1), runs: Vec::new() };
        let mut used = 0;
        'line: for run in line {
            let mut text = String::new();
            for c in run.text.chars() {
                if c == '\r' || c == '\n' {
                    continue;
                }
                let w = if c == '\t' { tab - used % tab } else { cells(c) };
                if used + w > cols {
                    if !job.wrap {
                        if !text.is_empty() {
                            row.runs.push(Run { text: std::mem::take(&mut text), color: run.color });
                        }
                        break 'line;
                    }
                    if !text.is_empty() {
                        row.runs.push(Run { text: std::mem::take(&mut text), color: run.color });
                    }
                    rows.push(std::mem::replace(&mut row, Row { number: None, runs: Vec::new() }));
                    used = 0;
                }
                if c == '\t' {
                    text.extend(std::iter::repeat_n(' ', w));
                } else {
                    text.push(c);
                }
                used += w;
            }
            if !text.is_empty() {
                row.runs.push(Run { text, color: run.color });
            }
        }
        rows.push(row);
    }
    if rows.is_empty() {
        rows.push(Row { number: None, runs: Vec::new() });
    }
    Plan { rows, per_page: per_page.max(1) }
}

/// Where things go on a page, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub px: f32,
    pub adv: f32,
    pub row_h: f32,
    pub margin: f32,
    pub band: f32,
    /// Width of the column of line numbers (0 when there is none).
    pub gutter: f32,
    pub cols: usize,
    pub per_page: usize,
}

/// The layout of a `w` × `h` pixel page at `dpi` for text of `pt` points whose characters are `adv` px wide at
/// `px` (`adv` is `px` times the font's width, measured by the caller), with room for `digits` of line number.
pub fn geometry(w: u32, h: u32, dpi: f32, pt: f32, adv_per_px: f32, digits: usize) -> Geometry {
    let px = (pt * dpi / 72.0).max(4.0);
    let adv = (px * adv_per_px).max(1.0);
    let margin = dpi * 12.0 / 25.4;
    let band = px * 1.9;
    let row_h = px * 1.28;
    let gutter = if digits > 0 { adv * (digits as f32 + 1.5) } else { 0.0 };
    let cols = (((w as f32 - 2.0 * margin - gutter) / adv).floor() as usize).max(1);
    let per_page = (((h as f32 - 2.0 * margin - 2.0 * band) / row_h).floor() as usize).max(1);
    Geometry { px, adv, row_h, margin, band, gutter, cols, per_page }
}

/// The margins of a page and the bands for the header and the footer, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub margin: f32,
    /// Height of the header band, and of the footer band.
    pub band: f32,
    /// Size of the header and footer text.
    pub small: f32,
}

/// The frame for pages drawn at `dpi`: 12 mm margins, 8 pt header and footer.
pub fn frame(dpi: f32) -> Frame {
    let small = 8.0 * dpi / 72.0;
    Frame { margin: dpi * 12.0 / 25.4, band: small * 2.4, small }
}

/// Clears the page to white and draws the header (the title, the date, a line) and the footer "Page n of N";
/// `n` is 0-based. The body goes between `margin + band` and `height - margin - band`.
pub fn draw_frame(g: &mut Gfx, f: &Frame, size: (u32, u32), title: &str, date: &str, n: u32, total: u32) {
    let (w, h) = size;
    let grey = Color::rgb(0x707070);
    g.clear(Color::rgb(0xFFFFFF));
    let (m, wf, small) = (f.margin, w as f32, f.small);
    let date_w = g.measure(date, small, FontId::Regular);
    let room = (wf - 2.0 * m - date_w - small * 2.0).max(10.0);
    let shown = rt_gfx::dialog::ellipsize(g, title, room, small, FontId::Regular);
    g.text(&shown, m, m + small, small, FontId::Regular, grey);
    g.text(date, wf - m - date_w, m + small, small, FontId::Regular, grey);
    let rule = (small / 8.0).max(1.0).round();
    g.fill_rect(Rect::new(m, m + f.band * 0.8, wf - 2.0 * m, rule), grey);
    let label = tf(tr("Page {} of {}"), &[&(n + 1).to_string(), &total.to_string()]);
    let lw = g.measure(&label, small, FontId::Regular);
    g.text(&label, (wf - lw) / 2.0, h as f32 - m, small, FontId::Regular, grey);
}

/// Draws page `n` of `plan` on a `g` that is already `w` × `h`.
pub fn draw_page(g: &mut Gfx, plan: &Plan, geo: &Geometry, n: u32, size: (u32, u32), title: &str, date: &str) {
    let grey = Color::rgb(0x707070);
    let m = geo.margin;
    draw_frame(g, &Frame { margin: m, band: geo.band, small: geo.px * 0.8 }, size, title, date, n, plan.pages());
    let top = m + geo.band;
    let digits = (geo.gutter / geo.adv - 1.5).round().max(0.0) as usize;
    for (i, row) in plan.page(n).iter().enumerate() {
        let base = top + i as f32 * geo.row_h + geo.px;
        if let Some(num) = row.number {
            let s = format!("{num:>digits$}");
            g.text(&s, m, base, geo.px, FontId::Mono, grey);
        }
        let mut x = m + geo.gutter;
        for run in &row.runs {
            g.text(&run.text, x, base, geo.px, FontId::Mono, run.color);
            x += run.text.chars().map(cells).sum::<usize>() as f32 * geo.adv;
        }
    }
}

/// The date for the page header, `2026-11-24`.
pub fn today() -> String {
    let t = rt_sys::local_time();
    format!("{}-{:02}-{:02}", t.year, t.month, t.day)
}

/// Shows the Print dialog and, if the user chooses a printer, runs `work` on it on a thread of its own (the
/// pages take a while to draw; the window stays alive). `done` is called on that thread with the number of pages
/// printed or the reason it failed (a panic included). `false` when the dialog was cancelled.
#[cfg(windows)]
pub fn start<W, D>(hwnd: rt_sys::win::HWND, work: W, done: D) -> bool
where
    W: FnOnce(&rt_sys::win::print::Printer) -> Result<u32, String> + Send + 'static,
    D: FnOnce(Result<u32, String>) + Send + 'static,
{
    let Some(printer) = rt_sys::win::print::choose(hwnd) else { return false };
    std::thread::Builder::new()
        .name("print".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&printer)))
                .unwrap_or_else(|_| Err("internal error".to_string()));
            drop(printer);
            done(result);
        })
        .is_ok()
}

/// `start` for a document of coloured text: `mono` names the fixed-width fonts of the app (as for the window).
#[cfg(windows)]
pub fn start_text<D>(hwnd: rt_sys::win::HWND, mono: &'static [&'static str], job: TextJob, done: D) -> bool
where
    D: FnOnce(Result<u32, String>) + Send + 'static,
{
    start(hwnd, move |p| print_on(p, mono, &job), done)
}

/// Prints `job` on `printer`; returns the number of pages.
#[cfg(windows)]
pub fn print_on(printer: &rt_sys::win::print::Printer, mono: &'static [&'static str], job: &TextJob) -> Result<u32, String> {
    let (w, h) = printer.page_size();
    let dpi = printer.render_dpi();
    let mut g = Gfx::new()?;
    if !mono.is_empty() {
        g.set_mono_names(mono);
    }
    g.resize(w, h);
    let probe = 100.0;
    let adv_per_px = g.advance('0', probe, FontId::Mono) / probe;
    let digits = if job.line_numbers { job.lines.len().max(1).to_string().len() } else { 0 };
    let geo = geometry(w, h, dpi, job.pt, adv_per_px, digits);
    let plan = plan(job, geo.cols, geo.per_page);
    let date = today();
    let total = plan.pages();
    printer.print(&job.title, total, &mut |n, pixels| {
        draw_page(&mut g, &plan, &geo, n, (w, h), &job.title, &date);
        g.copy_to(pixels);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(lines: &[&str], wrap: bool) -> TextJob {
        TextJob {
            title: "t.txt".into(),
            lines: lines.iter().map(|l| vec![Run::new(*l, Color::rgb(0))]).collect(),
            line_numbers: true,
            tab: 4,
            wrap,
            pt: 10.0,
        }
    }

    fn text_of(r: &Row) -> String {
        r.runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn long_lines_wrap_and_only_the_first_row_is_numbered() {
        let p = plan(&job(&["abcdefghij", "xy"], true), 4, 100);
        let rows: Vec<(Option<usize>, String)> = p.rows.iter().map(|r| (r.number, text_of(r))).collect();
        assert_eq!(rows, [(Some(1), "abcd".into()), (None, "efgh".into()), (None, "ij".into()), (Some(2), "xy".into())]);
    }

    #[test]
    fn without_wrapping_the_rest_is_cut() {
        let p = plan(&job(&["abcdefghij"], false), 4, 100);
        assert_eq!(p.rows.len(), 1);
        assert_eq!(text_of(&p.rows[0]), "abcd");
    }

    #[test]
    fn tabs_reach_the_next_stop_and_wide_characters_take_two_columns() {
        let p = plan(&job(&["a\tb", "\tc", "日本語"], true), 8, 100);
        assert_eq!(text_of(&p.rows[0]), "a   b");
        assert_eq!(text_of(&p.rows[1]), "    c");
        // Six columns of kanji: all three fit in 8, a fourth would not.
        let p = plan(&job(&["日本語字"], true), 6, 100);
        assert_eq!(p.rows.iter().map(text_of).collect::<Vec<_>>(), ["日本語", "字"]);
    }

    #[test]
    fn colours_survive_wrapping() {
        let red = Color::rgb(0xFF0000);
        let blue = Color::rgb(0x0000FF);
        let j = TextJob { lines: vec![vec![Run::new("abc", red), Run::new("defg", blue)]], ..job(&[], true) };
        let p = plan(&j, 5, 10);
        assert_eq!(p.rows[0].runs, [Run::new("abc", red), Run::new("de", blue)]);
        assert_eq!(p.rows[1].runs, [Run::new("fg", blue)]);
    }

    #[test]
    fn pages_hold_the_rows_that_fit() {
        let lines: Vec<String> = (0..25).map(|i| format!("line {i}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let p = plan(&job(&refs, true), 80, 10);
        assert_eq!(p.pages(), 3);
        assert_eq!(p.page(0).len(), 10);
        assert_eq!(p.page(2).len(), 5);
        assert_eq!(p.page(7).len(), 0, "past the end is empty, not a panic");
        // An empty document is one empty page.
        let e = plan(&job(&[], true), 80, 10);
        assert_eq!(e.pages(), 1);
    }

    #[test]
    fn a_page_fits_a_sensible_amount() {
        // A4 at 300 dpi, 10 pt Consolas-like (0.55 em wide).
        let g = geometry(2480, 3508, 300.0, 10.0, 0.55, 3);
        assert!((80..=100).contains(&g.cols), "{g:?}");
        // About 57 rows of 12.8 pt between the margins and the header and footer: what an A4 page holds.
        assert!((50..=65).contains(&g.per_page), "{g:?}");
        assert!(g.margin > 100.0 && g.margin < 160.0);
    }

    #[test]
    fn highlighted_text_becomes_coloured_lines() {
        let rust = highlighted_lines(rt_syntax::Lang::from_id("rust").expect("rust"), "fn main() {\r\n    // hi\r\n}\n");
        assert_eq!(rust.len(), 3, "the last newline ends a line, it does not start one");
        let colours: std::collections::HashSet<(u8, u8, u8)> = rust.iter().flatten().map(|r| (r.color.r, r.color.g, r.color.b)).collect();
        assert!(colours.len() >= 3, "keyword, comment and plain text differ: {colours:?}");
        let text: String = rust[1].iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, "    // hi", "no carriage return");
        let plain = highlighted_lines(rt_syntax::Lang::Plain, "a\n\nb");
        assert_eq!(plain.len(), 3);
        assert!(plain[1].is_empty());
        assert_eq!(highlighted_lines(rt_syntax::Lang::Plain, "").len(), 1);
    }

    /// Prints to a PDF with Windows' "Microsoft Print to PDF": `cargo test -p rt-ui -- --ignored prints_to_pdf`
    /// with `RT_PDF_OUT` set to the file to write; open it to see the pages.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn prints_to_pdf() {
        let out = std::env::var("RT_PDF_OUT").expect("RT_PDF_OUT");
        let printer = rt_sys::win::print::named("Microsoft Print to PDF", Some(std::path::Path::new(&out))).expect("the PDF printer");
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/print.rs")).expect("source");
        let job = TextJob {
            title: "print.rs".into(),
            lines: highlighted_lines(rt_syntax::Lang::from_id("rust").expect("rust"), &src),
            line_numbers: true,
            tab: 4,
            wrap: true,
            pt: 10.0,
        };
        let pages = print_on(&printer, &[], &job).expect("printed");
        assert!(pages >= 3, "{pages}");
        let head = std::fs::read(&out).expect("the PDF");
        assert!(head.starts_with(b"%PDF"), "a PDF file");
    }

    #[test]
    fn a_page_can_be_drawn() {
        let Ok(mut g) = Gfx::new() else { return };
        g.resize(600, 800);
        let p = plan(&job(&["hello", "world"], true), 40, 30);
        let geo = geometry(600, 800, 72.0, 10.0, 0.55, 1);
        draw_page(&mut g, &p, &geo, 0, (600, 800), "t.txt", "2026-11-24");
        let mut px = vec![0u32; 600 * 800];
        g.copy_to(&mut px);
        assert!(px.iter().any(|&c| c & 0xFF < 0x80), "something dark was drawn");
        assert_eq!(px[0], 0xFFFFFF, "on white paper");
    }
}
