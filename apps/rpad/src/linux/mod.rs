//! Linux integration, from `rt-sys`: clipboard, theme, fonts, folders,
//! dialogs and the single-instance socket with its tray icon.

pub mod dialogs;

pub use rt_sys::linux::{font_file, reveal, system_dark};

pub fn get_clipboard() -> Option<String> {
    rt_sys::clipboard_text()
}

pub fn set_clipboard(text: &str) {
    rt_sys::set_clipboard_text(text);
}

/// Single instance and the tray icon, for rpad.
pub mod shell {
    pub use rt_sys::linux::shell::{Config, Msg, Shell, TrayLabels};
    use std::path::PathBuf;

    pub const APP: &str = "rpad";

    pub fn resident_running() -> bool {
        rt_sys::linux::shell::resident_running(APP)
    }

    /// Hands `paths` to the running instance, if it is resident or `any` is set.
    pub fn hand_off(paths: &[PathBuf], any: bool) -> bool {
        rt_sys::linux::shell::hand_off(APP, paths, any)
    }
}
