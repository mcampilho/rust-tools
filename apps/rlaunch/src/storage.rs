//! Local persistence in a small `key=value` text file (see `rt-core`).
//!
//! Location: next to the executable when a `rlaunch.portable` marker is
//! there (a pen drive); otherwise `%APPDATA%\rlaunch` on Windows or
//! `$XDG_CONFIG_HOME/rlaunch` (`~/.config/rlaunch`) elsewhere.

pub use rt_core::store::{Location, Place, Store};

/// Where rlaunch keeps its settings.
pub const PLACE: Place = Place { app: "rlaunch", file: "settings.ini", file_marks_portable: false };

/// The data folder: portable, or the user's.
pub fn detect() -> Location {
    Location::detect(PLACE)
}
