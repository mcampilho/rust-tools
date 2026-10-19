//! CSV documents: the delimiter is detected from the first lines, records
//! are indexed once (respecting quoted fields with embedded line breaks),
//! and only the visible records are split into fields when drawing.

use crate::io::encoding;

pub struct CsvDoc {
    pub text: String,
    pub delim: char,
    /// Byte offset of each record.
    pub records: Vec<usize>,
    pub columns: usize,
    pub encoding: String,
}

/// Splits one record starting at `start`; returns the fields and the
/// offset after the record.
pub fn parse_record(text: &str, start: usize, delim: char) -> (Vec<String>, usize) {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = text[start..].char_indices().peekable();
    let mut in_quotes = false;
    let mut at_field_start = true;
    let mut end = text.len();
    while let Some((i, c)) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek().map(|p| p.1) == Some('"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        if c == '"' && at_field_start {
            in_quotes = true;
            at_field_start = false;
            continue;
        }
        if c == delim {
            fields.push(std::mem::take(&mut field));
            at_field_start = true;
            continue;
        }
        if c == '\n' {
            end = start + i + 1;
            break;
        }
        if c == '\r' && chars.peek().map(|p| p.1) == Some('\n') {
            continue;
        }
        at_field_start = false;
        field.push(c);
    }
    fields.push(field);
    (fields, end)
}

/// Picks the delimiter that splits the first lines most consistently.
pub fn detect_delimiter(text: &str) -> char {
    let sample: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(30).collect();
    let mut best = (',', 0usize, 0usize);
    for d in [',', ';', '\t', '|'] {
        let counts: Vec<usize> = sample.iter().map(|l| parse_record(l, 0, d).0.len() - 1).collect();
        let Some(&first) = counts.first() else { continue };
        if first == 0 {
            continue;
        }
        let consistent = counts.iter().filter(|&&c| c == first).count();
        // Prefer consistency, then more columns.
        if (consistent, first) > (best.1, best.2) {
            best = (d, consistent, first);
        }
    }
    best.0
}

impl CsvDoc {
    pub fn from_bytes(bytes: &[u8]) -> CsvDoc {
        let d = encoding::decode(bytes, encoding::system_legacy());
        CsvDoc::from_text(d.text, d.enc.name())
    }

    pub fn from_text(text: String, encoding: String) -> CsvDoc {
        let delim = detect_delimiter(&text);
        // Index records: a line break ends a record unless inside quotes.
        let mut records = Vec::with_capacity(text.len() / 60 + 1);
        let bytes = text.as_bytes();
        let mut in_quotes = false;
        let mut start = 0;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'"' => in_quotes = !in_quotes,
                b'\n' if !in_quotes => {
                    records.push(start);
                    start = i + 1;
                }
                _ => {}
            }
        }
        if start < bytes.len() && !text[start..].trim().is_empty() {
            records.push(start);
        }
        let columns = records.iter().take(1000).map(|&r| parse_record(&text, r, delim).0.len()).max().unwrap_or(0);
        CsvDoc { text, delim, records, columns, encoding }
    }

    pub fn record(&self, i: usize) -> Vec<String> {
        parse_record(&self.text, self.records[i], self.delim).0
    }

    pub fn rows(&self) -> usize {
        self.records.len()
    }
}

/// Whether a cell looks numeric (right-aligned in the grid).
pub fn is_number(s: &str) -> bool {
    let t = s.trim().trim_start_matches(['+', '-']).trim_end_matches('%');
    !t.is_empty() && t.chars().any(|c| c.is_ascii_digit()) && t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | ' ' | 'e' | 'E'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_quotes() {
        let d = CsvDoc::from_text("a,b,c\n1,\"x, y\",3\n\"multi\nline\",\"q\"\"q\",\r\n".into(), String::new());
        assert_eq!(d.delim, ',');
        assert_eq!(d.rows(), 3);
        assert_eq!(d.columns, 3);
        assert_eq!(d.record(1), vec!["1", "x, y", "3"]);
        assert_eq!(d.record(2), vec!["multi\nline", "q\"q", ""]);
    }

    #[test]
    fn delimiter_detection() {
        assert_eq!(detect_delimiter("a;b;c\n1;2,5;3\n4;5;6"), ';');
        assert_eq!(detect_delimiter("a\tb\n1\t2"), '\t');
        assert_eq!(detect_delimiter("a,b\n1,2"), ',');
        assert_eq!(detect_delimiter("name|age\nx|1"), '|');
    }

    #[test]
    fn numbers() {
        assert!(is_number("12.5"));
        assert!(is_number("-3"));
        assert!(is_number("45%"));
        assert!(!is_number("abc"));
        assert!(!is_number(""));
    }
}
