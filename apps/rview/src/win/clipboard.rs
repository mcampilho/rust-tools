//! Writes a [`Payload`] to the Windows clipboard, in every format it has.

use crate::clip::{dib, Payload};
use rt_sys::win::clipboard::{self as cb, Data};
use rt_sys::win::HWND;

/// Replaces the clipboard contents. Returns false if the clipboard could
/// not be opened or written.
pub fn set(hwnd: HWND, p: &Payload) -> bool {
    let image = p.image.as_ref().map(dib);
    let mut items = Vec::new();
    if !p.text.is_empty() {
        items.push(Data::Text(&p.text));
    }
    if let Some(h) = &p.html {
        items.push(Data::Html(h));
    }
    if let Some(r) = &p.rtf {
        items.push(Data::Rtf(r));
    }
    if let Some(i) = &image {
        items.push(Data::Dib(i));
    }
    cb::set(hwnd, &items)
}
