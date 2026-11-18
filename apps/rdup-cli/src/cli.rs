//! The command line: the options of `fdupes` that matter here, and a few more.

use rt_cli::Texts;
use rt_dups::filter::{parse_date, parse_size, Filter, Patterns};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Text,
    Json,
    Csv,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Opts {
    pub paths: Vec<String>,
    /// Show the size of the files of each group.
    pub size: bool,
    /// Sizes as `1.5K`.
    pub human: bool,
    pub summarize: bool,
    pub sameline: bool,
    pub omit_first: bool,
    pub quiet: bool,
    /// Leave out names that start with a dot.
    pub no_hidden: bool,
    /// The names of a hard-linked file count as copies.
    pub hard_links: bool,
    /// Look for duplicate folders instead of files.
    pub folders: bool,
    /// The least share two folders have in common, in thousandths.
    pub share: u16,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub after: Option<i64>,
    pub before: Option<i64>,
    pub ext: String,
    pub skip_ext: String,
    pub include: String,
    pub exclude: String,
    pub format: Format,
    /// Folder by folder only.
    pub no_index: bool,
    pub no_cache: bool,
    pub cache: Option<String>,
    pub lang: Option<String>,
    pub help: bool,
    pub version: bool,
}

impl Default for Opts {
    fn default() -> Opts {
        Opts {
            paths: Vec::new(),
            size: false,
            human: false,
            summarize: false,
            sameline: false,
            omit_first: false,
            quiet: false,
            no_hidden: false,
            hard_links: false,
            folders: false,
            share: 1000,
            min_size: None,
            max_size: None,
            after: None,
            before: None,
            ext: String::new(),
            skip_ext: String::new(),
            include: String::new(),
            exclude: String::new(),
            format: Format::Text,
            no_index: false,
            no_cache: false,
            cache: None,
            lang: None,
            help: false,
            version: false,
        }
    }
}

impl Opts {
    /// The filter of the files to compare.
    pub fn filter(&self) -> Filter {
        let mut f = Filter::min(self.min_size.unwrap_or(1));
        if let Some(m) = self.max_size {
            f.max_size = m;
        }
        f.after = self.after.unwrap_or(i64::MIN);
        f.before = self.before.unwrap_or(i64::MAX);
        f.only_ext = Patterns::parse_exts(&self.ext);
        f.skip_ext = Patterns::parse_exts(&self.skip_ext);
        f.only_names = Patterns::parse(&self.include);
        let hidden = if self.no_hidden { ".*" } else { "" };
        f.skip_names = Patterns::parse(&format!("{} {hidden}", self.exclude));
        f
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Unknown(String),
    Missing(String),
    /// A size, date or number that is not understood.
    Value {
        opt: String,
        value: String,
    },
}

impl Error {
    pub fn describe(&self, t: &Texts) -> String {
        match self {
            Error::Unknown(o) => t.fill("err.unknown", &[("opt", o)]),
            Error::Missing(o) => t.fill("err.missing", &[("opt", o)]),
            Error::Value { opt, value } => t.fill("err.value", &[("opt", opt), ("value", value)]),
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

fn size(v: &str, opt: &str) -> Result<u64, Error> {
    match parse_size(v) {
        Some(Some(n)) => Ok(n),
        _ => Err(Error::Value { opt: opt.to_string(), value: v.to_string() }),
    }
}

/// `end`: until the end of the period, not from its start.
fn date(v: &str, opt: &str, end: bool) -> Result<i64, Error> {
    match parse_date(v, end) {
        Some(Some(n)) => Ok(n),
        _ => Err(Error::Value { opt: opt.to_string(), value: v.to_string() }),
    }
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
                // Accepted as in fdupes: it is what this program does anyway.
                "recurse" | "noempty" => {}
                "size" => o.size = true,
                "human-readable" => o.human = true,
                "summarize" => o.summarize = true,
                "sameline" => o.sameline = true,
                "omitfirst" => o.omit_first = true,
                "quiet" => o.quiet = true,
                "nohidden" => o.no_hidden = true,
                "hardlinks" => o.hard_links = true,
                "folders" => o.folders = true,
                "share" => {
                    let v = value(inline, args, &mut i, &opt)?;
                    let pct = v.trim_end_matches('%').parse::<u16>().ok().filter(|p| (50..=100).contains(p));
                    o.share = pct.map(|p| p * 10).ok_or(Error::Value { opt: opt.clone(), value: v })?;
                    o.folders = true;
                }
                "json" => o.format = Format::Json,
                "csv" => o.format = Format::Csv,
                "no-index" => o.no_index = true,
                "no-cache" => o.no_cache = true,
                "min-size" => o.min_size = Some(size(&value(inline, args, &mut i, &opt)?, &opt)?),
                "max-size" => o.max_size = Some(size(&value(inline, args, &mut i, &opt)?, &opt)?),
                "after" => o.after = Some(date(&value(inline, args, &mut i, &opt)?, &opt, false)?),
                "before" => o.before = Some(date(&value(inline, args, &mut i, &opt)?, &opt, true)?),
                "ext" => o.ext = value(inline, args, &mut i, &opt)?,
                "skip-ext" => o.skip_ext = value(inline, args, &mut i, &opt)?,
                "include" => o.include = value(inline, args, &mut i, &opt)?,
                "exclude" => o.exclude = value(inline, args, &mut i, &opt)?,
                "cache" => o.cache = Some(value(inline, args, &mut i, &opt)?),
                "lang" => o.lang = Some(value(inline, args, &mut i, &opt)?),
                _ => return Err(Error::Unknown(opt)),
            }
        } else {
            for c in a.chars().skip(1) {
                match c {
                    'r' | 'R' | 'n' => {}
                    'S' => o.size = true,
                    'h' => o.human = true,
                    'm' => o.summarize = true,
                    '1' => o.sameline = true,
                    'f' => o.omit_first = true,
                    'q' => o.quiet = true,
                    'A' => o.no_hidden = true,
                    'H' => o.hard_links = true,
                    'D' => o.folders = true,
                    other => return Err(Error::Unknown(format!("-{other}"))),
                }
            }
        }
    }
    Ok(o)
}

/// The text of `--help`.
pub fn help(t: &Texts, prog: &str) -> String {
    let options: &[(&str, &str)] = &[
        ("-r, -R, --recurse", "opt.recurse"),
        ("-n, --noempty", "opt.noempty"),
        ("-S, --size", "opt.size"),
        ("-h, --human-readable", "opt.human"),
        ("-m, --summarize", "opt.summarize"),
        ("-1, --sameline", "opt.sameline"),
        ("-f, --omitfirst", "opt.omitfirst"),
        ("-q, --quiet", "opt.quiet"),
        ("-A, --nohidden", "opt.nohidden"),
        ("-H, --hardlinks", "opt.hardlinks"),
        ("-D, --folders", "opt.folders"),
        ("    --share=PERCENT", "opt.share"),
        ("    --min-size=SIZE", "opt.min_size"),
        ("    --max-size=SIZE", "opt.max_size"),
        ("    --after=DATE", "opt.after"),
        ("    --before=DATE", "opt.before"),
        ("    --ext=LIST", "opt.ext"),
        ("    --skip-ext=LIST", "opt.skip_ext"),
        ("    --include=NAMES", "opt.include"),
        ("    --exclude=NAMES", "opt.exclude"),
        ("    --json", "opt.json"),
        ("    --csv", "opt.csv"),
        ("    --no-index", "opt.no_index"),
        ("    --no-cache", "opt.no_cache"),
        ("    --cache=FILE", "opt.cache"),
        ("    --lang=en|pt|es", "opt.lang"),
        ("    --version", "opt.version"),
        ("    --help", "opt.help"),
    ];
    let mut s = format!(
        "{}: {prog} [{}]… {}…\n\n{}\n\n{}:\n",
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
        assert_eq!(o.format, Format::Text);
        assert!(!o.size && !o.summarize && !o.sameline && !o.omit_first);
        assert_eq!(o.filter(), Filter::default());
    }

    #[test]
    fn fdupes_options_cluster() {
        let o = parse(&args("-rSm x")).unwrap();
        assert!(o.size && o.summarize);
        let o = parse(&args("-R -n -1 -f -q -A -h")).unwrap();
        assert!(o.sameline && o.omit_first && o.quiet && o.no_hidden && o.human);
        assert!(o.paths.is_empty());
    }

    #[test]
    fn long_options_with_values() {
        let o = parse(&args("--min-size=100KB --max-size 1GB --after 2024-01 --before=2024-12-31 --ext jpg,png --skip-ext=tmp --include a* --exclude=b? --json")).unwrap();
        assert_eq!(o.min_size, Some(100 * 1024));
        assert_eq!(o.max_size, Some(1 << 30));
        assert!(o.after.unwrap() < o.before.unwrap());
        assert_eq!((o.ext.as_str(), o.skip_ext.as_str(), o.include.as_str(), o.exclude.as_str()), ("jpg,png", "tmp", "a*", "b?"));
        assert_eq!(o.format, Format::Json);
    }

    #[test]
    fn the_options_build_the_filter() {
        let o = parse(&args("--min-size 1KB --ext jpg --exclude cache -A")).unwrap();
        let f = o.filter();
        assert_eq!(f.min_size, 1024);
        assert!(f.keeps_ext("a.JPG") && !f.keeps_ext("a.png"));
        assert!(f.skip_names.matches("cache") && f.skip_names.matches(".git") && !f.skip_names.matches("src"));
        assert!(f.narrows());
    }

    #[test]
    fn the_end_of_a_day_includes_the_day() {
        let o = parse(&args("--after 2024-01-31 --before 2024-01-31")).unwrap();
        let (a, b) = (o.after.unwrap(), o.before.unwrap());
        assert!(b - a >= 23 * 3600 && b - a <= 25 * 3600, "{}", b - a);
    }

    #[test]
    fn double_dash_and_help() {
        assert_eq!(parse(&args("-- -odd")).unwrap().paths, ["-odd"]);
        assert!(parse(&args("/?")).unwrap().help);
    }

    #[test]
    fn mistakes_are_named() {
        assert_eq!(parse(&args("--nope")), Err(Error::Unknown("--nope".into())));
        assert_eq!(parse(&args("-x")), Err(Error::Unknown("-x".into())));
        assert_eq!(parse(&args("--ext")), Err(Error::Missing("--ext".into())));
        assert!(matches!(parse(&args("--min-size big")), Err(Error::Value { .. })));
        assert!(matches!(parse(&args("--after yesterday")), Err(Error::Value { .. })));
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
            let h = help(&t, "rdup-cli");
            assert!(h.contains("--min-size=SIZE") && h.contains(t.get("help.exit")), "{lang}");
            assert!(!Error::Value { opt: "--x".into(), value: "y".into() }.describe(&t).contains('{'), "{lang}");
        }
        let pt = Texts::new(include_str!("strings.txt"), "pt");
        assert_eq!(Error::Missing("--ext".into()).describe(&pt), "a opção '--ext' precisa de um valor");
    }
}
