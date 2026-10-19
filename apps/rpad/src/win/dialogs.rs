//! Common dialogs: open/save, folder picker, font chooser, message boxes.

use super::{from_wide, wide};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const FILTER: &str = "All files (*.*)\0*.*\0Text files (*.txt;*.log;*.md)\0*.txt;*.log;*.md;*.markdown\0Source code\0*.rs;*.c;*.h;*.cpp;*.hpp;*.cs;*.java;*.js;*.ts;*.py;*.go;*.sql;*.sh;*.ps1;*.bat;*.cmd\0Markup and data\0*.xml;*.html;*.htm;*.css;*.json;*.yaml;*.yml;*.toml;*.ini;*.csproj;*.xaml;*.svg\0\0";

fn base_ofn(hwnd: HWND, buf: &mut [u16], filter: &[u16], dir: &[u16]) -> OPENFILENAMEW {
    // SAFETY: zeroed is a valid initial state for this plain C struct.
    let mut ofn: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    ofn.hwndOwner = hwnd;
    ofn.lpstrFilter = filter.as_ptr();
    ofn.nFilterIndex = 1;
    ofn.lpstrFile = buf.as_mut_ptr();
    ofn.nMaxFile = buf.len() as u32;
    if dir.len() > 1 {
        ofn.lpstrInitialDir = dir.as_ptr();
    }
    ofn
}

/// Shows the Open dialog (multiple selection). Empty if cancelled.
pub fn open_files(hwnd: HWND, dir: Option<&Path>) -> Vec<PathBuf> {
    let mut buf = vec![0u16; 64 * 1024];
    let filter: Vec<u16> = FILTER.encode_utf16().collect();
    let d = wide(&dir.map(|d| d.display().to_string()).unwrap_or_default());
    let mut ofn = base_ofn(hwnd, &mut buf, &filter, &d);
    ofn.Flags = OFN_ALLOWMULTISELECT | OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    // SAFETY: ofn points to live buffers for the duration of the call.
    if unsafe { GetOpenFileNameW(&mut ofn) } == 0 {
        return Vec::new();
    }
    let parts: Vec<String> = buf.split(|c| *c == 0).take_while(|p| !p.is_empty()).map(String::from_utf16_lossy).collect();
    match parts.len() {
        0 => Vec::new(),
        1 => vec![PathBuf::from(&parts[0])],
        _ => {
            let dir = PathBuf::from(&parts[0]);
            parts[1..].iter().map(|f| dir.join(f)).collect()
        }
    }
}

/// Shows the Save As dialog.
pub fn save_file(hwnd: HWND, suggested: &str, dir: Option<&Path>) -> Option<PathBuf> {
    let mut buf = vec![0u16; 32 * 1024];
    let name: Vec<u16> = suggested.encode_utf16().collect();
    let n = name.len().min(buf.len() - 1);
    buf[..n].copy_from_slice(&name[..n]);
    let filter: Vec<u16> = FILTER.encode_utf16().collect();
    let d = wide(&dir.map(|d| d.display().to_string()).unwrap_or_default());
    let mut ofn = base_ofn(hwnd, &mut buf, &filter, &d);
    ofn.Flags = OFN_OVERWRITEPROMPT | OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    // SAFETY: as above.
    if unsafe { GetSaveFileNameW(&mut ofn) } == 0 {
        return None;
    }
    Some(PathBuf::from(from_wide(&buf)))
}

/// Folder picker.
pub fn pick_folder(hwnd: HWND, title: &str) -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{SHBrowseForFolderW, SHGetPathFromIDListW, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW};
    let t = wide(title);
    let mut name = vec![0u16; 260];
    let bi = BROWSEINFOW {
        hwndOwner: hwnd,
        pidlRoot: std::ptr::null_mut(),
        pszDisplayName: name.as_mut_ptr(),
        lpszTitle: t.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
        lpfn: None,
        lParam: 0,
        iImage: 0,
    };
    // SAFETY: buffers outlive the call; the returned PIDL is freed below.
    unsafe {
        let pidl = SHBrowseForFolderW(&bi);
        if pidl.is_null() {
            return None;
        }
        let mut path = vec![0u16; 32 * 1024];
        let ok = SHGetPathFromIDListW(pidl, path.as_mut_ptr()) != 0;
        CoTaskMemFree(pidl as *const _);
        ok.then(|| PathBuf::from(from_wide(&path)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    Cancel,
    Ok,
}

pub enum Buttons {
    Ok,
    OkCancel,
    YesNoCancel,
}

#[derive(Clone, Copy)]
pub enum Icon {
    Info,
    Warning,
    Error,
    Question,
}

pub fn message(hwnd: HWND, title: &str, text: &str, buttons: Buttons, icon: Icon) -> Answer {
    let (t, c) = (wide(text), wide(title));
    let b = match buttons {
        Buttons::Ok => MB_OK,
        Buttons::OkCancel => MB_OKCANCEL,
        Buttons::YesNoCancel => MB_YESNOCANCEL,
    };
    let i = match icon {
        Icon::Info => MB_ICONINFORMATION,
        Icon::Warning => MB_ICONWARNING,
        Icon::Error => MB_ICONERROR,
        Icon::Question => MB_ICONQUESTION,
    };
    // SAFETY: valid NUL-terminated strings; modal to hwnd.
    let r = unsafe { MessageBoxW(hwnd, t.as_ptr(), c.as_ptr(), b | i) };
    match r {
        IDYES => Answer::Yes,
        IDNO => Answer::No,
        IDOK => Answer::Ok,
        _ => Answer::Cancel,
    }
}

/// Font chooser. Returns (face name, size in points).
pub fn choose_font(hwnd: HWND, face: &str, size_pt: f32) -> Option<(String, f32)> {
    use windows_sys::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, ReleaseDC, LOGFONTW, LOGPIXELSY};
    // SAFETY: zeroed LOGFONTW/CHOOSEFONTW are valid initial states; the DC is released.
    unsafe {
        let mut lf: LOGFONTW = std::mem::zeroed();
        let dc = GetDC(hwnd);
        let dpi = GetDeviceCaps(dc, LOGPIXELSY as i32);
        ReleaseDC(hwnd, dc);
        lf.lfHeight = -((size_pt * dpi as f32 / 72.0).round() as i32);
        let f: Vec<u16> = face.encode_utf16().take(31).collect();
        lf.lfFaceName[..f.len()].copy_from_slice(&f);
        let mut cf: CHOOSEFONTW = std::mem::zeroed();
        cf.lStructSize = std::mem::size_of::<CHOOSEFONTW>() as u32;
        cf.hwndOwner = hwnd;
        cf.lpLogFont = &mut lf;
        cf.Flags = CF_INITTOLOGFONTSTRUCT | CF_SCREENFONTS | CF_NOVERTFONTS | CF_NOSCRIPTSEL;
        if ChooseFontW(&mut cf) == 0 {
            return None;
        }
        Some((from_wide(&lf.lfFaceName), cf.iPointSize as f32 / 10.0))
    }
}

/// Maps a font face name to its file via the registry font list.
pub fn font_file(face: &str) -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::*;
    let key = wide("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Fonts");
    let fonts_dir = std::env::var_os("WINDIR").map(|w| PathBuf::from(w).join("Fonts")).unwrap_or_else(|| "C:\\Windows\\Fonts".into());
    let mut found = None;
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        // SAFETY: enumerates values into fixed-size buffers whose sizes are passed in.
        unsafe {
            let mut h: HKEY = std::ptr::null_mut();
            if RegOpenKeyExW(root, key.as_ptr(), 0, KEY_READ, &mut h) != 0 {
                continue;
            }
            let mut i = 0;
            loop {
                let mut name = [0u16; 512];
                let mut name_len = name.len() as u32;
                let mut data = [0u16; 512];
                let mut data_len = (data.len() * 2) as u32;
                let mut ty = 0u32;
                let rc = RegEnumValueW(h, i, name.as_mut_ptr(), &mut name_len, std::ptr::null(), &mut ty, data.as_mut_ptr() as *mut u8, &mut data_len);
                if rc != 0 {
                    break;
                }
                i += 1;
                let n = from_wide(&name);
                // "Consolas (TrueType)", "Cascadia Mono Regular (TrueType)", "A & B (TrueType)"
                let base = n.split(" (").next().unwrap_or(&n);
                let matches = base.split(" & ").any(|p| p.eq_ignore_ascii_case(face) || p.eq_ignore_ascii_case(&format!("{face} Regular")));
                if matches {
                    let file = PathBuf::from(from_wide(&data));
                    found = Some(if file.is_absolute() { file } else { fonts_dir.join(file) });
                    break;
                }
            }
            RegCloseKey(h);
        }
        if found.is_some() {
            break;
        }
    }
    found
}
