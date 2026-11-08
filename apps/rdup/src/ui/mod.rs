//! Drawing: the theme, the icon, the pages and the menus drawn in the window.

pub mod appicon;
pub mod gfx;
#[cfg(not(windows))]
pub mod menu;
pub mod theme;
pub mod view;