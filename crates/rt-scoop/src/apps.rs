//! `lib\versions.ps1`, `lib\core.ps1`: what is installed and in which state.

use crate::buckets;
use crate::install::Scoop;
use crate::json::{self, Json};
use crate::manifest::Manifest;
use crate::resolve;
use crate::version;
use std::cmp::Ordering;
use std::path::PathBuf;
use std::time::SystemTime;

const INFO_FILES: [&str; 2] = ["scoop-install.json", "install.json"];
const MANIFEST_FILES: [&str; 2] = ["scoop-manifest.json", "manifest.json"];

/// `scoop-install.json`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstallInfo {
    pub architecture: String,
    pub bucket: Option<String>,
    pub url: Option<String>,
    pub hold: bool,
}

/// `installed_apps`: the folders of `apps`, without Scoop itself.
pub fn installed_names(scoop: &Scoop, global: bool) -> Vec<String> {
    buckets::folders(&scoop.apps(global)).into_iter().filter(|n| !n.eq_ignore_ascii_case("scoop")).collect()
}

fn first_file(dir: PathBuf, names: [&str; 2]) -> Option<PathBuf> {
    names.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

/// The file with the installation data of a version.
pub fn info_file(scoop: &Scoop, app: &str, version: &str, global: bool) -> Option<PathBuf> {
    first_file(scoop.app(app, global).join(version), INFO_FILES)
}

/// `Get-InstalledVersion`: the version folders with installation data, oldest
/// installation first.
pub fn installed_versions(scoop: &Scoop, app: &str, global: bool) -> Vec<String> {
    let mut found: Vec<(SystemTime, String)> = buckets::folders(&scoop.app(app, global))
        .into_iter()
        .filter(|v| v != "current" && !(v.starts_with('_') && v.contains(".old")))
        .filter_map(|v| {
            let time = std::fs::metadata(info_file(scoop, app, &v, global)?).ok()?.modified().ok()?;
            Some((time, v))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, v)| v).collect()
}

/// `Select-CurrentVersion`.
pub fn current_version(scoop: &Scoop, app: &str, global: bool) -> Option<String> {
    let current = scoop.app(app, global).join("current");
    let mut version = None;
    if !scoop.enabled("no_junction") {
        version = first_file(current.clone(), MANIFEST_FILES)
            .and_then(|p| Manifest::read(&p))
            .map(|m| m.version().to_string())
            .filter(|v| !v.is_empty());
        if version.as_deref() == Some("nightly") {
            version = std::fs::read_link(&current).ok().and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned()));
        }
    }
    version.or_else(|| installed_versions(scoop, app, global).pop())
}

/// The folder Scoop calls `$dir`: `current`, or the version with `no_junction`.
pub fn current_dir(scoop: &Scoop, app: &str, global: bool) -> PathBuf {
    let version = if scoop.enabled("no_junction") && app != "scoop" { current_version(scoop, app, global) } else { None };
    scoop.app(app, global).join(version.as_deref().unwrap_or("current"))
}

pub fn install_info(scoop: &Scoop, app: &str, version: &str, global: bool) -> Option<InstallInfo> {
    let j = json::read(&info_file(scoop, app, version, global)?)?;
    let text = |key: &str| Some(j.get(key).str().to_string()).filter(|s| !s.is_empty());
    Some(InstallInfo {
        architecture: j.get("architecture").str().to_string(),
        bucket: text("bucket"),
        url: text("url"),
        hold: j.get("hold") == &Json::Bool(true),
    })
}

pub fn installed_manifest(scoop: &Scoop, app: &str, version: &str, global: bool) -> Option<Manifest> {
    Manifest::read(&first_file(scoop.app(app, global).join(version), MANIFEST_FILES)?)
}

/// `installed`: in one of the two scopes, or in the one given.
pub fn installed(scoop: &Scoop, app: &str, global: Option<bool>) -> bool {
    let name = app.rsplit(['/', '\\']).next().unwrap_or(app);
    match global {
        Some(g) => !name.is_empty() && current_version(scoop, name, g).is_some(),
        None => installed(scoop, app, Some(false)) || installed(scoop, app, Some(true)),
    }
}

/// `failed`: there is a folder, but no usable installation in it.
pub fn failed(scoop: &Scoop, app: &str, global: bool) -> bool {
    let dir = scoop.app(app, global);
    let has_current = scoop.enabled("no_junction") || dir.join("current").exists();
    dir.exists() && !(has_current && installed(scoop, app, Some(global)))
}

/// `app_status`.
#[derive(Debug, Default)]
pub struct Status {
    pub installed: bool,
    pub version: String,
    pub latest: String,
    pub outdated: bool,
    pub failed: bool,
    pub hold: bool,
    pub deprecated: bool,
    /// The manifest is gone from where the app came from.
    pub removed: bool,
    pub missing_deps: Vec<String>,
}

/// The state of an installed app. A manifest installed from a web address is
/// not fetched here: such an app is shown as up to date.
pub fn status(scoop: &Scoop, app: &str, global: bool, today: &str) -> Status {
    let version = current_version(scoop, app, global);
    let mut s = Status {
        installed: version.is_some(),
        version: version.clone().unwrap_or_default(),
        latest: version.clone().unwrap_or_default(),
        failed: failed(scoop, app, global),
        ..Status::default()
    };
    let info = version.as_deref().and_then(|v| install_info(scoop, app, v, global)).unwrap_or_default();
    s.hold = info.hold;
    s.deprecated = buckets::deprecated(scoop, app, info.bucket.as_deref().unwrap_or("")).is_some();

    let remote = info.url.as_deref().is_some_and(resolve::is_remote);
    let manifest = match (&info.bucket, &info.url) {
        (Some(bucket), _) => buckets::manifest_path(scoop, app, bucket).and_then(|p| Manifest::read(&p)),
        (None, Some(url)) if !remote => Manifest::read(url.as_ref()),
        _ => None,
    };
    s.removed = manifest.is_none() && !remote;
    if let Some(m) = &manifest {
        if !m.version().is_empty() {
            s.latest = m.version().to_string();
        }
        s.missing_deps = m.depends().into_iter().filter(|d| !installed(scoop, &resolve::parse_app(d).app, None)).collect();
    }
    if !s.version.is_empty() && !s.latest.is_empty() {
        let nightly = scoop.enabled("update_nightly").then_some(today);
        let order = version::compare(&s.version, &s.latest, nightly);
        s.outdated = if scoop.enabled("force_update") { order != Ordering::Equal } else { order == Ordering::Greater };
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Tree;

    #[test]
    fn installed_outdated_held_failed_and_removed() {
        let t = Tree::new("status");
        t.manifest("main", "tool", r#"{"version":"1.2","depends":"extras/helper"}"#);
        t.app("tool", "1.1", r#"{"version":"1.1"}"#, r#"{"architecture":"64bit","bucket":"main","hold":true}"#);
        t.app("gone", "2.0", r#"{"version":"2.0"}"#, r#"{"architecture":"64bit","bucket":"main"}"#);
        t.dir("root/apps/broken/1.0");
        t.dir("root/apps/scoop/current");
        let s = t.scoop();

        assert_eq!(installed_names(&s, false), ["broken", "gone", "tool"]);
        assert_eq!(current_version(&s, "tool", false).as_deref(), Some("1.1"));
        let st = status(&s, "tool", false, "20260101");
        assert!(st.installed && st.outdated && st.hold && !st.failed && !st.removed);
        assert_eq!((st.version.as_str(), st.latest.as_str()), ("1.1", "1.2"));
        assert_eq!(st.missing_deps, ["extras/helper"]);
        assert!(status(&s, "gone", false, "").removed);
        assert!(status(&s, "broken", false, "").failed);
    }
}
