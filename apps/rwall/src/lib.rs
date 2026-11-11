//! rwall: a portable wallpaper changer that stays in the notification area.
//! The library holds everything but the window (`main.rs`), so it can be
//! tested without one.

pub mod autostart;
pub mod changer;
pub mod commands;
pub mod i18n;
pub mod json;
pub mod net;
pub mod picture;
pub mod schedule;
pub mod settings;
pub mod sources;
pub mod storage;
pub mod ui;
pub mod wallpaper;

