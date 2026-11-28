//! `update` of Scoop and of the buckets (`libexec\scoop-update.ps1`:
//! `Sync-Scoop`, `Sync-Bucket`) and the part of `status` that asks the
//! remotes. Updating apps is still Scoop's.
//!
//! Cases that would remove or replace something are handed to Scoop as they
//! are: a Scoop or a main bucket that is not a git repository, another
//! repository or branch configured, the SQLite cache.

use super::git::Git;
use super::{delegate, Ctx};
use crate::checks::parallel;
use crate::sys::{local_time, utc_offset};
use rt_scoop::buckets;
use rt_scoop::config::{parse_time, set_member};
use rt_scoop::disk::remove_tree;
use rt_scoop::json::{string, Json};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const REPOSITORY: &str = "https://github.com/ScoopInstaller/Scoop";
const BRANCH: &str = "master";

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Saves a setting and keeps it in what was read.
pub fn set_setting(ctx: &mut Ctx, name: &str, value: Option<Json>) -> Result<(), i32> {
    if let Err(why) = set_member(&ctx.scoop.config_path, name, value) {
        return Err(ctx.fail("error.config", &[("why", &why)]));
    }
    ctx.scoop.config = rt_scoop::json::read(&ctx.scoop.config_path).unwrap_or(Json::Obj(Vec::new()));
    Ok(())
}

/// `is_scoop_outdated`: three hours since the last update.
pub fn outdated(ctx: &mut Ctx) -> bool {
    match parse_time(ctx.scoop.setting("last_update").str(), utc_offset()) {
        Some(last) => now() - last >= 3 * 3600,
        None => {
            let _ = set_setting(ctx, "last_update", Some(string(local_time(SystemTime::now()).iso)));
            true
        }
    }
}

fn core_dir(ctx: &Ctx) -> PathBuf {
    ctx.scoop.app("scoop", false).join("current")
}

fn is_repository(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// The commits that came, without the routine ones (`Invoke-GitLog`).
fn log(git: &Git, dir: &Path, name: &str, from: &str) -> String {
    let name = match name {
        "" => String::new(),
        n => {
            let short: String = if n.chars().count() > 12 { n.chars().take(10).chain("..".chars()).collect() } else { n.to_string() };
            format!("%Cgreen{short:<12}%Creset ")
        }
    };
    let format = format!("--format=tformat: * %C(yellow)%h%Creset %<|(72,trunc)%s {name}%C(cyan)%cr%Creset");
    let color = if std::io::stdout().is_terminal() { "--color" } else { "--no-color" };
    let range = format!("{from}..HEAD");
    git.text(dir, &["--no-pager", "log", color, "--no-decorate", "--grep=^(chore)", "--invert-grep", "--abbrev=12", &format, &range])
        .unwrap_or_default()
}

/// Why the whole update is better done by Scoop, if it is.
fn hand_over(ctx: &Ctx, git: &Git) -> bool {
    let core = core_dir(ctx);
    let main = buckets::repository(&ctx.scoop, "main");
    let configured = |name: &str, default: &str| match ctx.scoop.setting(name).str() {
        "" => default.to_string(),
        v => v.to_string(),
    };
    let until = ctx.scoop.setting("hold_update_until").str();
    ctx.scoop.enabled("use_sqlite_cache")
        || !is_repository(&core)
        || (main.is_dir() && !is_repository(&main))
        || !ctx.scoop.shims(false).join("scoop.ps1").is_file()
        || (!until.is_empty() && parse_time(until, utc_offset()).is_none())
        || git
            .text(&core, &["config", "remote.origin.url"])
            .is_some_and(|url| !url.to_ascii_lowercase().contains(&configured("scoop_repo", REPOSITORY).to_ascii_lowercase()))
        || git.text(&core, &["branch", "--show-current"]).is_some_and(|b| b != configured("scoop_branch", BRANCH))
}

/// `set_config` of the defaults and `Test-ScoopCoreOnHold`: is the update of
/// Scoop itself held?
fn core_held(ctx: &mut Ctx) -> Result<bool, i32> {
    for (name, default) in [("scoop_repo", REPOSITORY), ("scoop_branch", BRANCH)] {
        if ctx.scoop.setting(name).str().is_empty() {
            set_setting(ctx, name, Some(string(default)))?;
        }
    }
    let mut held = false;
    if let Some(until) = parse_time(ctx.scoop.setting("hold_update_until").str(), utc_offset()) {
        if until > now() {
            let date = ctx.scoop.setting("hold_update_until").str().to_string();
            ctx.warn("update.held", &[("date", &date)]);
            held = true;
        } else {
            ctx.warn("update.hold_over", &[]);
            set_setting(ctx, "hold_update_until", None)?;
        }
    }
    Ok(held)
}

/// Pulls Scoop itself; false when everything has to stop.
fn sync_core(ctx: &mut Ctx, git: &Git, show_log: bool) -> bool {
    let core = core_dir(ctx);
    let title = ctx.t.get("update.scoop").to_string();
    ctx.say(title);
    if let Some(old) = core.parent().map(|p| p.join("old")).filter(|p| p.exists()) {
        let _ = remove_tree(&old);
    }
    if git.text(&core, &["diff", "HEAD", "--name-only"]).is_some_and(|d| !d.is_empty()) {
        if !ctx.scoop.enabled("autostash_on_conflict") {
            ctx.warn("update.uncommitted", &[]);
            return true;
        }
        ctx.warn("update.stashing", &[]);
        let message = format!("WIP at {}", local_time(SystemTime::now()).iso);
        git.run(Some(&core), &["stash", "push", "-m", &message, "-u", "-q"]);
    }
    let previous = git.text(&core, &["rev-parse", "HEAD"]);
    let pulled = git.run(Some(&core), &["pull", "--tags", "--force", "-q"]);
    if !pulled.ok {
        let why = pulled.err;
        ctx.warn("update.failed", &[("name", "Scoop"), ("why", &why)]);
        return false;
    }
    if let Some(previous) = previous.filter(|_| show_log) {
        let text = log(git, &core, "", &previous);
        if !text.is_empty() {
            ctx.say(text);
        }
    }
    // `shim "$currentdir\bin\scoop.ps1" $false`: the shims of Scoop itself,
    // and the shims folder on PATH.
    match rt_scoop::shims::create(&ctx.scoop, ctx.system, &core.join("bin").join("scoop.ps1"), false, None, &[]) {
        Ok(made) => super::machinery::say_made(ctx, &made, false),
        Err(why) => ctx.warn("error.config", &[("why", &why)]),
    }
    true
}

/// What pulling one bucket gave: its log, or why it failed.
fn pull(git: &Git, ctx_dir: &Path, name: &str, show_log: bool) -> Result<String, String> {
    let previous = git.text(ctx_dir, &["rev-parse", "HEAD"]);
    let pulled = git.run(Some(ctx_dir), &["pull", "-q"]);
    if !pulled.ok {
        return Err(pulled.err);
    }
    Ok(previous.filter(|_| show_log).map(|p| log(git, ctx_dir, name, &p)).unwrap_or_default())
}

/// Updates Scoop and every bucket, the buckets at the same time. `args` (`-q`,
/// `scoop`) go with the command when Scoop is given it.
pub fn sync(ctx: &mut Ctx, args: &[String]) -> i32 {
    let Some(git) = Git::find(ctx) else { return ctx.fail("error.no_git", &[]) };
    if hand_over(ctx, &git) {
        ctx.warn("update.by_scoop", &[]);
        let line: Vec<String> = std::iter::once("update".to_string()).chain(args.iter().cloned()).collect();
        return delegate::run(ctx, &line, ctx.root_given);
    }
    let held = match core_held(ctx) {
        Ok(held) => held,
        Err(code) => return code,
    };
    let show_log = ctx.scoop.setting("show_update_log") != &Json::Bool(false);
    if !held && !sync_core(ctx, &git, show_log) {
        return 1;
    }

    let title = ctx.t.get("update.buckets").to_string();
    ctx.say(title);
    let mut repositories = Vec::new();
    for name in buckets::local(&ctx.scoop) {
        let dir = buckets::repository(&ctx.scoop, &name);
        if is_repository(&dir) {
            repositories.push((name, dir));
        } else {
            let line = ctx.t.fill("update.not_git", &[("name", &name)]);
            ctx.say(line);
        }
    }
    let results = parallel(&repositories, |(name, dir)| pull(&git, dir, name, show_log));
    let mut failed = false;
    for ((name, _), result) in repositories.iter().zip(results) {
        match result {
            Ok(text) if !text.is_empty() => ctx.say(text),
            Ok(_) => {}
            Err(why) => {
                failed = true;
                ctx.warn("update.failed", &[("name", name), ("why", &why)]);
            }
        }
    }
    if let Err(code) = set_setting(ctx, "last_update", Some(string(local_time(SystemTime::now()).iso))) {
        return code;
    }
    if failed {
        return 1;
    }
    let done = ctx.t.get("update.done").to_string();
    ctx.say(done);
    0
}

/// Whether the remotes have commits this installation has not.
#[derive(Default)]
pub struct Remotes {
    pub scoop: bool,
    pub buckets: bool,
    pub unreachable: bool,
    /// Why, one line per repository that did not answer.
    pub errors: Vec<String>,
    /// Nothing was asked.
    pub skipped: bool,
}

/// `Test-UpdateStatus` of one repository: (behind, why the fetch failed).
fn behind(git: &Git, dir: &Path) -> (bool, Option<String>) {
    if !is_repository(dir) {
        return (true, None);
    }
    let fetched = git.run(Some(dir), &["fetch", "-q", "origin"]);
    let branch = git.text(dir, &["branch", "--show-current"]).unwrap_or_default();
    let range = format!("HEAD..origin/{branch}");
    let commits = git.text(dir, &["log", &range, "--oneline"]).is_some_and(|c| !c.is_empty());
    let why = (fetched.code == 128).then(|| {
        let line = fetched.err.lines().find(|l| l.starts_with("fatal:")).or_else(|| fetched.err.lines().next()).unwrap_or("");
        format!("{}: {line}", dir.display())
    });
    (commits, why)
}

/// Asks every repository at the same time.
pub fn remotes(ctx: &Ctx) -> Remotes {
    let Some(git) = Git::find(ctx) else { return Remotes { skipped: true, ..Remotes::default() } };
    let mut dirs = vec![core_dir(ctx)];
    dirs.extend(buckets::local(&ctx.scoop).iter().map(|b| buckets::repository(&ctx.scoop, b)));
    let found = parallel(&dirs, |dir| behind(&git, dir));
    Remotes {
        scoop: found.first().is_some_and(|f| f.0),
        buckets: found.iter().skip(1).any(|f| f.0),
        unreachable: found.iter().any(|f| f.1.is_some()),
        errors: found.into_iter().filter_map(|f| f.1).collect(),
        skipped: false,
    }
}
