//! Commands Scoop does not have: `outdated`, `leaves`, `uses`, `size`,
//! `bucket unused` and `hook`. They only read, except `hook`, which only
//! prints.

use super::table::{self, filesize};
use super::Ctx;
use crate::checks::parallel;
use crate::sys::today;
use rt_scoop::apps;
use rt_scoop::buckets;
use rt_scoop::disk::is_link;
use rt_scoop::json::{object, string, Json};
use rt_scoop::manifest::Arch;
use rt_scoop::resolve;
use std::path::Path;

/// Every installed app: (name, global).
fn installed(ctx: &Ctx) -> Vec<(String, bool)> {
    let mut all: Vec<(String, bool)> = apps::installed_names(&ctx.scoop, false).into_iter().map(|n| (n, false)).collect();
    all.extend(apps::installed_names(&ctx.scoop, true).into_iter().map(|n| (n, true)));
    all
}

fn number(n: u64) -> Json {
    Json::Num(n as f64)
}

/// `outdated`: the apps with a newer version in their bucket.
pub fn outdated(ctx: &mut Ctx) -> i32 {
    let day = today();
    let mut rows = Vec::new();
    let mut found = Vec::new();
    for (app, global) in installed(ctx) {
        let s = apps::status(&ctx.scoop, &app, global, &day);
        if !s.installed || !s.outdated {
            continue;
        }
        let mut info = Vec::new();
        if s.hold {
            info.push("Held package");
        }
        if global {
            info.push("Global install");
        }
        found.push(object([
            ("Name", string(app.clone())),
            ("Installed", string(s.version.clone())),
            ("Latest", string(s.latest.clone())),
            ("Held", Json::Bool(s.hold)),
            ("Global", Json::Bool(global)),
        ]));
        rows.push(vec![app, s.version, s.latest, info.join(", ")]);
    }
    if ctx.json {
        ctx.print_json(&Json::Arr(found));
    } else if rows.is_empty() {
        let line = ctx.t.get("outdated.none").to_string();
        ctx.say(line);
    } else {
        for line in table::lines(&["Name", "Installed", "Latest", "Info"], &rows) {
            ctx.say(line);
        }
    }
    0
}

/// What each installed app needs: (app, global, needs with alternatives).
fn needs_of(ctx: &Ctx) -> Vec<(String, bool, Vec<Vec<String>>)> {
    installed(ctx)
        .into_iter()
        .map(|(app, global)| {
            let version = apps::current_version(&ctx.scoop, &app, global).unwrap_or_default();
            let arch = apps::install_info(&ctx.scoop, &app, &version, global)
                .and_then(|i| Arch::parse(&i.architecture))
                .unwrap_or_else(|| ctx.arch());
            let needs = apps::installed_manifest(&ctx.scoop, &app, &version, global)
                .map(|m| resolve::needs(&ctx.scoop, &m, arch))
                .unwrap_or_default();
            (app, global, needs)
        })
        .collect()
}

fn uses_app(needs: &[Vec<String>], app: &str) -> bool {
    needs.iter().any(|alternatives| alternatives.iter().any(|a| a.eq_ignore_ascii_case(app)))
}

/// A table of installed apps (name, version, bucket), or their JSON.
fn show_apps(ctx: &mut Ctx, list: &[(String, bool)], none: &str, values: &[(&str, &str)]) {
    let mut rows = Vec::new();
    let mut json = Vec::new();
    for (app, global) in list {
        let version = apps::current_version(&ctx.scoop, app, *global).unwrap_or_default();
        let source = apps::install_info(&ctx.scoop, app, &version, *global).and_then(|i| i.bucket).unwrap_or_default();
        json.push(object([
            ("Name", string(app.clone())),
            ("Version", string(version.clone())),
            ("Source", string(source.clone())),
            ("Global", Json::Bool(*global)),
        ]));
        rows.push(vec![app.clone(), version, source, if *global { "Global install".into() } else { String::new() }]);
    }
    if ctx.json {
        ctx.print_json(&Json::Arr(json));
    } else if rows.is_empty() {
        let line = ctx.t.fill(none, values);
        ctx.say(line);
    } else {
        for line in table::lines(&["Name", "Version", "Source", "Info"], &rows) {
            ctx.say(line);
        }
    }
}

/// `leaves`: the installed apps no other installed app needs.
pub fn leaves(ctx: &mut Ctx) -> i32 {
    let all = needs_of(ctx);
    let list: Vec<(String, bool)> = all
        .iter()
        .filter(|(app, _, _)| !all.iter().any(|(other, _, needs)| !other.eq_ignore_ascii_case(app) && uses_app(needs, app)))
        .map(|(app, global, _)| (app.clone(), *global))
        .collect();
    show_apps(ctx, &list, "leaves.none", &[]);
    0
}

/// `uses <app>`: the installed apps that need it.
pub fn uses(ctx: &mut Ctx, args: &[String]) -> i32 {
    let Some(wanted) = args.first() else { return ctx.fail("error.missing", &[("what", "<app>")]) };
    let app = resolve::parse_app(wanted).app;
    let list: Vec<(String, bool)> =
        needs_of(ctx).into_iter().filter(|(_, _, needs)| uses_app(needs, &app)).map(|(name, global, _)| (name, global)).collect();
    show_apps(ctx, &list, "uses.none", &[("app", &app)]);
    0
}

/// The bytes under `path`, without going through links.
fn bytes(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if is_link(&meta) {
        return 0;
    }
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path).into_iter().flatten().flatten().map(|e| bytes(&e.path())).sum()
}

/// `size [app…]`: what each app takes: its folder (every version), its
/// persisted data and its downloads in the cache; the largest first.
pub fn size(ctx: &mut Ctx, args: &[String]) -> i32 {
    let mut list = installed(ctx);
    if !args.is_empty() {
        list.retain(|(app, _)| args.iter().any(|a| a.eq_ignore_ascii_case(app)));
    }
    let cache: Vec<(String, u64)> = std::fs::read_dir(&ctx.scoop.cache)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.file_name().to_string_lossy().to_lowercase(), e.metadata().ok()?.len())))
        .collect();
    let scoop = ctx.scoop.clone();
    let measured = parallel(&list, |(app, global)| {
        let dir = scoop.app(app, *global);
        let versions = version_count(&dir);
        (bytes(&dir), bytes(&scoop.persist(app, *global)), versions)
    });
    let mut rows: Vec<(String, bool, u64, u64, u64, usize)> = list
        .iter()
        .zip(measured)
        .map(|((app, global), (folder, persist, versions))| {
            let prefix = format!("{}#", app.to_lowercase());
            let cached = cache.iter().filter(|(n, _)| n.starts_with(&prefix)).map(|(_, s)| s).sum();
            (app.clone(), *global, folder, persist, cached, versions)
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.2 + r.3 + r.4));
    if ctx.json {
        let json = rows
            .iter()
            .map(|(app, global, folder, persist, cached, versions)| {
                object([
                    ("Name", string(app.clone())),
                    ("Global", Json::Bool(*global)),
                    ("Apps", number(*folder)),
                    ("Persist", number(*persist)),
                    ("Cache", number(*cached)),
                    ("Versions", number(*versions as u64)),
                ])
            })
            .collect();
        ctx.print_json(&Json::Arr(json));
        return 0;
    }
    let total: u64 = rows.iter().map(|r| r.2 + r.3 + r.4).sum();
    let shown: Vec<Vec<String>> = rows
        .iter()
        .map(|(app, _, folder, persist, cached, versions)| {
            vec![
                app.clone(),
                filesize(*folder),
                filesize(*persist),
                filesize(*cached),
                versions.to_string(),
                filesize(folder + persist + cached),
            ]
        })
        .collect();
    for line in table::lines(&["Name", "Apps", "Persist", "Cache", "Versions", "Total"], &shown) {
        ctx.say(line);
    }
    let line = ctx.t.fill("size.apps_total", &[("size", &filesize(total)), ("n", &rows.len().to_string())]);
    ctx.say(line);
    0
}

/// The version folders of an app (not `current`).
fn version_count(app_dir: &Path) -> usize {
    std::fs::read_dir(app_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().eq_ignore_ascii_case("current"))
        .count()
}

/// `bucket unused`: the local buckets no installed app came from.
pub fn unused_buckets(ctx: &mut Ctx) -> i32 {
    let used: Vec<String> = installed(ctx)
        .into_iter()
        .filter_map(|(app, global)| {
            let version = apps::current_version(&ctx.scoop, &app, global)?;
            apps::install_info(&ctx.scoop, &app, &version, global)?.bucket
        })
        .collect();
    let unused: Vec<String> = buckets::local(&ctx.scoop).into_iter().filter(|b| !used.iter().any(|u| u.eq_ignore_ascii_case(b))).collect();
    if ctx.json {
        ctx.print_json(&Json::Arr(unused.into_iter().map(string).collect()));
    } else if unused.is_empty() {
        let line = ctx.t.get("bucket.none_unused").to_string();
        ctx.say(line);
    } else {
        for b in unused {
            ctx.say(b);
        }
    }
    0
}

/// `hook`: a `scoop` function for the PowerShell profile that sends every
/// command to rscoop-cli (which hands to Scoop what it does not do itself).
pub fn hook(ctx: &mut Ctx) -> i32 {
    let exe = std::env::current_exe().map(|p| rt_scoop::disk::plain(&p).display().to_string()).unwrap_or_else(|_| "rscoop-cli.exe".into());
    let exe = format!("'{}'", exe.replace('\'', "''"));
    let text = format!(
        "# {}\n\
         function scoop {{\n    \
             if ($args.Count -eq 0) {{ & {exe} help }}\n    \
             elseif (\"$($args[0])\".StartsWith('-')) {{ & {exe} scoop @args }}\n    \
             else {{ & {exe} @args }}\n\
         }}",
        ctx.t.get("hook.comment")
    );
    ctx.say(text);
    0
}
