//! The links of an installed app (`lib\install.ps1`: `link_current`,
//! `unlink_current`, `persist_data`, `unlink_persist_data`,
//! `persist_permission`; `lib\psmodules.ps1`): the `current` junction, the
//! persisted data linked into the version folder, and the module folder.

use crate::disk::{self, remove_tree};
use crate::env::{self, Change};
use crate::install::Scoop;
use crate::json::Json;
use crate::manifest::Manifest;
use crate::system::System;
use std::path::{Path, PathBuf};

fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |e| format!("{}: {e}", path.display())
}

/// `link_current`: `current` points to `version_dir` (read-only, as Scoop
/// leaves it). Returns the folder the app is used from: `current`, or the
/// version folder with `no_junction`.
pub fn link_current(scoop: &Scoop, version_dir: &Path) -> Result<PathBuf, String> {
    if scoop.enabled("no_junction") {
        return Ok(version_dir.to_path_buf());
    }
    let current = version_dir.parent().unwrap_or(version_dir).join("current");
    if version_dir.file_name().is_some_and(|n| n.eq_ignore_ascii_case("current")) {
        return Err("Error: Version 'current' is not allowed!".into());
    }
    if exists(&current) {
        remove_tree(&current).map_err(|e| e.to_string())?;
    }
    disk::make_junction(&current, version_dir).map_err(io(&current))?;
    disk::set_link_readonly(&current, true);
    Ok(current)
}

/// `unlink_current`: removes the `current` junction. Returns the folder the
/// app was used from, as [`link_current`].
pub fn unlink_current(scoop: &Scoop, version_dir: &Path) -> Result<PathBuf, String> {
    if scoop.enabled("no_junction") {
        return Ok(version_dir.to_path_buf());
    }
    let current = version_dir.parent().unwrap_or(version_dir).join("current");
    if !exists(&current) {
        return Ok(version_dir.to_path_buf());
    }
    remove_tree(&current).map_err(|e| e.to_string())?;
    Ok(current)
}

/// `persist_def`: (the path in the app, the path in `persist`).
pub fn persist_entries(manifest: &Manifest) -> Vec<(String, String)> {
    let one = |v: &Json| -> Option<(String, String)> {
        let (source, target) = match v {
            Json::Arr(items) => (items.first().map(|s| s.str().to_string()).unwrap_or_default(), items.get(1).map(|s| s.str().to_string())),
            other => (other.str().to_string(), None),
        };
        let target = target.filter(|t| !t.is_empty()).unwrap_or_else(|| source.clone());
        (!source.is_empty()).then_some((source, target))
    };
    match manifest.get("persist") {
        Json::Arr(items) => items.iter().filter_map(one).collect(),
        Json::Null => Vec::new(),
        other => one(other).into_iter().collect(),
    }
}

/// `persist_data`: the data of `dir` (the folder the app is used from) moves
/// to `persist_dir` the first time, and is linked back: a junction for a
/// folder, a hard link for a file. What the app brings when there is data
/// already is kept as `<name>.original`. Returns the names persisted.
pub fn persist_data(manifest: &Manifest, dir: &Path, persist_dir: &Path) -> Result<Vec<String>, String> {
    let entries = persist_entries(manifest);
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    std::fs::create_dir_all(persist_dir).map_err(io(persist_dir))?;
    let mut done = Vec::new();
    for (name, target) in entries {
        done.push(name.clone());
        let source = dir.join(name.trim_end_matches('/').trim_end_matches('\\'));
        let target = persist_dir.join(&target);
        if target.exists() {
            if exists(&source) {
                let mut original = source.as_os_str().to_os_string();
                original.push(".original");
                let original = PathBuf::from(original);
                if original.is_file() {
                    let _ = std::fs::remove_file(&original);
                }
                std::fs::rename(&source, &original).map_err(io(&source))?;
            }
        } else if exists(&source) {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(io(parent))?;
            }
            std::fs::rename(&source, &target).map_err(io(&source))?;
        } else {
            std::fs::create_dir_all(&target).map_err(io(&target))?;
        }
        if target.is_dir() {
            disk::make_junction(&source, &target).map_err(io(&source))?;
            disk::set_link_readonly(&source, true);
        } else {
            std::fs::hard_link(&target, &source).map_err(io(&source))?;
        }
    }
    Ok(done)
}

/// `unlink_persist_data`: the junctions and hard links of `persist` in `dir`
/// go; what they point to stays.
pub fn unlink_persist_data(manifest: &Manifest, dir: &Path) {
    for (name, _) in persist_entries(manifest) {
        // `data/` names the link itself, not what it points to.
        let source = dir.join(name.trim_end_matches(['/', '\\']));
        let Ok(meta) = std::fs::symlink_metadata(&source) else { continue };
        if disk::is_link(&meta) || (meta.is_file() && disk::link_count(&source) > 1) {
            let _ = remove_tree(&source);
        }
    }
}

/// `persist_permission`: the users may write in the global `persist` folder.
pub fn persist_permission(scoop: &Scoop, manifest: &Manifest, global: bool, admin: bool) {
    if !(global && admin && !persist_entries(manifest).is_empty()) {
        return;
    }
    let mut path = scoop.base(true).join("persist").display().to_string();
    path.push('\\');
    // BUILTIN\Users: write, inherited by the files (`FileSystemAccessRule`
    // with ObjectInherit, set in place of the rules the group had).
    let _ = std::process::Command::new("icacls")
        .arg(path)
        .args(["/grant:r", "*S-1-5-32-545:(OI)(W)", "/Q"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// `modulesdir`.
pub fn modules_dir(scoop: &Scoop, global: bool) -> PathBuf {
    scoop.base(global).join("modules")
}

/// The name of the PowerShell module of an app (`psmodule.name`).
pub fn module_name(manifest: &Manifest) -> Option<String> {
    Some(manifest.get("psmodule").get("name").str().to_string()).filter(|n| !n.is_empty())
}

/// `uninstall_psmodule`: the module's link goes. Returns its path when it
/// was there.
pub fn uninstall_psmodule(scoop: &Scoop, manifest: &Manifest, global: bool) -> Option<PathBuf> {
    let link = modules_dir(scoop, global).join(module_name(manifest)?);
    if !link.exists() {
        return None;
    }
    let _ = remove_tree(&link);
    Some(link)
}

/// `install_psmodule`: the app's folder linked as a module, and the modules
/// folder in `PSModulePath`. Returns (the link, whether one was replaced,
/// the change of `PSModulePath`).
pub fn install_psmodule(
    scoop: &Scoop,
    system: &dyn System,
    manifest: &Manifest,
    dir: &Path,
    global: bool,
) -> Result<Option<(PathBuf, bool, Vec<Change>)>, String> {
    if manifest.get("psmodule") == &Json::Null {
        return Ok(None);
    }
    let name = module_name(manifest).ok_or("Invalid manifest: The 'name' property is missing from 'psmodule'.")?;
    let modules = modules_dir(scoop, global);
    std::fs::create_dir_all(&modules).map_err(io(&modules))?;
    let modules = env::absolute(&modules);
    let mut changes = Vec::new();
    let mut current = system.env_get("PSModulePath", global).map(|(v, _)| v);
    if !global && current.is_none() {
        current = Some(format!("{}\\Documents\\WindowsPowerShell\\Modules", std::env::var("USERPROFILE").unwrap_or_default()));
    }
    let current = current.unwrap_or_default();
    let dir_text = modules.display().to_string();
    if !current.to_lowercase().contains(&dir_text.to_lowercase()) {
        env::set_var(system, "PSModulePath", Some(&format!("{dir_text};{current}")), global)?;
        changes.push(Change::Added(dir_text));
    }
    let link = modules.join(&name);
    let replaced = exists(&link);
    if replaced {
        let _ = remove_tree(&link);
    }
    disk::make_junction(&link, dir).map_err(io(&link))?;
    Ok(Some((link, replaced, changes)))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::testing::Tree;

    fn manifest(text: &str) -> Manifest {
        Manifest(crate::json::parse(text).unwrap())
    }

    #[test]
    fn current_is_a_read_only_junction() {
        let t = Tree::new("links-current");
        let scoop = t.scoop();
        let v1 = t.dir("root/apps/x/1.0");
        let v2 = t.dir("root/apps/x/2.0");
        t.file("root/apps/x/2.0/two.txt", "2");
        let current = link_current(&scoop, &v1).unwrap();
        assert_eq!(current, t.dir("root/apps/x").join("current"));
        assert_eq!(link_current(&scoop, &v2).unwrap(), current);
        assert_eq!(std::fs::read_to_string(current.join("two.txt")).unwrap(), "2");
        use std::os::windows::fs::MetadataExt;
        assert!(std::fs::symlink_metadata(&current).unwrap().file_attributes() & 1 != 0);
        assert!(link_current(&scoop, &current).is_err());
        assert_eq!(unlink_current(&scoop, &v2).unwrap(), current);
        assert!(!exists(&current) && v2.join("two.txt").is_file());
        assert_eq!(unlink_current(&scoop, &v2).unwrap(), v2);
    }

    #[test]
    fn persisted_data_moves_once_and_is_linked() {
        let t = Tree::new("links-persist");
        let m = manifest(r#"{"persist":["data/",["conf.ini","settings.ini"],"empty"]}"#);
        let persist = t.dir("root/persist/x");
        let v1 = t.dir("root/apps/x/1.0");
        t.file("root/apps/x/1.0/data/a.txt", "a");
        t.file("root/apps/x/1.0/conf.ini", "one");
        assert_eq!(persist_data(&m, &v1, &persist).unwrap(), ["data/", "conf.ini", "empty"]);
        assert_eq!(std::fs::read_to_string(persist.join("data/a.txt")).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(persist.join("settings.ini")).unwrap(), "one");
        assert!(persist.join("empty").is_dir() && v1.join("empty").is_dir());
        assert_eq!(disk::link_count(&v1.join("conf.ini")), 2);

        // The next version brings its own: kept as .original.
        let v2 = t.dir("root/apps/x/2.0");
        t.file("root/apps/x/2.0/conf.ini", "two");
        persist_data(&m, &v2, &persist).unwrap();
        assert_eq!(std::fs::read_to_string(v2.join("conf.ini")).unwrap(), "one");
        assert_eq!(std::fs::read_to_string(v2.join("conf.ini.original")).unwrap(), "two");

        unlink_persist_data(&m, &v1);
        assert!(!exists(&v1.join("data")) && !exists(&v1.join("conf.ini")));
        assert_eq!(std::fs::read_to_string(persist.join("settings.ini")).unwrap(), "one");
        assert!(persist.join("data/a.txt").is_file());
    }

    #[test]
    fn a_module_is_linked_and_its_folder_put_in_the_path() {
        let t = Tree::new("links-module");
        let scoop = t.scoop();
        let s = crate::system::FakeSystem::default();
        s.env_put("PSModulePath", false, Some((r"C:\m", false))).unwrap();
        let m = manifest(r#"{"psmodule":{"name":"Mod"}}"#);
        let dir = t.dir("root/apps/mod/current");
        t.file("root/apps/mod/current/Mod.psm1", "");
        let (link, replaced, changes) = install_psmodule(&scoop, &s, &m, &dir, false).unwrap().unwrap();
        let modules = t.dir("root/modules");
        assert_eq!((link.clone(), replaced), (modules.join("Mod"), false));
        assert_eq!(changes, [Change::Added(modules.display().to_string())]);
        assert_eq!(s.var("PSModulePath", false).unwrap(), format!(r"{};C:\m", modules.display()));
        assert!(link.join("Mod.psm1").is_file());
        assert_eq!(uninstall_psmodule(&scoop, &m, false), Some(link.clone()));
        assert!(!exists(&link) && dir.join("Mod.psm1").is_file());
    }
}
