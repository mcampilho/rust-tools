//! Small OS integrations, from `rt-sys`: clipboard, local date, theme and
//! accent color; plus the keyboard Calculator key (Windows only).

use crate::date::Date;

pub use rt_sys::{accent, system_dark, ui_language};

pub fn get_clipboard() -> Option<String> {
    rt_sys::clipboard_text()
}

pub fn set_clipboard(text: &str) -> bool {
    rt_sys::set_clipboard_text(text)
}

/// Whether the keyboard Calculator key can be redirected to rcalc.
pub const CALC_KEY_SUPPORTED: bool = cfg!(windows);

/// True when the Calculator key launches this executable.
pub fn calc_key_registered() -> bool {
    #[cfg(windows)]
    {
        crate::winshell::calc_key_registered()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Redirects the Calculator key to rcalc (or restores the default).
pub fn set_calc_key(enable: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::winshell::set_calc_key(enable)
    }
    #[cfg(not(windows))]
    {
        let _ = enable;
        Err("not supported on this platform".into())
    }
}

/// Today's local date.
pub fn today() -> Date {
    let t = rt_sys::local_time();
    Date::new(t.year, t.month, t.day).unwrap_or(Date { y: 2026, m: 1, d: 1 })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn today_is_the_local_date() {
        let out = std::process::Command::new("date").arg("+%Y-%-m-%-d").output().unwrap();
        let today = super::today();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), format!("{}-{}-{}", today.y, today.m, today.d));
    }
}
