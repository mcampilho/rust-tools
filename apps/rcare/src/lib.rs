//! rcare: Windows maintenance in one window. The library holds everything
//! but the window (`main.rs`, `shell.rs`), so it can be tested without one.
//! The pages are the ones of the small apps (rclean, …): each app's library
//! holds its page, and the small app and rcare both use it.

pub mod appicon;
pub mod bg;
pub mod commands;
pub mod custom;
pub mod disks;
pub mod extensions;
pub mod extensions_page;
pub mod i18n;
pub mod maint;
pub mod net;
pub mod net_page;
pub mod pages;
pub mod privacy;
pub mod privacy_page;
pub mod protect;
pub mod protect_page;
pub mod ps;
pub mod registry_page;
pub mod repair;
pub mod repair_page;
pub mod services;
pub mod services_page;
pub mod settings;
pub mod sidebar;
pub mod speed;
pub mod speed_page;
pub mod start;
pub mod startup;
pub mod startup_page;
pub mod storage;
pub mod sysinfo;
pub mod sysinfo_page;
