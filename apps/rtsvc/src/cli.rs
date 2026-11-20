//! The command line of rtsvc.

use rt_cli::Texts;
use rt_svc::Visibility;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Help,
    Version,
    Install,
    Uninstall,
    Status,
    Console,
    /// What Windows starts.
    Run,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Opts {
    pub command: Command,
    pub dir: Option<String>,
    pub visibility: Visibility,
    pub yes: bool,
    pub purge: bool,
    pub lang: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    UnknownCommand(String),
    UnknownOption(String),
    Missing(String),
    BadValue { opt: String, value: String },
}

impl Error {
    pub fn describe(&self, t: &Texts) -> String {
        match self {
            Error::UnknownCommand(c) => t.fill("err.unknown_command", &[("cmd", c)]),
            Error::UnknownOption(o) => t.fill("err.unknown_option", &[("opt", o)]),
            Error::Missing(o) => t.fill("err.missing", &[("opt", o)]),
            Error::BadValue { opt, value } => t.fill("err.bad_value", &[("opt", opt), ("value", value)]),
        }
    }
}

/// The language asked for with `--lang`, found before the rest is read so that errors come in it.
pub fn lang_asked(args: &[String]) -> Option<String> {
    let at = args.iter().position(|a| a == "--lang" || a.starts_with("--lang="))?;
    match args[at].split_once('=') {
        Some((_, v)) => Some(v.to_string()),
        None => args.get(at + 1).cloned(),
    }
}

pub fn parse(args: &[String]) -> Result<Opts, Error> {
    let mut o = Opts { command: Command::Help, dir: None, visibility: Visibility::User, yes: false, purge: false, lang: None };
    let mut command = None;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        let (name, mut inline) = match a.strip_prefix("--").map(|l| l.split_once('=')) {
            Some(Some((n, v))) => (format!("--{n}"), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let mut value = |i: &mut usize| -> Result<String, Error> {
            if let Some(v) = inline.take() {
                return Ok(v);
            }
            let v = args.get(*i).ok_or_else(|| Error::Missing(name.clone()))?;
            *i += 1;
            Ok(v.clone())
        };
        match name.as_str() {
            "--help" | "-h" | "/?" => command = Some(Command::Help),
            "--version" => command = Some(Command::Version),
            "--yes" | "-y" => o.yes = true,
            "--purge" => o.purge = true,
            "--dir" => o.dir = Some(value(&mut i)?),
            "--lang" => o.lang = Some(value(&mut i)?),
            "--visibility" => {
                let v = value(&mut i)?;
                o.visibility = match v.as_str() {
                    "user" => Visibility::User,
                    "elevated" => Visibility::Elevated,
                    _ => return Err(Error::BadValue { opt: name.clone(), value: v }),
                };
            }
            "install" => command = Some(Command::Install),
            "uninstall" => command = Some(Command::Uninstall),
            "status" => command = Some(Command::Status),
            "console" => command = Some(Command::Console),
            "run" => command = Some(Command::Run),
            n if n.starts_with('-') => return Err(Error::UnknownOption(n.to_string())),
            n => return Err(Error::UnknownCommand(n.to_string())),
        }
    }
    o.command = command.unwrap_or(Command::Help);
    Ok(o)
}

/// The text of `--help`.
pub fn help(t: &Texts, prog: &str) -> String {
    let commands = [
        ("install", "help.install"),
        ("uninstall", "help.uninstall"),
        ("status", "help.status"),
        ("console", "help.console"),
        ("run", "help.run"),
    ];
    let options = [
        ("--dir FOLDER", "help.opt_dir"),
        ("--visibility user|elevated", "help.opt_visibility"),
        ("--yes", "help.opt_yes"),
        ("--purge", "help.opt_purge"),
        ("--lang en|pt|es", "help.opt_lang"),
    ];
    let mut s = format!(
        "{}: {prog} <{}> [{}]\n\n{}\n\n{}:\n",
        t.get("help.usage"),
        t.get("help.commands"),
        t.get("help.options"),
        t.get("help.description"),
        t.get("help.commands")
    );
    for (c, k) in commands {
        s.push_str(&format!("  {c:<28}{}\n", t.get(k)));
    }
    s.push_str(&format!("\n{}:\n", t.get("help.options")));
    for (o, k) in options {
        s.push_str(&format!("  {o:<28}{}\n", t.get(k)));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn commands_and_options() {
        let o = parse(&args("install --dir C:\\Programs --visibility=elevated --yes")).unwrap();
        assert_eq!(o.command, Command::Install);
        assert_eq!(o.dir.as_deref(), Some("C:\\Programs"));
        assert!(o.yes && o.visibility == Visibility::Elevated);
        assert_eq!(parse(&args("")).unwrap().command, Command::Help);
        assert_eq!(parse(&args("status")).unwrap().command, Command::Status);
        assert!(parse(&args("uninstall --purge")).unwrap().purge);
    }

    #[test]
    fn mistakes_are_named() {
        assert_eq!(parse(&args("frobnicate")), Err(Error::UnknownCommand("frobnicate".into())));
        assert_eq!(parse(&args("--nope")), Err(Error::UnknownOption("--nope".into())));
        assert_eq!(parse(&args("install --dir")), Err(Error::Missing("--dir".into())));
        assert!(matches!(parse(&args("--visibility everyone")), Err(Error::BadValue { .. })));
    }

    #[test]
    fn the_table_of_texts_is_complete_and_help_comes_in_three_languages() {
        let table = include_str!("strings.txt");
        assert_eq!(rt_cli::table_problems(table), Vec::<String>::new());
        for lang in ["en", "pt", "es"] {
            let t = Texts::new(table, lang);
            let h = help(&t, "rtsvc");
            assert!(h.contains("--visibility user|elevated") && h.contains(t.get("help.install")), "{lang}");
        }
    }
}
