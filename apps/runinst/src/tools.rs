//! Tabs of their own: Windows' optional features and capabilities (DISM),
//! and what development tools installed: npm's global packages, pip's
//! packages, `cargo install`'s crates and vcpkg's ports. A tab is there only
//! when its tool is (DISM only on Windows).
//!
//! Each is read on a thread of its own (npm and pip take a second or more);
//! the text readers below work on any system, for the tests.

use crate::app::Tab;
use crate::inventory::{Program, Scope, Source};
use rt_core::json::Json;
use std::path::{Path, PathBuf};

/// A program file on the `PATH` (on Windows also `.exe`, `.cmd`, `.bat`; the
/// Store's stand-ins in `WindowsApps` do not count: they only open the Store).
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ".bat"] } else { &[""] };
    for dir in std::env::split_paths(&path) {
        if cfg!(windows) && dir.to_string_lossy().contains("WindowsApps") {
            continue;
        }
        for e in exts {
            let p = dir.join(format!("{name}{e}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// The program a Scoop shim stands for (`<shim>.shim`: `path = "…"`), or the
/// file itself.
pub fn real_file(file: &Path) -> PathBuf {
    let shim = file.with_extension("shim");
    std::fs::read_to_string(&shim)
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("path").map(|r| r.trim_start_matches([' ', '=']).trim().trim_matches('"').to_string())))
        .map_or_else(|| file.to_path_buf(), PathBuf::from)
}

/// For whom a package in `path` is: everyone in the system's folders, else
/// the user running runinst.
fn scope_of(path: &str, me: &str) -> Scope {
    let p = path.replace('\\', "/").to_lowercase();
    let system = ["/usr/", "/opt/", "c:/program files", "c:/programdata"].iter().any(|s| p.starts_with(s));
    if system {
        Scope::Machine
    } else {
        Scope::User { name: me.to_string(), current: true }
    }
}

fn date_of(secs: i64) -> Option<(i32, u32, u32)> {
    (secs > 0).then(|| rt_scan::format::civil((secs / 86_400) as i32))
}

fn mtime(p: &Path) -> Option<i64> {
    let t = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
}

// ------------------------------------------------------------------ DISM

/// What `Get-CimInstance Win32_OptionalFeature` prints as asked
/// (`name<TAB>state<TAB>caption`): the features turned on (state 1).
pub fn parse_features(text: &str) -> Vec<Program> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (name, state, caption) = (f.next()?.trim(), f.next()?.trim(), f.next().unwrap_or("").trim());
            (state == "1" && !name.is_empty()).then(|| Program {
                name: if caption.is_empty() { name.to_string() } else { caption.to_string() },
                publisher: "Microsoft".into(),
                key: format!("DISM feature: {name}"),
                values: vec![("Package".into(), name.to_string()), ("Kind".into(), "feature".into())],
                sources: vec![Source::Dism],
                ..Program::default()
            })
        })
        .collect()
}

/// What `dism /Online /Get-Capabilities /Format:Table /English` prints: the
/// capabilities installed (`App.StepsRecorder~~~~0.0.1.0 | Installed`).
pub fn parse_capabilities(text: &str) -> Vec<Program> {
    text.lines()
        .filter_map(|l| {
            let (id, state) = l.split_once('|')?;
            let (id, state) = (id.trim(), state.trim());
            if state != "Installed" || !id.contains('~') {
                return None;
            }
            let mut parts = id.split('~').filter(|s| !s.is_empty());
            let name = parts.next()?.to_string();
            Some(Program {
                name,
                publisher: "Microsoft".into(),
                version: parts.next_back().unwrap_or("").to_string(),
                key: format!("DISM capability: {id}"),
                values: vec![("Package".into(), id.to_string()), ("Kind".into(), "capability".into())],
                sources: vec![Source::Dism],
                ..Program::default()
            })
        })
        .collect()
}

// ------------------------------------------------------------------ npm

/// A global package of npm, from its `package.json` in `dir`.
pub fn npm_package(dir: &Path, text: &str, me: &str) -> Option<Program> {
    let j = rt_core::json::parse(text).ok()?;
    let name = j.get("name").str();
    if name.is_empty() {
        return None;
    }
    // `author`: "Name <mail> (url)" or { "name": … }.
    let author = match j.get("author") {
        Json::Str(s) => s.split(['<', '(']).next().unwrap_or("").trim().to_string(),
        a => a.get("name").str().to_string(),
    };
    let mut values = vec![("Package".to_string(), name.to_string())];
    for k in ["description", "homepage", "license"] {
        let v = j.get(k).str();
        if !v.is_empty() {
            values.push((k.to_string(), v.to_string()));
        }
    }
    let loc = dir.to_string_lossy().into_owned();
    Some(Program {
        name: name.to_string(),
        publisher: author,
        version: j.get("version").str().to_string(),
        size: Some(crate::leftovers::size_of(dir, 50_000)),
        date: mtime(dir).and_then(date_of),
        scope: scope_of(&loc, me),
        location: Some(dir.to_path_buf()),
        key: format!("npm: {name}"),
        values,
        sources: vec![Source::Npm],
        ..Program::default()
    })
}

/// The global packages in npm's `root` (`npm root -g`), scoped ones
/// (`@scope/name`) too.
pub fn npm_packages(root: &Path, me: &str) -> Vec<Program> {
    let mut dirs = Vec::new();
    for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with('@') {
            dirs.extend(std::fs::read_dir(e.path()).into_iter().flatten().flatten().map(|s| s.path()));
        } else if !n.starts_with('.') {
            dirs.push(e.path());
        }
    }
    dirs.iter().filter_map(|d| std::fs::read_to_string(d.join("package.json")).ok().and_then(|t| npm_package(d, &t, me))).collect()
}

// ------------------------------------------------------------------ pip

/// What `python -m pip list -v --format=json` prints. Packages another
/// installer put there (the system's package manager) are parts of the
/// system and are not removed from here.
/// `top`: what `pip list --not-required` prints, the packages no other
/// needs (the others are dependencies: hidden as parts of the system, as apt's
/// automatic ones), when it could be read.
pub fn parse_pip(text: &str, top: Option<&str>, python: &str, me: &str) -> Vec<Program> {
    let Ok(j) = rt_core::json::parse(text) else { return Vec::new() };
    let top: Option<std::collections::HashSet<String>> =
        top.and_then(|t| rt_core::json::parse(t).ok()).map(|t| t.items().iter().map(|p| p.get("name").str().to_lowercase()).collect());
    j.items()
        .iter()
        .filter_map(|p| {
            let name = p.get("name").str();
            if name.is_empty() {
                return None;
            }
            let location = p.get("location").str();
            let installer = p.get("installer").str();
            // Without a word of who installed it, the system's folders say.
            let scope = scope_of(location, me);
            let by_pip = matches!(installer, "pip" | "uv" | "pipx") || installer.is_empty() && scope != Scope::Machine;
            let mut values = vec![("Package".to_string(), name.to_string()), ("Python".into(), python.to_string())];
            if !installer.is_empty() {
                values.push(("Installer".into(), installer.to_string()));
            }
            if !by_pip {
                values.push(("Managed".into(), "1".into()));
            }
            let dependency = top.as_ref().is_some_and(|t| !t.contains(&name.to_lowercase()));
            if dependency {
                values.push(("Dependency".into(), "1".into()));
            }
            let version = p.get("version").str();
            // Its `<name>-<version>.dist-info` says when it was installed.
            let info = [name.to_string(), name.replace('-', "_"), name.replace('-', "_").to_lowercase()]
                .iter()
                .map(|n| Path::new(location).join(format!("{n}-{version}.dist-info")))
                .find(|d| d.is_dir());
            Some(Program {
                name: name.to_string(),
                version: version.to_string(),
                date: info.as_deref().and_then(mtime).and_then(date_of),
                scope,
                location: (!location.is_empty()).then(|| PathBuf::from(location)),
                system: !by_pip || dependency,
                key: format!("pip: {name}"),
                values,
                sources: vec![Source::Pip],
                ..Program::default()
            })
        })
        .collect()
}

/// A pip package put there by another installer (the system's packages).
pub fn pip_by_other(p: &Program) -> bool {
    crate::linux_pkgs::value(p, "Managed").is_some()
}

// ------------------------------------------------------------------ Cargo

/// `cargo install`'s record (`<CARGO_HOME>/.crates2.json`): each crate, its
/// source and its programs in `bin` (their size and date).
pub fn parse_crates(text: &str, bin: &Path, me: &str) -> Vec<Program> {
    let Ok(j) = rt_core::json::parse(text) else { return Vec::new() };
    let Json::Obj(installs) = j.get("installs") else { return Vec::new() };
    installs
        .iter()
        .filter_map(|(key, v)| {
            // "ripgrep 14.1.0 (registry+https://github.com/rust-lang/crates.io-index)"
            let mut parts = key.splitn(3, ' ');
            let (name, version) = (parts.next()?, parts.next().unwrap_or(""));
            let source = parts.next().unwrap_or("").trim_matches(['(', ')']);
            let origin = match source.split_once('+') {
                Some(("registry", u)) if u.contains("crates.io") => "crates.io".to_string(),
                Some((kind, u)) => format!("{kind}: {}", u.split('#').next().unwrap_or(u).trim_start_matches("https://").trim_start_matches("file:///")),
                None => source.to_string(),
            };
            let bins: Vec<PathBuf> = v.get("bins").items().iter().map(|b| bin.join(b.str())).collect();
            let size = bins.iter().filter_map(|b| std::fs::metadata(b).ok()).map(|m| m.len()).sum::<u64>();
            let mut values = vec![("Package".to_string(), name.to_string()), ("Source".into(), source.to_string())];
            values.push(("Programs".into(), v.get("bins").items().iter().map(|b| b.str()).collect::<Vec<_>>().join(", ")));
            Some(Program {
                name: name.to_string(),
                publisher: origin,
                version: version.to_string(),
                size: (size > 0).then_some(size),
                date: bins.first().and_then(|b| mtime(b)).and_then(date_of),
                scope: Scope::User { name: me.to_string(), current: true },
                location: Some(bin.to_path_buf()),
                key: format!("cargo: {name}"),
                values,
                sources: vec![Source::Cargo],
                ..Program::default()
            })
        })
        .collect()
}

// ------------------------------------------------------------------ vcpkg

/// vcpkg's record (`<root>/installed/vcpkg/status`, as dpkg writes its own):
/// each port installed for each triplet (its features are not ports).
pub fn parse_vcpkg(text: &str, root: &Path) -> Vec<Program> {
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let get = |name: &str| block.lines().find_map(|l| l.strip_prefix(name).and_then(|r| r.strip_prefix(':'))).map(str::trim);
        let (Some(name), Some(status)) = (get("Package"), get("Status")) else { continue };
        if get("Feature").is_some() || !status.ends_with(" installed") || status.starts_with("purge") {
            continue;
        }
        let triplet = get("Architecture").unwrap_or("");
        let version = match get("Port-Version") {
            Some(pv) if pv != "0" => format!("{}#{pv}", get("Version").unwrap_or("")),
            _ => get("Version").unwrap_or("").to_string(),
        };
        let spec = if triplet.is_empty() { name.to_string() } else { format!("{name}:{triplet}") };
        let mut values = vec![("Package".to_string(), spec.clone())];
        if let Some(d) = get("Description") {
            values.push(("Description".into(), d.to_string()));
        }
        let dir = root.join("installed").join(triplet);
        out.push(Program {
            name: name.to_string(),
            publisher: triplet.to_string(),
            version,
            location: dir.is_dir().then_some(dir),
            key: format!("vcpkg: {spec}"),
            values,
            sources: vec![Source::Vcpkg],
            ..Program::default()
        });
    }
    out
}

/// vcpkg's folder: `VCPKG_ROOT`, or the folder of the `vcpkg` on the `PATH`
/// (through Scoop's shim), where `.vcpkg-root` is.
pub fn vcpkg_root() -> Option<(PathBuf, PathBuf)> {
    let exe = which("vcpkg").map(|f| real_file(&f));
    let root = std::env::var_os("VCPKG_ROOT").map(PathBuf::from).filter(|r| r.join(".vcpkg-root").exists()).or_else(|| exe.as_ref()?.parent().map(Path::to_path_buf))?;
    let exe = exe.unwrap_or_else(|| root.join(if cfg!(windows) { "vcpkg.exe" } else { "vcpkg" }));
    root.join(".vcpkg-root").exists().then_some((root, exe))
}

// ------------------------------------------------------------------ reading

/// What a command prints, when it ran well (no window on Windows).
fn output(file: &Path, args: &[&str]) -> Option<String> {
    let mut c = std::process::Command::new(file);
    c.args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let o = c.output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// npm through `cmd` on Windows (it is a `.cmd`).
pub fn npm_command() -> Option<(PathBuf, Vec<String>)> {
    let npm = which("npm")?;
    Some(if cfg!(windows) { (PathBuf::from("cmd.exe"), vec!["/d".into(), "/c".into(), "npm".into()]) } else { (npm, Vec::new()) })
}

/// The Python whose pip is listed: `python3`, `py`, then `python`.
pub fn python() -> Option<PathBuf> {
    ["python3", "py", "python"].iter().find_map(|n| which(n))
}

/// `CARGO_HOME`, or `~/.cargo`.
pub fn cargo_home(me: &crate::inventory::Me) -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| me.home.as_ref().map(|h| h.join(".cargo"))).filter(|h| h.is_dir())
}

/// Reads every tab of this file whose tool is there, each on a thread.
pub fn read_all(me: &crate::inventory::Me, admin: bool) -> Vec<(Tab, Vec<Program>)> {
    let name = me.name.clone();
    std::thread::scope(|s| {
        let dism = s.spawn(|| cfg!(windows).then(|| dism(admin)));
        let npm = s.spawn(|| {
            let (file, pre) = npm_command()?;
            let mut args: Vec<&str> = pre.iter().map(String::as_str).collect();
            args.extend(["root", "-g"]);
            let root = PathBuf::from(output(&file, &args)?.trim());
            Some(npm_packages(&root, &name))
        });
        let pip = s.spawn(|| {
            let py = python()?;
            let top = std::thread::scope(|s2| {
                let h = s2.spawn(|| output(&py, &["-m", "pip", "list", "--not-required", "--format=json", "--disable-pip-version-check"]));
                let all = output(&py, &["-m", "pip", "list", "-v", "--format=json", "--disable-pip-version-check"]);
                (all, h.join().ok().flatten())
            });
            let (text, top) = (top.0?, top.1);
            Some(parse_pip(&text, top.as_deref(), &py.to_string_lossy(), &name))
        });
        let cargo = s.spawn(|| {
            which("cargo")?;
            let home = cargo_home(me)?;
            let text = std::fs::read_to_string(home.join(".crates2.json")).unwrap_or_default();
            Some(parse_crates(&text, &home.join("bin"), &name))
        });
        let vcpkg = s.spawn(|| {
            let (root, _) = vcpkg_root()?;
            let text = std::fs::read_to_string(root.join("installed").join("vcpkg").join("status")).unwrap_or_default();
            Some(parse_vcpkg(&text, &root))
        });
        [(Tab::Dism, dism), (Tab::Npm, npm), (Tab::Pip, pip), (Tab::Cargo, cargo), (Tab::Vcpkg, vcpkg)]
            .into_iter()
            .filter_map(|(t, h)| h.join().ok().flatten().map(|l| (t, l)))
            .collect()
    })
}

/// Windows' features turned on (WMI: any user can read them) and, with
/// administrator rights, its capabilities (`dism` needs them).
fn dism(admin: bool) -> Vec<Program> {
    let ps = "Get-CimInstance Win32_OptionalFeature | ForEach-Object { $_.Name + [char]9 + $_.InstallState + [char]9 + $_.Caption }";
    let mut out = output(Path::new("powershell.exe"), &["-NoProfile", "-NonInteractive", "-Command", ps]).map(|t| parse_features(&t)).unwrap_or_default();
    if admin {
        if let Some(t) = output(Path::new("dism.exe"), &["/Online", "/Get-Capabilities", "/Format:Table", "/English"]) {
            out.extend(parse_capabilities(&t));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What this computer has: `cargo test -p runinst --lib tools_here -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn tools_here() {
        let t = std::time::Instant::now();
        let me = crate::inventory::me();
        let all = read_all(&me, std::env::var("RUNINST_ADMIN").is_ok());
        println!("{} tabs in {:?}", all.len(), t.elapsed());
        for (tab, list) in all {
            println!("{tab:?}: {} ({} shown)", list.len(), list.iter().filter(|p| p.visible()).count());
            for p in list.iter().take(12) {
                println!("  {} {} [{}] {:?} {:?} {:?}", p.name, p.version, p.publisher, p.size, p.date, p.scope);
            }
        }
    }

    #[test]
    fn dism_lists() {
        let f = parse_features("Printing-PrintToPDFServices-Features\t1\tMicrosoft Print to PDF\nWindows-Defender-Default-Definitions\t3\t\nTelnetClient\t1\t\n");
        assert_eq!(f.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Microsoft Print to PDF", "TelnetClient"]);
        assert_eq!(crate::linux_pkgs::package_id(&f[0]), "Printing-PrintToPDFServices-Features");
        let c = parse_capabilities(
            "Capability listing:\n\n---------- | --------\nCapability Identity | State\n---------- | --------\nApp.StepsRecorder~~~~0.0.1.0 | Installed\nBrowser.InternetExplorer~~~~0.0.11.0 | Not Present\n",
        );
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].name.as_str(), c[0].version.as_str(), crate::linux_pkgs::package_id(&c[0])), ("App.StepsRecorder", "0.0.1.0", "App.StepsRecorder~~~~0.0.1.0"));
    }

    #[test]
    fn npm_and_pip() {
        let dir = Path::new("C:/Users/ana/AppData/Roaming/npm/node_modules/@google/gemini-cli");
        let p = npm_package(dir, r#"{"name":"@google/gemini-cli","version":"0.9.0","author":"Google <x@y.z>","description":"Gemini in the terminal"}"#, "ana").unwrap();
        assert_eq!((p.name.as_str(), p.version.as_str(), p.publisher.as_str()), ("@google/gemini-cli", "0.9.0", "Google"));
        assert_eq!(p.scope, Scope::User { name: "ana".into(), current: true });
        let p = npm_package(Path::new("/usr/lib/node_modules/npm"), r#"{"name":"npm","version":"10.1","author":{"name":"GitHub Inc."}}"#, "ana").unwrap();
        assert_eq!((p.publisher.as_str(), p.scope.clone()), ("GitHub Inc.", Scope::Machine));
        let ps = parse_pip(
            r#"[{"name":"affine","version":"3.0.1","location":"D:\\Python\\Lib\\site-packages","installer":"pip"},{"name":"PyYAML","version":"6.0","location":"/usr/lib/python3/dist-packages","installer":"debian"}]"#,
            None,
            "python3",
            "ana",
        );
        assert_eq!(ps.len(), 2);
        let deps = parse_pip(r#"[{"name":"anyio","version":"4"},{"name":"Flask","version":"3"}]"#, Some(r#"[{"name":"flask","version":"3"}]"#), "py", "ana");
        assert!(deps[0].system && !deps[1].system, "a dependency is hidden; what was asked for is not");
        assert!(!ps[0].system && !pip_by_other(&ps[0]));
        assert!(ps[1].system && pip_by_other(&ps[1]) && ps[1].scope == Scope::Machine, "the system's package manager put it there");
    }

    #[test]
    fn crates_and_ports() {
        let t = r#"{"installs":{"ripgrep 14.1.0 (registry+https://github.com/rust-lang/crates.io-index)":{"bins":["rg.exe"]},"mytool 0.1.0 (git+https://github.com/me/mytool#abc123)":{"bins":["mytool.exe","helper.exe"]}}}"#;
        let cs = parse_crates(t, Path::new("Z:/nowhere/bin"), "ana");
        assert_eq!(cs.len(), 2);
        assert_eq!((cs[0].name.as_str(), cs[0].version.as_str(), cs[0].publisher.as_str()), ("ripgrep", "14.1.0", "crates.io"));
        assert_eq!(cs[1].publisher, "git: github.com/me/mytool");
        assert_eq!(crate::linux_pkgs::value(&cs[1], "Programs"), Some("mytool.exe, helper.exe"));
        let status = "Package: zlib\nVersion: 1.3.1\nPort-Version: 0\nArchitecture: x64-windows\nDescription: A compression library\nStatus: install ok installed\n\n\
                      Package: curl\nVersion: 8.10.1\nPort-Version: 2\nArchitecture: x64-windows\nStatus: install ok installed\n\n\
                      Package: curl\nFeature: ssl\nArchitecture: x64-windows\nStatus: install ok installed\n\n\
                      Package: fmt\nVersion: 11\nArchitecture: x64-windows\nStatus: purge ok not-installed\n";
        let ps = parse_vcpkg(status, Path::new("Z:/vcpkg"));
        assert_eq!(ps.iter().map(|p| (p.name.as_str(), p.version.as_str())).collect::<Vec<_>>(), [("zlib", "1.3.1"), ("curl", "8.10.1#2")]);
        assert_eq!(crate::linux_pkgs::package_id(&ps[1]), "curl:x64-windows");
    }

    #[test]
    fn scoop_shims() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("shim-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("vcpkg.shim"), "path = \"D:\\Portable\\scoop\\apps\\vcpkg\\current\\vcpkg.exe\"\n").unwrap();
        assert_eq!(real_file(&dir.join("vcpkg.exe")), PathBuf::from("D:\\Portable\\scoop\\apps\\vcpkg\\current\\vcpkg.exe"));
        assert_eq!(real_file(&dir.join("other.exe")), dir.join("other.exe"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
