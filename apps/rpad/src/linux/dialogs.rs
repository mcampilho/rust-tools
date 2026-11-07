//! Native dialogs through zenity or kdialog (see `rt-sys`), with rpad's
//! title and answers.

use crate::app::Answer;
use rt_sys::linux::dialogs as d;
use std::path::{Path, PathBuf};

pub use d::{available, choose_font, confirm, error, info, pick_folder, save_file};

pub fn ask_save(text: &str) -> Answer {
    match d::ask_save("rpad", text) {
        d::Answer::Yes => Answer::Yes,
        d::Answer::No => Answer::No,
        d::Answer::Cancel => Answer::Cancel,
    }
}

pub fn open_files(dir: Option<&Path>) -> Vec<PathBuf> {
    d::open_files(dir, &[], true)
}
