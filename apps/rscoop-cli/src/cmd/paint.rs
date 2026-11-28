//! The colours of the output, when the setting `rscoop_color_output` is on
//! and the output is a terminal (never with `--json`, nor with `NO_COLOR`).
//! ANSI codes, as PowerShell 7 writes its tables: the headers in bold green.

/// Table headers.
pub const HEADER: &str = "32;1";
/// The dashes under the headers.
pub const RULE: &str = "32";
/// The name of an app, a shim, a bucket; the start of a step.
pub const NAME: &str = "36";
/// Done, up to date, the newest version.
pub const GOOD: &str = "32";
/// A warning, the installed (older) version, something held.
pub const WARN: &str = "33";
/// An error, a failure, something missing.
pub const BAD: &str = "31";
/// Something worth noting (global, deprecated, removed).
pub const NOTE: &str = "35";
/// What matters less (where an app came from, when).
pub const DIM: &str = "90";
/// Totals.
pub const BOLD: &str = "1";

/// `text` in the colour `code`.
pub fn wrap(code: &str, text: &str) -> String {
    format!("\x1b[{code}m{text}\x1b[0m")
}

/// The colour of a cell of a table, by the header of its column.
pub fn cell(header: &str, text: &str) -> Option<&'static str> {
    match header {
        "Name" => Some(NAME),
        "Version" | "Latest" | "Latest Version" => Some(GOOD),
        "Installed" | "Installed Version" => Some(WARN),
        "Missing Dependencies" => Some(BAD),
        "Source" | "Updated" => Some(DIM),
        "Total" => Some(BOLD),
        "IsHidden" if text == "True" => Some(WARN),
        "Info" => Some(if text.contains("failed") || text.contains("removed") {
            BAD
        } else if text.contains("Held") {
            WARN
        } else {
            NOTE
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_by_their_column() {
        assert_eq!(wrap(GOOD, "ok"), "\x1b[32mok\x1b[0m");
        assert_eq!(cell("Info", "Held package"), Some(WARN));
        assert_eq!(cell("Info", "Install failed"), Some(BAD));
        assert_eq!(cell("IsHidden", "False"), None);
        assert_eq!(cell("Binaries", "rg.exe"), None);
    }
}
