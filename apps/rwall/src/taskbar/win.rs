//! The taskbar's look on Windows: the screens, their taskbar windows
//! (`Shell_TrayWnd` on the main screen, `Shell_SecondaryTrayWnd` on the
//! others) and a thread that keeps each one's DWM accent. Explorer puts its
//! own accent back when it redraws the bar (the Start menu, a maximized
//! window...), so the thread looks a few times a second and sets it again
//! only when it changed. When it stops, the bars are normal again.
//!
//! `SetWindowCompositionAttribute` is not documented, but it is what
//! Windows itself and the known taskbar tools use; it is found at run time,
//! and without it nothing happens.

use super::{Screen, Style};
use rt_sys::win::wide;
use std::ffi::c_void;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW, IsWindow};

/// How often the bars are looked at.
const LOOK: Duration = Duration::from_millis(100);
/// How often the list of bars is read again (a screen added, Explorer restarted).
const FIND: Duration = Duration::from_secs(2);

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct AccentPolicy {
    state: i32,
    flags: i32,
    color: u32,
    animation: i32,
}

#[repr(C)]
struct CompositionData {
    attribute: u32,
    data: *mut c_void,
    size: usize,
}

const WCA_ACCENT_POLICY: u32 = 19;

type Composition = unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL;

/// `user32`'s Get/SetWindowCompositionAttribute.
fn functions() -> Option<(Composition, Composition)> {
    let user32 = wide("user32.dll");
    // SAFETY: user32 is always loaded in a program with windows; the names
    // are NUL-terminated; the pointers have the documented-by-use signature.
    unsafe {
        let m = GetModuleHandleW(user32.as_ptr());
        if m.is_null() {
            return None;
        }
        let get = GetProcAddress(m, c"GetWindowCompositionAttribute".as_ptr().cast())?;
        let set = GetProcAddress(m, c"SetWindowCompositionAttribute".as_ptr().cast())?;
        type Found = unsafe extern "system" fn() -> isize;
        Some((std::mem::transmute::<Found, Composition>(get), std::mem::transmute::<Found, Composition>(set)))
    }
}

fn accent(f: Composition, hwnd: HWND, p: &mut AccentPolicy) -> bool {
    let mut d =
        CompositionData { attribute: WCA_ACCENT_POLICY, data: (p as *mut AccentPolicy).cast(), size: std::mem::size_of::<AccentPolicy>() };
    // SAFETY: `d` points at `p`, which lives through the call.
    unsafe { f(hwnd, &mut d) != 0 }
}

/// The policy for a style.
fn policy(st: Style, dark: bool) -> AccentPolicy {
    let (state, color) = st.accent(dark);
    // Flag 2: use the colour (for the clear and acrylic states).
    AccentPolicy { state, flags: if matches!(state, 2 | 4) { 2 } else { 0 }, color, animation: 0 }
}

/// Windows' name of the screen a monitor is ("DISPLAY1"), and its size.
fn monitor_info(m: HMONITOR) -> Option<(String, bool, u32, u32)> {
    // SAFETY: MONITORINFOEXW starts with MONITORINFO and says its size.
    unsafe {
        let mut mi: MONITORINFOEXW = std::mem::zeroed();
        mi.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(m, (&mut mi as *mut MONITORINFOEXW).cast::<MONITORINFO>()) == 0 {
            return None;
        }
        let len = mi.szDevice.iter().position(|c| *c == 0).unwrap_or(mi.szDevice.len());
        let name = String::from_utf16_lossy(&mi.szDevice[..len]);
        let r = mi.monitorInfo.rcMonitor;
        Some((
            name.trim_start_matches("\\\\.\\").to_string(),
            mi.monitorInfo.dwFlags & 1 != 0,
            (r.right - r.left) as u32,
            (r.bottom - r.top) as u32,
        ))
    }
}

/// The screens, the main one first.
pub fn screens() -> Vec<Screen> {
    unsafe extern "system" fn each(m: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the Vec below, alive through EnumDisplayMonitors.
        let list = unsafe { &mut *(data as *mut Vec<Screen>) };
        if let Some((id, primary, width, height)) = monitor_info(m) {
            list.push(Screen { id, primary, width, height });
        }
        1
    }
    let mut list: Vec<Screen> = Vec::new();
    // SAFETY: the callback only touches `list`.
    unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(each), &mut list as *mut Vec<Screen> as LPARAM) };
    list.sort_by_key(|s| (!s.primary, s.id.clone()));
    list
}

/// The taskbar windows, with the screen each is on.
fn bars() -> Vec<(HWND, String)> {
    let mut out = Vec::new();
    let mut add = |h: HWND| {
        if !h.is_null() {
            // SAFETY: any window handle; the monitor is only read.
            let m = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) };
            if let Some((id, ..)) = monitor_info(m) {
                out.push((h, id));
            }
        }
    };
    let (main, other) = (wide("Shell_TrayWnd"), wide("Shell_SecondaryTrayWnd"));
    // SAFETY: NUL-terminated class names; FindWindowExW walks the top-level windows.
    unsafe {
        add(FindWindowW(main.as_ptr(), std::ptr::null()));
        let mut h: HWND = std::ptr::null_mut();
        loop {
            h = FindWindowExW(std::ptr::null_mut(), h, other.as_ptr(), std::ptr::null());
            if h.is_null() {
                break;
            }
            add(h);
        }
    }
    out
}

enum Msg {
    Set(Vec<(String, Style)>, bool),
}

/// Keeps the taskbars' looks on a thread of its own; dropping it (or
/// rwall ending) makes them normal again.
pub struct Keeper {
    tx: mpsc::Sender<Msg>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Keeper {
    /// None when this Windows has no way to change the accent.
    pub fn start() -> Option<Keeper> {
        functions()?;
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new().name("taskbar".into()).spawn(move || run(rx)).ok()?;
        Some(Keeper { tx, thread: Some(thread) })
    }

    /// The looks wanted, by screen, and the theme (for the tint).
    pub fn set(&self, screens: Vec<(String, Style)>, dark: bool) {
        let _ = self.tx.send(Msg::Set(screens, dark));
    }
}

impl Drop for Keeper {
    fn drop(&mut self) {
        // Closing the channel ends the thread, which makes the bars normal.
        let (dead, _) = mpsc::channel();
        drop(std::mem::replace(&mut self.tx, dead));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(rx: mpsc::Receiver<Msg>) {
    let Some((get, set)) = functions() else { return };
    let mut want: Vec<(String, Style)> = Vec::new();
    let mut dark = true;
    let mut list = bars();
    let mut found = Instant::now();
    // The bars given a look, to make normal again at the end.
    let mut touched: Vec<HWND> = Vec::new();
    loop {
        match rx.recv_timeout(LOOK) {
            Ok(Msg::Set(w, d)) => {
                want = w;
                dark = d;
                list = bars();
                found = Instant::now();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        // SAFETY: IsWindow takes any handle.
        if found.elapsed() >= FIND || list.iter().any(|(h, _)| unsafe { IsWindow(*h) } == 0) {
            list = bars();
            found = Instant::now();
        }
        for (h, id) in &list {
            let st = want.iter().find(|(s, _)| s == id).map_or_else(Style::default, |(_, st)| *st);
            let mut p = policy(st, dark);
            if p.state == 0 && !touched.contains(h) {
                continue;
            }
            let mut now = AccentPolicy::default();
            if accent(get, *h, &mut now) && now == p {
                continue;
            }
            accent(set, *h, &mut p);
            if p.state != 0 && !touched.contains(h) {
                touched.push(*h);
            }
        }
    }
    for h in touched {
        let mut normal = AccentPolicy::default();
        // SAFETY: IsWindow takes any handle.
        if unsafe { IsWindow(h) } != 0 {
            accent(set, h, &mut normal);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screens_and_bars_are_found() {
        let s = screens();
        // A session with no screen (a service) has none: nothing to check.
        if s.is_empty() {
            return;
        }
        assert!(s[0].primary && s[0].id.starts_with("DISPLAY"), "{s:?}");
        assert!(s.iter().all(|x| x.width > 0 && x.height > 0));
        for (_, id) in bars() {
            assert!(s.iter().any(|x| x.id == id), "{id}");
        }
        assert!(functions().is_some());
    }

    /// How light the main taskbar looks on the screen (0 to 255): the mean of
    /// a few of its pixels, along its middle.
    fn bar_lightness() -> f32 {
        use windows_sys::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};
        use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
        let (h, _) = bars().into_iter().next().unwrap();
        // SAFETY: the screen's DC is read and released; the rect is written.
        unsafe {
            let mut r: RECT = std::mem::zeroed();
            GetWindowRect(h, &mut r);
            let dc = GetDC(std::ptr::null_mut());
            let (y, n) = ((r.top + r.bottom) / 2, 40);
            let mut sum = 0.0;
            for i in 0..n {
                let c = GetPixel(dc, r.left + (r.right - r.left) * (i * 2 + 1) / (n * 2), y);
                sum += ((c & 0xFF) + (c >> 8 & 0xFF) + (c >> 16 & 0xFF)) as f32 / 3.0;
            }
            ReleaseDC(std::ptr::null_mut(), dc);
            sum / n as f32
        }
    }

    /// This process's CPU time.
    fn cpu() -> Duration {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        // SAFETY: the four times are written.
        unsafe {
            let (mut a, mut b, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) = std::mem::zeroed();
            GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut k, &mut u);
            let t = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32 | u64::from(f.dwLowDateTime)) * 100;
            Duration::from_nanos(t(k) + t(u))
        }
    }

    /// Makes this PC's main taskbar clear with a full white tint for two
    /// seconds, checks on the screen that it turned light, and makes it
    /// normal again (run on request: it changes the real taskbar).
    #[test]
    #[ignore]
    fn clear_white_for_two_seconds() {
        let before = bar_lightness();
        let k = Keeper::start().unwrap();
        let all: Vec<(String, Style)> =
            screens().into_iter().map(|s| (s.id, Style { look: super::super::Look::Clear, tint: 100 })).collect();
        k.set(all, false);
        std::thread::sleep(Duration::from_millis(500));
        let (c0, t0) = (cpu(), Instant::now());
        std::thread::sleep(Duration::from_millis(1500));
        let used = (cpu() - c0).as_secs_f64() / t0.elapsed().as_secs_f64() * 100.0;
        let during = bar_lightness();
        drop(k);
        std::thread::sleep(Duration::from_millis(500));
        let after = bar_lightness();
        println!("lightness before {before:.0}, white {during:.0}, after {after:.0}; keeping it: {used:.2} % of a core");
        // Some of the pixels are the bar's icons, which stay as they were.
        assert!(during > before + 100.0, "white: {before} → {during}");
        // Not exactly as before: a button under the mouse may be lit then.
        assert!(after < during - 100.0, "normal again: {during} → {after}");
    }
}
