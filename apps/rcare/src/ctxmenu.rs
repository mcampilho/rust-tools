//! Context Menu Manager: what programs add to Explorer's right-click menu
//! (the user's choice, 2026-11-23, from what was left for later).
//!
//! Two kinds of entries, looked for under the classes of the machine
//! (`HKLM\Software\Classes`) and of the user (`HKCU\Software\Classes`), for
//! files (`*`, `AllFilesystemObjects`), folders (`Directory`, `Folder`), the
//! background of a folder and of the desktop, and drives:
//!
//! - **Commands** (`<class>\shell\<verb>`, with a `command`): turned off with
//!   an empty `LegacyDisable` value in the verb's key, which Explorer honours;
//!   turned on by taking that value away. The same command in several classes
//!   ("Open with Code" for files, folders and the background) is one row.
//! - **Shell extensions** (`<class>\shellex\ContextMenuHandlers\<name>`, a
//!   COM class): turned off as ShellExView does, by naming the class under
//!   `HKLM\…\Shell Extensions\Blocked`; turned on by taking that name away
//!   (from the user's `Blocked` too). This needs administrator rights, and
//!   it blocks the class wherever Explorer would load it.
//!
//! Nothing is deleted, so there is no copy to keep: turning an entry on again
//! puts it back as it was. Windows' own entries (their program in the
//! Windows folder) are left out.

use std::path::{Path, PathBuf};

/// The classes looked into, with what the page says about them (a translation key).
pub const CLASSES: [(&str, &str); 7] = [
    ("*", "Files"),
    ("AllFilesystemObjects", "Files"),
    ("Directory", "Folders"),
    ("Folder", "Folders"),
    ("Directory\\Background", "Folder background"),
    ("DesktopBackground", "Desktop"),
    ("Drive", "Drives"),
];

/// Where Explorer looks for blocked shell extensions.
#[cfg_attr(not(windows), allow(dead_code))]
const BLOCKED: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Shell Extensions\\Blocked";

/// How an entry is turned off and on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum How {
    /// A command: its verb keys (of the machine or not, path under the root).
    Verb(Vec<(bool, String)>),
    /// A shell extension: its COM class, `{…}`.
    Handler(String),
}

/// One entry of the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    /// The program or library behind it, when it can be told.
    pub program: Option<PathBuf>,
    /// Where the entry shows (translation keys of `CLASSES`, each once).
    pub places: Vec<&'static str>,
    pub how: How,
    pub enabled: bool,
    /// The program is not there any more.
    pub missing: bool,
    /// The company of the program (its version information), to tell whose it is.
    pub company: Option<String>,
}

impl Item {
    /// Whether changing it needs administrator rights.
    pub fn machine(&self) -> bool {
        match &self.how {
            How::Verb(keys) => keys.iter().any(|(m, _)| *m),
            How::Handler(_) => true,
        }
    }

    /// What the kind column says (a translation key).
    pub fn kind_label(&self) -> &'static str {
        match self.how {
            How::Verb(_) => "Command",
            How::Handler(_) => "Shell extension",
        }
    }
}

/// The label of a menu entry as it shows: without `&` (a doubled one is kept).
pub fn clean_label(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '&' {
            if it.peek() == Some(&'&') {
                out.push('&');
                it.next();
            }
            continue;
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// Whether `file` is Windows' own: in the Windows folder `windir`.
pub fn is_windows_own(file: &Path, windir: &Path) -> bool {
    let f = file.to_string_lossy().to_lowercase().replace('/', "\\");
    let w = windir.to_string_lossy().to_lowercase().replace('/', "\\");
    let w = w.trim_end_matches('\\');
    !w.is_empty() && (f == w || f.starts_with(&format!("{w}\\")))
}

/// Whether a text is a COM class id, `{8-4-4-4-12}`.
pub fn is_clsid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 38
        && b[0] == b'{'
        && b[37] == b'}'
        && s[1..37].char_indices().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { c == '-' } else { c.is_ascii_hexdigit() })
}

/// One entry as found in one class, before the same ones are put together.
#[derive(Clone, Debug)]
pub struct Found {
    pub name: String,
    pub program: Option<PathBuf>,
    pub place: &'static str,
    pub how: How,
    pub enabled: bool,
}

/// Puts the same entries together (a command with the same name and program;
/// a shell extension with the same class), sorted by name. An entry is on
/// only when it is on everywhere.
pub fn group(found: Vec<Found>) -> Vec<Item> {
    let mut out: Vec<Item> = Vec::new();
    for f in found {
        let same = out.iter().position(|it| match (&it.how, &f.how) {
            (How::Handler(a), How::Handler(b)) => a.eq_ignore_ascii_case(b),
            (How::Verb(_), How::Verb(_)) => it.name.eq_ignore_ascii_case(&f.name) && it.program == f.program,
            _ => false,
        });
        match same {
            Some(i) => {
                let it = &mut out[i];
                if !it.places.contains(&f.place) {
                    it.places.push(f.place);
                }
                it.enabled &= f.enabled;
                if let (How::Verb(keys), How::Verb(more)) = (&mut it.how, f.how) {
                    keys.extend(more);
                }
            }
            None => {
                let missing = f.program.as_ref().is_some_and(|p| p.is_absolute() && !p.exists());
                out.push(Item {
                    name: f.name,
                    program: f.program,
                    places: vec![f.place],
                    how: f.how,
                    enabled: f.enabled,
                    missing,
                    company: None,
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.kind_label().cmp(b.kind_label())));
    out
}

/// A text that may be an indirect string (`@shell32.dll,-8506`), read as it shows.
#[cfg(windows)]
fn indirect(s: &str) -> String {
    use windows_sys::Win32::UI::Shell::SHLoadIndirectString;
    if !s.starts_with('@') {
        return s.to_string();
    }
    let w: Vec<u16> = s.encode_utf16().chain([0]).collect();
    let mut out = vec![0u16; 512];
    // SAFETY: a NUL-terminated input and an output buffer of the size given.
    let hr = unsafe { SHLoadIndirectString(w.as_ptr(), out.as_mut_ptr(), out.len() as u32, std::ptr::null_mut()) };
    if hr != 0 {
        return String::new();
    }
    let n = out.iter().position(|c| *c == 0).unwrap_or(out.len());
    String::from_utf16_lossy(&out[..n])
}

/// A COM class: its name and its library (machine's classes first, then the user's).
#[cfg(windows)]
fn class_info(clsid: &str) -> (Option<String>, Option<PathBuf>) {
    use rt_sys::win::reg::{Key, Root, View};
    for root in [Root::LocalMachine, Root::CurrentUser] {
        if let Some(k) = Key::open(root, &format!("Software\\Classes\\CLSID\\{clsid}"), View::Bits64) {
            let name = k.string("").map(|s| indirect(&s)).filter(|s| !s.is_empty());
            let dll = k.sub("InprocServer32", View::Bits64).and_then(|d| d.string("")).map(|s| PathBuf::from(s.trim_matches('"')));
            let dll = dll.or_else(|| {
                k.sub("LocalServer32", View::Bits64).and_then(|d| d.string("")).and_then(|s| rinst::inventory::command_file(&s))
            });
            if name.is_some() || dll.is_some() {
                return (name, dll);
            }
        }
    }
    (None, None)
}

/// What programs add to Explorer's right-click menu, Windows' own left out.
#[cfg(windows)]
pub fn read() -> Vec<Item> {
    use rt_sys::win::reg::{Key, Root, View};
    let windir = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
    let blocked: Vec<String> = [Root::LocalMachine, Root::CurrentUser]
        .iter()
        .filter_map(|r| Key::open(*r, BLOCKED, View::Bits64))
        .flat_map(|k| k.values().into_iter().map(|(n, _)| n.to_lowercase()))
        .collect();
    let mut found = Vec::new();
    for (machine, root) in [(true, Root::LocalMachine), (false, Root::CurrentUser)] {
        for (class, place) in CLASSES.iter() {
            let base = format!("Software\\Classes\\{class}");
            // Commands.
            if let Some(shell) = Key::open(root, &format!("{base}\\shell"), View::Bits64) {
                for verb in shell.subkeys() {
                    let Some(k) = shell.sub(&verb, View::Bits64) else { continue };
                    let program = match k.sub("command", View::Bits64).and_then(|c| c.string("")) {
                        Some(cmd) => rinst::inventory::command_file(&cmd),
                        None => ["ExplorerCommandHandler", "DelegateExecute"]
                            .iter()
                            .find_map(|v| k.string(v))
                            .filter(|c| is_clsid(c))
                            .and_then(|c| class_info(&c).1),
                    };
                    // Without a program it cannot be told whose it is: Windows' own, most likely.
                    let Some(program) = program else { continue };
                    let program = PathBuf::from(rt_sys::win::reg::expand(&program.to_string_lossy()));
                    // A program without a folder (`cmd.exe`, `wsl.exe`) is Windows' own when System32 has it.
                    if is_windows_own(&program, &windir) || (program.is_relative() && windir.join("System32").join(&program).is_file()) {
                        continue;
                    }
                    let label = k.string("MUIVerb").or_else(|| k.string("")).map(|s| clean_label(&indirect(&s)));
                    let name = label.filter(|s| !s.is_empty()).unwrap_or_else(|| verb.clone());
                    let enabled = k.value("LegacyDisable").is_none();
                    found.push(Found {
                        name,
                        program: Some(program),
                        place,
                        how: How::Verb(vec![(machine, format!("{base}\\shell\\{verb}"))]),
                        enabled,
                    });
                }
            }
            // Shell extensions.
            if let Some(handlers) = Key::open(root, &format!("{base}\\shellex\\ContextMenuHandlers"), View::Bits64) {
                for h in handlers.subkeys() {
                    let clsid = if is_clsid(&h) {
                        h.clone()
                    } else {
                        match handlers.sub(&h, View::Bits64).and_then(|k| k.string("")) {
                            Some(c) if is_clsid(&c) => c,
                            _ => continue,
                        }
                    };
                    let (cname, dll) = class_info(&clsid);
                    let Some(dll) = dll else { continue };
                    let dll = PathBuf::from(rt_sys::win::reg::expand(&dll.to_string_lossy()));
                    if is_windows_own(&dll, &windir) {
                        continue;
                    }
                    let name = if is_clsid(&h) {
                        cname.unwrap_or_else(|| dll.file_stem().map_or(h.clone(), |s| s.to_string_lossy().into_owned()))
                    } else {
                        h.trim_start_matches(' ').to_string()
                    };
                    let enabled = !blocked.contains(&clsid.to_lowercase());
                    found.push(Found { name, program: Some(dll), place, how: How::Handler(clsid), enabled });
                }
            }
        }
    }
    let mut items = group(found);
    for it in &mut items {
        it.company = it.program.as_deref().and_then(crate::services::company);
    }
    items
}

#[cfg(not(windows))]
pub fn read() -> Vec<Item> {
    Vec::new()
}

#[cfg(windows)]
fn set_value(machine: bool, key: &str, name: &str, data: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_SZ};
    let root = if machine { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
    let k: Vec<u16> = key.encode_utf16().chain([0]).collect();
    let n: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let d: Vec<u16> = data.encode_utf16().chain([0]).collect();
    // SAFETY: NUL-terminated key, name and data; the data's size in bytes, its NUL included.
    let rc = unsafe { RegSetKeyValueW(root, k.as_ptr(), n.as_ptr(), REG_SZ, d.as_ptr().cast(), (d.len() * 2) as u32) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(rc as i32).to_string())
    }
}

/// Takes a value away; one that is not there is fine.
#[cfg(windows)]
fn delete_value(machine: bool, key: &str, name: &str) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let root = if machine { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
    let k: Vec<u16> = key.encode_utf16().chain([0]).collect();
    let n: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: NUL-terminated key and value names.
    let rc = unsafe { RegDeleteKeyValueW(root, k.as_ptr(), n.as_ptr()) };
    if rc == 0 || rc == 2 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(rc as i32).to_string())
    }
}

/// Turns an entry on or off. Err: why not ("access denied" without administrator rights).
#[cfg(windows)]
pub fn set_enabled(item: &Item, on: bool) -> Result<(), String> {
    match &item.how {
        How::Verb(keys) => {
            for (machine, key) in keys {
                if on {
                    delete_value(*machine, key, "LegacyDisable")?;
                } else {
                    set_value(*machine, key, "LegacyDisable", "")?;
                }
            }
            Ok(())
        }
        How::Handler(clsid) => {
            if on {
                delete_value(true, BLOCKED, clsid)?;
                delete_value(false, BLOCKED, clsid)
            } else {
                set_value(true, BLOCKED, clsid, &item.name)
            }
        }
    }
}

#[cfg(not(windows))]
pub fn set_enabled(_item: &Item, _on: bool) -> Result<(), String> {
    Err("Windows only".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(clean_label("Open with &Code"), "Open with Code");
        assert_eq!(clean_label("Tom && Jerry"), "Tom & Jerry");
        assert_eq!(clean_label("  &7-Zip "), "7-Zip");
    }

    #[test]
    fn windows_own() {
        let w = Path::new("C:\\Windows");
        assert!(is_windows_own(Path::new("C:\\WINDOWS\\System32\\shell32.dll"), w));
        assert!(is_windows_own(Path::new("c:/windows/explorer.exe"), Path::new("C:\\Windows\\")));
        assert!(!is_windows_own(Path::new("C:\\WindowsApps\\x.dll"), w));
        assert!(!is_windows_own(Path::new("C:\\Program Files\\7-Zip\\7-zip.dll"), w));
        assert!(!is_windows_own(Path::new("x.dll"), Path::new("")));
    }

    #[test]
    fn class_ids() {
        assert!(is_clsid("{23170F69-40C1-278A-1000-000100020000}"));
        assert!(!is_clsid("{23170F69-40C1-278A-1000-00010002000}"));
        assert!(!is_clsid("7-Zip"));
        assert!(!is_clsid("{23170F69X40C1-278A-1000-000100020000}"));
    }

    #[test]
    fn the_same_entries_are_one_row() {
        let code = Some(PathBuf::from("C:\\Code\\Code.exe"));
        let verb = |place, key: &str, on| Found {
            name: "Open with Code".into(),
            program: code.clone(),
            place,
            how: How::Verb(vec![(false, key.into())]),
            enabled: on,
        };
        let zip = |place| Found {
            name: "7-Zip".into(),
            program: Some(PathBuf::from("C:\\7-Zip\\7-zip.dll")),
            place,
            how: How::Handler("{23170F69-40C1-278A-1000-000100020000}".into()),
            enabled: true,
        };
        let items = group(vec![
            verb("Files", "a", true),
            zip("Files"),
            verb("Folders", "b", false),
            zip("Folders"),
            zip("Folders"),
            verb("Folder background", "c", true),
        ]);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "7-Zip");
        assert_eq!(items[0].places, ["Files", "Folders"]);
        assert!(items[0].machine() && items[0].enabled);
        let c = &items[1];
        assert_eq!(c.places, ["Files", "Folders", "Folder background"]);
        assert_eq!(c.how, How::Verb(vec![(false, "a".into()), (false, "b".into()), (false, "c".into())]));
        // Off in one place: shown off, and turning it on turns all on.
        assert!(!c.enabled && !c.machine());
        // The program is not on this PC; elsewhere `C:\…` is not an absolute path, so it cannot be told.
        assert_eq!(c.missing, cfg!(windows));
    }

    /// Turns a command of its own off and on (run on request: it writes
    /// `HKCU\Software\Classes\*\shell\rcare-ctx-test` and takes it away).
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn toggles_a_command_of_its_own() {
        let key = "HKCU\\Software\\Classes\\*\\shell\\rcare-ctx-test";
        let reg = |args: &[&str]| std::process::Command::new("reg.exe").args(args).output().map(|o| o.status.success()).unwrap_or(false);
        let notepad = std::env::temp_dir().join("rcare-ctx-test.exe");
        std::fs::write(&notepad, b"").unwrap();
        assert!(reg(&["add", key, "/ve", "/d", "rcare &test", "/f"]));
        assert!(reg(&["add", &format!("{key}\\command"), "/ve", "/d", &format!("\"{}\" \"%1\"", notepad.display()), "/f"]));
        let find = || read().into_iter().find(|i| i.name == "rcare test");
        let it = find().expect("the command is listed");
        assert!(it.enabled && !it.machine() && it.places == ["Files"]);
        set_enabled(&it, false).unwrap();
        assert!(!find().unwrap().enabled, "turned off");
        set_enabled(&it, true).unwrap();
        assert!(find().unwrap().enabled, "turned on again");
        assert!(reg(&["delete", key, "/f"]));
        let _ = std::fs::remove_file(&notepad);
        assert!(find().is_none());
    }

    /// What programs add to the menu on this PC.
    #[test]
    #[ignore]
    fn here() {
        for i in read() {
            println!("{:?} on={} missing={} {:?} {:?}", i.name, i.enabled, i.missing, i.places, i.program);
        }
    }
}
