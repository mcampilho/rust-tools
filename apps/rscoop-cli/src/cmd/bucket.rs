//! `bucket add|rm` (`add_bucket`, `rm_bucket` in `lib\buckets.ps1`).

use super::git::Git;
use super::Ctx;
use regex_lite::Regex;
use rt_scoop::buckets;
use rt_scoop::disk::remove_tree;

/// A name that is a folder of `buckets` and nothing else. Scoop takes any
/// text, and `bucket rm ..` would remove the whole installation.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
        && !name.ends_with(['.', ' '])
}

/// `Convert-RepositoryUri`: `host/user/repository`, to compare addresses.
pub fn same_form(url: &str) -> Option<String> {
    let pattern = Regex::new(r"(?:@|/{1,3})(?:www\.|.*@)?([^/]+?)(?::\d+)?[:/](.+)/(.+?)(?:\.git)?/?$").ok()?;
    let c = pattern.captures(url)?;
    Some(format!("{}/{}/{}", &c[1], &c[2], &c[3]).to_ascii_lowercase())
}

/// `bucket add <name> [repository]`.
pub fn add(ctx: &mut Ctx, args: &[String]) -> i32 {
    let Some(name) = args.first() else { return ctx.fail("error.missing", &[("what", "<name>")]) };
    if !plain_name(name) {
        return ctx.fail("bucket.bad_name", &[("name", name)]);
    }
    let known = buckets::known(&ctx.scoop).into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, url)| url);
    let Some(repository) = args.get(1).cloned().or(known) else { return ctx.fail("bucket.unknown", &[("name", name)]) };
    let Some(git) = Git::find(ctx) else { return ctx.fail("error.no_git", &[]) };
    let dir = ctx.scoop.buckets().join(name);
    if dir.exists() {
        ctx.warn("bucket.exists", &[("name", name)]);
        return 2;
    }
    let Some(form) = same_form(&repository).filter(|_| !repository.starts_with('-')) else {
        return ctx.fail("bucket.bad_url", &[("url", &repository)]);
    };
    for other in buckets::local(&ctx.scoop) {
        let path = buckets::repository(&ctx.scoop, &other);
        if buckets::origin(&path).and_then(|u| same_form(&u)).as_deref() == Some(form.as_str()) {
            ctx.warn("bucket.same_repo", &[("name", &other), ("url", &repository)]);
            return 2;
        }
    }
    let line = ctx.t.get("bucket.checking").to_string();
    ctx.say(line);
    let listed = git.run(None, &["ls-remote", "--", &repository]);
    if !listed.ok {
        return ctx.fail("bucket.not_a_repo", &[("url", &repository), ("why", &listed.err)]);
    }
    if let Err(e) = std::fs::create_dir_all(ctx.scoop.buckets()) {
        let why = e.to_string();
        return ctx.fail("bucket.clone_failed", &[("url", &repository), ("why", &why)]);
    }
    let target = dir.display().to_string();
    let cloned = git.run(None, &["clone", "-q", "--", &repository, &target]);
    if !cloned.ok {
        let _ = remove_tree(&dir);
        return ctx.fail("bucket.clone_failed", &[("url", &repository), ("why", &cloned.err)]);
    }
    let done = ctx.t.fill("bucket.added", &[("name", name)]);
    ctx.say(done);
    0
}

/// `bucket rm <name>`.
pub fn rm(ctx: &mut Ctx, args: &[String]) -> i32 {
    let Some(name) = args.first() else { return ctx.fail("error.missing", &[("what", "<name>")]) };
    if !plain_name(name) {
        return ctx.fail("bucket.bad_name", &[("name", name)]);
    }
    let dir = ctx.scoop.buckets().join(name);
    if std::fs::symlink_metadata(&dir).is_err() {
        return ctx.fail("bucket.not_found", &[("name", name)]);
    }
    // A bucket that is a link to a folder elsewhere loses only the link.
    if let Err(e) = remove_tree(&dir) {
        let why = e.to_string();
        return ctx.fail("error.remove", &[("why", &why)]);
    }
    let done = ctx.t.fill("bucket.removed", &[("name", name)]);
    ctx.say(done);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_and_names() {
        let form = Some("github.com/ScoopInstaller/Main".to_ascii_lowercase());
        for url in [
            "https://github.com/ScoopInstaller/Main",
            "https://github.com/ScoopInstaller/Main.git",
            "git@github.com:ScoopInstaller/Main.git",
            "ssh://git@github.com:22/ScoopInstaller/Main/",
        ] {
            assert_eq!(same_form(url), form, "{url}");
        }
        assert_eq!(same_form("main"), None);
        assert_eq!(same_form(r"C:\repo"), None);
        for bad in ["", ".", "..", "a/b", r"a\b", "c:", "x.", "*"] {
            assert!(!plain_name(bad), "{bad}");
        }
        assert!(plain_name("nerd-fonts") && plain_name("my.bucket"));
    }
}
