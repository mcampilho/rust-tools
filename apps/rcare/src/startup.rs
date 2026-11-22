//! What starts with Windows, as Task Manager's Startup tab shows it: the
//! `Run` keys of the user and of the machine (64 and 32 bits) and the two
//! Startup folders. Turning an item off or on is done as Task Manager does
//! it — a value under `Explorer\StartupApproved` — so nothing is deleted and
//! Windows' own tab agrees with rcare. Items of the machine need
//! administrator rights to change.

use std::path::{Path, PathBuf};

/// Where an item starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    /// `HKCU\…\Run`.
    UserRun,
    /// `HKLM\…\Run`, 64-bit view.
    MachineRun,
    /// `HKLM\…\Run` of 32-bit programs (`WOW6432Node`).
    MachineRun32,
    /// The user's Startup folder.
    UserFolder,
    /// The Startup folder of every user.
    CommonFolder,
}

/// `HKCU` or `HKLM` relative path of the keys (read on Windows only).
#[cfg_attr(not(windows), allow(dead_code))]
const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
#[cfg_attr(not(windows), allow(dead_code))]
const APPROVED: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved";

impl Place {
    pub const ALL: [Place; 5] = [Place::UserRun, Place::MachineRun, Place::MachineRun32, Place::UserFolder, Place::CommonFolder];

    /// Whether the item belongs to the machine (changing it needs administrator rights).
    pub fn machine(self) -> bool {
        matches!(self, Place::MachineRun | Place::MachineRun32 | Place::CommonFolder)
    }

    /// The `StartupApproved` subkey Windows keeps its on/off state in.
    pub fn approved_subkey(self) -> &'static str {
        match self {
            Place::UserRun | Place::MachineRun => "Run",
            Place::MachineRun32 => "Run32",
            Place::UserFolder | Place::CommonFolder => "StartupFolder",
        }
    }

    /// What the list says about where it is (a translation key).
    pub fn label(self) -> &'static str {
        match self {
            Place::UserRun => "Registry, this user",
            Place::MachineRun | Place::MachineRun32 => "Registry, every user",
            Place::UserFolder => "Startup folder, this user",
            Place::CommonFolder => "Startup folder, every user",
        }
    }
}

/// One item that starts with Windows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The value's name, or the file's name in a Startup folder.
    pub name: String,
    /// What it runs: the command line, or the shortcut's target.
    pub command: String,
    /// The program it runs, when that can be told.
    pub file: Option<PathBuf>,
    pub place: Place,
    pub enabled: bool,
    /// The program is not there any more (it starts nothing).
    pub missing: bool,
}

impl Entry {
    /// The name shown: without `.lnk`.
    pub fn shown_name(&self) -> &str {
        self.name.strip_suffix(".lnk").unwrap_or(&self.name)
    }
}

/// Whether the bytes Windows keeps under `StartupApproved` mean "on": the
/// first byte is even (02, 06) when on, odd (03, 07) when turned off; no value is on.
pub fn approved_on(bytes: Option<&[u8]>) -> bool {
    bytes.and_then(|b| b.first()).is_none_or(|b| b & 1 == 0)
}

/// The 12 bytes Task Manager writes: 02 (on) or 03 (off), three zeros, and,
/// when off, the time it was turned off (a FILETIME, little-endian).
pub fn approved_bytes(on: bool, filetime: u64) -> [u8; 12] {
    let mut b = [0u8; 12];
    b[0] = if on { 2 } else { 3 };
    if !on {
        b[4..].copy_from_slice(&filetime.to_le_bytes());
    }
    b
}

/// Now, as a FILETIME (100 ns since 1601).
pub fn filetime_now() -> u64 {
    let since_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    (since_unix.as_secs() + 11_644_473_600) * 10_000_000 + u64::from(since_unix.subsec_nanos() / 100)
}

/// The program of a command line, and whether it is gone. A program
/// without a folder (found on the `PATH`) is never said to be gone.
#[cfg_attr(not(windows), allow(dead_code))]
fn program_of(command: &str) -> (Option<PathBuf>, bool) {
    let file = rinst::inventory::command_file(command);
    let missing = file.as_ref().is_some_and(|f| f.is_absolute() && !f.exists());
    (file, missing)
}

/// An item of a Startup folder: a shortcut (its target) or a file itself.
pub fn folder_entry(path: &Path, place: Place, approved: Option<&[u8]>) -> Option<Entry> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    if name.eq_ignore_ascii_case("desktop.ini") || path.is_dir() {
        return None;
    }
    let is_lnk = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
    let (command, file) = if is_lnk {
        let target = std::fs::read(path).ok().and_then(|b| rt_sys::lnk::target_of(&b, path.parent()?));
        (target.as_ref().map_or_else(String::new, |t| t.display().to_string()), target)
    } else {
        (path.display().to_string(), Some(path.to_path_buf()))
    };
    let missing = file.as_ref().is_some_and(|f| f.is_absolute() && !f.exists());
    Some(Entry { name, command, file, place, enabled: approved_on(approved), missing })
}

/// The Startup folders: the user's and every user's.
pub fn folder_of(place: Place) -> Option<PathBuf> {
    match place {
        Place::UserFolder => {
            std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup"))
        }
        Place::CommonFolder => {
            std::env::var_os("ProgramData").map(|a| PathBuf::from(a).join("Microsoft\\Windows\\Start Menu\\Programs\\StartUp"))
        }
        _ => None,
    }
}

/// Everything that starts with Windows, by place and name.
#[cfg(windows)]
pub fn read() -> Vec<Entry> {
    use rt_sys::win::reg::{Key, Root, Value, View};
    let approved = |place: Place, name: &str| -> Option<Vec<u8>> {
        let root = if place.machine() { Root::LocalMachine } else { Root::CurrentUser };
        let key = Key::open(root, &format!("{APPROVED}\\{}", place.approved_subkey()), View::Bits64)?;
        match key.value(name)? {
            Value::Binary(b) => Some(b),
            _ => None,
        }
    };
    let mut out = Vec::new();
    for (place, root, view) in [
        (Place::UserRun, Root::CurrentUser, View::Bits64),
        (Place::MachineRun, Root::LocalMachine, View::Bits64),
        (Place::MachineRun32, Root::LocalMachine, View::Bits32),
    ] {
        let Some(key) = Key::open(root, RUN, view) else { continue };
        for (name, value) in key.values() {
            let command = match value {
                Value::Str(s) => s,
                Value::Expand(s) => rt_sys::win::reg::expand(&s),
                _ => continue,
            };
            if name.is_empty() || command.trim().is_empty() {
                continue;
            }
            let (file, missing) = program_of(&command);
            let enabled = approved_on(approved(place, &name).as_deref());
            out.push(Entry { name, command, file, place, enabled, missing });
        }
    }
    for place in [Place::UserFolder, Place::CommonFolder] {
        let Some(dir) = folder_of(place) else { continue };
        let Ok(items) = std::fs::read_dir(&dir) else { continue };
        for it in items.flatten() {
            let name = it.file_name().to_string_lossy().into_owned();
            if let Some(e) = folder_entry(&it.path(), place, approved(place, &name).as_deref()) {
                out.push(e);
            }
        }
    }
    out.sort_by(|a, b| a.shown_name().to_lowercase().cmp(&b.shown_name().to_lowercase()).then(a.name.cmp(&b.name)));
    out
}

#[cfg(not(windows))]
pub fn read() -> Vec<Entry> {
    Vec::new()
}

/// Turns an item on or off as Task Manager does. Err: why not (an item of
/// the machine without administrator rights says "access denied").
#[cfg(windows)]
pub fn set_enabled(e: &Entry, on: bool) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_BINARY};
    let root = if e.place.machine() { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
    let key: Vec<u16> = format!("{APPROVED}\\{}", e.place.approved_subkey()).encode_utf16().chain([0]).collect();
    let name: Vec<u16> = e.name.encode_utf16().chain([0]).collect();
    let bytes = approved_bytes(on, filetime_now());
    // SAFETY: NUL-terminated key and value names; the data is the 12 bytes given with their length.
    let rc = unsafe { RegSetKeyValueW(root, key.as_ptr(), name.as_ptr(), REG_BINARY, bytes.as_ptr().cast(), bytes.len() as u32) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(rc as i32).to_string())
    }
}

#[cfg(not(windows))]
pub fn set_enabled(_e: &Entry, _on: bool) -> Result<(), String> {
    Err("Windows only".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_managers_bytes() {
        assert!(approved_on(None));
        assert!(approved_on(Some(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])));
        assert!(approved_on(Some(&[6, 0, 0, 0])));
        assert!(!approved_on(Some(&[3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8])));
        assert!(!approved_on(Some(&[7])));
        assert!(approved_on(Some(&[])));
        let off = approved_bytes(false, 0x0102_0304_0506_0708);
        assert_eq!(off, [3, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1]);
        assert!(!approved_on(Some(&off)));
        assert_eq!(approved_bytes(true, 99), [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        // Around now, not around 1601 or the year 30000.
        let years = filetime_now() / 10_000_000 / 3600 / 24 / 365;
        assert!((420..500).contains(&years), "{years}");
    }

    #[test]
    fn folder_items() {
        let dir = std::env::temp_dir().join(format!("rcare-startup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bat = dir.join("hello.bat");
        std::fs::write(&bat, "@echo hi").unwrap();
        std::fs::write(dir.join("desktop.ini"), "").unwrap();
        let e = folder_entry(&bat, Place::UserFolder, None).unwrap();
        assert!(e.enabled && !e.missing && e.file.as_deref() == Some(bat.as_path()));
        let off = folder_entry(&bat, Place::UserFolder, Some(&approved_bytes(false, 1))).unwrap();
        assert!(!off.enabled);
        assert!(folder_entry(&dir.join("desktop.ini"), Place::UserFolder, None).is_none());
        // A shortcut that cannot be read starts nothing that can be told.
        let lnk = dir.join("Broken.lnk");
        std::fs::write(&lnk, b"not a shortcut").unwrap();
        let b = folder_entry(&lnk, Place::CommonFolder, None).unwrap();
        assert_eq!((b.shown_name(), b.file.is_none(), b.place.machine()), ("Broken", true, true));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn places() {
        for p in Place::ALL {
            assert!(!p.label().is_empty() && !p.approved_subkey().is_empty());
        }
        assert!(!Place::UserRun.machine() && Place::MachineRun32.machine());
        assert_eq!(Place::MachineRun32.approved_subkey(), "Run32");
    }

    /// Turns an item of its own off and on as Task Manager does, and takes it
    /// away again (run on request: it writes the user's `Run` and
    /// `StartupApproved\Run` keys, under the name `rcare-startup-test`).
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn toggles_an_item_of_its_own() {
        let name = "rcare-startup-test";
        let reg = |args: &[&str]| std::process::Command::new("reg.exe").args(args).output().map(|o| o.status.success()).unwrap_or(false);
        let run = format!("HKCU\\{RUN}");
        let approved = format!("HKCU\\{APPROVED}\\Run");
        assert!(reg(&["add", &run, "/v", name, "/t", "REG_SZ", "/d", "C:\\Windows\\System32\\notepad.exe", "/f"]));
        let find = || read().into_iter().find(|e| e.name == name && e.place == Place::UserRun);
        let e = find().expect("the item is listed");
        assert!(e.enabled && !e.missing);
        set_enabled(&e, false).unwrap();
        assert!(!find().unwrap().enabled, "turned off");
        set_enabled(&e, true).unwrap();
        assert!(find().unwrap().enabled, "turned on again");
        assert!(reg(&["delete", &run, "/v", name, "/f"]));
        assert!(reg(&["delete", &approved, "/v", name, "/f"]));
        assert!(find().is_none());
    }

    /// What starts with Windows on this PC.
    #[test]
    #[ignore]
    fn here() {
        for e in read() {
            println!("{:?} {:?} on={} missing={} {}", e.place, e.name, e.enabled, e.missing, e.command);
        }
    }
}
