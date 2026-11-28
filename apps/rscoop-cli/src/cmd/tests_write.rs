//! The commands that write, on made-up installations under
//! `target\rscoop-tests`: git repositories on the disk, a web server of a few
//! lines and a `scoop.ps1` that only notes what it was asked.

use super::tests::{rscoop, write};
use crate::sys::make_junction;
use rt_scoop::json::{self, Json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn git(dir: &Path, args: &[&str]) -> String {
    std::fs::create_dir_all(dir).unwrap();
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.org", "-c", "init.defaultBranch=master", "-c", "core.autocrlf=false"])
        .args(args)
        .output()
        .expect("git is needed for these tests");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn url(dir: &Path) -> String {
    format!("file:///{}", dir.display().to_string().replace('\\', "/").trim_start_matches('/'))
}

pub(super) fn commit(repository: &Path, file: &str, text: &str, message: &str) {
    write(repository, file, text);
    git(repository, &["add", "-A"]);
    git(repository, &["commit", "-q", "-m", message]);
}

/// `repository` as a source, cloned into `into`.
fn published(source: &Path, into: &Path, file: &str) {
    git(source, &["init", "-q"]);
    commit(source, file, "first", "first");
    let parent = into.parent().unwrap();
    std::fs::create_dir_all(parent).unwrap();
    git(parent, &["clone", "-q", &url(source), &into.display().to_string()]);
}

fn setting(root: &Path, name: &str) -> Json {
    json::read(&root.join("home/.config/scoop/config.json")).map_or(Json::Null, |c| c.get(name).clone())
}

fn configure(root: &Path, members: &[(&str, String)]) {
    for (name, value) in members {
        rt_scoop::config::set_member(
            &root.join("home").join(".config").join("scoop").join("config.json"),
            name,
            Some(Json::Str(value.clone())),
        )
        .unwrap();
    }
}

/// An installation whose Scoop and `main` bucket are clones of `remote-*`.
fn installation(name: &str) -> (PathBuf, PathBuf) {
    let base = crate::tests::scratch(name);
    let root = base.join("root");
    let core = root.join("apps").join("scoop").join("current");
    published(&base.join("remote-scoop"), &core, "bin/scoop.ps1");
    // The made-up Scoop notes its arguments.
    commit(
        &base.join("remote-scoop"),
        "bin/scoop.ps1",
        "Add-Content -Path (Join-Path $env:SCOOP 'handed.txt') -Value ($args -join ' ')\n",
        "script",
    );
    git(&core, &["pull", "-q"]);
    published(&base.join("remote-main"), &root.join("buckets").join("main"), "bucket/a.json");
    write(&root, "shims/scoop.ps1", "x");
    configure(&root, &[("scoop_repo", url(&base.join("remote-scoop"))), ("scoop_branch", "master".into())]);
    (base, root)
}

fn handed(root: &Path) -> String {
    std::fs::read_to_string(root.join("handed.txt")).unwrap_or_default().trim().to_string()
}

#[test]
fn update_pulls_scoop_and_the_buckets() {
    let (base, root) = installation("update");
    let (code, out, err) = rscoop(&root, &["status"]);
    assert!(code == 0 && out.contains("Scoop is up to date."), "{out}{err}");

    commit(&base.join("remote-main"), "bucket/new.json", "{}", "new: added");
    commit(&base.join("remote-main"), "bucket/a.json", "second", "(chore) routine");
    let (_, out, _) = rscoop(&root, &["status"]);
    assert!(out.contains("Buckets are out of date"), "{out}");
    let (_, out, _) = rscoop(&root, &["status", "--local"]);
    assert!(!out.contains("out of date"), "{out}");

    write(&root, "buckets/plain/bucket/x.json", "{}");
    let (code, out, err) = rscoop(&root, &["update"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(root.join("buckets/main/bucket/new.json").is_file(), "{out}{err}");
    assert!(out.contains("new: added") && !out.contains("routine"), "{out}");
    assert!(out.contains("'plain' is not a git repository") && out.contains("updated successfully"), "{out}");
    assert!(!setting(&root, "last_update").str().is_empty());
    assert_eq!(handed(&root), "", "nothing is handed to Scoop");
    // Scoop's own shims are made again, as `Sync-Scoop` ends.
    let core = root.join("apps").join("scoop").join("current").join("bin").join("scoop.ps1");
    let shim = std::fs::read_to_string(root.join("shims").join("scoop.ps1")).unwrap();
    assert!(shim.starts_with(&format!("# {}\r\n$path = Join-Path $PSScriptRoot", core.display())), "{shim}");
    assert!(root.join("shims").join("scoop.cmd").is_file() && root.join("shims").join("scoop").is_file());
}

#[test]
fn update_leaves_local_changes_and_held_scoop_alone() {
    let (base, root) = installation("held");
    let core = root.join("apps/scoop/current");
    commit(&base.join("remote-scoop"), "new.txt", "x", "feat: new");
    configure(&root, &[("hold_update_until", "2999-01-01".into())]);
    let (code, _, err) = rscoop(&root, &["update"]);
    assert!(code == 0 && err.contains("is held until"), "{err}");
    assert!(!core.join("new.txt").exists());

    configure(&root, &[("hold_update_until", "2000-01-01".into())]);
    write(&core, "bin/scoop.ps1", "changed here");
    let (_, _, err) = rscoop(&root, &["update"]);
    assert!(err.contains("enabled again") && err.contains("Uncommitted changes"), "{err}");
    assert_eq!(std::fs::read_to_string(core.join("bin/scoop.ps1")).unwrap(), "changed here");
    assert!(!core.join("new.txt").exists());
    assert_eq!(setting(&root, "hold_update_until"), Json::Null);
}

#[test]
fn what_only_scoop_should_do_is_handed_over() {
    let (_, root) = installation("hand-over");
    // Another branch configured: Scoop would reset its repository.
    configure(&root, &[("scoop_branch", "develop".into())]);
    let (code, _, err) = rscoop(&root, &["update"]);
    assert!(code == 0 && err.contains("handing it to Scoop"), "{err}");
    assert_eq!(handed(&root), "update");
    assert_eq!(rscoop(&root, &["update", "-q"]).0, 0);
    // Without RSCOOP_NATIVE=1, what installs is still Scoop's.
    let system = rt_scoop::system::FakeSystem::new(root.join("menu"));
    assert_eq!(super::tests::rscoop_native(&root, &["update", "git"], &system, false).0, 0);
    assert_eq!(super::tests::rscoop_native(&root, &["install", "x"], &system, false).0, 0);
    assert_eq!(handed(&root).lines().collect::<Vec<_>>(), ["update", "update -q", "update git", "install x"]);
}

#[test]
fn buckets_are_added_and_removed() {
    let (base, root) = installation("buckets");
    let source = base.join("remote-extra");
    git(&source, &["init", "-q"]);
    commit(&source, "bucket/e.json", "{}", "e");

    let (code, out, err) = rscoop(&root, &["bucket", "add", "extra", &url(&source)]);
    assert!(code == 0 && root.join("buckets/extra/bucket/e.json").is_file(), "{out}{err}");
    assert_eq!(rscoop(&root, &["bucket", "add", "extra", &url(&source)]).0, 2);
    let (code, _, err) = rscoop(&root, &["bucket", "add", "again", &url(&source)]);
    assert!(code == 2 && err.contains("extra already has"), "{err}");
    let (code, _, err) = rscoop(&root, &["bucket", "add", "gone", &url(&base.join("nowhere"))]);
    assert!(code == 1 && !root.join("buckets/gone").exists(), "{err}");

    // Never anything outside the buckets.
    for bad in ["..", ".", r"..\apps", "main/.."] {
        let (code, _, err) = rscoop(&root, &["bucket", "rm", bad]);
        assert!(code == 1 && err.contains("is not a bucket name"), "{bad}: {err}");
    }
    assert!(root.join("apps").is_dir());
    // A git repository has read-only files: they go too.
    let (code, _, err) = rscoop(&root, &["bucket", "rm", "extra"]);
    assert!(code == 0 && !root.join("buckets/extra").exists(), "{err}");
    assert!(source.join("bucket/e.json").is_file());
    // A bucket that is a link loses the link only.
    make_junction(&root.join("buckets/linked").display().to_string(), &source.display().to_string()).unwrap();
    assert_eq!(rscoop(&root, &["bucket", "rm", "linked"]).0, 0);
    assert!(source.join("bucket/e.json").is_file() && !root.join("buckets/linked").exists());
}

fn installed(root: &Path, version: &str) {
    write(root, &format!("apps/tool/{version}/scoop-manifest.json"), &format!(r#"{{"version":"{version}","persist":"data"}}"#));
    write(root, &format!("apps/tool/{version}/scoop-install.json"), r#"{"architecture":"64bit","bucket":"main"}"#);
}

#[test]
fn cleanup_removes_old_versions_and_nothing_else() {
    let root = crate::tests::scratch("cleanup");
    write(&root, "persist/tool/data/kept.txt", "precious");
    for version in ["1.0", "2.0", "3.0"] {
        installed(&root, version);
        make_junction(
            &root.join("apps/tool").join(version).join("data").display().to_string(),
            &root.join("persist/tool/data").display().to_string(),
        )
        .unwrap();
    }
    make_junction(&root.join("apps/tool/current").display().to_string(), &root.join("apps/tool/3.0").display().to_string()).unwrap();
    for name in ["tool#1.0#a.zip", "tool#3.0#b.zip", "toolbox#1.0#c.zip", "left.download"] {
        write(&root, &format!("cache/{name}"), "x");
    }
    // An app whose current version cannot be told keeps everything.
    write(&root, "apps/odd/1.0/file", "x");
    write(&root, "apps/odd/2.0/file", "x");

    let (code, out, err) = rscoop(&root, &["cleanup", "*", "-k"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("Removing tool: 1.0 2.0"), "{out}");
    assert!(!root.join("apps/tool/1.0").exists() && !root.join("apps/tool/2.0").exists());
    assert!(root.join("apps/tool/3.0/scoop-manifest.json").is_file() && root.join("apps/tool/current/data/kept.txt").is_file());
    assert_eq!(std::fs::read_to_string(root.join("persist/tool/data/kept.txt")).unwrap(), "precious");
    assert!(root.join("apps/odd/1.0/file").is_file() && root.join("apps/odd/2.0/file").is_file());
    let mut left: Vec<String> =
        std::fs::read_dir(root.join("cache")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    left.sort();
    assert_eq!(left, ["tool#3.0#b.zip", "toolbox#1.0#c.zip"]);
    let (code, out, _) = rscoop(&root, &["cleanup", "tool"]);
    assert!(code == 0 && out.contains("already clean"), "{out}");
    assert_eq!(rscoop(&root, &["cleanup", ".."]).0, 1);
    assert_eq!(rscoop(&root, &["cleanup"]).0, 1);

    // The cache, by name.
    let (code, out, _) = rscoop(&root, &["cache", "rm", "tool"]);
    assert!(code == 0 && out.contains("Deleted: 1 files"), "{out}");
    assert!(root.join("cache/toolbox#1.0#c.zip").is_file());
    assert_eq!(rscoop(&root, &["cache", "rm", "*"]).0, 0);
    assert_eq!(std::fs::read_dir(root.join("cache")).unwrap().count(), 0);
}

#[test]
fn hold_config_and_alias() {
    let root = crate::tests::scratch("settings");
    installed(&root, "1.0");
    make_junction(&root.join("apps/tool/current").display().to_string(), &root.join("apps/tool/1.0").display().to_string()).unwrap();
    let info = root.join("apps/tool/1.0/scoop-install.json");

    assert_eq!(rscoop(&root, &["hold", "tool"]).0, 0);
    let held = json::read(&info).unwrap();
    assert_eq!((held.get("hold"), held.get("bucket").str()), (&Json::Bool(true), "main"));
    assert!(rscoop(&root, &["hold", "tool"]).1.contains("already held"));
    assert_eq!(rscoop(&root, &["unhold", "tool"]).0, 0);
    assert_eq!(json::read(&info).unwrap().get("hold"), &Json::Null);
    assert!(rscoop(&root, &["unhold", "tool"]).1.contains("not held"));
    assert_eq!(rscoop(&root, &["hold", "absent"]).0, 1);

    assert_eq!(rscoop(&root, &["hold", "scoop"]).0, 0);
    assert!(setting(&root, "hold_update_until").str().starts_with("20"));
    assert_eq!(rscoop(&root, &["unhold", "scoop"]).0, 0);
    assert_eq!(setting(&root, "hold_update_until"), Json::Null);

    assert_eq!(rscoop(&root, &["config", "Aria2-Split", "8"]).0, 0);
    assert_eq!(rscoop(&root, &["config", "debug", "true"]).0, 0);
    assert_eq!((setting(&root, "aria2-split"), setting(&root, "debug")), (Json::Num(8.0), Json::Bool(true)));
    assert!(rscoop(&root, &["config", "aria2-split"]).1.starts_with('8'));
    assert_eq!(rscoop(&root, &["config", "rm", "debug"]).0, 0);
    assert_eq!(setting(&root, "debug"), Json::Null);

    assert_eq!(rscoop(&root, &["alias", "add", "up", "scoop update *", "Update all"]).0, 0);
    assert_eq!(std::fs::read_to_string(root.join("shims/scoop-up.ps1")).unwrap(), "# Summary: Update all\nscoop update *\r\n");
    assert_eq!(setting(&root, "alias").get("up").str(), "scoop-up");
    let (_, out, _) = rscoop(&root, &["alias", "list", "-v"]);
    assert!(out.contains("scoop update *") && out.contains("Update all"), "{out}");
    assert_eq!(rscoop(&root, &["alias", "add", "up", "x"]).0, 1);
    assert_eq!(rscoop(&root, &["alias", "add", r"..\x", "x"]).0, 1);
    assert_eq!(rscoop(&root, &["alias", "rm", "up"]).0, 0);
    assert!(!root.join("shims/scoop-up.ps1").exists());
    assert!(rscoop(&root, &["alias", "list"]).1.contains("No alias found"));
    // Settings of the user are kept.
    assert_eq!(setting(&root, "aria2-split"), Json::Num(8.0));
}

/// What the server was sent, one request after another.
type Requests = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// Serves `/name` with the given bytes, `/moved` as a redirect to
/// `/good.zip`, and 404 for anything else.
pub(super) fn server(files: Vec<(&'static str, &'static [u8])>) -> (String, Requests) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let requests = Requests::default();
    let seen = requests.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut request = [0u8; 4096];
            let n = stream.read(&mut request).unwrap_or(0);
            let text = String::from_utf8_lossy(&request[..n]).into_owned();
            let path = text.split_whitespace().nth(1).unwrap_or("");
            seen.lock().unwrap().push(text.clone());
            if path == "/moved" {
                let _ = stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: /good.zip\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                continue;
            }
            let (status, body) = match files.iter().find(|(name, _)| path == format!("/{name}")) {
                Some((_, body)) => ("200 OK", *body),
                None => ("404 Not Found", &b""[..]),
            };
            let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(body));
        }
    });
    (address, requests)
}

#[test]
fn downloads_are_kept_and_checked() {
    let root = crate::tests::scratch("download");
    let (base, requests) = server(vec![("good.zip", b"abc"), ("bad.zip", b"abc")]);
    let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let manifest = |name: &str, file: &str, hash: &str| {
        write(&root, &format!("buckets/main/bucket/{name}.json"), &format!(r#"{{"version":"1.0","url":"{base}/{file}","hash":"{hash}"}}"#));
    };
    manifest("good", "good.zip", sha);
    manifest("bad", "bad.zip", "00");
    manifest("missing", "missing.zip", sha);
    // Updated a moment ago, so that `download` does not update first.
    configure(&root, &[("last_update", crate::sys::local_time(std::time::SystemTime::now()).iso)]);
    let cached = |app: &str, file: &str| rt_scoop::download::cache_path(&root.join("cache"), app, "1.0", &format!("{base}/{file}"));

    let (code, out, err) = rscoop(&root, &["download", "good"]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(std::fs::read(cached("good", "good.zip")).unwrap(), b"abc");
    assert!(out.contains("hash OK"), "{out}");
    let (_, out, _) = rscoop(&root, &["download", "good"]);
    assert!(out.contains("already in the cache"), "{out}");

    let (code, _, err) = rscoop(&root, &["download", "bad"]);
    assert!(code == 1 && err.contains("Hash check failed"), "{err}");
    assert!(!cached("bad", "bad.zip").exists());
    assert_eq!(rscoop(&root, &["download", "bad", "-s"]).0, 0);

    let (code, _, err) = rscoop(&root, &["download", "missing"]);
    assert!(code == 1 && err.contains("HTTP 404"), "{err}");
    assert!(!cached("missing", "missing.zip").exists());
    // A header meant for one address does not go to where it redirects.
    manifest("moved", "moved", sha);
    rt_scoop::config::set_member(
        &root.join("home").join(".config").join("scoop").join("config.json"),
        "private_hosts",
        json::parse(r#"[{"match":"/moved$","headers":"X-Secret=yes"}]"#),
    )
    .unwrap();
    requests.lock().unwrap().clear();
    let (code, out, err) = rscoop(&root, &["download", "moved"]);
    assert_eq!(code, 0, "{out}{err}");
    let seen = requests.lock().unwrap().clone();
    assert!(seen[0].starts_with("GET /moved") && seen[0].contains("X-Secret: yes"), "{seen:?}");
    assert!(seen[1].starts_with("GET /good.zip") && !seen[1].contains("X-Secret"), "{seen:?}");

    // Half a download of aria2 is not a download.
    let half = cached("good", "good.zip");
    std::fs::write(&half, "a").unwrap();
    std::fs::write(format!("{}.aria2", half.display()), "").unwrap();
    assert_eq!(rscoop(&root, &["download", "good"]).0, 0);
    assert_eq!(std::fs::read(&half).unwrap(), b"abc");
    assert!(!Path::new(&format!("{}.aria2", half.display())).exists());

    assert_eq!(rscoop(&root, &["download", "absent"]).0, 1);
    assert_eq!(rscoop(&root, &["download"]).0, 1);
}
