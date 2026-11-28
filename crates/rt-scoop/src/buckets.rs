//! `lib\buckets.ps1`: the local buckets and their manifests.

use crate::install::Scoop;
use crate::json::{self, Json};
use std::path::{Path, PathBuf};

/// `buckets.json` of Scoop v0.6.0, used when the installation has none.
const KNOWN: [(&str, &str); 10] = [
    ("main", "https://github.com/ScoopInstaller/Main"),
    ("extras", "https://github.com/ScoopInstaller/Extras"),
    ("versions", "https://github.com/ScoopInstaller/Versions"),
    ("nirsoft", "https://github.com/ScoopInstaller/Nirsoft"),
    ("sysinternals", "https://github.com/niheaven/scoop-sysinternals"),
    ("php", "https://github.com/ScoopInstaller/PHP"),
    ("nerd-fonts", "https://github.com/matthewjberger/scoop-nerd-fonts"),
    ("nonportable", "https://github.com/ScoopInstaller/Nonportable"),
    ("java", "https://github.com/ScoopInstaller/Java"),
    ("games", "https://github.com/Calinou/scoop-games"),
];

/// The known buckets (name, repository), in Scoop's order.
pub fn known(scoop: &Scoop) -> Vec<(String, String)> {
    let file = scoop.app("scoop", false).join("current").join("buckets.json");
    match json::read(&file) {
        Some(Json::Obj(members)) if !members.is_empty() => members.into_iter().map(|(k, v)| (k, v.str().to_string())).collect(),
        _ => KNOWN.iter().map(|(n, u)| (n.to_string(), u.to_string())).collect(),
    }
}

/// The names of the folders in `dir`, ordered as Windows lists them.
pub(crate) fn folders(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort_by_key(|n| n.to_ascii_lowercase());
    names
}

/// `Get-LocalBucket`: the known ones first, in their order, then the others.
pub fn local(scoop: &Scoop) -> Vec<String> {
    let mut names = folders(&scoop.buckets());
    let known = known(scoop);
    names.sort_by_key(|n| known.iter().position(|(k, _)| k.eq_ignore_ascii_case(n)).unwrap_or(usize::MAX));
    names
}

/// `Find-BucketDirectory -Root`: the repository.
pub fn repository(scoop: &Scoop, name: &str) -> PathBuf {
    scoop.buckets().join(if name.is_empty() { "main" } else { name })
}

/// `Find-BucketDirectory`: where the manifests are.
pub fn manifest_dir(scoop: &Scoop, name: &str) -> PathBuf {
    let repository = repository(scoop, name);
    let inner = repository.join("bucket");
    if inner.is_dir() {
        inner
    } else {
        repository
    }
}

/// A manifest file: app, path and, when asked for, the file's metadata
/// (free on Windows, where the directory listing carries it).
type Found = (String, PathBuf, Option<std::fs::Metadata>);

fn walk(dir: &Path, out: &mut Vec<Found>, depth: u8, metadata: bool) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if depth < 8 && !name.starts_with('.') {
                walk(&path, out, depth + 1, metadata);
            }
        } else if let Some(app) = name.len().checked_sub(5).and_then(|n| name.get(..n)) {
            if name[app.len()..].eq_ignore_ascii_case(".json") {
                let meta = if metadata { entry.metadata().ok() } else { None };
                out.push((app.to_string(), path, meta));
            }
        }
    }
}

/// [`manifests_under`] with the metadata of each file (size and time, for an index).
pub fn manifests_with_metadata(dir: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    walk(dir, &mut out, 0, true);
    out.sort_by_cached_key(|(app, _, _)| app.to_ascii_lowercase());
    out
}

/// Every manifest under `dir`, as (app, file), ordered by app.
pub fn manifests_under(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    walk(dir, &mut out, 0, false);
    out.sort_by_cached_key(|(app, _, _)| app.to_ascii_lowercase());
    out.into_iter().map(|(app, path, _)| (app, path)).collect()
}

/// `manifest_path`: the manifest of `app` in `bucket`.
pub fn manifest_path(scoop: &Scoop, app: &str, bucket: &str) -> Option<PathBuf> {
    if app.is_empty() || app.contains(['/', '\\', '?', ':', '*', '<', '>', '|']) {
        return None;
    }
    let dir = manifest_dir(scoop, bucket);
    let direct = dir.join(format!("{app}.json"));
    if direct.is_file() {
        return Some(direct);
    }
    manifests_under(&dir).into_iter().find(|(name, _)| name.eq_ignore_ascii_case(app)).map(|(_, p)| p)
}

/// The buckets that have a manifest of `app`, in order (`Find-AppBucket`).
pub fn containing(scoop: &Scoop, app: &str) -> Vec<(String, PathBuf)> {
    local(scoop).into_iter().filter_map(|b| manifest_path(scoop, app, &b).map(|p| (b, p))).collect()
}

/// The manifest of `app` in the `deprecated` folder of `bucket`.
pub fn deprecated(scoop: &Scoop, app: &str, bucket: &str) -> Option<PathBuf> {
    let dir = repository(scoop, bucket).join("deprecated");
    if !dir.is_dir() {
        return None;
    }
    manifests_under(&dir).into_iter().find(|(name, _)| name.eq_ignore_ascii_case(app)).map(|(_, p)| p)
}

/// `remote.origin.url` of a repository, from its `config` file.
pub fn origin(repository: &Path) -> Option<String> {
    let text = json::read_text(&repository.join(".git").join("config"))?;
    let mut in_origin = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line == "[remote \"origin\"]";
        } else if in_origin {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "url" {
                    return Some(value.trim().to_string());
                }
            }
        }
    }
    None
}
