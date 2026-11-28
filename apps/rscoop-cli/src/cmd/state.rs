//! The state of the installation: `status --local`, `bucket list|known`,
//! `cache show` and `config`.

use super::{table, update, Ctx};
use crate::checks::parallel;
use crate::sys::{local_time, today};
use rt_scoop::apps;
use rt_scoop::buckets;
use rt_scoop::json::{self, object, string, Json};
use rt_scoop::resolve;
use std::path::PathBuf;

pub fn status(ctx: &mut Ctx, args: &[String]) -> i32 {
    // Without `--local` the remotes are asked first.
    let local = args.iter().any(|a| a == "-l" || a == "--local");
    let remotes = if local { update::Remotes { skipped: true, ..Default::default() } } else { update::remotes(ctx) };
    if !ctx.json {
        let message = if remotes.scoop {
            Some("status.scoop_behind")
        } else if remotes.buckets {
            Some("status.buckets_behind")
        } else if !remotes.unreachable && !remotes.skipped {
            Some("status.up_to_date")
        } else {
            None
        };
        if remotes.unreachable {
            ctx.warn("status.unreachable", &[]);
            for line in &remotes.errors {
                ctx.warn("error.config", &[("why", line)]);
            }
        }
        if let Some(key) = message {
            let line = ctx.t.get(key).to_string();
            ctx.say(line);
        }
    }
    let mut names = Vec::new();
    // Scoop shows the global apps first here.
    for global in [true, false] {
        names.extend(apps::installed_names(&ctx.scoop, global).into_iter().map(|n| (n, global)));
    }
    let (scoop, day) = (&ctx.scoop, today());
    let states = parallel(&names, |(name, global)| apps::status(scoop, name, *global, &day, &super::download::remote_manifest));
    let rows: Vec<(String, apps::Status)> = names
        .into_iter()
        .zip(states)
        .map(|((name, _), s)| (name, s))
        .filter(|(_, s)| s.outdated || s.failed || s.deprecated || s.removed || !s.missing_deps.is_empty())
        .collect();
    let latest = |s: &apps::Status| if s.outdated { s.latest.clone() } else { String::new() };
    let info = |s: &apps::Status| {
        [(s.failed, "Install failed"), (s.hold, "Held package"), (s.deprecated, "Deprecated"), (s.removed, "Manifest removed")]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, text)| *text)
            .collect::<Vec<_>>()
    };
    if ctx.json {
        let list = rows
            .iter()
            .map(|(name, s)| {
                object([
                    ("Name", string(name.clone())),
                    ("Installed Version", string(s.version.clone())),
                    ("Latest Version", string(latest(s))),
                    ("Missing Dependencies", string(s.missing_deps.join(" | "))),
                    ("Info", string(info(s).join(", "))),
                ])
            })
            .collect();
        ctx.print_json(&Json::Arr(list));
        return 0;
    }
    if rows.is_empty() {
        if !(remotes.scoop || remotes.buckets || remotes.unreachable) {
            let text = ctx.t.get("status.ok").to_string();
            ctx.say(text);
        }
        return 0;
    }
    let shown = |ctx: &Ctx, text: &str| {
        let key = match text {
            "Install failed" => "info.failed",
            "Held package" => "info.held",
            "Deprecated" => "info.deprecated",
            _ => "info.removed",
        };
        ctx.t.get(key).to_string()
    };
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|(name, s)| {
            let notes: Vec<String> = info(s).iter().map(|i| shown(ctx, i)).collect();
            vec![name.clone(), s.version.clone(), latest(s), s.missing_deps.join(" | "), notes.join(", ")]
        })
        .collect();
    ctx.table(&["Name", "Installed Version", "Latest Version", "Missing Dependencies", "Info"], &cells);
    0
}

pub struct Bucket {
    pub name: String,
    pub source: String,
    /// The date of the last commit (ISO), else of the folder.
    pub updated: String,
    pub manifests: usize,
}

/// `list_buckets`.
pub fn buckets_found(ctx: &Ctx) -> Vec<Bucket> {
    let scoop = &ctx.scoop;
    let git = resolve::helper(scoop, "git", r"mingw64\bin\git.exe").unwrap_or_else(|| PathBuf::from("git.exe"));
    let names = buckets::local(scoop);
    parallel(&names, |name| {
        let repository = buckets::repository(scoop, name);
        let inner = repository.join("bucket");
        let commit = repository
            .join(".git")
            .is_dir()
            .then(|| std::process::Command::new(&git).arg("-C").arg(&repository).args(["log", "--format=%aI", "-n", "1"]).output().ok())
            .flatten()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty());
        let updated = commit.or_else(|| std::fs::metadata(&inner).and_then(|m| m.modified()).ok().map(|t| local_time(t).iso));
        let source = buckets::origin(&repository).unwrap_or_else(|| repository.display().to_string());
        let manifests = if inner.is_dir() { buckets::manifests_under(&inner).len() } else { 0 };
        Bucket { name: name.clone(), source, updated: updated.unwrap_or_default(), manifests }
    })
}

pub fn bucket_json(b: &Bucket) -> Json {
    object([
        ("Name", string(b.name.clone())),
        ("Source", string(b.source.clone())),
        ("Updated", string(b.updated.clone())),
        ("Manifests", Json::Num(b.manifests as f64)),
    ])
}

pub fn bucket(ctx: &mut Ctx, args: &[String]) -> i32 {
    if args.first().map(String::as_str) == Some("known") {
        let names: Vec<String> = buckets::known(&ctx.scoop).into_iter().map(|(n, _)| n).collect();
        if ctx.json {
            ctx.print_json(&Json::Arr(names.into_iter().map(string).collect()));
        } else {
            names.into_iter().for_each(|n| ctx.say(n));
        }
        return 0;
    }
    let found = buckets_found(ctx);
    if ctx.json {
        ctx.print_json(&Json::Arr(found.iter().map(bucket_json).collect()));
        return 0;
    }
    if found.is_empty() {
        ctx.warn("bucket.none", &[]);
        return 2;
    }
    let cells: Vec<Vec<String>> = found
        .into_iter()
        .map(|b| {
            let updated = b.updated.get(..19).map(|d| d.replace('T', " ")).unwrap_or(b.updated);
            vec![b.name, b.source, updated, b.manifests.to_string()]
        })
        .collect();
    ctx.table(&["Name", "Source", "Updated", "Manifests"], &cells);
    0
}

/// `cache show`: the downloads kept, `app#version#url`.
pub fn cache(ctx: &mut Ctx, args: &[String]) -> i32 {
    let mut wanted: Vec<String> = args.iter().filter(|a| *a != "show").map(|a| a.to_ascii_lowercase()).collect();
    if wanted.iter().any(|w| w == "*") {
        wanted.clear();
    }
    let mut files: Vec<(String, u64)> = std::fs::read_dir(&ctx.scoop.cache)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), e.metadata().ok().filter(|m| m.is_file())?.len())))
        .filter(|(name, _)| {
            let lower = name.to_ascii_lowercase();
            lower.split_once('#').is_some_and(|(app, _)| wanted.is_empty() || wanted.iter().any(|w| w == app))
        })
        .collect();
    files.sort_by_key(|(name, _)| name.to_ascii_lowercase());
    let total: u64 = files.iter().map(|(_, n)| n).sum();
    let parts = |name: &str| {
        let mut p = name.split('#');
        (p.next().unwrap_or("").to_string(), p.next().unwrap_or("").to_string())
    };
    if ctx.json {
        let list = files
            .iter()
            .map(|(name, size)| {
                let (app, version) = parts(name);
                object([("Name", string(app)), ("Version", string(version)), ("Length", Json::Num(*size as f64))])
            })
            .collect();
        ctx.print_json(&Json::Arr(list));
        return 0;
    }
    if !files.is_empty() {
        let cells: Vec<Vec<String>> = files
            .iter()
            .map(|(name, size)| {
                let (app, version) = parts(name);
                vec![app, version, size.to_string()]
            })
            .collect();
        ctx.table(&["Name", "Version", "Length"], &cells);
    }
    let text = ctx.t.fill("cache.total", &[("n", &files.len().to_string()), ("size", &table::filesize(total))]);
    ctx.say(text);
    0
}

pub fn config(ctx: &mut Ctx, args: &[String]) -> i32 {
    let text = |v: &Json| match v {
        Json::Str(s) => s.clone(),
        Json::Bool(b) => if *b { "True" } else { "False" }.to_string(),
        other => json::pretty(other),
    };
    let Some(name) = args.first() else {
        if ctx.json {
            let config = ctx.scoop.config.clone();
            ctx.print_json(&config);
        } else if let Json::Obj(members) = &ctx.scoop.config {
            let items: Vec<(String, String)> = members.iter().map(|(k, v)| (k.clone(), text(v))).collect();
            ctx.pairs(&items);
        }
        return 0;
    };
    let value = ctx.scoop.setting(name).clone();
    if ctx.json {
        ctx.print_json(&value);
    } else if value == Json::Null {
        let line = ctx.t.fill("config.unset", &[("name", name)]);
        ctx.say(line);
    } else {
        ctx.say(text(&value));
    }
    0
}
