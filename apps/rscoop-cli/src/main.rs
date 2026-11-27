//! rscoop-cli: Scoop, faster. The commands that only read are done here
//! (`cmd`, over the `rt-scoop` crate) and the others are handed to Scoop.
//! Without a command it validates the installation, finds what still points to
//! an old Scoop root and, with --fix, repairs it (a Rust port of scoop-check).

// Scoop is Windows only: elsewhere the program just says so.
#![cfg_attr(not(windows), allow(dead_code))]

mod cache;
mod checks;
mod cli;
mod cmd;
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
    let environ: std::collections::HashMap<String, String> = std::env::vars().collect();
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    // Buffered: some commands write thousands of lines. Flushed before
    // anything else writes to the console.
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    let mut err = std::io::stderr();
    let code = match cmd::command_line(args) {
        Some(line) => {
            let outside = cmd::Outside {
                vars: environ,
                exe: std::env::current_exe().ok(),
                system_arch: sys::system_arch(),
                registry: &registry::WinRegistry,
            };
            cmd::run(line, outside, &mut out, &mut err)
        }
        None => cli::run(args, &registry::WinRegistry, environ, &home, &mut out, &mut err, None),
    };
    let _ = std::io::Write::flush(&mut out);
    code
}

#[cfg(not(windows))]
fn run_here(_args: &[String]) -> i32 {
    let t = text::texts(&text::system_language());
    eprintln!("{}", t.get("cli.not_windows"));
    2
}
