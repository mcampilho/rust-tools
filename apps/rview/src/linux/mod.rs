//! Linux integration: clipboard, theme, language, the open dialog, and
//! opening links and folders. The tray icon is in `shell`.

pub mod shell;

use crate::clip::Payload;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

/// Kept alive: on X11 the copied data lives in the process that copied it.
static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

fn with_clipboard<T>(f: impl FnOnce(&mut arboard::Clipboard) -> Option<T>) -> Option<T> {
    let mut slot = CLIPBOARD.lock().ok()?;
    if slot.is_none() {
        *slot = arboard::Clipboard::new().ok();
    }
    f(slot.as_mut()?)
}

/// Writes the selection. A clipboard write here holds one kind of data, plus
/// text with HTML: an image goes alone, and RTF is a Windows habit.
pub fn set_clipboard(p: &Payload) -> bool {
    with_clipboard(|c| {
        if let Some(b) = &p.image {
            let image = arboard::ImageData { width: b.w as usize, height: b.h as usize, bytes: (&b.rgba[..]).into() };
            return c.set_image(image).ok();
        }
        match &p.html {
            // The "HTML Format" header is a Windows clipboard convention.
            Some(h) => c.set_html(h.find("<html>").map_or(h.as_str(), |i| &h[i..]), Some(p.text.as_str())).ok(),
            None => c.set_text(p.text.as_str()).ok(),
        }
    })
    .is_some()
}

fn xdg_open(target: std::ffi::OsString) {
    std::thread::spawn(move || {
        let _ = Command::new("xdg-open").arg(target).status();
    });
}

/// Shows the folder of `path` in the file manager.
pub fn reveal(path: &Path) {
    xdg_open(path.parent().unwrap_or(path).as_os_str().to_os_string());
}

/// Opens a web or mail link in the default program. Nothing else: a link in a
/// document must never start a program.
pub fn open_url(url: &str) {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:") {
        xdg_open(url.trim().into());
    }
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

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// Whether a tool for the Open dialog is installed.
pub fn has_dialogs() -> bool {
    on_path("zenity") || on_path("kdialog")
}

/// Asks for a file through zenity or kdialog, the desktop's own first. The
/// call blocks, like the Win32 dialog. `filter` has (name, "*.a;*.b") pairs.
pub fn open_file(dir: Option<&Path>, filter: &[(String, &str)]) -> Option<PathBuf> {
    let kde = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_uppercase().contains("KDE"));
    let use_kdialog = on_path("kdialog") && (kde || !on_path("zenity"));
    let start = dir.map_or_else(String::new, |d| format!("{}/", d.display()));
    let mut cmd = if use_kdialog {
        let mut c = Command::new("kdialog");
        let list: Vec<String> = filter.iter().map(|(name, pat)| format!("{name} ({})", pat.replace(';', " "))).collect();
        c.arg("--getopenfilename").arg(&start).arg(list.join("|"));
        c
    } else {
        let mut c = Command::new("zenity");
        c.arg("--file-selection").arg(format!("--filename={start}"));
        for (name, pat) in filter {
            c.arg(format!("--file-filter={name} | {}", pat.replace(';', " ")));
        }
        c
    };
    let out = cmd.stderr(Stdio::null()).output().ok().filter(|o| o.status.success())?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().next().filter(|l| !l.is_empty()).map(PathBuf::from)
}
