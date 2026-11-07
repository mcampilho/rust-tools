//! Linux integration: clipboard, theme, fonts, and opening links and
//! folders. Dialogs (through zenity or kdialog) are in [`dialogs`], single
//! instance and the notification-area icon in [`shell`].

pub mod dialogs;
pub mod shell;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// Kept alive: on X11 the copied data lives in the process that copied it.
static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

/// Runs `f` with the process's clipboard (opened on first use).
pub fn with_clipboard<T>(f: impl FnOnce(&mut arboard::Clipboard) -> Option<T>) -> Option<T> {
    let mut slot = CLIPBOARD.lock().ok()?;
    if slot.is_none() {
        *slot = arboard::Clipboard::new().ok();
    }
    f(slot.as_mut()?)
}

/// Opens `target` with the desktop's default program, without waiting.
pub fn xdg_open(target: std::ffi::OsString) {
    std::thread::spawn(move || {
        let _ = Command::new("xdg-open").arg(target).status();
    });
}

/// Shows `path` in the file manager: the folder itself, or the folder of a file.
pub fn reveal(path: &Path) {
    let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
    xdg_open(dir.as_os_str().to_os_string());
}

/// Moves `path` to the desktop's trash: with `gio trash` when it is there,
/// otherwise into the user's own trash (freedesktop.org specification), which
/// only takes what is on the same file system as the home folder.
pub fn trash(path: &Path) -> Result<(), String> {
    if on_path("gio") {
        let ok = Command::new("gio").arg("trash").arg("--").arg(path).status().is_ok_and(|s| s.success());
        if ok && path.symlink_metadata().is_err() {
            return Ok(());
        }
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or("no home folder")?;
    trash_into(path, &data.join("Trash"))
}

/// Moves `path` into the trash folder `trash` (with `files` and `info`).
pub fn trash_into(path: &Path, trash: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&info).map_err(|e| e.to_string())?;
    let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
    let dev = |p: &Path| std::fs::symlink_metadata(p).map(|m| m.dev()).map_err(|e| e.to_string());
    if dev(&abs)? != dev(&files)? {
        return Err("the trash is on another file system".into());
    }
    let base = abs.file_name().ok_or("no name")?.to_string_lossy().into_owned();
    let t = crate::local_time();
    let date = format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second);
    let encoded: String = abs
        .as_os_str()
        .to_string_lossy()
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect();
    for n in 1..10_000 {
        let name = if n == 1 { base.clone() } else { format!("{base}.{n}") };
        let entry = info.join(format!("{name}.trashinfo"));
        // The info file is created first, and only if new: it reserves the name.
        let Ok(mut f) = std::fs::OpenOptions::new().write(true).create_new(true).open(&entry) else { continue };
        use std::io::Write;
        f.write_all(format!("[Trash Info]\nPath={encoded}\nDeletionDate={date}\n").as_bytes()).map_err(|e| e.to_string())?;
        return std::fs::rename(&abs, files.join(&name)).map_err(|e| {
            let _ = std::fs::remove_file(&entry);
            e.to_string()
        });
    }
    Err("no free name in the trash".into())
}

fn gsetting(key: &str) -> Option<String> {
    let out = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", key])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_lowercase())
}

/// GNOME and the desktops that share its settings.
pub fn system_dark() -> Option<bool> {
    if std::env::var("GTK_THEME").is_ok_and(|t| t.to_lowercase().contains("dark")) {
        return Some(true);
    }
    let scheme = gsetting("color-scheme");
    let theme = gsetting("gtk-theme");
    (scheme.is_some() || theme.is_some()).then(|| [scheme, theme].iter().flatten().any(|v| v.contains("dark")))
}

/// Whether a program `name` is on the PATH.
pub fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// File of the font family `face`, or None when it is not installed.
pub fn font_file(face: &str) -> Option<PathBuf> {
    if face.is_empty() {
        return None;
    }
    let out = Command::new("fc-match").args(["-f", "%{family}\n%{file}", face]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (family, file) = text.split_once('\n')?;
    // fc-match always answers: with its nearest font when `face` is missing.
    family.split(',').any(|f| f.eq_ignore_ascii_case(face)).then(|| PathBuf::from(file))
}

/// Monospaced font families, sorted.
pub fn mono_families() -> Vec<String> {
    let Ok(out) = Command::new("fc-list").args([":spacing=100", "family"]).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut v: Vec<String> = text
        .lines()
        .filter_map(|l| l.split(',').next())
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .collect();
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_go_into_the_trash_folder() {
        let dir = std::env::temp_dir().join(format!("rt-sys-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data/sub")).unwrap();
        std::fs::write(dir.join("data/sub/a b.txt"), "x").unwrap();
        let trash = dir.join("Trash");
        trash_into(&dir.join("data/sub"), &trash).unwrap();
        assert!(!dir.join("data/sub").exists());
        assert!(trash.join("files/sub/a b.txt").exists());
        let info = std::fs::read_to_string(trash.join("info/sub.trashinfo")).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath=/") && info.contains("/data/sub\n") && info.contains("DeletionDate="));
        // A second item of the same name gets another.
        std::fs::create_dir_all(dir.join("data/sub")).unwrap();
        trash_into(&dir.join("data/sub"), &trash).unwrap();
        assert!(trash.join("files/sub.2").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn programs_on_the_path() {
        assert!(on_path("sh"));
        assert!(!on_path("no-such-program-123"));
        assert_eq!(font_file(""), None);
    }
}
