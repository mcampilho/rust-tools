//! How rcare starts: as administrator by default (the user's choice,
//! 2026-11-23: most of what rcare is for — fixing and improving Windows —
//! needs it). `--user` starts it without asking. Resident start-ups
//! (`--tray`, used by "Start with Windows") never ask, or Windows would show
//! its prompt at every sign-in; File > Restart as Administrator stays.

/// The argument that starts rcare without administrator rights.
pub const USER: &str = "--user";

/// Whether this start should ask Windows for administrator rights.
pub fn wants_elevation(args: &[String], elevated: bool) -> bool {
    let has = |names: &[&str]| args.iter().any(|a| names.contains(&a.as_str()));
    !elevated && !has(&[USER, "/user", "--tray", "/tray", "--help", "-h", "/?", "--register", "--unregister", "--run"])
}

/// The command line for the elevated start: the same arguments, each quoted
/// when it needs it (folders with spaces; a drive's `C:\` keeps its slash
/// out of the quote's way).
pub fn relaunch_args(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if a.is_empty() || a.contains([' ', '\t', '"']) {
                // A trailing backslash would escape the closing quote.
                let body = a.replace('"', "");
                let body = if body.ends_with('\\') { format!("{body}\\") } else { body };
                format!("\"{body}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn who_asks() {
        assert!(wants_elevation(&v(&[]), false));
        assert!(wants_elevation(&v(&["C:\\Users"]), false));
        assert!(!wants_elevation(&v(&[]), true), "already administrator");
        assert!(!wants_elevation(&v(&["--user"]), false));
        assert!(!wants_elevation(&v(&["--tray"]), false), "no prompt at every sign-in");
        assert!(!wants_elevation(&v(&["--help"]), false));
        assert!(!wants_elevation(&v(&["--register"]), false));
        assert!(!wants_elevation(&v(&["--run", "sfc"]), false), "a scheduled run is started with the rights Task Scheduler gives");
    }

    #[test]
    fn arguments_are_passed_on() {
        assert_eq!(relaunch_args(&v(&[])), "");
        assert_eq!(relaunch_args(&v(&["--new-window", "D:\\Fotos"])), "--new-window D:\\Fotos");
        assert_eq!(relaunch_args(&v(&["C:\\Program Files\\X"])), "\"C:\\Program Files\\X\"");
        assert_eq!(relaunch_args(&v(&["C:\\My Stuff\\"])), "\"C:\\My Stuff\\\\\"");
    }
}
