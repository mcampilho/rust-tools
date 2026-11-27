//! Checking for a newer version of an app, only when the user asks
//! (approved on 2026-11-17): the releases of the GitHub repository, tagged
//! `<app>-v<version>`, each with `<app>-<version>-windows-x86_64.zip` or
//! `…-linux-x86_64.tar.gz` and a `SHA256SUMS.txt`.
//!
//! Installing downloads the package, checks it against the published sum,
//! unpacks it with the system's `tar` (Windows 10 has it, and it reads zip)
//! and puts the new program in place of this one. A running Windows
//! program cannot be overwritten but can be renamed: the old one becomes
//! `<app>.exe.old` and goes at the next start ([`clean_old`]). An app that
//! Scoop installed is left to Scoop ([`scoop_update`]), so that Scoop knows
//! which version is there.

use std::path::{Path, PathBuf};

/// Where the releases are.
pub const REPO: &str = "mcampilho/rust-tools";

/// The largest package and sums file accepted.
const MAX_PACKAGE: usize = 64 << 20;
const MAX_LIST: usize = 8 << 20;

/// A release of an app, for this system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    /// The release's web page (its notes).
    pub page: String,
    /// The package for this system, and where to download it.
    pub asset: String,
    pub asset_url: String,
    /// `SHA256SUMS.txt`, if published.
    pub sums_url: Option<String>,
}

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    UpToDate,
    Newer(Release),
}

/// "1.2.10" as numbers; None if it is not one.
pub fn parse_version(v: &str) -> Option<Vec<u64>> {
    let v = v.trim().trim_start_matches('v');
    let parts: Option<Vec<u64>> = v.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| !p.is_empty())
}

/// Whether version `a` comes after `b`.
pub fn is_newer(a: &str, b: &str) -> bool {
    match (parse_version(a), parse_version(b)) {
        (Some(mut x), Some(mut y)) => {
            let n = x.len().max(y.len());
            x.resize(n, 0);
            y.resize(n, 0);
            x > y
        }
        _ => false,
    }
}

/// The package of `app` version `v` for this system.
pub fn asset_name(app: &str, v: &str) -> String {
    if cfg!(windows) {
        format!("{app}-{v}-windows-x86_64.zip")
    } else {
        format!("{app}-{v}-linux-x86_64.tar.gz")
    }
}

/// The newest release of `app` in GitHub's list of releases (JSON), with a
/// package for this system; drafts and pre-releases are left out.
pub fn latest(app: &str, list: &str) -> Option<Release> {
    let json = rt_core::json::parse(list).ok()?;
    let prefix = format!("{app}-v");
    json.items()
        .iter()
        .filter(|r| {
            !matches!(r.get("draft"), rt_core::json::Json::Bool(true)) && !matches!(r.get("prerelease"), rt_core::json::Json::Bool(true))
        })
        .filter_map(|r| {
            let version = r.get("tag_name").str().strip_prefix(&prefix)?.to_string();
            parse_version(&version)?;
            let want = asset_name(app, &version);
            let assets = r.get("assets").items();
            let url_of =
                |name: &str| assets.iter().find(|a| a.get("name").str() == name).map(|a| a.get("browser_download_url").str().to_string());
            Some(Release {
                asset_url: url_of(&want)?,
                sums_url: url_of("SHA256SUMS.txt"),
                asset: want,
                page: r.get("html_url").str().to_string(),
                version,
            })
        })
        .reduce(|a, b| if is_newer(&b.version, &a.version) { b } else { a })
}

/// The sum given for `file` in a `sha256sum` list.
pub fn sum_of(list: &str, file: &str) -> Option<String> {
    list.lines().find_map(|l| {
        let (sum, name) = l.trim().split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == file).then(|| sum.to_ascii_lowercase())
    })
}

fn agent(app: &str, version: &str) -> String {
    format!("{app}/{version}")
}

/// Asks GitHub whether there is a version of `app` newer than `current`.
pub fn check(app: &str, current: &str) -> Result<Found, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=100");
    let r = crate::net::get(&url, MAX_LIST, &agent(app, current))?;
    match latest(app, &String::from_utf8_lossy(&r.body)) {
        Some(rel) if is_newer(&rel.version, current) => Ok(Found::Newer(rel)),
        _ => Ok(Found::UpToDate),
    }
}

/// Whether `exe` is under Scoop's folder for `app` (`…\scoop\apps\<app>\…`).
pub fn scoop_path(exe: &Path, app: &str) -> bool {
    let parts: Vec<String> = exe.components().map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase()).collect();
    parts.windows(3).any(|w| w[0] == "scoop" && w[1] == "apps" && w[2] == app.to_ascii_lowercase())
}

/// Whether `exe` is under `<root>\apps\<app>` for one of Scoop's roots.
pub fn under_scoop_root(exe: &Path, app: &str, roots: &[PathBuf]) -> bool {
    let lower = |p: &Path| p.to_string_lossy().to_ascii_lowercase().replace('/', "\\");
    let e = lower(exe);
    roots.iter().any(|r| e.starts_with(&format!("{}\\apps\\{}\\", lower(r).trim_end_matches('\\'), app.to_ascii_lowercase())))
}

/// Whether this program was installed by Scoop (in a folder called
/// `scoop`, or under `%SCOOP%` or `%SCOOP_GLOBAL%`).
pub fn installed_by_scoop(app: &str) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let roots: Vec<PathBuf> = ["SCOOP", "SCOOP_GLOBAL"].iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
    cfg!(windows) && (scoop_path(&exe, app) || under_scoop_root(&exe, app, &roots))
}

/// The old program left by an update, beside this one.
fn old_of(exe: &Path) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    exe.with_file_name(name)
}

/// Removes the program an update left behind (call at start).
pub fn clean_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_of(&exe));
    }
}

/// Downloads `rel`, checks its sum, unpacks it and puts the new program in
/// place of this one. Returns the program's path (to start it again).
pub fn install(app: &str, current: &str, rel: &Release) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    install_at(app, current, rel, &exe)?;
    Ok(exe)
}

/// [`install`], into `exe` (tests put it in a folder of their own).
pub fn install_at(app: &str, current: &str, rel: &Release, exe: &Path) -> Result<(), String> {
    let exe = exe.to_path_buf();
    let ua = agent(app, current);
    let package = crate::net::get(&rel.asset_url, MAX_PACKAGE, &ua)?.body;
    // Without a published sum, nothing is installed.
    let sums_url = rel.sums_url.as_deref().ok_or("SHA256SUMS.txt is missing from the release")?;
    let sums = crate::net::get(sums_url, MAX_LIST, &ua)?.body;
    let want = sum_of(&String::from_utf8_lossy(&sums), &rel.asset).ok_or("the package has no published SHA-256 sum")?;
    let got = rt_core::sha256::hex(&package);
    if got != want {
        return Err(format!("the download does not match its SHA-256 sum ({got} instead of {want})"));
    }
    let dir = std::env::temp_dir().join(format!("{app}-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let result = unpack_and_place(app, rel, &package, &dir, &exe);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn unpack_and_place(app: &str, rel: &Release, package: &[u8], dir: &Path, exe: &Path) -> Result<(), String> {
    let file = dir.join(&rel.asset);
    std::fs::write(&file, package).map_err(|e| e.to_string())?;
    let mut tar = std::process::Command::new("tar");
    tar.arg("-xf").arg(&file).arg("-C").arg(dir);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        tar.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = tar.output().map_err(|e| format!("tar: {e}"))?;
    if !out.status.success() {
        return Err(format!("tar: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let program = if cfg!(windows) { format!("{app}.exe") } else { app.to_string() };
    let folder = rel.asset.trim_end_matches(".zip").trim_end_matches(".tar.gz");
    let new = dir.join(folder).join(&program);
    if !new.is_file() {
        return Err(format!("{program} is not in the package"));
    }
    replace(exe, &new)
}

/// Puts `new` where `exe` is; the running one is renamed out of the way and
/// put back if the copy fails.
pub fn replace(exe: &Path, new: &Path) -> Result<(), String> {
    let old = old_of(exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).map_err(|e| format!("{}: {e}", exe.display()))?;
    if let Err(e) = std::fs::copy(new, exe) {
        let _ = std::fs::remove_file(exe);
        let _ = std::fs::rename(&old, exe);
        return Err(format!("{}: {e}", exe.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// Starts the program at `exe` again, a moment after this one ends (so that
/// it does not hand itself over to this one), with the options this one
/// was started with (`--tray`...), not its files.
pub fn restart(exe: &Path) -> Result<(), String> {
    let flags: Vec<String> = std::env::args().skip(1).filter(|a| a.starts_with("--")).collect();
    #[cfg(windows)]
    {
        restart_command(exe, &flags).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("sh")
            .arg("-c")
            .arg("sleep 2; exec \"$0\" \"$@\"")
            .arg(exe)
            .args(&flags)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// The command line of [`restart`], for `cmd`: waits about two seconds, then
/// starts `exe`. `ping` does the waiting, because `timeout` gives up at once
/// without a console to read from.
#[cfg(windows)]
fn restart_line(exe: &Path, flags: &[String]) -> String {
    let flags: String = flags.iter().map(|f| format!(" \"{f}\"")).collect();
    format!("ping -n 3 127.0.0.1 >nul & start \"\" \"{}\"{flags}", exe.display())
}

/// The line goes to `cmd` as it is (`raw_arg`): `Command`'s own quoting
/// writes `\"`, which `cmd` does not read, and `start` then looked for `\\`.
#[cfg(windows)]
fn restart_command(exe: &Path, flags: &[String]) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("cmd");
    cmd.raw_arg(format!("/d /s /c \"{}\"", restart_line(exe, flags))).creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

/// Opens a console that waits for this program to end and runs
/// `scoop update <app>` (the caller then ends).
pub fn scoop_update(app: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        // `start` gives it a window of its own; `pause` keeps Scoop's answer
        // on the screen.
        let script = format!("timeout /t 2 /nobreak >nul & scoop update {app} & pause");
        std::process::Command::new("cmd")
            .args(["/c", "start", &format!("scoop update {app}"), "cmd", "/c", &script])
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("Scoop is only on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"[
      {"tag_name":"rpad-v1.3.0","draft":false,"prerelease":true,"html_url":"p130","assets":[]},
      {"tag_name":"rpad-v1.2.10","draft":false,"prerelease":false,"html_url":"p1210",
       "assets":[{"name":"rpad-1.2.10-windows-x86_64.zip","browser_download_url":"w1210"},
                 {"name":"rpad-1.2.10-linux-x86_64.tar.gz","browser_download_url":"l1210"},
                 {"name":"SHA256SUMS.txt","browser_download_url":"s1210"}]},
      {"tag_name":"rpad-v1.2.9","draft":false,"prerelease":false,"html_url":"p129",
       "assets":[{"name":"rpad-1.2.9-windows-x86_64.zip","browser_download_url":"w129"},
                 {"name":"rpad-1.2.9-linux-x86_64.tar.gz","browser_download_url":"l129"}]},
      {"tag_name":"rview-v9.0.0","draft":false,"prerelease":false,"html_url":"x","assets":[]},
      {"tag_name":"rpad-v2.0.0","draft":true,"prerelease":false,"html_url":"d","assets":[]}
    ]"#;

    #[test]
    fn versions() {
        assert!(is_newer("1.2.10", "1.2.9"));
        assert!(is_newer("1.3", "1.2.9"));
        assert!(!is_newer("1.2.0", "1.2"));
        assert!(!is_newer("1.2.9", "1.2.10"));
        assert!(!is_newer("x", "1.0"));
        assert_eq!(parse_version("v0.1.2"), Some(vec![0, 1, 2]));
    }

    #[test]
    fn the_newest_release_with_a_package_for_here() {
        let r = latest("rpad", LIST).unwrap();
        assert_eq!(r.version, "1.2.10");
        assert_eq!(r.page, "p1210");
        assert_eq!(r.asset_url, if cfg!(windows) { "w1210" } else { "l1210" });
        assert_eq!(r.sums_url.as_deref(), Some("s1210"));
        assert_eq!(latest("rsize", LIST), None);
        assert_eq!(latest("rpad", "not json"), None);
    }

    #[test]
    fn sums() {
        let list = "abc123  rpad-1.2.10-windows-x86_64.zip\nDEF456 *rpad-1.2.10-linux-x86_64.tar.gz\n";
        assert_eq!(sum_of(list, "rpad-1.2.10-windows-x86_64.zip").as_deref(), Some("abc123"));
        assert_eq!(sum_of(list, "rpad-1.2.10-linux-x86_64.tar.gz").as_deref(), Some("def456"));
        assert_eq!(sum_of(list, "other"), None);
    }

    /// Scoop is a Windows thing (and these are Windows paths).
    #[cfg(windows)]
    #[test]
    fn scoop_folders() {
        assert!(scoop_path(Path::new(r"E:\Portable\scoop\apps\rpad\current\rpad.exe"), "rpad"));
        assert!(scoop_path(Path::new(r"C:\Users\a\scoop\apps\RPAD\1.2.0\rpad.exe"), "rpad"));
        assert!(!scoop_path(Path::new(r"E:\Projects\Git\rpad.exe"), "rpad"));
        assert!(!scoop_path(Path::new(r"E:\scoop\apps\rview\current\rpad.exe"), "rpad"));
        let roots = [PathBuf::from(r"D:\Apps\")];
        assert!(under_scoop_root(Path::new(r"d:\apps\apps\rpad\current\rpad.exe"), "rpad", &roots));
        assert!(!under_scoop_root(Path::new(r"D:\Apps\rpad.exe"), "rpad", &roots));
    }

    /// `cmd` gets the quotes as written, not `\"` (it then looked for `\\`).
    #[cfg(windows)]
    #[test]
    fn the_restart_line_reaches_cmd_as_written() {
        let exe = Path::new(r"C:\a folder & more\app.exe");
        let flags = ["--tray".to_string()];
        let line = restart_line(exe, &flags);
        assert!(line.ends_with(r#"& start "" "C:\a folder & more\app.exe" "--tray""#), "{line}");
        let cmd = restart_command(exe, &flags);
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, [format!("/d /s /c \"{line}\"")]);
    }
    #[test]
    fn a_program_is_replaced_and_the_old_one_kept_aside() {
        let d = std::env::temp_dir().join(format!("rt-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let (exe, new) = (d.join("app.exe"), d.join("new.exe"));
        std::fs::write(&exe, "old").unwrap();
        std::fs::write(&new, "new").unwrap();
        replace(&exe, &new).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(old_of(&exe)).unwrap(), "old");
        // A failed copy puts the old one back.
        assert!(replace(&exe, &d.join("missing.exe")).is_err());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// Asks GitHub for real (run on request: it needs the network).
    #[test]
    #[ignore]
    fn github_answers() {
        let found = check("rview", "0.0.1").unwrap();
        assert!(matches!(found, Found::Newer(ref r) if r.sums_url.is_some()), "{found:?}");
        assert_eq!(check("rview", "999.0.0").unwrap(), Found::UpToDate);
    }

    /// Downloads rview's newest package, checks its sum, unpacks it and
    /// puts it in place of a stand-in program in a folder of its own (run
    /// on request: it needs the network).
    #[test]
    #[ignore]
    fn installs_a_real_release() {
        let Found::Newer(rel) = check("rview", "0.0.1").unwrap() else { panic!("no release") };
        let d = std::env::temp_dir().join(format!("rt-update-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let exe = d.join(if cfg!(windows) { "rview.exe" } else { "rview" });
        std::fs::write(&exe, "stand-in").unwrap();
        install_at("rview", "0.0.1", &rel, &exe).unwrap();
        let new = std::fs::read(&exe).unwrap();
        assert!(new.len() > 1 << 20, "{} bytes", new.len());
        assert!(new.starts_with(if cfg!(windows) { b"MZ" } else { b"\x7fELF" }));
        assert_eq!(std::fs::read_to_string(old_of(&exe)).unwrap(), "stand-in");
        std::fs::remove_dir_all(&d).unwrap();
    }
}
