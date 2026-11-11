//! Local persistence in a small `key=value` text file (see `rt-core`), and
//! the folder of downloaded pictures beside it.
//!
//! Location: next to the executable when a `rwall.portable` marker is
//! there; otherwise `%APPDATA%\rwall` on Windows or
//! `$XDG_CONFIG_HOME/rwall` (`~/.config/rwall`) elsewhere.

pub use rt_core::store::{Location, Place, Store};
use std::path::PathBuf;

/// Where rwall keeps its settings.
pub const PLACE: Place = Place { app: "rwall", file: "settings.ini", file_marks_portable: false };

/// The data folder: portable, or the user's.
pub fn detect() -> Location {
    Location::detect(PLACE)
}

/// The folder of downloaded (and converted) pictures.
pub fn images_dir(location: &Location) -> Option<PathBuf> {
    location.file("images")
}
