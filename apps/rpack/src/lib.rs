//! rpack: a portable archiver. The library holds everything but the window
//! (`main.rs`), so it can be tested without one.

pub mod app;
pub mod archive;
pub mod commands;
pub mod create;
pub mod extract;
pub mod hash;
pub mod i18n;
pub mod settings;
pub mod storage;
pub mod tree;
pub mod ui;

// Sizes and dates as shown, shared with rsize and rdup.
pub use rt_scan::format;
