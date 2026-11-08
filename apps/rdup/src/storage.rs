//! Local persistence in a small `key=value` text file (see `rt-core`).
//!
//! Location: next to the executable when a `rdup.portable` marker is
//! there; otherwise `%APPDATA%\rdup` on Windows or
//! `$XDG_CONFIG_HOME/rdup` (`~/.config/rdup`) elsewhere.

pub use rt_core::store::{Location, Place, Store};

/// Where rdup keeps its settings.
pub const PLACE: Place = Place { app: "rdup", file: "settings.ini", file_marks_portable: false };

/// The data folder: portable, or the user's.
pub fn detect() -> Location {
    Location::detect(PLACE)
}
