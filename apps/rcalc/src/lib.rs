//! rcalc library: calculation engine, conversions, persistence and the
//! software-rendered UI. The binary (`main.rs`) only hosts the window.

pub mod app;
pub mod calc;
pub mod convert;
pub mod date;
pub mod expr;
pub mod format;
pub mod i18n;
pub mod num;
pub mod platform;
pub mod storage;
pub mod ui;
#[cfg(windows)]
pub mod winshell;
