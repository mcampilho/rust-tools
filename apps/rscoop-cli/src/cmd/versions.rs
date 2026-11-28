//! `app@version` (`generate_user_manifest`, `Find-HistoricalManifestInGit`
//! in `lib\manifest.ps1`): the manifest of a version that is not the one in
//! the bucket, from the bucket's git history, written to
//! `<root>\workspace\<app>.json`. The app is then installed from that file,
//! as Scoop does (an app "from a path": `url` in its install information,
//! no bucket). A manifest Scoop would have to make by `autoupdate`
//! (downloading and hashing) is Scoop's.

use super::git::Git;
use super::Ctx;
use rt_scoop::buckets;
use rt_scoop::json::{self, Json};
use rt_scoop::manifest::Manifest;
use rt_scoop::resolve;
use std::path::PathBuf;

/// What became of `app@version`.
pub enum Found {
    /// The file to install from.
    Path(PathBuf),
    /// Only Scoop can do it (`autoupdate`, the history turned off).
    Scoop,
}

/// `Write-ManifestToUserCache`: `Out-UTF8File`, the text and a line end.
fn write_workspace(ctx: &Ctx, app: &str, text: &str) -> Option<PathBuf> {
    let dir = ctx.scoop.root.join("workspace");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{app}.json"));
    std::fs::write(&path, format!("{text}\r\n")).ok()?;
    Some(path)
}

/// The text `Invoke-Git` gives back: its lines joined by `\n`.
fn lines(text: &str) -> String {
    text.lines().collect::<Vec<_>>().join("\n")
}

fn version_of(text: &str) -> Option<String> {
    match json::parse(text)? {
        j @ Json::Obj(_) => Some(Manifest(j).version().to_string()),
        _ => None,
    }
}

/// `Find-HistoricalManifestInGit`: the text of the manifest at the version.
fn from_git(ctx: &mut Ctx, git: &Git, app: &str, bucket: &str, version: &str) -> Option<String> {
    let repository = buckets::repository(&ctx.scoop, bucket);
    if !repository.join(".git").exists() {
        ctx.warn("version.not_git", &[("bucket", bucket)]);
        return None;
    }
    let inner = buckets::manifest_dir(&ctx.scoop, bucket);
    let relative = inner.join(format!("{app}.json"));
    let relative = relative.strip_prefix(&repository).ok()?.display().to_string().replace('\\', "/");
    let show = |spec: &str| git.text(&repository, &["show", &format!("{spec}:{relative}")]).map(|t| lines(&t));
    if let Some(head) = show("HEAD").filter(|t| version_of(t).as_deref() == Some(version)) {
        return Some(head);
    }
    let escaped = regex_lite::escape(version);
    let pattern = format!("version.: .{escaped}");
    let literal = format!("version: {version}");
    let commit = [("-G", pattern.as_str()), ("-S", literal.as_str())].iter().find_map(|(flag, what)| {
        git.text(&repository, &["log", "--follow", "-n", "1", "--format=%H", flag, what, "--", &relative])
            .and_then(|out| out.lines().map(str::trim).find(|l| !l.is_empty()).map(String::from))
    })?;
    [format!("{commit}^"), commit].iter().find_map(|spec| show(spec).filter(|t| version_of(t).as_deref() == Some(version)))
}

/// `generate_user_manifest`. Err: the exit code when Scoop would stop.
pub fn generate(ctx: &mut Ctx, app: &str, bucket: Option<&str>, version: &str) -> Result<Found, i32> {
    let query = match bucket {
        Some(b) => format!("{b}/{app}"),
        None => app.to_string(),
    };
    let r = resolve::resolve(&ctx.scoop, &query);
    let (Some(manifest), Some(path)) = (r.manifest, r.path) else {
        return Err(ctx.fail("error.no_manifest", &[("app", &query)]));
    };
    if manifest.version().eq_ignore_ascii_case(version) {
        return Ok(Found::Path(path));
    }
    let line = ctx.t.fill("version.resolving", &[("app", app), ("version", version)]);
    ctx.say(line);
    if ctx.scoop.setting("use_git_history") == &Json::Bool(false) {
        return Ok(Found::Scoop);
    }
    let Some(git) = Git::find(ctx) else { return Ok(Found::Scoop) };
    let bucket = r.bucket.unwrap_or_default();
    match from_git(ctx, &git, &r.app, &bucket, version) {
        Some(text) => match write_workspace(ctx, &r.app, &text) {
            Some(path) => Ok(Found::Path(path)),
            None => Err(ctx.fail("error.remove", &[("why", "workspace")])),
        },
        // Made by `autoupdate`, if the manifest has it: Scoop's.
        None => Ok(Found::Scoop),
    }
}
