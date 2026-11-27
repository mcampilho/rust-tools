//! Questions about one app: `info`, `cat`, `which`, `prefix`, `depends`, `home`.

use super::{table, Ctx};
use crate::checks::{parse_shim, shim_kind};
use crate::sys::{local_time, open_url};
use rt_scoop::apps;
use rt_scoop::buckets;
use rt_scoop::json::{self, object, string, Json};
use rt_scoop::manifest::{strings, truthy, Arch, Manifest};
use rt_scoop::resolve::{self, DependencyError, Resolved};
use std::path::{Path, PathBuf};

/// The app of the first argument with its manifest, or the error shown.
fn found(ctx: &mut Ctx, args: &[String]) -> Result<(String, Resolved, Manifest), i32> {
    let Some(query) = args.iter().find(|a| !a.starts_with('-')) else {
        return Err(ctx.fail("error.missing", &[("what", "<app>")]));
    };
    let mut r = resolve::resolve(&ctx.scoop, query);
    if !r.also_in.is_empty() {
        let chosen = format!("{}/{}", r.bucket.as_deref().unwrap_or(""), r.app);
        let app = r.app.clone();
        ctx.warn("warn.buckets", &[("app", &app), ("chosen", &chosen)]);
    }
    match r.manifest.take() {
        Some(m) => Ok((query.clone(), r, m)),
        None if r.url.as_deref().is_some_and(resolve::is_remote) => Err(ctx.fail("error.remote", &[("app", query)])),
        None => Err(ctx.fail("error.no_manifest", &[("app", query)])),
    }
}

pub fn cat(ctx: &mut Ctx, args: &[String]) -> i32 {
    match found(ctx, args) {
        Ok((_, _, manifest)) => {
            ctx.print_json(&manifest.0);
            0
        }
        Err(code) => code,
    }
}

pub fn home(ctx: &mut Ctx, args: &[String]) -> i32 {
    let (query, _, manifest) = match found(ctx, args) {
        Ok(f) => f,
        Err(code) => return code,
    };
    let url = manifest.get("homepage").str();
    // Only web pages: a manifest must not be able to start a program here.
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return ctx.fail("error.no_homepage", &[("app", &query)]);
    }
    if open_url(url) {
        0
    } else {
        ctx.fail("error.start", &[("what", url), ("why", "ShellExecute")])
    }
}

pub fn prefix(ctx: &mut Ctx, args: &[String]) -> i32 {
    let Some(app) = args.first() else { return ctx.fail("error.missing", &[("what", "<app>")]) };
    let dir = [false, true].iter().map(|&g| apps::current_dir(&ctx.scoop, app, g)).find(|p| p.is_dir());
    match dir {
        Some(dir) => {
            let text = dir.display().to_string();
            if ctx.json {
                ctx.print_json(&string(text));
            } else {
                ctx.say(text);
            }
            0
        }
        None => ctx.fail("error.no_path", &[("app", app)]),
    }
}

/// The program a shim in `shims` starts.
fn shim_target(shims: &Path, name: &str) -> Option<PathBuf> {
    ["shim", "ps1", "cmd", "bash"].iter().find_map(|kind| {
        let file = if *kind == "bash" { shims.join(name) } else { shims.join(format!("{name}.{kind}")) };
        let text = json::read_text(&file)?;
        let (target, _) = parse_shim(kind, &text, &shims.display().to_string());
        Some(PathBuf::from(target?.shown()))
    })
}

/// `Get-CommandPath`, without PowerShell's aliases and functions.
/// The file `Get-Command` finds for a program name on `PATH` (`.ps1` first,
/// then `PATHEXT`). Aliases and functions of PowerShell are not known here.
pub(super) fn find_command(ctx: &Ctx, command: &str) -> Option<PathBuf> {
    let extensions: Vec<String> = std::iter::once(".ps1".to_string())
        .chain(ctx.env.get("PATHEXT").unwrap_or(".COM;.EXE;.BAT;.CMD").split(';').filter(|e| !e.is_empty()).map(str::to_ascii_lowercase))
        .collect();
    let has_extension = Path::new(command).extension().is_some();
    ctx.env.path().find_map(|dir| {
        let exact = has_extension.then(|| dir.join(command)).filter(|p| p.is_file());
        exact.or_else(|| extensions.iter().map(|e| dir.join(format!("{command}{e}"))).find(|p| p.is_file()))
    })
}

pub fn which(ctx: &mut Ctx, args: &[String]) -> i32 {
    let Some(command) = args.first() else { return ctx.fail("error.missing", &[("what", "<command>")]) };
    let file = find_command(ctx, command);
    let found = file.and_then(|file| {
        let dir = file.parent()?;
        let same = |a: &Path, b: &Path| a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy());
        if [false, true].iter().any(|&g| same(dir, &ctx.scoop.shims(g))) {
            let name = file.file_name()?.to_string_lossy().into_owned();
            let base = shim_kind(&name).map_or(name.clone(), |(base, _, _)| base);
            shim_target(dir, &base).filter(|t| t.exists())
        } else {
            Some(file)
        }
    });
    match found {
        Some(path) => {
            let text = ctx.friendly(&path);
            if ctx.json {
                ctx.print_json(&string(path.display().to_string()));
            } else {
                ctx.say(text);
            }
            0
        }
        None => {
            ctx.warn("which.none", &[("command", command)]);
            2
        }
    }
}

pub fn depends(ctx: &mut Ctx, args: &[String]) -> i32 {
    let mut arch = ctx.arch();
    let mut app = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-a" | "--arch" => {
                i += 1;
                let value = args.get(i).map(String::as_str).unwrap_or("");
                match Arch::parse(value) {
                    Some(a) => arch = a,
                    None => return ctx.fail("error.arch", &[("arch", value)]),
                }
            }
            _ => app = app.or(Some(args[i].clone())),
        }
        i += 1;
    }
    let Some(app) = app else { return ctx.fail("error.missing", &[("what", "<app>")]) };
    let deps = match resolve::dependencies(&ctx.scoop, &app, arch) {
        Ok(d) => d,
        Err(DependencyError::NoManifest(name)) => return ctx.fail("error.no_manifest", &[("app", &name)]),
        Err(DependencyError::Circular(from, to)) => return ctx.fail("error.circular", &[("from", &from), ("to", &to)]),
    };
    let rows: Vec<(String, String)> = deps
        .iter()
        .map(|d| match d.split_once('/') {
            Some((bucket, name)) if !resolve::is_remote(d) && !d.contains('\\') => (bucket.to_string(), name.to_string()),
            _ => (d.clone(), resolve::resolve(&ctx.scoop, d).app),
        })
        .collect();
    if ctx.json {
        let list = rows.iter().map(|(s, n)| object([("Source", string(s.clone())), ("Name", string(n.clone()))])).collect();
        ctx.print_json(&Json::Arr(list));
    } else {
        let cells: Vec<Vec<String>> = rows.into_iter().map(|(s, n)| vec![s, n]).collect();
        for line in table::lines(&["Source", "Name"], &cells) {
            ctx.say(line);
        }
    }
    0
}

/// The size of every file under `path`, without going into links.
fn tree_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return 0;
    }
    std::fs::read_dir(path).into_iter().flatten().flatten().map(|e| tree_size(&e.path())).sum()
}

fn license(manifest: &Manifest, verbose: bool) -> String {
    let license = manifest.get("license");
    let spdx = |id: &str| format!("https://spdx.org/licenses/{id}.html");
    let text = license.str();
    if text.is_empty() {
        let (id, url) = (license.get("identifier").str(), license.get("url").str());
        return if verbose && !url.is_empty() { format!("{id} ({url})") } else { id.to_string() };
    }
    if !verbose || resolve::is_remote(text) {
        return text.to_string();
    }
    let urls: Vec<String> = text.split(['|', ',']).map(spdx).collect();
    format!("{text} ({})", urls.join(", "))
}

/// The last commit of a manifest: (date, author).
fn last_change(ctx: &Ctx, file: &Path) -> Option<(String, String)> {
    let git = resolve::helper(&ctx.scoop, "git", r"mingw64\bin\git.exe").unwrap_or_else(|| PathBuf::from("git.exe"));
    let out = std::process::Command::new(git)
        .arg("-C")
        .arg(file.parent()?)
        .args(["log", "-1", "-s", "--format=%aI#%an"])
        .arg(file)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (date, author) = text.trim().split_once('#')?;
    // 2026-11-27T10:20:30+00:00 -> 2026-11-27 10:20:30
    Some((date.get(..19)?.replace('T', " "), author.to_string()))
}

pub fn info(ctx: &mut Ctx, args: &[String]) -> i32 {
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    let (_, r, manifest) = match found(ctx, args) {
        Ok(f) => f,
        Err(code) => return code,
    };
    let app = r.app.clone();
    let scoop = ctx.scoop.clone();
    let global = apps::installed(&scoop, &app, Some(true));
    let version = apps::current_version(&scoop, &app, global);
    let installed = version.is_some();
    let install = version.as_deref().and_then(|v| apps::install_info(&scoop, &app, v, global)).unwrap_or_default();
    let arch = Arch::parse(&install.architecture).unwrap_or_else(|| ctx.arch());
    let bucket = install.bucket.clone().or_else(|| r.bucket.clone());
    let manifest_file = install.url.clone().map(PathBuf::from).or_else(|| r.path.clone());
    let deprecated = bucket.as_deref().and_then(|b| buckets::deprecated(&scoop, &app, b));
    let (dir, original_dir, persist_dir) = if verbose {
        let shown = |p: PathBuf| p.display().to_string();
        (
            shown(apps::current_dir(&scoop, &app, global)),
            shown(scoop.app(&app, global).join(manifest.version())),
            shown(scoop.persist(&app, global)),
        )
    } else {
        ("<root>".to_string(), "<root>".to_string(), "<root>".to_string())
    };
    let substitute = |s: &str| s.replace("$original_dir", &original_dir).replace("$persist_dir", &persist_dir).replace("$dir", &dir);

    let mut items: Vec<(String, String)> = Vec::new();
    let mut add = |key: &str, value: String| {
        if !value.is_empty() {
            items.push((key.to_string(), value));
        }
    };
    add("Name", if deprecated.is_some() { format!("{app} (DEPRECATED)") } else { app.clone() });
    add("Description", manifest.get("description").str().to_string());
    add(
        "Version",
        match &version {
            Some(v) if v != manifest.version() => ctx.t.fill("info.update", &[("version", v), ("latest", manifest.version())]),
            _ => manifest.version().to_string(),
        },
    );
    add("Source", bucket.clone().or_else(|| r.url.clone()).unwrap_or_default());
    add("Website", manifest.get("homepage").str().trim_end_matches('/').to_string());
    add("License", license(&manifest, verbose));
    add("Dependencies", manifest.depends().join(" | "));
    if let Some(file) = deprecated.as_ref().or(manifest_file.as_ref()).filter(|p| p.is_file()) {
        let (when, who) = last_change(ctx, file).unwrap_or_else(|| {
            let time = std::fs::metadata(file).and_then(|m| m.modified()).ok().map(|t| local_time(t).short);
            (time.unwrap_or_default(), String::new())
        });
        add("Updated at", when);
        add("Updated by", who);
        if verbose {
            add("Manifest", file.display().to_string());
        }
    }
    if installed {
        let versions = apps::installed_versions(&scoop, &app, global);
        let shown: Vec<String> = versions
            .iter()
            .map(|v| {
                if verbose {
                    scoop.app(&app, global).join(v).display().to_string()
                } else if global {
                    format!("{v} *global*")
                } else {
                    v.clone()
                }
            })
            .collect();
        add("Installed", shown.join("\n"));
        if verbose {
            let all = tree_size(&scoop.app(&app, global));
            let current = version.as_deref().map_or(0, |v| tree_size(&scoop.app(&app, global).join(v)));
            let persisted = tree_size(&scoop.persist(&app, global));
            let prefix = format!("{}#", app.to_ascii_lowercase());
            let cached: u64 = std::fs::read_dir(&scoop.cache)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().to_ascii_lowercase().starts_with(&prefix))
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum();
            let old = all.saturating_sub(current);
            let size = if persisted + cached + old == 0 {
                table::filesize(current)
            } else {
                [
                    ("size.current", current),
                    ("size.old", old),
                    ("size.persisted", persisted),
                    ("size.cached", cached),
                    ("size.total", all + persisted + cached),
                ]
                .iter()
                .filter(|(_, n)| *n > 0)
                .map(|(key, n)| format!("{}: {}", ctx.t.get(key), table::filesize(*n)))
                .collect::<Vec<_>>()
                .join("\n")
            };
            add("Installed size", size);
        }
    }
    let binaries: Vec<String> = manifest
        .bins(arch)
        .into_iter()
        .map(|b| match b.alias {
            Some(alias) => format!("{alias}.{}", b.target.rsplit('.').next().unwrap_or("")),
            None => b.target,
        })
        .collect();
    add("Binaries", binaries.join(" | "));
    add("Shortcuts", manifest.shortcut_names(arch).join(" | "));
    if let Json::Obj(vars) = manifest.arch("env_set", arch) {
        add("Environment", vars.iter().map(|(k, v)| format!("{k} = {}", substitute(v.str()))).collect::<Vec<_>>().join("\n"));
    }
    let added: Vec<String> =
        strings(manifest.arch("env_add_path", arch)).iter().map(|p| if p == "." { dir.clone() } else { format!("{dir}\\{p}") }).collect();
    add("Path Added", added.join("\n"));
    if let Json::Obj(suggestions) = manifest.get("suggest") {
        add("Suggestions", suggestions.iter().flat_map(|(_, v)| strings(v)).collect::<Vec<_>>().join(" | "));
    }
    if truthy(manifest.get("notes")) {
        add("Notes", strings(manifest.get("notes")).iter().map(|n| substitute(n)).collect::<Vec<_>>().join("\n"));
    }

    if ctx.json {
        let members = items.into_iter().map(|(k, v)| (k, string(v))).collect();
        ctx.print_json(&Json::Obj(members));
    } else {
        for line in table::pairs(&items) {
            ctx.say(line);
        }
    }
    0
}
