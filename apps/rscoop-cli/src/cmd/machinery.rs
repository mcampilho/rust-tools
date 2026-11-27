//! What `reset` and `uninstall` (and later `install`) share: the shims of an
//! app, the messages of the changes to the environment, and the check for
//! programs still running.

use super::query::find_command;
use super::Ctx;
use rt_scoop::env::Change;
use rt_scoop::manifest::{Arch, Manifest};
use rt_scoop::shims;
use std::path::{Path, PathBuf};

/// The messages of `Add-Path`, `Remove-Path`, `env_set` and `env_rm`.
pub fn say_changes(ctx: &mut Ctx, changes: &[Change], global: bool) {
    let scope = |user: &str, machine: &str| if global { machine.to_string() } else { user.to_string() };
    for change in changes {
        let line = match change {
            Change::Added(path) => ctx.t.fill(&scope("path.adding", "path.adding_global"), &[("path", &ctx.friendly(Path::new(path)))]),
            Change::Removed(path) => {
                ctx.t.fill(&scope("path.removing", "path.removing_global"), &[("path", &ctx.friendly(Path::new(path)))])
            }
            Change::Set(name, value) => ctx.t.fill(&scope("env.setting", "env.setting_global"), &[("name", name), ("value", value)]),
            Change::Unset(name) => ctx.t.fill(&scope("env.removing", "env.removing_global"), &[("name", name)]),
        };
        ctx.say(line);
    }
}

/// `test_running_process`: true when the app must be left alone.
pub fn still_running(ctx: &mut Ctx, app: &str, global: bool) -> bool {
    let running = rt_scoop::system::running_from(ctx.system, &ctx.scoop.app(app, global));
    if running.is_empty() {
        return false;
    }
    let ignore = ctx.scoop.enabled("ignore_running_processes");
    ctx.warn(if ignore { "running.ignored" } else { "running.stop" }, &[("app", app)]);
    for p in &running {
        let line = format!("  {} ({}) {}", p.name, p.id, p.path.display());
        ctx.say(line);
    }
    !ignore
}

/// The name of the shim of one `bin` entry (`shim_def`).
pub fn shim_name(bin: &rt_scoop::manifest::Bin) -> String {
    match bin.alias.as_deref().filter(|a| !a.is_empty()) {
        Some(alias) => alias.to_string(),
        None => {
            let leaf = bin.target.rsplit(['\\', '/']).next().unwrap_or(&bin.target);
            shims::strip_ext(leaf).to_string()
        }
    }
}

/// `create_shims`; Err is the exit code when Scoop would stop (`abort`).
pub fn create_shims(ctx: &mut Ctx, manifest: &Manifest, dirs: [&Path; 3], global: bool, arch: Arch) -> Result<(), i32> {
    let [dir, original_dir, persist_dir] = dirs;
    for bin in manifest.bins(arch) {
        let shown = match &bin.alias {
            Some(alias) => alias.clone(),
            None => shim_name(&bin),
        };
        let line = ctx.t.fill("shim.creating", &[("name", &shown)]);
        ctx.say(line);
        let inside = dir.join(&bin.target);
        let path: Option<PathBuf> = if inside.is_file() {
            Some(inside)
        } else if Path::new(&bin.target).is_file() {
            Some(PathBuf::from(&bin.target))
        } else {
            find_command(ctx, &bin.target)
        };
        let Some(path) = path else { return Err(ctx.fail("shim.missing", &[("target", &bin.target)])) };
        let args: Vec<String> = bin.args.iter().map(|a| rt_scoop::shortcuts::substitute(a, dir, original_dir, persist_dir)).collect();
        let name = bin.alias.clone().filter(|a| !a.is_empty());
        match shims::create(&ctx.scoop, ctx.system, &path, global, name.as_deref(), &args) {
            Ok(made) => say_made(ctx, &made, global),
            Err(why) => return Err(ctx.fail("error.config", &[("why", &why)])),
        }
    }
    Ok(())
}

/// The messages of `shim`.
pub fn say_made(ctx: &mut Ctx, made: &shims::Made, global: bool) {
    say_changes(ctx, &made.path, global);
    for o in &made.overwritten {
        if o.from.is_empty() {
            ctx.warn("shim.overwriting_external", &[("shim", &o.shim), ("target", &o.target)]);
        } else {
            ctx.warn("shim.overwriting", &[("shim", &o.shim), ("target", &o.target), ("app", &o.from)]);
        }
    }
    if let Some(exe) = &made.gui {
        let line = ctx.t.fill("shim.gui", &[("path", &exe.display().to_string())]);
        ctx.say(line);
    }
}

/// `rm_shim` of one name, with its messages.
pub fn remove_shim(ctx: &mut Ctx, dir: &Path, name: &str, app: Option<&str>) {
    for file in shims::remove(dir, name, app) {
        let line = ctx.t.fill("shim.removing", &[("name", &file)]);
        ctx.say(line);
    }
}

/// `rm_shims`.
pub fn remove_shims(ctx: &mut Ctx, app: &str, manifest: &Manifest, global: bool, arch: Arch) {
    let dir = ctx.scoop.shims(global);
    for bin in manifest.bins(arch) {
        remove_shim(ctx, &dir, &shim_name(&bin), Some(app));
    }
}
