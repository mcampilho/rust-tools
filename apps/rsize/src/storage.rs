//! Local persistence in a small `key=value` text file (see `rt-core`).
//!
//! Location: next to the executable when a `rsize.portable` marker is
//! there; otherwise `%APPDATA%\rsize` on Windows or
//! `$XDG_CONFIG_HOME/rsize` (`~/.config/rsize`) elsewhere.

pub use rt_core::store::{Location, Place, Store};

/// Where rsize keeps its settings.
pub const PLACE: Place = Place { app: "rsize", file: "settings.ini", file_marks_portable: false };

/// The data folder: portable, or the user's.
pub fn detect() -> Location {
    Location::detect(PLACE)
}
