//! `install` (`libexec\scoop-install.ps1`, `install_app`), in Scoop's order:
//! the download into the cache and the hash, then the extraction, the
//! scripts and the installer, `current`, shims, shortcuts, module, `PATH`,
//! variables, persisted data, `post_install`, and the two JSON files.
//!
//! Done here: the dependencies, the downloads, and everything that links the
//! app in. Done in Scoop's own code, in one PowerShell per app
//! ([`rt_scoop::hooks`]): the extraction (`Invoke-Extraction`: 7-Zip, msi,
//! Inno Setup, dark, zip without 7-Zip), `pre_install`, the installer and
//! `post_install`. What only Scoop does (`app@version`, manifests from a
//! file or a web address, the SQLite cache, a proxy, repairing a failed
//! installation, `show_manifest`) sends the whole command to Scoop.

use super::args::{getopt, Spec};
use super::download::{aria2_program, job, to_cache};
use super::machinery::{create_shims, remove_shims, say_changes, Scripts};
use super::{delegate, update, Ctx};
use crate::sys::{is_admin, today, windows_11};
use rt_scoop::apps;
use rt_scoop::disk::remove_tree;
use rt_scoop::download::file_name;
use rt_scoop::env::{self, Vars};
use rt_scoop::hooks::{boolean, quote, INSTALL_LIBS};
use rt_scoop::json::{self, Json};
use rt_scoop::links;
use rt_scoop::manifest::{Arch, Manifest};
use rt_scoop::resolve::{self, parse_app, DependencyError};
use rt_scoop::shortcuts::{self, Made};
use std::path::{Path, PathBuf};

const SPEC: [Spec; 6] = [
    ('g', "global", false),
    ('i', "independent", false),
    ('k', "no-cache", false),
    ('s', "skip-hash-check", false),
    ('u', "no-update-scoop", false),
    ('a', "arch", true),
];

/// The options of one `install` (also used by `update`).
pub struct Options {
    pub global: bool,
    pub use_cache: bool,
    pub check_hash: bool,
    pub arch: Arch,
    /// `$cmd` for the scripts: `install` or `update`.
    pub command: &'static str,
}

/// Why Scoop has to do the whole command, if it does.
fn only_scoop(ctx: &Ctx, queries: &[String], global: bool) -> bool {
    let proxy = !matches!(ctx.scoop.setting("proxy").str(), "" | "default");
    proxy
        || ctx.scoop.enabled("use_sqlite_cache")
        || ctx.scoop.enabled("show_manifest")
        || queries.iter().any(|q| {
            let parsed = parse_app(q);
            let name = parsed.app.rsplit(['/', '\\']).next().unwrap_or(&parsed.app).trim_end_matches(".json").to_string();
            parsed.version.is_some()
                || q.ends_with(".json")
                || resolve::is_remote(q)
                || Path::new(q).is_file()
                // A failed installation is repaired by Scoop's reset or uninstall.
                || [global, !global].iter().any(|&g| apps::failed(&ctx.scoop, &name, g))
                // Downloads only Scoop makes (FTP, FossHub, a private release).
                || job(ctx, q).is_none()
        })
}

pub fn run(ctx: &mut Ctx, args: &[String]) -> i32 {
    let opts = match getopt(args, &SPEC) {
        Ok(o) => o,
        Err(option) => return ctx.fail("cli.unknown_option", &[("opt", &option)]),
    };
    let global = opts.has('g');
    let arch = match opts.value('a') {
        Some(a) => match Arch::parse(a) {
            Some(a) => a,
            None => return ctx.fail("error.arch", &[("arch", a)]),
        },
        None => ctx.arch(),
    };
    if opts.rest.is_empty() {
        return ctx.fail("error.missing", &[("what", "<app>")]);
    }
    if global && !is_admin() {
        return ctx.fail("error.admin", &[]);
    }
    let mut explicit: Vec<String> = Vec::new();
    for a in &opts.rest {
        if !explicit.contains(a) {
            explicit.push(a.clone());
        }
    }
    let delegate_all = |ctx: &mut Ctx| {
        let mut line = vec!["install".to_string()];
        line.extend(args.iter().cloned());
        delegate::run(ctx, &line, ctx.root_given)
    };
    if only_scoop(ctx, &explicit, global) {
        return delegate_all(ctx);
    }
    if update::outdated(ctx) {
        if opts.has('u') {
            ctx.warn("install.outdated", &[]);
        } else {
            update::sync(ctx, &[]);
        }
    }
    if explicit.len() == 1 {
        let app = parse_app(&explicit[0]).app;
        if let Some(version) = apps::current_version(&ctx.scoop, &app, global) {
            let hint = if global { " --global" } else { "" };
            ctx.warn("install.already", &[("app", &app), ("version", &version), ("global", hint)]);
            return 0;
        }
    }

    // The dependencies first, each once (`Get-Dependency`).
    let mut all: Vec<String> = Vec::new();
    for query in &explicit {
        let wanted = if opts.has('i') {
            let r = resolve::resolve(&ctx.scoop, query);
            if r.manifest.is_none() {
                return ctx.fail("error.no_manifest", &[("app", query)]);
            }
            vec![match &r.bucket {
                Some(b) => format!("{b}/{}", r.app),
                None => r.app.clone(),
            }]
        } else {
            match resolve::dependencies(&ctx.scoop, query, arch) {
                Ok(list) => list,
                Err(DependencyError::NoManifest(app)) => return ctx.fail("error.no_manifest", &[("app", &app)]),
                Err(DependencyError::Circular(from, to)) => return ctx.fail("error.circular", &[("from", &from), ("to", &to)]),
            }
        };
        for entry in wanted {
            if !all.contains(&entry) {
                all.push(entry);
            }
        }
    }
    // Dependencies Scoop would have to do bring it the whole command.
    if only_scoop(ctx, &all, global) {
        return delegate_all(ctx);
    }
    let mut to_install = Vec::new();
    for entry in all {
        let app = parse_app(&entry).app;
        if apps::installed(&ctx.scoop, &app, Some(global)) {
            if explicit.iter().any(|e| parse_app(e).app.eq_ignore_ascii_case(&app)) {
                let version = apps::current_version(&ctx.scoop, &app, global).unwrap_or_default();
                ctx.warn("install.skipping", &[("app", &app), ("version", &version)]);
            }
        } else {
            to_install.push(entry);
        }
    }
    if aria2_program(ctx).is_some() && ctx.scoop.setting("aria2-warning-enabled") != &Json::Bool(false) {
        ctx.warn("install.aria2", &[]);
    }
    let options = Options { global, use_cache: !opts.has('k'), check_hash: !opts.has('s'), arch, command: "install" };
    let mut suggested: Vec<(String, Json)> = Vec::new();
    for entry in &to_install {
        match install_app(ctx, entry, &options, None) {
            Ok(Some(suggest)) => suggested.push(suggest),
            Ok(None) => {}
            Err(code) => return code,
        }
    }
    show_suggestions(ctx, &suggested);
    0
}

/// `show_suggestions`.
pub fn show_suggestions(ctx: &mut Ctx, suggested: &[(String, Json)]) {
    let mut installed = apps::installed_names(&ctx.scoop, true);
    installed.extend(apps::installed_names(&ctx.scoop, false));
    for (app, features) in suggested {
        let Json::Obj(features) = features else { continue };
        for (_, wanted) in features {
            let choices = rt_scoop::manifest::strings(wanted);
            let fulfilled = choices.iter().any(|c| installed.iter().any(|i| i.eq_ignore_ascii_case(&parse_app(c).app)));
            if !fulfilled && !choices.is_empty() {
                ctx.warn("install.suggest", &[("app", app), ("choices", &choices.join("' or '"))]);
            }
        }
    }
}

/// The variables `install_app` has when the scripts run.
fn setup(app: &str, bucket: &str, version: &str, arch: Arch, files: &[String], o: &Options) -> String {
    let names: Vec<String> = files.iter().map(|f| quote(f)).collect();
    [
        format!("$cmd = {}", quote(o.command)),
        format!("$global = {}", boolean(o.global)),
        format!("$use_cache = {}", boolean(o.use_cache)),
        format!("$check_hash = {}", boolean(o.check_hash)),
        format!("$null, $manifest, $bucket, $url = Get-Manifest {}", quote(&format!("{bucket}/{app}"))),
        format!("$app = {}", quote(app)),
        format!("$version = {}", quote(version)),
        format!("$is_nightly = {}", boolean(version.starts_with("nightly-"))),
        format!("$architecture = {}", quote(arch.name())),
        "$dir = versiondir $app $version $global".into(),
        "$original_dir = $dir".into(),
        "$persist_dir = persistdir $app $global".into(),
        format!("$fname = @({})", names.join(", ")),
        "$suggested = @{}".into(),
    ]
    .join("\n")
}

/// The variables a value of `env_set` can name while `install_app` runs.
#[allow(clippy::too_many_arguments)]
fn vars<'a>(
    app: &str,
    bucket: &str,
    version: &str,
    dirs: [&Path; 3],
    arch: Arch,
    global: bool,
    name: &str,
    env: &'a dyn Fn(&str) -> Option<String>,
) -> Vars<'a> {
    let [dir, original, persist] = dirs;
    Vars {
        known: vec![
            ("dir", dir.display().to_string()),
            ("original_dir", original.display().to_string()),
            ("persist_dir", persist.display().to_string()),
            ("version", version.into()),
            ("app", app.into()),
            ("bucket", bucket.into()),
            ("global", if global { "True" } else { "False" }.into()),
            ("architecture", arch.name().into()),
            ("arch", arch.name().into()),
            ("name", name.into()),
        ],
        env,
    }
}

/// `install_app`. Ok(Some): what the app suggests. Err: the exit code when
/// Scoop would stop the whole command. `scripts`: the PowerShell of the
/// command, when one is open already (`update`), else one of its own.
pub fn install_app(ctx: &mut Ctx, entry: &str, o: &Options, scripts: Option<&mut Scripts>) -> Result<Option<(String, Json)>, i32> {
    let r = resolve::resolve(&ctx.scoop, entry);
    let (Some(manifest), Some(path)) = (r.manifest.clone(), r.path.clone()) else {
        return Err(ctx.fail("error.no_manifest", &[("app", entry)]));
    };
    let app = r.app.clone();
    let bucket = r.bucket.clone().unwrap_or_default();
    let mut version = manifest.version().to_string();
    if version.is_empty() {
        return Err(ctx.fail("install.no_version", &[]));
    }
    if let Some(c) = version.chars().find(|c| !(c.is_alphanumeric() || "._-+".contains(*c))) {
        return Err(ctx.fail("install.bad_version", &[("char", &c.to_string())]));
    }
    let mut check_hash = o.check_hash;
    if version == "nightly" {
        ctx.warn("install.nightly", &[]);
        version = format!("nightly-{}", today());
        check_hash = false;
    }
    let Some(arch) = manifest.supported_arch(o.arch, windows_11()) else {
        ctx.warn("install.no_arch", &[("app", &app)]);
        return Ok(None);
    };
    let line = ctx.t.fill("install.start", &[("app", &app), ("version", &version), ("arch", arch.name()), ("bucket", &bucket)]);
    ctx.say(line);

    let version_dir = ctx.scoop.app(&app, o.global).join(&version);
    std::fs::create_dir_all(&version_dir).map_err(|e| ctx.fail("error.remove", &[("why", &e.to_string())]))?;
    let original = env::absolute(&version_dir);
    let persist_dir = ctx.scoop.persist(&app, o.global);

    let placed = (|| -> Result<PathBuf, i32> {
        // The files, from the cache into the version folder.
        let Some(cached) = to_cache(ctx, &app, &version, &manifest, arch, o.use_cache, check_hash, aria2_program(ctx).as_deref()) else {
            return Err(1);
        };
        let mut names = Vec::new();
        for (url, file) in &cached {
            let name = file_name(url).to_string();
            let to = original.join(&name);
            let copied = if o.use_cache { std::fs::copy(file, &to).map(|_| ()) } else { std::fs::rename(file, &to) };
            copied.map_err(|e| ctx.fail("error.remove", &[("why", &format!("{}: {e}", to.display()))]))?;
            names.push(name);
        }

        let variables = setup(&app, &bucket, &version, arch, &names, o);
        let dir = match scripts {
            Some(host) => {
                host.set(ctx, &variables)?;
                link_in(ctx, host, &app, &bucket, &version, &manifest, arch, &original, &persist_dir, o.global)?
            }
            None => {
                let mut host = Scripts::new(&INSTALL_LIBS, variables);
                let done = link_in(ctx, &mut host, &app, &bucket, &version, &manifest, arch, &original, &persist_dir, o.global);
                host.finish();
                done?
            }
        };

        // `save_installed_manifest` and `save_install_info`.
        let saved = dir.join("scoop-manifest.json");
        std::fs::copy(&path, &saved).map_err(|e| ctx.fail("error.remove", &[("why", &format!("{}: {e}", saved.display()))]))?;
        let mut info = Vec::new();
        if !bucket.is_empty() {
            info.push(("bucket".to_string(), json::string(bucket.clone())));
        }
        info.push(("architecture".to_string(), json::string(arch.name())));
        let text = format!("{}\r\n", json::pretty(&Json::Obj(info)).replace('\n', "\r\n"));
        let file = dir.join("scoop-install.json");
        std::fs::write(&file, text).map_err(|e| ctx.fail("error.remove", &[("why", &format!("{}: {e}", file.display()))]))?;
        Ok(dir)
    })();
    // Scoop leaves a failed installation for the next `install` to purge;
    // here it goes at once.
    let dir = match placed {
        Ok(dir) => dir,
        Err(code) => {
            roll_back(ctx, &app, &original, &manifest, arch, o.global);
            return Err(code);
        }
    };

    let line = ctx.t.fill("install.done", &[("app", &app), ("version", &version)]);
    ctx.say(line);
    let notes = rt_scoop::manifest::strings(manifest.get("notes")).join("\n");
    if !notes.is_empty() {
        let notes = shortcuts::substitute(&notes, &dir, &original, &persist_dir);
        ctx.say("Notes");
        ctx.say("-----");
        ctx.say(notes);
        ctx.say("-----");
    }
    let suggest = manifest.get("suggest");
    Ok((suggest != &Json::Null).then(|| (app, suggest.clone())))
}

/// From the extraction to `post_install`; returns the folder the app is
/// used from (`current`).
#[allow(clippy::too_many_arguments)]
fn link_in(
    ctx: &mut Ctx,
    host: &mut Scripts,
    app: &str,
    bucket: &str,
    version: &str,
    manifest: &Manifest,
    arch: Arch,
    original: &Path,
    persist_dir: &Path,
    global: bool,
) -> Result<PathBuf, i32> {
    host.run(ctx, "Invoke-Extraction -Path $dir -Name $fname -Manifest $manifest -ProcessorArchitecture $architecture")?;
    host.hook(ctx, "pre_install")?;
    host.run(
        ctx,
        "Invoke-Installer -Path $dir -Name $fname -Manifest $manifest -ProcessorArchitecture $architecture -AppName $app -Global:$global",
    )?;
    let fail = |ctx: &mut Ctx, why: String| ctx.fail("error.config", &[("why", &why)]);

    match env::ensure_dir_not_in_path(ctx.system, original, global) {
        Ok((removed, machine)) => {
            for p in removed {
                let line = ctx.t.fill("install.installer_path", &[("path", &ctx.friendly(Path::new(&p)))]);
                ctx.say(line);
            }
            for p in machine {
                ctx.warn("install.installer_system_path", &[("path", &p)]);
            }
        }
        Err(why) => return Err(fail(ctx, why)),
    }
    if !ctx.scoop.enabled("no_junction") {
        let current = original.parent().unwrap_or(original).join("current");
        let line = ctx.t.fill("link.linking", &[("from", &ctx.friendly(&current)), ("to", &ctx.friendly(original))]);
        ctx.say(line);
    }
    let dir = links::link_current(&ctx.scoop, original).map_err(|why| fail(ctx, why))?;
    host.run(ctx, &format!("$dir = {}", quote(&dir.display().to_string())))?;
    create_shims(ctx, manifest, [&dir, original, persist_dir], global, arch)?;
    for made in shortcuts::create(ctx.system, manifest, [&dir, original, persist_dir], global, arch) {
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
    match links::install_psmodule(&ctx.scoop, ctx.system, manifest, &dir, global) {
        Ok(Some((link, replaced, changes))) => {
            for change in changes {
                if let env::Change::Added(p) = change {
                    let line =
                        ctx.t.fill(if global { "module.path_global" } else { "module.path" }, &[("path", &ctx.friendly(Path::new(&p)))]);
                    ctx.say(line);
                }
            }
            let name = links::module_name(manifest).unwrap_or_default();
            let line = ctx.t.fill("module.installing", &[("name", &name)]);
            ctx.say(line);
            if replaced {
                ctx.warn("module.replaced", &[("path", &ctx.friendly(&link))]);
            }
            let line = ctx.t.fill("link.linking", &[("from", &ctx.friendly(&link)), ("to", &ctx.friendly(&dir))]);
            ctx.say(line);
        }
        Ok(None) => {}
        Err(why) => return Err(fail(ctx, why)),
    }
    let changes = env::env_add_path(ctx.system, &ctx.scoop, manifest, &dir, global, arch).map_err(|why| fail(ctx, why))?;
    say_changes(ctx, &changes, global);

    // `env_set`: here when the values are plain, else in Scoop's code.
    let env_lookup = |name: &str| ctx.env.get(name).map(String::from);
    let values: Option<Vec<(String, String)>> = env::env_set_values(manifest, arch)
        .into_iter()
        .map(|(name, value)| {
            let v = vars(app, bucket, version, [&dir, original, persist_dir], arch, global, &name, &env_lookup);
            env::expand(&value, &v).map(|e| (name, e))
        })
        .collect();
    match values {
        Some(values) => {
            let changes = env::env_set(ctx.system, &values, global).map_err(|why| fail(ctx, why))?;
            say_changes(ctx, &changes, global);
        }
        None => host.run(ctx, "env_set $manifest $global $architecture")?,
    }

    match links::persist_data(manifest, &dir, persist_dir) {
        Ok(names) => {
            for name in names {
                let line = ctx.t.fill("persist.persisting", &[("name", &name)]);
                ctx.say(line);
            }
        }
        Err(why) => ctx.warn("error.config", &[("why", &why)]),
    }
    links::persist_permission(&ctx.scoop, manifest, global, is_admin());
    host.hook(ctx, "post_install")?;
    Ok(dir)
}

/// What a failed installation left, gone as Scoop's `uninstall` of it would
/// take it (without its scripts, which did not finish): shims (another
/// app's come back), shortcuts, `current` if it points here, module, `PATH`,
/// variables, the persisted links and the version folder. The persisted
/// data stays.
pub fn roll_back(ctx: &mut Ctx, app: &str, version_dir: &Path, manifest: &Manifest, arch: Arch, global: bool) {
    ctx.warn("install.rolling_back", &[("app", app)]);
    remove_shims(ctx, app, manifest, global, arch);
    shortcuts::remove(ctx.system, manifest, global, arch);
    let app_dir = version_dir.parent().unwrap_or(version_dir).to_path_buf();
    let current = app_dir.join("current");
    let ours = std::fs::read_link(&current).is_ok_and(|t| env::absolute(&t) == env::absolute(version_dir));
    if ours {
        let _ = links::unlink_current(&ctx.scoop, version_dir);
    }
    let refdir = if ctx.scoop.enabled("no_junction") { version_dir.to_path_buf() } else { current };
    let module_here = links::module_name(manifest).is_some_and(|n| {
        let link = links::modules_dir(&ctx.scoop, global).join(n);
        std::fs::read_link(&link)
            .is_ok_and(|t| env::absolute(&t) == env::absolute(&refdir) || env::absolute(&t) == env::absolute(version_dir))
    });
    if module_here {
        links::uninstall_psmodule(&ctx.scoop, manifest, global);
    }
    let _ = env::env_rm_path(ctx.system, &ctx.scoop, manifest, &refdir, global, arch);
    let _ = env::env_rm(ctx.system, manifest, global, arch);
    links::unlink_persist_data(manifest, version_dir);
    if let Err(e) = remove_tree(version_dir) {
        ctx.warn("error.remove", &[("why", &e.to_string())]);
    }
    if std::fs::read_dir(&app_dir).is_ok_and(|mut d| d.next().is_none()) {
        let _ = remove_tree(&app_dir);
    }
}
