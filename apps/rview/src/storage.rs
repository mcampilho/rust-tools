//! Local persistence in a small `key=value` text file (see `rt-core`).
//!
//! Location: next to the executable when a `rview.portable` marker is
//! there; otherwise `%APPDATA%\rview` on Windows or
//! `$XDG_CONFIG_HOME/rview` (`~/.config/rview`) elsewhere.

pub use rt_core::store::{Location, Place, Store};

/// Where rview keeps its settings.
pub const PLACE: Place = Place { app: "rview", file: "settings.ini", file_marks_portable: false };

/// The data folder: portable, or the user's.
pub fn detect() -> Location {
    Location::detect(PLACE)
}
