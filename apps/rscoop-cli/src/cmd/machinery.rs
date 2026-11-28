//! What `reset` and `uninstall` (and later `install`) share: the shims of an
//! app, the messages of the changes to the environment, and the check for
//! programs still running.

use super::query::find_command;
use super::Ctx;
use rt_scoop::env::Change;
use rt_scoop::hooks::{Host, Stop};
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
/// Ok: what the shims added to `PATH` (the shims folder, once).
pub fn create_shims(ctx: &mut Ctx, manifest: &Manifest, dirs: [&Path; 3], global: bool, arch: Arch) -> Result<Vec<Change>, i32> {
    let [dir, original_dir, persist_dir] = dirs;
    let mut added = Vec::new();
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
            Ok(made) => {
                say_made(ctx, &made, global);
                added.extend(made.path);
            }
            Err(why) => return Err(ctx.fail("error.config", &[("why", &why)])),
        }
    }
    Ok(added)
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

/// PowerShell for the scripts of one app, started when the first is needed.
pub struct Scripts {
    host: Option<Host>,
    libs: &'static [&'static str],
    setup: String,
}

impl Scripts {
    /// `libs`: those the command loads; `setup`: its variables.
    pub fn new(libs: &'static [&'static str], setup: String) -> Scripts {
        Scripts { host: None, libs, setup }
    }

    /// More variables: run now if PowerShell is there, else when it starts.
    pub fn set(&mut self, ctx: &mut Ctx, setup: &str) -> Result<(), i32> {
        if self.host.is_none() {
            self.setup = format!("{}\n{setup}", self.setup);
            return Ok(());
        }
        for line in setup.lines() {
            self.run(ctx, line)?;
        }
        Ok(())
    }

    /// Runs a line; Err is the exit code that ends the whole command.
    pub fn run(&mut self, ctx: &mut Ctx, line: &str) -> Result<(), i32> {
        let _ = ctx.out.flush();
        if self.host.is_none() {
            let powershell = super::delegate::powershell(ctx);
            match Host::start(&powershell, &ctx.scoop, self.libs, &self.setup) {
                Ok(h) => self.host = Some(h),
                Err(stop) => return Err(stopped(ctx, stop)),
            }
        }
        match self.host.as_mut().map(|h| h.run(line)) {
            Some(Err(stop)) => {
                self.host = None;
                Err(stopped(ctx, stop))
            }
            _ => Ok(()),
        }
    }

    pub fn hook(&mut self, ctx: &mut Ctx, kind: &str) -> Result<(), i32> {
        self.run(ctx, &format!("Invoke-HookScript -HookType '{kind}' -Manifest $manifest -ProcessorArchitecture $architecture"))
    }

    pub fn finish(self) {
        if let Some(h) = self.host {
            h.finish();
        }
    }
}

fn stopped(ctx: &mut Ctx, stop: Stop) -> i32 {
    match stop {
        Stop::Start(why) => ctx.fail("error.start", &[("what", "PowerShell"), ("why", &why)]),
        Stop::Ended(code) => code,
    }
}

/// What Scoop's own `Add-Path`, `Remove-Path`, `env_set` and `env_rm` also
/// change in the session they run in (`$env:PATH`, `env:`), sent to the
/// session of the scripts, so that a later script finds what Scoop's would
/// (the `post_install` of python runs `python`). `force`: `Add-Path -Force`.
pub fn mirror(ctx: &mut Ctx, host: &mut Scripts, changes: &[Change], force: bool) -> Result<(), i32> {
    use rt_scoop::hooks::{boolean, quote};
    let list = |paths: &[&String]| format!("@({})", paths.iter().map(|p| quote(p)).collect::<Vec<_>>().join(", "));
    let mut lines = Vec::new();
    let mut i = 0;
    while i < changes.len() {
        match &changes[i] {
            Change::Added(_) | Change::Removed(_) => {
                let adding = matches!(changes[i], Change::Added(_));
                let mut group = Vec::new();
                while let Some(Change::Added(p) | Change::Removed(p)) = changes.get(i) {
                    if matches!(changes[i], Change::Added(_)) != adding {
                        break;
                    }
                    group.push(p);
                    i += 1;
                }
                let paths = list(&group);
                lines.push(if adding {
                    format!(
                        "$__rscoop_in, $__rscoop_rest = Split-PathLikeEnvVar {paths} $env:PATH; if (!$__rscoop_in -or {}) {{ $env:PATH = ({paths} + $__rscoop_rest) -join ';' }}",
                        boolean(force)
                    )
                } else {
                    format!("$__rscoop_in, $__rscoop_rest = Split-PathLikeEnvVar {paths} $env:PATH; if ($__rscoop_in) {{ $env:PATH = $__rscoop_rest }}")
                });
                continue;
            }
            Change::Set(name, value) => lines.push(format!("Set-Content {} {}", quote(&format!("env:\\{name}")), quote(value))),
            Change::Unset(name) => {
                let item = quote(&format!("env:\\{name}"));
                lines.push(format!("if (Test-Path {item}) {{ Remove-Item {item} }}"));
            }
        }
        i += 1;
    }
    if lines.is_empty() {
        return Ok(());
    }
    host.set(ctx, &lines.join("\n"))
}
