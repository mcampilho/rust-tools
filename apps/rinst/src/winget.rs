//! What `winget list` says: the package ids winget knows, and the newer
//! versions its sources have. winget is slow to answer (seconds), so it runs
//! after the rest has been shown, and its rows are then joined to the
//! programs: by the registry key or the package it names (`ARP\…`, `MSIX\…`),
//! or else by name.

use crate::inventory::{Program, Scope, Source, WingetInfo};

/// One row of `winget list`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub name: String,
    pub id: String,
    pub version: String,
    pub available: String,
    pub source: String,
}

/// Reads the table winget prints: the columns start where the words of the
/// header start (the line above the dashes); a long name ends in `…`.
pub fn parse(text: &str) -> Vec<Row> {
    let lines: Vec<&str> = text.lines().map(|l| l.rsplit('\r').next().unwrap_or(l)).collect();
    let Some(dash) = lines.iter().position(|l| l.trim().len() > 10 && l.trim().chars().all(|c| c == '-')) else { return Vec::new() };
    let Some(header) = dash.checked_sub(1).map(|i| lines[i]) else { return Vec::new() };
    let head: Vec<char> = header.chars().collect();
    let mut starts = Vec::new();
    for (i, c) in head.iter().enumerate() {
        if !c.is_whitespace() && (i == 0 || head[i - 1].is_whitespace()) {
            starts.push(i);
        }
    }
    if starts.len() < 3 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in &lines[dash + 1..] {
        let chars: Vec<char> = line.chars().collect();
        if chars.len() <= starts[1] {
            continue;
        }
        let cell = |k: usize| -> String {
            let a = starts[k].min(chars.len());
            let b = starts.get(k + 1).copied().unwrap_or(chars.len()).min(chars.len());
            chars[a..b].iter().collect::<String>().trim().to_string()
        };
        let (name, id) = (cell(0), cell(1));
        if id.is_empty() || name.is_empty() {
            continue;
        }
        // Five columns: name, id, version, available, source; four when
        // nothing has an update (no "available").
        let (version, available, source) = match starts.len() {
            n if n >= 5 => (cell(2), cell(3), cell(4)),
            4 => (cell(2), String::new(), cell(3)),
            _ => (cell(2), String::new(), String::new()),
        };
        out.push(Row { name, id, version, available, source });
    }
    out
}

/// Runs `winget list` (no window); None when winget is not there or fails.
#[cfg(windows)]
pub fn run() -> Option<String> {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("winget")
        .args(["list", "--disable-interactivity", "--accept-source-agreements"])
        .creation_flags(NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    (!text.trim().is_empty()).then_some(text)
}

#[cfg(not(windows))]
pub fn run() -> Option<String> {
    None
}

fn plain(s: &str) -> String {
    s.trim_end_matches('…').chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Joins winget's rows to the programs; rows that match none become
/// programs of their own.
pub fn merge(programs: &mut Vec<Program>, rows: Vec<Row>) {
    for r in rows {
        let info = WingetInfo { id: r.id.clone(), available: (!r.available.is_empty()).then(|| r.available.clone()), repo: r.source.clone() };
        let found = if let Some(rest) = r.id.strip_prefix("ARP\\") {
            // ARP\Machine\X64\<key> or ARP\User\X64\<key>.
            let mut parts = rest.splitn(3, '\\');
            let (who, _, key) = (parts.next().unwrap_or(""), parts.next(), parts.next().unwrap_or(""));
            programs.iter().position(|p| {
                p.sources.contains(&Source::Registry)
                    && p.key.rsplit('\\').next().is_some_and(|k| k.eq_ignore_ascii_case(key))
                    && (who != "User" || matches!(p.scope, Scope::User { current: true, .. }))
            })
        } else if let Some(full) = r.id.strip_prefix("MSIX\\") {
            programs.iter().position(|p| p.package.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(full)))
        } else {
            None
        };
        let found = found.or_else(|| {
            let n = plain(&r.name);
            let truncated = r.name.ends_with('…');
            let same = |p: &Program| {
                let pn = plain(&p.name);
                pn == n || truncated && pn.starts_with(&n)
            };
            // The same name and version first, then the same name.
            programs.iter().position(|p| same(p) && p.version == r.version && p.winget.is_none()).or_else(|| programs.iter().position(|p| same(p) && p.winget.is_none()))
        });
        match found {
            Some(i) => {
                let p = &mut programs[i];
                if !p.sources.contains(&Source::Winget) {
                    p.sources.push(Source::Winget);
                }
                p.winget = Some(info);
            }
            None => programs.push(Program {
                name: r.name.trim_end_matches('…').to_string(),
                version: r.version,
                sources: vec![Source::Winget],
                key: format!("winget: {}", r.id),
                winget: Some(info),
                ..Program::default()
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "   - \r   \\ \r
Name                     Id                                   Version      Available    Source
--------------------------------------------------------------------------------------------------
ChatGPT                  OpenAI.ChatGPT                       26.1103.0    26.1106.0    winget
Bitdefender Total Sec…   ARP\\Machine\\X64\\Bitdefender          27.0.65.370
Microsoft Edge           Microsoft.Edge                       142.0.1                   winget
Paint                    MSIX\\Microsoft.Paint_11.2_x64__8we   11.2
5 upgrades available.
";

    #[test]
    fn reads_the_table() {
        let rows = parse(LIST);
        assert_eq!(rows.len(), 4, "{rows:#?}");
        assert_eq!(rows[0], Row { name: "ChatGPT".into(), id: "OpenAI.ChatGPT".into(), version: "26.1103.0".into(), available: "26.1106.0".into(), source: "winget".into() });
        assert_eq!(rows[1].id, "ARP\\Machine\\X64\\Bitdefender");
        assert_eq!(rows[2].available, "");
        assert_eq!(rows[2].source, "winget");
        assert!(parse("No installed package found.").is_empty());
    }

    #[test]
    fn joins_to_the_programs() {
        let mut programs = vec![
            Program { name: "ChatGPT".into(), version: "26.1103.0".into(), sources: vec![Source::Store], package: Some("OpenAI.Codex_26".into()), ..Program::default() },
            Program { name: "Bitdefender Total Security".into(), sources: vec![Source::Registry], key: "HKEY_LOCAL_MACHINE\\…\\Uninstall\\Bitdefender".into(), ..Program::default() },
            Program { name: "Paint".into(), sources: vec![Source::Store], package: Some("Microsoft.Paint_11.2_x64__8we".into()), ..Program::default() },
        ];
        merge(&mut programs, parse(LIST));
        assert_eq!(programs.len(), 4);
        assert_eq!(programs[0].winget.as_ref().map(|w| w.available.clone()), Some(Some("26.1106.0".into())));
        assert_eq!(programs[0].sources, [Source::Store, Source::Winget]);
        assert!(programs[1].sources.contains(&Source::Winget));
        assert!(programs[2].sources.contains(&Source::Winget));
        // Edge is not among the programs: a row of its own.
        assert_eq!((programs[3].name.as_str(), programs[3].sources.as_slice()), ("Microsoft Edge", &[Source::Winget][..]));
    }
}
