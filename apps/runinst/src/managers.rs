//! Package managers with records of their own, read from their folders
//! (faster than running them): Scoop (`scoop list`) and Chocolatey
//! (`choco list`). Each has its own tab when it is there.

use crate::inventory::{Program, Scope, Source};
use rt_core::json::Json;
use std::path::{Path, PathBuf};

/// Scoop's folders: the desktop user's (their `SCOOP`, or `~\scoop`) and the
/// global one (`SCOOP_GLOBAL`, or `C:\ProgramData\scoop`), those that exist.
pub fn scoop_roots(me: &crate::inventory::Me) -> Vec<(PathBuf, bool)> {
    let env = |v: &str| std::env::var_os(v).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty());
    // When runinst runs as another account, its variables are that
    // account's: the desktop user's `SCOOP` is in their registry.
    let user_var = if me.runs_as { env("SCOOP") } else { user_variable(me, "SCOOP") };
    let user = user_var.or_else(|| me.home.as_ref().map(|h| h.join("scoop")));
    let global = env("SCOOP_GLOBAL").or_else(|| env("ProgramData").map(|d| d.join("scoop")));
    [(user, false), (global, true)].into_iter().filter_map(|(p, g)| p.filter(|p| p.join("apps").is_dir()).map(|p| (p, g))).collect()
}

/// A variable of the desktop user's environment (their `Environment` key).
fn user_variable(me: &crate::inventory::Me, name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use rt_sys::win::reg::{Key, Root, View};
        Key::open(Root::Users, &format!("{}\\Environment", me.sid), View::Bits64)?.string(name).map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        let _ = (me, name);
        None
    }
}

/// Every app of Scoop, the desktop user's and the global ones.
pub fn scoop_all(me: &crate::inventory::Me) -> Vec<Program> {
    let roots = scoop_roots(me);
    let buckets = scoop_buckets(&roots);
    roots.iter().flat_map(|(root, global)| scoop_apps(root, *global, &me.name, &buckets)).collect()
}

/// The buckets Scoop has (`<root>\buckets\<name>`), from the user's root
/// (the global one uses them too).
pub fn scoop_buckets(roots: &[(PathBuf, bool)]) -> Vec<PathBuf> {
    roots
        .iter()
        .flat_map(|(r, _)| std::fs::read_dir(r.join("buckets")).map(|rd| rd.flatten().map(|e| e.path()).collect::<Vec<_>>()).unwrap_or_default())
        .collect()
}

/// Whether a bucket still has the app's manifest (in `bucket\`, or at its
/// top as old buckets have it).
fn has_manifest(bucket: &Path, app: &str) -> bool {
    let file = format!("{app}.json");
    bucket.join("bucket").join(&file).is_file() || bucket.join(&file).is_file() || bucket.join("deprecated").join(&file).is_file()
}

/// The apps Scoop installed in `root` (`global`: for every user).
/// `user`: the desktop user's name; `buckets`: the buckets there are, to see
/// whether an app's manifest is still in one (`scoop list` says "Manifest
/// removed" otherwise; "Install failed" when `install.json` is missing).
pub fn scoop_apps(root: &Path, global: bool, user: &str, buckets: &[PathBuf]) -> Vec<Program> {
    let Ok(rd) = std::fs::read_dir(root.join("apps")) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        // Scoop keeps itself here too.
        if name.eq_ignore_ascii_case("scoop") {
            continue;
        }
        let current = e.path().join("current");
        // Scoop writes `manifest.json` and `install.json`; newer ones, with
        // a `scoop-` before the name.
        let read = |f: &str| {
            [format!("scoop-{f}"), f.to_string()]
                .iter()
                .find_map(|n| std::fs::read_to_string(current.join(n)).ok())
                .and_then(|t| rt_core::json::parse(t.trim_start_matches('\u{feff}')).ok())
        };
        let manifest = read("manifest.json").unwrap_or(Json::Null);
        let install = read("install.json").unwrap_or(Json::Null);
        // Without a manifest, the version is the folder `current` leads to.
        let linked = std::fs::read_link(&current).ok().and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned())).filter(|n| n != "current");
        let version = Some(manifest.get("version").str().to_string()).filter(|v| !v.is_empty()).or(linked).unwrap_or_default();
        let mtime = std::fs::metadata(&current).ok().and_then(|m| m.modified().ok()).map(|t| match t.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_secs() as i64,
            Err(_) => 0,
        });
        let bucket = install.get("bucket").str().to_string();
        let mut problems = Vec::new();
        if matches!(install, Json::Null) {
            problems.push(crate::inventory::Problem::InstallFailed);
        }
        // Installed from a URL or a file: no bucket to look in.
        let from_url = !install.get("url").str().is_empty();
        let in_bucket = if bucket.is_empty() {
            buckets.iter().any(|b| has_manifest(b, &name))
        } else {
            buckets.iter().filter(|b| b.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&bucket))).any(|b| has_manifest(b, &name))
        };
        if !from_url && !buckets.is_empty() && !in_bucket {
            problems.push(crate::inventory::Problem::ManifestRemoved);
        }
        let mut values: Vec<(String, String)> = Vec::new();
        for (k, v) in [("description", manifest.get("description").str()), ("homepage", manifest.get("homepage").str()), ("bucket", bucket.as_str()), ("architecture", install.get("architecture").str())] {
            if !v.is_empty() {
                values.push((k.to_string(), v.to_string()));
            }
        }
        out.push(Program {
            name: name.clone(),
            publisher: bucket,
            version,
            date: mtime.filter(|&s| s > 0).map(|s| rt_scan::format::civil((s / 86_400) as i32)),
            location: Some(current.clone()),
            uninstall: Some(format!("scoop uninstall {}{name}", if global { "--global " } else { "" })),
            scope: if global { Scope::Machine } else { Scope::User { name: user.to_string(), current: true } },
            key: e.path().display().to_string(),
            values,
            sources: vec![Source::Scoop],
            problems,
            ..Program::default()
        });
    }
    out
}

/// Chocolatey's folder (`ChocolateyInstall`, or `C:\ProgramData\chocolatey`), if it exists.
pub fn choco_root() -> Option<PathBuf> {
    let env = |v: &str| std::env::var_os(v).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty());
    env("ChocolateyInstall").or_else(|| env("ProgramData").map(|d| d.join("chocolatey"))).filter(|p| p.join("lib").is_dir())
}

/// The text of `<tag>…</tag>` in a small XML file (a `.nuspec`).
fn xml_tag(xml: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let Some(a) = xml.find(&open) else { return String::new() };
    let rest = &xml[a + open.len()..];
    let text = rest.split(&format!("</{tag}>")).next().unwrap_or("");
    text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").trim().to_string()
}

/// The packages Chocolatey installed (every one is for every user).
pub fn choco_packages(root: &Path) -> Vec<Program> {
    let Ok(rd) = std::fs::read_dir(root.join("lib")) else { return Vec::new() };
    let mut out = Vec::new();
    for e in rd.flatten() {
        let dir = e.path();
        let id = e.file_name().to_string_lossy().into_owned();
        let Some(nuspec) = std::fs::read_dir(&dir).ok().and_then(|rd| rd.flatten().map(|f| f.path()).find(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("nuspec")))) else { continue };
        let xml = std::fs::read_to_string(&nuspec).unwrap_or_default();
        let title = xml_tag(&xml, "title");
        let mut values = Vec::new();
        for k in ["id", "summary", "projectUrl", "tags"] {
            let v = xml_tag(&xml, k);
            if !v.is_empty() {
                values.push((k.to_string(), v));
            }
        }
        out.push(Program {
            name: if title.is_empty() { id.clone() } else { title },
            publisher: xml_tag(&xml, "authors"),
            version: xml_tag(&xml, "version"),
            location: Some(dir.clone()),
            uninstall: Some(format!("choco uninstall {id}")),
            scope: Scope::Machine,
            key: nuspec.display().to_string(),
            values,
            sources: vec![Source::Chocolatey],
            ..Program::default()
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("runinst-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn scoop_and_chocolatey_folders() {
        let d = scratch("managers");
        let app = d.join("scoop/apps/ripgrep/current");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("scoop-manifest.json"), "\u{feff}{\"version\": \"14.1.0\", \"description\": \"Search tool\", \"homepage\": \"https://x\"}").unwrap();
        std::fs::write(app.join("install.json"), "{\"bucket\": \"main\", \"architecture\": \"64bit\"}").unwrap();
        std::fs::create_dir_all(d.join("scoop/apps/scoop/current")).unwrap();
        std::fs::create_dir_all(d.join("scoop/buckets/main/bucket")).unwrap();
        std::fs::write(d.join("scoop/buckets/main/bucket/ripgrep.json"), "{}").unwrap();
        // An install that did not finish, whose manifest is gone.
        std::fs::create_dir_all(d.join("scoop/apps/broken/1.0")).unwrap();
        let buckets = scoop_buckets(&[(d.join("scoop"), false)]);
        let mut apps = scoop_apps(&d.join("scoop"), false, "me", &buckets);
        apps.sort_by(|a, b| b.name.cmp(&a.name));
        assert_eq!(apps.len(), 2);
        let a = &apps[0];
        assert_eq!((a.name.as_str(), a.version.as_str(), a.publisher.as_str()), ("ripgrep", "14.1.0", "main"));
        assert_eq!(a.uninstall.as_deref(), Some("scoop uninstall ripgrep"));
        assert!(a.values.iter().any(|(k, v)| k == "description" && v == "Search tool"));
        assert!(a.problems.is_empty());
        use crate::inventory::Problem;
        assert_eq!(apps[1].problems, [Problem::InstallFailed, Problem::ManifestRemoved]);
        let g = scoop_apps(&d.join("scoop"), true, "me", &buckets);
        assert!(g.iter().any(|p| p.uninstall.as_deref() == Some("scoop uninstall --global ripgrep")));
        let lib = d.join("choco/lib/git");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("git.nuspec"), "<?xml?><package><metadata><id>git</id><version>2.47.0</version><title>Git</title><authors>Git &amp; friends</authors></metadata></package>").unwrap();
        let p = choco_packages(&d.join("choco"));
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].name.as_str(), p[0].version.as_str(), p[0].publisher.as_str()), ("Git", "2.47.0", "Git & friends"));
        assert_eq!(p[0].uninstall.as_deref(), Some("choco uninstall git"));
    }
}
