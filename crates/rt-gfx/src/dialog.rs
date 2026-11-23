//! A message panel drawn over the window (About, Keyboard shortcuts), and the
//! text helpers it uses. Lines containing a tab are shown as two aligned
//! columns: a bold key and its description.

use crate::gfx::{Align, Color, FontId, Gfx, Rect};

/// Colours of the panel, from the app's theme.
#[derive(Clone, Copy, Debug)]
pub struct DialogStyle {
    pub dark: bool,
    pub text: Color,
    pub dim: Color,
    pub border: Color,
}

/// Word-wraps `text` to `width` pixels.
pub fn wrap_words(g: &mut Gfx, text: &str, width: f32, px: f32, font: FontId) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if !line.is_empty() && g.measure(&candidate, px, font) > width {
            out.push(std::mem::take(&mut line));
            line = word.to_string();
        } else {
            line = candidate;
        }
    }
    out.push(line);
    out
}

/// `s` cut to `max` pixels with an ellipsis at the end, if it does not fit.
pub fn ellipsize(g: &mut Gfx, s: &str, max: f32, px: f32, font: FontId) -> String {
    if g.measure(s, px, font) <= max {
        return s.to_string();
    }
    let dots = g.measure("…", px, font);
    let mut out = String::new();
    let mut w = 0.0;
    for c in s.chars() {
        let a = g.advance(c, px, font);
        if w + a + dots > max {
            break;
        }
        w += a;
        out.push(c);
    }
    out.push('…');
    out
}

/// `s` cut to `max` pixels with an ellipsis in the middle, so that both the
/// start and the end of a path stay visible.
pub fn ellipsize_middle(g: &mut Gfx, s: &str, max: f32, px: f32, font: FontId) -> String {
    if g.measure(s, px, font) <= max {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let budget = (max - g.measure("…", px, font)).max(0.0) / 2.0;
    let (mut head, mut wh) = (0, 0.0);
    while head < chars.len() {
        let a = g.advance(chars[head], px, font);
        if wh + a > budget {
            break;
        }
        wh += a;
        head += 1;
    }
    let (mut tail, mut wt) = (chars.len(), 0.0);
    while tail > head {
        let a = g.advance(chars[tail - 1], px, font);
        if wt + a > budget {
            break;
        }
        wt += a;
        tail -= 1;
    }
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[tail..]);
    out
}

/// The rows of a message at one letter size, in as many columns as it needs
/// (up to three) and the window allows.
struct MessageRows {
    /// (key, text); a heading among keys is a text starting with `\u{1}`.
    rows: Vec<(String, String)>,
    /// The key column's width (0: no keys).
    col: f32,
    columns: usize,
    /// The panel's width, the letter size and the line height.
    w: f32,
    px: f32,
    lh: f32,
    /// Rows that fit in one column.
    fit: usize,
}

#[allow(clippy::too_many_arguments)]
fn message_rows(g: &mut Gfx, text: &str, px: f32, s: f32, full: Rect, pad: f32, gap: f32, chrome: f32) -> MessageRows {
    let lh = (px * 1.55).round();
    let fit = (((full.h - 16.0 * s - chrome) / lh).floor() as usize).max(1);
    // Two-column rows: the key column fits the widest key.
    let key_w = text.lines().filter_map(|l| l.split_once('\t')).map(|(k, _)| g.measure(k, px, FontId::Bold)).fold(0.0f32, f32::max);
    let rows_for = |g: &mut Gfx, inner: f32| {
        let col = if key_w > 0.0 { (key_w + 20.0 * s).min(inner * 0.55) } else { 0.0 };
        let mut rows: Vec<(String, String)> = Vec::new();
        for line in text.lines() {
            match line.split_once('\t') {
                Some((k, v)) => {
                    // A key too wide for its column: its text on the next line.
                    let alone = g.measure(k, px, FontId::Bold) > col - 10.0 * s;
                    if alone {
                        rows.push((k.to_string(), String::new()));
                    }
                    for (i, part) in wrap_words(g, v, inner - col, px, FontId::Regular).into_iter().enumerate() {
                        rows.push((if i == 0 && !alone { k.to_string() } else { String::new() }, part));
                    }
                }
                // Among keys, a line of its own is a heading.
                None if col > 0.0 && !line.trim().is_empty() => rows.push((String::new(), format!("\u{1}{line}"))),
                None => {
                    for part in wrap_words(g, line, inner, px, FontId::Regular) {
                        rows.push((String::new(), part));
                    }
                }
            }
        }
        (rows, col)
    };
    let mut w = (600.0 * s).min(full.w - 32.0 * s).max(200.0 * s);
    let (mut rows, mut col) = rows_for(g, w - 2.0 * pad);
    let mut columns = 1;
    for n in [2usize, 3] {
        if rows.len() <= fit * columns {
            break;
        }
        let wide = (n as f32 * 420.0 * s).min(full.w - 32.0 * s);
        let inner = (wide - 2.0 * pad - (n - 1) as f32 * gap) / n as f32;
        if inner < 230.0 * s {
            break;
        }
        (rows, col) = rows_for(g, inner);
        w = wide;
        columns = n;
    }
    MessageRows { rows, col, columns, w, px, lh, fit }
}

/// Draws a message panel over `full`, dimming what is behind it. Returns
/// (panel, OK button), for clicks.
pub fn draw_message(g: &mut Gfx, full: Rect, st: &DialogStyle, s: f32, title: &str, text: &str) -> (Rect, Rect) {
    let (panel, buttons) = draw_panel(g, full, st, s, title, text, &[("OK", true)]);
    (panel, buttons.first().copied().unwrap_or_default())
}

/// Draws a message panel with its own buttons (label, whether it is the main
/// one), right-aligned in that order. Returns (panel, the buttons), for clicks.
pub fn draw_panel(
    g: &mut Gfx,
    full: Rect,
    st: &DialogStyle,
    s: f32,
    title: &str,
    text: &str,
    buttons: &[(&str, bool)],
) -> (Rect, Vec<Rect>) {
    g.fill_rect(full, Color::rgba(0x000000, if st.dark { 0x90 } else { 0x60 }));
    let pad = (24.0 * s).round();
    let title_px = 17.0 * s;
    let gap = (28.0 * s).round();
    let btn_h = (32.0 * s).round();
    let chrome = pad + title_px * 1.6 + 8.0 * s + 18.0 * s + btn_h + pad;
    // A long list (keyboard shortcuts) that does not fit in height goes side
    // by side, in two or three columns, when the window is wide enough; and
    // if even so it does not fit, in smaller letters.
    let mut fitted = None;
    for size in [13.5, 12.5, 11.5, 10.5] {
        let l = message_rows(g, text, size * s, s, full, pad, gap, chrome);
        let fits = l.rows.len() <= l.fit * l.columns;
        fitted = Some(l);
        if fits {
            break;
        }
    }
    let Some(MessageRows { rows, col, columns, w, px, lh, .. }) = fitted else { return (Rect::default(), Vec::new()) };
    let inner = (w - 2.0 * pad - (columns - 1) as f32 * gap) / columns as f32;
    let per_column = rows.len().div_ceil(columns).max(1);
    let h = (chrome + per_column.min(rows.len()) as f32 * lh).min(full.h - 16.0 * s);
    let panel = Rect::new((full.x + (full.w - w) / 2.0).round(), full.y + ((full.h - h) / 2.0).round().max(8.0 * s), w.round(), h.round());
    let bg = if st.dark { Color::rgb(0x2B2B2B) } else { Color::rgb(0xFFFFFF) };
    let r = (8.0 * s).round();
    for i in 1..=4 {
        let d = i as f32 * s;
        g.fill_rrect(
            Rect::new(panel.x - d, panel.y - d + 2.0 * s, panel.w + 2.0 * d, panel.h + 2.0 * d),
            r + d,
            Color::rgba(0x000000, 0x18),
        );
    }
    g.fill_rrect(panel, r, bg);
    g.stroke_rrect(panel, r, 1.0, st.border);
    let mut y = panel.y + pad;
    let (asc, _) = g.metrics(title_px, FontId::Bold);
    g.text(title, panel.x + pad, y + asc, title_px, FontId::Bold, st.text);
    y += title_px * 1.6 + 8.0 * s;
    let (asc, _) = g.metrics(px, FontId::Regular);
    let bottom = panel.bottom() - pad - btn_h - 18.0 * s;
    let top = y;
    for (i, (k, v)) in rows.into_iter().enumerate() {
        let c = i / per_column;
        if i % per_column == 0 {
            y = top;
        }
        if y + lh > bottom + 1.0 {
            continue;
        }
        let x = panel.x + pad + c as f32 * (inner + gap);
        let base = (y + (lh + asc) / 2.0 - px * 0.15).round();
        if !k.is_empty() {
            g.text(&k, x, base, px, FontId::Bold, st.text);
        }
        if col > 0.0 && k.is_empty() && v.starts_with('\u{1}') {
            // A heading among the keys ("Editor"): across the columns, bold.
            g.text(&v[1..], x, base, px, FontId::Bold, st.text);
        } else {
            g.text(&v, x + col, base, px, FontId::Regular, if col > 0.0 { st.dim } else { st.text });
        }
        y += lh;
    }
    // The buttons, right-aligned: the last one at the right edge.
    let accent = if st.dark { Color::rgb(0x4CC2FF) } else { Color::rgb(0x0067C0) };
    let widths: Vec<f32> = buttons.iter().map(|(l, _)| (g.measure(l, 13.0 * s, FontId::Bold) + 32.0 * s).max(88.0 * s).round()).collect();
    let mut x = panel.right() - pad;
    let mut rects = vec![Rect::default(); buttons.len()];
    for (i, ((label, main), bw)) in buttons.iter().zip(&widths).enumerate().rev() {
        x -= bw;
        let r = Rect::new(x, panel.bottom() - pad - btn_h, *bw, btn_h);
        if *main {
            g.fill_rrect(r, (4.0 * s).round(), accent);
            g.text_in(label, r, 13.0 * s, FontId::Bold, if st.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) }, Align::Center);
        } else {
            g.stroke_rrect(r, (4.0 * s).round(), 1.0, st.border);
            g.text_in(label, r, 13.0 * s, FontId::Regular, st.text, Align::Center);
        }
        rects[i] = r;
        x -= 8.0 * s;
    }
    (panel, rects)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_cut_to_fit() {
        let Ok(mut g) = Gfx::new() else { return };
        let long = "C:\\Users\\someone\\Documents\\Projects\\a very long folder name\\file.txt";
        let px = 12.0;
        let max = 160.0;
        for cut in [ellipsize(&mut g, long, max, px, FontId::Regular), ellipsize_middle(&mut g, long, max, px, FontId::Regular)] {
            assert!(cut.contains('…'));
            assert!(g.measure(&cut, px, FontId::Regular) <= max + 0.5, "{cut}");
        }
        assert!(ellipsize_middle(&mut g, long, max, px, FontId::Regular).ends_with("txt"));
        assert_eq!(ellipsize(&mut g, "short", max, px, FontId::Regular), "short");
        let lines = wrap_words(&mut g, "one two three four five six seven", 60.0, px, FontId::Regular);
        assert!(lines.len() > 1);
    }

    #[test]
    fn panel_fits_the_window() {
        let Ok(mut g) = Gfx::new() else { return };
        g.resize(500, 400);
        let st = DialogStyle { dark: true, text: Color::rgb(0xFFFFFF), dim: Color::rgb(0x999999), border: Color::rgb(0x444444) };
        let full = Rect::new(0.0, 0.0, 500.0, 400.0);
        let (panel, ok) = draw_message(&mut g, full, &st, 1.0, "About", "Line one\nKey\tValue");
        assert!(panel.x >= 0.0 && panel.right() <= 500.0 && panel.bottom() <= 400.0);
        assert!(panel.contains(ok.x + 1.0, ok.y + 1.0));
    }
}
