//! Windows shell integration: notification-area (tray) icon, single
//! instance activation, the keyboard Calculator key, and its registration.
//!
//! A hidden top-level window of class [`CLASS`] receives tray callbacks and
//! "activate" requests from a second launch. Its window procedure runs on the
//! UI thread (winit pumps all thread messages) and forwards events to the
//! application through a callback.

use std::cell::RefCell;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ,
    RRF_RT_REG_SZ,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, VK_LAUNCH_APP2};
use windows_sys::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const CLASS: &str = "rcalc.shell.v1";
const WM_TRAY: u32 = WM_APP + 1;
const WM_ACTIVATE_REQUEST: u32 = WM_APP + 2;
const HOTKEY_ID: i32 = 0x7C01;
const CMD_OPEN: usize = 1;
const CMD_EXIT: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellEvent {
    /// Show and focus the calculator window.
    Activate,
    /// Quit the resident instance.
    Exit,
}

struct State {
    sink: Box<dyn Fn(ShellEvent)>,
    tray: bool,
    icon: HICON,
    taskbar_created: u32,
    menu_labels: (String, String),
    tip: String,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// If another instance is running, asks it to show its window and returns
/// true. Called before anything else at startup so the second launch exits
/// in milliseconds.
pub fn activate_existing() -> bool {
    let class = wide(CLASS);
    // SAFETY: plain Win32 calls with valid NUL-terminated strings.
    unsafe {
        let h = FindWindowW(class.as_ptr(), std::ptr::null());
        if h.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, &mut pid);
        // Let the running instance bring itself to the foreground.
        AllowSetForegroundWindow(pid);
        PostMessageW(h, WM_ACTIVATE_REQUEST, 0, 0) != 0
    }
}

pub struct Shell {
    hwnd: HWND,
}

impl Shell {
    /// Creates the hidden shell window. With `tray`, adds the notification
    /// area icon and grabs the Calculator key while running.
    pub fn new(tray: bool, tip: &str, menu: (&str, &str), sink: Box<dyn Fn(ShellEvent)>) -> Option<Shell> {
        let class = wide(CLASS);
        // SAFETY: registers a window class whose procedure only touches the
        // thread-local STATE, then creates one hidden window of that class.
        unsafe {
            let hinst = GetModuleHandleW(std::ptr::null());
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinst,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                return None;
            }
            let icon = load_icon(hinst);
            let taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
            STATE.with(|s| {
                *s.borrow_mut() = Some(State {
                    sink,
                    tray,
                    icon,
                    taskbar_created,
                    menu_labels: (menu.0.to_string(), menu.1.to_string()),
                    tip: tip.to_string(),
                })
            });
            if tray {
                add_icon(hwnd);
                RegisterHotKey(hwnd, HOTKEY_ID, 0, VK_LAUNCH_APP2 as u32);
            }
            Some(Shell { hwnd })
        }
    }

    /// Updates the tray menu labels (after a language change).
    pub fn set_labels(&self, tip: &str, open: &str, exit: &str) {
        STATE.with(|s| {
            if let Some(st) = s.borrow_mut().as_mut() {
                st.menu_labels = (open.to_string(), exit.to_string());
                st.tip = tip.to_string();
            }
        });
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        // SAFETY: removes the icon/hotkey owned by our window and destroys it.
        unsafe {
            let tray = STATE.with(|s| s.borrow().as_ref().is_some_and(|st| st.tray));
            if tray {
                let nid = icon_data(self.hwnd);
                Shell_NotifyIconW(NIM_DELETE, &nid);
                UnregisterHotKey(self.hwnd, HOTKEY_ID);
            }
            DestroyWindow(self.hwnd);
        }
        STATE.with(|s| *s.borrow_mut() = None);
    }
}

unsafe fn load_icon(hinst: windows_sys::Win32::Foundation::HINSTANCE) -> HICON {
    let cx = GetSystemMetrics(SM_CXSMICON);
    let cy = GetSystemMetrics(SM_CYSMICON);
    // Resource ID 1 is embedded by build.rs.
    let h = LoadImageW(hinst, 1 as _, IMAGE_ICON, cx, cy, LR_DEFAULTCOLOR);
    if !h.is_null() {
        return h as HICON;
    }
    LoadIconW(std::ptr::null_mut(), IDI_APPLICATION)
}

unsafe fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid
}

unsafe fn add_icon(hwnd: HWND) {
    let mut nid = icon_data(hwnd);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    STATE.with(|s| {
        if let Some(st) = s.borrow().as_ref() {
            nid.hIcon = st.icon;
            let tip: Vec<u16> = st.tip.encode_utf16().take(nid.szTip.len() - 1).collect();
            nid.szTip[..tip.len()].copy_from_slice(&tip);
        }
    });
    Shell_NotifyIconW(NIM_ADD, &nid);
}

fn emit(e: ShellEvent) {
    // Take the callback out while calling it so it may re-enter safely.
    let st = STATE.with(|s| s.borrow_mut().take());
    if let Some(st) = st {
        (st.sink)(e);
        STATE.with(|s| {
            let mut b = s.borrow_mut();
            if b.is_none() {
                *b = Some(st);
            }
        });
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let (open, exit) = STATE
        .with(|s| s.borrow().as_ref().map(|st| st.menu_labels.clone()))
        .unwrap_or_default();
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    AppendMenuW(menu, MF_STRING, CMD_OPEN, wide(&open).as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(menu, MF_STRING, CMD_EXIT, wide(&exit).as_ptr());
    SetMenuDefaultItem(menu, CMD_OPEN as u32, 0);
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    // Required so the menu closes when the user clicks elsewhere.
    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
        pt.x,
        pt.y,
        0,
        hwnd,
        std::ptr::null(),
    );
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu);
    match cmd as usize {
        CMD_OPEN => emit(ShellEvent::Activate),
        CMD_EXIT => emit(ShellEvent::Exit),
        _ => {}
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ACTIVATE_REQUEST => {
            emit(ShellEvent::Activate);
            0
        }
        WM_HOTKEY if wp as i32 == HOTKEY_ID => {
            emit(ShellEvent::Activate);
            0
        }
        WM_TRAY => {
            match lp as u32 {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => emit(ShellEvent::Activate),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            0
        }
        _ => {
            let recreate = STATE.with(|s| {
                s.borrow()
                    .as_ref()
                    .is_some_and(|st| st.tray && st.taskbar_created == msg && msg != 0)
            });
            if recreate {
                // Explorer restarted: put the icon back.
                add_icon(hwnd);
                return 0;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
    }
}

// ---------------------------------------------------------------- Calculator key

/// Shell setting that decides what the keyboard Calculator key
/// (`VK_LAUNCH_APP2`, AppKey 18) starts, per user.
const APPKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\AppKey\\18";

fn exe_command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", exe.display()))
}

/// True when the Calculator key is set to start this executable.
pub fn calc_key_registered() -> bool {
    registered_at(APPKEY)
}

fn registered_at(path: &str) -> bool {
    let Some(cmd) = exe_command() else { return false };
    let (k, v) = (wide(path), wide("ShellExecute"));
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: reads a REG_SZ into a correctly sized buffer.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            k.as_ptr(),
            v.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut _,
            &mut size,
        )
    };
    if rc != 0 {
        return false;
    }
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).eq_ignore_ascii_case(&cmd)
}

/// Points the Calculator key at this executable, or restores the Windows
/// default. Only the current user's setting (HKCU) is changed.
pub fn set_calc_key(enable: bool) -> Result<(), String> {
    set_at(APPKEY, enable)
}

fn set_at(path: &str, enable: bool) -> Result<(), String> {
    let k = wide(path);
    let v = wide("ShellExecute");
    // SAFETY: standard registry create/set/delete on HKCU with valid buffers.
    unsafe {
        let mut key: HKEY = std::ptr::null_mut();
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            k.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        );
        if rc != 0 {
            return Err(format!("registry error {rc}"));
        }
        let rc = if enable {
            let cmd = wide(&exe_command().ok_or("executable path unavailable")?);
            RegSetValueExW(key, v.as_ptr(), 0, REG_SZ, cmd.as_ptr() as *const u8, (cmd.len() * 2) as u32)
        } else {
            match RegDeleteValueW(key, v.as_ptr()) {
                2 => 0, // ERROR_FILE_NOT_FOUND: already default
                rc => rc,
            }
        };
        RegCloseKey(key);
        if rc != 0 {
            return Err(format!("registry error {rc}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Registry::RegDeleteTreeW;

    /// Exercises the registry logic on a throwaway key (never the real
    /// AppKey setting), then removes it.
    #[test]
    fn calc_key_registration_round_trip() {
        let root = format!("Software\\rcalc-test-{}", std::process::id());
        let path = format!("{root}\\AppKey\\18");
        assert!(!registered_at(&path));
        set_at(&path, true).unwrap();
        assert!(registered_at(&path));
        set_at(&path, false).unwrap();
        assert!(!registered_at(&path));
        // Disabling twice is fine.
        set_at(&path, false).unwrap();
        let r = wide(&root);
        // SAFETY: deletes only the test key created above.
        unsafe {
            RegDeleteTreeW(HKEY_CURRENT_USER, r.as_ptr());
        }
    }
}
