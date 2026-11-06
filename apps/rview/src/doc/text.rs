//! Plain text documents: decoded once into a `String` with a line index, so
//! drawing only touches the visible lines however large the file is.

use crate::io::encoding::{self, Decoded};
use crate::ui::layout;

pub const TAB: usize = 4;
/// Above this size, word wrap is not offered (the wrap index would cost
/// noticeable time on every resize).
pub const WRAP_LIMIT: usize = 16 * 1024 * 1024;

pub struct TextDoc {
    pub text: String,
    /// Byte offset of the start of each line.
    pub lines: Vec<usize>,
    pub encoding: String,
    pub eol: &'static str,
    /// Longest line in bytes (an upper bound for its width in cells).
    pub longest: usize,
}

impl TextDoc {
    pub fn from_bytes(bytes: &[u8]) -> TextDoc {
        let d: Decoded = encoding::decode(bytes, encoding::system_legacy());
        TextDoc::from_decoded(d.text, d.enc.name(), d.eol.name())
    }

    pub fn from_decoded(text: String, encoding: String, eol: &'static str) -> TextDoc {
        let mut lines = Vec::with_capacity(text.len() / 40 + 1);
        lines.push(0);
        let mut longest = 0;
        let mut last = 0;
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                longest = longest.max(i - last);
                last = i + 1;
                lines.push(i + 1);
            }
        }
        longest = longest.max(text.len() - last);
        TextDoc { text, lines, encoding, eol, longest }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line(&self, i: usize) -> &str {
        let start = self.lines[i];
        let end = self.lines.get(i + 1).map_or(self.text.len(), |e| e - 1);
        &self.text[start..end.max(start)]
    }
}

/// Wrapped-row index: `starts[i]` is the first visual row of line `i`.
pub struct WrapIndex {
    pub cells: usize,
    pub starts: Vec<usize>,
    pub rows: usize,
}

impl WrapIndex {
    pub fn build(doc: &TextDoc, cells: usize) -> WrapIndex {
        let cells = cells.max(8);
        let mut starts = Vec::with_capacity(doc.line_count());
        let mut rows = 0;
        for i in 0..doc.line_count() {
            starts.push(rows);
            let line = doc.line(i);
            // Fast path: short ASCII lines take one row.
            rows += if line.len() <= cells && line.is_ascii() && !line.contains('\t') { 1 } else { layout::wrap(line, cells, TAB).len() };
        }
        WrapIndex { cells, starts, rows }
    }

    /// Line containing visual row `row`, and the row within that line.
    pub fn locate(&self, row: usize) -> (usize, usize) {
        let i = self.starts.partition_point(|&s| s <= row).saturating_sub(1);
        (i, row - self.starts[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_wrap() {
        let d = TextDoc::from_bytes(b"one\r\ntwo two two two\r\n\r\nend");
        assert_eq!(d.line_count(), 4);
        assert_eq!(d.line(1), "two two two two");
        assert_eq!(d.line(2), "");
        assert_eq!(d.line(3), "end");
        assert_eq!(d.eol, "CRLF");
        let w = WrapIndex::build(&d, 8);
        assert_eq!(w.starts, vec![0, 1, 3, 4]);
        assert_eq!(w.rows, 5);
        assert_eq!(w.locate(2), (1, 1));
        assert_eq!(w.locate(4), (3, 0));
    }

    #[test]
    fn utf16_and_legacy_text() {
        let mut b = vec![0xFF, 0xFE];
        b.extend("héllo\nwörld".encode_utf16().flat_map(|u| u.to_le_bytes()));
        let d = TextDoc::from_bytes(&b);
        assert_eq!(d.line(1), "wörld");
        assert!(d.encoding.contains("UTF-16"));
    }
}
