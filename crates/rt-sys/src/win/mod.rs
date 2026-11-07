//! Win32 integration: the main window's message routing and single instance
//! ([`window`]), native menus ([`menu`]) with a dark style ([`darkmode`]),
//! common dialogs ([`dialogs`]), the clipboard ([`clipboard`]), the
//! notification-area icon ([`tray`]), and system settings.
//!
//! `unsafe` is confined to these modules: every block calls documented Win32
//! APIs with buffers whose sizes and lifetimes are checked locally.

pub mod clipboard;
pub mod darkmode;
pub mod dialogs;
pub mod menu;
pub mod tray;
pub mod window;

pub use window::{hand_off, resident_running, set_mnemonics, set_resident, subclass, Ident, WinMsg};
/// The window handle type of these functions.
pub use windows_sys::Win32::Foundation::HWND;

use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// A NUL-terminated UTF-16 string.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The text of a UTF-16 buffer, up to its first NUL.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// A DWORD value under HKEY_CURRENT_USER.
pub fn reg_dword(key: &str, value: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
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

/// Whether apps should use the dark theme (Windows personalization setting).
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

/// System setting for the text caret blink time in milliseconds (None = no blink).
pub fn caret_blink_ms() -> Option<u32> {
    // SAFETY: no preconditions.
    let ms = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCaretBlinkTime() };
    (ms != 0 && ms != u32::MAX).then_some(ms)
}

/// First preferred Windows display language as a BCP 47 tag ("pt-PT").
pub fn ui_language() -> Option<String> {
    use windows_sys::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
    let mut count = 0u32;
    let mut len = 0u32;
    // SAFETY: the first call queries the buffer size, the second fills it.
    unsafe {
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, std::ptr::null_mut(), &mut len) == 0 || len == 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize];
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut count, buf.as_mut_ptr(), &mut len) == 0 {
            return None;
        }
        Some(from_wide(&buf))
    }
}

/// Opens `target` with its default program. Callers check what it is: see
/// [`crate::open_url`].
pub fn shell_open(target: &str) -> bool {
    let (op, target) = (wide("open"), wide(target));
    // SAFETY: valid NUL-terminated strings; no parent window needed.
    let r = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    r as usize > 32
}

/// Whether this process runs with administrator rights (elevated).
pub fn elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo-handle of this process; the token is closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }
    let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut len = 0u32;
    let size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
    // SAFETY: `e` is as large as stated; `token` is open.
    let ok = unsafe { GetTokenInformation(token, TokenElevation, (&mut e as *mut TOKEN_ELEVATION).cast(), size, &mut len) } != 0;
    // SAFETY: closes the token opened above.
    unsafe { CloseHandle(token) };
    ok && e.TokenIsElevated != 0
}

/// Starts this program again with administrator rights, with `args`
/// (Windows asks the user first). False when it did not start: the user
/// said no, or it could not run.
pub fn restart_elevated(args: &str) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let (verb, file, params) = (wide("runas"), wide(&exe.to_string_lossy()), wide(args));
    // SAFETY: valid NUL-terminated strings; no parent window needed.
    let r = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), params.as_ptr(), std::ptr::null(), SW_SHOWNORMAL)
    };
    r as usize > 32
}

/// Opens Explorer with `path` selected (explicit user command only).
pub fn reveal_in_explorer(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}

/// Prints `text` to the console the app was started from, or shows it in a
/// message box when there is none (started from Explorer or a shortcut).
pub fn report(title: &str, text: &str) {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // SAFETY: attaching to the parent's console has no other effect.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0 {
        println!("\n{text}");
    } else {
        dialogs::message(std::ptr::null_mut(), title, text, dialogs::Buttons::Ok, dialogs::Icon::Info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_strings_round_trip() {
        let w = wide("ação");
        assert_eq!(w.last(), Some(&0));
        assert_eq!(from_wide(&w), "ação");
        assert_eq!(from_wide(&[0x61, 0, 0x62]), "a");
    }

    #[test]
    fn system_settings_answer() {
        assert!(ui_language().is_some_and(|t| t.contains('-')));
        let _ = (system_dark(), accent(), caret_blink_ms(), elevated());
    }
}
