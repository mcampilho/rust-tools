//! The commands of Scoop that rscoop-cli does itself: those that read, and
//! the upkeep of the installation (update of Scoop and buckets, downloads,
//! cleanup, hold, configuration), and `shim`, `install`, `update <app>`,
//! `import`, `reset` and `uninstall` (these five only with RSCOOP_NATIVE=1 for now).
//! The rest is handed to the Scoop of the installation (`delegate`).

mod args;
mod bucket;
mod checkup;
mod delegate;
mod download;
mod git;
mod import;
mod install;
mod listing;
mod machinery;
mod maintain;
mod query;
mod reset;
mod shim;
mod state;
pub mod table;
#[cfg(all(test, windows))]
mod tests;
#[cfg(all(test, windows))]
mod tests_ops;
#[cfg(all(test, windows))]
mod tests_write;
mod uninstall;
mod update;
mod update_apps;

use crate::registry::Registry;
use crate::text::{system_language, T};
use rt_scoop::install::{core_root, Env, Scoop};
use rt_scoop::json::{self, Json};
use rt_scoop::manifest::Arch;
use rt_scoop::system::System;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

/// The commands of this program: name, usage, key of the description.
pub const NATIVE: [(&str, &str, &str); 27] = [
    ("list", "[query]", "cmd.list"),
    ("search", "[query]", "cmd.search"),
    ("info", "<app> [-v]", "cmd.info"),
    ("cat", "<app>", "cmd.cat"),
    ("status", "[--local]", "cmd.status"),
    ("update", "[app…|*] [-gfiksqa]", "cmd.update"),
    ("which", "<command>", "cmd.which"),
    ("prefix", "<app>", "cmd.prefix"),
    ("depends", "<app> [-a arch]", "cmd.depends"),
    ("home", "<app>", "cmd.home"),
    ("bucket", "list|known|add|rm", "cmd.bucket"),
    ("cache", "show|rm [app…]", "cmd.cache"),
    ("config", "[name [value]] | rm name", "cmd.config"),
    ("download", "<app…>", "cmd.download"),
    ("cleanup", "<app…>|* [-gk]", "cmd.cleanup"),
    ("hold", "<app…> [-g]", "cmd.hold"),
    ("unhold", "<app…> [-g]", "cmd.unhold"),
    ("alias", "add|rm|list", "cmd.alias"),
    ("shim", "add|rm|list|info [-g]", "cmd.shim"),
    ("install", "<app…> [-giksu] [-a arch]", "cmd.install"),
    ("import", "<scoopfile.json>", "cmd.import"),
    ("reset", "<app…>|*", "cmd.reset"),
    ("uninstall", "<app…> [-gp]", "cmd.uninstall"),
    ("export", "[-c]", "cmd.export"),
    ("checkup", "", "cmd.checkup"),
    ("help", "[command]", "cmd.help"),
    ("scoop", "<command…>", "cmd.scoop"),
];

pub struct Ctx<'a> {
    pub scoop: Scoop,
    pub env: Env,
    pub t: T,
    /// `--lang`, for the checker of `checkup`.
    pub lang: Option<String>,
    pub json: bool,
    /// `--root` was given: Scoop, when it takes over, works on that one.
    pub root_given: bool,
    pub system_arch: Arch,
    /// The registry, the Start menu and the processes.
    pub system: &'a dyn System,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

impl Ctx<'_> {
    /// One line of output (a closed pipe is not an error).
    pub fn say(&mut self, line: impl AsRef<str>) {
        let _ = writeln!(self.out, "{}", line.as_ref());
    }

    pub fn print_json(&mut self, value: &Json) {
        let text = json::pretty(value);
        self.say(text);
    }

    /// A message on stderr; returns the exit code 1.
    pub fn fail(&mut self, key: &str, args: &[(&str, &str)]) -> i32 {
        let text = self.t.fill(key, args);
        let _ = writeln!(self.err, "{text}");
        1
    }

    pub fn warn(&mut self, key: &str, args: &[(&str, &str)]) {
        let _ = self.fail(key, args);
    }

    pub fn arch(&self) -> Arch {
        self.scoop.default_arch(self.system_arch)
    }

    /// `~\…` for a path under the user's folder (`friendly_path`).
    pub fn friendly(&self, path: &std::path::Path) -> String {
        match path.strip_prefix(&self.env.home) {
            Ok(rest) if !self.env.home.as_os_str().is_empty() => format!("~\\{}", rest.display()),
            _ => path.display().to_string(),
        }
    }
}

/// What the command line asks for, before the command itself.
#[derive(Debug, Default, PartialEq)]
pub struct Line {
    pub root: Option<String>,
    pub lang: Option<String>,
    pub json: bool,
    pub command: String,
    pub args: Vec<String>,
}

/// The command and its arguments, or None when the line is for the checker
/// (no command: the options of rscoop-cli before it had commands).
pub fn command_line(args: &[String]) -> Option<Line> {
    let mut line = Line::default();
    let mut i = 0;
    while i < args.len() {
        let (name, inline) = match args[i].split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (args[i].as_str(), None),
        };
        match name {
            "--root" | "--lang" => {
                let value = inline.or_else(|| {
                    i += 1;
                    args.get(i).cloned()
                })?;
                if name == "--root" {
                    line.root = Some(value);
                } else {
                    line.lang = Some(value);
                }
            }
            "--json" => line.json = true,
            n if n.starts_with('-') => return None,
            n => {
                line.command = n.to_ascii_lowercase();
                line.args = args[i + 1..].to_vec();
                return Some(line);
            }
        }
        i += 1;
    }
    None
}

/// Takes `--json`, `--root` and `--lang` out of the arguments of a command
/// done here, wherever they are.
fn own_options(line: &mut Line) {
    let mut rest = Vec::new();
    let mut args = std::mem::take(&mut line.args).into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => line.json = true,
            "--root" => line.root = args.next().or(line.root.take()),
            "--lang" => line.lang = args.next().or(line.lang.take()),
            _ => rest.push(arg),
        }
    }
    line.args = rest;
}

/// Does rscoop-cli do this command with these arguments itself?
pub fn is_native(command: &str, args: &[String]) -> bool {
    let first = args.first().map(String::as_str);
    match command {
        "bucket" => matches!(first, Some("list" | "known" | "add" | "rm")),
        "config" => first != Some("--help"),
        "scoop" => false,
        // Scoop shows its own help; lter asks the user to choose.
        _ if matches!(first, Some("-h" | "--help" | "/?")) => false,
        "shim" => first != Some("alter"),
        // Uninstalling Scoop itself is Scoop's.
        "uninstall" => !args.iter().any(|a| a.eq_ignore_ascii_case("scoop")),
        other => NATIVE.iter().any(|(name, _, _)| *name == other),
    }
}

/// What a command needs from the outside.
pub struct Outside<'a> {
    pub vars: HashMap<String, String>,
    pub exe: Option<PathBuf>,
    pub system_arch: Arch,
    pub registry: &'a dyn Registry,
    pub system: &'a dyn System,
}

/// Runs a command; returns the exit code.
pub fn run(mut line: Line, outside: Outside, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let env = Env::new(outside.vars.clone());
    // What removes apps stays Scoop's until it has been compared with Scoop
    // in a throw-away Windows (docs/rscoop.pt.md §7); RSCOOP_NATIVE=1 tries it.
    let writes_apps = matches!(line.command.as_str(), "install" | "import" | "reset" | "uninstall")
        || (line.command == "update" && update_apps::updates_apps(&line.args));
    let on_trial = writes_apps && env.get("RSCOOP_NATIVE") != Some("1");
    let native = is_native(&line.command, &line.args) && !on_trial;
    if native {
        own_options(&mut line);
    }
    let t = crate::text::texts(&line.lang.clone().unwrap_or_else(system_language));
    let located = core_root(&env, outside.exe.as_deref());
    let scoop = Scoop::locate(&env, line.root.as_deref(), located);
    let root_given = line.root.is_some();
    let mut ctx = Ctx {
        scoop,
        env,
        t,
        lang: line.lang.clone(),
        json: line.json,
        root_given,
        system_arch: outside.system_arch,
        system: outside.system,
        out,
        err,
    };
    if !native {
        let mut args = line.args.clone();
        if line.command != "scoop" {
            args.insert(0, line.command.clone());
        }
        return delegate::run(&mut ctx, &args, root_given);
    }
    let args = &line.args;
    match line.command.as_str() {
        "list" => listing::list(&mut ctx, args),
        "search" => listing::search(&mut ctx, args),
        "export" => listing::export(&mut ctx, args),
        "info" => query::info(&mut ctx, args),
        "cat" => query::cat(&mut ctx, args),
        "which" => query::which(&mut ctx, args),
        "prefix" => query::prefix(&mut ctx, args),
        "depends" => query::depends(&mut ctx, args),
        "home" => query::home(&mut ctx, args),
        "status" => state::status(&mut ctx, args),
        "update" if update_apps::updates_apps(args) => update_apps::run(&mut ctx, args),
        "update" => update::sync(&mut ctx, args),
        "bucket" => match args.first().map(String::as_str) {
            Some("add") => bucket::add(&mut ctx, &args[1..]),
            Some("rm") => bucket::rm(&mut ctx, &args[1..]),
            _ => state::bucket(&mut ctx, args),
        },
        "cache" => match args.first().map(String::as_str) {
            Some("rm") => maintain::cache_rm(&mut ctx, &args[1..]),
            _ => state::cache(&mut ctx, args),
        },
        "config" => match args.as_slice() {
            [] => state::config(&mut ctx, args),
            [name] if name != "rm" => state::config(&mut ctx, args),
            _ => maintain::config_write(&mut ctx, args),
        },
        "download" => download::run(&mut ctx, args),
        "cleanup" => maintain::cleanup(&mut ctx, args),
        "hold" => maintain::hold(&mut ctx, args, true),
        "unhold" => maintain::hold(&mut ctx, args, false),
        "alias" => maintain::alias(&mut ctx, args),
        "shim" => shim::run(&mut ctx, args),
        "install" => install::run(&mut ctx, args),
        "import" => import::run(&mut ctx, args),
        "reset" => reset::run(&mut ctx, args),
        "uninstall" => uninstall::run(&mut ctx, args),
        "checkup" => checkup::run(&mut ctx, args, &outside),
        _ => help(&mut ctx, args),
    }
}

fn help(ctx: &mut Ctx, args: &[String]) -> i32 {
    if let Some(command) = args.first() {
        return match NATIVE.iter().find(|(name, _, _)| name == command) {
            Some((name, usage, key)) => {
                let text = format!("{}: rscoop-cli {name} {usage}\n\n{}", ctx.t.get("cli.usage"), ctx.t.get(key));
                ctx.say(text);
                0
            }
            // Scoop's own help for what Scoop does.
            None => delegate::run(ctx, &["help".to_string(), command.clone()], false),
        };
    }
    let mut text =
        format!("{}: rscoop-cli <{}> [--json] [--root PATH] [--lang LANG]\n\n", ctx.t.get("cli.usage"), ctx.t.get("cli.command"));
    for (name, usage, key) in NATIVE {
        text += &format!("  {:<26} {}\n", format!("{name} {usage}"), ctx.t.get(key));
    }
    text += &format!("\n{}\n\n{}", ctx.t.get("cli.others"), ctx.t.get("cli.checker"));
    ctx.say(text);
    0
}
