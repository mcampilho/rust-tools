//! Invalid registry entries (phase 4, Windows): entries that name a file or
//! folder that is gone. Each kind is looked for where Windows keeps it:
//!
//! - `Uninstall` entries whose uninstaller and folder are gone (not Windows
//!   Installer's: those are its own);
//! - `App Paths` whose program is gone;
//! - startup entries (`Run`) whose program is gone;
//! - `MuiCache`: names Explorer kept of programs gone;
//! - "Open with" programs (`Classes\Applications`) gone;
//! - `SharedDLLs` counts of files gone;
//! - Windows Installer's folder list with folders gone;
//! - fonts whose file is gone.
//!
//! Only absolute paths are checked, never on the network or on a drive not
//! there now. Everything is saved to a `.reg` file before it is deleted
//! (`rt_sys::regfile`); double-click it to put it back.

use crate::clean::{Cleaned, Found};
use crate::targets::{Group, Kind, Target};
use std::path::Path;

/// Marks a translation key (the test of the translations looks for it).
const fn k(s: &'static str) -> &'static str {
    s
}

/// What is looked for (a target's `id` says which).
pub fn targets() -> Vec<Target> {
    let t = |id: &str, name: &'static str, about: &'static str, admin: bool| Target {
        kind: Kind::Registry,
        admin,
        ..Target::files(id, Group::Registry, name, about, Vec::new())
    };
    vec![
        t(
            "reg.uninstall",
            k("Uninstall entries"),
            k("Your entries in Apps and features of programs whose uninstaller and folder are gone."),
            false,
        ),
        t("reg.uninstall-all", k("Uninstall entries of the system"), k("The same, among the programs installed for every user."), true),
        t("reg.run", k("Startup entries"), k("Programs started with Windows for you that are gone."), false),
        t("reg.run-all", k("Startup entries of the system"), k("Programs started with Windows for every user that are gone."), true),
        t("reg.apppaths", k("Program paths"), k("App Paths: where Windows finds a program by its name, for programs gone."), true),
        t("reg.muicache", k("Names of programs"), k("The names Explorer kept of programs that are gone (MuiCache)."), false),
        t("reg.openwith", k("Open with"), k("Programs offered in Open with that are gone."), false),
        t("reg.shareddlls", k("Shared files"), k("Counts of shared files that are gone (SharedDLLs)."), true),
        t("reg.folders", k("Installer folders"), k("Folders Windows Installer keeps a list of that are gone."), true),
        t("reg.fonts", k("Fonts"), k("Fonts listed whose file is gone."), true),
    ]
}

/// The program a command runs: `"C:\x\a.exe" /S` → `C:\x\a.exe`; unquoted,
/// up to `.exe` (paths with spaces are common).
pub fn command_file(cmd: &str) -> Option<String> {
    let c = cmd.trim();
    if let Some(rest) = c.strip_prefix('"') {
        return rest.split('"').next().filter(|p| !p.is_empty()).map(str::to_string);
    }
    let lower = c.to_lowercase();
    match lower.find(".exe") {
        Some(i) => Some(c[..i + 4].to_string()),
        None => c.split_whitespace().next().map(str::to_string),
    }
}

/// A MuiCache name's program: `C:\x\a.exe.FriendlyAppName` → `C:\x\a.exe`.
pub fn mui_program(name: &str) -> Option<&str> {
    if name.starts_with('@') {
        return None;
    }
    name.strip_suffix(".FriendlyAppName").or_else(|| name.strip_suffix(".ApplicationCompany"))
}

/// Whether `path` is gone, as far as can be told: an absolute local path on a
/// drive that is there. `wow32`: from the 32-bit view, where `System32` is
/// `SysWOW64`.
pub fn gone(path: &str, wow32: bool) -> bool {
    let p = Path::new(path.trim().trim_matches('"'));
    if !crate::shortcuts::checkable(p) || p.exists() {
        return false;
    }
    if wow32 {
        let lower = path.to_lowercase();
        if let Some(i) = lower.find("\\system32\\") {
            let other = format!("{}\\SysWOW64\\{}", &path[..i], &path[i + 10..]);
            return !Path::new(&other).exists();
        }
    }
    true
}

/// Saves the entries found to `backup` and deletes them.
pub fn clean(found: &Found, backup: &Path) -> Cleaned {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut c = Cleaned::default();
    #[cfg(windows)]
    {
        let entries: Vec<rt_sys::regfile::Entry> = found.entries.iter().map(|(e, _)| e.clone()).collect();
        for r in rt_sys::regfile::save_then_delete(&entries, backup) {
            match r {
                Ok(()) => c.files += 1,
                Err(e) => {
                    c.failed += 1;
                    c.error.get_or_insert(e);
                }
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (found, backup);
    c
}

/// Looks for the entries of `t`.
pub fn analyse(t: &Target) -> Found {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut f = Found::default();
    #[cfg(windows)]
    {
        f.entries = win::find(&t.id);
        f.items = f.entries.len() as u64;
    }
    #[cfg(not(windows))]
    let _ = t;
    f
}

#[cfg(windows)]
mod win {
    use super::*;
    use rt_sys::regfile::Entry;
    use rt_sys::win::reg::{expand, Key, Root, Value, View};
    use std::path::PathBuf;

    const UNINSTALL: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
    const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const RUN_ONCE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce";
    const APP_PATHS: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\App Paths";

    fn root_name(r: Root) -> &'static str {
        match r {
            Root::LocalMachine => "HKEY_LOCAL_MACHINE",
            Root::CurrentUser => "HKEY_CURRENT_USER",
            Root::Users => "HKEY_USERS",
        }
    }

    /// The key as written, in the 64-bit view (`WOW6432Node` for the 32-bit one).
    fn key_path(r: Root, path: &str, view: View) -> String {
        let path = match (view, path.split_once('\\')) {
            (View::Bits32, Some((first, rest))) if first.eq_ignore_ascii_case("Software") && r == Root::LocalMachine => {
                format!("{first}\\WOW6432Node\\{rest}")
            }
            _ => path.to_string(),
        };
        format!("{}\\{path}", root_name(r))
    }

    fn text(v: &Value) -> Option<String> {
        match v {
            Value::Str(s) => Some(s.clone()),
            Value::Expand(s) => Some(expand(s)),
            _ => None,
        }
    }

    /// The views of a root: both for the system's, one for the user's.
    fn views(r: Root) -> &'static [View] {
        if r == Root::LocalMachine {
            &[View::Bits64, View::Bits32]
        } else {
            &[View::Bits64]
        }
    }

    pub fn find(id: &str) -> Vec<(Entry, String)> {
        let mut out = Vec::new();
        let key_entry = |r: Root, path: String, view: View, why: String| (Entry { key: key_path(r, &path, view), value: None }, why);
        match id {
            "reg.uninstall" | "reg.uninstall-all" => {
                let r = if id == "reg.uninstall" { Root::CurrentUser } else { Root::LocalMachine };
                for &view in views(r) {
                    let Some(k) = Key::open(r, UNINSTALL, view) else { continue };
                    for sub in k.subkeys() {
                        let Some(e) = k.sub(&sub, view) else { continue };
                        if e.number("WindowsInstaller") == Some(1) {
                            continue;
                        }
                        let Some(file) = e.string("UninstallString").and_then(|u| command_file(&u)) else { continue };
                        let loc = e.string("InstallLocation");
                        let wow = view == View::Bits32;
                        if gone(&file, wow) && loc.as_deref().is_none_or(|l| gone(l, wow)) {
                            let name = e.string("DisplayName").unwrap_or_else(|| sub.clone());
                            out.push(key_entry(r, format!("{UNINSTALL}\\{sub}"), view, format!("{name} → {file}")));
                        }
                    }
                }
            }
            "reg.run" | "reg.run-all" => {
                let r = if id == "reg.run" { Root::CurrentUser } else { Root::LocalMachine };
                for &view in views(r) {
                    for run in [RUN, RUN_ONCE] {
                        let Some(k) = Key::open(r, run, view) else { continue };
                        for (name, v) in k.values() {
                            let Some(file) = text(&v).and_then(|c| command_file(&c)) else { continue };
                            if gone(&file, view == View::Bits32) {
                                out.push((Entry { key: key_path(r, run, view), value: Some(name.clone()) }, format!("{name} → {file}")));
                            }
                        }
                    }
                }
            }
            "reg.apppaths" => {
                for r in [Root::CurrentUser, Root::LocalMachine] {
                    for &view in views(r) {
                        let Some(k) = Key::open(r, APP_PATHS, view) else { continue };
                        for sub in k.subkeys() {
                            let Some(file) = k.sub(&sub, view).and_then(|e| e.string("")) else { continue };
                            if gone(&file, view == View::Bits32) {
                                out.push(key_entry(r, format!("{APP_PATHS}\\{sub}"), view, format!("{sub} → {file}")));
                            }
                        }
                    }
                }
            }
            "reg.muicache" => {
                let path = "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\MuiCache";
                if let Some(k) = Key::open(Root::CurrentUser, path, View::Bits64) {
                    for (name, _) in k.values() {
                        if let Some(file) = mui_program(&name).filter(|f| gone(f, false)) {
                            out.push((
                                Entry { key: key_path(Root::CurrentUser, path, View::Bits64), value: Some(name.clone()) },
                                file.to_string(),
                            ));
                        }
                    }
                }
            }
            "reg.openwith" => {
                let path = "Software\\Classes\\Applications";
                if let Some(k) = Key::open(Root::CurrentUser, path, View::Bits64) {
                    for app in k.subkeys() {
                        let Some(shell) = k.sub(&format!("{app}\\shell"), View::Bits64) else { continue };
                        // Every verb's program: gone when all of them are.
                        let files: Vec<String> = shell
                            .subkeys()
                            .iter()
                            .filter_map(|verb| {
                                shell
                                    .sub(&format!("{verb}\\command"), View::Bits64)
                                    .and_then(|c| c.string(""))
                                    .and_then(|c| command_file(&c))
                            })
                            .collect();
                        if !files.is_empty() && files.iter().all(|f| gone(f, false)) {
                            out.push(key_entry(Root::CurrentUser, format!("{path}\\{app}"), View::Bits64, format!("{app} → {}", files[0])));
                        }
                    }
                }
            }
            "reg.shareddlls" => {
                let path = "Software\\Microsoft\\Windows\\CurrentVersion\\SharedDLLs";
                for &view in views(Root::LocalMachine) {
                    let Some(k) = Key::open(Root::LocalMachine, path, view) else { continue };
                    for (name, _) in k.values() {
                        if gone(&name, view == View::Bits32) {
                            out.push((Entry { key: key_path(Root::LocalMachine, path, view), value: Some(name.clone()) }, name));
                        }
                    }
                }
            }
            "reg.folders" => {
                let path = "Software\\Microsoft\\Windows\\CurrentVersion\\Installer\\Folders";
                if let Some(k) = Key::open(Root::LocalMachine, path, View::Bits64) {
                    for (name, _) in k.values() {
                        // `Config.Msi` is Windows Installer's own, made at each install.
                        if gone(&name, false) && !name.to_lowercase().contains("\\config.msi") {
                            out.push((Entry { key: key_path(Root::LocalMachine, path, View::Bits64), value: Some(name.clone()) }, name));
                        }
                    }
                }
            }
            "reg.fonts" => {
                let path = "Software\\Microsoft\\Windows NT\\CurrentVersion\\Fonts";
                let fonts = PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into())).join("Fonts");
                for r in [Root::LocalMachine, Root::CurrentUser] {
                    let Some(k) = Key::open(r, path, View::Bits64) else { continue };
                    for (name, v) in k.values() {
                        let Some(file) = text(&v).filter(|f| !f.trim().is_empty()) else { continue };
                        let full =
                            if Path::new(&file).is_absolute() { file.clone() } else { fonts.join(&file).to_string_lossy().into_owned() };
                        if gone(&full, false) {
                            out.push((
                                Entry { key: key_path(r, path, View::Bits64), value: Some(name.clone()) },
                                format!("{name} → {full}"),
                            ));
                        }
                    }
                }
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_and_names() {
        assert_eq!(command_file("\"C:\\Program Files\\A\\unins000.exe\" /SILENT").as_deref(), Some("C:\\Program Files\\A\\unins000.exe"));
        assert_eq!(command_file("C:\\Program Files\\B\\b.exe -x").as_deref(), Some("C:\\Program Files\\B\\b.exe"));
        assert_eq!(command_file("rundll32.exe shell32.dll,Control_RunDLL").as_deref(), Some("rundll32.exe"));
        assert_eq!(mui_program("C:\\Tools\\x.exe.FriendlyAppName"), Some("C:\\Tools\\x.exe"));
        assert_eq!(mui_program("@C:\\Windows\\system32\\shell32.dll,-22019"), None);
        assert_eq!(mui_program("LangID"), None);
        // Relative names and the network are never called gone.
        assert!(!gone("rundll32.exe", false));
        assert!(!gone("\\\\server\\share\\a.exe", false));
        let here = std::env::current_exe().unwrap().to_string_lossy().into_owned();
        assert!(!gone(&here, false));
        assert!(gone(&format!("{here}.gone"), false));
    }

    /// A startup entry of its own to a program gone: found, saved to a
    /// `.reg` file and deleted (writes to HKCU\...\Run):
    /// `cargo test -p rclean --lib cleans_a_startup_entry -- --ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn cleans_a_startup_entry() {
        use rt_sys::regfile::run_hidden;
        let name = format!("rclean-test-{}", std::process::id());
        let key = "HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
        run_hidden("reg.exe", &["add", key, "/v", &name, "/d", "\"C:\\No Such Folder\\gone.exe\" -x", "/f"]).unwrap();
        let t = targets().into_iter().find(|t| t.id == "reg.run").unwrap();
        let f = analyse(&t);
        let mine: Vec<_> = f.entries.iter().filter(|(e, _)| e.value.as_deref() == Some(&name)).cloned().collect();
        assert_eq!(mine.len(), 1, "{:?}", f.entries);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("regtest-{}", std::process::id()));
        let backup = dir.join("reg.run.reg");
        let c = clean(&Found { entries: mine, ..Found::default() }, &backup);
        assert_eq!((c.files, c.failed), (1, 0), "{:?}", c.error);
        let saved = std::fs::read(&backup).unwrap();
        let text = String::from_utf16_lossy(&saved[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>());
        assert!(text.contains(&name) && text.contains("gone.exe"), "{text}");
        assert!(!analyse(&t).entries.iter().any(|(e, _)| e.value.as_deref() == Some(&name)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What this computer has: `cargo test -p rclean --lib registry_here -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn registry_here() {
        for t in targets() {
            let f = analyse(&t);
            println!("{}: {}", t.id, f.entries.len());
            for (e, why) in f.entries.iter().take(8) {
                println!("    {} {} | {why}", e.key, e.value.as_deref().map(|v| format!("› {v}")).unwrap_or_default());
            }
        }
    }
}
