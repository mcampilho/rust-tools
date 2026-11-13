//! Local persistence in a small `key=value` text file (see `rt-core`).
//!
//! Location: next to the executable when a `runinst.portable` marker is
//! there; otherwise `%APPDATA%\runinst` on Windows or
//! `$XDG_CONFIG_HOME/runinst` (`~/.config/runinst`) elsewhere.

pub use rt_core::store::{Location, Place, Store};

/// Where runinst keeps its settings.
pub const PLACE: Place = Place { app: "runinst", file: "settings.ini", file_marks_portable: false };

/// The data folder: portable, or the user's.
pub fn detect() -> Location {
    Location::detect(PLACE)
}
