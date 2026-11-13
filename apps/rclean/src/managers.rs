//! The caches of package managers (phase 2): what each one downloaded or
//! built, cleaned by its own command where it has one (`scoop cache rm`,
//! `pip cache purge`, `npm cache clean`, `apt-get clean`…), so it keeps its
//! own records right; Cargo and vcpkg have none, and lose the folders.
//!
//! A manager is listed when it is on the `PATH` (or its folder is there).
//! Where its cache is may only be known by asking it (`npm config get
//! cache`): that is done while analysing, on the worker thread.

use crate::targets::{at, Env, Filter, Group, Kind, Place, Target};
use std::path::{Path, PathBuf};

/// A command that cleans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cmd {
    pub file: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Linux: needs root (`pkexec` asks when rclean is not root).
    pub root: bool,
}

impl Cmd {
    pub fn new(file: impl Into<PathBuf>, args: &[&str]) -> Cmd {
        Cmd { file: file.into(), args: args.iter().map(|a| a.to_string()).collect(), env: Vec::new(), root: false }
    }

    fn root(mut self) -> Cmd {
        self.root = true;
        self
    }

    /// As shown in the details.
    pub fn text(&self) -> String {
        let name = self.file.file_name().map_or_else(|| self.file.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
        let mut parts = vec![name];
        parts.extend(self.args.iter().cloned());
        parts.join(" ")
    }
}

/// Where a cache is, when only its manager knows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Locate {
    /// Ask a command; the folder is the first line it prints, then `sub`.
    Ask(Cmd, &'static str),
    /// Snap's disabled revisions (`snap list --all`).
    SnapRevisions,
}

/// A program file on the `PATH` (on Windows also `.exe`, `.cmd`, `.bat`; not
/// the Store's stand-ins in `WindowsApps`).
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    for dir in std::env::split_paths(&path) {
        if cfg!(windows) && dir.to_string_lossy().contains("WindowsApps") {
            continue;
        }
        for e in exts {
            let p = dir.join(format!("{name}{e}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// The program a Scoop shim stands for (`<shim>.shim`: `path = "…"`).
pub fn real_file(file: &Path) -> PathBuf {
    std::fs::read_to_string(file.with_extension("shim"))
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("path").map(|r| r.trim_start_matches([' ', '=']).trim().trim_matches('"').to_string())))
        .map_or_else(|| file.to_path_buf(), PathBuf::from)
}

/// A tool's command: `.cmd` scripts (npm, yarn, pnpm on Windows) through
/// `cmd /d /c`.
fn tool(name: &str, args: &[&str]) -> Option<Cmd> {
    let file = which(name)?;
    let script = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    Some(if script {
        let mut all = vec!["/d", "/c", name];
        all.extend(args);
        Cmd::new("cmd.exe", &all)
    } else {
        Cmd::new(file, args)
    })
}

/// The Python whose pip is cleaned: `python3`, `py`, then `python`.
fn python() -> Option<PathBuf> {
    ["python3", "py", "python"].iter().find_map(|n| which(n))
}

/// Marks a translation key (the test of the translations looks for it).
const fn k(s: &'static str) -> &'static str {
    s
}

fn manager(id: &str, group: &'static str, name: &'static str, about: &'static str, places: Vec<Place>, commands: Vec<Cmd>, default_on: bool) -> Target {
    Target { kind: if commands.is_empty() { Kind::Files } else { Kind::Command }, default_on, commands, ..Target::files(id, Group::Tool(group), name, about, places) }
}

fn all_of(dirs: &[PathBuf]) -> Vec<Place> {
    dirs.iter().map(|d| Place { dir: d.clone(), filter: Filter::All }).collect()
}

/// Scoop's folder: `SCOOP`, or `~\scoop`.
fn scoop_root(e: &Env) -> Option<PathBuf> {
    std::env::var_os("SCOOP").map(PathBuf::from).or_else(|| Some(e.home.join("scoop"))).filter(|r| r.join("apps").is_dir())
}

/// Versions of Scoop's apps that are not the current one (`apps\<app>\<version>`).
pub fn scoop_old_versions(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for app in std::fs::read_dir(root.join("apps")).into_iter().flatten().flatten() {
        let current = std::fs::read_link(app.path().join("current")).ok().and_then(|t| t.file_name().map(|n| n.to_os_string()));
        let Some(current) = current else { continue };
        for v in std::fs::read_dir(app.path()).into_iter().flatten().flatten() {
            let n = v.file_name();
            if n != "current" && n != current && v.file_type().is_ok_and(|t| t.is_dir()) {
                out.push(v.path());
            }
        }
    }
    out.sort();
    out
}

/// The package managers found, with their caches.
pub fn targets(e: &Env) -> Vec<Target> {
    let mut out = Vec::new();
    let about_dl = k("Packages it downloaded. It downloads them again when they are needed.");
    if let Some(root) = scoop_root(e) {
        let script = root.join("apps").join("scoop").join("current").join("bin").join("scoop.ps1");
        let scoop = |args: &[&str]| {
            let mut all = vec!["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"];
            let s = script.to_string_lossy().into_owned();
            all.push(&s);
            all.extend(args);
            let mut c = Cmd::new("powershell.exe", &all);
            c.env.push(("SCOOP".into(), root.to_string_lossy().into_owned()));
            c
        };
        out.push(manager(
            "scoop.cache",
            "Scoop",
            k("Downloads"),
            k("The installers Scoop downloaded. It downloads them again to install or update."),
            all_of(&[root.join("cache")]),
            vec![scoop(&["cache", "rm", "*"])],
            true,
        ));
        out.push(manager(
            "scoop.old",
            "Scoop",
            k("Old versions"),
            k("Versions of apps kept after an update. Without them, scoop reset cannot go back to an older one."),
            all_of(&scoop_old_versions(&root)),
            vec![scoop(&["cleanup", "*"])],
            false,
        ));
    }
    if let Some(choco) = which("choco") {
        out.push(Target { admin: true, ..manager("choco.cache", "Chocolatey", k("Cache"), about_dl, all_of(&[e.temp.join("chocolatey")]), vec![Cmd::new(choco, &["cache", "remove", "-y"])], true) });
    }
    if let Some(py) = python() {
        let dir = std::env::var_os("PIP_CACHE_DIR").map(PathBuf::from).unwrap_or_else(|| if cfg!(windows) { at(&e.local, "pip/cache") } else { e.cache.join("pip") });
        out.push(manager("pip.cache", "pip", k("Cache"), k("Packages pip downloaded and built. It downloads them again when they are needed."), all_of(&[dir]), vec![Cmd::new(py, &["-m", "pip", "cache", "purge"])], true));
    }
    let asked = |id: &str, group: &'static str, name: &'static str, about: &'static str, clean: Option<Cmd>, ask: Option<Cmd>, sub: &'static str, on: bool| {
        let (clean, ask) = (clean?, ask?);
        Some(Target { locate: Some(Locate::Ask(ask, sub)), ..manager(id, group, name, about, Vec::new(), vec![clean], on) })
    };
    out.extend(asked("npm.cache", "npm", k("Cache"), about_dl, tool("npm", &["cache", "clean", "--force"]), tool("npm", &["config", "get", "cache"]), "_cacache", true));
    out.extend(asked("yarn.cache", "Yarn", k("Cache"), about_dl, tool("yarn", &["cache", "clean"]), tool("yarn", &["cache", "dir"]), "", true));
    out.extend(asked(
        "pnpm.store",
        "pnpm",
        k("Store"),
        k("Packages pnpm keeps for every project. Only those no project uses go."),
        tool("pnpm", &["store", "prune"]),
        tool("pnpm", &["store", "path"]),
        "",
        false,
    ));
    out.extend(asked("go.cache", "Go", k("Build cache"), k("What go build compiled. It is compiled again when needed."), tool("go", &["clean", "-cache"]), tool("go", &["env", "GOCACHE"]), "", true));
    if let Some(dotnet) = which("dotnet") {
        let dir = if cfg!(windows) { at(&e.local, "NuGet/v3-cache") } else { at(&e.home, ".local/share/NuGet/v3-cache") };
        out.push(manager(
            "nuget.http",
            "NuGet",
            k("HTTP cache"),
            k("Package lists and packages NuGet downloaded. The packages your projects use stay."),
            all_of(&[dir]),
            vec![Cmd::new(dotnet, &["nuget", "locals", "http-cache", "--clear"])],
            true,
        ));
    }
    let cargo_home = std::env::var_os("CARGO_HOME").map(PathBuf::from).unwrap_or_else(|| e.home.join(".cargo"));
    if cargo_home.join("registry").is_dir() || cargo_home.join("git").is_dir() {
        out.push(manager(
            "cargo.cache",
            "Cargo",
            k("Registry cache"),
            k("The crates Cargo downloaded and unpacked. It downloads them again when a project needs them."),
            all_of(&[at(&cargo_home, "registry/cache"), at(&cargo_home, "registry/src"), at(&cargo_home, "git/checkouts")]),
            Vec::new(),
            true,
        ));
    }
    let vcpkg = std::env::var_os("VCPKG_ROOT").map(PathBuf::from).or_else(|| which("vcpkg").map(|f| real_file(&f)).and_then(|f| f.parent().map(Path::to_path_buf)));
    if let Some(root) = vcpkg.filter(|r| r.join(".vcpkg-root").exists()) {
        out.push(manager(
            "vcpkg.build",
            "vcpkg",
            k("Downloads and build trees"),
            k("Sources vcpkg downloaded and the folders it built them in. Installed ports stay."),
            all_of(&[root.join("downloads"), root.join("buildtrees"), root.join("packages")]),
            Vec::new(),
            true,
        ));
    }
    if !cfg!(windows) {
        out.extend(linux_system());
    }
    out
}

/// Linux' own package managers (their caches are root's).
fn linux_system() -> Vec<Target> {
    let mut out = Vec::new();
    let about = k("Packages it downloaded to install or update. What is installed stays.");
    let sys = |id: &str, group: &'static str, dir: &str, cmd: Option<Cmd>| {
        let cmd = cmd?;
        Path::new(dir).is_dir().then(|| manager(id, group, k("Package cache"), about, all_of(&[PathBuf::from(dir)]), vec![cmd.root()], true))
    };
    out.extend(sys("apt.cache", "apt", "/var/cache/apt/archives", which("apt-get").map(|f| Cmd::new(f, &["clean"]))));
    out.extend(sys("dnf.cache", "dnf", "/var/cache/dnf", which("dnf").map(|f| Cmd::new(f, &["clean", "all"]))));
    out.extend(sys("zypper.cache", "zypper", "/var/cache/zypp/packages", which("zypper").map(|f| Cmd::new(f, &["--non-interactive", "clean", "--all"]))));
    out.extend(sys("pacman.cache", "pacman", "/var/cache/pacman/pkg", which("pacman").map(|f| Cmd::new(f, &["-Scc", "--noconfirm"]))));
    if let Some(f) = which("flatpak") {
        out.push(Target {
            measure: false,
            ..manager("flatpak.unused", "Flatpak", k("Unused runtimes"), k("Runtimes and extensions no installed app needs any more."), Vec::new(), vec![Cmd::new(f, &["uninstall", "--unused", "-y"])], false)
        });
    }
    if which("snap").is_some() {
        // Its commands are only known once its revisions are read.
        out.push(Target {
            locate: Some(Locate::SnapRevisions),
            kind: Kind::Command,
            ..manager("snap.old", "Snap", k("Old revisions"), k("Older revisions Snap keeps after each update, to go back to."), Vec::new(), Vec::new(), false)
        });
    }
    out
}

/// The disabled revisions of Snap (`snap list --all`): their files, and the
/// commands that remove them.
pub fn snap_disabled(text: &str) -> Vec<(PathBuf, Cmd)> {
    let mut lines = text.lines();
    let Some(head) = lines.next() else { return Vec::new() };
    let rev_col = head.split_whitespace().position(|w| w == "Rev").unwrap_or(2);
    lines
        .filter(|l| l.split_whitespace().last() == Some("disabled") || l.contains("disabled,") || l.contains(",disabled"))
        .filter_map(|l| {
            let w: Vec<&str> = l.split_whitespace().collect();
            let (name, rev) = (*w.first()?, *w.get(rev_col)?);
            let file = PathBuf::from(format!("/var/lib/snapd/snaps/{name}_{rev}.snap"));
            Some((file, Cmd::new("snap", &["remove", name, &format!("--revision={rev}")]).root()))
        })
        .collect()
}

/// What a command prints, when it ran well (no window on Windows).
pub fn output(c: &Cmd) -> Option<String> {
    let o = command(c).stderr(std::process::Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn command(c: &Cmd) -> std::process::Command {
    let mut p = std::process::Command::new(&c.file);
    p.args(&c.args).envs(c.env.iter().map(|(k, v)| (k, v))).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        p.creation_flags(0x0800_0000);
    }
    p
}

/// Whether rclean runs as root (Linux).
pub fn is_root() -> bool {
    std::fs::read_to_string("/proc/self/status").is_ok_and(|s| s.lines().any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(2) == Some("0")))
}

/// Runs a cleaning command; Err with its last lines when it did not work.
/// Root's commands go through `pkexec` (with their variables through `env`).
pub fn run(c: &Cmd) -> Result<(), String> {
    let elevated;
    let c = if c.root && !cfg!(windows) && !is_root() {
        let Some(pkexec) = which("pkexec") else { return Err(crate::i18n::tr("It needs root, and pkexec was not found: start rclean as root.")) };
        let mut args: Vec<String> = vec!["env".into()];
        args.extend(c.env.iter().map(|(k, v)| format!("{k}={v}")));
        args.push(c.file.to_string_lossy().into_owned());
        args.extend(c.args.iter().cloned());
        elevated = Cmd { file: pkexec, args, env: Vec::new(), root: false };
        &elevated
    } else {
        c
    };
    let o = command(c).output().map_err(|e| format!("{}: {e}", c.text()))?;
    if o.status.success() {
        return Ok(());
    }
    let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&o.stderr));
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines[lines.len().saturating_sub(3)..].join(" ");
    Err(format!("{} ({}): {tail}", c.text(), o.status.code().unwrap_or(-1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_revisions() {
        let t = "Name    Version   Rev    Tracking       Publisher   Notes\n\
                 core22  20260909  2987   latest/stable  canonical** base\n\
                 core22  20260801  2950   latest/stable  canonical** base,disabled\n\
                 genact  0.4.0     62     latest/stable  svenstaro   disabled\n";
        let d = snap_disabled(t);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].0, PathBuf::from("/var/lib/snapd/snaps/core22_2950.snap"));
        assert_eq!(d[1].1.text(), "snap remove genact --revision=62");
        assert!(d[1].1.root);
    }

    #[test]
    fn scoop_old_versions_and_shims() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("managers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let app = root.join("apps").join("rg");
        for v in ["14.0", "14.1"] {
            std::fs::create_dir_all(app.join(v)).unwrap();
        }
        let linked = {
            #[cfg(windows)]
            {
                // A junction, as Scoop makes (a symbolic link needs rights).
                std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(app.join("current")).arg(app.join("14.1")).output().is_ok_and(|o| o.status.success())
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(app.join("14.1"), app.join("current")).is_ok()
            }
        };
        if linked {
            assert_eq!(scoop_old_versions(&root), [app.join("14.0")]);
        }
        std::fs::write(root.join("vcpkg.shim"), "path = \"D:\\x\\vcpkg.exe\"\n").unwrap();
        assert_eq!(real_file(&root.join("vcpkg.exe")), PathBuf::from("D:\\x\\vcpkg.exe"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
