//! Win32 integration, from `rt-sys`, with rview's identity, plus what only
//! rview needs: file associations (`assoc`) and copying formatted content
//! (`clipboard`).
//!
//! `unsafe` is confined to this module and `media.rs`: every block calls
//! documented Win32 APIs with buffers whose sizes and lifetimes are checked
//! locally.

pub mod assoc;
pub mod clipboard;

pub use rt_sys::win::{darkmode, print_to_console, report, reveal_in_explorer, set_mnemonics, system_dark, tray, WinMsg, HWND};
use rt_sys::win::Ident;
use std::path::PathBuf;

/// How rview instances find each other.
const IDENT: Ident = Ident { name: "rview", copydata: 0x5256_4945 }; // "RVIE"

/// Installs the main window's subclass (menu commands, files from later
/// launches, tray clicks, theme changes, dark menus).
pub fn subclass(hwnd: HWND, mnemonics: &str, sink: Box<dyn Fn(WinMsg)>) {
    rt_sys::win::subclass(hwnd, &IDENT, mnemonics, sink);
}

/// Marks the window as the resident (tray) instance for later launches.
pub fn set_resident(hwnd: HWND) {
    rt_sys::win::set_resident(hwnd, &IDENT);
}

/// Sends `paths` to a running resident instance, which shows its window.
/// Returns true if handed over (this process should exit).
pub fn hand_off_to_resident(paths: &[PathBuf]) -> bool {
    rt_sys::win::hand_off(&IDENT, paths, false, false)
}

/// Opens a web or mail link; nothing else (see `rt_sys::open_url`).
pub fn open_url(url: &str) -> bool {
    rt_sys::open_url(url)
}

/// The Open dialog.
pub mod dialogs {
    use rt_sys::win::dialogs as d;
    use rt_sys::win::HWND;
    use std::path::{Path, PathBuf};

    /// Shows the Open dialog. `filter` is `(label, patterns)` pairs.
    pub fn open_file(hwnd: HWND, dir: Option<&Path>, filter: &[(String, &str)]) -> Option<PathBuf> {
        d::open_file(hwnd, dir, &d::filter(filter))
    }
}

/// Native Win32 menus built from rview's menu tree.
pub mod menu {
    use crate::commands::Command;
    use rt_sys::win::HWND;

    pub type MenuBar = rt_sys::win::menu::MenuBar<Command>;

    /// Shows a context menu at the mouse pointer and returns the chosen
    /// command. `None` entries are separators; `items` holds each command
    /// and whether it is enabled.
    pub fn popup(hwnd: HWND, items: &[Option<(Command, bool)>]) -> Option<Command> {
        rt_sys::win::menu::popup_at_cursor(hwnd, items)
    }
}
