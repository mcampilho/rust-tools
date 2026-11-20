//! rclean: a portable cleaner. The library holds everything but the window
//! (`main.rs`), so it can be tested without one.

pub mod app;
pub mod commands;
pub mod i18n;
pub mod procs;
pub mod settings;
pub mod storage;
pub mod ui;

// What is cleaned: the engine, shared with the privileged service.
pub use rt_clean::{clean, managers, registry, shortcuts, targets};

// Sizes and counts as shown, shared with rsize, rdup and rinst.
pub use rt_scan::format;
