//! Linux integration: clipboard, theme, language, fonts, and opening folders.
//! Dialogs are in `dialogs`, single instance and the tray icon in `shell`.

pub mod dialogs;
pub mod shell;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// Kept alive: on X11 the copied text lives in the process that copied it.
static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

fn with_clipboard<T>(f: impl FnOnce(&mut arboard::Clipboard) -> Option<T>) -> Option<T> {
    let mut slot = CLIPBOARD.lock().ok()?;
    if slot.is_none() {
        *slot = arboard::Clipboard::new().ok();
    }
    f(slot.as_mut()?)
}

pub fn get_clipboard() -> Option<String> {
    with_clipboard(|c| c.get_text().ok())
}

pub fn set_clipboard(text: &str) {
    with_clipboard(|c| c.set_text(text).ok());
}

/// Shows the folder of `path` in the file manager.
pub fn reveal(path: &Path) {
    let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) }.to_path_buf();
    std::thread::spawn(move || {
        let _ = Command::new("xdg-open").arg(dir).status();
    });
}

fn gsetting(key: &str) -> Option<String> {
    let out = Command::new("gsettings").args(["get", "org.gnome.desktop.interface", key]).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_lowercase())
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

pub fn ui_language() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|l| !l.is_empty()))
        .map(|l| l.split(['.', '@']).next().unwrap_or("").replace('_', "-"))
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
    let Ok(out) = Command::new("fc-list").args([":spacing=100", "family"]).output() else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut v: Vec<String> = text.lines().filter_map(|l| l.split(',').next()).map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect();
    v.sort();
    v.dedup();
    v
}
