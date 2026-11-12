//! The layer behind the desktop icons, and what the desktop is doing.
//!
//! Windows has no public way to draw behind the icons. Every live wallpaper
//! program uses the same trick: the undocumented message 0x052C asks the
//! desktop window (`Progman`) for a `WorkerW` window behind the icon layer
//! (`SHELLDLL_DefView`), and the wallpaper becomes a child of it. Icons,
//! files, notifications and widgets stay above, untouched. Since Windows 11
//! 24H2 the icon layer stays inside `Progman`; there the wallpaper becomes a
//! child of `Progman`, placed just below the icons.

use super::Px;
use windows::core::{w, BOOL, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{ClientToScreen, EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONULL};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::StationsAndDesktops::{CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, UOI_NAME};
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Where the wallpaper window goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Host {
    pub parent: HWND,
    /// The icon layer, when it shares the parent: the wallpaper goes just
    /// below it.
    pub icons: Option<HWND>,
}

/// Finds (or makes Explorer create) the layer behind the icons.
pub fn find_host() -> Option<Host> {
    // SAFETY: plain window queries; the callback gets a pointer to a local
    // that outlives the call.
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        let mut res = 0usize;
        // Undocumented, used by every live wallpaper program since Windows Vista.
        SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(1), SMTO_NORMAL, 1000, Some(&mut res));
        SendMessageTimeoutW(progman, 0x052C, WPARAM(0), LPARAM(0), SMTO_NORMAL, 1000, Some(&mut res));
        // Windows 11 24H2 and later (or no WorkerW was made): the icons stay
        // in Progman.
        if let Ok(icons) = FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null()) {
            return Some(Host { parent: progman, icons: Some(icons) });
        }
        // Before: the WorkerW that comes after the one holding the icons.
        let mut found = HWND::default();
        let _ = EnumWindows(Some(next_workerw), LPARAM(&mut found as *mut HWND as isize));
        (!found.is_invalid()).then_some(Host { parent: found, icons: None })
    }
}

/// The top-level window that holds the desktop icons: `Progman`, or the
/// `WorkerW` the icons moved to (see `find_host`). Widgets pinned to the
/// desktop are owned by it, so they stay just above the icons.
pub fn icons_window() -> Option<HWND> {
    // SAFETY: plain window queries; the callback gets a pointer to a local
    // that outlives the call.
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        if FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null()).is_ok() {
            return Some(progman);
        }
        let mut found = HWND::default();
        let _ = EnumWindows(Some(holds_icons), LPARAM(&mut found as *mut HWND as isize));
        (!found.is_invalid()).then_some(found)
    }
}

unsafe extern "system" fn holds_icons(top: HWND, lp: LPARAM) -> BOOL {
    // SAFETY: called by EnumWindows with the pointer given by icons_window.
    unsafe {
        if FindWindowExW(Some(top), None, w!("SHELLDLL_DefView"), PCWSTR::null()).is_ok() {
            *(lp.0 as *mut HWND) = top;
            return BOOL(0);
        }
    }
    BOOL(1)
}

unsafe extern "system" fn next_workerw(top: HWND, lp: LPARAM) -> BOOL {
    // SAFETY: called by EnumWindows with the pointer given by find_host.
    unsafe {
        if FindWindowExW(Some(top), None, w!("SHELLDLL_DefView"), PCWSTR::null()).is_ok() {
            if let Ok(w) = FindWindowExW(None, Some(top), w!("WorkerW"), PCWSTR::null()) {
                *(lp.0 as *mut HWND) = w;
                return BOOL(0);
            }
        }
    }
    BOOL(1)
}

/// The screens, in pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The whole desktop, in screen coordinates.
    pub all: Px,
    /// Each screen, from the top left of `all`.
    pub screens: Vec<Px>,
}

pub fn layout() -> Layout {
    // SAFETY: no preconditions.
    let all = unsafe {
        Px {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            w: GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            h: GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        }
    };
    let mut rects: Vec<RECT> = Vec::new();
    // SAFETY: the callback gets a pointer to `rects`, alive during the call.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(add_monitor), LPARAM(&mut rects as *mut Vec<RECT> as isize));
    }
    let mut screens: Vec<Px> =
        rects.iter().map(|r| Px { x: r.left - all.x, y: r.top - all.y, w: r.right - r.left, h: r.bottom - r.top }).filter(|p| p.w > 0 && p.h > 0).collect();
    if screens.is_empty() {
        screens.push(Px { x: 0, y: 0, w: all.w, h: all.h });
    }
    Layout { all, screens }
}

unsafe extern "system" fn add_monitor(m: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    // SAFETY: called by EnumDisplayMonitors with the pointer given by layout.
    unsafe {
        if GetMonitorInfoW(m, &mut mi).as_bool() {
            (*(lp.0 as *mut Vec<RECT>)).push(mi.rcMonitor);
        }
    }
    BOOL(1)
}

/// Puts `win` behind the icons, over `rect` (screen coordinates).
pub fn attach(win: HWND, host: &Host, rect: Px) -> bool {
    // SAFETY: plain window calls on windows that exist (or fail).
    unsafe {
        if SetParent(win, Some(host.parent)).is_err() {
            return false;
        }
        SetWindowLongPtrW(win, GWL_STYLE, (WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS).0 as isize);
        let mut origin = POINT::default();
        let _ = ClientToScreen(host.parent, &mut origin);
        let (x, y) = (rect.x - origin.x, rect.y - origin.y);
        // Just below the icons when they share the parent; else on top of
        // whatever else lives in the layer (another wallpaper program's).
        let after = host.icons.unwrap_or(HWND_TOP);
        SetWindowPos(win, Some(after), x, y, rect.w, rect.h, SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED).is_ok()
    }
}

/// Whether `win` is still where `attach` put it (Explorer may have restarted).
pub fn still_attached(win: HWND, host: &Host) -> bool {
    // SAFETY: plain window queries.
    unsafe { IsWindow(Some(host.parent)).as_bool() && GetParent(win).is_ok_and(|p| p == host.parent) }
}

/// Shows the still wallpaper again, where the dynamic one was.
pub fn repaint_wallpaper() {
    let mut buf = [0u16; 1024];
    // SAFETY: the buffer is as long as said; setting the same picture again
    // only repaints it.
    unsafe {
        if SystemParametersInfoW(SPI_GETDESKWALLPAPER, buf.len() as u32, Some(buf.as_mut_ptr().cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).is_ok() {
            let _ = SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, Some(buf.as_mut_ptr().cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        }
    }
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: the buffer is as long as said.
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Windows of the shell itself, which never pause the wallpaper.
fn is_shell(h: HWND) -> bool {
    matches!(
        class_of(h).as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "Windows.UI.Core.CoreWindow" | "rwall-live" | "rwall-live-control" | "rwall-widget" | "rwall-widget-control"
    )
}

fn rect_of(h: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    // SAFETY: plain window query.
    unsafe { GetWindowRect(h, &mut r) }.ok().map(|_| r)
}

fn monitor_rect(m: HMONITOR) -> Option<RECT> {
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    // SAFETY: plain query.
    unsafe { GetMonitorInfoW(m, &mut mi) }.as_bool().then_some(mi.rcMonitor)
}

fn covers(r: &RECT, m: &RECT) -> bool {
    r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
}

fn cloaked(h: HWND) -> bool {
    let mut v = 0u32;
    // SAFETY: the value is a u32, as DWMWA_CLOAKED needs.
    unsafe { DwmGetWindowAttribute(h, DWMWA_CLOAKED, (&mut v as *mut u32).cast(), 4) }.is_ok() && v != 0
}

/// A program or a game fills its screen (including the taskbar).
pub fn full_screen_app() -> bool {
    // SAFETY: plain queries.
    unsafe {
        if let Ok(s) = SHQueryUserNotificationState() {
            if s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE {
                return true;
            }
        }
        let fg = GetForegroundWindow();
        if fg.is_invalid() || is_shell(fg) {
            return false;
        }
        let m = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
        match (rect_of(fg), monitor_rect(m)) {
            (Some(r), Some(mr)) => covers(&r, &mr),
            _ => false,
        }
    }
}

/// Every screen is covered by a maximized (or full-screen) window.
pub fn desktop_covered() -> bool {
    let mut monitors: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callbacks get pointers to locals alive during the calls.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(add_handle), LPARAM(&mut monitors as *mut Vec<HMONITOR> as isize));
        if monitors.is_empty() {
            return false;
        }
        let mut covered: Vec<HMONITOR> = Vec::new();
        let _ = EnumWindows(Some(add_covered), LPARAM(&mut covered as *mut Vec<HMONITOR> as isize));
        monitors.iter().all(|m| covered.contains(m))
    }
}

unsafe extern "system" fn add_handle(m: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
    // SAFETY: called with the pointer given by desktop_covered.
    unsafe { (*(lp.0 as *mut Vec<HMONITOR>)).push(m) };
    BOOL(1)
}

unsafe extern "system" fn add_covered(h: HWND, lp: LPARAM) -> BOOL {
    // SAFETY: plain window queries; the pointer comes from desktop_covered.
    unsafe {
        if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() || cloaked(h) {
            return BOOL(1);
        }
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 || is_shell(h) {
            return BOOL(1);
        }
        let m = MonitorFromWindow(h, MONITOR_DEFAULTTONULL);
        if m.is_invalid() {
            return BOOL(1);
        }
        let full = matches!((rect_of(h), monitor_rect(m)), (Some(r), Some(mr)) if covers(&r, &mr));
        if IsZoomed(h).as_bool() || full {
            (*(lp.0 as *mut Vec<HMONITOR>)).push(m);
        }
    }
    BOOL(1)
}

/// The computer is locked (or another desktop, such as a UAC prompt, is in front).
pub fn locked() -> bool {
    // SAFETY: the desktop handle is closed before returning; the buffer is
    // as long as said.
    unsafe {
        let Ok(d) = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) else { return true };
        let mut buf = [0u16; 64];
        let mut need = 0u32;
        let ok = GetUserObjectInformationW(HANDLE(d.0), UOI_NAME, Some(buf.as_mut_ptr().cast()), (buf.len() * 2) as u32, Some(&mut need)).is_ok();
        let _ = CloseDesktop(d);
        let name = String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(buf.len())]);
        ok && !name.eq_ignore_ascii_case("Default")
    }
}

/// The computer runs on its battery.
pub fn on_battery() -> bool {
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: plain query.
    unsafe { GetSystemPowerStatus(&mut s) }.is_ok() && s.ACLineStatus == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screens_and_states() {
        let l = layout();
        assert!(!l.screens.is_empty());
        for s in &l.screens {
            assert!(s.x >= 0 && s.y >= 0 && s.x + s.w <= l.all.w && s.y + s.h <= l.all.h, "{s:?} outside {:?}", l.all);
        }
        // Only that they answer; their values depend on the computer.
        let _ = (full_screen_app(), desktop_covered(), locked(), on_battery());
        assert!(covers(&RECT { left: -1, top: 0, right: 1921, bottom: 1080 }, &RECT { left: 0, top: 0, right: 1920, bottom: 1080 }));
        assert!(!covers(&RECT { left: 0, top: 0, right: 1920, bottom: 1040 }, &RECT { left: 0, top: 0, right: 1920, bottom: 1080 }));
    }
}
