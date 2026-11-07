//! Local persistence in a small `key=value` text file (see `rt-core`).
//!
//! Location: next to the executable when a `rpad.portable` marker is
//! there; otherwise `%APPDATA%\rpad` on Windows or
//! `$XDG_CONFIG_HOME/rpad` (`~/.config/rpad`) elsewhere.

pub use rt_core::store::{Location, Place, Store};

pub const FILE_NAME: &str = "settings.ini";

/// Where rpad keeps its settings.
pub const PLACE: Place = Place { app: "rpad", file: FILE_NAME, file_marks_portable: false };

/// The data folder: portable, or the user's.
pub fn detect() -> Location {
    Location::detect(PLACE)
}
