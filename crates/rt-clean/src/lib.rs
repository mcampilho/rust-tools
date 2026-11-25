//! What the cleaner cleans, without a window: the targets, measuring them
//! and removing them. rclean shows it; the privileged service measures it in
//! the background.

pub mod clean;
#[cfg(windows)]
pub mod in_place;
pub mod managers;
pub mod registry;
pub mod shortcuts;
pub mod snapshot;
pub mod targets;
