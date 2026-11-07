//! The Windows clipboard: text, and several formats at once (text, HTML,
//! RTF, image) for copying formatted content.

use super::{from_wide, wide};
use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;

/// One format of a clipboard write.
pub enum Data<'a> {
    /// Plain text (line breaks become CRLF).
    Text(&'a str),
    /// CF_HTML, with its header (see the "HTML Format" documentation).
    Html(&'a str),
    Rtf(&'a str),
    /// A packed device-independent bitmap (BITMAPINFOHEADER + pixels).
    Dib(&'a [u8]),
}

/// Copies `bytes` into a movable memory block and hands it to the clipboard
/// (which owns it from then on).
///
/// # Safety
/// The clipboard must be open on this thread.
unsafe fn put(format: u32, bytes: &[u8]) -> bool {
    let block = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
    if block.is_null() {
        return false;
    }
    let p = GlobalLock(block);
    if p.is_null() {
        GlobalFree(block);
        return false;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
    GlobalUnlock(block);
    if SetClipboardData(format, block as HANDLE).is_null() {
        GlobalFree(block);
        return false;
    }
    true
}

/// Text in the clipboard's format: UTF-16, CRLF line breaks, NUL-terminated.
fn utf16_bytes(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() * 2 + 2);
    let mut last = 0u16;
    for u in text.encode_utf16() {
        if u == 0x0A && last != 0x0D {
            out.extend(0x0Du16.to_le_bytes());
        }
        out.extend(u.to_le_bytes());
        last = u;
    }
    out.extend(0u16.to_le_bytes());
    out
}

fn with_nul(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() + 1);
    v.extend_from_slice(s.as_bytes());
    v.push(0);
    v
}

/// Opens the clipboard, retrying for a moment: another program may hold it.
///
/// # Safety
/// On success the caller must call `CloseClipboard` on this thread.
unsafe fn open(hwnd: HWND) -> bool {
    for _ in 0..10 {
        if OpenClipboard(hwnd) != 0 {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    false
}

/// Replaces the clipboard contents with every format in `items`. Returns
/// false if the clipboard could not be opened or written.
pub fn set(hwnd: HWND, items: &[Data]) -> bool {
    // SAFETY: the clipboard is opened on this thread, closed on every path
    // after that, and each buffer is copied into its own block by `put`.
    unsafe {
        if !open(hwnd) {
            return false;
        }
        let mut ok = EmptyClipboard() != 0;
        for item in items {
            ok &= match item {
                Data::Text(t) => put(CF_UNICODETEXT, &utf16_bytes(t)),
                Data::Html(h) => put(RegisterClipboardFormatW(wide("HTML Format").as_ptr()), &with_nul(h)),
                Data::Rtf(r) => put(RegisterClipboardFormatW(wide("Rich Text Format").as_ptr()), &with_nul(r)),
                Data::Dib(b) => put(CF_DIB, b),
            };
        }
        CloseClipboard();
        ok
    }
}

/// Replaces the clipboard contents with `text`.
pub fn set_text(text: &str) -> bool {
    set(std::ptr::null_mut(), &[Data::Text(text)])
}

/// The text in the clipboard.
pub fn get_text() -> Option<String> {
    // SAFETY: standard clipboard protocol; reads stay within GlobalSize.
    unsafe {
        if !open(std::ptr::null_mut()) {
            return None;
        }
        let mut out = None;
        let h = GetClipboardData(CF_UNICODETEXT);
        if !h.is_null() {
            let p = GlobalLock(h) as *const u16;
            if !p.is_null() {
                let max = GlobalSize(h) / 2;
                out = Some(from_wide(std::slice::from_raw_parts(p, max)));
                GlobalUnlock(h);
            }
        }
        CloseClipboard();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_uses_crlf_and_a_terminator() {
        let units = |s: &str| -> Vec<u16> { utf16_bytes(s).as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect() };
        assert_eq!(units("a\nb"), vec![97, 13, 10, 98, 0]);
        assert_eq!(units("a\r\nb"), vec![97, 13, 10, 98, 0]);
        assert_eq!(units("é"), vec![0xE9, 0]);
        assert_eq!(with_nul("ab"), b"ab\0");
    }
}
