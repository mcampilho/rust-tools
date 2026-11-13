//! The platform-independent core shared by the rust-tools apps:
//!
//! - [`i18n`]: ten UI languages, looked up by their US English text in a
//!   table per app, with a common table for the shared crates.
//! - [`store`]: settings in a small `key=value` file, in the user's data
//!   folder or next to the executable (portable mode).
//! - [`menu`]: the menu tree that both native Win32 menus and the menus
//!   drawn in the window are built from.
//!
//! No dependencies: everything here also runs in tests without a window.

pub mod i18n;
pub mod json;
pub mod menu;
pub mod store;
pub mod textedit;
