//! Explorer's thumbnails and icons: a still of a video for the settings
//! window, and the icons of the files in a fence.

use std::path::Path;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_RESIZETOFIT, SIIGBF_THUMBNAILONLY};

/// The thumbnail Explorer shows for `path`, at most `w`×`h`, as RGBA rows.
/// Call it on a thread of its own (it sets up COM there).
pub fn video(path: &Path, w: u32, h: u32) -> Option<(u32, u32, Vec<u8>)> {
    shell_bitmap(path, w, h, SIIGBF_RESIZETOFIT | SIIGBF_THUMBNAILONLY).map(|(w, h, mut buf)| {
        for p in buf.as_chunks_mut::<4>().0 {
            p[3] = 255;
        }
        (w, h, buf)
    })
}

/// What Explorer shows for `path` (a picture's thumbnail, else its icon),
/// at most `px` across, with its transparency. Call it on a thread of its
/// own (it sets up COM there).
pub fn shell_image(path: &Path, px: u32) -> Option<tiny_skia::Pixmap> {
    let (w, h, mut buf) = shell_bitmap(path, px, px, SIIGBF_RESIZETOFIT)?;
    let chunks = buf.as_chunks_mut::<4>().0;
    // Thumbnails of photos come without alpha: opaque.
    if chunks.iter().all(|p| p[3] == 0) {
        chunks.iter_mut().for_each(|p| p[3] = 255);
    }
    // Premultiplied already; keep it valid.
    for p in chunks.iter_mut() {
        let a = p[3];
        p[0] = p[0].min(a);
        p[1] = p[1].min(a);
        p[2] = p[2].min(a);
    }
    tiny_skia::Pixmap::from_vec(buf, tiny_skia::IntSize::from_wh(w, h)?)
}

/// The bitmap of `IShellItemImageFactory::GetImage`, as RGBA rows (the
/// alpha as Windows gives it).
fn shell_bitmap(path: &Path, w: u32, h: u32, flags: windows::Win32::UI::Shell::SIIGBF) -> Option<(u32, u32, Vec<u8>)> {
    let name = HSTRING::from(path.as_os_str());
    // SAFETY: COM and GDI calls on objects made here and released before
    // returning; the buffer is as large as the bitmap.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None).ok()?;
        let bmp = factory.GetImage(SIZE { cx: w as i32, cy: h as i32 }, flags).ok()?;
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
