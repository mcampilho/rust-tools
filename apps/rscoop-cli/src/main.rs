//! rscoop-cli: validates a Scoop installation, finds what still points to an old
//! Scoop root and, with --fix, repairs it. A Rust port of scoop-check, with a
//! SQLite cache of the files already read.

// Scoop is Windows only: elsewhere the program just says so.
#![cfg_attr(not(windows), allow(dead_code))]

mod cache;
mod checks;
mod cli;
mod fix;
mod lnk;
mod paths;
mod registry;
mod report;
mod sys;
#[cfg(test)]
mod tests;
mod text;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run_here(&args);
    std::process::exit(code);
}

#[cfg(windows)]
fn run_here(args: &[String]) -> i32 {
    let environ = std::env::vars().collect();
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let mut out = std::io::stdout().lock();
    let mut err = std::io::stderr();
    cli::run(args, &registry::WinRegistry, environ, &home, &mut out, &mut err, None)
}

#[cfg(not(windows))]
fn run_here(_args: &[String]) -> i32 {
    let t = text::T::new(&text::system_language());
    eprintln!("{}", t.get("cli.not_windows"));
    2
}
