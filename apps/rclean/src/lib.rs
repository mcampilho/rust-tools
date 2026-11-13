//! rclean: a portable cleaner. The library holds everything but the window
//! (`main.rs`), so it can be tested without one.

pub mod app;
pub mod clean;
pub mod commands;
pub mod i18n;
pub mod procs;
pub mod settings;
pub mod storage;
pub mod targets;
pub mod ui;

// Sizes and counts as shown, shared with rsize, rdup and runinst.
pub use rt_scan::format;
