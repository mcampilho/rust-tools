//! rsize: a portable disk space analyzer. The library holds everything but
//! the window (`main.rs`), so it can be tested without one.

pub mod app;
pub mod chart;
pub mod commands;
pub mod export;
pub mod i18n;
pub mod settings;
pub mod storage;
pub mod treemap;
pub mod ui;

// Reading the disk, the tree and the formatting of sizes and dates are
// shared with rdup (crates/rt-scan).
pub use rt_scan as scan;
pub use rt_scan::{format, model, volumes};
