//! `uninstall` (`libexec\scoop-uninstall.ps1`), in Scoop's order:
//! `pre_uninstall`, programs still running, `uninstaller`, shims, shortcuts,
//! `current`, module, `PATH`, variables, persisted links, the version folder,
//! `post_uninstall`, the older versions, `--purge`. The scripts of the
//! manifest run in Scoop's own code ([`rt_scoop::hooks`]); the rest is done
//! here. Uninstalling Scoop itself is Scoop's.

use super::args::getopt;
use super::machinery::{mirror, remove_shims, say_changes, still_running, Scripts};
use super::maintain::confirm;
use super::paint;
use super::Ctx;
use crate::sys::is_admin;
use rt_scoop::apps;
use rt_scoop::disk::remove_tree;
use rt_scoop::hooks::{boolean, quote, UNINSTALL_LIBS};
use rt_scoop::json::Json;
use rt_scoop::links;
use rt_scoop::manifest::{truthy, Arch, Manifest};
use rt_scoop::{env, shortcuts};
use std::path::Path;

/// The scripts of a manifest that `uninstall` runs.
fn scripts(manifest: &Manifest, arch: Arch) -> [bool; 3] {
    let uninstaller = manifest.arch("uninstaller", arch);
    [
        truthy(manifest.arch("pre_uninstall", arch)),
        truthy(uninstaller.get("file")) || truthy(uninstaller.get("args")) || truthy(uninstaller.get("script")),
        truthy(manifest.arch("post_uninstall", arch)),
    ]
}

/// The variables of `scoop-uninstall.ps1` when the scripts run (`$cmd` is
/// that of `exec` in `scoop.ps1`, which manifests ask).
fn setup(app: &str, version: &str, global: bool, purge: bool) -> String {
    [
        "$cmd = 'uninstall'".to_string(),
        format!("$app = {}", quote(app)),
        format!("$global = {}", boolean(global)),
        format!("$purge = {}", boolean(purge)),
        format!("$version = {}", quote(version)),
        "$appDir = appdir $app $global".into(),
        "$dir = versiondir $app $version $global".into(),
        "$persist_dir = persistdir $app $global".into(),
        "$manifest = installed_manifest $app $version $global".into(),
        "$install = install_info $app $version $global".into(),
        "$architecture = $install.architecture".into(),
        "$bucket = $install.bucket".into(),
    ]
    .join("\n")
}

pub fn run(ctx: &mut Ctx, args: &[String]) -> i32 {
    let opts = match getopt(args, &[('g', "global", false), ('p', "purge", false)]) {
        Ok(o) => o,
        Err(option) => return ctx.fail("cli.unknown_option", &[("opt", &option)]),
    };
    let (global, purge) = (opts.has('g'), opts.has('p'));
    if opts.rest.is_empty() {
        return ctx.fail("error.missing", &[("what", "<app>")]);
    }
    if global && !is_admin() {
        return ctx.fail("error.admin", &[]);
    }
    let mut names: Vec<String> = Vec::new();
    for name in &opts.rest {
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    let (targets, _) = confirm(ctx, &names, global);
    for (app, _) in &targets {
        if apps::failed(&ctx.scoop, app, global) {
            ctx.warn("app.failed", &[("app", app)]);
        }
    }
    for (app, global) in targets {
        if let Err(code) = uninstall_one(ctx, &app, global, purge) {
            return code;
        }
    }
    0
}

fn say_removed(ctx: &mut Ctx, key: &str, path: &Path) {
    let line = ctx.t.fill(key, &[("path", &ctx.friendly(path))]);
    ctx.say(line);
}

/// Err: the exit code when Scoop would stop the whole command.
fn uninstall_one(ctx: &mut Ctx, app: &str, global: bool, purge: bool) -> Result<(), i32> {
    let app_dir = ctx.scoop.app(app, global);
    let version = apps::current_version(&ctx.scoop, app, global);
    let mut manifest = None;
    if let Some(version) = &version {
        let line = ctx.t.fill("uninstall.start", &[("app", app), ("version", version)]);
        ctx.say_as(line, paint::NAME);
        let dir = app_dir.join(version);
        let m = apps::installed_manifest(&ctx.scoop, app, version, global).unwrap_or(Manifest(Json::Obj(Vec::new())));
        let arch =
            apps::install_info(&ctx.scoop, app, version, global).and_then(|i| Arch::parse(&i.architecture)).unwrap_or_else(|| ctx.arch());
        let [pre, uninstaller, post] = scripts(&m, arch);
        let mut host = Scripts::new(&UNINSTALL_LIBS, setup(app, version, global, purge));
        let done = remove_version(ctx, app, global, &dir, &m, arch, [pre, uninstaller, post], &mut host);
        host.finish();
        match done {
            Err(code) => return Err(code),
            Ok(false) => return Ok(()),
            Ok(true) => {}
        }
        manifest = Some(m);
    }

    // The older versions, and whatever else is in the app's folder.
    let mut others: Vec<String> = std::fs::read_dir(&app_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.eq_ignore_ascii_case("current"))
        .collect();
    others.sort_by_key(|n| n.to_lowercase());
    for old in others {
        let line = ctx.t.fill("uninstall.older", &[("version", &old)]);
        ctx.say(line);
        let dir = app_dir.join(&old);
        if let Some(m) = &manifest {
            links::unlink_persist_data(m, &dir);
        }
        if remove_tree(&dir).is_err() {
            ctx.warn("remove.in_use", &[("path", &ctx.friendly(&dir))]);
            return Ok(());
        }
    }
    let current = app_dir.join("current");
    if std::fs::symlink_metadata(&current).is_ok() {
        remove_tree(&current).map_err(|e| ctx.fail("error.remove", &[("why", &e.to_string())]))?;
    }
    if std::fs::read_dir(&app_dir).is_ok_and(|mut d| d.next().is_none()) {
        remove_tree(&app_dir).map_err(|e| ctx.fail("error.remove", &[("why", &e.to_string())]))?;
    }
    if purge {
        let line = ctx.t.get("uninstall.purge").to_string();
        ctx.say(line);
        let persist = ctx.scoop.persist(app, global);
        if persist.exists() && remove_tree(&persist).is_err() {
            ctx.warn("remove.in_use", &[("path", &ctx.friendly(&persist))]);
            return Ok(());
        }
    }
    let line = ctx.t.fill("uninstall.done", &[("app", app)]);
    ctx.say_as(line, paint::GOOD);
    Ok(())
}

/// The current version goes. Ok(false): this app stops here (Scoop's
/// `continue`), the next ones go on.
#[allow(clippy::too_many_arguments)]
fn remove_version(
    ctx: &mut Ctx,
    app: &str,
    global: bool,
    dir: &Path,
    m: &Manifest,
    arch: Arch,
    scripts: [bool; 3],
    host: &mut Scripts,
) -> Result<bool, i32> {
    let [pre, uninstaller, post] = scripts;
    if pre {
        host.hook(ctx, "pre_uninstall")?;
    }
    if still_running(ctx, app, global) {
        return Ok(false);
    }
    if uninstaller {
        host.run(ctx, "Invoke-Installer -Path $dir -Manifest $manifest -ProcessorArchitecture $architecture -Global:$global -Uninstall")?;
    }
    remove_shims(ctx, app, m, global, arch);
    for (link, _) in shortcuts::remove(ctx.system, m, global, arch) {
        say_removed(ctx, "shortcut.removing", &link);
    }
    let current = dir.parent().unwrap_or(dir).join("current");
    let had_current = !ctx.scoop.enabled("no_junction") && std::fs::symlink_metadata(&current).is_ok();
    if had_current {
        say_removed(ctx, "link.unlinking", &current);
    }
    let refdir = links::unlink_current(&ctx.scoop, dir).map_err(|why| ctx.fail("error.config", &[("why", &why)]))?;
    if let Some(name) = links::module_name(m) {
        let line = ctx.t.fill("module.uninstalling", &[("name", &name)]);
        ctx.say(line);
        if let Some(link) = links::uninstall_psmodule(&ctx.scoop, m, global) {
            say_removed(ctx, "module.removing", &link);
        }
    }
    let mut changes = Vec::new();
    let done = (|| -> Result<(), String> {
        changes.extend(env::env_rm_path(ctx.system, &ctx.scoop, m, &refdir, global, arch)?);
        changes.extend(env::env_rm(ctx.system, m, global, arch)?);
        Ok(())
    })();
    say_changes(ctx, &changes, global);
    done.map_err(|why| ctx.fail("error.config", &[("why", &why)]))?;
    mirror(ctx, host, &changes, false)?;

    links::unlink_persist_data(m, dir);
    if remove_tree(dir).is_err() && dir.exists() {
        ctx.warn("remove.in_use", &[("path", &ctx.friendly(dir))]);
        return Ok(false);
    }
    if post {
        host.hook(ctx, "post_uninstall")?;
    }
    Ok(true)
}
