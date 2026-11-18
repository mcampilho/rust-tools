//! Common dialogs: open/save, folder picker, font chooser, message boxes.

use super::{from_wide, wide};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// A file type filter for the Open and Save dialogs from `(label, patterns)`
/// pairs, with patterns like `*.txt;*.md`.
pub fn filter(pairs: &[(String, &str)]) -> String {
    let mut f = String::new();
    for (label, pat) in pairs {
        f.push_str(&format!("{label} ({pat})\0{pat}\0"));
    }
    f.push('\0');
    f
}

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

fn dir_wide(dir: Option<&Path>) -> Vec<u16> {
    wide(&dir.map(|d| d.display().to_string()).unwrap_or_default())
}

/// Shows the Open dialog for several files. Empty if cancelled. `filter`
/// comes from [`filter`] (or is a raw `label\0patterns\0...\0` string).
pub fn open_files(hwnd: HWND, dir: Option<&Path>, filter: &str) -> Vec<PathBuf> {
    let mut buf = vec![0u16; 64 * 1024];
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let d = dir_wide(dir);
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

/// Shows the Open dialog for one file.
pub fn open_file(hwnd: HWND, dir: Option<&Path>, filter: &str) -> Option<PathBuf> {
    let mut buf = vec![0u16; 32 * 1024];
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let d = dir_wide(dir);
    let mut ofn = base_ofn(hwnd, &mut buf, &filter, &d);
    ofn.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    // SAFETY: ofn points to live buffers for the duration of the call.
    if unsafe { GetOpenFileNameW(&mut ofn) } == 0 {
        return None;
    }
    Some(PathBuf::from(from_wide(&buf)))
}

/// Shows the Save As dialog.
pub fn save_file(hwnd: HWND, suggested: &str, dir: Option<&Path>, filter: &str) -> Option<PathBuf> {
    let mut buf = vec![0u16; 32 * 1024];
    let name: Vec<u16> = suggested.encode_utf16().collect();
    let n = name.len().min(buf.len() - 1);
    buf[..n].copy_from_slice(&name[..n]);
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let d = dir_wide(dir);
    let mut ofn = base_ofn(hwnd, &mut buf, &filter, &d);
    ofn.Flags = OFN_OVERWRITEPROMPT | OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    // SAFETY: as above.
    if unsafe { GetSaveFileNameW(&mut ofn) } == 0 {
        return None;
    }
    Some(PathBuf::from(from_wide(&buf)))
}

/// Folder picker: Windows' own file dialog choosing a folder (it follows the
/// system's light or dark theme), or the old folder tree where that dialog
/// cannot be made.
pub fn pick_folder(hwnd: HWND, title: &str) -> Option<PathBuf> {
    match item_dialog_folder(hwnd, title) {
        Ok(chosen) => chosen,
        Err(()) => tree_folder(hwnd, title),
    }
}

/// The Common Item Dialog (`IFileOpenDialog`) with `FOS_PICKFOLDERS`, called
/// through its COM table (windows-sys has the functions, not the
/// interfaces). Err when it cannot be made; Ok(None) when cancelled.
fn item_dialog_folder(hwnd: HWND, title: &str) -> Result<Option<PathBuf>, ()> {
    use std::ffi::c_void;
    use windows_sys::core::{GUID, HRESULT, PCWSTR, PWSTR};
    use windows_sys::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    const CLSID_FILE_OPEN_DIALOG: GUID = GUID::from_u128(0xDC1C5A9C_E88A_4DDE_A5A1_60F82A20AEF7);
    const IID_IFILE_OPEN_DIALOG: GUID = GUID::from_u128(0xD57C7288_D4AD_4768_BE02_9D969532D960);
    const FOS_NOCHANGEDIR: u32 = 0x8;
    const FOS_PICKFOLDERS: u32 = 0x20;
    const FOS_FORCEFILESYSTEM: u32 = 0x40;
    const SIGDN_FILESYSPATH: u32 = 0x8005_8000;
    // The slots used, in the order of IUnknown, IModalWindow, IFileDialog
    // (Shobjidl.h) and IShellItem.
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;
    type Show = unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT;
    type SetOptions = unsafe extern "system" fn(*mut c_void, u32) -> HRESULT;
    type GetOptions = unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT;
    type SetTitle = unsafe extern "system" fn(*mut c_void, PCWSTR) -> HRESULT;
    type GetResult = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT;
    type GetDisplayName = unsafe extern "system" fn(*mut c_void, u32, *mut PWSTR) -> HRESULT;
    /// The function in slot `i` of a COM object's table.
    ///
    /// SAFETY (callers): `obj` is a live COM object whose table has slot `i`
    /// with the signature `T`.
    unsafe fn slot<T: Copy>(obj: *mut c_void, i: usize) -> T {
        let table = *(obj as *const *const usize);
        std::mem::transmute_copy(&*table.add(i))
    }
    // SAFETY: COM on this (the window's) thread; balanced below when this
    // call started it (S_OK or S_FALSE), left alone when another mode is on.
    let init = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    let result = (|| {
        let mut dlg: *mut c_void = std::ptr::null_mut();
        // SAFETY: known class and interface ids; `dlg` receives the object.
        let hr = unsafe {
            CoCreateInstance(&CLSID_FILE_OPEN_DIALOG, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_IFILE_OPEN_DIALOG, &mut dlg)
        };
        if hr < 0 || dlg.is_null() {
            return Err(());
        }
        // SAFETY: `dlg` is an IFileOpenDialog: slots 2 Release, 3 Show,
        // 9 SetOptions, 10 GetOptions, 17 SetTitle, 20 GetResult; the strings
        // outlive the calls; every object got is released once.
        unsafe {
            let mut opts = 0u32;
            slot::<GetOptions>(dlg, 10)(dlg, &mut opts);
            slot::<SetOptions>(dlg, 9)(dlg, opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR);
            let t = wide(title);
            slot::<SetTitle>(dlg, 17)(dlg, t.as_ptr());
            let shown = slot::<Show>(dlg, 3)(dlg, hwnd);
            let mut chosen = None;
            if shown >= 0 {
                let mut item: *mut c_void = std::ptr::null_mut();
                if slot::<GetResult>(dlg, 20)(dlg, &mut item) >= 0 && !item.is_null() {
                    // IShellItem: slot 5 GetDisplayName.
                    let mut name: PWSTR = std::ptr::null_mut();
                    if slot::<GetDisplayName>(item, 5)(item, SIGDN_FILESYSPATH, &mut name) >= 0 && !name.is_null() {
                        let len = (0..).take_while(|&i| *name.add(i) != 0).count();
                        chosen = Some(PathBuf::from(String::from_utf16_lossy(std::slice::from_raw_parts(name, len))));
                        CoTaskMemFree(name as *const c_void);
                    }
                    slot::<Release>(item, 2)(item);
                }
            }
            slot::<Release>(dlg, 2)(dlg);
            Ok(chosen)
        }
    })();
    if init >= 0 {
        // SAFETY: balances the CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
    result
}

/// The old folder tree (`SHBrowseForFolder`), where the dialog above cannot
/// be made.
fn tree_folder(hwnd: HWND, title: &str) -> Option<PathBuf> {
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

/// A message box, modal to `hwnd` (null: none).
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
                let rc = RegEnumValueW(
                    h,
                    i,
                    name.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null(),
                    &mut ty,
                    data.as_mut_ptr() as *mut u8,
                    &mut data_len,
                );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_are_double_nul_terminated() {
        let f = filter(&[("Text".to_string(), "*.txt;*.md"), ("All".to_string(), "*.*")]);
        assert_eq!(f, "Text (*.txt;*.md)\0*.txt;*.md\0All (*.*)\0*.*\0\0");
    }

    #[test]
    fn segoe_ui_is_found() {
        assert!(font_file("Segoe UI").is_some_and(|p| p.exists()));
        assert_eq!(font_file("No Such Font 123"), None);
    }
}
