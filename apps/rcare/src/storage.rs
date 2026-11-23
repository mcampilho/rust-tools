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

/// Where rcare keeps its files beside the settings (copies, schedules, logs):
/// the settings' folder, or the program's when there is none.
pub fn data_dir(location: &Location) -> std::path::PathBuf {
    location
        .dir
        .clone()
        .or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())))
        .unwrap_or_else(std::env::temp_dir)
}
