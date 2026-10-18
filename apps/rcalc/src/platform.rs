//! Small OS integrations: clipboard, local date, theme and accent color.
//! Windows uses Win32 directly; other platforms get conservative fallbacks.

use crate::date::Date;

#[cfg(windows)]
mod imp {
    use super::Date;
    use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, SYSTEMTIME};
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData};
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn reg_dword(key: &str, value: &str) -> Option<u32> {
        let (k, v) = (wide(key), wide(value));
        let mut data: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: valid NUL-terminated strings and a correctly sized output buffer.
        let rc = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                k.as_ptr(),
                v.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                &mut data as *mut u32 as *mut _,
                &mut size,
            )
        };
        (rc == 0).then_some(data)
    }

    /// First preferred Windows display language as a BCP 47 tag ("pt-PT").
    pub fn ui_language() -> Option<String> {
        use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
        let mut count = 0u32;
        let mut len = 0u32;
        // SAFETY: first call queries the buffer size, second fills it.
        unsafe {
            if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, std::ptr::null_mut(), &mut len) == 0 || len == 0 {
                return None;
            }
            let mut buf = vec![0u16; len as usize];
            if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, buf.as_mut_ptr(), &mut len) == 0 {
                return None;
            }
            let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..end]))
        }
    }

    pub fn today() -> Option<Date> {
        // SAFETY: GetLocalTime only writes the provided struct.
        let st = unsafe {
            let mut st: SYSTEMTIME = std::mem::zeroed();
            GetLocalTime(&mut st);
            st
        };
        Date::new(st.wYear as i32, st.wMonth as u32, st.wDay as u32)
    }

    pub fn system_dark() -> Option<bool> {
        reg_dword(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
            "AppsUseLightTheme",
        )
        .map(|v| v == 0)
    }

    /// System accent color as 0xRRGGBB.
    pub fn accent() -> Option<u32> {
        // Stored as 0xAABBGGRR.
        let v = reg_dword("Software\\Microsoft\\Windows\\DWM", "AccentColor")?;
        let (r, g, b) = (v & 0xFF, (v >> 8) & 0xFF, (v >> 16) & 0xFF);
        Some(r << 16 | g << 8 | b)
    }

    pub fn get_clipboard() -> Option<String> {
        // SAFETY: standard clipboard protocol; the handle is only used while
        // the clipboard is open and the memory is locked.
        unsafe {
            if OpenClipboard(0 as _) == 0 {
                return None;
            }
            let mut out = None;
            let h = GetClipboardData(CF_UNICODETEXT as u32);
            if h as usize != 0 {
                let p = GlobalLock(h as HGLOBAL) as *const u16;
                if !p.is_null() {
                    let mut len = 0usize;
                    while *p.add(len) != 0 && len < 1 << 20 {
                        len += 1;
                    }
                    out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len)));
                    GlobalUnlock(h as HGLOBAL);
                }
            }
            CloseClipboard();
            out
        }
    }

    pub fn set_clipboard(text: &str) -> bool {
        let w = wide(text);
        // SAFETY: allocates a movable global block, copies the UTF-16 text in,
        // and transfers ownership to the clipboard on success.
        unsafe {
            if OpenClipboard(0 as _) == 0 {
                return false;
            }
            EmptyClipboard();
            let bytes = w.len() * 2;
            let h = GlobalAlloc(GMEM_MOVEABLE, bytes);
            let mut ok = false;
            if h as usize != 0 {
                let p = GlobalLock(h) as *mut u16;
                if !p.is_null() {
                    std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                    GlobalUnlock(h);
                    ok = SetClipboardData(CF_UNICODETEXT as u32, h as HANDLE) as usize != 0;
                }
                if !ok {
                    GlobalFree(h);
                }
            }
            CloseClipboard();
            ok
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Date;

    pub fn today() -> Option<Date> {
        // UTC date; local time zones are not resolved on this platform.
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        Date::from_days((secs / 86_400) as i64)
    }
    pub fn system_dark() -> Option<bool> {
        None
    }
    pub fn ui_language() -> Option<String> {
        std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LANG"))
            .ok()
            .map(|v| v.split('.').next().unwrap_or("").replace('_', "-"))
    }
    pub fn accent() -> Option<u32> {
        None
    }
    pub fn get_clipboard() -> Option<String> {
        None
    }
    pub fn set_clipboard(_text: &str) -> bool {
        false
    }
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

pub use imp::{accent, get_clipboard, set_clipboard, system_dark, ui_language};

pub fn today() -> Date {
    imp::today().unwrap_or(Date { y: 2026, m: 1, d: 1 })
}
