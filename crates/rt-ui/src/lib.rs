//! The window shell the rust-tools apps share, on top of winit and
//! softbuffer: making the window and its drawing surface, the theme of the
//! title bar and the native menus, the keyboard and mouse of the menus drawn
//! in the window (Linux), the message panel (About, Keyboard shortcuts, help),
//! the resident mode's icon (Linux), the Explorer menu of folders
//! (Windows), and the controls of settings pages (`controls`).
//!
//! Each app keeps its own `Shell` and event handler; these are the pieces
//! that were the same in each of them.

pub mod controls;
pub mod menus;
pub mod message;
pub mod window;

#[cfg(windows)]
pub mod explorer;
#[cfg(not(windows))]
pub mod resident;

pub use message::Message;
