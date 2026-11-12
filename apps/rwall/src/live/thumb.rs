//! A still of a video for the settings window, from Explorer's thumbnails.

use std::path::Path;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT, SIIGBF_THUMBNAILONLY};

/// The thumbnail Explorer shows for `path`, at most `w`×`h`, as RGBA rows.
/// Call it on a thread of its own (it sets up COM there).
pub fn video(path: &Path, w: u32, h: u32) -> Option<(u32, u32, Vec<u8>)> {
    let name = HSTRING::from(path.as_os_str());
    // SAFETY: COM and GDI calls on objects made here and released before
    // returning; the buffer is as large as the bitmap.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None).ok()?;
        let bmp = factory.GetImage(SIZE { cx: w as i32, cy: h as i32 }, SIIGBF_RESIZETOFIT | SIIGBF_THUMBNAILONLY).ok()?;
        let mut info = BITMAP::default();
        let got = GetObjectW(HGDIOBJ(bmp.0), size_of::<BITMAP>() as i32, Some((&mut info as *mut BITMAP).cast()));
        let (bw, bh) = (info.bmWidth, info.bmHeight.abs());
        let result = if got > 0 && bw > 0 && bh > 0 {
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: bw,
                    biHeight: -bh,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut buf = vec![0u8; (bw * bh * 4) as usize];
            let dc = GetDC(None);
            let rows = GetDIBits(dc, bmp, 0, bh as u32, Some(buf.as_mut_ptr().cast()), &mut bmi, DIB_RGB_COLORS);
            ReleaseDC(None, dc);
            (rows > 0).then(|| {
                for p in buf.as_chunks_mut::<4>().0 {
                    p.swap(0, 2);
                    p[3] = 255;
                }
                (bw as u32, bh as u32, buf)
            })
        } else {
            None
        };
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        result
    }
}
