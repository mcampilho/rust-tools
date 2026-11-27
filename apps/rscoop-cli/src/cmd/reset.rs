//! `reset` (`libexec\scoop-reset.ps1`): an installed version made the one in
//! use again: `current`, shims, shortcuts, environment and persisted data.

use super::args::getopt;
use super::machinery::{create_shims, say_changes, still_running};
use super::{delegate, Ctx};
use crate::sys::is_admin;
use rt_scoop::apps;
use rt_scoop::env::{self, Vars};
use rt_scoop::links;
use rt_scoop::manifest::{Arch, Manifest};
use rt_scoop::resolve::parse_app;
use rt_scoop::shortcuts::{self, Made};
use std::path::{Path, PathBuf};

/// One app to reset, as Scoop works it out.
pub(super) struct Target {
    pub app: String,
    pub global: bool,
    pub version: String,
    pub manifest: Manifest,
}

/// The variables `env_set` can name while `reset` runs (`$name` is the
/// variable being set, as in `env_set`).
fn vars<'a>(
    ctx: &'a Ctx,
    t: &'a Target,
    dir: &Path,
    original: &Path,
    name: &str,
    arch: Arch,
    env: &'a dyn Fn(&str) -> Option<String>,
) -> Vars<'a> {
    let persist = ctx.scoop.persist(&t.app, t.global);
    let global = if t.global { "True" } else { "False" };
    Vars {
        known: vec![
            ("dir", dir.display().to_string()),
            ("original_dir", original.display().to_string()),
            ("persist_dir", persist.display().to_string()),
            ("version", t.version.clone()),
            ("app", t.app.clone()),
            ("global", global.into()),
            ("architecture", arch.name().into()),
            ("arch", arch.name().into()),
            ("name", name.into()),
            ("bucket", String::new()),
            ("cmd", "reset".into()),
        ],
        env,
    }
}

/// Why an app is left alone: the key of the message and its values.
type Refusal = (String, Vec<(&'static str, String)>);

/// The folder the app is used from after `link_current`.
fn used_dir(ctx: &Ctx, version_dir: &Path) -> PathBuf {
    if ctx.scoop.enabled("no_junction") {
        version_dir.to_path_buf()
    } else {
        version_dir.parent().unwrap_or(version_dir).join("current")
    }
}

fn arch_of(ctx: &Ctx, app: &str, version: &str, global: bool) -> Arch {
    apps::install_info(&ctx.scoop, app, version, global).and_then(|i| Arch::parse(&i.architecture)).unwrap_or_else(|| ctx.arch())
}

pub fn run(ctx: &mut Ctx, args: &[String]) -> i32 {
    let opts = match getopt(args, &[('a', "all", false)]) {
        Ok(o) => o,
        Err(option) => return ctx.fail("cli.unknown_option", &[("opt", &option)]),
    };
    let all = opts.has('a') || opts.rest.iter().any(|a| a == "*");
    if opts.rest.is_empty() && !all {
        return ctx.fail("error.missing", &[("what", "<app>")]);
    }
    let named: Vec<(String, Option<bool>)> = if all {
        let mut list: Vec<(String, Option<bool>)> =
            apps::installed_names(&ctx.scoop, false).into_iter().map(|n| (n, Some(false))).collect();
        list.extend(apps::installed_names(&ctx.scoop, true).into_iter().map(|n| (n, Some(true))));
        list
    } else {
        opts.rest.iter().map(|a| (a.clone(), None)).collect()
    };

    // What Scoop would refuse is said now; what only PowerShell can expand
    // sends the whole command to Scoop before anything is changed.
    let mut targets: Vec<Result<Target, Refusal>> = Vec::new();
    for (query, global) in named {
        let wanted = parse_app(&query);
        let app = wanted.app.clone();
        let global = global.unwrap_or_else(|| apps::installed(&ctx.scoop, &app, Some(true)));
        if app.eq_ignore_ascii_case("scoop") {
            continue;
        }
        if !apps::installed(&ctx.scoop, &app, None) {
            targets.push(Err(("error.no_path".into(), vec![("app", app)])));
            continue;
        }
        let Some(version) = wanted.version.clone().or_else(|| apps::current_version(&ctx.scoop, &app, global)) else {
            targets.push(Err(("error.no_path".into(), vec![("app", app)])));
            continue;
        };
        let Some(manifest) = apps::installed_manifest(&ctx.scoop, &app, &version, global) else {
            targets.push(Err(("reset.no_version".into(), vec![("app", app), ("version", version)])));
            continue;
        };
        targets.push(Ok(Target { app, global, version, manifest }));
    }
    let env_lookup = |name: &str| ctx.env.get(name).map(String::from);
    for t in targets.iter().flatten() {
        let arch = arch_of(ctx, &t.app, &t.version, t.global);
        let version_dir = ctx.scoop.app(&t.app, t.global).join(&t.version);
        let dir = used_dir(ctx, &version_dir);
        for (name, value) in env::env_set_values(&t.manifest, arch) {
            if env::expand(&value, &vars(ctx, t, &dir, &version_dir, &name, arch, &env_lookup)).is_none() {
                ctx.warn("reset.by_scoop", &[("name", &name)]);
                let mut line = vec!["reset".to_string()];
                line.extend(args.iter().cloned());
                return delegate::run(ctx, &line, ctx.root_given);
            }
        }
    }

    for target in targets {
        let t = match target {
            Ok(t) => t,
            Err((key, values)) => {
                let values: Vec<(&str, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
                ctx.warn(&key, &values);
                continue;
            }
        };
        if t.global && !is_admin() {
            ctx.warn("reset.global_admin", &[("app", &t.app), ("version", &t.version)]);
            continue;
        }
        if let Err(code) = reset_one(ctx, &t) {
            return code;
        }
    }
    0
}

/// Err: the exit code when Scoop would stop the whole command.
pub(super) fn reset_one(ctx: &mut Ctx, t: &Target) -> Result<(), i32> {
    let line = ctx.t.fill("reset.start", &[("app", &t.app), ("version", &t.version)]);
    ctx.say(line);
    let original = env::absolute(&ctx.scoop.app(&t.app, t.global).join(&t.version));
    let persist_dir = ctx.scoop.persist(&t.app, t.global);
    if still_running(ctx, &t.app, t.global) {
        return Ok(());
    }
    let arch = arch_of(ctx, &t.app, &t.version, t.global);
    let fail = |ctx: &mut Ctx, why: String| ctx.fail("error.config", &[("why", &why)]);

    if !ctx.scoop.enabled("no_junction") {
        let current = original.parent().unwrap_or(&original).join("current");
        let line = ctx.t.fill("link.linking", &[("from", &ctx.friendly(&current)), ("to", &ctx.friendly(&original))]);
        ctx.say(line);
    }
    let dir = links::link_current(&ctx.scoop, &original).map_err(|why| fail(ctx, why))?;
    create_shims(ctx, &t.manifest, [&dir, &original, &persist_dir], t.global, arch)?;
    for made in shortcuts::create(ctx.system, &t.manifest, [&dir, &original, &persist_dir], t.global, arch) {
        match made {
            Made::Created(name, file) => {
                let line = ctx.t.fill("shortcut.creating", &[("name", &name), ("file", &file)]);
                ctx.say(line);
            }
            Made::NoTarget(name, file, path) => {
                ctx.warn("shortcut.no_target", &[("name", &name), ("file", &file), ("path", &path.display().to_string())])
            }
            Made::NoIcon(name, file, path) => {
                ctx.warn("shortcut.no_icon", &[("name", &name), ("file", &file), ("path", &path.display().to_string())])
            }
            Made::Failed(name, why) => ctx.warn("shortcut.failed", &[("name", &name), ("why", &why)]),
        }
    }

    // Old values out, the manifest's in.
    let env_lookup = |name: &str| ctx.env.get(name).map(String::from);
    let values: Vec<(String, String)> = env::env_set_values(&t.manifest, arch)
        .into_iter()
        .filter_map(|(name, value)| {
            let expanded = env::expand(&value, &vars(ctx, t, &dir, &original, &name, arch, &env_lookup))?;
            Some((name, expanded))
        })
        .collect();
    let mut changes = Vec::new();
    let done = (|| -> Result<(), String> {
        changes.extend(env::env_rm_path(ctx.system, &ctx.scoop, &t.manifest, &dir, t.global, arch)?);
        changes.extend(env::env_rm(ctx.system, &t.manifest, t.global, arch)?);
        changes.extend(env::env_add_path(ctx.system, &ctx.scoop, &t.manifest, &dir, t.global, arch)?);
        changes.extend(env::env_set(ctx.system, &values, t.global)?);
        Ok(())
    })();
    say_changes(ctx, &changes, t.global);
    done.map_err(|why| fail(ctx, why))?;

    links::unlink_persist_data(&t.manifest, &original);
    match links::persist_data(&t.manifest, &dir, &persist_dir) {
        Ok(names) => {
            for name in names {
                let line = ctx.t.fill("persist.persisting", &[("name", &name)]);
                ctx.say(line);
            }
        }
        Err(why) => ctx.warn("error.config", &[("why", &why)]),
    }
    links::persist_permission(&ctx.scoop, &t.manifest, t.global, is_admin());
    Ok(())
}
