//! The health of the drives, as Windows itself keeps it: `Get-Volume` (no
//! administrator rights needed) says whether a volume is healthy or needs a
//! repair. Nothing is changed here; repairing is `chkdsk` (later, on the
//! Repair page).

/// A drive with a letter, as `Get-Volume` describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub letter: char,
    /// `Healthy`, `Warning`, `Unhealthy` (Windows' own words, the same in every language).
    pub health: String,
    /// What Windows says about it, such as `OK` or `Full Repair Needed`.
    pub status: String,
    pub file_system: String,
    pub label: String,
}

impl Volume {
    /// Windows says it needs looking at.
    pub fn needs_repair(&self) -> bool {
        !self.health.eq_ignore_ascii_case("Healthy")
    }
}

/// The PowerShell that lists the volumes, one per line: letter|health|status|file system|label.
pub const SCRIPT: &str = "Get-Volume | Where-Object DriveLetter | ForEach-Object { '{0}|{1}|{2}|{3}|{4}' -f $_.DriveLetter, $_.HealthStatus, $_.OperationalStatus, $_.FileSystem, $_.FileSystemLabel }";

/// The lines `SCRIPT` prints, read.
pub fn parse(text: &str) -> Vec<Volume> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.trim_end_matches('\r').splitn(5, '|');
            let letter = f.next()?.trim().chars().next().filter(char::is_ascii_alphabetic)?;
            let health = f.next()?.trim().to_string();
            if health.is_empty() {
                return None;
            }
            let status = f.next().unwrap_or("").trim().to_string();
            let file_system = f.next().unwrap_or("").trim().to_string();
            let label = f.next().unwrap_or("").trim().to_string();
            Some(Volume { letter: letter.to_ascii_uppercase(), health, status, file_system, label })
        })
        .collect()
}

/// Asks Windows, without a window; None when PowerShell could not be run.
#[cfg(windows)]
pub fn read() -> Option<Vec<Volume>> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .ok()?;
    out.status.success().then(|| parse(&String::from_utf8_lossy(&out.stdout)))
}

#[cfg(not(windows))]
pub fn read() -> Option<Vec<Volume>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_get_volume_prints_is_read() {
        let v = parse("J|Healthy|Unknown||\r\nE|Healthy|OK|NTFS|FILES\r\nS|Warning|Full Repair Needed|exFAT|SOFTWARE\r\n\r\n|Healthy|OK|NTFS|x\nc|Healthy|OK|NTFS|a|b\n");
        assert_eq!(v.len(), 4);
        assert_eq!(
            v[2],
            Volume {
                letter: 'S',
                health: "Warning".into(),
                status: "Full Repair Needed".into(),
                file_system: "exFAT".into(),
                label: "SOFTWARE".into()
            }
        );
        assert!(v[2].needs_repair() && !v[1].needs_repair());
        // The label keeps a `|` of its own.
        assert_eq!((v[3].letter, v[3].label.as_str()), ('C', "a|b"));
    }

    /// The drives of this PC, as Windows sees them.
    #[test]
    #[ignore]
    fn here() {
        for v in read().expect("PowerShell") {
            println!("{v:?}");
        }
    }
}
