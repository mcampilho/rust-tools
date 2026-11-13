//! The programs installed, as Windows lists them for "Apps and features":
//! the `Uninstall` keys of the system (64-bit and 32-bit views), of the user
//! running runinst, and of every other user whose key can be read: those with
//! a session open under `HKEY_USERS`, and, with administrator rights, the
//! others from their `NTUSER.DAT`.
//!
//! The system's keys can be read by anyone; other users' need administrator
//! rights. What could not be read is counted, so the window can say so.

use std::path::{Path, PathBuf};

/// For whom a program is installed.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Scope {
    /// Every user (the system's keys).
    #[default]
    Machine,
    /// One user: their name, and whether it is the user running runinst.
    User { name: String, current: bool },
}

/// 64-bit, 32-bit, or a user's key (where the two are one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Bits {
    B64,
    B32,
    #[default]
    Any,
}

/// Where a program was found. A program found in several places is listed
/// once, with all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    /// An `Uninstall` entry of the registry ("Apps and features").
    Registry,
    /// A Windows Installer product (`MsiEnumProductsEx`; what the old
    /// `Win32_Product` lists, without its side effects).
    Msi,
    /// A Store or MSIX package (`Get-AppxPackage`).
    Store,
    /// `winget list`.
    Winget,
    Scoop,
    Chocolatey,
    /// Linux: Debian's packages (`dpkg`, removed with `apt-get`).
    Dpkg,
    /// Linux: `rpm`'s packages (removed with `dnf`, `zypper` or `rpm`).
    Rpm,
    Flatpak,
    Snap,
}

/// What `winget list` says of a program.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct WingetInfo {
    /// The package's id (`OpenAI.ChatGPT`), or winget's name for an entry
    /// it does not know (`ARP\Machine\X64\…`, `MSIX\…`).
    pub id: String,
    /// A newer version in winget's sources.
    pub available: Option<String>,
    /// The source it comes from (`winget`, `msstore`), if any.
    pub repo: String,
}

/// A program, from its `Uninstall` entry, a package, or a package manager.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Program {
    pub name: String,
    pub publisher: String,
    pub version: String,
    /// `EstimatedSize`, in bytes.
    pub size: Option<u64>,
    /// `InstallDate` (year, month, day).
    pub date: Option<(i32, u32, u32)>,
    pub location: Option<PathBuf>,
    pub uninstall: Option<String>,
    pub quiet: Option<String>,
    pub modify: Option<String>,
    /// `DisplayIcon`: the file and the icon's index in it.
    pub icon: Option<(PathBuf, i32)>,
    /// A Windows Installer package: its product code (`{GUID}`).
    pub msi: Option<String>,
    /// `SystemComponent`: Windows hides it from its own list.
    pub system: bool,
    /// An update of another program (`ParentKeyName`, `ReleaseType`, a KB).
    pub update: bool,
    /// `NoRemove`: it says it cannot be removed.
    pub no_remove: bool,
    pub scope: Scope,
    pub bits: Bits,
    /// Where the entry is, as `regedit` writes it.
    pub key: String,
    /// Every value of the entry, as text.
    pub values: Vec<(String, String)>,
    /// Where it was found.
    pub sources: Vec<Source>,
    /// A Store or MSIX package: its full name, and the id of its first app
    /// (for its icon).
    pub package: Option<String>,
    pub app_id: Option<String>,
    pub winget: Option<WingetInfo>,
    /// What is wrong with it.
    pub problems: Vec<Problem>,
}

/// Something wrong with an installed program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// Scoop: its install did not finish (no `install.json`); dpkg: a
    /// package half installed.
    InstallFailed,
    /// Scoop: no bucket has its manifest any more.
    ManifestRemoved,
    /// Its uninstaller is gone (and its folder too, or it has none): an
    /// entry left by a program removed some other way.
    UninstallerMissing,
    /// dpkg: removed, but its settings in `/etc` are still there (removing
    /// it again purges them).
    ConfigOnly,
}

impl Program {
    /// Whether Windows' own list would show it.
    pub fn visible(&self) -> bool {
        !self.system && !self.update
    }
}

/// The user whose programs are "mine": the one logged on to the desktop,
/// even when runinst runs as another account (an administrator's, asked for
/// by Windows when it was restarted with administrator rights).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Me {
    pub sid: String,
    pub name: String,
    /// Their profile folder.
    pub home: Option<PathBuf>,
    /// runinst runs as this user (not as another account).
    pub runs_as: bool,
}

/// What was read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    pub me: Me,
    pub programs: Vec<Program>,
    /// Other users whose programs could not be read (no administrator rights,
    /// or their file in use and their key not open): their names.
    pub unread_users: Vec<String>,
    /// Other users whose programs were read: their names and profile folders
    /// (where their shortcuts are).
    pub other_users: Vec<(String, PathBuf)>,
}

/// `InstallDate` as Windows writes it: `20241105` (some write `5/11/2024`).
pub fn parse_date(s: &str) -> Option<(i32, u32, u32)> {
    let s = s.trim();
    if s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit()) {
        let (y, m, d) = (s[..4].parse().ok()?, s[4..6].parse().ok()?, s[6..].parse().ok()?);
        return ((1980..2200).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d)).then_some((y, m, d));
    }
    let parts: Vec<&str> = s.split(['/', '-', '.']).collect();
    if let [a, b, c] = parts[..] {
        let (a, b, c): (u32, u32, i32) = (a.parse().ok()?, b.parse().ok()?, c.parse().ok()?);
        // Month first (as the US writes it) unless that cannot be.
        let (m, d) = if a <= 12 { (a, b) } else { (b, a) };
        return ((1980..2200).contains(&c) && (1..=12).contains(&m) && (1..=31).contains(&d)).then_some((c, m, d));
    }
    None
}

/// `DisplayIcon`: `"C:\app\app.exe",0` → the file and the index.
pub fn parse_icon(s: &str) -> Option<(PathBuf, i32)> {
    let s = s.trim();
    let (file, index) = match s.rsplit_once(',') {
        Some((f, i)) if i.trim().parse::<i32>().is_ok() => (f, i.trim().parse().unwrap_or(0)),
        _ => (s, 0),
    };
    let file = file.trim().trim_matches('"').trim();
    (!file.is_empty()).then(|| (PathBuf::from(file), index))
}

/// Whether an entry's uninstaller is gone, and its folder with it (or it
/// names none): left by a program removed some other way. Windows
/// Installer's and commands of the system (`MsiExec.exe`, `rundll32`) do not
/// count.
pub fn uninstaller_gone(uninstall: Option<&str>, location: Option<&Path>, msi: bool) -> bool {
    if msi {
        return false;
    }
    let Some(file) = uninstall.and_then(command_file) else { return false };
    if !file.is_absolute() || file.exists() {
        return false;
    }
    location.is_none_or(|l| l.as_os_str().is_empty() || !l.exists())
}

/// The program a command runs: `"C:\x\unins000.exe" /SILENT` → `C:\x\unins000.exe`.
pub fn command_file(cmd: &str) -> Option<PathBuf> {
    let c = cmd.trim();
    if let Some(rest) = c.strip_prefix('"') {
        return rest.split('"').next().filter(|p| !p.is_empty()).map(PathBuf::from);
    }
    // Unquoted: up to the first ".exe" (paths with spaces are common).
    let lower = c.to_lowercase();
    match lower.find(".exe") {
        Some(i) => Some(PathBuf::from(&c[..i + 4])),
        None => c.split_whitespace().next().map(PathBuf::from),
    }
}

/// Whether an entry is an update: by its own marks, or a KB in the name.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_update(name: &str, release: Option<&str>, parent: bool) -> bool {
    let kb = name.contains("(KB") || name.starts_with("KB") && name[2..].chars().take(6).all(|c| c.is_ascii_digit());
    parent || kb || release.is_some_and(|r| matches!(r.to_lowercase().as_str(), "update" | "hotfix" | "security update" | "service pack"))
}

/// Adds the Windows Installer products: to the entry of the same product
/// code, or, without one, as an entry of their own (hidden as a system
/// component: such products are parts of others). `user_of`: a SID's name.
pub fn merge_msi(programs: &mut Vec<Program>, products: Vec<crate::msi::Product>, user_of: &dyn Fn(&str) -> Scope) {
    for m in products {
        let scope = m.user.as_deref().map_or(Scope::Machine, user_of);
        let same = programs.iter().position(|p| {
            p.sources.contains(&Source::Registry)
                && p.scope == scope
                && (p.msi.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&m.code)) || p.key.rsplit('\\').next().is_some_and(|k| k.eq_ignore_ascii_case(&m.code)))
        });
        match same {
            Some(i) => {
                let p = &mut programs[i];
                p.sources.push(Source::Msi);
                p.msi.get_or_insert(m.code);
            }
            None => programs.push(Program {
                name: m.name,
                publisher: m.publisher,
                version: m.version,
                date: m.date,
                location: m.location,
                uninstall: Some(format!("MsiExec.exe /X{}", m.code)),
                key: format!("Windows Installer: {}", m.code),
                msi: Some(m.code),
                system: true,
                scope,
                sources: vec![Source::Msi],
                ..Program::default()
            }),
        }
    }
}

/// Adds the Store and MSIX packages: one entry for each user who has one.
pub fn add_packages(programs: &mut Vec<Program>, packages: Vec<crate::appx::Package>, user_of: &dyn Fn(&str) -> Scope) {
    for pk in packages {
        let users: Vec<Scope> = if pk.users.is_empty() { vec![Scope::Machine] } else { pk.users.iter().map(|u| user_of(u)).collect() };
        for scope in users {
            let mut values = vec![("PackageFullName".to_string(), pk.full_name.clone())];
            if let Some(a) = &pk.app_id {
                values.push(("AppUserModelId".into(), a.clone()));
            }
            programs.push(Program {
                name: pk.display.clone(),
                publisher: pk.publisher.clone(),
                version: pk.version.clone(),
                date: pk.date,
                location: pk.location.clone(),
                system: pk.system,
                scope,
                key: format!("Package: {}", pk.full_name),
                values,
                sources: vec![Source::Store],
                package: Some(pk.full_name.clone()),
                app_id: pk.app_id.clone(),
                ..Program::default()
            });
        }
    }
}

#[cfg(windows)]
pub use imp::{me, read};

#[cfg(not(windows))]
/// The user running runinst.
pub fn me() -> Me {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    Me { sid: String::new(), name: std::env::var("USER").unwrap_or_default(), home, runs_as: true }
}

#[cfg(not(windows))]
/// The packages of dpkg and rpm (Flatpak and Snap have their own tabs).
pub fn read() -> Inventory {
    Inventory { me: me(), programs: crate::linux_pkgs::dpkg_and_rpm(), ..Inventory::default() }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use rt_sys::win::reg::{Key, Root, View};

    const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
    const PROFILES: &str = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ProfileList";

    /// One `Uninstall` key: every entry with a name.
    fn entries(key: &Key, view: View, scope: &Scope, bits: Bits, where_: &str, out: &mut Vec<Program>) {
        for sub in key.subkeys() {
            let Some(k) = key.sub(&sub, view) else { continue };
            let Some(name) = k.string("DisplayName") else { continue };
            let s = |n: &str| k.string(n);
            let flag = |n: &str| k.number(n).is_some_and(|v| v != 0);
            let release = s("ReleaseType");
            let parent = s("ParentKeyName").is_some();
            let guid = sub.starts_with('{') && sub.ends_with('}') && sub.len() == 38;
            out.push(Program {
                update: is_update(&name, release.as_deref(), parent),
                name,
                publisher: s("Publisher").unwrap_or_default(),
                version: s("DisplayVersion").unwrap_or_default(),
                size: k.number("EstimatedSize").filter(|&v| v > 0).map(|kb| kb * 1024),
                date: s("InstallDate").and_then(|d| parse_date(&d)),
                location: s("InstallLocation").map(|l| PathBuf::from(l.trim_matches('"'))),
                uninstall: s("UninstallString"),
                quiet: s("QuietUninstallString"),
                modify: s("ModifyPath"),
                icon: s("DisplayIcon").and_then(|i| parse_icon(&i)),
                msi: (flag("WindowsInstaller") && guid).then(|| sub.clone()),
                system: flag("SystemComponent"),
                no_remove: flag("NoRemove"),
                scope: scope.clone(),
                bits,
                key: format!("{where_}\\{sub}"),
                values: k.values().into_iter().map(|(n, v)| (if n.is_empty() { "(Default)".into() } else { n }, v.text())).collect(),
                sources: vec![Source::Registry],
                ..Program::default()
            });
            if let Some(p) = out.last_mut() {
                if uninstaller_gone(p.uninstall.as_deref(), p.location.as_deref(), p.msi.is_some()) {
                    p.problems.push(Problem::UninstallerMissing);
                }
            }
        }
    }

    /// The user of the desktop (see `Me`).
    pub fn me() -> Me {
        let token = rt_sys::win::user_sid().unwrap_or_default();
        let session = rt_sys::win::session_user();
        let sid = session.as_deref().and_then(rt_sys::win::sid_of).unwrap_or_else(|| token.clone());
        let name = session.unwrap_or_else(|| std::env::var("USERNAME").unwrap_or_default());
        let home = Key::open(Root::LocalMachine, &format!("{PROFILES}\\{sid}"), View::Bits64).and_then(|k| k.string("ProfileImagePath")).map(PathBuf::from);
        Me { runs_as: sid.eq_ignore_ascii_case(&token), sid, name, home }
    }

    pub fn read() -> Inventory {
        let mut inv = Inventory { me: me(), ..Inventory::default() };
        // The system: 64-bit and 32-bit views (the same key on 32-bit Windows).
        for (view, bits, path) in [
            (View::Bits64, Bits::B64, "HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
            (View::Bits32, Bits::B32, "HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        ] {
            if let Some(k) = Key::open(Root::LocalMachine, UNINSTALL, view) {
                entries(&k, view, &Scope::Machine, bits, path, &mut inv.programs);
            }
        }
        // The same entries in both views (a 32-bit Windows): once.
        let mut seen = std::collections::HashSet::new();
        inv.programs.retain(|p| seen.insert((p.key.rsplit('\\').next().unwrap_or("").to_string(), p.name.clone(), p.version.clone())));
        // The desktop's user: HKEY_CURRENT_USER, or their key under
        // HKEY_USERS when runinst runs as another account.
        let me = inv.me.sid.clone();
        let scope = Scope::User { name: inv.me.name.clone(), current: true };
        // A few 32-bit installers write under the user's own WOW6432Node.
        const UNINSTALL32: &str = "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
        for (path, bits) in [(UNINSTALL, Bits::Any), (UNINSTALL32, Bits::B32)] {
            let (k, place) = if inv.me.runs_as {
                (Key::open(Root::CurrentUser, path, View::Bits64), format!("HKEY_CURRENT_USER\\{path}"))
            } else {
                (Key::open(Root::Users, &format!("{me}\\{path}"), View::Bits64), format!("HKEY_USERS\\{me}\\{path}"))
            };
            if let Some(k) = k {
                entries(&k, View::Bits64, &scope, bits, &place, &mut inv.programs);
            }
        }
        // Every other user with a profile: under HKEY_USERS if their key is
        // open, else from their NTUSER.DAT (administrator rights).
        let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        let profiles = Key::open(Root::LocalMachine, PROFILES, View::Bits64);
        for sid in profiles.as_ref().map(|p| p.subkeys()).unwrap_or_default() {
            if let Some(p) = profiles.as_ref().and_then(|k| k.sub(&sid, View::Bits64)).and_then(|k| k.string("ProfileImagePath")) {
                let name = PathBuf::from(&p).file_name().map_or(sid.clone(), |n| n.to_string_lossy().into_owned());
                names.insert(sid.to_uppercase(), name);
            }
        }
        let my = inv.me.name.clone();
        let user_of = |sid: &str| -> Scope {
            if sid.eq_ignore_ascii_case(&me) {
                Scope::User { name: my.clone(), current: true }
            } else {
                Scope::User { name: names.get(&sid.to_uppercase()).cloned().unwrap_or_else(|| sid.to_string()), current: false }
            }
        };
        let admin = rt_sys::win::elevated();
        merge_msi(&mut inv.programs, crate::msi::products(admin), &user_of);
        add_packages(&mut inv.programs, crate::appx::packages(admin, &me), &user_of);
        let Some(profiles) = profiles else { return inv };
        for sid in profiles.subkeys() {
            if !sid.starts_with("S-1-5-21-") || sid.eq_ignore_ascii_case(&me) {
                continue;
            }
            let Some(p) = profiles.sub(&sid, View::Bits64).and_then(|k| k.string("ProfileImagePath")) else { continue };
            let home = PathBuf::from(&p);
            let name = home.file_name().map_or(sid.clone(), |n| n.to_string_lossy().into_owned());
            let scope = Scope::User { name: name.clone(), current: false };
            if let Some(k) = Key::open(Root::Users, &format!("{sid}\\{UNINSTALL}"), View::Bits64) {
                entries(&k, View::Bits64, &scope, Bits::Any, &format!("HKEY_USERS\\{sid}\\{UNINSTALL}"), &mut inv.programs);
                inv.other_users.push((name, home.clone()));
                continue;
            }
            // Their key may be open without an Uninstall key: nothing to show.
            if Key::open(Root::Users, &sid, View::Bits64).is_some() {
                inv.other_users.push((name, home.clone()));
                continue;
            }
            match Key::load_hive(&home.join("NTUSER.DAT")) {
                Ok(hive) => {
                    if let Some(k) = hive.sub(UNINSTALL, View::Bits64) {
                        entries(&k, View::Bits64, &scope, Bits::Any, &format!("{}\\NTUSER.DAT\\{UNINSTALL}", home.display()), &mut inv.programs);
                    }
                    inv.other_users.push((name, home.clone()));
                }
                Err(_) => inv.unread_users.push(name),
            }
        }
        inv
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uninstallers_gone() {
        let here = std::env::current_exe().unwrap();
        let exe = format!("\"{}\" /x", here.display());
        assert!(!uninstaller_gone(Some(&exe), None, false));
        let gone = if cfg!(windows) { "\"C:\\No such\\unins000.exe\"" } else { "\"/no/such/unins000.exe\"" };
        assert!(uninstaller_gone(Some(gone), None, false));
        // Its folder still there: not left alone.
        assert!(!uninstaller_gone(Some(gone), here.parent(), false));
        assert!(!uninstaller_gone(Some(gone), None, true));
        assert!(!uninstaller_gone(Some("MsiExec.exe /X{1}"), None, false));
        assert!(!uninstaller_gone(None, None, false));
    }

    #[test]
    fn dates_icons_commands() {
        assert_eq!(parse_date("20241105"), Some((2024, 11, 5)));
        assert_eq!(parse_date("5/11/2024"), Some((2024, 5, 11)));
        assert_eq!(parse_date("25/11/2024"), Some((2024, 11, 25)));
        assert_eq!(parse_date("20241341"), None);
        assert_eq!(parse_date("yesterday"), None);
        assert_eq!(parse_icon("\"C:\\app\\app.exe\",0"), Some((PathBuf::from("C:\\app\\app.exe"), 0)));
        assert_eq!(parse_icon("C:\\a b\\x.ico"), Some((PathBuf::from("C:\\a b\\x.ico"), 0)));
        assert_eq!(parse_icon("C:\\x.dll,-101"), Some((PathBuf::from("C:\\x.dll"), -101)));
        assert_eq!(parse_icon(" "), None);
        assert_eq!(command_file("\"C:\\Program Files\\X\\unins000.exe\" /SILENT"), Some(PathBuf::from("C:\\Program Files\\X\\unins000.exe")));
        assert_eq!(command_file("C:\\Program Files\\Y\\uninstall.exe /S"), Some(PathBuf::from("C:\\Program Files\\Y\\uninstall.exe")));
        assert_eq!(command_file("MsiExec.exe /X{1234}"), Some(PathBuf::from("MsiExec.exe")));
        assert!(is_update("Security Update for X (KB123456)", None, false));
        assert!(is_update("KB5034441", None, false));
        assert!(is_update("Thing", Some("Hotfix"), false));
        assert!(!is_update("KBase Editor", None, false));
    }

    #[cfg(windows)]
    #[test]
    fn reads_this_computer() {
        let inv = read();
        // Any Windows has something installed for everyone.
        assert!(inv.programs.iter().any(|p| p.scope == Scope::Machine), "{} programs", inv.programs.len());
        assert!(inv.programs.iter().all(|p| !p.name.is_empty() && !p.key.is_empty()));
    }
}
