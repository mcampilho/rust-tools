//! The command line: the options of `du` that matter here, and a few more.

use rt_cli::Texts;

/// How sizes are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Bytes,
    /// Whole KiB, rounded up (the default, as in `du`).
    Kib,
    Mib,
    /// `1.5K`, `23M`, steps of 1024.
    Human,
    /// `1.5k`, `23M`, steps of 1000.
    Si,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    Size,
    Name,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
    Csv,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Opts {
    pub paths: Vec<String>,
    pub unit: Unit,
    pub summarize: bool,
    pub all: bool,
    pub total: bool,
    pub max_depth: Option<usize>,
    /// The length of the files, not the space they take on the disk.
    pub apparent: bool,
    pub order: Order,
    pub top: Option<usize>,
    pub format: Format,
    /// Folder by folder only.
    pub no_index: bool,
    pub lang: Option<String>,
    pub help: bool,
    pub version: bool,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            paths: Vec::new(),
            unit: Unit::Kib,
            summarize: false,
            all: false,
            total: false,
            max_depth: None,
            apparent: false,
            order: Order::Size,
            top: None,
            format: Format::Text,
            no_index: false,
            lang: None,
            help: false,
            version: false,
        }
    }
}

impl Opts {
    /// How many levels below each path are listed (`-s` is none).
    pub fn depth(&self) -> Option<usize> {
        if self.summarize {
            Some(0)
        } else {
            self.max_depth
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Unknown(String),
    Missing(String),
    Number { opt: String, value: String },
    Choice { opt: String, value: String, choices: &'static str },
}

impl Error {
    pub fn describe(&self, t: &Texts) -> String {
        match self {
            Error::Unknown(o) => t.fill("err.unknown", &[("opt", o)]),
            Error::Missing(o) => t.fill("err.missing", &[("opt", o)]),
            Error::Number { opt, value } => t.fill("err.number", &[("opt", opt), ("value", value)]),
            Error::Choice { opt, value, choices } => t.fill("err.choice", &[("opt", opt), ("value", value), ("choices", choices)]),
        }
    }
}

/// The value of an option: after `=`, or the next argument.
fn value(inline: Option<String>, args: &[String], i: &mut usize, opt: &str) -> Result<String, Error> {
    if let Some(v) = inline {
        return Ok(v);
    }
    let v = args.get(*i).ok_or_else(|| Error::Missing(opt.to_string()))?;
    *i += 1;
    Ok(v.clone())
}

fn number(v: &str, opt: &str) -> Result<usize, Error> {
    v.parse().map_err(|_| Error::Number { opt: opt.to_string(), value: v.to_string() })
}

/// The language asked for with `--lang`, found before the rest is read so
/// that errors come in it.
pub fn lang_asked(args: &[String]) -> Option<String> {
    let at = args.iter().position(|a| a == "--lang" || a.starts_with("--lang="))?;
    match args[at].split_once('=') {
        Some((_, v)) => Some(v.to_string()),
        None => args.get(at + 1).cloned(),
    }
}

pub fn parse(args: &[String]) -> Result<Opts, Error> {
    let mut o = Opts::default();
    let mut i = 0;
    let mut paths_only = false;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if paths_only || a == "-" || !a.starts_with('-') {
            if a == "/?" {
                o.help = true;
            } else {
                o.paths.push(a.clone());
            }
            continue;
        }
        if a == "--" {
            paths_only = true;
        } else if let Some(long) = a.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let opt = format!("--{name}");
            match name {
                "help" => o.help = true,
                "version" => o.version = true,
                "human-readable" => o.unit = Unit::Human,
                "si" => o.unit = Unit::Si,
                "bytes" => {
                    o.unit = Unit::Bytes;
                    o.apparent = true;
                }
                "summarize" => o.summarize = true,
                "all" => o.all = true,
                "total" => o.total = true,
                "apparent-size" => o.apparent = true,
                "json" => o.format = Format::Json,
                "csv" => o.format = Format::Csv,
                "no-index" => o.no_index = true,
                "max-depth" => o.max_depth = Some(number(&value(inline, args, &mut i, &opt)?, &opt)?),
                "top" => o.top = Some(number(&value(inline, args, &mut i, &opt)?, &opt)?),
                "lang" => o.lang = Some(value(inline, args, &mut i, &opt)?),
                "sort" => {
                    let v = value(inline, args, &mut i, &opt)?;
                    o.order = match v.as_str() {
                        "size" => Order::Size,
                        "name" => Order::Name,
                        _ => return Err(Error::Choice { opt, value: v, choices: "size, name" }),
                    };
                }
                _ => return Err(Error::Unknown(opt)),
            }
        } else {
            // A cluster of short options: -sh, -ad2, -d 2.
            let cluster: Vec<char> = a.chars().skip(1).collect();
            let mut k = 0;
            while k < cluster.len() {
                match cluster[k] {
                    'h' => o.unit = Unit::Human,
                    'b' => {
                        o.unit = Unit::Bytes;
                        o.apparent = true;
                    }
                    'k' => o.unit = Unit::Kib,
                    'm' => o.unit = Unit::Mib,
                    's' => o.summarize = true,
                    'a' => o.all = true,
                    'c' => o.total = true,
                    'd' => {
                        let rest: String = cluster[k + 1..].iter().collect();
                        let v = if rest.is_empty() { value(None, args, &mut i, "-d")? } else { rest };
                        o.max_depth = Some(number(&v, "-d")?);
                        break;
                    }
                    other => return Err(Error::Unknown(format!("-{other}"))),
                }
                k += 1;
            }
        }
    }
    Ok(o)
}

/// The text of `--help`.
pub fn help(t: &Texts, prog: &str) -> String {
    let options: [(&str, &str); 18] = [
        ("-h, --human-readable", "opt.human"),
        ("    --si", "opt.si"),
        ("-b, --bytes", "opt.bytes"),
        ("-k", "opt.kib"),
        ("-m", "opt.mib"),
        ("-s, --summarize", "opt.summarize"),
        ("-a, --all", "opt.all"),
        ("-c, --total", "opt.total"),
        ("-d, --max-depth=N", "opt.depth"),
        ("    --apparent-size", "opt.apparent"),
        ("    --sort=ORDER", "opt.sort"),
        ("    --top=N", "opt.top"),
        ("    --json", "opt.json"),
        ("    --csv", "opt.csv"),
        ("    --no-index", "opt.no_index"),
        ("    --lang=en|pt|es", "opt.lang"),
        ("    --version", "opt.version"),
        ("    --help", "opt.help"),
    ];
    let mut s = format!(
        "{}: {prog} [{}]… [{}]…\n\n{}\n\n{}:\n",
        t.get("help.usage"),
        t.get("help.options"),
        t.get("help.paths"),
        t.get("help.description"),
        t.get("help.options")
    );
    for (flag, key) in options {
        s.push_str(&format!("  {flag:<24}{}\n", t.get(key)));
    }
    s.push_str(&format!("\n{}\n", t.get("help.exit")));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn paths_and_defaults() {
        let o = parse(&args("a b")).unwrap();
        assert_eq!(o.paths, ["a", "b"]);
        assert_eq!(o.unit, Unit::Kib);
        assert_eq!(o.depth(), None);
        assert!(!o.apparent);
    }

    #[test]
    fn short_options_cluster() {
        let o = parse(&args("-sh x")).unwrap();
        assert!(o.summarize && o.unit == Unit::Human);
        assert_eq!(o.depth(), Some(0));
        let o = parse(&args("-ahd2 x")).unwrap();
        assert!(o.all && o.unit == Unit::Human);
        assert_eq!(o.max_depth, Some(2));
        let o = parse(&args("-d 3 -c")).unwrap();
        assert_eq!(o.max_depth, Some(3));
        assert!(o.total && o.paths.is_empty());
    }

    #[test]
    fn long_options_with_values() {
        let o = parse(&args("--max-depth=1 --top 5 --sort=name --json --apparent-size --no-index --lang pt")).unwrap();
        assert_eq!((o.max_depth, o.top, o.order, o.format), (Some(1), Some(5), Order::Name, Format::Json));
        assert!(o.apparent && o.no_index);
        assert_eq!(o.lang.as_deref(), Some("pt"));
        assert_eq!(parse(&args("--csv --si")).unwrap().format, Format::Csv);
    }

    #[test]
    fn bytes_means_the_length() {
        let o = parse(&args("-b")).unwrap();
        assert!(o.unit == Unit::Bytes && o.apparent);
    }

    #[test]
    fn double_dash_ends_the_options() {
        let o = parse(&args("-- -odd")).unwrap();
        assert_eq!(o.paths, ["-odd"]);
        assert!(parse(&args("/?")).unwrap().help);
    }

    #[test]
    fn mistakes_are_named() {
        assert_eq!(parse(&args("--nope")), Err(Error::Unknown("--nope".into())));
        assert_eq!(parse(&args("-x")), Err(Error::Unknown("-x".into())));
        assert_eq!(parse(&args("--top")), Err(Error::Missing("--top".into())));
        assert_eq!(parse(&args("-d")), Err(Error::Missing("-d".into())));
        assert!(matches!(parse(&args("-d x")), Err(Error::Number { .. })));
        assert!(matches!(parse(&args("--sort date")), Err(Error::Choice { .. })));
    }

    #[test]
    fn the_language_is_found_before_parsing() {
        assert_eq!(lang_asked(&args("--lang es x")).as_deref(), Some("es"));
        assert_eq!(lang_asked(&args("x --lang=pt")).as_deref(), Some("pt"));
        assert_eq!(lang_asked(&args("x")), None);
    }

    #[test]
    fn help_and_errors_come_in_three_languages() {
        for lang in ["en", "pt", "es"] {
            let t = Texts::new(include_str!("strings.txt"), lang);
            let h = help(&t, "rsize-cli");
            assert!(h.contains("--max-depth=N") && h.contains(t.get("help.exit")), "{lang}");
            assert!(!Error::Unknown("--x".into()).describe(&t).contains("{opt}"), "{lang}");
        }
        let pt = Texts::new(include_str!("strings.txt"), "pt");
        assert_eq!(Error::Missing("--top".into()).describe(&pt), "a opção '--top' precisa de um valor");
    }
}
