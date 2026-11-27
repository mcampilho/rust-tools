//! The commands on a made-up installation under `target\rscoop-tests`.

use super::*;
use crate::registry::FakeRegistry;
use crate::tests::scratch;
use std::path::Path;

pub(super) fn write(root: &Path, relative: &str, text: &str) {
    let path = relative.split('/').fold(root.to_path_buf(), |p, part| p.join(part));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn app(root: &Path, name: &str, version: &str, info: &str) {
    for folder in [version, "current"] {
        write(root, &format!("apps/{name}/{folder}/scoop-manifest.json"), &format!(r#"{{"version":"{version}"}}"#));
        write(root, &format!("apps/{name}/{folder}/scoop-install.json"), info);
    }
}

/// An installation with two apps, one of them outdated and held.
fn installation(name: &str) -> std::path::PathBuf {
    let root = scratch(name);
    write(
        &root,
        "buckets/main/bucket/ripgrep.json",
        r#"{"version":"14.1.0","description":"Search tool","homepage":"https://example.org/rg/","license":"MIT","url":"u.zip","bin":["rg.exe",["sub\\x.exe","xx"]],"notes":"See $dir"}"#,
    );
    write(&root, "buckets/main/bucket/tool.json", r#"{"version":"2.0","url":"u.exe","depends":"ripgrep"}"#);
    app(&root, "ripgrep", "14.1.0", r#"{"architecture":"64bit","bucket":"main"}"#);
    app(&root, "tool", "1.0", r#"{"architecture":"32bit","bucket":"main","hold":true}"#);
    write(&root, "shims/rg.shim", &format!("path = \"{}\"\n", root.join("apps/ripgrep/current/rg.exe").display()));
    write(&root, "shims/rg.exe", "x");
    write(&root, "apps/ripgrep/current/rg.exe", "x");
    write(&root, "cache/ripgrep#14.1.0#abc.zip", "12345");
    root
}

/// (exit code, stdout, stderr)
pub(super) fn rscoop(root: &Path, args: &[&str]) -> (i32, String, String) {
    rscoop_on(root, args, &rt_scoop::system::FakeSystem::new(root.join("menu")))
}

/// The same, with the registry, Start menu and processes of `system`.
pub(super) fn rscoop_on(root: &Path, args: &[&str], system: &rt_scoop::system::FakeSystem) -> (i32, String, String) {
    rscoop_native(root, args, system, true)
}

/// `native`: with RSCOOP_NATIVE=1, which `reset` and `uninstall` need to be done here.
pub(super) fn rscoop_native(root: &Path, args: &[&str], system: &rt_scoop::system::FakeSystem, native: bool) -> (i32, String, String) {
    let mut all = vec!["--lang".to_string(), "en".into(), "--root".into(), root.display().to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    let line = command_line(&all).expect("a command");
    let registry = FakeRegistry::new();
    let vars = HashMap::from([
        ("RSCOOP_NATIVE".to_string(), if native { "1" } else { "" }.to_string()),
        ("USERPROFILE".to_string(), root.join("home").display().to_string()),
        // The shims of the installation, then the real programs (git, PowerShell).
        ("PATH".to_string(), format!("{};{}", root.join("shims").display(), std::env::var("PATH").unwrap_or_default())),
    ]);
    let outside = Outside { vars, exe: None, system_arch: Arch::X64, registry: &registry, system };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run(line, outside, &mut out, &mut err);
    (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
}

#[test]
fn the_line_without_a_command_is_for_the_checker() {
    let words = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
    for checker in ["--json", "--root x --fix", "-h", "--version", "--lang pt"] {
        assert_eq!(command_line(&words(checker)), None, "{checker}");
    }
    let line = command_line(&words("--root=r --json List a --json")).unwrap();
    assert_eq!((line.command.as_str(), line.root.as_deref(), line.json), ("list", Some("r"), true));
    assert_eq!(line.args, ["a", "--json"]);
}

#[test]
fn what_installs_or_removes_apps_goes_to_scoop() {
    let native = |c: &str, a: &[&str]| is_native(c, &a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert!(native("list", &[]) && native("status", &["--local"]) && native("config", &["debug"]));
    assert!(native("bucket", &["list"]) && native("cache", &[]) && native("cache", &["git"]));
    assert!(native("update", &[]) && native("update", &["-q"]) && native("status", &[]) && native("cleanup", &["*"]));
    assert!(native("bucket", &["add", "x"]) && native("config", &["a", "b"]) && native("download", &["x"]));
    assert!(native("uninstall", &["x", "-p"]) && native("reset", &["*"]) && native("shim", &["add", "x", "y"]) && native("shim", &[]));
    for (command, args) in [
        ("install", vec!["x"]),
        ("update", vec!["x"]),
        ("update", vec!["*"]),
        ("update", vec!["-a"]),
        ("uninstall", vec!["x", "Scoop"]),
        ("reset", vec!["--help"]),
        ("import", vec!["x"]),
        ("config", vec!["--help"]),
        ("shim", vec!["alter", "x"]),
        ("scoop", vec!["list"]),
    ] {
        assert!(!native(command, &args), "{command} {args:?}");
    }
}

#[test]
fn list_status_and_search() {
    let root = installation("commands");
    let (code, out, _) = rscoop(&root, &["list"]);
    assert_eq!(code, 0);
    assert!(out.contains("ripgrep 14.1.0  main") && out.contains("Held package, 32bit"), "{out}");
    let (_, out, _) = rscoop(&root, &["list", "^too", "--json"]);
    let list = rt_scoop::json::parse(&out).unwrap();
    assert_eq!(list.items().len(), 1);
    assert_eq!(list.at(0).get("Info").str(), "Held package, 32bit");

    let (_, out, _) = rscoop(&root, &["status", "--local", "--json"]);
    let status = rt_scoop::json::parse(&out).unwrap();
    assert_eq!(status.items().len(), 1, "{out}");
    assert_eq!((status.at(0).get("Name").str(), status.at(0).get("Latest Version").str()), ("tool", "2.0"));
    assert_eq!(status.at(0).get("Info").str(), "Held package");

    // By name, then by the name of a program.
    let (code, out, _) = rscoop(&root, &["search", "RIP"]);
    assert!(code == 0 && out.contains("ripgrep 14.1.0  main"), "{out}");
    let (_, out, _) = rscoop(&root, &["search", "xx"]);
    assert!(out.contains("ripgrep") && out.contains("xx"), "{out}");
    let (code, _, err) = rscoop(&root, &["search", "nothing-like-it"]);
    assert!(code == 1 && err.contains("nothing-like-it"));
    assert_eq!(rscoop(&root, &["search", "("]).0, 1);
}

#[test]
fn one_app() {
    let root = installation("one-app");
    let (_, out, _) = rscoop(&root, &["info", "ripgrep"]);
    for part in ["Website", "https://example.org/rg", "Installed   : 14.1.0", "rg.exe | xx.exe", "See <root>"] {
        assert!(out.contains(part), "{part}: {out}");
    }
    let (_, out, _) = rscoop(&root, &["info", "tool", "--json"]);
    assert_eq!(rt_scoop::json::parse(&out).unwrap().get("Version").str(), "1.0 (Update to 2.0 available)");
    let (_, out, _) = rscoop(&root, &["cat", "ripgrep"]);
    assert!(out.starts_with("{\n    \"version\": \"14.1.0\","), "{out}");
    assert_eq!(rscoop(&root, &["prefix", "tool"]).1.trim_end(), root.join("apps").join("tool").join("current").display().to_string());
    assert_eq!(rscoop(&root, &["prefix", "absent"]).0, 1);
    let (_, out, _) = rscoop(&root, &["which", "rg"]);
    assert!(out.trim_end().ends_with(r"ripgrep\current\rg.exe"), "{out}");
    assert_eq!(rscoop(&root, &["which", "absent"]).0, 2);
    let (_, out, _) = rscoop(&root, &["depends", "tool", "--json"]);
    let deps = rt_scoop::json::parse(&out).unwrap();
    let names: Vec<&str> = deps.items().iter().map(|d| d.get("Name").str()).collect();
    assert_eq!(names, ["ripgrep", "tool"]);
}

#[test]
fn the_state_of_the_installation() {
    let root = installation("state");
    let (_, out, _) = rscoop(&root, &["cache", "show", "ripgrep"]);
    assert!(out.contains("ripgrep 14.1.0") && out.contains("Total: 1 files, 5 B"), "{out}");
    let (_, out, _) = rscoop(&root, &["bucket", "list", "--json"]);
    assert_eq!(rt_scoop::json::parse(&out).unwrap().at(0).get("Manifests").num(), Some(2.0));
    assert!(rscoop(&root, &["bucket", "known"]).1.starts_with("main\n"));
    let (_, out, _) = rscoop(&root, &["export"]);
    let export = rt_scoop::json::parse(&out).unwrap();
    assert_eq!(export.get("apps").items().len(), 2);
    assert_eq!(export.get("buckets").at(0).get("Name").str(), "main");
    assert!(rscoop(&root, &["config", "debug"]).1.contains("'debug' is not set"));
    let (code, out, _) = rscoop(&root, &["help"]);
    assert!(code == 0 && out.contains("search [query]"));
    // No Scoop in this made-up root, so nothing can be handed over.
    let (code, _, err) = rscoop(&root, &["install", "x"]);
    assert!(code == 1 && err.contains("There is no Scoop"), "{err}");
}
