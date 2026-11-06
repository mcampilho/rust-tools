//! Explorer integration for the current user only (HKCU: no administrator
//! rights, nothing machine-wide). Nothing here runs unless the user asks
//! (command line or File › Windows Integration), and `unregister` removes
//! everything rview wrote.
//!
//! - `register`: "Open with rview" on the right-click menu of supported
//!   files, and rview in their "Open with" list.
//! - `associate`: also registers rview in Windows' Default apps and makes
//!   it the default viewer for the types where no default was chosen by
//!   the user. Windows protects the user's own choices (`UserChoice`);
//!   rview never touches them: those types are left for the user to pick
//!   in Settings.

use super::wide;
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::Registry::*;

/// Supported extensions and the ProgID (document kind) each opens as.
pub const EXTENSIONS: &[(&str, &str)] = &[
    (".pdf", "rview.pdf"),
    (".md", "rview.md"),
    (".markdown", "rview.md"),
    (".html", "rview.html"),
    (".htm", "rview.html"),
    (".txt", "rview.txt"),
    (".log", "rview.txt"),
    (".csv", "rview.csv"),
    (".tsv", "rview.csv"),
];

const PROGIDS: &[(&str, &str)] = &[
    ("rview.pdf", "PDF Document"),
    ("rview.md", "Markdown Document"),
    ("rview.html", "HTML Document"),
    ("rview.txt", "Text Document"),
    ("rview.csv", "CSV Document"),
];

/// Registry locations, relative to HKEY_CURRENT_USER. Tests point them
/// at a scratch key.
pub struct Keys {
    pub classes: String,
    pub app: String,
    pub registered: String,
    pub file_exts: String,
}

impl Keys {
    pub fn user() -> Keys {
        Keys {
            classes: "Software\\Classes".into(),
            app: "Software\\rview".into(),
            registered: "Software\\RegisteredApplications".into(),
            file_exts: "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Off,
    /// Registered for this rview.exe.
    Here,
    /// Registered for an rview.exe in another folder (moved or a copy).
    Elsewhere,
}

/// What `associate` could do per extension.
#[derive(Debug, Default)]
pub struct Defaults {
    /// rview became the default.
    pub set: Vec<&'static str>,
    /// rview already was the default.
    pub already: Vec<&'static str>,
    /// The user chose another app; only they can change it (in Settings).
    pub user_choice: Vec<&'static str>,
}

pub type Error = u32;

fn command(exe: &str) -> String {
    format!("\"{exe}\" \"%1\"")
}

fn icon(exe: &str) -> String {
    format!("\"{exe}\",0")
}

fn set_str(key: &str, name: Option<&str>, value: &str) -> Result<(), Error> {
    let k = wide(key);
    let n = name.map(wide);
    let v = wide(value);
    // SAFETY: NUL-terminated strings; the data length includes the NUL.
    // RegSetKeyValueW creates the key when it does not exist.
    let rc = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ref().map_or(null(), |n| n.as_ptr()), REG_SZ, v.as_ptr().cast(), (v.len() * 2) as u32) };
    if rc != 0 {
        return Err(rc);
    }
    Ok(())
}

fn get_str(key: &str, name: Option<&str>) -> Option<String> {
    let k = wide(key);
    let n = name.map(wide);
    let np = n.as_ref().map_or(null(), |n| n.as_ptr());
    let mut size = 0u32;
    // SAFETY: the first call asks for the size, the second fills a buffer
    // of exactly that size.
    unsafe {
        if RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), np, RRF_RT_REG_SZ, null_mut(), null_mut(), &mut size) != 0 {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
        let mut size = (buf.len() * 2) as u32;
        if RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), np, RRF_RT_REG_SZ, null_mut(), buf.as_mut_ptr().cast(), &mut size) != 0 {
            return None;
        }
        Some(super::from_wide(&buf))
    }
}

const NOT_FOUND: u32 = 2;

fn ok_if_missing(rc: u32) -> Result<(), Error> {
    if rc == 0 || rc == NOT_FOUND {
        Ok(())
    } else {
        Err(rc)
    }
}

fn delete_tree(key: &str) -> Result<(), Error> {
    let k = wide(key);
    // SAFETY: NUL-terminated subkey of HKCU.
    ok_if_missing(unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, k.as_ptr()) })
}

/// Deletes one value (`""` is the key's default value).
fn delete_value(key: &str, name: &str) -> Result<(), Error> {
    let (k, n) = (wide(key), wide(name));
    // SAFETY: NUL-terminated strings.
    ok_if_missing(unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr()) })
}

/// Deletes `key` only when it has no values and no subkeys left.
fn delete_if_empty(key: &str) -> Result<(), Error> {
    let k = wide(key);
    let mut h: HKEY = null_mut();
    // SAFETY: the key is opened, inspected and closed; only an empty key is
    // deleted.
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, k.as_ptr(), 0, KEY_READ, &mut h) != 0 {
            return Ok(());
        }
        let (mut subkeys, mut values) = (0u32, 0u32);
        let rc = RegQueryInfoKeyW(h, null_mut(), null_mut(), null(), &mut subkeys, null_mut(), null_mut(), &mut values, null_mut(), null_mut(), null_mut(), null_mut());
        RegCloseKey(h);
        if rc != 0 || subkeys > 0 || values > 0 {
            return Ok(());
        }
        ok_if_missing(RegDeleteKeyW(HKEY_CURRENT_USER, k.as_ptr()))
    }
}

/// Tells Explorer that associations changed (refreshes menus and icons).
fn notify() {
    use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    // SAFETY: documented call with no items.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as _, SHCNF_IDLIST, null(), null()) };
}

/// Whether the context menu entry exists, and for which rview.exe.
pub fn status(keys: &Keys, exe: &str) -> Status {
    let key = format!("{}\\SystemFileAssociations\\.pdf\\shell\\rview\\command", keys.classes);
    match get_str(&key, None) {
        None => Status::Off,
        Some(c) if c.eq_ignore_ascii_case(&command(exe)) => Status::Here,
        Some(_) => Status::Elsewhere,
    }
}

/// Adds "Open with rview" (`verb` is its translated label) to the context
/// menu of the supported types and rview to their "Open with" list.
pub fn register(keys: &Keys, exe: &str, verb: &str) -> Result<(), Error> {
    let (cmd, icon) = (command(exe), icon(exe));
    for (progid, name) in PROGIDS {
        let k = format!("{}\\{progid}", keys.classes);
        set_str(&k, None, name)?;
        set_str(&format!("{k}\\DefaultIcon"), None, &icon)?;
        set_str(&format!("{k}\\shell\\open\\command"), None, &cmd)?;
    }
    let app = format!("{}\\Applications\\rview.exe", keys.classes);
    set_str(&app, Some("FriendlyAppName"), "rview")?;
    set_str(&format!("{app}\\DefaultIcon"), None, &icon)?;
    set_str(&format!("{app}\\shell\\open\\command"), None, &cmd)?;
    for (ext, progid) in EXTENSIONS {
        set_str(&format!("{app}\\SupportedTypes"), Some(ext), "")?;
        set_str(&format!("{}\\{ext}\\OpenWithProgids", keys.classes), Some(progid), "")?;
        let v = format!("{}\\SystemFileAssociations\\{ext}\\shell\\rview", keys.classes);
        set_str(&v, None, verb)?;
        set_str(&v, Some("MUIVerb"), verb)?;
        set_str(&v, Some("Icon"), &icon)?;
        set_str(&format!("{v}\\command"), None, &cmd)?;
    }
    notify();
    Ok(())
}

/// `register`, plus Default apps registration and the default viewer for
/// every type without a user choice. The previous default of such a type
/// (if any) is kept and restored by `unregister`.
pub fn associate(keys: &Keys, exe: &str, verb: &str) -> Result<Defaults, Error> {
    register(keys, exe, verb)?;
    let cap = format!("{}\\Capabilities", keys.app);
    set_str(&cap, Some("ApplicationName"), "rview")?;
    set_str(&cap, Some("ApplicationDescription"), "A small, fast, portable document viewer.")?;
    set_str(&cap, Some("ApplicationIcon"), &icon(exe))?;
    for (ext, progid) in EXTENSIONS {
        set_str(&format!("{cap}\\FileAssociations"), Some(ext), progid)?;
    }
    set_str(&keys.registered, Some("rview"), &cap)?;
    let mut d = Defaults::default();
    for (ext, progid) in EXTENSIONS {
        let ours = |s: &str| s.eq_ignore_ascii_case(progid) || s.eq_ignore_ascii_case("Applications\\rview.exe");
        match get_str(&format!("{}\\{ext}\\UserChoice", keys.file_exts), Some("ProgId")) {
            Some(c) if ours(&c) => d.already.push(ext),
            Some(_) => d.user_choice.push(ext),
            None => {
                let key = format!("{}\\{ext}", keys.classes);
                match get_str(&key, None) {
                    Some(c) if ours(&c) => d.already.push(ext),
                    prev => {
                        if let Some(p) = prev.filter(|p| !p.is_empty()) {
                            set_str(&format!("{}\\Backup", keys.app), Some(ext), &p)?;
                        }
                        set_str(&key, None, progid)?;
                        d.set.push(ext);
                    }
                }
            }
        }
    }
    notify();
    Ok(d)
}

/// Removes everything `register` and `associate` wrote, restoring the
/// previous defaults. Keys of other programs are left alone.
pub fn unregister(keys: &Keys) -> Result<(), Error> {
    for (progid, _) in PROGIDS {
        delete_tree(&format!("{}\\{progid}", keys.classes))?;
    }
    delete_tree(&format!("{}\\Applications\\rview.exe", keys.classes))?;
    let backup = format!("{}\\Backup", keys.app);
    for (ext, progid) in EXTENSIONS {
        let sfa = format!("{}\\SystemFileAssociations\\{ext}", keys.classes);
        delete_tree(&format!("{sfa}\\shell\\rview"))?;
        delete_if_empty(&format!("{sfa}\\shell"))?;
        delete_if_empty(&sfa)?;
        let key = format!("{}\\{ext}", keys.classes);
        delete_value(&format!("{key}\\OpenWithProgids"), progid)?;
        delete_if_empty(&format!("{key}\\OpenWithProgids"))?;
        if get_str(&key, None).is_some_and(|c| c.eq_ignore_ascii_case(progid)) {
            match get_str(&backup, Some(ext)) {
                Some(prev) => set_str(&key, None, &prev)?,
                None => delete_value(&key, "")?,
            }
        }
        delete_if_empty(&key)?;
    }
    delete_value(&keys.registered, "rview")?;
    delete_tree(&keys.app)?;
    notify();
    Ok(())
}

/// Opens Windows Settings › Default apps.
pub fn open_default_apps() {
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let (op, target) = (wide("open"), wide("ms-settings:defaultapps"));
    // SAFETY: valid NUL-terminated strings; a fixed, known target.
    unsafe { windows_sys::Win32::UI::Shell::ShellExecuteW(null_mut(), op.as_ptr(), target.as_ptr(), null(), null(), SW_SHOWNORMAL) };
}

/// The extensions as a readable list (".pdf, .md, ...").
pub fn list(exts: &[&str]) -> String {
    exts.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything under a scratch key of HKCU that is removed afterwards.
    fn scratch() -> (String, Keys) {
        let root = format!("Software\\rview-test-{}", std::process::id());
        let keys = Keys {
            classes: format!("{root}\\Classes"),
            app: format!("{root}\\App"),
            registered: format!("{root}\\RegisteredApplications"),
            file_exts: format!("{root}\\FileExts"),
        };
        (root, keys)
    }

    #[test]
    fn register_associate_unregister_round_trip() {
        let (root, keys) = scratch();
        let exe = "C:\\Tools\\rview.exe";
        let _ = delete_tree(&root);
        // Another program's state that must survive: a user choice for
        // .pdf, a previous default for .txt and an unrelated verb.
        set_str(&format!("{}\\.pdf\\UserChoice", keys.file_exts), Some("ProgId"), "OtherPdf").unwrap();
        set_str(&format!("{}\\.txt", keys.classes), None, "txtfile").unwrap();
        set_str(&format!("{}\\SystemFileAssociations\\.md\\shell\\edit\\command", keys.classes), None, "other").unwrap();

        assert_eq!(status(&keys, exe), Status::Off);
        register(&keys, exe, "Open with rview").unwrap();
        assert_eq!(status(&keys, exe), Status::Here);
        assert_eq!(status(&keys, "D:\\rview.exe"), Status::Elsewhere);
        let verb = format!("{}\\SystemFileAssociations\\.csv\\shell\\rview", keys.classes);
        assert_eq!(get_str(&verb, Some("MUIVerb")).as_deref(), Some("Open with rview"));
        assert_eq!(get_str(&format!("{verb}\\command"), None).as_deref(), Some("\"C:\\Tools\\rview.exe\" \"%1\""));
        // Registering alone does not change defaults.
        assert_eq!(get_str(&format!("{}\\.txt", keys.classes), None).as_deref(), Some("txtfile"));

        let d = associate(&keys, exe, "Open with rview").unwrap();
        assert_eq!(d.user_choice, vec![".pdf"]);
        assert!(d.set.contains(&".txt") && d.set.contains(&".md") && !d.set.contains(&".pdf"));
        assert_eq!(get_str(&format!("{}\\.txt", keys.classes), None).as_deref(), Some("rview.txt"));
        assert_eq!(get_str(&keys.registered, Some("rview")), Some(format!("{}\\Capabilities", keys.app)));
        // A second run finds everything in place.
        let again = associate(&keys, exe, "Open with rview").unwrap();
        assert!(again.set.is_empty() && again.already.len() == EXTENSIONS.len() - 1);

        unregister(&keys).unwrap();
        assert_eq!(status(&keys, exe), Status::Off);
        assert_eq!(get_str(&format!("{}\\.txt", keys.classes), None).as_deref(), Some("txtfile"));
        assert_eq!(get_str(&format!("{}\\.md", keys.classes), None), None);
        assert_eq!(get_str(&format!("{}\\.pdf\\UserChoice", keys.file_exts), Some("ProgId")).as_deref(), Some("OtherPdf"));
        assert_eq!(get_str(&format!("{}\\SystemFileAssociations\\.md\\shell\\edit\\command", keys.classes), None).as_deref(), Some("other"));
        assert_eq!(get_str(&format!("{}\\rview.pdf", keys.classes), None), None);
        assert_eq!(get_str(&keys.registered, Some("rview")), None);
        assert_eq!(get_str(&format!("{}\\Capabilities", keys.app), Some("ApplicationName")), None);
        delete_tree(&root).unwrap();
    }
}
