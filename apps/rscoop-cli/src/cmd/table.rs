//! Tables and lists as Scoop shows them, and sizes as it writes them.

/// The lines of a table: headers, a rule of dashes, one line per row. Columns
/// that are empty in every row are left out (the program draws through
/// `Ctx::table`).
#[cfg(test)]
pub fn lines(headers: &[&str], rows: &[Vec<String>]) -> Vec<String> {
    render(headers, rows, None)
}

/// A whole number or a size (`12`, `1.5 MB`, `0 B`), not a version (`1.2`).
fn numeric(text: &str) -> bool {
    match text.split_once(' ') {
        None => !text.is_empty() && text.chars().all(|c| c.is_ascii_digit() || c == ','),
        Some((number, unit)) => {
            !number.is_empty()
                && number.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',')
                && ["B", "KB", "MB", "GB", "TB"].contains(&unit)
        }
    }
}

/// The colour of a cell, by its column's header (see [`super::paint::cell`]).
pub type Style<'a> = &'a dyn Fn(&str, &str) -> Option<&'static str>;

/// [`lines`], with colours when `style` is given. Columns of numbers and
/// sizes are aligned to the right, as PowerShell aligns numbers.
pub fn render(headers: &[&str], rows: &[Vec<String>], style: Option<Style>) -> Vec<String> {
    use super::paint::{wrap, HEADER, RULE};
    let used: Vec<usize> = (0..headers.len()).filter(|&c| rows.iter().any(|r| !r[c].is_empty())).collect();
    let width = |c: usize| rows.iter().map(|r| r[c].chars().count()).chain([headers[c].chars().count()]).max().unwrap_or(0);
    let widths: Vec<usize> = used.iter().map(|&c| width(c)).collect();
    let right: Vec<bool> = used
        .iter()
        .map(|&c| {
            let cells: Vec<&str> = rows.iter().map(|r| r[c].as_str()).filter(|s| !s.is_empty()).collect();
            !cells.is_empty() && cells.iter().all(|s| numeric(s))
        })
        .collect();
    let paint = |code: Option<&str>, text: &str| match (style.is_some(), code) {
        (true, Some(code)) if !text.is_empty() => wrap(code, text),
        _ => text.to_string(),
    };
    let line = |cells: Vec<(&str, Option<&str>)>| {
        let last = cells.len().saturating_sub(1);
        let mut out = String::new();
        for (i, (text, code)) in cells.iter().enumerate() {
            let pad = " ".repeat(widths[i].saturating_sub(text.chars().count()));
            if i > 0 {
                out.push(' ');
            }
            if right[i] {
                out += &pad;
                out += &paint(*code, text);
            } else {
                out += &paint(*code, text);
                if i != last {
                    out += &pad;
                }
            }
        }
        out.trim_end().to_string()
    };
    let rule: Vec<String> = used.iter().map(|&c| "-".repeat(headers[c].chars().count())).collect();
    let mut out = vec![
        line(used.iter().map(|&c| (headers[c], Some(HEADER))).collect()),
        line(rule.iter().map(|r| (r.as_str(), Some(RULE))).collect()),
    ];
    out.extend(rows.iter().map(|r| line(used.iter().map(|&c| (r[c].as_str(), style.and_then(|s| s(headers[c], &r[c])))).collect())));
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
    fn numbers_to_the_right_and_colours_by_column() {
        let rows = vec![vec!["git".into(), "1.5 MB".into()], vec!["7zip".into(), "12 B".into()]];
        assert_eq!(lines(&["Name", "Size"], &rows), ["Name   Size", "----   ----", "git  1.5 MB", "7zip   12 B"]);
        let style: Style = &|header, _| (header == "Name").then_some("36");
        let colored = render(&["Name", "Size"], &rows, Some(style));
        assert_eq!(colored[0], "\x1b[32;1mName\x1b[0m   \x1b[32;1mSize\x1b[0m");
        assert_eq!(colored[2], "\x1b[36mgit\x1b[0m  1.5 MB");
    }
    #[test]
    fn sizes_as_scoop_writes_them() {
        assert_eq!(filesize(0), "0 B");
        assert_eq!(filesize(1024), "1024 B");
        assert_eq!(filesize(1536), "1.5 KB");
        assert_eq!(filesize(3 * (1 << 20) + 1), "3.0 MB");
    }
}
