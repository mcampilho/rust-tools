//! Drawing: the theme, the icon, the window and the menus drawn in it.

pub mod appicon;
pub mod gfx;
#[cfg(not(windows))]
pub mod menu;
pub mod theme;
pub mod view;
