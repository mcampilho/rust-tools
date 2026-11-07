//! Notification-area (tray) icon for resident mode (`--tray`).
//!
//! The icon belongs to a window of the app. For the main window, the subclass
//! in [`super::window`] routes its callback message to the app as
//! [`super::WinMsg::Tray`]; another window can call [`event`] and [`restore`]
//! from its own window procedure.

use super::wide;
use std::cell::RefCell;
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Callback message of the tray icon.
pub const WM_TRAY: u32 = WM_APP + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// Left click: show the window.
    Activate,
    /// Right click: show the tray menu.
    Menu,
}

thread_local! {
    /// Tooltip of the installed icon (None when no icon is shown).
    static TIP: RefCell<Option<String>> = const { RefCell::new(None) };
}

unsafe fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid
}

unsafe fn notify(hwnd: HWND, op: u32, tip: &str) -> bool {
    let mut nid = icon_data(hwnd);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    let hinst = GetModuleHandleW(std::ptr::null());
    let (cx, cy) = (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON));
    // Resource 1 is the application icon embedded by build.rs.
    let icon = LoadImageW(hinst, 1 as _, IMAGE_ICON, cx, cy, LR_DEFAULTCOLOR);
    nid.hIcon = if icon.is_null() {
        LoadIconW(std::ptr::null_mut(), IDI_APPLICATION)
    } else {
        icon as HICON
    };
    let t: Vec<u16> = tip.encode_utf16().take(nid.szTip.len() - 1).collect();
    nid.szTip[..t.len()].copy_from_slice(&t);
    Shell_NotifyIconW(op, &nid) != 0
}

/// Adds the icon (or updates its tooltip if already shown).
pub fn show(hwnd: HWND, tip: &str) {
    let existed = TIP.with(|t| t.borrow_mut().replace(tip.to_string()).is_some());
    // SAFETY: the notify data points at our live window and local buffers.
    unsafe {
        notify(hwnd, if existed { NIM_MODIFY } else { NIM_ADD }, tip);
    }
}

/// Removes the icon.
pub fn remove(hwnd: HWND) {
    if TIP.with(|t| t.borrow_mut().take()).is_some() {
        // SAFETY: deletes the icon we added for this window.
        unsafe {
            let nid = icon_data(hwnd);
            Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

/// Puts the icon back after Explorer restarted (TaskbarCreated).
pub fn restore(hwnd: HWND) {
    if let Some(tip) = TIP.with(|t| t.borrow().clone()) {
        // SAFETY: as in `show`.
        unsafe {
            notify(hwnd, NIM_ADD, &tip);
        }
    }
}

/// Maps the icon's callback `lParam` to an event.
pub fn event(lp: isize) -> Option<TrayEvent> {
    match lp as u32 {
        WM_LBUTTONUP | WM_LBUTTONDBLCLK => Some(TrayEvent::Activate),
        WM_RBUTTONUP | WM_CONTEXTMENU => Some(TrayEvent::Menu),
        _ => None,
    }
}

/// Shows the tray menu at the cursor. Returns 1 for `open`, 2 for `exit`.
pub fn menu(hwnd: HWND, open: &str, exit: &str) -> Option<u8> {
    // SAFETY: a temporary popup menu, destroyed before returning; strings
    // outlive the calls that use them.
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return None;
        }
        let (o, e) = (wide(open), wide(exit));
        AppendMenuW(menu, MF_STRING, 1, o.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, 2, e.as_ptr());
        SetMenuDefaultItem(menu, 1, 0);
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
        match cmd {
            1 => Some(1),
            2 => Some(2),
            _ => None,
        }
    }
}
