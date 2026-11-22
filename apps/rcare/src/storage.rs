//! Local persistence in a small `key=value` text file (see `rt-core`).
//!
//! Location: next to the executable when a `rcare.portable` marker is
//! there; otherwise `%APPDATA%\rcare`.

pub use rt_core::store::{Location, Place, Store};

/// Where rcare keeps its settings.
pub const PLACE: Place = Place { app: "rcare", file: "settings.ini", file_marks_portable: false };

/// The data folder: portable, or the user's.
pub fn detect() -> Location {
    Location::detect(PLACE)
}
