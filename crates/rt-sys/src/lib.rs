//! Operating-system integration for the rust-tools apps.
//!
//! The functions at the top level work the same on Windows and Linux. The
//! [`win`] and [`linux`] modules hold what only makes sense on one of them:
//! Win32 menus, dark menus, the tray icon and single instance on Windows;
//! zenity/kdialog dialogs, StatusNotifierItem and a Unix socket on Linux.
//!
//! Linux builds include both X11 and Wayland support (arboard here, winit
//! and softbuffer in the apps), so one executable runs on either.

// An `HWND` is an opaque window handle: Rust never dereferences it, and
// Win32 checks it on every call (an invalid one only makes the call fail).
#![cfg_attr(windows, allow(clippy::not_unsafe_ptr_arg_deref))]

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(windows)]
pub mod win;

use rt_core::i18n::Lang;
use std::path::Path;

/// First preferred display language as a BCP 47 tag ("pt-PT").
pub fn ui_language() -> Option<String> {
    #[cfg(windows)]
    return win::ui_language();
    #[cfg(not(windows))]
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|l| !l.is_empty()))
        .map(|l| l.split(['.', '@']).next().unwrap_or("").replace('_', "-"))
}

/// The system display language, mapped to a supported locale.
pub fn system_lang() -> Lang {
    Lang::best_match(&ui_language().unwrap_or_default())
}

/// Whether apps should use the dark theme (None: unknown).
pub fn system_dark() -> Option<bool> {
    #[cfg(windows)]
    return win::system_dark();
    #[cfg(target_os = "linux")]
    return linux::system_dark();
    #[cfg(not(any(windows, target_os = "linux")))]
    None
}

/// System accent colour as 0xRRGGBB (Windows only).
pub fn accent() -> Option<u32> {
    #[cfg(windows)]
    return win::accent();
    #[cfg(not(windows))]
    None
}

/// Text caret blink time in milliseconds (None: no blinking).
pub fn caret_blink_ms() -> Option<u32> {
    #[cfg(windows)]
    return win::caret_blink_ms();
    #[cfg(not(windows))]
    Some(530)
}

/// Text in the clipboard.
pub fn clipboard_text() -> Option<String> {
    #[cfg(windows)]
    return win::clipboard::get_text();
    #[cfg(target_os = "linux")]
    return linux::with_clipboard(|c| c.get_text().ok());
    #[cfg(not(any(windows, target_os = "linux")))]
    None
}

/// Replaces the clipboard with `text`.
pub fn set_clipboard_text(text: &str) -> bool {
    #[cfg(windows)]
    return win::clipboard::set_text(text);
    #[cfg(target_os = "linux")]
    return linux::with_clipboard(|c| c.set_text(text).ok()).is_some();
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = text;
        false
    }
}

/// Whether `url` is a web or mail link, the only kind an app may open: a
/// link in a document must never start a program.
pub fn is_web_link(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")
}

/// Opens a web or mail link in the default browser or mail program.
/// Anything else is refused (false).
pub fn open_url(url: &str) -> bool {
    if !is_web_link(url) {
        return false;
    }
    #[cfg(windows)]
    return win::shell_open(url.trim());
    #[cfg(target_os = "linux")]
    {
        linux::xdg_open(url.trim().into());
        true
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    false
}

/// Shows `path` in the file manager (selected on Windows; its folder on Linux).
pub fn reveal(path: &Path) {
    #[cfg(windows)]
    win::reveal_in_explorer(path);
    #[cfg(target_os = "linux")]
    linux::reveal(path);
    #[cfg(not(any(windows, target_os = "linux")))]
    let _ = path;
}

/// Moves `path` to the Recycle Bin (Windows) or the trash (Linux). Err when
/// it is still there; an empty message means the user said no.
pub fn recycle(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    return win::recycle(std::ptr::null_mut(), path);
    #[cfg(target_os = "linux")]
    return linux::trash(path);
    #[cfg(not(any(windows, target_os = "linux")))]
    Err(format!("cannot move {} to the trash here", path.display()))
}

/// Local date and time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

/// The local date and time (UTC where the time zone is unknown).
pub fn local_time() -> LocalTime {
    #[cfg(windows)]
    {
        // SAFETY: GetLocalTime only writes the provided struct.
        let st = unsafe {
            let mut st: windows_sys::Win32::Foundation::SYSTEMTIME = std::mem::zeroed();
            windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut st);
            st
        };
        LocalTime {
            year: st.wYear as i32,
            month: st.wMonth as u32,
            day: st.wDay as u32,
            hour: st.wHour as u32,
            minute: st.wMinute as u32,
            second: st.wSecond as u32,
        }
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: time accepts a null pointer; localtime_r only writes `tm`.
        let tm = unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&now, &mut tm);
            tm
        };
        LocalTime {
            year: tm.tm_year + 1900,
            month: tm.tm_mon as u32 + 1,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
        }
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()) as i64;
        // Days to a civil date (Howard Hinnant's algorithm).
        let z = secs.div_euclid(86_400) + 719_468;
        let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        let t = secs.rem_euclid(86_400) as u32;
        LocalTime {
            year,
            month,
            day,
            hour: t / 3600,
            minute: t / 60 % 60,
            second: t % 60,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_web_links_open() {
        assert!(is_web_link(" https://example.com "));
        assert!(is_web_link("MAILTO:a@b.c"));
        assert!(!is_web_link("file:///etc/passwd"));
        assert!(!is_web_link("C:\\Windows\\notepad.exe"));
        assert!(!open_url("javascript:alert(1)"));
    }

    #[test]
    fn local_time_is_plausible() {
        let t = local_time();
        assert!(t.year >= 2024 && (1..=12).contains(&t.month) && (1..=31).contains(&t.day));
        assert!(t.hour < 24 && t.minute < 60 && t.second < 61);
    }

    /// The date from the `date` command (in the same time zone).
    #[cfg(target_os = "linux")]
    #[test]
    fn local_date_matches_the_system() {
        let out = std::process::Command::new("date").arg("+%Y-%-m-%-d").output().unwrap();
        let t = local_time();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            format!("{}-{}-{}", t.year, t.month, t.day)
        );
    }
}
