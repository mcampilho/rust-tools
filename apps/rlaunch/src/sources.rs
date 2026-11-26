//! Where the menu's programs come from: folders of shortcuts and programs
//! (the Start menu, Scoop's part of it, the user's own) and, on Linux, the
//! `.desktop` files.

use crate::category::{self, Category};
use crate::settings::Settings;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// A program of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// The shortcut, program or `.desktop` file that is opened.
    pub path: PathBuf,
    pub category: Category,
    /// Linux: the command of a `.desktop` file.
    pub exec: Vec<String>,
}

/// A folder to look through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub dir: PathBuf,
    /// Folders inside it that are not looked into.
    pub skip: Vec<PathBuf>,
}

/// How deep a folder is followed.
const DEPTH: usize = 3;
/// A folder given by mistake (a whole drive) stops here.
const MOST: usize = 5000;

/// Words that mark what comes with a program rather than being one.
const JUNK: &[&str] = &[
    "uninstall",
    "unins000",
    "desinstalar",
    "readme",
    "read me",
    "release notes",
    "license",
    "licence",
    "documentation",
    "manual",
    "help",
    "website",
    "web site",
    "homepage",
    "changelog",
    "what's new",
    "faq",
];

fn is_junk(name: &str) -> bool {
    let n = name.to_lowercase();
    // An uninstaller called just that; "Remove Empty Folders" is a program.
    if n == "remove" {
        return true;
    }
    JUNK.iter().any(|j| n == *j || n.starts_with(&format!("{j} ")) || n.ends_with(&format!(" {j}")) || n.contains(&format!(" {j} ")))
        || n.starts_with("uninstall")
        || n.starts_with("unins0")
}

/// The kinds of files that are programs or lead to one.
fn is_launchable(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "lnk" | "exe" | "bat" | "cmd" | "appref-ms")
}

/// Whether a shortcut's target is something to run: shortcuts to documents,
/// web pages and help files are left out.
fn target_runs(target: &Path) -> bool {
    let ext = target.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    !matches!(ext.as_str(), "txt" | "chm" | "htm" | "html" | "pdf" | "url" | "md" | "rtf" | "doc" | "docx" | "hlp")
}

fn entry_of(path: &Path) -> Option<Entry> {
    let name = path.file_stem()?.to_string_lossy().into_owned();
    if !is_launchable(path) || is_junk(&name) {
        return None;
    }
    let mut stem = String::new();
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
        let target = std::fs::read(path).ok().and_then(|b| rt_sys::lnk::target_of(&b, path.parent().unwrap_or(path)));
        if let Some(t) = target {
            if !target_runs(&t) {
                return None;
            }
            // The target may be written with backslashes, wherever this runs.
            let text = t.to_string_lossy();
            let file = text.rsplit(['\\', '/']).next().unwrap_or("");
            if is_junk(file.split('.').next().unwrap_or("")) {
                return None;
            }
            stem = file.rsplit_once('.').map_or(file, |x| x.0).to_string();
        }
    }
    let category = category::guess(&[&name, &stem]);
    Some(Entry { name, path: path.to_path_buf(), category, exec: Vec::new() })
}

fn walk(dir: &Path, skip: &[PathBuf], depth: usize, found: &mut Vec<Entry>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<_> = read.flatten().collect();
    items.sort_by_key(|e| e.file_name());
    for item in items {
        if found.len() >= MOST {
            return;
        }
        let path = item.path();
        // `file_type` does not follow links, so a junction is never entered.
        let Ok(kind) = item.file_type() else { continue };
        if kind.is_dir() {
            let hidden = item.file_name().to_string_lossy().starts_with(['.', '$']);
            if depth < DEPTH && !hidden && !skip.contains(&path) {
                walk(&path, skip, depth + 1, found);
            }
        } else if let Some(e) = entry_of(&path) {
            found.push(e);
        }
    }
}

/// The folders the settings ask for, Scoop's first: when two have the same
/// program, the first one found stays.
#[cfg(windows)]
pub fn roots(st: &Settings) -> Vec<Root> {
    let menus: Vec<PathBuf> = ["APPDATA", "ProgramData"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|d| PathBuf::from(d).join("Microsoft\\Windows\\Start Menu\\Programs"))
        .collect();
    roots_in(st, &menus)
}

/// Elsewhere there is no Start menu: only the user's folders.
#[cfg(not(windows))]
pub fn roots(st: &Settings) -> Vec<Root> {
    roots_in(st, &[])
}

pub fn roots_in(st: &Settings, menus: &[PathBuf]) -> Vec<Root> {
    let mut out = Vec::new();
    let scoop: Vec<PathBuf> = menus.iter().map(|m| m.join("Scoop Apps")).collect();
    if st.scoop {
        out.extend(scoop.iter().map(|d| Root { dir: d.clone(), skip: Vec::new() }));
    }
    out.extend(st.folders.iter().map(|d| Root { dir: d.clone(), skip: Vec::new() }));
    if st.start_menu {
        out.extend(menus.iter().map(|d| Root { dir: d.clone(), skip: scoop.clone() }));
    }
    out
}

/// Every program of the folders (and of the `.desktop` files given), one of
/// each name, sorted by name.
pub fn collect(roots: &[Root], desktop_dirs: &[PathBuf]) -> Vec<Entry> {
    let mut found = Vec::new();
    for r in roots {
        walk(&r.dir, &r.skip, 0, &mut found);
    }
    for dir in desktop_dirs {
        desktop::read_dir(dir, &mut found);
    }
    let mut seen = HashSet::new();
    found.retain(|e| seen.insert(e.name.to_lowercase()));
    found.sort_by_cached_key(|e| e.name.to_lowercase());
    found
}

/// The programs the settings ask for, on this system.
pub fn scan(st: &Settings) -> Vec<Entry> {
    let desktop_dirs = if cfg!(target_os = "linux") && st.start_menu { desktop::dirs() } else { Vec::new() };
    collect(&roots(st), &desktop_dirs)
}

/// The `.desktop` files of Linux. Plain text, so it is tested anywhere.
pub mod desktop {
    use super::{Category, Entry};
    use std::path::{Path, PathBuf};

    pub fn dirs() -> Vec<PathBuf> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let user = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.map(|h| h.join(".local/share")));
        let shared = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        user.into_iter()
            .chain(shared.split(':').filter(|s| !s.is_empty()).map(PathBuf::from))
            .chain([PathBuf::from("/var/lib/flatpak/exports/share"), PathBuf::from("/var/lib/snapd/desktop")])
            .map(|d| d.join("applications"))
            .collect()
    }

    fn category_of(list: &str) -> Option<Category> {
        list.split(';').find_map(|c| match c {
            "Network" => Some(Category::Internet),
            "AudioVideo" | "Audio" | "Video" => Some(Category::Multimedia),
            "Graphics" => Some(Category::Graphics),
            "Office" => Some(Category::Office),
            "Development" => Some(Category::Development),
            "System" | "Settings" => Some(Category::System),
            "Security" => Some(Category::Security),
            "Game" => Some(Category::Games),
            "Utility" => Some(Category::Utilities),
            _ => None,
        })
    }

    /// The words of an `Exec` line, without its `%f`-like field codes.
    pub fn command(exec: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut word = String::new();
        let (mut quoted, mut any) = (false, false);
        let mut it = exec.chars().peekable();
        while let Some(c) = it.next() {
            match c {
                '"' => {
                    quoted = !quoted;
                    any = true;
                }
                '\\' if quoted => word.extend(it.next()),
                '%' => {
                    if it.next() == Some('%') {
                        word.push('%');
                    }
                }
                c if c.is_whitespace() && !quoted => {
                    if any || !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                    any = false;
                }
                c => word.push(c),
            }
        }
        if any || !word.is_empty() {
            words.push(word);
        }
        words
    }

    /// The program of a `.desktop` file's text, if it belongs in a menu.
    pub fn parse(text: &str, path: &Path) -> Option<Entry> {
        let mut name = None;
        let (mut exec, mut categories, mut kind) = ("", "", "");
        let mut inside = false;
        for line in text.lines() {
            if line.starts_with('[') {
                inside = line.trim() == "[Desktop Entry]";
                continue;
            }
            let Some((k, v)) = line.split_once('=').filter(|_| inside) else { continue };
            match k.trim() {
                "Name" => name = Some(v.trim().to_string()),
                "Exec" => exec = v.trim(),
                "Categories" => categories = v.trim(),
                "Type" => kind = v.trim(),
                "NoDisplay" | "Hidden" | "Terminal" if v.trim() == "true" => return None,
                _ => {}
            }
        }
        let name = name?;
        let exec = command(exec);
        if kind != "Application" || exec.is_empty() {
            return None;
        }
        let category = category_of(categories).unwrap_or_else(|| super::category::guess(&[&name]));
        Some(Entry { name, path: path.to_path_buf(), category, exec })
    }

    pub fn read_dir(dir: &Path, found: &mut Vec<Entry>) {
        let Ok(read) = std::fs::read_dir(dir) else { return };
        for item in read.flatten() {
            let path = item.path();
            if path.extension().is_none_or(|e| e != "desktop") {
                continue;
            }
            if let Some(e) = std::fs::read_to_string(&path).ok().and_then(|t| parse(&t, &path)) {
                found.push(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("rlaunch-tests").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(dir: &Path, file: &str, bytes: &[u8]) {
        let p = dir.join(file);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    #[test]
    fn folders_give_programs_without_the_rest() {
        let d = scratch("folders");
        let menu = d.join("menu");
        let link = |target: &str| rt_sys::lnk::make(target, None);
        put(&menu, "Scoop Apps/Firefox.lnk", &link("C:\\scoop\\firefox.exe"));
        put(&menu, "Firefox.lnk", &link("C:\\Program Files\\firefox.exe"));
        put(&menu, "Tools/Zed.exe", b"x");
        put(&menu, "Tools/Uninstall Zed.lnk", &link("C:\\z\\unins000.exe"));
        put(&menu, "Tools/Zed Manual.lnk", &link("C:\\z\\manual.pdf"));
        put(&menu, "Tools/Zed Web Page.lnk", &link("C:\\z\\page.html"));
        put(&menu, "Tools/notes.txt", b"x");
        let own = d.join("own");
        put(&own, "Calc/calc.bat", b"x");
        let mut st = Settings { folders: vec![own], ..Settings::default() };
        let all = collect(&roots_in(&st, std::slice::from_ref(&menu)), &[]);
        let names: Vec<&str> = all.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["calc", "Firefox", "Zed"]);
        // Scoop's shortcut wins over the other one.
        assert_eq!(all[1].path, menu.join("Scoop Apps").join("Firefox.lnk"));
        // Scoop turned off: its folder is not entered through the Start menu.
        st.scoop = false;
        let all = collect(&roots_in(&st, std::slice::from_ref(&menu)), &[]);
        assert_eq!(all[1].path, menu.join("Firefox.lnk"));
        st.start_menu = false;
        assert_eq!(collect(&roots_in(&st, &[menu]), &[]).len(), 1);
    }

    #[test]
    fn desktop_files_are_read() {
        let text = "[Desktop Entry]\nType=Application\nName=GIMP\nName[pt]=GIMP\nExec=\"/opt/my gimp/gimp\" --new %U\nCategories=GTK;Graphics;\n[Desktop Action x]\nName=Other\nExec=other\n";
        let e = desktop::parse(text, Path::new("/x/gimp.desktop")).unwrap();
        assert_eq!((e.name.as_str(), e.category), ("GIMP", Category::Graphics));
        assert_eq!(e.exec, ["/opt/my gimp/gimp", "--new"]);
        assert!(desktop::parse("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n", Path::new("x")).is_none());
        assert!(desktop::parse("[Desktop Entry]\nType=Link\nName=X\nExec=x\n", Path::new("x")).is_none());
    }
}
