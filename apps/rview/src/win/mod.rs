//! Win32 integration: native menus (with dark mode), common dialogs,
//! message routing from the main window, the tray icon, hand-off to a
//! resident instance, and system settings.
//!
//! `unsafe` is confined to this module and `media.rs`: every block calls
//! documented Win32 APIs with buffers whose sizes and lifetimes are checked
//! locally.

pub mod assoc;
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
    /// Files sent by another launch (empty: just show the window).
    OpenFiles(Vec<PathBuf>),
    Tray(tray::TrayEvent),
    /// The Windows light/dark app setting may have changed.
    SystemTheme,
}

type Sink = Box<dyn Fn(WinMsg)>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    static MNEMONICS: RefCell<String> = const { RefCell::new(String::new()) };
    static TASKBAR_CREATED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

const COPYDATA_OPEN: usize = 0x5256_4945; // "RVIE"
/// Set on a window running in resident (tray) mode.
const RESIDENT_PROP: &str = "rview.resident.v1";

fn emit(m: WinMsg) {
    SINK.with(|s| {
        if let Some(f) = s.borrow().as_ref() {
            f(m);
        }
    });
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

fn find_resident() -> HWND {
    struct Found(HWND);
    unsafe extern "system" fn enum_proc(hwnd: HWND, lp: LPARAM) -> i32 {
        let prop = wide(RESIDENT_PROP);
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
    found.0
}

/// Sends `paths` to a running resident instance, which shows its window.
/// Returns true if handed over (this process should exit).
pub fn hand_off_to_resident(paths: &[PathBuf]) -> bool {
    let target = find_resident();
    if target.is_null() {
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
        GetWindowThreadProcessId(target, &mut pid);
        AllowSetForegroundWindow(pid);
        SendMessageW(target, WM_COPYDATA, 0, &cds as *const COPYDATASTRUCT as LPARAM) != 0
    }
}

/// Updates the menu access keys (after the menu language changed).
pub fn set_mnemonics(mnemonics: &str) {
    MNEMONICS.with(|m| *m.borrow_mut() = mnemonics.to_lowercase());
}

/// Whether a WM_SETTINGCHANGE carries "ImmersiveColorSet" (the light/dark
/// app setting or the accent colour changed).
///
/// # Safety
/// `lp` is the message's lParam: null or a NUL-terminated UTF-16 string.
unsafe fn is_color_set_change(lp: LPARAM) -> bool {
    if lp == 0 {
        return false;
    }
    let want: Vec<u16> = "ImmersiveColorSet\0".encode_utf16().collect();
    let p = lp as *const u16;
    // Compare unit by unit, stopping at the first difference, so nothing
    // past the string's terminator is read.
    for (i, w) in want.iter().enumerate() {
        if *p.add(i) != *w {
            return false;
        }
    }
    true
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
            if cds.dwData != COPYDATA_OPEN {
                return DefSubclassProc(hwnd, msg, wp, lp);
            }
            let paths = if cds.lpData.is_null() || cds.cbData == 0 {
                Vec::new()
            } else {
                let units = std::slice::from_raw_parts(cds.lpData as *const u16, cds.cbData as usize / 2);
                String::from_utf16_lossy(units).split('\n').filter(|p| !p.is_empty()).map(PathBuf::from).collect()
            };
            emit(WinMsg::OpenFiles(paths));
            1
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
        WM_SETTINGCHANGE if is_color_set_change(lp) => {
            emit(WinMsg::SystemTheme);
            DefSubclassProc(hwnd, msg, wp, lp)
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

/// Prints `text` to the console rview was started from, or shows it in a
/// message box when there is none (started from Explorer or a shortcut).
pub fn report(title: &str, text: &str) {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    // SAFETY: attaching to the parent's console has no other effect.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0 {
        println!("\n{text}");
    } else {
        dialogs::message(std::ptr::null_mut(), title, text);
    }
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
