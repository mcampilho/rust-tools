//! Win32 integration: native menus, common dialogs, clipboard, message
//! routing from the main window and single-instance hand-off.
//!
//! `unsafe` is confined to this module: every block calls documented Win32
//! APIs with buffers whose sizes and lifetimes are checked locally.

pub mod dialogs;
pub mod menu;

use std::cell::RefCell;
use std::path::PathBuf;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Messages from the window procedure to the application.
#[derive(Debug, Clone)]
pub enum WinMsg {
    Command(u32),
    /// Files sent by a second instance.
    OpenFiles(Vec<PathBuf>),
}

type Sink = Box<dyn Fn(WinMsg)>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    static MNEMONICS: RefCell<String> = const { RefCell::new(String::new()) };
}

const COPYDATA_OPEN: usize = 0x5250_4144; // "RPAD"
pub const INSTANCE_PROP: &str = "rpad.instance.v1";

/// Installs a subclass on the main window to receive menu commands,
/// files from other instances and to suppress the error beep for Alt+key
/// shortcuts that are not menu mnemonics.
pub fn subclass(hwnd: HWND, mnemonics: &str, sink: Box<dyn Fn(WinMsg)>) {
    SINK.with(|s| *s.borrow_mut() = Some(sink));
    MNEMONICS.with(|m| *m.borrow_mut() = mnemonics.to_lowercase());
    // SAFETY: hwnd is our live top-level window; the procedure only touches
    // thread-local state on this (the UI) thread.
    unsafe {
        SetWindowSubclass(hwnd, Some(subclass_proc), 1, 0);
        let prop = wide(INSTANCE_PROP);
        SetPropW(hwnd, prop.as_ptr(), 1 as _);
    }
}

fn emit(m: WinMsg) {
    SINK.with(|s| {
        if let Some(f) = s.borrow().as_ref() {
            f(m);
        }
    });
}

unsafe extern "system" fn subclass_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match msg {
        WM_COMMAND if (wp >> 16) & 0xFFFF == 0 && lp == 0 => {
            emit(WinMsg::Command((wp & 0xFFFF) as u32));
            0
        }
        WM_COPYDATA => {
            let cds = &*(lp as *const COPYDATASTRUCT);
            if cds.dwData == COPYDATA_OPEN && !cds.lpData.is_null() {
                let units = std::slice::from_raw_parts(cds.lpData as *const u16, cds.cbData as usize / 2);
                let paths = String::from_utf16_lossy(units).split('\n').filter(|p| !p.is_empty()).map(PathBuf::from).collect();
                emit(WinMsg::OpenFiles(paths));
                return 1;
            }
            DefSubclassProc(hwnd, msg, wp, lp)
        }
        WM_SYSCHAR => {
            // winit needs every WM_SYSCHAR to complete the Alt+key event and
            // answers it without DefWindowProc (so no error beep). The menu
            // bar only opens through DefWindowProc, so call it for real
            // mnemonics (and Alt+Space for the system menu).
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

/// Finds another running instance and sends it `paths`. Returns true if
/// the files were handed over (this process should exit).
pub fn hand_off_to_existing(paths: &[PathBuf]) -> bool {
    struct Found(HWND);
    unsafe extern "system" fn enum_proc(hwnd: HWND, lp: LPARAM) -> i32 {
        let prop = wide(INSTANCE_PROP);
        if !GetPropW(hwnd, prop.as_ptr()).is_null() {
            (*(lp as *mut Found)).0 = hwnd;
            return 0;
        }
        1
    }
    let mut found = Found(std::ptr::null_mut());
    // SAFETY: the callback writes only into `found`, which outlives the call.
    unsafe {
        EnumWindows(Some(enum_proc), &mut found as *mut Found as LPARAM);
    }
    if found.0.is_null() {
        return false;
    }
    let joined: String = paths
        .iter()
        .map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()).to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    let data: Vec<u16> = joined.encode_utf16().collect();
    let cds = COPYDATASTRUCT { dwData: COPYDATA_OPEN, cbData: (data.len() * 2) as u32, lpData: data.as_ptr() as *mut _ };
    // SAFETY: WM_COPYDATA marshals `data` synchronously; buffers live until return.
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(found.0, &mut pid);
        AllowSetForegroundWindow(pid);
        let ok = SendMessageW(found.0, WM_COPYDATA, 0, &cds as *const COPYDATASTRUCT as LPARAM) != 0;
        if ok {
            if IsIconic(found.0) != 0 {
                ShowWindow(found.0, SW_RESTORE);
            }
            SetForegroundWindow(found.0);
        }
        ok
    }
}

// ---------------------------------------------------------------- clipboard

pub fn get_clipboard() -> Option<String> {
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
    // SAFETY: standard clipboard protocol; reads stay within GlobalSize.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let mut out = None;
        let h = GetClipboardData(CF_UNICODETEXT as u32);
        if !h.is_null() {
            let p = GlobalLock(h) as *const u16;
            if !p.is_null() {
                let max = GlobalSize(h) / 2;
                let slice = std::slice::from_raw_parts(p, max);
                out = Some(from_wide(slice));
                GlobalUnlock(h);
            }
        }
        CloseClipboard();
        out
    }
}

pub fn set_clipboard(text: &str) -> bool {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
    // Windows expects CRLF in clipboard text.
    let text = text.replace('\n', "\r\n");
    let w = wide(&text);
    // SAFETY: allocates, fills and hands a movable block to the clipboard.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2);
        let mut ok = false;
        if !h.is_null() {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
                GlobalUnlock(h);
                ok = !SetClipboardData(CF_UNICODETEXT as u32, h).is_null();
            }
            if !ok {
                GlobalFree(h);
            }
        }
        CloseClipboard();
        ok
    }
}

/// Opens Explorer with `path` selected (explicit user command only).
pub fn reveal_in_explorer(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{}", path.display())).spawn();
}

/// System setting for the text caret blink time in milliseconds (None = no blink).
pub fn caret_blink_ms() -> Option<u32> {
    // SAFETY: no preconditions.
    let ms = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCaretBlinkTime() };
    (ms != 0 && ms != u32::MAX).then_some(ms)
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

/// System accent color as 0xRRGGBB.
pub fn accent() -> Option<u32> {
    // Stored as 0xAABBGGRR.
    let v = reg_dword("Software\\Microsoft\\Windows\\DWM", "AccentColor")?;
    let (r, g, b) = (v & 0xFF, (v >> 8) & 0xFF, (v >> 16) & 0xFF);
    Some(r << 16 | g << 8 | b)
}
