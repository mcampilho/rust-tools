//! rsize: a portable disk space analyzer. The library holds everything but
//! the window (`main.rs`), so it can be tested without one.

pub mod app;
pub mod chart;
pub mod commands;
pub mod export;
pub mod format;
pub mod i18n;
pub mod model;
pub mod scan;
pub mod settings;
pub mod storage;
pub mod treemap;
pub mod ui;
pub mod volumes;
