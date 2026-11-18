//! Command line.

use crate::cache::{scoop_db_manifests, Cache};
use crate::checks::{find_roots, run_check, Context, CHECK_IDS};
use crate::fix::{Fixer, PowerShellRunner, Status, FIX_ORDER};
use crate::registry::Registry;
use crate::report::{fix_report, json_report, summary, text_report, Extras};
use crate::text::{system_language, LANGUAGES, T};
use rt_core::json::Json;
use std::io::Write;
use std::time::Instant;

pub struct Opts {
    pub root: Option<String>,
    pub old_roots: Vec<String>,
    pub json: bool,
    pub checks: Option<String>,
    pub skip: Option<String>,
    pub lang: Option<String>,
    pub max: usize,
    pub config_depth: usize,
    pub persist_depth: usize,
    pub fix: Option<String>,
    pub dry_run: bool,
    pub backup_dir: Option<String>,
    pub quiet: bool,
    pub no_cache: bool,
    pub help: bool,
    pub version: bool,
}

fn split(v: &Option<String>) -> Vec<String> {
    v.as_deref().unwrap_or("").split(',').map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect()
}

fn parse(args: &[String], t: &T) -> Result<Opts, String> {
    let mut o = Opts {
        root: None,
        old_roots: Vec::new(),
        json: false,
        checks: None,
        skip: None,
        lang: None,
        max: 50,
        config_depth: 3,
        persist_depth: 5,
        fix: None,
        dry_run: false,
        backup_dir: None,
        quiet: false,
        no_cache: false,
        help: false,
        version: false,
    };
    let mut i = 0;
    while i < args.len() {
        let (name, inline) = match args[i].split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (args[i].clone(), None),
        };
        i += 1;
        let value = |i: &mut usize| -> Result<String, String> {
            if let Some(v) = &inline {
                return Ok(v.clone());
            }
            let v = args.get(*i).cloned().ok_or_else(|| t.fill("cli.needs_value", &[("opt", &name)]))?;
            *i += 1;
            Ok(v)
        };
        let number = |v: String| v.parse::<usize>().map_err(|_| t.fill("cli.bad_value", &[("opt", &name), ("value", &v)]));
        match name.as_str() {
            "--root" => o.root = Some(value(&mut i)?),
            "--old-root" => o.old_roots.push(value(&mut i)?),
            "--json" => o.json = true,
            "--checks" => o.checks = Some(value(&mut i)?),
            "--skip" => o.skip = Some(value(&mut i)?),
            "--lang" => {
                let v = value(&mut i)?;
                if !LANGUAGES.contains(&v.as_str()) {
                    return Err(t.fill("cli.bad_value", &[("opt", "--lang"), ("value", &v)]));
                }
                o.lang = Some(v);
            }
            "--max" => o.max = number(value(&mut i)?)?,
            "--config-depth" => o.config_depth = number(value(&mut i)?)?,
            "--persist-depth" => o.persist_depth = number(value(&mut i)?)?,
            "--fix" => {
                o.fix = Some(match inline.clone() {
                    Some(v) => v,
                    None => match args.get(i).filter(|a| !a.starts_with('-')) {
                        Some(v) => {
                            i += 1;
                            v.clone()
                        }
                        None => "all".into(),
                    },
                })
            }
            "--dry-run" => o.dry_run = true,
            "--backup-dir" => o.backup_dir = Some(value(&mut i)?),
            "-q" | "--quiet" => o.quiet = true,
            "--no-cache" => o.no_cache = true,
            "-h" | "--help" | "/?" => o.help = true,
            "-V" | "--version" => o.version = true,
            _ => return Err(t.fill("cli.unknown_option", &[("opt", &name)])),
        }
    }
    Ok(o)
}

fn help(t: &T) -> String {
    let ids = CHECK_IDS.join(", ");
    let fix_ids = FIX_ORDER.join(", ");
    let rows: Vec<(&str, String)> = vec![
        ("--root PATH", t.get("cli.root").into()),
        ("--old-root PATH", t.get("cli.old_root").into()),
        ("--json", t.get("cli.json").into()),
        ("--checks LIST", t.fill("cli.checks", &[("ids", &ids)])),
        ("--skip LIST", t.get("cli.skip").into()),
        ("--lang LANG", t.fill("cli.lang", &[("langs", &LANGUAGES.join(", "))])),
        ("--max N", t.get("cli.max").into()),
        ("--config-depth N", t.get("cli.config_depth").into()),
        ("--persist-depth N", t.get("cli.persist_depth").into()),
        ("--fix [LIST]", t.fill("cli.fix", &[("ids", &fix_ids)])),
        ("--dry-run", t.get("cli.dry_run").into()),
        ("--backup-dir PATH", t.get("cli.backup_dir").into()),
        ("--no-cache", t.get("cli.no_cache").into()),
        ("-q, --quiet", t.get("cli.quiet").into()),
        ("-h, --help", t.get("cli.help").into()),
        ("-V, --version", t.get("cli.version").into()),
    ];
    let mut s =
        format!("{}: rscoop [{}]\n\n{}\n\n{}:\n", t.get("cli.usage"), t.get("cli.options"), t.get("cli.description"), t.get("cli.options"));
    for (k, v) in rows {
        s += &format!("  {k:<20} {v}\n");
    }
    s
}

fn lang_from_args(args: &[String]) -> Option<String> {
    for (i, a) in args.iter().enumerate() {
        if a == "--lang" {
            return args.get(i + 1).cloned();
        }
        if let Some(v) = a.strip_prefix("--lang=") {
            return Some(v.to_string());
        }
    }
    None
}

/// Where our cache lives.
pub fn cache_file() -> std::path::PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("rscoop").join("cache.db")
}

/// Run with `args`; returns the exit code (0 nothing found, 1 problems or failed repairs, 2 bad options).
pub fn run(
    args: &[String],
    registry: &dyn Registry,
    environ: std::collections::HashMap<String, String>,
    home: &str,
    out: &mut dyn Write,
    err: &mut dyn Write,
    powershell: Option<PowerShellRunner>,
) -> i32 {
    let t = T::new(&lang_from_args(args).unwrap_or_else(system_language));
    let o = match parse(args, &t) {
        Ok(o) => o,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 2;
        }
    };
    if o.help {
        let _ = write!(out, "{}", help(&t));
        return 0;
    }
    if o.version {
        let _ = writeln!(out, "rscoop {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    let selected = split(&o.checks);
    let skipped = split(&o.skip);
    for name in selected.iter().chain(&skipped) {
        if !CHECK_IDS.contains(&name.as_str()) {
            let _ = writeln!(err, "{}", t.fill("cli.unknown_check", &[("name", name), ("ids", &CHECK_IDS.join(", "))]));
            return 2;
        }
    }
    let only_fix: Option<Vec<String>> = match o.fix.as_deref() {
        None | Some("all") => None,
        Some(_) => Some(split(&o.fix)),
    };
    for name in only_fix.iter().flatten() {
        if !FIX_ORDER.contains(&name.as_str()) {
            let _ = writeln!(err, "{}", t.fill("cli.unknown_check", &[("name", name), ("ids", &FIX_ORDER.join(", "))]));
            return 2;
        }
    }
    let ids: Vec<&str> = CHECK_IDS
        .iter()
        .copied()
        .filter(|c| (selected.is_empty() || selected.iter().any(|s| s == c)) && !skipped.iter().any(|s| s == c))
        .collect();

    let roots = find_roots(registry, &environ, home, o.root.as_deref());
    let use_scoop_db = matches!(roots.config.get("use_sqlite_cache"), Json::Bool(true));
    let mut ctx = Context::new(roots, registry).with_old_roots(&o.old_roots);
    ctx.environ = environ;
    ctx.config_depth = o.config_depth;
    ctx.persist_depth = o.persist_depth;
    if !o.no_cache {
        ctx.cache = Cache::load(&cache_file());
    }
    if use_scoop_db {
        ctx.scoop_db = scoop_db_manifests(&ctx.root);
    }

    let start = Instant::now();
    let mut results = Vec::new();
    for id in ids {
        if !o.quiet {
            let _ = writeln!(err, "{}", t.fill("report.progress", &[("title", t.get(&format!("check.{id}")))]));
        }
        let t0 = Instant::now();
        let mut r = run_check(&ctx, id);
        r.seconds = Some(t0.elapsed().as_secs_f64());
        results.push(r);
    }
    let seconds = start.elapsed().as_secs_f64();
    if let Err(e) = ctx.cache.save() {
        let _ = writeln!(err, "cache: {e}");
    }

    let hits = ctx.cache.hits.load(std::sync::atomic::Ordering::Relaxed).to_string();
    let misses = ctx.cache.misses.load(std::sync::atomic::Ordering::Relaxed).to_string();
    let extras = Extras {
        cache: match ctx.cache.file() {
            Some(f) => format!("{}  ({})", f.display(), t.fill("status.cache", &[("hits", &hits), ("misses", &misses)])),
            None => t.get("status.off").into(),
        },
        scoop_db: match &ctx.scoop_db {
            Some(db) => {
                format!("{}  ({})", crate::paths::join(&ctx.root, "scoop.db"), t.fill("status.scoop_db", &[("n", &db.len().to_string())]))
            }
            None => t.get("status.off").into(),
        },
    };

    let mut fix = None;
    if o.fix.is_some() || o.dry_run {
        if !o.quiet {
            let _ = writeln!(err, "{}", t.get("fix.progress"));
        }
        let fixer = Fixer::new(&ctx, o.dry_run, o.backup_dir.as_deref(), powershell);
        fix = Some(fixer.run(&results, only_fix.as_deref()));
    }

    if o.json {
        let f = fix.as_ref().map(|(l, b, d)| (l, b.as_str(), *d));
        let _ = write!(out, "{}", json_report(&ctx, &results, &t, Some(seconds), f));
    } else {
        let _ = write!(out, "{}", text_report(&ctx, &results, &t, o.max, Some(seconds), &extras));
        if let Some((log, backup, dry)) = &fix {
            let _ = write!(out, "{}", fix_report(log, backup, *dry, &t, o.max));
        }
    }
    if let Some((log, _, dry)) = &fix {
        if !dry {
            return i32::from(log.count(Status::Failed) > 0);
        }
    }
    let (e, w, _) = summary(&results);
    i32::from(e + w > 0)
}
