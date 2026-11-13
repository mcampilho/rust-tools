//! Local persistence in a small `key=value` text file (see `rt-core`).
//!
//! Location: next to the executable when a `rclean.portable` marker is
//! there; otherwise `%APPDATA%\rclean` on Windows or
//! `$XDG_CONFIG_HOME/rclean` (`~/.config/rclean`) elsewhere.

pub use rt_core::store::{Location, Place, Store};

/// Where rclean keeps its settings.
pub const PLACE: Place = Place { app: "rclean", file: "settings.ini", file_marks_portable: false };

/// The data folder: portable, or the user's.
pub fn detect() -> Location {
    Location::detect(PLACE)
}
