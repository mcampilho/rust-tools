//! Local persistence in a small `key=value` text file (see `rt-core`).
//!
//! Location: next to the executable when a `rpack.portable` marker is
//! there; otherwise `%APPDATA%\rpack` on Windows or
//! `$XDG_CONFIG_HOME/rpack` (`~/.config/rpack`) elsewhere.

pub use rt_core::store::{Location, Place, Store};

/// Where rpack keeps its settings.
pub const PLACE: Place = Place { app: "rpack", file: "settings.ini", file_marks_portable: false };

/// The data folder: portable, or the user's.
pub fn detect() -> Location {
    Location::detect(PLACE)
}
