//! Broken shortcuts (phase 3): shortcuts that open something gone. On
//! Windows, `.lnk` files (read with `rt_sys::lnk`) on the Start menu, the
//! desktop, the taskbar, Send To and the recent items; on Linux, `.desktop`
//! entries whose program is gone, and links to nothing in `~/.local/bin`.
//!
//! A target on the network, or on a drive that is not there now (a USB disk
//! unplugged), is not called gone: nobody can tell. Broken shortcuts go to
//! the Recycle Bin (the trash), so they can be put back.

use crate::clean::Found;
use crate::targets::{at, Env, Filter, Group, Kind, Place, Target};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Marks a translation key (the test of the translations looks for it).
const fn k(s: &'static str) -> &'static str {
    s
}

fn target(id: &str, name: &'static str, about: &'static str, places: Vec<Place>, admin: bool) -> Target {
    Target { kind: Kind::Shortcuts, admin, ..Target::files(id, Group::Shortcuts, name, about, places) }
}

fn deep(dir: PathBuf) -> Place {
    Place { dir, filter: Filter::All }
}

/// Only the folder itself (`Filter::Prefix("")` does not go down).
fn flat(dir: PathBuf) -> Place {
    Place { dir, filter: Filter::Prefix("") }
}

/// Where shortcuts are, on Windows.
pub fn windows(e: &Env) -> Vec<Target> {
    let menu = "Microsoft/Windows/Start Menu/Programs";
    let public_desktop = e.home.parent().map_or_else(|| PathBuf::from("C:\\Users\\Public\\Desktop"), |u| u.join("Public").join("Desktop"));
    vec![
        target("lnk.menu", k("Start menu"), k("Shortcuts on your Start menu that open something gone."), vec![deep(at(&e.roaming, menu))], false),
        target("lnk.menu-all", k("Start menu of every user"), k("The same, on the Start menu shared by every user."), vec![deep(at(&e.program_data, menu))], true),
        target("lnk.desktop", k("Desktop"), k("Shortcuts on your desktop that open something gone."), vec![flat(e.desktop.clone())], false),
        target("lnk.desktop-all", k("Desktop of every user"), k("The same, on the desktop shared by every user."), vec![flat(public_desktop)], true),
        target(
            "lnk.taskbar",
            k("Taskbar and Quick Launch"),
            k("Pinned and Quick Launch shortcuts that open something gone."),
            vec![deep(at(&e.roaming, "Microsoft/Internet Explorer/Quick Launch"))],
            false,
        ),
        target("lnk.sendto", k("Send To"), k("Items of the Send To menu that open something gone."), vec![flat(at(&e.roaming, "Microsoft/Windows/SendTo"))], false),
        target(
            "lnk.recent",
            k("Recent items"),
            k("The list of files opened lately, where the file is gone."),
            vec![flat(at(&e.roaming, "Microsoft/Windows/Recent"))],
            false,
        ),
    ]
}

/// Where shortcuts are, on Linux.
pub fn linux(e: &Env) -> Vec<Target> {
    vec![
        target("desktop.menu", k("Menu entries"), k("Your menu entries (.desktop files) whose program is gone."), vec![deep(e.data.join("applications"))], false),
        target("desktop.desktop", k("Desktop"), k("Shortcuts on your desktop that open something gone."), vec![flat(e.desktop.clone())], false),
        target("desktop.autostart", k("Autostart"), k("Programs started when you log in that are gone."), vec![flat(e.config.join("autostart"))], false),
        target("desktop.bin", k("Program links"), k("Links in ~/.local/bin to programs that are gone."), vec![flat(at(&e.home, ".local/bin"))], false),
    ]
}

/// Whether a target can be told to be gone: not on the network, and its
/// drive (Windows) is there.
fn checkable(t: &Path) -> bool {
    let s = t.to_string_lossy();
    if s.starts_with("\\\\") || s.starts_with("//") || !t.is_absolute() {
        return false;
    }
    match t.components().next() {
        Some(std::path::Component::Prefix(p)) => Path::new(&format!("{}\\", p.as_os_str().to_string_lossy())).exists(),
        _ => true,
    }
}

/// What a `.lnk` file opens, when it is gone.
pub fn broken_lnk(path: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(path).ok()?;
    let t = rt_sys::lnk::target_of(&bytes, path.parent()?)?;
    (checkable(&t) && !t.exists()).then_some(t)
}

/// A program on the `PATH`.
fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// The program a `.desktop` entry starts, when it is gone: `TryExec`, else
/// the first word of `Exec` (past `env` and its `NAME=value`s), absolute or
/// on the `PATH`. Links (`Type=Link`) and entries without a program are
/// never gone.
pub fn broken_desktop(text: &str, on_path: &dyn Fn(&str) -> bool) -> Option<String> {
    let field = |name: &str| text.lines().find_map(|l| l.strip_prefix(name).and_then(|r| r.trim_start().strip_prefix('='))).map(str::trim);
    if field("Type").is_some_and(|t| t != "Application") {
        return None;
    }
    let program = match field("TryExec").filter(|t| !t.is_empty()) {
        Some(t) => t.trim_matches('"').to_string(),
        None => {
            let exec = field("Exec")?;
            let mut words = exec.split_whitespace().map(|w| w.trim_matches('"'));
            let mut first = words.next()?;
            if first == "env" {
                first = words.find(|w| !w.contains('='))?;
            }
            first.to_string()
        }
    };
    let gone = if program.starts_with('/') { !Path::new(&program).exists() } else { !on_path(&program) };
    gone.then_some(program)
}

/// Looks for broken shortcuts in `t`'s places: their files and sizes, and
/// what each opened.
pub fn analyse(t: &Target, stop: &AtomicBool) -> Found {
    let mut f = Found::default();
    for place in &t.places {
        let mut stack = vec![place.dir.clone()];
        while let Some(dir) = stack.pop() {
            if stop.load(Ordering::Relaxed) {
                return f;
            }
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let path = e.path();
                let Ok(ft) = e.file_type() else { continue };
                let ext = path.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
                let gone = if ft.is_symlink() {
                    // A link to nothing (Linux: ~/.local/bin, the desktop).
                    (!path.exists()).then(|| std::fs::read_link(&path).map(|t| t.to_string_lossy().into_owned()).unwrap_or_default())
                } else if ft.is_dir() {
                    if place.deep() {
                        stack.push(path.clone());
                    }
                    None
                } else if ext == "lnk" {
                    broken_lnk(&path).map(|t| t.to_string_lossy().into_owned())
                } else if ext == "desktop" {
                    std::fs::read_to_string(&path).ok().and_then(|text| broken_desktop(&text, &on_path))
                } else {
                    None
                };
                if let Some(target) = gone {
                    let size = std::fs::symlink_metadata(&path).map_or(0, |m| m.len());
                    f.bytes += size;
                    f.files.push((path.clone(), size));
                    f.links.push((path, target));
                }
            }
        }
    }
    f
}

/// Moves the broken shortcuts found to the Recycle Bin (the trash).
pub fn clean(found: &Found, stop: &AtomicBool) -> crate::clean::Cleaned {
    let mut c = crate::clean::Cleaned::default();
    for (path, size) in &found.files {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match rt_sys::recycle(path) {
            Ok(()) => {
                c.bytes += size;
                c.files += 1;
            }
            Err(_) if std::fs::symlink_metadata(path).is_err() => {}
            Err(_) => c.failed += 1,
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entries() {
        let there = |n: &str| n == "firefox";
        assert_eq!(broken_desktop("[Desktop Entry]\nType=Application\nExec=/opt/gone/app %U\n", &there), Some("/opt/gone/app".into()));
        assert_eq!(broken_desktop("[Desktop Entry]\nExec=firefox %u\n", &there), None);
        assert_eq!(broken_desktop("[Desktop Entry]\nExec=env GDK_SCALE=2 oldtool --x\n", &there), Some("oldtool".into()));
        assert_eq!(broken_desktop("[Desktop Entry]\nTryExec=gonething\nExec=firefox\n", &there), Some("gonething".into()));
        assert_eq!(broken_desktop("[Desktop Entry]\nType=Link\nURL=https://example.org\n", &there), None);
        assert_eq!(broken_desktop("[Desktop Entry]\nName=No program\n", &there), None);
    }

    #[test]
    fn broken_shortcuts_found() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("lnk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let menu = root.join("menu").join("Some App");
        std::fs::create_dir_all(&menu).unwrap();
        let exists = root.join("there.exe");
        std::fs::write(&exists, b"x").unwrap();
        let gone = root.join("gone.exe");
        std::fs::write(menu.join("Good.lnk"), rt_sys::lnk::make(&exists.to_string_lossy(), None)).unwrap();
        std::fs::write(menu.join("Bad.lnk"), rt_sys::lnk::make(&gone.to_string_lossy(), None)).unwrap();
        std::fs::write(menu.join("Net.lnk"), rt_sys::lnk::make("\\\\server\\share\\x.exe", None)).unwrap();
        std::fs::write(menu.join("readme.txt"), b"not a shortcut").unwrap();
        let t = target("t", "", "", vec![deep(root.join("menu"))], false);
        let f = analyse(&t, &AtomicBool::new(false));
        assert_eq!(f.links, [(menu.join("Bad.lnk"), gone.to_string_lossy().into_owned())], "only the one to a file gone; not the network one");
        let flat_t = target("t", "", "", vec![flat(root.join("menu"))], false);
        assert_eq!(analyse(&flat_t, &AtomicBool::new(false)).count(), 0, "a flat place does not go down");
        let _ = std::fs::remove_dir_all(&root);
    }
}
