//! Win32 integration: the main window's message routing and single instance
//! ([`window`]), native menus ([`menu`]) with a dark style ([`darkmode`]),
//! common dialogs ([`dialogs`]), the clipboard ([`clipboard`]), the
//! notification-area icon ([`tray`]), and system settings.
//!
//! `unsafe` is confined to these modules: every block calls documented Win32
//! APIs with buffers whose sizes and lifetimes are checked locally.

pub mod assoc;
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

/// Moves `path` (a file, or a folder with what it holds) to the Recycle Bin.
/// When it cannot go there (a network drive, too large), Windows asks
/// before deleting it for good. Err when it is still there: it failed, or
/// the user said no.
pub fn recycle(owner: HWND, path: &std::path::Path) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_WANTNUKEWARNING, FO_DELETE, SHFILEOPSTRUCTW};
    // A list of paths, each NUL-terminated, ending with an empty one.
    let mut from: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().collect();
    from.extend([0, 0]);
    let mut op = SHFILEOPSTRUCTW {
        hwnd: owner,
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    // SAFETY: `from` is double NUL-terminated and outlives the call.
    let r = unsafe { SHFileOperationW(&mut op) };
    if path.symlink_metadata().is_err() {
        Ok(())
    } else if r != 0 {
        Err(std::io::Error::from_raw_os_error(r).to_string())
    } else {
        Err(String::new())
    }
}

/// Opens Explorer with `path` selected (explicit user command only).
pub fn reveal_in_explorer(path: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    // Explorer reads `/select,"C:\a b\c"` but not the whole argument quoted,
    // which is how a path with spaces would be passed otherwise. A Windows
    // path cannot hold a quote.
    let _ = std::process::Command::new("explorer.exe").raw_arg(reveal_arg(path)).spawn();
}

/// The argument that makes Explorer select `path`.
fn reveal_arg(path: &std::path::Path) -> String {
    format!("/select,\"{}\"", path.display())
}

#[cfg(test)]
mod reveal_tests {
    #[test]
    fn paths_with_spaces_are_quoted_after_select() {
        assert_eq!(super::reveal_arg(std::path::Path::new(r"C:\My Files\a b.txt")), r#"/select,"C:\My Files\a b.txt""#);
    }
}

/// Prints `text` to the console the app was started from, or shows it in a
/// message box when there is none (started from Explorer or a shortcut).
pub fn report(title: &str, text: &str) {
    if !print_to_console(text) {
        dialogs::message(std::ptr::null_mut(), title, text, dialogs::Buttons::Ok, dialogs::Icon::Info);
    }
}

/// Writes `text` to the console the app was started from, and gives the
/// prompt back. False when there is no such console (started from Explorer,
/// a shortcut or Run).
///
/// A program with a window does not hold the console it was started from:
/// the shell has already shown its prompt when the text comes, and seems to
/// wait until Enter is pressed. So Enter is pressed for the user, and the
/// shell shows its prompt again below the text.
pub fn print_to_console(text: &str) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows_sys::Win32::System::Console::{
        AttachConsole, FreeConsole, WriteConsoleInputW, WriteConsoleW, ATTACH_PARENT_PROCESS, INPUT_RECORD, INPUT_RECORD_0, KEY_EVENT, KEY_EVENT_RECORD,
        KEY_EVENT_RECORD_0,
    };
    // SAFETY: attaching to the parent's console has no other effect.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return false;
    }
    let open = |name: &str| {
        let n = wide(name);
        // SAFETY: a NUL-terminated name of the attached console's buffer.
        unsafe { CreateFileW(n.as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut()) }
    };
    let out = open("CONOUT$");
    if out != INVALID_HANDLE_VALUE {
        let units: Vec<u16> = format!("\r\n{}\r\n", text.replace("\r\n", "\n").replace('\n', "\r\n")).encode_utf16().collect();
        let mut written = 0;
        // SAFETY: `units` outlives the call; the handle is open.
        unsafe {
            WriteConsoleW(out, units.as_ptr().cast(), units.len() as u32, &mut written, std::ptr::null());
            CloseHandle(out);
        }
    }
    let input = open("CONIN$");
    if input != INVALID_HANDLE_VALUE {
        const VK_RETURN: u16 = 0x0D;
        let key = |down: bool| INPUT_RECORD {
            EventType: KEY_EVENT as u16,
            Event: INPUT_RECORD_0 {
                KeyEvent: KEY_EVENT_RECORD {
                    bKeyDown: down as i32,
                    wRepeatCount: 1,
                    wVirtualKeyCode: VK_RETURN,
                    wVirtualScanCode: 0x1C,
                    uChar: KEY_EVENT_RECORD_0 { UnicodeChar: u16::from(b'\r') },
                    dwControlKeyState: 0,
                },
            },
        };
        let keys = [key(true), key(false)];
        let mut n = 0;
        // SAFETY: two plain records written to the console's input; the handle is open.
        unsafe {
            WriteConsoleInputW(input, keys.as_ptr(), keys.len() as u32, &mut n);
            CloseHandle(input);
        }
    }
    // SAFETY: lets the console go; nothing of it is used after this.
    unsafe { FreeConsole() };
    true
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

    /// Puts a small file in the Recycle Bin (run on request: it stays there).
    #[test]
    #[ignore]
    fn recycles_a_file() {
        let dir = std::env::temp_dir().join(format!("rt-sys-recycle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("rt-sys recycle test.txt");
        std::fs::write(&f, "test").unwrap();
        recycle(std::ptr::null_mut(), &f).unwrap();
        assert!(!f.exists());
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn system_settings_answer() {
        assert!(ui_language().is_some_and(|t| t.contains('-')));
        let _ = (system_dark(), accent(), caret_blink_ms(), elevated());
    }
}
