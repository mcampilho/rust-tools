//! Windows' own tools, run without a window: PowerShell for what Windows
//! only says there (volumes, Defender, the network) and plain programs
//! (`netsh`, `MpCmdRun`). What they print is read by each module's `parse`,
//! which the tests check without Windows.

/// Runs a PowerShell script; what it printed, or None when it could not run or failed.
#[cfg(windows)]
pub fn run(script: &str) -> Option<String> {
    let out = output("powershell.exe", &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(windows)]
fn output(exe: &str, args: &[&str]) -> Option<std::process::Output> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .ok()
}

#[cfg(not(windows))]
pub fn run(_script: &str) -> Option<String> {
    None
}

/// Runs a program and waits: (whether it ended well, what it printed, out and err). None: it could not be started.
#[cfg(windows)]
pub fn command(exe: &str, args: &[&str]) -> Option<(bool, String)> {
    let out = output(exe, args)?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        text.push_str(&err);
    }
    Some((out.status.success(), text))
}

#[cfg(not(windows))]
pub fn command(_exe: &str, _args: &[&str]) -> Option<(bool, String)> {
    None
}

/// The last line that says something: what a tool that failed printed last.
pub fn last_line(text: &str) -> String {
    text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("").to_string()
}

/// A text for a PowerShell single-quoted string (`'` doubled).
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_quotes() {
        assert_eq!(last_line("one\r\n\r\ntwo \r\n  \n"), "two");
        assert_eq!(last_line(""), "");
        assert_eq!(quote("Wi-Fi 'home'"), "'Wi-Fi ''home'''");
    }
}
