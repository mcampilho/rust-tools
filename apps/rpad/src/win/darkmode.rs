//! Dark menus.
//!
//! Windows has no public API for dark menus. Like other Win32 editors
//! (Notepad++, Notepad3), rpad uses two well-known undocumented mechanisms:
//!
//! - uxtheme's `SetPreferredAppMode` (ordinal 135, Windows 10 1903+) and
//!   `FlushMenuThemes` (ordinal 136) switch popup menus (menu bar
//!   drop-downs, context menus, the tray menu) to the dark style. On 1809 the
//!   same ordinal is `AllowDarkModeForApp(BOOL)`.
//! - The menu bar itself is drawn by handling the undocumented
//!   `WM_UAHDRAWMENU` / `WM_UAHDRAWMENUITEM` messages, and the one-pixel
//!   light line Windows draws below it is painted over after `WM_NCPAINT`.
//!
//! Every entry point is looked up at run time; if it is missing (older or
//! future Windows) menus simply stay in the system style.

use super::wide;
use std::cell::Cell;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows_sys::Win32::UI::Controls::{CloseThemeData, DrawThemeTextEx, OpenThemeData, DRAWITEMSTRUCT, DTTOPTS, DTT_TEXTCOLOR};
use windows_sys::Win32::UI::Shell::DefSubclassProc;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const WM_UAHDRAWMENU: u32 = 0x0091;
// Owner-draw item states (winuser.h).
const ODS_SELECTED: u32 = 0x0001;
const ODS_GRAYED: u32 = 0x0002;
const ODS_DISABLED: u32 = 0x0004;
const ODS_HOTLIGHT: u32 = 0x0040;
const ODS_INACTIVE: u32 = 0x0080;
const ODS_NOACCEL: u32 = 0x0100;
pub const WM_UAHDRAWMENUITEM: u32 = 0x0092;

/// Menu bar colours as 0xRRGGBB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuColors {
    pub bar: u32,
    pub text: u32,
    pub text_disabled: u32,
    pub hot: u32,
}

thread_local! {
    static COLORS: Cell<Option<MenuColors>> = const { Cell::new(None) };
}

#[repr(C)]
struct UahMenu {
    hmenu: HMENU,
    hdc: HDC,
    flags: u32,
}

#[repr(C)]
struct UahMenuItem {
    position: i32,
    // UAHMENUITEMMETRICS (union of SIZE[4]) and UAHMENUPOPUPMETRICS.
    metrics: [u32; 8],
    popup_metrics: [u32; 5],
}

#[repr(C)]
struct UahDrawMenuItem {
    dis: DRAWITEMSTRUCT,
    um: UahMenu,
    umi: UahMenuItem,
}

fn colorref(rgb: u32) -> u32 {
    ((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF)
}

fn windows_build() -> u32 {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let (k, v) = (wide("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"), wide("CurrentBuildNumber"));
    let mut buf = [0u16; 32];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: reads a REG_SZ into a fixed buffer whose size is passed in.
    let rc = unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, k.as_ptr(), v.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as *mut _, &mut size) };
    if rc != 0 {
        return 0;
    }
    super::from_wide(&buf).trim().parse().unwrap_or(0)
}

/// Switches popup menus of this process to dark (`Some(true)`), light
/// (`Some(false)`) or the Windows app setting (`None`).
fn set_app_mode(dark: Option<bool>) {
    // SAFETY: uxtheme.dll is a system library; the ordinals are resolved at
    // run time and only called when present, with the documented-by-use
    // signatures `fn(i32) -> i32` and `fn()`.
    unsafe {
        let lib = LoadLibraryW(wide("uxtheme.dll").as_ptr());
        if lib.is_null() {
            return;
        }
        let ord = |n: usize| GetProcAddress(lib, n as *const u8);
        let build = windows_build();
        if let Some(f) = ord(135) {
            if build >= 18362 {
                // SetPreferredAppMode: 1 AllowDark (follow system), 2 ForceDark, 3 ForceLight.
                let f: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
                f(match dark {
                    None => 1,
                    Some(true) => 2,
                    Some(false) => 3,
                });
            } else if build >= 17763 {
                // AllowDarkModeForApp(BOOL) on 1809.
                let f: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(f);
                f(i32::from(dark != Some(false)));
            }
        }
        if build >= 17763 {
            if let Some(f) = ord(104) {
                // RefreshImmersiveColorPolicyState
                let f: unsafe extern "system" fn() = std::mem::transmute(f);
                f();
            }
            if let Some(f) = ord(136) {
                // FlushMenuThemes
                let f: unsafe extern "system" fn() = std::mem::transmute(f);
                f();
            }
        }
    }
}

/// Applies the menu theme: dark colours for the menu bar (None = system
/// style) and the matching popup-menu mode.
pub fn apply(hwnd: HWND, colors: Option<MenuColors>) {
    COLORS.with(|c| c.set(colors));
    set_app_mode(Some(colors.is_some()));
    // SAFETY: repaints our own window's menu bar and frame.
    unsafe {
        DrawMenuBar(hwnd);
        RedrawWindow(hwnd, std::ptr::null(), std::ptr::null_mut(), RDW_FRAME | RDW_INVALIDATE);
    }
}

/// Handles the dark menu bar messages. Returns None to let the default
/// processing run.
///
/// # Safety
/// Must be called from the window's subclass procedure with that message's
/// original parameters.
pub unsafe fn handle(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
    let colors = COLORS.with(Cell::get)?;
    match msg {
        WM_UAHDRAWMENU => {
            let um = &*(lp as *const UahMenu);
            let mut mbi: MENUBARINFO = std::mem::zeroed();
            mbi.cbSize = std::mem::size_of::<MENUBARINFO>() as u32;
            GetMenuBarInfo(hwnd, OBJID_MENU, 0, &mut mbi);
            let mut wr: RECT = std::mem::zeroed();
            GetWindowRect(hwnd, &mut wr);
            let mut rc = mbi.rcBar;
            OffsetRect(&mut rc, -wr.left, -wr.top);
            // Extend one pixel down to cover the separator line.
            rc.bottom += 1;
            let brush = CreateSolidBrush(colorref(colors.bar));
            FillRect(um.hdc, &rc, brush);
            DeleteObject(brush);
            Some(1)
        }
        WM_UAHDRAWMENUITEM => {
            let item = &*(lp as *const UahDrawMenuItem);
            let mut text = [0u16; 256];
            let mut mii: MENUITEMINFOW = std::mem::zeroed();
            mii.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
            mii.fMask = MIIM_STRING;
            mii.dwTypeData = text.as_mut_ptr();
            mii.cch = (text.len() - 1) as u32;
            GetMenuItemInfoW(item.um.hmenu, item.umi.position as u32, 1, &mut mii);
            let state = item.dis.itemState;
            let hot = state & (ODS_HOTLIGHT | ODS_SELECTED) != 0;
            let disabled = state & (ODS_GRAYED | ODS_DISABLED | ODS_INACTIVE) != 0;
            let mut rc = item.dis.rcItem;
            let brush = CreateSolidBrush(colorref(if hot { colors.hot } else { colors.bar }));
            FillRect(item.um.hdc, &rc, brush);
            DeleteObject(brush);
            let mut flags = DT_CENTER | DT_SINGLELINE | DT_VCENTER;
            if state & ODS_NOACCEL != 0 {
                flags |= DT_HIDEPREFIX;
            }
            let theme = OpenThemeData(hwnd, wide("Menu").as_ptr());
            let color = colorref(if disabled { colors.text_disabled } else { colors.text });
            if theme != 0 {
                let mut opts: DTTOPTS = std::mem::zeroed();
                opts.dwSize = std::mem::size_of::<DTTOPTS>() as u32;
                opts.dwFlags = DTT_TEXTCOLOR;
                opts.crText = color;
                // MENU_BARITEM = 8, MBI_NORMAL = 1
                DrawThemeTextEx(theme, item.um.hdc, 8, 1, text.as_ptr(), -1, flags, &mut rc, &opts);
                CloseThemeData(theme);
            } else {
                SetBkMode(item.um.hdc, TRANSPARENT as _);
                SetTextColor(item.um.hdc, color);
                DrawTextW(item.um.hdc, text.as_ptr(), -1, &mut rc, flags);
            }
            Some(1)
        }
        WM_NCPAINT | WM_NCACTIVATE => {
            let r = DefSubclassProc(hwnd, msg, wp, lp);
            // Paint over the light line between the menu bar and the client area.
            let mut client: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut client);
            MapWindowPoints(hwnd, std::ptr::null_mut(), &mut client as *mut RECT as *mut POINT, 2);
            let mut wr: RECT = std::mem::zeroed();
            GetWindowRect(hwnd, &mut wr);
            OffsetRect(&mut client, -wr.left, -wr.top);
            let mut line = client;
            line.bottom = line.top;
            line.top -= 1;
            let dc = GetWindowDC(hwnd);
            let brush = CreateSolidBrush(colorref(colors.bar));
            FillRect(dc, &line, brush);
            DeleteObject(brush);
            ReleaseDC(hwnd, dc);
            Some(r)
        }
        _ => None,
    }
}
