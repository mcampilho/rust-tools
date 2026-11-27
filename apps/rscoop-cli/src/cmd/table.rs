//! Tables and lists as Scoop shows them, and sizes as it writes them.

/// The lines of a table: headers, a rule of dashes, one line per row. Columns
/// that are empty in every row are left out.
pub fn lines(headers: &[&str], rows: &[Vec<String>]) -> Vec<String> {
    let used: Vec<usize> = (0..headers.len()).filter(|&c| rows.iter().any(|r| !r[c].is_empty())).collect();
    let width = |c: usize| rows.iter().map(|r| r[c].chars().count()).chain([headers[c].chars().count()]).max().unwrap_or(0);
    let widths: Vec<usize> = used.iter().map(|&c| width(c)).collect();
    let line = |cells: Vec<&str>| {
        let padded: Vec<String> = cells.iter().zip(&widths).map(|(s, w)| format!("{s:<w$}")).collect();
        padded.join(" ").trim_end().to_string()
    };
    let rule: Vec<String> = used.iter().map(|&c| "-".repeat(headers[c].chars().count())).collect();
    let mut out = vec![line(used.iter().map(|&c| headers[c]).collect()), line(rule.iter().map(String::as_str).collect())];
    out.extend(rows.iter().map(|r| line(used.iter().map(|&c| r[c].as_str()).collect())));
    out
}

/// `Name : value` lines; a value of several lines is indented under itself.
pub fn pairs(items: &[(String, String)]) -> Vec<String> {
    let width = items.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
    let mut out = Vec::new();
    for (key, value) in items {
        for (i, part) in value.split('\n').enumerate() {
            let head = if i == 0 { format!("{key:<width$} :") } else { " ".repeat(width + 2) };
            out.push(format!("{head} {part}").trim_end().to_string());
        }
    }
    out
}

/// `filesize`: one decimal, in steps of 1024.
pub fn filesize(bytes: u64) -> String {
    for (shift, unit) in [(30, "GB"), (20, "MB"), (10, "KB")] {
        if bytes > 1 << shift {
            return format!("{:.1} {unit}", bytes as f64 / (1u64 << shift) as f64);
        }
    }
    format!("{bytes} B")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_columns_go() {
        let rows = vec![vec!["git".into(), String::new(), "2".into()], vec!["abcdefg".into(), String::new(), String::new()]];
        assert_eq!(lines(&["Name", "Info", "V"], &rows), ["Name    V", "----    -", "git     2", "abcdefg"]);
        assert_eq!(pairs(&[("A".into(), "1\n2".into()), ("Long".into(), "x".into())]), ["A    : 1", "       2", "Long : x"]);
    }

    #[test]
    fn sizes_as_scoop_writes_them() {
        assert_eq!(filesize(0), "0 B");
        assert_eq!(filesize(1024), "1024 B");
        assert_eq!(filesize(1536), "1.5 KB");
        assert_eq!(filesize(3 * (1 << 20) + 1), "3.0 MB");
    }
}
