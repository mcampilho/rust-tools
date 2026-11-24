//! File › Print (Windows). What is on screen is drawn again for paper, on the thread that prints: Markdown and
//! HTML are laid out for the page (the column as wide as the margins allow, `<details>` open) and cut into pages
//! between lines; CSV prints as a table; text as coloured lines of a fixed-width font; a PDF page by page, fitted
//! to the paper. Every page has the file name and date on top and "Page n of N" below.

use crate::doc::csv::{is_number, CsvDoc};
use crate::rich::layout::{self, ImgState, Item, Layout, Palette};
use crate::rich::{Align, Block, Cell, Inline, Kind as BlockKind, RichDoc, Row, Style, Table};
use crate::ui::gfx::{Color, Gfx, Rect};
use rt_sys::win::print::Printer;
use std::collections::HashMap;
use std::path::PathBuf;
use tiny_skia::Pixmap;

/// The state and the picture of each image of the document (as the window has them).
pub type Images = HashMap<String, (ImgState, Option<Pixmap>)>;

/// What to print; made on the window's thread, printed on another.
pub enum Job {
    Text(rt_ui::print::TextJob),
    Rich { title: String, doc: Box<RichDoc>, images: Images },
    Pdf { title: String, path: PathBuf },
}

impl Job {
    pub fn title(&self) -> &str {
        match self {
            Job::Text(t) => &t.title,
            Job::Rich { title, .. } | Job::Pdf { title, .. } => title,
        }
    }
}

/// Prints `job` on `printer`; returns the number of pages.
pub fn run(job: Job, printer: &Printer) -> Result<u32, String> {
    match job {
        Job::Text(t) => rt_ui::print::print_on(printer, crate::ui::gfx::RVIEW_MONO, &t),
        Job::Rich { title, doc, images } => print_rich(&title, *doc, &images, printer),
        Job::Pdf { title, path } => print_pdf(&title, &path, printer),
    }
}

/// A table for a CSV document: the first record is the header; numbers are on the right.
pub fn csv_doc(d: &CsvDoc) -> RichDoc {
    let mut table = Table::default();
    for i in 0..d.rows() {
        let header = i == 0;
        let cells = d
            .record(i)
            .into_iter()
            .map(|text| {
                let style = Style { bold: header, size: 0.9, ..Style::default() };
                let align = if !header && is_number(&text) { Align::Right } else { Align::Left };
                let para = Block { margin_bottom: 0.0, ..Block::para(vec![Inline::Text(text, style)]) };
                Cell { blocks: vec![para], header, align, colspan: 1, bg: None }
            })
            .collect();
        table.rows.push(Row { cells });
    }
    RichDoc { blocks: vec![Block { kind: BlockKind::Table(table), ..Block::default() }], ..RichDoc::default() }
}

/// Opens every `<details>`: on paper there is nothing to click.
fn expand_details(blocks: &mut [Block]) {
    for b in blocks {
        match &mut b.kind {
            BlockKind::Details(d) => {
                d.open = true;
                expand_details(&mut d.body);
            }
            BlockKind::Group(g) => expand_details(g),
            BlockKind::List(l) => l.items.iter_mut().for_each(|i| expand_details(&mut i.blocks)),
            BlockKind::Table(t) => t.rows.iter_mut().flat_map(|r| r.cells.iter_mut()).for_each(|c| expand_details(&mut c.blocks)),
            _ => {}
        }
    }
}

/// The vertical extent of everything a page break must not cut through: text, pictures and shapes.
fn unbreakable(l: &Layout) -> Vec<(f32, f32)> {
    l.items
        .iter()
        .filter_map(|it| match it {
            Item::Text { y, px, .. } => Some((y - px * 0.95, y + px * 0.3)),
            Item::Image { r, .. } => Some((r.y, r.bottom())),
            Item::Shape { pts, width, .. } => {
                let (a, b) = pts.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
                Some((a - width, b + width))
            }
            Item::Fill { .. } => None,
        })
        .collect()
}

/// Page ranges `(top, bottom)` in layout coordinates, from `top` to `bottom`, each at most `page_h` tall and
/// ending between lines where it can: a break that would cut through text moves up above it.
pub fn page_ranges(boxes: &[(f32, f32)], top: f32, bottom: f32, page_h: f32) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    let mut y = top;
    while y < bottom - 0.5 {
        let hard = y + page_h;
        if hard >= bottom {
            out.push((y, bottom));
            break;
        }
        let mut e = hard;
        for _ in 0..200 {
            let crossing = boxes.iter().filter(|(a, b)| *a < e && *b > e).map(|(a, _)| *a).fold(e, f32::min);
            if crossing >= e {
                break;
            }
            e = crossing;
        }
        // Only what is taller than a page (the break moved back to the very top of this one) is cut through,
        // where the page ends; anything that fits goes whole to the next page, however little is left here.
        if e <= y + 1.0 {
            e = hard;
        }
        out.push((y, e));
        y = e;
    }
    if out.is_empty() {
        out.push((top, bottom.max(top + 1.0)));
    }
    out
}

fn print_rich(title: &str, mut doc: RichDoc, images: &Images, printer: &Printer) -> Result<u32, String> {
    let (w, h) = printer.page_size();
    let dpi = printer.render_dpi();
    let mut g = Gfx::new()?;
    g.set_mono_names(crate::ui::gfx::RVIEW_MONO);
    g.resize(w, h);
    let frame = rt_ui::print::frame(dpi);
    // 10.5 pt text; the column is as wide as the margins leave (Markdown's narrow reading column is for screens).
    let em = (10.5 * dpi / 72.0 * 4.0).round() / 4.0;
    let pad = (em * 2.0).round();
    let content_w = w as f32 - 2.0 * frame.margin;
    doc.max_width = None;
    expand_details(&mut doc.blocks);
    doc.flipped.clear();
    let base = doc.base.clone();
    let resolve = |src: &str| {
        let k = crate::view::image_key(&base, src);
        let state = images.get(&k).map_or(ImgState::Failed, |i| i.0);
        (k, state)
    };
    let l = layout::layout(&doc, &mut g, content_w + 2.0 * pad, em, Palette::paper(), &resolve);
    // A little air under the header's line.
    let gap = frame.small * 0.7;
    let body_top = frame.margin + frame.band + gap;
    let body_h = (h as f32 - 2.0 * (frame.margin + frame.band) - gap).max(em);
    let pages = page_ranges(&unbreakable(&l), pad, (l.height - pad).max(pad + 1.0), body_h);
    let date = rt_ui::print::today();
    let total = pages.len() as u32;
    printer.print(title, total, &mut |n, pixels| {
        let (y0, y1) = pages[n as usize];
        rt_ui::print::draw_frame(&mut g, &frame, (w, h), title, &date, n, total);
        // Only this page's slice: a page that ends early (above a picture that goes to the next page) must not
        // show the top of what the next one prints whole.
        let body = Rect::new(frame.margin, body_top, content_w, (y1 - y0).min(body_h));
        g.set_clip(Some(body));
        // Layout x of the column's left edge is `pad`; its top at page `y0` goes to the top of the body.
        let (ox, oy) = (frame.margin - pad, body_top - y0);
        for (item, &(a, b)) in l.items.iter().zip(&l.spans) {
            if b <= y0 || a >= y1 {
                continue;
            }
            match item {
                Item::Text { x, y, s, px, font, color } => {
                    g.text(s, (x + ox).round(), y + oy, *px, *font, *color);
                }
                Item::Fill { r, color } => g.fill_rect(Rect::new(r.x + ox, r.y + oy, r.w, r.h), *color),
                Item::Shape { pts, color, width, close } => {
                    let pts: Vec<(f32, f32)> = pts.iter().map(|p| (p.0 + ox, p.1 + oy)).collect();
                    if *width > 0.0 {
                        g.polyline(&pts, *width, *color, *close);
                    } else {
                        g.fill_polygon(&pts, *color);
                    }
                }
                Item::Image { r, key } => {
                    let dest = Rect::new(r.x + ox, r.y + oy, r.w, r.h);
                    match images.get(key).and_then(|i| i.1.as_ref()) {
                        Some(p) => g.draw_image(p, dest),
                        None => g.stroke_rrect(dest, 2.0, 1.0, Color::rgb(0xBBBBBB)),
                    }
                }
            }
        }
        g.set_clip(None);
        g.copy_to(pixels);
    })
}

/// Each page of the PDF, as large as the paper allows inside small margins, centred.
fn print_pdf(title: &str, path: &std::path::Path, printer: &Printer) -> Result<u32, String> {
    let src = crate::media::PdfSource::open(path)?;
    let (w, h) = printer.page_size();
    let margin = printer.render_dpi() * 8.0 / 25.4;
    let total = src.sizes.len() as u32;
    printer.print(title, total, &mut |n, pixels| {
        let Some(&(pw, ph)) = src.sizes.get(n as usize) else { return };
        let scale = ((w as f32 - 2.0 * margin) / pw).min((h as f32 - 2.0 * margin) / ph);
        let Some(bmp) = src.render(n, (pw * scale).round().max(16.0) as u32) else { return };
        let (ox, oy) = ((w as i64 - bmp.w as i64) / 2, (h as i64 - bmp.h as i64) / 2);
        for y in 0..bmp.h as i64 {
            let dy = y + oy;
            if dy < 0 || dy >= h as i64 {
                continue;
            }
            for x in 0..bmp.w as i64 {
                let dx = x + ox;
                if dx < 0 || dx >= w as i64 {
                    continue;
                }
                let i = (y as usize * bmp.w as usize + x as usize) * 4;
                // Premultiplied RGBA over white paper.
                let a = bmp.rgba[i + 3] as u32;
                let c = |v: u8| (v as u32 + 255 - a).min(255);
                pixels[dy as usize * w as usize + dx as usize] = c(bmp.rgba[i]) << 16 | c(bmp.rgba[i + 1]) << 8 | c(bmp.rgba[i + 2]);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_end_between_lines() {
        // Lines 10 px tall every 12 px from 0; pages 100 tall: 0..100 cuts through the line at 96..106.
        let lines: Vec<(f32, f32)> = (0..30).map(|i| (i as f32 * 12.0, i as f32 * 12.0 + 10.0)).collect();
        let r = page_ranges(&lines, 0.0, 360.0, 100.0);
        assert_eq!(r[0], (0.0, 96.0), "the page ends above the line it would have cut");
        for w in r.windows(2) {
            assert_eq!(w[0].1, w[1].0, "no gap and no overlap");
        }
        assert_eq!(r.last().map(|p| p.1), Some(360.0));
        for (a, b) in &r {
            assert!(b - a <= 100.0 + 1e-3);
            assert!(lines.iter().all(|(la, lb)| !(*la < *b && *lb > *b)), "no line is cut at {b}");
        }
    }

    #[test]
    fn a_picture_that_does_not_fit_goes_whole_to_the_next_page() {
        // A line, then a picture 40 tall from 20 to 60, on pages 50 tall.
        let boxes = [(0.0, 10.0), (20.0, 60.0)];
        let r = page_ranges(&boxes, 0.0, 70.0, 50.0);
        assert_eq!(r[0], (0.0, 20.0), "the first page ends above the picture, with room to spare");
        assert!(r[1].0 <= 20.0 && r[1].1 >= 60.0, "the picture is whole on the second: {r:?}");
    }

    #[test]
    fn something_taller_than_the_page_is_cut_where_the_page_ends() {
        let r = page_ranges(&[(0.0, 500.0)], 0.0, 500.0, 100.0);
        assert_eq!(r.len(), 5);
        assert_eq!(r[0], (0.0, 100.0));
    }

    #[test]
    fn an_empty_document_is_one_page() {
        assert_eq!(page_ranges(&[], 0.0, 0.0, 100.0), [(0.0, 1.0)]);
        assert_eq!(page_ranges(&[], 10.0, 60.0, 100.0), [(10.0, 60.0)]);
    }

    #[test]
    fn csv_becomes_a_table_with_a_header() {
        let d = CsvDoc::from_text("name,qty\nbolt,12\nnut,3.5\n".to_string(), "UTF-8".to_string());
        let doc = csv_doc(&d);
        let BlockKind::Table(t) = &doc.blocks[0].kind else { panic!() };
        assert_eq!(t.rows.len(), 3);
        assert!(t.rows[0].cells.iter().all(|c| c.header));
        assert_eq!(t.rows[1].cells[1].align, Align::Right);
        assert_eq!(t.rows[1].cells[0].align, Align::Left);
    }

    #[test]
    fn details_are_opened_for_paper() {
        let mut doc = crate::rich::markdown::parse("<details>\n<summary>S</summary>\n\nbody\n</details>\n");
        expand_details(&mut doc.blocks);
        let BlockKind::Details(d) = &doc.blocks[0].kind else { panic!() };
        assert!(d.open);
    }

    /// Prints a file to a PDF with "Microsoft Print to PDF" and no dialog:
    /// `RVIEW_PRINT_IN=doc.md RVIEW_PRINT_OUT=out.pdf ./dev.ps1 test -p rview '--' --ignored prints_a_file`
    #[test]
    #[ignore]
    fn prints_a_file_to_pdf() {
        let (input, out) =
            (std::env::var("RVIEW_PRINT_IN").expect("RVIEW_PRINT_IN"), std::env::var("RVIEW_PRINT_OUT").expect("RVIEW_PRINT_OUT"));
        let path = std::path::Path::new(&input);
        let content = crate::doc::load(path, crate::doc::Kind::of(path)).expect("load");
        let title = "test".to_string();
        let job = match content {
            crate::doc::Content::Rich(mut d) => {
                d.base = path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
                // RVIEW_PRINT_IMG: the images (relative to the document) to print as pictures.
                let mut images = Images::new();
                for src in std::env::var("RVIEW_PRINT_IMG").unwrap_or_default().split(';').filter(|s| !s.is_empty()) {
                    let key = crate::view::image_key(&d.base, src);
                    let bmp = crate::media::decode_here(&std::fs::read(&key).expect("image")).expect("decoded");
                    let pix = crate::view::pixmap(bmp).expect("pixmap");
                    images.insert(key, (ImgState::Ready(pix.width(), pix.height()), Some(pix)));
                }
                Job::Rich { title, doc: Box::new(d), images }
            }
            crate::doc::Content::Csv(d) => Job::Rich { title, doc: Box::new(csv_doc(&d)), images: HashMap::new() },
            crate::doc::Content::Pdf => Job::Pdf { title, path: path.to_path_buf() },
            crate::doc::Content::Text(d) => Job::Text(rt_ui::print::TextJob {
                title,
                lines: rt_ui::print::highlighted_lines(crate::syntax::Lang::Plain, &d.text),
                line_numbers: false,
                tab: 4,
                wrap: true,
                pt: 10.0,
            }),
        };
        let printer = rt_sys::win::print::named("Microsoft Print to PDF", Some(std::path::Path::new(&out))).expect("the PDF printer");
        let pages = run(job, &printer).expect("printed");
        assert!(pages >= 1);
        assert!(std::fs::read(&out).expect("the PDF").starts_with(b"%PDF"));
    }

    #[test]
    fn a_document_lays_out_and_prints_its_pages() {
        let Ok(mut g) = Gfx::new() else { return };
        g.resize(800, 1000);
        let src = (0..200).map(|i| format!("Paragraph {i} of the document, with some words in it.\n\n")).collect::<String>();
        let doc = crate::rich::markdown::parse(&src);
        let none = |s: &str| (s.to_string(), ImgState::Failed);
        let l = layout::layout(&doc, &mut g, 800.0, 16.0, Palette::paper(), &none);
        let pages = page_ranges(&unbreakable(&l), 32.0, l.height - 32.0, 900.0);
        assert!(pages.len() >= 5, "{}", pages.len());
    }
}
