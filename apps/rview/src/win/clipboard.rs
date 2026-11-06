//! Writes a [`Payload`] to the Windows clipboard.

use super::wide;
use crate::clip::{dib, Payload};
use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;

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

/// Replaces the clipboard contents. Returns false if the clipboard could
/// not be opened or written.
pub fn set(hwnd: HWND, p: &Payload) -> bool {
    let text = (!p.text.is_empty()).then(|| utf16_bytes(&p.text));
    let html = p.html.as_deref().map(with_nul);
    let rtf = p.rtf.as_deref().map(with_nul);
    let image = p.image.as_ref().map(dib);
    // SAFETY: the clipboard is opened on this thread, closed on every path
    // after that, and each buffer is copied into its own block by `put`.
    unsafe {
        // Another program may hold the clipboard for a moment.
        let mut open = false;
        for _ in 0..10 {
            open = OpenClipboard(hwnd) != 0;
            if open {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        if !open {
            return false;
        }
        let mut ok = EmptyClipboard() != 0;
        if let Some(t) = &text {
            ok &= put(CF_UNICODETEXT, t);
        }
        if let Some(h) = &html {
            ok &= put(RegisterClipboardFormatW(wide("HTML Format").as_ptr()), h);
        }
        if let Some(r) = &rtf {
            ok &= put(RegisterClipboardFormatW(wide("Rich Text Format").as_ptr()), r);
        }
        if let Some(i) = &image {
            ok &= put(CF_DIB, i);
        }
        CloseClipboard();
        ok
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
    }
}
