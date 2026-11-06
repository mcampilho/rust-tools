//! Monospace text layout in character cells: tabs expand to tab stops,
//! East Asian wide characters and emoji take two cells, combining marks
//! take none. Pure functions shared by rendering, hit testing and caret
//! movement.

/// Display width of a character in cells.
pub fn cells(c: char) -> usize {
    let u = c as u32;
    if u < 0x300 {
        return 1;
    }
    // Combining marks and zero-width characters.
    if (0x300..=0x36F).contains(&u)
        || (0x1AB0..=0x1AFF).contains(&u)
        || (0x1DC0..=0x1DFF).contains(&u)
        || (0x20D0..=0x20FF).contains(&u)
        || (0xFE20..=0xFE2F).contains(&u)
        || u == 0x200B
        || u == 0x200C
        || u == 0x200D
        || u == 0xFE0F
    {
        return 0;
    }
    // Wide: Hangul Jamo, CJK, Hiragana/Katakana, Hangul syllables, CJK
    // compatibility, fullwidth forms, emoji.
    if (0x1100..=0x115F).contains(&u)
        || (0x2E80..=0x303E).contains(&u)
        || (0x3041..=0x33FF).contains(&u)
        || (0x3400..=0x4DBF).contains(&u)
        || (0x4E00..=0x9FFF).contains(&u)
        || (0xA000..=0xA4CF).contains(&u)
        || (0xAC00..=0xD7A3).contains(&u)
        || (0xF900..=0xFAFF).contains(&u)
        || (0xFE30..=0xFE4F).contains(&u)
        || (0xFF00..=0xFF60).contains(&u)
        || (0xFFE0..=0xFFE6).contains(&u)
        || (0x1F300..=0x1F64F).contains(&u)
        || (0x1F900..=0x1F9FF).contains(&u)
        || (0x20000..=0x3FFFD).contains(&u)
    {
        return 2;
    }
    1
}

/// Advances `col` past `c`.
#[inline]
pub fn advance(col: usize, c: char, tab: usize) -> usize {
    if c == '\t' {
        (col / tab + 1) * tab
    } else {
        col + cells(c)
    }
}

/// Start char indices of each visual row of `line` wrapped to `width`
/// cells (the first is always 0). Breaks after whitespace when possible.
pub fn wrap(line: &str, width: usize, tab: usize) -> Vec<usize> {
    let mut rows = vec![0];
    let width = width.max(8);
    let mut col = 0;
    let mut last_break: Option<usize> = None; // index after a space
    let mut row_start = 0;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = advance(col, c, tab);
        if next > width && (c == ' ' || c == '\t') {
            // Trailing whitespace may hang past the edge; break after it.
            col = next;
            last_break = Some(i + 1);
            i += 1;
            continue;
        }
        if next > width && i > row_start {
            let brk = match last_break {
                Some(b) if b > row_start => b,
                _ => i,
            };
            rows.push(brk);
            row_start = brk;
            // Recompute the column of the new row up to i.
            col = 0;
            for &ch in &chars[brk..i] {
                col = advance(col, ch, tab);
            }
            last_break = None;
            continue;
        }
        col = next;
        if c == ' ' || c == '\t' {
            last_break = Some(i + 1);
        }
        i += 1;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(cells('a'), 1);
        assert_eq!(cells('日'), 2);
        assert_eq!(cells('\u{301}'), 0);
        assert_eq!(cells('😀'), 2);
        assert_eq!(advance(1, '\t', 4), 4);
        assert_eq!("日本x".chars().fold(0, |c, ch| advance(c, ch, 4)), 5);
    }

    #[test]
    fn wrapping() {
        assert_eq!(wrap("hello world foo", 11, 4), vec![0, 12]);
        assert_eq!(wrap("abcdefghijklmnop", 8, 4), vec![0, 8]);
        assert_eq!(wrap("short", 80, 4), vec![0]);
        assert_eq!(wrap("", 80, 4), vec![0]);
        // Every row fits; trailing spaces may hang past the edge.
        let line = "the quick brown fox jumps over the lazy dog again and again";
        let rows = wrap(line, 10, 4);
        for w in rows.windows(2) {
            let seg: String = line.chars().skip(w[0]).take(w[1] - w[0]).collect();
            let seg = seg.trim_end();
            assert!(seg.chars().fold(0, |c, ch| advance(c, ch, 4)) <= 10, "{seg:?}");
        }
    }
}
