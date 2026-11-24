//! The main window's messages and single instance.
//!
//! [`subclass`] installs a window procedure on the app's main window that
//! forwards menu commands, files sent by a later launch, tray icon clicks and
//! theme changes to the app, draws dark menus, and suppresses the error beep
//! for Alt+key shortcuts that are not menu mnemonics.
//!
//! A later launch finds the running instance by a window property and hands
//! it its files with `WM_COPYDATA` ([`hand_off`]).

use super::{darkmode, tray, wide};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// How instances of an app recognize each other.
#[derive(Debug, Clone, Copy)]
pub struct Ident {
    /// App name: the window properties are `<name>.instance.v1` and
    /// `<name>.resident.v1`.
    pub name: &'static str,
    /// `WM_COPYDATA` tag of a file hand-off, e.g. `0x5250_4144` ("RPAD").
    pub copydata: usize,
}

impl Ident {
    fn instance_prop(&self) -> Vec<u16> {
        wide(&format!("{}.instance.v1", self.name))
    }

    fn resident_prop(&self) -> Vec<u16> {
        wide(&format!("{}.resident.v1", self.name))
    }
}

/// Messages from the window procedure to the application.
#[derive(Debug, Clone)]
pub enum WinMsg {
    Command(u32),
    /// Files sent by another launch (empty: just show the window).
    OpenFiles(Vec<PathBuf>),
    Tray(tray::TrayEvent),
    /// The Windows light/dark app setting or the accent colour may have changed.
    SystemTheme,
}

type Sink = Box<dyn Fn(WinMsg)>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
    static MNEMONICS: RefCell<String> = const { RefCell::new(String::new()) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    static COPYDATA: Cell<usize> = const { Cell::new(0) };
}

/// Installs the subclass on the main window. `mnemonics` holds the access
/// keys of the menu bar (they open it with Alt).
pub fn subclass(hwnd: HWND, ident: &Ident, mnemonics: &str, sink: Sink) {
    SINK.with(|s| *s.borrow_mut() = Some(sink));
    COPYDATA.with(|c| c.set(ident.copydata));
    set_mnemonics(mnemonics);
    // SAFETY: hwnd is our live top-level window; the procedure only touches
    // thread-local state on this (the UI) thread.
    unsafe {
        SetWindowSubclass(hwnd, Some(subclass_proc), 1, 0);
        SetPropW(hwnd, ident.instance_prop().as_ptr(), 1 as _);
        TASKBAR_CREATED.with(|t| t.set(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr())));
    }
}

/// Marks the window as the resident (tray) instance for later launches.
pub fn set_resident(hwnd: HWND, ident: &Ident) {
    // SAFETY: sets a property on our own live window.
    unsafe {
        SetPropW(hwnd, ident.resident_prop().as_ptr(), 1 as _);
    }
}

/// Updates the menu access keys (after the menu language changed).
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
            if cds.dwData != COPYDATA.with(Cell::get) {
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
        m if m != 0 && m == TASKBAR_CREATED.with(Cell::get) => {
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

/// Finds the window of another running instance (any, or only a resident one).
fn find_instance(ident: &Ident, resident_only: bool) -> HWND {
    struct Found(HWND, Vec<u16>);
    unsafe extern "system" fn enum_proc(hwnd: HWND, lp: LPARAM) -> i32 {
        let f = &mut *(lp as *mut Found);
        if !GetPropW(hwnd, f.1.as_ptr()).is_null() {
            f.0 = hwnd;
            return 0;
        }
        1
    }
    let prop = if resident_only { ident.resident_prop() } else { ident.instance_prop() };
    let mut found = Found(std::ptr::null_mut(), prop);
    // SAFETY: the callback writes only into `found`, which outlives the call.
    unsafe {
        EnumWindows(Some(enum_proc), &mut found as *mut Found as LPARAM);
    }
    found.0
}

/// The process [`hand_off`] would send to (a resident instance first; any
/// one if `any`), and whether its window is visible and minimized.
pub fn instance_state(ident: &Ident, any: bool) -> Option<(u32, bool, bool)> {
    let mut target = find_instance(ident, true);
    if target.is_null() && any {
        target = find_instance(ident, false);
    }
    if target.is_null() {
        return None;
    }
    let mut pid = 0;
    // SAFETY: queries on a window handle found just now.
    unsafe {
        GetWindowThreadProcessId(target, &mut pid);
        Some((pid, IsWindowVisible(target) != 0, IsIconic(target) != 0))
    }
}

/// Lets the process started next bring its window forward. For an instance that was just handed
/// a file by the user's click and starts another window for it.
pub fn allow_any_foreground() {
    // SAFETY: plain call; ASFW_ANY (u32::MAX) grants the right to any process.
    unsafe {
        AllowSetForegroundWindow(u32::MAX);
    }
}

/// True if a resident (tray) instance is running.
pub fn resident_running(ident: &Ident) -> bool {
    !find_instance(ident, true).is_null()
}

/// Finds another running instance (a resident one first; any one if `any`)
/// and sends it `paths`; it shows its window. With `activate`, also brings
/// that window to the front from here. Returns true if handed over (this
/// process should exit).
pub fn hand_off(ident: &Ident, paths: &[PathBuf], any: bool, activate: bool) -> bool {
    let mut target = find_instance(ident, true);
    if target.is_null() && any {
        target = find_instance(ident, false);
    }
    if target.is_null() {
        return false;
    }
    let joined: String = paths
        .iter()
        .map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()).to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    let data: Vec<u16> = joined.encode_utf16().collect();
    let cds = COPYDATASTRUCT { dwData: ident.copydata, cbData: (data.len() * 2) as u32, lpData: data.as_ptr() as *mut _ };
    // SAFETY: WM_COPYDATA marshals `data` synchronously; buffers live until return.
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(target, &mut pid);
        AllowSetForegroundWindow(pid);
        let ok = SendMessageW(target, WM_COPYDATA, 0, &cds as *const COPYDATASTRUCT as LPARAM) != 0;
        if ok && activate {
            // The running instance shows and fills its window in its own time (the files reach
            // it as an event), and a hidden window (in the tray) cannot be brought forward: wait
            // until it is visible, up to a second. Then bring it forward from here: this process
            // was started by the user's click (Explorer's menu, a double click) and may do it,
            // while the instance already running may not, and its window would stay behind
            // the others, as if nothing had opened.
            for _ in 0..100 {
                if IsWindowVisible(target) != 0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if IsIconic(target) != 0 {
                ShowWindow(target, SW_RESTORE);
            }
            SetForegroundWindow(target);
        }
        ok
    }
}
