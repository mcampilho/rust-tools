//! Win32 integration: native menus (with dark mode), common dialogs,
//! menu-command routing from the main window, and system settings.
//!
//! `unsafe` is confined to this module and `media.rs`: every block calls
//! documented Win32 APIs with buffers whose sizes and lifetimes are checked
//! locally.

pub mod darkmode;
pub mod dialogs;
pub mod menu;

use std::cell::RefCell;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

type Sink = Box<dyn Fn(u32)>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    static MNEMONICS: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Installs a subclass on the main window that forwards menu commands to
/// `sink`, opens menus for their access keys, and draws dark menus.
pub fn subclass(hwnd: HWND, mnemonics: &str, sink: Sink) {
    SINK.with(|s| *s.borrow_mut() = Some(sink));
    set_mnemonics(mnemonics);
    // SAFETY: hwnd is our live top-level window; the procedure only touches
    // thread-local state on this (the UI) thread.
    unsafe {
        SetWindowSubclass(hwnd, Some(subclass_proc), 1, 0);
    }
}

/// Updates the menu access keys (after the menu language changed).
pub fn set_mnemonics(mnemonics: &str) {
    MNEMONICS.with(|m| *m.borrow_mut() = mnemonics.to_lowercase());
}

unsafe extern "system" fn subclass_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if let Some(r) = darkmode::handle(hwnd, msg, wp, lp) {
        return r;
    }
    match msg {
        WM_COMMAND if (wp >> 16) & 0xFFFF == 0 && lp == 0 => {
            let id = (wp & 0xFFFF) as u32;
            SINK.with(|s| {
                if let Some(f) = s.borrow().as_ref() {
                    f(id);
                }
            });
            0
        }
        WM_SYSCHAR => {
            // winit needs every WM_SYSCHAR to complete the Alt+key event and
            // answers it without DefWindowProc (so no error beep). The menu
            // bar only opens through DefWindowProc, so call it for real
            // access keys (and Alt+Space for the system menu).
            let r = DefSubclassProc(hwnd, msg, wp, lp);
            let c = char::from_u32(wp as u32).map(|c| c.to_ascii_lowercase());
            let is_mnemonic = c.is_some_and(|c| c.is_alphanumeric() && MNEMONICS.with(|m| m.borrow().contains(c)));
            if is_mnemonic || c == Some(' ') {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            r
        }
        _ => DefSubclassProc(hwnd, msg, wp, lp),
    }
}

/// Opens a web or mail link in the default browser or mail program. Only
/// `http`, `https` and `mailto` links are accepted, so a document can never
/// start a local program.
pub fn open_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("mailto:")) {
        return false;
    }
    let (op, target) = (wide("open"), wide(url.trim()));
    // SAFETY: valid NUL-terminated strings; no parent window needed.
    let r = unsafe { windows_sys::Win32::UI::Shell::ShellExecuteW(std::ptr::null_mut(), op.as_ptr(), target.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    r as usize > 32
}

/// Opens Explorer with `path` selected.
pub fn reveal_in_explorer(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{}", path.display())).spawn();
}

fn reg_dword(key: &str, value: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let (k, v) = (wide(key), wide(value));
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: valid NUL-terminated strings and a correctly sized output buffer.
    let rc = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), v.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut data as *mut u32 as *mut _, &mut size) };
    (rc == 0).then_some(data)
}

/// Whether apps should use the dark theme (Windows personalization setting).
pub fn system_dark() -> Option<bool> {
    reg_dword("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize", "AppsUseLightTheme").map(|v| v == 0)
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
