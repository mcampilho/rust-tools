//! rtsvc: the rust-tools service (see `docs/service.md`). It lets the tools
//! read system-wide information without being started as administrator, and
//! does in the background what a tool started as administrator asks.
//!
//! `rtsvc install` and `uninstall` need an elevated prompt; `status` and
//! `console` do not. Windows only.

#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

mod cli;
#[cfg(windows)]
mod config;
#[cfg(windows)]
mod handler;
#[cfg(windows)]
mod install;
#[cfg(windows)]
mod scm;
#[cfg(windows)]
mod service;

use cli::{Command, Opts};
use rt_cli::Texts;
use std::io::Write;

const PROG: &str = "rtsvc";
const TABLE: &str = include_str!("strings.txt");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(&args, &mut std::io::stdin().lock(), &mut std::io::stdout().lock(), &mut std::io::stderr().lock());
    std::process::exit(code);
}

/// Runs a command. Returns the exit status: 0 done, 1 not done or not answering, 2 wrong usage.
fn run(args: &[String], input: &mut dyn std::io::BufRead, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let lang = cli::lang_asked(args).and_then(|l| rt_cli::pick(&l)).unwrap_or_else(|| rt_cli::language("RTSVC_LANG"));
    let t = Texts::new(TABLE, &lang);
    let o = match cli::parse(args) {
        Ok(o) => o,
        Err(e) => {
            let _ = writeln!(err, "{PROG}: {}\n{}", e.describe(&t), t.fill("err.try", &[("prog", PROG)]));
            return 2;
        }
    };
    match o.command {
        Command::Help => {
            let _ = write!(out, "{}", cli::help(&t, PROG));
            0
        }
        Command::Version => {
            let _ = writeln!(out, "{PROG} {}", env!("CARGO_PKG_VERSION"));
            0
        }
        cmd => dispatch(cmd, &o, &t, input, out, err),
    }
}

#[cfg(not(windows))]
fn dispatch(_cmd: Command, _o: &Opts, t: &Texts, _input: &mut dyn std::io::BufRead, _out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let _ = writeln!(err, "{PROG}: {}", t.get("err.not_windows"));
    2
}

#[cfg(windows)]
fn dispatch(cmd: Command, o: &Opts, t: &Texts, input: &mut dyn std::io::BufRead, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match cmd {
        Command::Install => {
            let opts = install::InstallOpts { dir: o.dir.clone().map(Into::into), visibility: o.visibility, yes: o.yes };
            install::install(&opts, t, input, out, err)
        }
        Command::Uninstall => install::uninstall(o.purge, t, out, err),
        Command::Status => install::status(t, out),
        Command::Run => service::run_service(),
        Command::Console => {
            let _ = writeln!(out, "{}", t.fill("msg.console", &[("pipe", rt_svc::DEV_PIPE_NAME)]));
            match service::console(o.visibility, service::console_logger()) {
                Ok(()) => 0,
                Err(e) => {
                    let _ = writeln!(err, "{PROG}: {e}");
                    1
                }
            }
        }
        Command::Help | Command::Version => 0,
    }
}
