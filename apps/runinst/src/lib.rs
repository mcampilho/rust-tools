//! runinst: a portable uninstaller. The library holds everything but the
//! window (`main.rs`), so it can be tested without one.

pub mod app;
pub mod appx;
pub mod commands;
pub mod i18n;
pub mod inventory;
pub mod leftovers;
pub mod managers;
pub mod msi;
pub mod settings;
pub mod shortcuts;
pub mod storage;
pub mod ui;
pub mod uninstall;
pub mod winget;

// Sizes and dates as shown, shared with rsize and rdup.
pub use rt_scan::format;
