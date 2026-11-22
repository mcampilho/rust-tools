//! rcare: Windows maintenance in one window. The library holds everything
//! but the window (`main.rs`, `shell.rs`), so it can be tested without one.
//! The pages are the ones of the small apps (rclean, …): each app's library
//! holds its page, and the small app and rcare both use it.

pub mod appicon;
pub mod commands;
pub mod custom;
pub mod disks;
pub mod i18n;
pub mod maint;
pub mod pages;
pub mod settings;
pub mod sidebar;
pub mod startup;
pub mod startup_page;
pub mod storage;
