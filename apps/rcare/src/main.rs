//! rcare: Windows maintenance in one window (see `lib.rs`).

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod shell;

#[cfg(windows)]
fn main() {
    shell::run();
}

/// rcare maintains Windows: elsewhere the program just says so.
#[cfg(not(windows))]
fn main() {
    eprintln!("rcare: this program is for Windows only.");
    std::process::exit(1);
}
