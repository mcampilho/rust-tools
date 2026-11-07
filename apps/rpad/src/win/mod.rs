//! Win32 integration, from `rt-sys`, with rpad's identity and file types.

pub mod menu;

pub use rt_sys::win::{accent, caret_blink_ms, darkmode, set_mnemonics, system_dark, tray, WinMsg};
use rt_sys::win::{HWND, Ident};
use std::path::PathBuf;

/// How rpad instances find each other.
const IDENT: Ident = Ident { name: "rpad", copydata: 0x5250_4144 }; // "RPAD"

/// Installs the main window's subclass (menu commands, files from later
/// launches, tray clicks, dark menus).
pub fn subclass(hwnd: HWND, mnemonics: &str, sink: Box<dyn Fn(WinMsg)>) {
    rt_sys::win::subclass(hwnd, &IDENT, mnemonics, sink);
}

/// Marks the window as the resident (tray) instance for later launches.
pub fn set_resident(hwnd: HWND) {
    rt_sys::win::set_resident(hwnd, &IDENT);
}

/// True if a resident (tray) instance is running.
pub fn resident_running() -> bool {
    rt_sys::win::resident_running(&IDENT)
}

/// Sends `paths` to a running instance (a resident one first; any one if
/// `any`) and brings it to the front. True if handed over.
pub fn hand_off_to_existing(paths: &[PathBuf], any: bool) -> bool {
    rt_sys::win::hand_off(&IDENT, paths, any, true)
}

pub fn get_clipboard() -> Option<String> {
    rt_sys::clipboard_text()
}

pub fn set_clipboard(text: &str) {
    rt_sys::set_clipboard_text(text);
}

pub fn reveal_in_explorer(path: &std::path::Path) {
    rt_sys::win::reveal_in_explorer(path);
}

/// The common dialogs, with rpad's file types.
pub mod dialogs {
    pub use rt_sys::win::dialogs::*;
    use rt_sys::win::HWND;
    use std::path::{Path, PathBuf};

    const FILTER: &str = "All files (*.*)\0*.*\0Text files (*.txt;*.log;*.md)\0*.txt;*.log;*.md;*.markdown\0Source code\0*.rs;*.c;*.h;*.cpp;*.hpp;*.cs;*.java;*.js;*.ts;*.py;*.go;*.sql;*.sh;*.ps1;*.bat;*.cmd\0Markup and data\0*.xml;*.html;*.htm;*.css;*.json;*.yaml;*.yml;*.toml;*.ini;*.csproj;*.xaml;*.svg\0\0";

    /// Shows the Open dialog (multiple selection). Empty if cancelled.
    pub fn open_files(hwnd: HWND, dir: Option<&Path>) -> Vec<PathBuf> {
        rt_sys::win::dialogs::open_files(hwnd, dir, FILTER)
    }

    /// Shows the Save As dialog.
    pub fn save_file(hwnd: HWND, suggested: &str, dir: Option<&Path>) -> Option<PathBuf> {
        rt_sys::win::dialogs::save_file(hwnd, suggested, dir, FILTER)
    }
}
