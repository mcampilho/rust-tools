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

/// Draws a message panel over `full`, dimming what is behind it. Returns
/// (panel, OK button), for clicks.
pub fn draw_message(g: &mut Gfx, full: Rect, st: &DialogStyle, s: f32, title: &str, text: &str) -> (Rect, Rect) {
    g.fill_rect(full, Color::rgba(0x000000, if st.dark { 0x90 } else { 0x60 }));
    let pad = (24.0 * s).round();
    let px = 13.5 * s;
    let lh = (px * 1.55).round();
    let title_px = 17.0 * s;
    let w = (600.0 * s).min(full.w - 32.0 * s).max(200.0 * s);
    let inner = w - 2.0 * pad;
    // Two-column rows: the key column fits the widest key.
    let key_w = text
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(k, _)| g.measure(k, px, FontId::Bold))
        .fold(0.0f32, f32::max);
    let col = if key_w > 0.0 { (key_w + 20.0 * s).min(inner * 0.55) } else { 0.0 };
    let mut rows: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        match line.split_once('\t') {
            Some((k, v)) => {
                for (i, part) in wrap_words(g, v, inner - col, px, FontId::Regular).into_iter().enumerate() {
                    rows.push((if i == 0 { k.to_string() } else { String::new() }, part));
                }
            }
            None => {
                for part in wrap_words(g, line, inner, px, FontId::Regular) {
                    rows.push((String::new(), part));
                }
            }
        }
    }
    let btn_h = (32.0 * s).round();
    let h = (pad + title_px * 1.6 + 8.0 * s + rows.len() as f32 * lh + 18.0 * s + btn_h + pad).min(full.h - 16.0 * s);
    let panel = Rect::new(
        ((full.w - w) / 2.0).round(),
        ((full.h - h) / 2.0).round().max(8.0 * s),
        w.round(),
        h.round(),
    );
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
    for (k, v) in rows {
        if y + lh > bottom + 1.0 {
            break;
        }
        let base = (y + (lh + asc) / 2.0 - px * 0.15).round();
        if !k.is_empty() {
            g.text(&k, panel.x + pad, base, px, FontId::Bold, st.text);
        }
        g.text(&v, panel.x + pad + col, base, px, FontId::Regular, if col > 0.0 { st.dim } else { st.text });
        y += lh;
    }
    let bw = (88.0 * s).round();
    let ok = Rect::new(panel.right() - pad - bw, panel.bottom() - pad - btn_h, bw, btn_h);
    let accent = if st.dark { Color::rgb(0x4CC2FF) } else { Color::rgb(0x0067C0) };
    g.fill_rrect(ok, (4.0 * s).round(), accent);
    g.text_in(
        "OK",
        ok,
        13.0 * s,
        FontId::Bold,
        if st.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) },
        Align::Center,
    );
    (panel, ok)
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
