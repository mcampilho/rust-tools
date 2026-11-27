//! `download` (`libexec\scoop-download.ps1`, `Invoke-CachedDownload`,
//! `Invoke-CachedAria2Download`): the files of apps into the cache, checked.

use super::args::{getopt, Spec};
use super::table::filesize;
use super::{delegate, update, Ctx};
use crate::sys::{today, windows_11};
use rt_scoop::download::{cache_path, check_hash, expected_hash, file_name, headers, needs_scoop, request_url, HashError};
use rt_scoop::manifest::{Arch, Manifest};
use rt_scoop::resolve::{self, parse_app};
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

const SPEC: [Spec; 4] = [('f', "force", false), ('s', "skip-hash-check", false), ('u', "no-update-scoop", false), ('a', "arch", true)];

/// What is sent as the browser. Some hosts (SourceForge) send the file to a
/// downloader and a web page to anything else.
fn agent() -> String {
    format!("Scoop/1.0 (+http://scoop.sh/) rscoop-cli/{} (Windows NT 10.0; Win64; x64)", env!("CARGO_PKG_VERSION"))
}

/// `Get-GitHubToken`.
fn github_token(ctx: &Ctx) -> Option<String> {
    let configured = Some(ctx.scoop.setting("gh_token").str()).filter(|t| !t.is_empty()).map(String::from);
    ctx.env
        .get("SCOOP_GH_TOKEN")
        .map(String::from)
        .or(configured)
        .or_else(|| ["GH_TOKEN", "GITHUB_TOKEN"].iter().find_map(|n| ctx.env.get(n).map(String::from)))
}

pub(super) struct Job {
    pub app: String,
    pub bucket: Option<String>,
    pub manifest: Manifest,
}

/// The app of one query, or why Scoop has to do the whole command (None).
pub(super) fn job(ctx: &Ctx, query: &str) -> Option<Result<Job, &'static str>> {
    let wanted = parse_app(query);
    let r = resolve::resolve(
        &ctx.scoop,
        &match &wanted.bucket {
            Some(b) => format!("{b}/{}", wanted.app),
            None => wanted.app.clone(),
        },
    );
    if r.url.as_deref().is_some_and(resolve::is_remote) {
        return None;
    }
    let Some(manifest) = r.manifest else { return Some(Err("error.no_manifest")) };
    // Another version has to be found in the history or generated.
    if wanted.version.is_some_and(|v| v != manifest.version()) {
        return None;
    }
    let token = github_token(ctx).is_some();
    let every_url = [Arch::X64, Arch::X86, Arch::Arm64].iter().flat_map(|&a| manifest.urls(a)).collect::<Vec<_>>();
    let special = every_url.iter().any(|u| {
        needs_scoop(&ctx.scoop, u)
            || (token && u.contains("github.com/") && u.contains("/releases/download/"))
            || u.ends_with(".meta4")
            || u.ends_with(".metalink")
    });
    (!special).then_some(Ok(Job { app: r.app, bucket: r.bucket, manifest }))
}

/// One file into `to`, through `to.download`.
fn fetch(ctx: &mut Ctx, url: &str, to: &Path, manifest: &Manifest) -> Result<(), String> {
    let (scoop, token) = (ctx.scoop.clone(), github_token(ctx));
    let sent = |address: &str| headers(&scoop, manifest, address, token.as_deref());
    let mut partial = to.as_os_str().to_os_string();
    partial.push(".download");
    let partial = PathBuf::from(partial);
    let name = file_name(url).to_string();
    let terminal = std::io::stderr().is_terminal();
    let mut last = 0;
    let result = rt_sys::net::download(&request_url(url), &partial, &sent, &agent(), &mut |done, total| {
        if terminal && total > 0 {
            let percent = done * 100 / total;
            if percent != last {
                last = percent;
                let _ = write!(std::io::stderr(), "\r{name} ({}) {percent:>3}%", filesize(total));
            }
        }
    });
    if terminal && last > 0 {
        let _ = writeln!(ctx.err);
    }
    match result {
        Ok(size) => {
            if !terminal {
                let line = ctx.t.fill("download.got", &[("name", &name), ("size", &filesize(size))]);
                ctx.say(line);
            }
            std::fs::rename(&partial, to).map_err(|e| format!("{}: {e}", to.display()))
        }
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            Err(e)
        }
    }
}

/// Downloads with aria2, with the options Scoop gives it; false if it failed.
fn aria2(ctx: &mut Ctx, program: &Path, app: &str, files: &[(String, PathBuf)], manifest: &Manifest) -> bool {
    let list = ctx.scoop.cache.join(format!("{app}.txt"));
    let mut text = String::new();
    for (url, cached) in files {
        text += &format!("{}\n", request_url(url));
        if !url.contains("sourceforge.net") {
            text += &format!("    referer={}\n", &url[..url.len() - url.rsplit('/').next().unwrap_or("").len()]);
        }
        let name = cached.file_name().unwrap_or_default().to_string_lossy();
        text += &format!("    dir={}\n    out={name}\n", ctx.scoop.cache.display());
    }
    if std::fs::write(&list, text).is_err() {
        return false;
    }
    let setting = |name: &str, default: &str| match ctx.scoop.setting(name) {
        rt_scoop::json::Json::Num(n) => n.to_string(),
        v if !v.str().is_empty() => v.str().to_string(),
        _ => default.to_string(),
    };
    let mut command = std::process::Command::new(program);
    command
        .arg(format!("--input-file={}", list.display()))
        .arg(format!("--user-agent={}", agent()))
        .args(["--allow-overwrite=true", "--auto-file-renaming=false"])
        .arg(format!("--retry-wait={}", setting("aria2-retry-wait", "2")))
        .arg(format!("--split={}", setting("aria2-split", "5")))
        .arg(format!("--max-connection-per-server={}", setting("aria2-max-connection-per-server", "5")))
        .arg(format!("--min-split-size={}", setting("aria2-min-split-size", "5M")))
        .args(["--console-log-level=warn", "--enable-color=false", "--no-conf=true", "--follow-metalink=true"])
        .args(["--metalink-preferred-protocol=https", "--min-tls-version=TLSv1.2", "--continue", "--summary-interval=0"])
        .arg(format!("--stop-with-process={}", std::process::id()))
        .arg("--auto-save-interval=1");
    if let rt_scoop::json::Json::Obj(cookies) = manifest.get("cookie") {
        let value: Vec<String> = cookies.iter().map(|(k, v)| format!("{k}={}", v.str())).collect();
        command.arg(format!("--header=Cookie: {}", value.join(";")));
    }
    for option in rt_scoop::manifest::strings(ctx.scoop.setting("aria2-options")) {
        command.arg(option);
    }
    let _ = ctx.out.flush();
    let ok = command.status().is_ok_and(|s| s.success());
    if ok {
        let _ = std::fs::remove_file(&list);
    }
    ok
}

/// `<file>.aria2`: what aria2 keeps while a download is unfinished.
fn control_file(cached: &Path) -> PathBuf {
    let mut name = cached.as_os_str().to_os_string();
    name.push(".aria2");
    PathBuf::from(name)
}

/// Why a hash did not match, as one text.
fn hash_message(ctx: &Ctx, e: &HashError) -> String {
    match e {
        HashError::UnknownKind(kind) => ctx.t.fill("download.hash_kind", &[("kind", kind)]),
        HashError::Read(why) => why.clone(),
        HashError::Differs(expected, actual) => ctx.t.fill("download.hash_differs", &[("expected", expected), ("actual", actual)]),
    }
}

pub fn run(ctx: &mut Ctx, args: &[String]) -> i32 {
    let opts = match getopt(args, &SPEC) {
        Ok(o) => o,
        Err(option) => return ctx.fail("cli.unknown_option", &[("opt", &option)]),
    };
    let mut apps: Vec<String> = Vec::new();
    for app in &opts.rest {
        if !apps.contains(app) {
            apps.push(app.clone());
        }
    }
    if apps.is_empty() {
        return ctx.fail("error.missing", &[("what", "<app>")]);
    }
    let arch = match opts.value('a') {
        Some(a) => match Arch::parse(a) {
            Some(a) => a,
            None => return ctx.fail("error.arch", &[("arch", a)]),
        },
        None => ctx.arch(),
    };
    // Settled before anything is done: what only Scoop can do goes to Scoop.
    let proxy = !matches!(ctx.scoop.setting("proxy").str(), "" | "default");
    if proxy || apps.iter().any(|a| job(ctx, a).is_none()) {
        let mut line = vec!["download".to_string()];
        line.extend(args.iter().cloned());
        return delegate::run(ctx, &line, ctx.root_given);
    }
    // As Scoop: an old installation is updated first, unless `-u`.
    if update::outdated(ctx) && (opts.has('u') || update::sync(ctx, &[]) != 0) {
        ctx.warn("download.outdated", &[]);
    }
    let use_cache = !opts.has('f');
    if !use_cache {
        ctx.warn("download.no_cache", &[]);
    }
    if let Err(e) = std::fs::create_dir_all(&ctx.scoop.cache) {
        let why = e.to_string();
        return ctx.fail("error.remove", &[("why", &why)]);
    }
    let program = aria2_program(ctx);

    let mut failed = false;
    for query in &apps {
        let job = match job(ctx, query) {
            Some(Ok(j)) => j,
            Some(Err(key)) => {
                failed = true;
                ctx.warn(key, &[("app", query)]);
                continue;
            }
            None => continue,
        };
        failed |= !one(ctx, &job, arch, use_cache, !opts.has('s'), program.as_deref());
    }
    i32::from(failed)
}

fn one(ctx: &mut Ctx, job: &Job, arch: Arch, use_cache: bool, mut verify: bool, program: Option<&Path>) -> bool {
    let app = job.app.clone();
    let mut version = job.manifest.version().to_string();
    if version.is_empty() || !version.chars().all(|c| c.is_alphanumeric() || ".-+_".contains(c)) {
        ctx.warn("download.bad_version", &[("app", &app), ("version", &version)]);
        return false;
    }
    if version == "nightly" {
        version = format!("nightly-{}", today());
        verify = false;
    }
    let Some(arch) = job.manifest.supported_arch(arch, windows_11()) else {
        ctx.warn("download.no_arch", &[("app", &app)]);
        return false;
    };
    let from = job.bucket.clone().unwrap_or_default();
    let line = ctx.t.fill("download.start", &[("app", &app), ("arch", arch.name()), ("bucket", &from)]);
    ctx.say(line);
    let good = to_cache(ctx, &app, &version, &job.manifest, arch, use_cache, verify, program).is_some();
    if good {
        let line = ctx.t.fill("download.done", &[("app", &app), ("version", &version)]);
        ctx.say(line);
    }
    good
}

/// The aria2 Scoop would use, if any.
pub(super) fn aria2_program(ctx: &Ctx) -> Option<PathBuf> {
    resolve::helper(&ctx.scoop, "aria2", "aria2c.exe").filter(|_| ctx.scoop.setting("aria2-enabled") != &rt_scoop::json::Json::Bool(false))
}

/// The files of `manifest` into the cache (`Invoke-CachedDownload`, or aria2),
/// with their hashes checked: (address, file in the cache) for each one, or
/// None when one failed (the reason was shown).
#[allow(clippy::too_many_arguments)]
pub(super) fn to_cache(
    ctx: &mut Ctx,
    app: &str,
    version: &str,
    manifest: &Manifest,
    arch: Arch,
    use_cache: bool,
    verify: bool,
    program: Option<&Path>,
) -> Option<Vec<(String, PathBuf)>> {
    if let Err(e) = std::fs::create_dir_all(&ctx.scoop.cache) {
        ctx.warn("error.remove", &[("why", &format!("{}: {e}", ctx.scoop.cache.display()))]);
        return None;
    }
    let files: Vec<(String, PathBuf)> =
        manifest.urls(arch).into_iter().map(|u| (u.clone(), cache_path(&ctx.scoop.cache, app, version, &u))).collect();
    // A control file of aria2 beside one means it is only half there.
    let mut wanted: Vec<(String, PathBuf)> =
        files.iter().filter(|(_, c)| !use_cache || !c.is_file() || control_file(c).is_file()).cloned().collect();
    for (url, _) in files.iter().filter(|f| !wanted.contains(f)) {
        let line = ctx.t.fill("download.cached", &[("name", file_name(url))]);
        ctx.say(line);
    }
    if let Some(program) = program.filter(|_| !wanted.is_empty()) {
        if aria2(ctx, program, app, &wanted, manifest) {
            wanted.clear();
        } else if ctx.scoop.setting("aria2-fallback-enabled") == &rt_scoop::json::Json::Bool(false) {
            // What is half downloaded must not pass for a download.
            for (_, cached) in wanted.iter().filter(|(_, c)| control_file(c).is_file()) {
                let _ = std::fs::remove_file(cached);
                let _ = std::fs::remove_file(control_file(cached));
            }
            ctx.warn("download.aria2_failed", &[("app", app)]);
            return None;
        } else {
            ctx.warn("download.aria2_fallback", &[]);
        }
    }
    for (url, cached) in &wanted {
        if let Err(why) = fetch(ctx, url, cached, manifest) {
            ctx.warn("download.failed", &[("url", url), ("why", &why)]);
            return None;
        }
        let _ = std::fs::remove_file(control_file(cached));
    }
    let mut good = true;
    for (url, cached) in &files {
        if !verify {
            let line = ctx.t.get("download.no_check").to_string();
            ctx.say(line);
            continue;
        }
        let name = file_name(url).to_string();
        let Some(expected) = expected_hash(manifest, arch, url) else {
            ctx.warn("download.no_hash", &[("name", &name)]);
            continue;
        };
        match check_hash(cached, &expected) {
            Ok(()) => {
                let line = ctx.t.fill("download.hash_ok", &[("name", &name)]);
                ctx.say(line);
            }
            Err(e) => {
                let why = hash_message(ctx, &e);
                ctx.warn("download.hash_failed", &[("app", app), ("url", url), ("why", &why)]);
                let _ = std::fs::remove_file(cached);
                if url.contains("sourceforge.net") {
                    ctx.warn("download.sourceforge", &[]);
                }
                good = false;
            }
        }
    }
    good.then_some(files)
}
