//! Local persistence in a small `key=value` text file (see `rt-core`).
//!
//! Location: next to the executable when a `rcalc.portable` marker (or an
//! existing `rcalc.ini`) is there; otherwise `%APPDATA%\rcalc` on Windows or
//! `$XDG_CONFIG_HOME/rcalc` (`~/.config/rcalc`) elsewhere. If the location is
//! not writable the app keeps working and reports that nothing is saved.

pub use rt_core::store::{Location, Place, Store};

pub const FILE_NAME: &str = "rcalc.ini";
pub const PORTABLE_MARKER: &str = "rcalc.portable";
pub const CURRENCY_FILE: &str = "currency.txt";

/// Where rcalc keeps its settings and history.
pub const PLACE: Place = Place { app: "rcalc", file: FILE_NAME, file_marks_portable: true };

/// The data folder: portable, or the user's.
pub fn detect() -> Location {
    Location::detect(PLACE)
}
