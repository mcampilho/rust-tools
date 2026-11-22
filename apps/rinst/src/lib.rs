//! rinst: a portable uninstaller. The library holds everything but the
//! window (`main.rs`), so it can be tested without one.

pub mod app;
pub mod commands;
pub mod i18n;
pub mod leftovers;
pub mod managers;
pub mod page;
pub mod settings;
pub mod shortcuts;
pub mod storage;
pub mod tools;
pub mod ui;
pub mod uninstall;
pub mod winget;

// The programs installed: the inventory, shared with the privileged service.
pub use rt_inst::{appx, inventory, linux_pkgs, msi};

// Sizes and dates as shown, shared with rsize and rdup.
pub use rt_scan::format;
