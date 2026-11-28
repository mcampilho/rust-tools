//! `update <app…>|*` (`libexec\scoop-update.ps1`: the list, then `update`
//! for each app): the new version into the cache with its hash checked,
//! the old one unlinked as `uninstall` would (its folder stays, for
//! `cleanup`), then the new one installed with the missing dependencies.
//! The scripts of the old manifest run with `$cmd = 'update'`, in the same
//! PowerShell as those of the new one. Apps installed from a file, a web
//! address or as `app@version`, the SQLite cache, a proxy and a failed
//! installation send the whole command to Scoop.

use super::args::{getopt, Spec};
use super::download::{aria2_program, to_cache};
use super::install::{install_app, show_suggestions, Options};
use super::machinery::{remove_shims, say_changes, still_running, Scripts};
use super::maintain::confirm;
use super::{delegate, update, Ctx};
use crate::sys::{is_admin, today};
use rt_scoop::apps;
use rt_scoop::buckets;
use rt_scoop::env;
use rt_scoop::hooks::{boolean, quote, INSTALL_LIBS};
use rt_scoop::json::Json;
use rt_scoop::links;
use rt_scoop::manifest::{Arch, Manifest};
use rt_scoop::resolve::{self, parse_app};

const SPEC: [Spec; 7] = [
    ('g', "global", false),
    ('f', "force", false),
    ('i', "independent", false),
    ('k', "no-cache", false),
    ('s', "skip-hash-check", false),
    ('q', "quiet", false),
    ('a', "all", false),
];

/// Does this `update` line update apps (not only Scoop and the buckets)?
pub fn updates_apps(args: &[String]) -> bool {
    !args.iter().all(|a| ["-q", "--quiet", "scoop"].contains(&a.as_str()))
}

/// What only Scoop does for an installed app.
fn only_scoop(ctx: &Ctx, app: &str, global: bool) -> bool {
    let info = apps::current_version(&ctx.scoop, app, global).and_then(|v| apps::install_info(&ctx.scoop, app, &v, global));
    apps::failed(&ctx.scoop, app, global) || info.is_some_and(|i| i.url.is_some())
}

pub fn run(ctx: &mut Ctx, args: &[String]) -> i32 {
    let opts = match getopt(args, &SPEC) {
        Ok(o) => o,
        Err(option) => return ctx.fail("cli.unknown_option", &[("opt", &option)]),
    };
    let (global, force, quiet, all) = (opts.has('g'), opts.has('f'), opts.has('q'), opts.has('a'));
    let to_scoop = |ctx: &mut Ctx| {
        let mut line = vec!["update".to_string()];
        line.extend(args.iter().cloned());
        delegate::run(ctx, &line, ctx.root_given)
    };
    let proxy = !matches!(ctx.scoop.setting("proxy").str(), "" | "default");
    if proxy || ctx.scoop.enabled("use_sqlite_cache") {
        return to_scoop(ctx);
    }
    if global && !is_admin() {
        return ctx.fail("error.admin", &[]);
    }
    let named: Vec<String> = opts.rest.iter().filter(|a| !a.eq_ignore_ascii_case("scoop")).cloned().collect();
    let every = all || named.iter().any(|a| a == "*");
    let targets: Vec<(String, bool)> = if every {
        let mut t: Vec<(String, bool)> = apps::installed_names(&ctx.scoop, false).into_iter().map(|n| (n, false)).collect();
        if global {
            t.extend(apps::installed_names(&ctx.scoop, true).into_iter().map(|n| (n, true)));
        }
        t
    } else if named.is_empty() {
        Vec::new()
    } else {
        confirm(ctx, &named, global).0
    };
    if targets.iter().any(|(app, g)| only_scoop(ctx, app, *g)) {
        return to_scoop(ctx);
    }
    if opts.rest.iter().any(|a| a.eq_ignore_ascii_case("scoop")) || update::outdated(ctx) {
        update::sync(ctx, &[]);
    }

    let day = today();
    let mut outdated = Vec::new();
    for (app, g) in &targets {
        let status = apps::status(&ctx.scoop, app, *g, &day, &super::download::remote_manifest);
        if status.installed && (force || status.outdated) {
            if status.hold {
                ctx.warn("update.held_app", &[("app", app), ("version", &status.version)]);
            } else {
                outdated.push((app.clone(), *g));
                let scope = if *g { " (global)" } else { "" };
                let line = format!("{app}: {} -> {}{scope}", status.version, status.latest);
                ctx.say(line);
            }
        } else if !every {
            if status.installed {
                let line = ctx.t.fill("update.latest", &[("app", app), ("version", &status.version)]);
                ctx.say(line);
            } else {
                let line = ctx.t.get("update.reinstall").to_string();
                ctx.say(line);
            }
        }
    }
    if !targets.is_empty() {
        if !outdated.is_empty() && aria2_program(ctx).is_some() && ctx.scoop.setting("aria2-warning-enabled") != &Json::Bool(false) {
            ctx.warn("install.aria2", &[]);
        }
        let line = match outdated.len() {
            0 => ctx.t.get("update.all_latest").to_string(),
            1 => ctx.t.get("update.one").to_string(),
            n => ctx.t.fill("update.many", &[("n", &n.to_string())]),
        };
        ctx.say(line);
    }
    // The new versions into the cache, several at a time.
    let mut planned = Vec::new();
    for (app, g) in &outdated {
        let Some(old) = apps::current_version(&ctx.scoop, app, *g) else { continue };
        let info = apps::install_info(&ctx.scoop, app, &old, *g).unwrap_or_default();
        let arch = Arch::parse(&info.architecture).unwrap_or_else(|| ctx.arch());
        let bucket = info.bucket.unwrap_or_else(|| "main".into());
        planned.extend(super::install::planned(ctx, &[format!("{bucket}/{app}")], arch));
    }
    super::download::prefetch(ctx, &planned);

    let mut suggested = Vec::new();
    for (app, g) in outdated {
        let o = Options { global: g, use_cache: !opts.has('k'), check_hash: !opts.has('s'), arch: ctx.arch(), command: "update" };
        match update_app(ctx, &app, &o, force, quiet, opts.has('i')) {
            Ok(s) => suggested.extend(s),
            Err(code) => return code,
        }
    }
    show_suggestions(ctx, &suggested);
    0
}

/// The variables of `update` when the scripts of the old manifest run.
fn setup(app: &str, old: &str, new: &str, arch: Arch, bucket: &str, o: &Options, force: bool) -> String {
    [
        "$cmd = 'update'".to_string(),
        format!("$app = {}", quote(app)),
        format!("$global = {}", boolean(o.global)),
        format!("$force = {}", boolean(force)),
        format!("$use_cache = {}", boolean(o.use_cache)),
        "$check_hash = $false".into(),
        format!("$old_version = {}", quote(old)),
        "$old_manifest = installed_manifest $app $old_version $global".into(),
        "$install = install_info $app $old_version $global".into(),
        format!("$architecture = {}", quote(arch.name())),
        format!("$bucket = {}", quote(bucket)),
        "$url = $null".into(),
        "$manifest = manifest $app $bucket $null".into(),
        format!("$version = {}", quote(new)),
        "$dir = versiondir $app $old_version $global".into(),
        "$persist_dir = persistdir $app $global".into(),
    ]
    .join("\n")
}

/// `update` of one app. Ok: what was suggested. Err: the exit code when Scoop
/// would stop the whole command.
fn update_app(ctx: &mut Ctx, app: &str, o: &Options, force: bool, quiet: bool, independent: bool) -> Result<Vec<(String, Json)>, i32> {
    let global = o.global;
    let Some(old) = apps::current_version(&ctx.scoop, app, global) else { return Ok(Vec::new()) };
    let old_manifest = apps::installed_manifest(&ctx.scoop, app, &old, global).unwrap_or(Manifest(Json::Obj(Vec::new())));
    let info = apps::install_info(&ctx.scoop, app, &old, global).unwrap_or_default();
    let arch = Arch::parse(&info.architecture).unwrap_or(o.arch);
    let bucket = info.bucket.clone().unwrap_or_else(|| "main".into());
    let manifest = buckets::manifest_path(&ctx.scoop, app, &bucket).and_then(|p| Manifest::read(&p));
    let Some(manifest) = manifest.filter(|m| !m.version().is_empty()) else {
        ctx.warn("update.no_manifest", &[("app", app)]);
        return Ok(Vec::new());
    };
    let mut new = manifest.version().to_string();
    let mut check_hash = o.check_hash;
    if new == "nightly" {
        if !quiet {
            ctx.warn("install.nightly", &[]);
        }
        new = format!("nightly-{}", today());
        check_hash = false;
    }
    if !force && old.eq_ignore_ascii_case(&new) {
        if !quiet {
            ctx.warn("update.already_latest", &[("app", app), ("version", &new)]);
        }
        return Ok(Vec::new());
    }
    let line = ctx.t.fill("update.updating", &[("app", app), ("old", &old), ("new", &new)]);
    ctx.say(line);
    if still_running(ctx, app, global) {
        let line = ctx.t.get("update.running").to_string();
        ctx.say(line);
        return Ok(Vec::new());
    }
    let line = ctx.t.get("update.downloading").to_string();
    ctx.say(line);
    if to_cache(ctx, app, &new, &manifest, arch, true, check_hash, aria2_program(ctx).as_deref()).is_none() {
        return Err(1);
    }

    let dir = ctx.scoop.app(app, global).join(&old);
    let mut host = Scripts::new(&INSTALL_LIBS, setup(app, &old, &new, arch, &bucket, o, force));
    // What to undo if the new version fails: the old one was unlinked, and
    // maybe moved aside.
    let mut unlinked = false;
    let mut aside_at: Option<std::path::PathBuf> = None;
    let done = (|| -> Result<Vec<(String, Json)>, i32> {
        host.run(ctx, "Invoke-HookScript -HookType 'pre_uninstall' -Manifest $old_manifest -ProcessorArchitecture $architecture")?;
        let line = ctx.t.fill("update.uninstalling", &[("app", app), ("version", &old)]);
        ctx.say(line);
        host.run(
            ctx,
            "Invoke-Installer -Path $dir -Manifest $old_manifest -ProcessorArchitecture $architecture -Global:$global -Uninstall",
        )?;
        remove_shims(ctx, app, &old_manifest, global, arch);
        let current = dir.parent().unwrap_or(&dir).join("current");
        if !ctx.scoop.enabled("no_junction") && std::fs::symlink_metadata(&current).is_ok() {
            let line = ctx.t.fill("link.unlinking", &[("path", &ctx.friendly(&current))]);
            ctx.say(line);
        }
        let refdir = links::unlink_current(&ctx.scoop, &dir).map_err(|why| ctx.fail("error.config", &[("why", &why)]))?;
        unlinked = true;
        if let Some(name) = links::module_name(&old_manifest) {
            let line = ctx.t.fill("module.uninstalling", &[("name", &name)]);
            ctx.say(line);
            if let Some(link) = links::uninstall_psmodule(&ctx.scoop, &old_manifest, global) {
                let line = ctx.t.fill("module.removing", &[("path", &ctx.friendly(&link))]);
                ctx.say(line);
            }
        }
        let mut changes = Vec::new();
        let removed = (|| -> Result<(), String> {
            changes.extend(env::env_rm_path(ctx.system, &ctx.scoop, &old_manifest, &refdir, global, arch)?);
            changes.extend(env::env_rm(ctx.system, &old_manifest, global, arch)?);
            Ok(())
        })();
        say_changes(ctx, &changes, global);
        removed.map_err(|why| ctx.fail("error.config", &[("why", &why)]))?;
        // The same version again: the old folder is kept aside.
        if force && old.eq_ignore_ascii_case(&new) {
            let parent = dir.parent().unwrap_or(&dir).to_path_buf();
            let mut aside = parent.join(format!("_{new}.old"));
            let mut i = 1;
            while aside.exists() {
                aside = parent.join(format!("_{new}.old({i})"));
                i += 1;
            }
            std::fs::rename(&dir, &aside).map_err(|e| ctx.fail("error.remove", &[("why", &e.to_string())]))?;
            aside_at = Some(aside);
        }
        host.run(ctx, "Invoke-HookScript -HookType 'post_uninstall' -Manifest $old_manifest -ProcessorArchitecture $architecture")?;

        // The new version, and the dependencies it now needs.
        let entry = format!("{bucket}/{app}");
        let installing = Options { global, use_cache: o.use_cache, check_hash: false, arch, command: o.command };
        let mut suggested = Vec::new();
        if !independent {
            let deps = resolve::dependencies(&ctx.scoop, &entry, arch).unwrap_or_default();
            for dep in deps.into_iter().filter(|d| !d.eq_ignore_ascii_case(&entry)) {
                if !apps::installed(&ctx.scoop, &parse_app(&dep).app, None) {
                    suggested.extend(install_app(ctx, &dep, &installing, None)?);
                }
            }
        }
        suggested.extend(install_app(ctx, &entry, &installing, Some(&mut host))?);
        Ok(suggested)
    })();
    host.finish();
    if done.is_err() && unlinked {
        // The new version was rolled back: the old one is the one in use again.
        ctx.warn("update.restoring", &[("app", app), ("version", &old)]);
        if let Some(aside) = aside_at {
            let _ = rt_scoop::disk::remove_tree(&dir);
            let _ = std::fs::rename(&aside, &dir);
        }
        let target = super::reset::Target { app: app.to_string(), global, version: old.clone(), manifest: old_manifest };
        let _ = super::reset::reset_one(ctx, &target);
    }
    done
}
