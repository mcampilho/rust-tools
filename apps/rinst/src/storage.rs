//! Local persistence in a small `key=value` text file (see `rt-core`).
//!
//! Location: next to the executable when a `rinst.portable` marker is
//! there; otherwise `%APPDATA%\rinst` on Windows or
//! `$XDG_CONFIG_HOME/rinst` (`~/.config/rinst`) elsewhere.
//!
//! rinst was called runinst until 2026-11-17: the old marker and settings
//! folder take the new name the first time it starts.

pub use rt_core::store::{Location, Place, Store};

/// Where rinst keeps its settings.
pub const PLACE: Place = Place { app: "rinst", file: "settings.ini", file_marks_portable: false };

/// The name rinst had before.
pub const OLD_NAME: &str = "runinst";

/// The data folder: portable, or the user's (taken over from the old name).
pub fn detect() -> Location {
    Location::detect_renamed(PLACE, OLD_NAME)
}
