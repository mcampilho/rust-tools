//! `lib\shortcuts.ps1`: the shortcuts of an app in `Start Menu\Programs\Scoop
//! Apps`, in the subfolders their names give.

use crate::manifest::{Arch, Manifest};
use crate::system::System;
use std::path::{Path, PathBuf};

/// `shortcut_folder` (made if it is not there, as Scoop does).
pub fn folder(system: &dyn System, global: bool) -> Option<PathBuf> {
    let folder = system.start_menu(global)?.join("Programs").join("Scoop Apps");
    let _ = std::fs::create_dir_all(&folder);
    Some(folder)
}

/// One entry of `shortcuts`: target, name, arguments, icon.
pub struct Entry {
    pub target: String,
    pub name: String,
    pub arguments: String,
    pub icon: Option<String>,
}

pub fn entries(manifest: &Manifest, arch: Arch) -> Vec<Entry> {
    manifest
        .arch("shortcuts", arch)
        .items()
        .iter()
        .map(|s| Entry {
            target: s.at(0).str().to_string(),
            name: s.at(1).str().to_string(),
            arguments: s.at(2).str().to_string(),
            icon: (s.items().len() >= 4).then(|| s.at(3).str().to_string()),
        })
        .collect()
}

/// `substitute` of `$dir`, `$original_dir` and `$persist_dir`, the longest
/// name first.
pub fn substitute(text: &str, dir: &Path, original_dir: &Path, persist_dir: &Path) -> String {
    text.replace("$original_dir", &original_dir.display().to_string())
        .replace("$persist_dir", &persist_dir.display().to_string())
        .replace("$dir", &dir.display().to_string())
}

/// What became of one shortcut.
#[derive(Debug, PartialEq, Eq)]
pub enum Made {
    /// (name, file name of the target)
    Created(String, String),
    /// The target is not there: (name, file name, target).
    NoTarget(String, String, PathBuf),
    /// The icon is not there: (name, file name, icon).
    NoIcon(String, String, PathBuf),
    Failed(String, String),
}

/// `[System.IO.Path]::Combine`: a rooted second part wins.
fn combine(dir: &Path, part: &str) -> PathBuf {
    let p = Path::new(part);
    if p.has_root() {
        p.to_path_buf()
    } else {
        dir.join(part)
    }
}

/// `create_startmenu_shortcuts`.
pub fn create(system: &dyn System, manifest: &Manifest, dirs: [&Path; 3], global: bool, arch: Arch) -> Vec<Made> {
    let [dir, original_dir, persist_dir] = dirs;
    let mut made = Vec::new();
    for entry in entries(manifest, arch) {
        let target = crate::env::absolute(&combine(dir, &entry.target));
        let file = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let name = entry.name.clone();
        if !target.is_file() {
            made.push(Made::NoTarget(name, file, target));
            continue;
        }
        let icon = entry.icon.as_deref().map(|i| crate::env::absolute(&combine(dir, i)));
        if let Some(icon) = icon.as_ref().filter(|i| !i.is_file()) {
            made.push(Made::NoIcon(name, file, icon.clone()));
            continue;
        }
        let Some(folder) = folder(system, global) else {
            made.push(Made::Failed(name, "Start menu".into()));
            continue;
        };
        let link = folder.join(format!("{name}.lnk"));
        if let Some(parent) = link.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let arguments = substitute(&entry.arguments, dir, original_dir, persist_dir);
        let working = target.parent().unwrap_or(&target);
        match system.shortcut(&link, &target, working, &arguments, icon.as_deref()) {
            Ok(()) => made.push(Made::Created(name, file)),
            Err(why) => made.push(Made::Failed(name, why)),
        }
    }
    made
}

/// `rm_startmenu_shortcuts`: (the shortcut, whether it was there).
pub fn remove(system: &dyn System, manifest: &Manifest, global: bool, arch: Arch) -> Vec<(PathBuf, bool)> {
    let Some(folder) = folder(system, global) else { return Vec::new() };
    entries(manifest, arch)
        .into_iter()
        .map(|e| {
            let link = crate::env::absolute(&folder.join(format!("{}.lnk", e.name)));
            let there = link.is_file() && std::fs::remove_file(&link).is_ok();
            (link, there)
        })
        .collect()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::system::FakeSystem;
    use crate::testing::Tree;

    #[test]
    fn shortcuts_in_their_folders() {
        let t = Tree::new("shortcuts");
        let s = FakeSystem::new(t.dir("menu"));
        let dir = t.dir("root/apps/x/current");
        t.file("root/apps/x/current/x.exe", "");
        let m = Manifest(
            crate::json::parse(
                r#"{"shortcuts":[["x.exe","Tools\\X","--data \"$persist_dir\\d\""],["none.exe","None"],["x.exe","Icon","","x.ico"]]}"#,
            )
            .unwrap(),
        );
        let persist = t.dir("root/persist/x");
        let made = create(&s, &m, [&dir, &dir, &persist], false, Arch::X64);
        assert_eq!(made[0], Made::Created("Tools\\X".into(), "x.exe".into()));
        assert!(matches!(&made[1], Made::NoTarget(n, f, _) if n == "None" && f == "none.exe"));
        assert!(matches!(&made[2], Made::NoIcon(..)));
        let link = t.dir("menu/user/Programs/Scoop Apps/Tools").join("X.lnk");
        let text = std::fs::read_to_string(&link).unwrap();
        assert_eq!(text, format!("{}\n{}\n--data \"{}\\d\"\n\n", dir.join("x.exe").display(), dir.display(), persist.display()));
        let gone = remove(&s, &m, false, Arch::X64);
        assert_eq!(gone.iter().map(|(_, there)| *there).collect::<Vec<_>>(), [true, false, false]);
        assert!(!link.exists());
    }
}
