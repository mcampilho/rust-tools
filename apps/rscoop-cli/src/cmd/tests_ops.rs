//! `shim`, `reset` and `uninstall` on made-up installations, with the
//! registry, Start menu and processes of a [`FakeSystem`]. The scripts of
//! manifests run in a real PowerShell, over a made-up Scoop library that
//! notes what it is asked.

use super::tests::{rscoop_native, rscoop_on, write};
use crate::tests::scratch;
use rt_scoop::system::{FakeSystem, Process};
use std::path::{Path, PathBuf};

/// The header of a program, enough for the subsystem (2: with a window).
fn exe(subsystem: u16) -> Vec<u8> {
    let mut e = vec![0u8; 0x200];
    e[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    e[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
    e[0xDC..0xDE].copy_from_slice(&subsystem.to_le_bytes());
    e
}

/// The made-up library: the functions the host calls, writing to notes.txt.
const LIB: &str = r#"
function appdir($app, $global) { "$env:SCOOP\apps\$app" }
function versiondir($app, $version, $global) { "$(appdir $app $global)\$version" }
function persistdir($app, $global) { "$env:SCOOP\persist\$app" }
function installed_manifest($app, $version, $global) { Get-Content "$(versiondir $app $version $global)\scoop-manifest.json" -Raw | ConvertFrom-Json }
function install_info($app, $version, $global) { Get-Content "$(versiondir $app $version $global)\scoop-install.json" -Raw | ConvertFrom-Json }
function Note($text) { Add-Content -Path "$env:SCOOP\notes.txt" -Value $text }
function Invoke-HookScript($HookType, $Manifest, $ProcessorArchitecture) {
    $script = $Manifest.$HookType
    if ($script) { Invoke-Command ([scriptblock]::Create($script -join "`r`n")) }
}
function Invoke-Installer($Path, $Name, $Manifest, $ProcessorArchitecture, $AppName, [switch]$Global, [switch]$Uninstall) {
    Note "installer $Path $ProcessorArchitecture $Uninstall"
}
function Get-Manifest($query) {
    $bucket, $name = $query -split '/'
    $m = Get-Content "$env:SCOOP\buckets\$bucket\bucket\$name.json" -Raw | ConvertFrom-Json
    return $name, $m, $bucket, $null
}
function manifest($app, $bucket, $url) { (Get-Manifest "$bucket/$app")[1] }
function Invoke-Extraction($Path, $Name, $Manifest, $ProcessorArchitecture) { Note "extract $($Name -join ',') $ProcessorArchitecture" }
"#;

/// An app `tool` with two versions installed (`current` on 2.0), another
/// app `other` whose shim `tool` was replaced, and Scoop's `shim.exe`.
fn installation(name: &str) -> PathBuf {
    let root = scratch(name);
    let core = at(&root, "apps/scoop/current");
    write(&core, "bin/scoop.ps1", "Add-Content -Path (Join-Path $env:SCOOP 'handed.txt') -Value ($args -join ' ')\n");
    write(&core, "lib/core.ps1", LIB);
    std::fs::create_dir_all(core.join("supporting/shims/kiennq")).unwrap();
    std::fs::write(core.join("supporting/shims/kiennq/shim.exe"), exe(3)).unwrap();
    let manifest = |version: &str| {
        format!(
            r#"{{"version":"{version}","bin":["tool.exe",["tool.exe","tl","--from $dir"]],"shortcuts":[["tool.exe","Tools\\Tool"]],
            "env_add_path":"bin","env_set":{{"TOOL_HOME":"$dir","TOOL_DATA":"$persist_dir\\d"}},"persist":["data","conf.ini"],
            "pre_uninstall":"Note \"pre $cmd $app $version $global\"","post_uninstall":"Note \"post $dir\"","uninstaller":{{"script":"x"}}}}"#
        )
    };
    for version in ["1.0", "2.0"] {
        let dir = at(&root, "apps/tool").join(version);
        write(&dir, "scoop-manifest.json", &manifest(version));
        write(&dir, "scoop-install.json", r#"{"architecture":"64bit","bucket":"main"}"#);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("tool.exe"), exe(2)).unwrap();
        write(&dir, "conf.ini", &format!("conf {version}"));
    }
    crate::sys::make_junction(&at(&root, "apps/tool/current").display().to_string(), &at(&root, "apps/tool/2.0").display().to_string())
        .unwrap();
    let other = at(&root, "apps/other/1.0");
    write(&other, "scoop-manifest.json", r#"{"version":"1.0","bin":"tool.exe"}"#);
    write(&other, "scoop-install.json", r#"{"architecture":"64bit","bucket":"main"}"#);
    std::fs::write(other.join("tool.exe"), exe(3)).unwrap();
    write(&root, "shims/tool.shim", &format!("path = \"{}\"\r\n", other.join("tool.exe").display()));
    std::fs::write(at(&root, "shims/tool.exe"), exe(3)).unwrap();
    root
}

/// `root` and a relative path written with `/`.
fn at(root: &Path, relative: &str) -> PathBuf {
    relative.split('/').fold(root.to_path_buf(), |p, part| p.join(part))
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

#[test]
fn reset_links_shims_shortcuts_variables_and_persisted_data() {
    let root = installation("reset");
    let system = FakeSystem::new(root.join("menu"));
    let (code, out, err) = rscoop_on(&root, &["reset", "tool@1.0"], &system);
    assert_eq!(code, 0, "{out}{err}");
    let current = at(&root, "apps/tool/current");
    assert_eq!(read(&current.join("conf.ini")), "conf 1.0", "{out}");
    let shims = root.join("shims");
    assert_eq!(read(&shims.join("tool.shim")), format!("path = \"{}\"\r\n", current.join("tool.exe").display()));
    assert_eq!(
        read(&shims.join("tl.shim")),
        format!("path = \"{}\"\r\nargs = --from {}", current.join("tool.exe").display(), current.display())
    );
    assert_eq!(rt_scoop::shims::pe_subsystem(&shims.join("tool.exe")), 2);
    assert!(out.contains("Making ") && out.contains("Creating shim for 'tl'."), "{out}");
    assert!(at(&root, "menu/user/Programs/Scoop Apps/Tools/Tool.lnk").is_file());
    let path = system.var("PATH", false).unwrap();
    assert!(path.starts_with(&format!("{};{}", current.join("bin").display(), shims.display())), "{path}");
    assert_eq!(system.var("TOOL_HOME", false), Some(current.display().to_string()));
    assert_eq!(system.var("TOOL_DATA", false), Some(format!("{}\\d", at(&root, "persist/tool").display())));
    // The data went to persist, and is linked back.
    let persist = at(&root, "persist/tool");
    assert_eq!(read(&persist.join("conf.ini")), "conf 1.0");
    assert!(persist.join("data").is_dir() && at(&root, "apps/tool/1.0/data").is_dir());
    assert_eq!(rt_scoop::disk::link_count(&at(&root, "apps/tool/1.0/conf.ini")), 2);

    // Back to 2.0: its own file is kept aside, the persisted one is used.
    let (code, out, err) = rscoop_on(&root, &["reset", "tool@2.0"], &system);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(read(&current.join("conf.ini")), "conf 1.0");
    assert_eq!(read(&at(&root, "apps/tool/2.0/conf.ini.original")), "conf 2.0");
    // PATH once, at the front.
    let path = system.var("PATH", false).unwrap();
    assert_eq!(path.matches(&current.join("bin").display().to_string()).count(), 1, "{path}");
    assert_eq!(rscoop_on(&root, &["reset", "none"], &system).0, 0);
}

#[test]
fn uninstall_runs_the_scripts_and_puts_back_the_other_apps_shim() {
    let root = installation("uninstall");
    let system = FakeSystem::new(root.join("menu"));
    let (code, out, err) = rscoop_on(&root, &["reset", "tool"], &system);
    assert_eq!(code, 0, "{out}{err}");
    assert!(err.contains("installed from other"), "{err}");
    assert!(at(&root, "shims/tool.shim.other").is_file());

    let (code, out, err) = rscoop_on(&root, &["uninstall", "tool", "--purge"], &system);
    assert_eq!(code, 0, "{out}{err}");
    let notes = read(&root.join("notes.txt"));
    let v2 = at(&root, "apps/tool/2.0");
    assert_eq!(notes, format!("pre uninstall tool 2.0 False\r\ninstaller {} 64bit True\r\npost {}\r\n", v2.display(), v2.display()));
    assert!(!at(&root, "apps/tool").exists() && !at(&root, "persist/tool").exists(), "{out}");
    // The shim of the other app is back; this one's are gone.
    let shims = root.join("shims");
    assert!(read(&shims.join("tool.shim")).contains(r"apps\other\1.0\tool.exe"));
    assert!(!shims.join("tl.shim").exists() && !shims.join("tl.exe").exists());
    assert!(!at(&root, "menu/user/Programs/Scoop Apps/Tools/Tool.lnk").exists());
    assert_eq!(system.var("TOOL_HOME", false), None);
    assert!(!system.var("PATH", false).unwrap_or_default().contains(r"apps\tool"));
    assert!(out.contains("'tool' was uninstalled.") && out.contains("Removing older version (1.0)."), "{out}");
}

#[test]
fn uninstall_leaves_a_running_app_and_hands_scoop_itself_over() {
    let root = installation("uninstall-running");
    let mut system = FakeSystem::new(root.join("menu"));
    system.running = vec![Process { id: 42, name: "tool.exe".into(), path: at(&root, "apps/tool/2.0/tool.exe") }];
    let (code, out, err) = rscoop_on(&root, &["uninstall", "tool"], &system);
    assert_eq!(code, 0, "{out}{err}");
    assert!(err.contains("still running") && out.contains("tool.exe (42)"), "{err}{out}");
    assert!(at(&root, "apps/tool/2.0").is_dir());
    assert_eq!(read(&root.join("notes.txt")).lines().count(), 1, "only pre_uninstall ran");

    assert_eq!(rscoop_on(&root, &["uninstall", "scoop"], &system).0, 0);
    assert_eq!(read(&root.join("handed.txt")).trim(), "uninstall scoop");
    // Without RSCOOP_NATIVE=1 it is Scoop's still.
    assert_eq!(rscoop_native(&root, &["uninstall", "tool"], &system, false).0, 0);
    assert_eq!(read(&root.join("handed.txt")).lines().last(), Some("uninstall tool"));
    let (code, _, err) = rscoop_on(&root, &["uninstall", "nothing"], &system);
    assert!(code == 0 && err.contains("'nothing' is not installed."), "{err}");
}

#[test]
fn a_variable_only_powershell_can_expand_sends_reset_to_scoop() {
    let root = installation("reset-expression");
    let system = FakeSystem::new(root.join("menu"));
    let manifest = at(&root, "apps/tool/2.0/scoop-manifest.json");
    std::fs::write(&manifest, read(&manifest).replace(r#""TOOL_HOME":"$dir""#, r#""TOOL_HOME":"$(Get-Date)""#)).unwrap();
    let (code, _, err) = rscoop_on(&root, &["reset", "tool"], &system);
    assert!(code == 0 && err.contains("handing the command to Scoop"), "{err}");
    assert_eq!(read(&root.join("handed.txt")).trim(), "reset tool");
    assert!(!at(&root, "shims/tl.shim").exists());
}

/// The real machine, where it changes nothing outside `target`: a shortcut
/// written by COM and read back, and this test among the processes.
#[test]
fn real_shortcuts_and_processes() {
    use rt_scoop::system::{System, WinSystem};
    let root = scratch("real");
    let target = at(&root, "apps/x/1.0/x.exe");
    write(&root, "apps/x/1.0/x.exe", "");
    write(&root, "apps/x/1.0/x.ico", "");
    let link = root.join("X.lnk");
    WinSystem.shortcut(&link, &target, target.parent().unwrap(), "--a \"b c\"", Some(&at(&root, "apps/x/1.0/x.ico"))).unwrap();
    let read = crate::lnk::read(&link.display().to_string()).unwrap();
    assert_eq!(read.target.as_deref(), Some(target.display().to_string().as_str()));
    assert_eq!(read.arguments.as_deref(), Some("--a \"b c\""));
    assert_eq!(read.working_dir.as_deref(), Some(target.parent().unwrap().display().to_string().as_str()));
    let me = std::env::current_exe().unwrap();
    assert!(WinSystem.processes().iter().any(|p| p.id == std::process::id() && p.path == me));
    assert!(WinSystem.start_menu(false).is_some_and(|m| m.ends_with("Start Menu")));
}

#[test]
fn shims_are_added_listed_shown_and_removed() {
    let root = installation("shim-command");
    let system = FakeSystem::new(root.join("menu"));
    let target = at(&root, "apps/other/1.0/tool.exe");
    let (code, out, err) = rscoop_on(&root, &["shim", "add", "mine", &target.display().to_string(), "--", "-x", "--global"], &system);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(read(&at(&root, "shims/mine.shim")), format!("path = \"{}\"\r\nargs = -x --global", target.display()));
    let (code, out, _) = rscoop_on(&root, &["shim", "list", "^mi"], &system);
    assert!(code == 0 && out.contains("mine") && out.contains("other") && !out.contains("tool."), "{out}");
    let (_, out, _) = rscoop_on(&root, &["--json", "shim", "info", "mine"], &system);
    let info = rt_scoop::json::parse(&out).unwrap();
    assert_eq!(info.get("Source").str(), "other");
    assert_eq!(info.get("Path").str(), at(&root, "shims/mine.exe").display().to_string());
    let (code, out, _) = rscoop_on(&root, &["shim", "rm", "mine"], &system);
    assert!(code == 0 && out.contains("Removing shim 'mine.shim'.") && out.contains("Removing shim 'mine.exe'."), "{out}");
    assert_eq!(rscoop_on(&root, &["shim", "rm", "mine"], &system).0, 3);
    assert_eq!(rscoop_on(&root, &["shim", "info", "none"], &system).0, 3);
    assert_eq!(rscoop_on(&root, &["shim", "add", "x", r"C:\no\such.exe"], &system).0, 3);
    // `alter` asks the user: Scoop's.
    rscoop_on(&root, &["shim", "alter", "tool"], &system);
    assert_eq!(read(&root.join("handed.txt")).trim(), "shim alter tool");
}

#[test]
fn install_downloads_extracts_links_and_records_and_uninstall_takes_it_back() {
    let root = installation("install");
    let system = FakeSystem::new(root.join("menu"));
    static HELLO: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let body: &'static [u8] = HELLO.get_or_init(|| exe(3));
    let (address, _) = super::tests_write::server(vec![("hello.exe", body), ("dep.exe", body)]);
    let hash = rt_core::sha256::hex(body);
    write(
        &root,
        "buckets/main/bucket/hello.json",
        &format!(
            r#"{{"version":"1.0","url":"{address}/hello.exe","hash":"{hash}","bin":"hello.exe","depends":"main/dep",
            "shortcuts":[["hello.exe","Hello"]],"env_set":{{"HELLO":"$dir"}},"persist":"data","notes":"See $dir",
            "pre_install":"Note \"pre $cmd $version\"","post_install":"Note \"post $dir\"","suggest":{{"x":"main/jq"}}}}"#
        ),
    );
    write(&root, "buckets/main/bucket/dep.json", &format!(r#"{{"version":"2.0","url":"{address}/dep.exe","hash":"{hash}"}}"#));
    let (code, out, err) = rscoop_on(&root, &["install", "hello", "-u"], &system);
    assert_eq!(code, 0, "{out}{err}");
    let notes = read(&at(&root, "notes.txt"));
    let current = at(&root, "apps/hello/current");
    let pre = notes.find("pre install 1.0").expect(&notes);
    let post = notes.find(&format!("post {}", current.display())).expect(&notes);
    assert!(pre < post && notes.starts_with("extract dep.exe 64bit"), "{notes}");
    assert!(at(&root, "apps/hello/1.0/hello.exe").is_file() && at(&root, "apps/dep/2.0/dep.exe").is_file());
    assert_eq!(read(&at(&root, "apps/hello/1.0/scoop-manifest.json")), read(&at(&root, "buckets/main/bucket/hello.json")));
    assert_eq!(
        read(&at(&root, "apps/hello/1.0/scoop-install.json")),
        "{\r\n    \"bucket\": \"main\",\r\n    \"architecture\": \"64bit\"\r\n}\r\n"
    );
    assert_eq!(std::fs::metadata(current.join("hello.exe")).unwrap().len(), body.len() as u64);
    assert!(read(&at(&root, "shims/hello.shim")).contains(&current.join("hello.exe").display().to_string()));
    assert_eq!(system.var("HELLO", false), Some(current.display().to_string()));
    assert!(at(&root, "persist/hello/data").is_dir() && at(&root, "menu/user/Programs/Scoop Apps/Hello.lnk").is_file());
    assert!(out.contains("'hello' (1.0) was installed successfully!") && out.contains(&format!("See {}", current.display())), "{out}");
    assert!(err.contains("'hello' suggests installing 'main/jq'."), "{err}");
    assert!(std::fs::read_dir(at(&root, "cache")).unwrap().count() == 2);

    let (code, _, err) = rscoop_on(&root, &["install", "hello", "-u"], &system);
    assert!(code == 0 && err.contains("is already installed"), "{err}");
    let (code, out, err) = rscoop_on(&root, &["uninstall", "hello"], &system);
    assert_eq!(code, 0, "{out}{err}");
    assert!(!at(&root, "apps/hello").exists() && system.var("HELLO", false).is_none());
    // Scoop's own: a version asked for.
    rscoop_on(&root, &["install", "dep@1.0"], &system);
    assert_eq!(read(&at(&root, "handed.txt")).trim(), "install dep@1.0");
}

#[test]
fn update_unlinks_the_old_version_and_installs_the_new_one() {
    let root = installation("update-app");
    let system = FakeSystem::new(root.join("menu"));
    // Scoop and the buckets were just updated: nothing to pull.
    rt_scoop::config::set_member(&at(&root, "home/.config/scoop/config.json"), "last_update", Some(rt_scoop::json::string("2999-01-01")))
        .unwrap();
    static BODY: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let body: &'static [u8] = BODY.get_or_init(|| exe(3));
    let (address, _) = super::tests_write::server(vec![("one.exe", body), ("two.exe", body)]);
    let hash = rt_core::sha256::hex(body);
    let manifest = |version: &str, file: &str| {
        format!(
            r#"{{"version":"{version}","url":"{address}/{file}#/up.exe","hash":"{hash}","bin":"up.exe","env_set":{{"UP":"$version"}},
            "pre_uninstall":"Note \"preun $cmd $version $old_version\"","post_install":"Note \"post $cmd $version\""}}"#
        )
    };
    write(&root, "buckets/main/bucket/up.json", &manifest("1.0", "one.exe"));
    let (code, out, err) = rscoop_on(&root, &["install", "up"], &system);
    assert_eq!(code, 0, "{out}{err}");
    let (code, out, _) = rscoop_on(&root, &["update", "up"], &system);
    assert!(code == 0 && out.contains("up: 1.0 (latest version)"), "{out}");

    write(&root, "buckets/main/bucket/up.json", &manifest("2.0", "two.exe"));
    let (code, out, err) = rscoop_on(&root, &["update", "up"], &system);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("up: 1.0 -> 2.0") && out.contains("Updating 'up' (1.0 -> 2.0)"), "{out}");
    let notes = read(&at(&root, "notes.txt"));
    assert!(notes.contains("post install 1.0\r\npreun update 2.0 1.0\r\n") && notes.ends_with("post update 2.0\r\n"), "{notes}");
    let current = at(&root, "apps/up/current");
    assert_eq!(read(&current.join("scoop-manifest.json")), manifest("2.0", "two.exe"));
    assert!(at(&root, "apps/up/1.0/up.exe").is_file() && current.join("up.exe").is_file());
    assert_eq!(system.var("UP", false).as_deref(), Some("2.0"));
    // The same version again, forced: the old folder is kept aside.
    let (code, out, err) = rscoop_on(&root, &["update", "up", "--force"], &system);
    assert_eq!(code, 0, "{out}{err}");
    assert!(at(&root, "apps/up/_2.0.old").is_dir() && at(&root, "apps/up/2.0/up.exe").is_file(), "{out}");
}

#[test]
fn import_sets_adds_installs_and_holds() {
    let root = installation("import");
    let system = FakeSystem::new(root.join("menu"));
    let config = at(&root, "home/.config/scoop/config.json");
    rt_scoop::config::set_member(&config, "last_update", Some(rt_scoop::json::string("2999-01-01"))).unwrap();
    static BODY: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let body: &'static [u8] = BODY.get_or_init(|| exe(3));
    let (address, _) = super::tests_write::server(vec![("kept.exe", body)]);
    let hash = rt_core::sha256::hex(body);
    write(
        &root,
        "buckets/main/bucket/kept.json",
        &format!(r#"{{"version":"1.0","url":"{address}/kept.exe","hash":"{hash}","bin":"kept.exe"}}"#),
    );
    let scoopfile = at(&root, "scoopfile.json");
    write(
        &root,
        "scoopfile.json",
        r#"{"config":{"aria2-enabled":"false","Show_Update_Log":false},"buckets":[{"Name":"main","Source":"https://example.org/main"}],
        "apps":[{"Name":"kept","Version":"1.0","Source":"main","Info":"Held package"},{"Name":"old","Version":"0.9","Source":"<auto-generated>","Info":""}]}"#,
    );
    let (code, out, err) = rscoop_on(&root, &["import", &scoopfile.display().to_string()], &system);
    assert_eq!(code, 0, "{out}{err}");
    let settings = rt_scoop::json::read(&config).unwrap();
    assert_eq!(settings.get("aria2-enabled"), &rt_scoop::json::Json::Bool(false));
    assert_eq!(settings.get("show_update_log"), &rt_scoop::json::Json::Bool(false));
    assert!(out.contains("'aria2-enabled' has been set to 'False'"), "{out}");
    let info = read(&at(&root, "apps/kept/1.0/scoop-install.json"));
    assert!(info.contains("\"hold\": true"), "{info}{out}{err}");
    // An app installed as app@version is Scoop's.
    assert_eq!(read(&at(&root, "handed.txt")).trim(), "install old@0.9");
    let (code, _, err) = rscoop_on(&root, &["import", &at(&root, "none.json").display().to_string()], &system);
    assert!(code == 1 && err.contains("not a valid JSON"), "{err}");
}

#[test]
fn a_failed_install_is_undone_and_a_failed_update_puts_the_old_version_back() {
    let root = installation("roll-back");
    let system = FakeSystem::new(root.join("menu"));
    rt_scoop::config::set_member(&at(&root, "home/.config/scoop/config.json"), "last_update", Some(rt_scoop::json::string("2999-01-01")))
        .unwrap();
    static BODY: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    let body: &'static [u8] = BODY.get_or_init(|| exe(3));
    let (address, _) = super::tests_write::server(vec![("one.exe", body), ("two.exe", body)]);
    let hash = rt_core::sha256::hex(body);
    // `tool` is the name of the other app's shim: it must come back.
    let manifest = |version: &str, file: &str, post: &str| {
        format!(
            r#"{{"version":"{version}","url":"{address}/{file}#/tool.exe","hash":"{hash}","bin":"tool.exe","env_set":{{"BAD":"$version"}},
            "env_add_path":".","persist":"data","post_install":"{post}"}}"#
        )
    };
    write(&root, "buckets/main/bucket/bad.json", &manifest("1.0", "one.exe", "throw 'broken'"));
    let path_before = system.var("PATH", false);
    let (code, out, err) = rscoop_on(&root, &["install", "bad"], &system);
    assert_ne!(code, 0, "{out}{err}");
    assert!(err.contains("The installation of 'bad' failed"), "{err}");
    assert!(!at(&root, "apps/bad").exists(), "{out}");
    assert!(read(&at(&root, "shims/tool.shim")).contains(r"apps\other\1.0\tool.exe"));
    assert!(!at(&root, "shims/tool.shim.other").exists());
    assert_eq!(system.var("BAD", false), None);
    assert_eq!(system.var("PATH", false).filter(|p| p.contains(r"apps\bad")), None, "{path_before:?}");
    assert!(at(&root, "persist/bad/data").is_dir(), "the persisted data stays");

    // 1.0 that works, then a 2.0 that fails: 1.0 is in use again.
    write(&root, "buckets/main/bucket/bad.json", &manifest("1.0", "one.exe", "Note 'ok'"));
    let (code, out, err) = rscoop_on(&root, &["install", "bad"], &system);
    assert_eq!(code, 0, "{out}{err}");
    write(&root, "buckets/main/bucket/bad.json", &manifest("2.0", "two.exe", "throw 'broken'"));
    let (code, out, err) = rscoop_on(&root, &["update", "bad"], &system);
    assert_ne!(code, 0, "{out}{err}");
    assert!(err.contains("'bad' (1.0) is the version in use again."), "{err}");
    let current = at(&root, "apps/bad/current");
    assert_eq!(rt_scoop::env::absolute(&std::fs::read_link(&current).unwrap()), at(&root, "apps/bad/1.0"));
    assert!(!at(&root, "apps/bad/2.0").exists());
    assert!(read(&at(&root, "shims/tool.shim")).contains(&current.join("tool.exe").display().to_string()));
    assert_eq!(system.var("BAD", false).as_deref(), Some("1.0"));
    assert!(system.var("PATH", false).unwrap().contains(&current.display().to_string()));
}
