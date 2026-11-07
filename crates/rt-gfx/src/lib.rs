//! Drawing for the rust-tools apps, all on the CPU:
//!
//! - [`gfx`]: shapes with tiny-skia and text with ab_glyph, in physical
//!   pixels, with the system fonts and fallbacks for symbols and CJK.
//! - [`layout`]: monospace text in character cells (tabs, wide characters,
//!   wrapping).
//! - [`menu`]: menus drawn in the window, for systems without Win32 menus.

pub mod gfx;
pub mod layout;
pub mod menu;

pub use gfx::{Align, Color, FontId, Gfx, Rect};
