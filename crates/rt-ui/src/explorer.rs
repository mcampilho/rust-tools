//! "Analyze with rsize", "Find duplicates with rdup"…: the Explorer menu of
//! folders and drives, for the current user only.

use rt_sys::win::assoc::{self, Keys, Status};

/// The verb is on the menu of folders, and it starts this executable.
pub fn folders_here(app: &str) -> bool {
    let exe = assoc::current_exe().unwrap_or_default();
    assoc::folder_status(app, &Keys::user(app), &exe) == Status::Here
}

/// Texts of [`set_folders`], already translated.
pub struct FolderTexts {
    /// What the menu shows ("Analyze with rsize").
    pub verb: String,
    pub added: String,
    pub removed: String,
    /// With "{}" for the error number.
    pub failed: String,
}

/// Adds the verb to the Explorer menu of folders and drives, or takes it
/// out; returns the message to show.
pub fn set_folders(app: &str, on: bool, t: &FolderTexts) -> Result<String, String> {
    let keys = Keys::user(app);
    let failed = |e: assoc::Error| t.failed.replace("{}", &e.to_string());
    if on {
        let exe = assoc::current_exe()?;
        assoc::register_folders(app, &keys, &exe, &t.verb).map_err(failed)?;
        Ok(t.added.clone())
    } else {
        assoc::unregister_folders(app, &keys).map_err(failed)?;
        Ok(t.removed.clone())
    }
}
