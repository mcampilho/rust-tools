//! `Get-Manifest` (`lib\manifest.ps1`) and `lib\depends.ps1`: from what the
//! user typed to a manifest, and what has to be installed before it.

use crate::apps;
use crate::buckets;
use crate::install::Scoop;
use crate::manifest::{strings, truthy, Arch, Manifest};
use std::path::{Path, PathBuf};

/// `[bucket/]app[@version]`.
#[derive(Debug, PartialEq, Eq)]
pub struct AppRef {
    pub bucket: Option<String>,
    pub app: String,
    pub version: Option<String>,
}

/// `parse_app`. A path or an address comes back whole, as the app.
pub fn parse_app(text: &str) -> AppRef {
    let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    let (name, version) = match text.split_once('@') {
        Some((n, v)) => (n, Some(v.to_string())),
        None => (text, None),
    };
    let (bucket, app) = match name.split_once('/') {
        Some((b, a)) if plain(b) => (Some(b.to_string()), a),
        _ => (None, name),
    };
    if plain(app) || (app.ends_with(".json") && bucket.is_some()) {
        AppRef { bucket, app: app.to_string(), version }
    } else {
        AppRef { bucket: None, app: text.to_string(), version: None }
    }
}

/// A web address or a network path.
pub fn is_remote(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    ["http://", "https://", "ftp://", "ftps://", r"\\"].iter().any(|p| lower.starts_with(p))
}

/// `appname_from_url`.
fn name_from_path(path: &str) -> String {
    let leaf = path.rsplit(['/', '\\']).next().unwrap_or(path);
    leaf.strip_suffix(".json").unwrap_or(leaf).to_string()
}

#[derive(Debug)]
pub struct Resolved {
    pub app: String,
    pub bucket: Option<String>,
    /// Set for an app that comes from a file or an address, not from a bucket.
    pub url: Option<String>,
    pub manifest: Option<Manifest>,
    /// The file the manifest was read from.
    pub path: Option<PathBuf>,
    /// Other buckets that have it too.
    pub also_in: Vec<String>,
}

impl Resolved {
    fn new(app: String) -> Resolved {
        Resolved { app, bucket: None, url: None, manifest: None, path: None, also_in: Vec::new() }
    }

    fn read(mut self, path: Option<PathBuf>) -> Resolved {
        self.manifest = path.as_deref().and_then(Manifest::read);
        self.path = path.filter(|_| self.manifest.is_some());
        self
    }
}

/// `Get-Manifest`. An installed app without a bucket in the query is looked up
/// where it was installed from. Web addresses are not fetched: they come back
/// without a manifest.
pub fn resolve(scoop: &Scoop, query: &str) -> Resolved {
    let query = query.trim_start_matches('/');
    if is_remote(query) {
        let mut r = Resolved::new(name_from_path(query));
        r.url = Some(query.to_string());
        return r;
    }
    let parsed = parse_app(query);
    if parsed.bucket.is_none() && apps::installed(scoop, &parsed.app, None) {
        let app = parsed.app.clone();
        let global = apps::installed(scoop, &app, Some(true));
        if let Some(info) = apps::current_version(scoop, &app, global).and_then(|v| apps::install_info(scoop, &app, &v, global)) {
            let mut r = Resolved::new(app.clone());
            if let Some(bucket) = info.bucket {
                let path = buckets::manifest_path(scoop, &app, &bucket).or_else(|| buckets::deprecated(scoop, &app, &bucket));
                r.bucket = Some(bucket);
                return r.read(path);
            }
            let local = info.url.as_deref().map(PathBuf::from).filter(|p| p.is_file());
            let installed = apps::current_dir(scoop, &app, global).join("scoop-manifest.json");
            r.url = info.url;
            return r.read(local.or(Some(installed)));
        }
    }
    let mut r = Resolved::new(parsed.app.clone());
    if let Some(bucket) = parsed.bucket {
        let path = buckets::manifest_path(scoop, &parsed.app, &bucket);
        r.bucket = Some(bucket);
        return r.read(path);
    }
    let mut found = buckets::containing(scoop, &parsed.app);
    if !found.is_empty() {
        let (bucket, path) = found.remove(0);
        r.bucket = Some(bucket);
        r.also_in = found.into_iter().map(|(b, _)| b).collect();
        return r.read(Some(path));
    }
    if Path::new(&parsed.app).is_file() {
        let path = std::path::absolute(&parsed.app).unwrap_or_else(|_| PathBuf::from(&parsed.app));
        r.app = name_from_path(&parsed.app);
        r.url = Some(path.display().to_string());
        return r.read(Some(path));
    }
    r
}

/// `Get-HelperPath`.
pub fn helper(scoop: &Scoop, app: &str, file: &str) -> Option<PathBuf> {
    [false, true].iter().map(|&g| apps::current_dir(scoop, app, g).join(file)).find(|p| p.is_file())
}

fn helper_installed(scoop: &Scoop, name: &str) -> bool {
    let candidates: &[(&str, &str)] = match name {
        "7zip" => &[("7zip", "7z.exe")],
        "lessmsi" => &[("lessmsi", "lessmsi.exe")],
        "innounp" => &[("innounp-unicode", "innounp.exe"), ("innounp", "innounp.exe")],
        "dark" => &[("dark", "dark.exe"), ("wixtoolset", "wix.exe")],
        _ => &[],
    };
    candidates.iter().any(|(app, file)| helper(scoop, app, file).is_some())
}

/// `Test-7zipRequirement`: does 7-Zip unpack this download?
pub fn needs_7zip(url: &str) -> bool {
    const KINDS: [&str; 22] = [
        "001", "7z", "bz", "bz2", "bzip", "bzip2", "gz", "img", "iso", "lzma", "lzh", "nupkg", "rar", "tar", "tbz", "tgz", "tpz", "txz",
        "tzst", "zst", "xz", "tbz2",
    ];
    let lower = url.to_ascii_lowercase();
    let mut parts = lower.rsplit('.');
    let is_kind = |s: &str| KINDS.contains(&s);
    let last = parts.next().unwrap_or("");
    // One more extension after the archive's, as in `.7z.exe`, still counts.
    let second_counts = !last.is_empty() && !last.chars().any(|c| c.is_ascii_digit());
    is_kind(last) || (second_counts && parts.next().is_some_and(is_kind))
}

/// The apps that can be each helper.
fn helper_apps(name: &str) -> &'static [&'static str] {
    match name {
        "7zip" => &["7zip"],
        "lessmsi" => &["lessmsi"],
        "innounp" => &["innounp-unicode", "innounp"],
        "dark" => &["dark", "wixtoolset"],
        _ => &[],
    }
}

/// What an app needs installed: for each need, the apps that meet it (one
/// for each of `depends`, the alternatives for a helper).
pub fn needs(scoop: &Scoop, manifest: &Manifest, arch: Arch) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> =
        helpers_needed(scoop, manifest, arch).iter().map(|h| helper_apps(h).iter().map(|a| a.to_string()).collect()).collect();
    for d in manifest.depends() {
        let app = parse_app(&d).app;
        if !out.iter().any(|alts| alts.iter().any(|a| a.eq_ignore_ascii_case(&app))) {
            out.push(vec![app]);
        }
    }
    out
}

/// The helpers `manifest` needs, installed or not.
fn helpers_needed(scoop: &Scoop, manifest: &Manifest, arch: Arch) -> Vec<&'static str> {
    let mut script = strings(manifest.arch("pre_install", arch));
    script.extend(strings(manifest.arch("installer", arch).get("script")));
    script.extend(strings(manifest.arch("post_install", arch)));
    let calls = |name: &str| script.iter().any(|l| l.to_ascii_lowercase().contains(&name.to_ascii_lowercase()));
    let urls = manifest.urls(arch);
    let mut helpers = Vec::new();
    if (urls.iter().any(|u| needs_7zip(u)) || calls("Expand-7zipArchive ")) && !scoop.enabled("use_external_7zip") {
        helpers.push("7zip");
    }
    if (urls.iter().any(|u| u.to_ascii_lowercase().ends_with(".msi")) || calls("Expand-MsiArchive ")) && scoop.enabled("use_lessmsi") {
        helpers.push("lessmsi");
    }
    if truthy(manifest.get("innosetup")) || calls("Expand-InnoArchive ") {
        helpers.push("innounp");
    }
    if calls("Expand-DarkArchive ") {
        helpers.push("dark");
    }
    helpers
}

/// `Get-InstallationHelper`: the helpers still missing to install `manifest`.
pub fn helpers_for(scoop: &Scoop, manifest: &Manifest, arch: Arch) -> Vec<String> {
    helpers_needed(scoop, manifest, arch).into_iter().filter(|h| !helper_installed(scoop, h)).map(String::from).collect()
}

/// `Get-Dependency`: what `query` needs, in the order of installation, with
/// the app itself at the end. Errors name what could not be resolved.
pub fn dependencies(scoop: &Scoop, query: &str, arch: Arch) -> Result<Vec<String>, DependencyError> {
    let mut resolved = Vec::new();
    visit(scoop, query, arch, &mut resolved, &mut Vec::new())?;
    Ok(resolved)
}

#[derive(Debug, PartialEq, Eq)]
pub enum DependencyError {
    NoManifest(String),
    Circular(String, String),
}

fn visit(scoop: &Scoop, query: &str, arch: Arch, resolved: &mut Vec<String>, open: &mut Vec<String>) -> Result<(), DependencyError> {
    let r = resolve(scoop, query);
    let Some(manifest) = &r.manifest else { return Err(DependencyError::NoManifest(query.to_string())) };
    open.push(r.app.clone());
    let mut deps = helpers_for(scoop, manifest, arch);
    for d in manifest.depends() {
        if !deps.contains(&d) {
            deps.push(d);
        }
    }
    for dep in deps {
        let name = parse_app(&dep).app;
        if resolved.iter().any(|x| parse_app(x).app.eq_ignore_ascii_case(&name)) {
            continue;
        }
        if open.iter().any(|x| x.eq_ignore_ascii_case(&name)) {
            return Err(DependencyError::Circular(r.app.clone(), dep));
        }
        visit(scoop, &dep, arch, resolved, open)?;
    }
    open.retain(|x| x != &r.app);
    let entry = match (&r.bucket, &r.url) {
        (Some(bucket), _) => format!("{bucket}/{}", r.app),
        (None, Some(url)) => url.clone(),
        _ => r.app.clone(),
    };
    if !resolved.contains(&entry) {
        resolved.push(entry);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Tree;

    fn parsed(bucket: Option<&str>, app: &str, version: Option<&str>) -> AppRef {
        AppRef { bucket: bucket.map(String::from), app: app.into(), version: version.map(String::from) }
    }

    #[test]
    fn what_the_user_types() {
        assert_eq!(parse_app("git"), parsed(None, "git", None));
        assert_eq!(parse_app("main/git@2.1"), parsed(Some("main"), "git", Some("2.1")));
        assert_eq!(parse_app(r"C:\x\y.json"), parsed(None, r"C:\x\y.json", None));
        for url in ["x.zip", "x.7z", "x.tar.gz", "x.tgz", "x.exe#/dl.7z", "x.7z.exe", "x.rar"] {
            assert_eq!(needs_7zip(url), url != "x.zip", "{url}");
        }
    }

    #[test]
    fn buckets_in_order_and_dependencies_first() {
        let t = Tree::new("resolve");
        t.manifest("zz", "tool", r#"{"version":"9","url":"u.exe"}"#);
        t.manifest("main", "tool", r#"{"version":"1","url":"u.7z","depends":"zz/lib"}"#);
        t.manifest("zz", "lib", r#"{"version":"1","url":"u.exe"}"#);
        t.manifest("main", "7zip", r#"{"version":"23","url":"u.msi"}"#);
        t.manifest("main", "loop", r#"{"version":"1","url":"u","depends":"loop2"}"#);
        t.manifest("main", "loop2", r#"{"version":"1","url":"u","depends":"loop"}"#);
        let s = t.scoop();
        let r = resolve(&s, "tool");
        assert_eq!((r.bucket.as_deref(), r.also_in.as_slice()), (Some("main"), ["zz".to_string()].as_slice()));
        assert_eq!(resolve(&s, "zz/tool").manifest.unwrap().version(), "9");
        assert!(resolve(&s, "nothing").manifest.is_none());
        assert_eq!(dependencies(&s, "tool", Arch::X64).unwrap(), ["main/7zip", "zz/lib", "main/tool"]);
        assert_eq!(dependencies(&s, "loop", Arch::X64), Err(DependencyError::Circular("loop2".into(), "loop".into())));
        assert_eq!(dependencies(&s, "nothing", Arch::X64), Err(DependencyError::NoManifest("nothing".into())));
    }
}
