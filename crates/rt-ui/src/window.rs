//! The main window: making it with its drawing surface, its icon, its theme,
//! and remembering its size.

use rt_core::menu::M;
use std::rc::Rc;
use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Icon, Theme, Window, WindowAttributes};

/// The surface the frames are copied to.
pub type Surface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

/// A window and its surface. On failure it says why on the standard error
/// (as `app: …`), ends the event loop and gives None.
pub fn create(el: &ActiveEventLoop, attrs: WindowAttributes, app: &str) -> Option<(Rc<Window>, Surface)> {
    let window = match el.create_window(attrs) {
        Ok(w) => Rc::new(w),
        Err(e) => {
            eprintln!("{app}: cannot create window: {e}");
            el.exit();
            return None;
        }
    };
    match softbuffer::Context::new(window.clone()).and_then(|ctx| softbuffer::Surface::new(&ctx, window.clone())) {
        Ok(s) => Some((window, s)),
        Err(e) => {
            eprintln!("{app}: cannot create drawing surface: {e}");
            el.exit();
            None
        }
    }
}

/// Copies a frame to the window.
pub fn present(surface: &mut Surface, gfx: &rt_gfx::Gfx) {
    if let Ok(mut buf) = surface.buffer_mut() {
        gfx.copy_to(&mut buf);
        let _ = buf.present();
    }
}

/// The window icon, from a function that draws it as straight RGBA.
pub fn icon(render: fn(u32) -> Vec<u8>) -> Option<Icon> {
    let n = 32;
    Icon::from_rgba(render(n), n, n).ok()
}

pub fn theme(dark: bool) -> Theme {
    if dark {
        Theme::Dark
    } else {
        Theme::Light
    }
}

/// Light or dark title bar.
pub fn apply_theme(window: &Window, dark: bool) {
    window.set_theme(Some(theme(dark)));
    #[cfg(windows)]
    rt_sys::win::darkmode::refresh_caption(hwnd_of(window));
}

/// Light or dark native menu bar (Windows).
#[cfg(windows)]
pub fn menu_theme(hwnd: rt_sys::win::HWND, dark: bool) {
    let colors = dark.then_some(rt_sys::win::darkmode::MenuColors { bar: 0x202020, text: 0xE4E4E4, text_disabled: 0x7A7A7A, hot: 0x3D3D3D });
    rt_sys::win::darkmode::apply(hwnd, colors);
}

#[cfg(windows)]
pub fn hwnd_of(window: &Window) -> rt_sys::win::HWND {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get() as _,
        _ => std::ptr::null_mut(),
    }
}

/// The access keys of the menu bar's titles, in the current language
/// ("&File" → 'F'), for Alt+key in the window.
pub fn mnemonics_of<C>(tree: &[(&'static str, Vec<M<C>>)]) -> String {
    tree.iter().filter_map(|(t, _)| rt_core::i18n::tr(t).split('&').nth(1).and_then(|s| s.chars().next())).collect()
}

/// Keeps the window's size (logical pixels) and whether it is maximized in
/// `saved`. A maximized, full-screen or minimized window keeps the size it
/// had before.
pub fn remember_size(window: &Window, saved: &mut Option<(f64, f64, bool)>) {
    let max = window.is_maximized();
    if !max && window.fullscreen().is_none() && !window.is_minimized().unwrap_or(false) {
        let s: LogicalSize<f64> = window.inner_size().to_logical(window.scale_factor());
        *saved = Some((s.width, s.height, false));
    } else if let Some((w, h, _)) = *saved {
        *saved = Some((w, h, max));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_keys() {
        let tree: Vec<(&'static str, Vec<M<u8>>)> = vec![("&File", Vec::new()), ("V&iew", Vec::new()), ("Help", Vec::new())];
        assert_eq!(mnemonics_of(&tree), "Fi");
        assert_eq!(theme(true), Theme::Dark);
    }
}
