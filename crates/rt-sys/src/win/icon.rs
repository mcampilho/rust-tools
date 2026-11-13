//! The icons of programs: an icon resource of an `.exe`, `.dll` or `.ico`
//! (by index, or by resource id when negative), as RGBA pixels.

use super::wide;
use std::path::Path;
use windows_sys::Win32::Graphics::Gdi::{DeleteObject, GetDC, GetDIBits, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS};
use windows_sys::Win32::UI::Shell::ExtractIconExW;
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

/// The rows of a bitmap as 32-bit BGRA, top row first.
///
/// # Safety
/// `bmp` must be a valid bitmap handle of `w`×`h`.
unsafe fn bits(bmp: *mut core::ffi::c_void, w: i32, h: i32) -> Option<Vec<u8>> {
    let mut bmi: BITMAPINFO = std::mem::zeroed();
    bmi.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: -h,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..std::mem::zeroed()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let dc = GetDC(std::ptr::null_mut());
    let rows = GetDIBits(dc, bmp, 0, h as u32, buf.as_mut_ptr().cast(), &mut bmi, DIB_RGB_COLORS);
    ReleaseDC(std::ptr::null_mut(), dc);
    (rows > 0).then_some(buf)
}

/// The size of a bitmap.
///
/// # Safety
/// `bmp` must be a valid bitmap handle.
unsafe fn size_of_bitmap(bmp: *mut core::ffi::c_void) -> Option<(i32, i32)> {
    use windows_sys::Win32::Graphics::Gdi::{GetObjectW, BITMAP};
    let mut info: BITMAP = std::mem::zeroed();
    let got = GetObjectW(bmp, std::mem::size_of::<BITMAP>() as i32, (&mut info as *mut BITMAP).cast());
    (got > 0 && info.bmWidth > 0 && info.bmHeight != 0).then_some((info.bmWidth, info.bmHeight.abs()))
}

/// The pixels of an icon handle: (width, height, straight RGBA).
///
/// # Safety
/// `icon` must be a valid icon handle.
unsafe fn pixels(icon: HICON) -> Option<(u32, u32, Vec<u8>)> {
    let mut ii: ICONINFO = std::mem::zeroed();
    if GetIconInfo(icon, &mut ii) == 0 {
        return None;
    }
    let result = (|| {
        let color = ii.hbmColor;
        if color.is_null() {
            return None;
        }
        let (w, h) = size_of_bitmap(color)?;
        let mut px = bits(color, w, h)?;
        // BGRA → RGBA.
        for p in px.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        // An old icon without alpha: its mask says what is transparent.
        if px.as_chunks::<4>().0.iter().all(|p| p[3] == 0) {
            let mask = (!ii.hbmMask.is_null()).then(|| bits(ii.hbmMask, w, h)).flatten();
            for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let clear = mask.as_ref().is_some_and(|m| m[i * 4] != 0);
                p[3] = if clear { 0 } else { 255 };
            }
        }
        Some((w as u32, h as u32, px))
    })();
    if !ii.hbmColor.is_null() {
        DeleteObject(ii.hbmColor);
    }
    if !ii.hbmMask.is_null() {
        DeleteObject(ii.hbmMask);
    }
    result
}

/// The large icon (32×32 at 100 %) number `index` of a file; a negative
/// index is a resource id. None when the file has no such icon.
pub fn extract(file: &Path, index: i32) -> Option<(u32, u32, Vec<u8>)> {
    let f = wide(&file.to_string_lossy());
    let mut large: HICON = std::ptr::null_mut();
    // SAFETY: a NUL-terminated path and room for one icon handle.
    let n = unsafe { ExtractIconExW(f.as_ptr(), index, &mut large, std::ptr::null_mut(), 1) };
    if n == 0 || n == u32::MAX || large.is_null() {
        return None;
    }
    // SAFETY: a valid icon handle, destroyed once after use.
    unsafe {
        let out = pixels(large);
        DestroyIcon(large);
        out
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn icons_of_the_system() {
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
        let (w, h, px) = super::extract(&std::path::Path::new(&windir).join("explorer.exe"), 0).expect("explorer's icon");
        assert!(w >= 16 && h >= 16);
        assert_eq!(px.len(), (w * h * 4) as usize);
        let px = px.as_chunks::<4>().0;
        assert!(px.iter().any(|p| p[3] == 255) && px.iter().any(|p| p[3] == 0));
        assert!(super::extract(std::path::Path::new("C:\\no\\such.exe"), 0).is_none());
    }
}
