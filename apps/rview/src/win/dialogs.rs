//! Common dialogs: the Open dialog and message boxes.

use super::{from_wide, wide};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Shows the Open dialog. `filter` is `(label, patterns)` pairs.
pub fn open_file(hwnd: HWND, dir: Option<&Path>, filter: &[(String, &str)]) -> Option<PathBuf> {
    let mut buf = vec![0u16; 32 * 1024];
    let mut f = String::new();
    for (label, pat) in filter {
        f.push_str(&format!("{label} ({pat})\0{pat}\0"));
    }
    f.push('\0');
    let filter: Vec<u16> = f.encode_utf16().collect();
    let d = wide(&dir.map(|d| d.display().to_string()).unwrap_or_default());
    // SAFETY: zeroed is a valid initial state for this plain C struct; all
    // pointers refer to buffers that outlive the call.
    unsafe {
        let mut ofn: OPENFILENAMEW = std::mem::zeroed();
        ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = hwnd;
        ofn.lpstrFilter = filter.as_ptr();
        ofn.nFilterIndex = 1;
        ofn.lpstrFile = buf.as_mut_ptr();
        ofn.nMaxFile = buf.len() as u32;
        if d.len() > 1 {
            ofn.lpstrInitialDir = d.as_ptr();
        }
        ofn.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
        if GetOpenFileNameW(&mut ofn) == 0 {
            return None;
        }
    }
    Some(PathBuf::from(from_wide(&buf)))
}

pub fn message(hwnd: HWND, title: &str, text: &str) {
    let (t, c) = (wide(text), wide(title));
    // SAFETY: valid NUL-terminated strings; modal to hwnd.
    unsafe {
        MessageBoxW(hwnd, t.as_ptr(), c.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}
