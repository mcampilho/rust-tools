//! Looking after an installation: `cache rm`, `cleanup`, `hold`, `unhold`,
//! `config <name> <value>|rm` and `alias`.

use super::args::{getopt, Spec};
use super::paint;
use super::table::filesize;
use super::{delegate, update, Ctx};
use crate::sys::{is_admin, local_time};
use regex_lite::{escape, RegexBuilder};
use rt_scoop::apps;
use rt_scoop::config::{set_member, typed_value};
use rt_scoop::disk::{remove_tree, write_file};
use rt_scoop::json::{self, string, Json};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The files of the cache, in the order of their names.
fn cache_files(ctx: &Ctx) -> Vec<(String, PathBuf, u64)> {
    let mut files: Vec<(String, PathBuf, u64)> = std::fs::read_dir(&ctx.scoop.cache)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(|m| m.is_file())?;
            Some((e.file_name().to_string_lossy().into_owned(), e.path(), meta.len()))
        })
        .collect();
    files.sort_by_key(|(name, _, _)| name.to_ascii_lowercase());
    files
}

pub(super) fn remove_file(ctx: &mut Ctx, path: &Path) -> bool {
    match remove_tree(path) {
        Ok(()) => true,
        Err(e) => {
            let why = e.to_string();
            ctx.warn("error.remove", &[("why", &why)]);
            false
        }
    }
}

/// `cache rm <app…>|*`.
pub fn cache_rm(ctx: &mut Ctx, apps: &[String]) -> i32 {
    if apps.is_empty() {
        return ctx.fail("error.missing", &[("what", "<app(s)>")]);
    }
    let all = apps.iter().any(|a| a == "*" || a == "-a" || a == "--all");
    // Each name is a regular expression, as in Scoop.
    let pattern = RegexBuilder::new(&format!("^({})#", apps.join("|"))).case_insensitive(true).build();
    let pattern = match pattern {
        _ if all => None,
        Ok(p) => Some(p),
        Err(e) => {
            let why = e.to_string();
            return ctx.fail("error.regex", &[("why", &why)]);
        }
    };
    let (mut count, mut total, mut failed) = (0, 0, false);
    for (name, path, size) in cache_files(ctx) {
        if pattern.as_ref().is_some_and(|p| !p.is_match(&name)) {
            continue;
        }
        let line = ctx.t.fill("cache.removing", &[("name", &name)]);
        ctx.say(line);
        if !remove_file(ctx, &path) {
            failed = true;
            continue;
        }
        count += 1;
        total += size;
        // The list aria2 was given, if one was left.
        if let Some((app, _)) = name.split_once('#') {
            let list = ctx.scoop.cache.join(format!("{app}.txt"));
            if list.is_file() {
                failed |= !remove_file(ctx, &list);
            }
        }
    }
    let line = ctx.t.fill("cache.deleted", &[("n", &count.to_string()), ("size", &filesize(total))]);
    ctx.say(line);
    i32::from(failed)
}

/// The apps a command names, in one scope; errors are shown for the others
/// (`Confirm-InstallationStatus`).
pub(super) fn confirm(ctx: &mut Ctx, names: &[String], global: bool) -> (Vec<(String, bool)>, bool) {
    let mut found = Vec::new();
    let mut missing = false;
    for name in names.iter().filter(|n| !n.eq_ignore_ascii_case("scoop")) {
        let app = rt_scoop::resolve::parse_app(name).app;
        if !app.is_empty() && !app.contains(['/', '\\', ':']) && app != "." && app != ".." && ctx.scoop.app(&app, global).is_dir() {
            found.push((app, global));
        } else {
            missing = true;
            let key = if ctx.scoop.app(&app, !global).is_dir() { "app.other_scope" } else { "error.no_path" };
            ctx.warn(key, &[("app", &app)]);
        }
    }
    (found, missing)
}

/// Removes the versions of one app that are not the current one.
fn clean_app(ctx: &mut Ctx, app: &str, global: bool, verbose: bool, cache: bool) -> bool {
    // Without knowing which version is the current one, nothing is removed
    // (Scoop would then remove them all).
    let Some(current) = apps::current_version(&ctx.scoop, app, global) else {
        if verbose {
            ctx.warn("cleanup.unknown_current", &[("app", app)]);
        }
        return true;
    };
    let mut good = true;
    if cache {
        let prefix = format!("{app}#").to_ascii_lowercase();
        let keep = format!("{app}#{current}#").to_ascii_lowercase();
        for (name, path, _) in cache_files(ctx) {
            let name = name.to_ascii_lowercase();
            if name.starts_with(&prefix) && !name.starts_with(&keep) {
                good &= remove_file(ctx, &path);
            }
        }
    }
    let app_dir = ctx.scoop.app(app, global);
    // The folder `current` points to is never removed, whatever its name.
    let target = std::fs::read_link(app_dir.join("current")).ok().and_then(|t| t.file_name().map(|n| n.to_ascii_lowercase()));
    let old: Vec<String> = std::fs::read_dir(&app_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|v| !v.eq_ignore_ascii_case(&current) && !v.eq_ignore_ascii_case("current"))
        .filter(|v| target.as_deref().is_none_or(|t| !t.eq_ignore_ascii_case(v)))
        .collect();
    if old.is_empty() {
        if verbose {
            let line = ctx.t.fill("cleanup.clean", &[("app", app)]);
            ctx.say(line);
        }
        return good;
    }
    let line = ctx.t.fill("cleanup.removing", &[("app", app), ("versions", &old.join(" "))]);
    ctx.say(line);
    for version in &old {
        good &= remove_file(ctx, &app_dir.join(version));
    }
    good
}

const CLEANUP: [Spec; 3] = [('a', "all", false), ('g', "global", false), ('k', "cache", false)];

pub fn cleanup(ctx: &mut Ctx, args: &[String]) -> i32 {
    let opts = match getopt(args, &CLEANUP) {
        Ok(o) => o,
        Err(option) => return ctx.fail("cli.unknown_option", &[("opt", &option)]),
    };
    let (global, cache) = (opts.has('g'), opts.has('k'));
    let all = opts.has('a') || opts.rest.iter().any(|a| a == "*");
    if opts.rest.is_empty() && !all {
        return ctx.fail("error.missing", &[("what", "<app>")]);
    }
    if global && !is_admin() {
        return ctx.fail("error.admin", &[]);
    }
    let (targets, mut failed) = if all {
        let mut targets: Vec<(String, bool)> = apps::installed_names(&ctx.scoop, false).into_iter().map(|n| (n, false)).collect();
        if global {
            targets.extend(apps::installed_names(&ctx.scoop, true).into_iter().map(|n| (n, true)));
        }
        (targets, false)
    } else {
        confirm(ctx, &opts.rest, global)
    };
    for (app, global) in targets {
        failed |= !clean_app(ctx, &app, global, !all, cache);
    }
    if cache {
        for (name, path, _) in cache_files(ctx) {
            if name.to_ascii_lowercase().ends_with(".download") {
                failed |= !remove_file(ctx, &path);
            }
        }
    }
    if all && !failed {
        let line = ctx.t.get("cleanup.done").to_string();
        ctx.say_as(line, paint::GOOD);
    }
    i32::from(failed)
}

/// Sets `hold` in the install information of the current version.
fn set_hold(ctx: &mut Ctx, app: &str, global: bool, hold: bool) -> bool {
    if !apps::installed(&ctx.scoop, app, Some(global)) || app.contains(['/', '\\']) {
        ctx.warn(if global { "app.not_global" } else { "error.no_path" }, &[("app", app)]);
        return false;
    }
    let dir = apps::current_dir(&ctx.scoop, app, global);
    let read = ["scoop-install.json", "install.json"].iter().map(|n| dir.join(n)).find(|p| p.is_file());
    let Some(Json::Obj(members)) = read.as_deref().and_then(json::read) else {
        ctx.warn("hold.failed", &[("app", app)]);
        return false;
    };
    let held = members.iter().any(|(k, v)| k == "hold" && *v == Json::Bool(true));
    if held == hold {
        let key = if hold { "hold.already" } else { "hold.not_held" };
        let line = ctx.t.fill(key, &[("app", app)]);
        ctx.say(line);
        return true;
    }
    // Scoop writes the file it reads under its present name.
    let target = dir.join("scoop-install.json");
    let mut all = members;
    all.retain(|(k, _)| k != "hold");
    if hold {
        all.push(("hold".to_string(), Json::Bool(true)));
    }
    let text = format!("{}\r\n", json::pretty(&Json::Obj(all)).replace('\n', "\r\n"));
    if let Err(e) = write_file(&target, &text) {
        let why = e.to_string();
        ctx.warn("error.config", &[("why", &why)]);
        return false;
    }
    let line = ctx.t.fill(if hold { "hold.held" } else { "hold.released" }, &[("app", app)]);
    ctx.say(line);
    true
}

/// `hold` and `unhold`.
pub fn hold(ctx: &mut Ctx, args: &[String], hold: bool) -> i32 {
    let opts = match getopt(args, &[('g', "global", false)]) {
        Ok(o) => o,
        Err(option) => return ctx.fail("cli.unknown_option", &[("opt", &option)]),
    };
    let global = opts.has('g');
    if opts.rest.is_empty() {
        return ctx.fail("error.missing", &[("what", "<app>")]);
    }
    if global && !is_admin() {
        return ctx.fail("error.admin", &[]);
    }
    let mut failed = false;
    for app in &opts.rest {
        if app.eq_ignore_ascii_case("scoop") {
            let until = hold.then(|| local_time(SystemTime::now() + Duration::from_secs(24 * 3600)).iso);
            if let Err(code) = update::set_setting(ctx, "hold_update_until", until.clone().map(string)) {
                return code;
            }
            let line = match &until {
                Some(date) => ctx.t.fill("hold.scoop", &[("date", date)]),
                None => ctx.t.fill("hold.released", &[("app", "scoop")]),
            };
            ctx.say(line);
        } else {
            failed |= !set_hold(ctx, app, global, hold);
        }
    }
    i32::from(failed)
}

/// `config <name> <value>` and `config rm <name>`.
pub fn config_write(ctx: &mut Ctx, args: &[String]) -> i32 {
    let (name, value) = match args {
        [rm, name] if rm == "rm" => (name.to_ascii_lowercase(), None),
        [name, value] if name != "rm" => (name.to_ascii_lowercase(), Some(typed_value(value))),
        _ => return ctx.fail("error.missing", &[("what", "<name> <value>")]),
    };
    // Changing these moves PATH entries or builds Scoop's database.
    if name == "use_isolated_path" || (name == "use_sqlite_cache" && value == Some(Json::Bool(true))) {
        let mut line = vec!["config".to_string()];
        line.extend(args.iter().cloned());
        return delegate::run(ctx, &line, ctx.root_given);
    }
    let removed = value.is_none();
    if let Err(code) = update::set_setting(ctx, &name, value) {
        return code;
    }
    let line = if removed {
        ctx.t.fill("config.removed", &[("name", &name)])
    } else {
        ctx.t.fill("config.set", &[("name", &name), ("value", &args[1])])
    };
    ctx.say(line);
    0
}

/// The aliases of the configuration: name and the name of its script.
fn aliases(ctx: &Ctx) -> Vec<(String, Json)> {
    match ctx.scoop.setting("alias") {
        Json::Obj(members) => members.clone(),
        _ => Vec::new(),
    }
}

/// A script of the shims folder runs as a Scoop command: only a plain name.
fn alias_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || "-_.".contains(c)) && !name.starts_with('.')
}

/// `alias add|rm|list`.
pub fn alias(ctx: &mut Ctx, args: &[String]) -> i32 {
    let Some(sub) = args.first().map(String::as_str) else { return ctx.fail("error.missing", &[("what", "<add|rm|list>")]) };
    let opts = match getopt(&args[1..], &[('v', "verbose", false)]) {
        Ok(o) => o,
        Err(option) => return ctx.fail("cli.unknown_option", &[("opt", &option)]),
    };
    let shims = ctx.scoop.shims(false);
    let script = |name: &str| shims.join(format!("scoop-{name}.ps1"));
    let mut all = aliases(ctx);
    match (sub, opts.rest.as_slice()) {
        ("add", [name, command, rest @ ..]) => {
            if !alias_name(name) {
                return ctx.fail("alias.bad_name", &[("name", name)]);
            }
            if all.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) || script(name).exists() {
                return ctx.fail("alias.exists", &[("name", name)]);
            }
            let description = rest.first().map(String::as_str).unwrap_or("");
            let text = format!("# Summary: {description}\n{command}\r\n");
            if let Err(e) = write_file(&script(name), &text) {
                let why = e.to_string();
                return ctx.fail("error.config", &[("why", &why)]);
            }
            all.push((name.clone(), string(format!("scoop-{name}"))));
        }
        ("rm", [name, ..]) => {
            if !alias_name(name) || !all.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
                return ctx.fail("alias.unknown", &[("name", name)]);
            }
            if script(name).exists() && !remove_file(ctx, &script(name)) {
                return 1;
            }
            all.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        }
        ("list", _) => {
            all.sort_by_key(|(n, _)| n.to_ascii_lowercase());
            let rows: Vec<Vec<String>> = all
                .iter()
                .map(|(name, _)| match json::read_text(&script(name)) {
                    Some(text) => {
                        let summary = text.lines().next().and_then(|l| l.strip_prefix("# Summary:")).unwrap_or("").trim().to_string();
                        let command: Vec<&str> = text.lines().skip(1).map(str::trim).collect();
                        let summary = if opts.has('v') { summary } else { String::new() };
                        vec![name.clone(), command.join(" ").trim().to_string(), summary]
                    }
                    None => vec![name.clone(), "<BROKEN>".to_string(), String::new()],
                })
                .collect();
            if rows.is_empty() {
                let line = ctx.t.get("alias.none").to_string();
                ctx.say(line);
            } else {
                ctx.table(&["Name", "Command", "Summary"], &rows);
            }
            return 0;
        }
        ("add" | "rm", _) => return ctx.fail("error.missing", &[("what", "<name> <command>")]),
        _ => {
            let wanted = format!("alias {}", escape(sub));
            return ctx.fail("cli.unknown_option", &[("opt", &wanted)]);
        }
    }
    // Kept in the configuration of the user (or the portable one).
    match set_member(&ctx.scoop.config_path, "alias", Some(Json::Obj(all))) {
        Ok(()) => 0,
        Err(why) => ctx.fail("error.config", &[("why", &why)]),
    }
}
