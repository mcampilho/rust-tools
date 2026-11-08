//! rdup: a portable duplicate file finder. The library holds everything but
//! the window (`main.rs`), so it can be tested without one.

pub mod actions;
pub mod app;
pub mod cache;
pub mod commands;
pub mod filter;
pub mod folders;
pub mod i18n;
pub mod search;
pub mod settings;
pub mod storage;
pub mod ui;

// Reading the disk, and the formatting of sizes and dates, are shared with
// rsize (crates/rt-scan).
pub use rt_scan::{format, volumes};
