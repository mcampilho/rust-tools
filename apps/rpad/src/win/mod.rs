//! Win32 integration: native menus, common dialogs, clipboard, message
//! routing from the main window and single-instance hand-off.
//!
//! `unsafe` is confined to this module: every block calls documented Win32
//! APIs with buffers whose sizes and lifetimes are checked locally.

pub mod darkmode;
pub mod dialogs;
pub mod menu;
pub mod tray;

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
    /// Files sent by a second instance (empty: just show the window).
    OpenFiles(Vec<PathBuf>),
    Tray(tray::TrayEvent),
}

type Sink = Box<dyn Fn(WinMsg)>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    static MNEMONICS: RefCell<String> = const { RefCell::new(String::new()) };
    static TASKBAR_CREATED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

const COPYDATA_OPEN: usize = 0x5250_4144; // "RPAD"
pub const INSTANCE_PROP: &str = "rpad.instance.v1";
/// Set on a window running in resident (tray) mode.
pub const RESIDENT_PROP: &str = "rpad.resident.v1";

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
        TASKBAR_CREATED.with(|t| t.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr())));
    }
}

/// Marks the window as the resident (tray) instance for later launches.
pub fn set_resident(hwnd: HWND) {
    let prop = wide(RESIDENT_PROP);
    // SAFETY: sets a property on our own live window.
    unsafe {
        SetPropW(hwnd, prop.as_ptr(), 1 as _);
    }
}

/// Updates the menu mnemonics (after the menu language changed).
pub fn set_mnemonics(mnemonics: &str) {
    MNEMONICS.with(|m| *m.borrow_mut() = mnemonics.to_lowercase());
}

fn emit(m: WinMsg) {
    SINK.with(|s| {
        if let Some(f) = s.borrow().as_ref() {
            f(m);
        }
    });
}

unsafe extern "system" fn subclass_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if let Some(r) = darkmode::handle(hwnd, msg, wp, lp) {
        return r;
    }
    match msg {
        WM_COMMAND if (wp >> 16) & 0xFFFF == 0 && lp == 0 => {
            emit(WinMsg::Command((wp & 0xFFFF) as u32));
            0
        }
        WM_COPYDATA => {
            let cds = &*(lp as *const COPYDATASTRUCT);
            if cds.dwData == COPYDATA_OPEN {
                let paths = if cds.lpData.is_null() || cds.cbData == 0 {
                    Vec::new()
                } else {
                    let units = std::slice::from_raw_parts(cds.lpData as *const u16, cds.cbData as usize / 2);
                    String::from_utf16_lossy(units).split('\n').filter(|p| !p.is_empty()).map(PathBuf::from).collect()
                };
                emit(WinMsg::OpenFiles(paths));
                return 1;
            }
            DefSubclassProc(hwnd, msg, wp, lp)
        }
        tray::WM_TRAY => {
            if let Some(e) = tray::event(lp) {
                emit(WinMsg::Tray(e));
            }
            0
        }
        m if m != 0 && m == TASKBAR_CREATED.with(|t| t.get()) => {
            // Explorer restarted: put the tray icon back.
            tray::restore(hwnd);
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

/// Finds the window of another running instance (any, or only a resident one).
fn find_instance(resident_only: bool) -> HWND {
    struct Found(HWND, bool);
    unsafe extern "system" fn enum_proc(hwnd: HWND, lp: LPARAM) -> i32 {
        let f = &mut *(lp as *mut Found);
        let prop = wide(if f.1 { RESIDENT_PROP } else { INSTANCE_PROP });
        if !GetPropW(hwnd, prop.as_ptr()).is_null() {
            f.0 = hwnd;
            return 0;
        }
        1
    }
    let mut found = Found(std::ptr::null_mut(), resident_only);
    // SAFETY: the callback writes only into `found`, which outlives the call.
    unsafe {
        EnumWindows(Some(enum_proc), &mut found as *mut Found as LPARAM);
    }
    found.0
}

/// True if a resident (tray) instance is running.
pub fn resident_running() -> bool {
    !find_instance(true).is_null()
}

/// Finds another running instance (a resident one first) and sends it
/// `paths`; it shows its window. Returns true if handed over (this
/// process should exit). `any` allows non-resident instances.
pub fn hand_off_to_existing(paths: &[PathBuf], any: bool) -> bool {
    let mut target = find_instance(true);
    if target.is_null() && any {
        target = find_instance(false);
    }
    let found = (target,);
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
