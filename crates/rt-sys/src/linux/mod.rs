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
    fn programs_on_the_path() {
        assert!(on_path("sh"));
        assert!(!on_path("no-such-program-123"));
        assert_eq!(font_file(""), None);
    }
}
