//! Linux integration, from `rt-sys`, plus copying a selection with its
//! formats: clipboard, theme, the Open dialog, links and folders, and the
//! resident mode with its tray icon.

use crate::clip::Payload;
use std::path::{Path, PathBuf};

pub use rt_sys::linux::{reveal, system_dark};

/// Writes the selection. A clipboard write here holds one kind of data, plus
/// text with HTML: an image goes alone, and RTF is a Windows habit.
pub fn set_clipboard(p: &Payload) -> bool {
    rt_sys::linux::with_clipboard(|c| {
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

/// Opens a web or mail link in the default program. Nothing else: a link in a
/// document must never start a program.
pub fn open_url(url: &str) {
    rt_sys::open_url(url);
}

/// Whether a tool for the Open dialog is installed.
pub fn has_dialogs() -> bool {
    rt_sys::linux::dialogs::available()
}

/// Asks for a file through zenity or kdialog. `filter` has (name,
/// "*.a;*.b") pairs.
pub fn open_file(dir: Option<&Path>, filter: &[(String, &str)]) -> Option<PathBuf> {
    rt_sys::linux::dialogs::open_files(dir, filter, false).into_iter().next()
}

/// Resident mode: the socket for later launches and the tray icon.
pub mod shell {
    pub use rt_sys::linux::shell::{Config, Msg, Shell, TrayLabels};
    use std::path::PathBuf;

    pub const APP: &str = "rview";

    /// Hands `paths` to the resident instance, if there is one.
    pub fn hand_off_to_resident(paths: &[PathBuf]) -> bool {
        rt_sys::linux::shell::hand_off(APP, paths, false)
    }
}
