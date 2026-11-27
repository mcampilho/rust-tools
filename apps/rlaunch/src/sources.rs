//! Where the menu's programs come from: folders of shortcuts and programs
//! (the Start menu, Scoop's part of it, the user's own) and, on Linux, the
//! `.desktop` files.

use crate::category::{self, Category, Group};
use crate::portableapps;
use crate::settings::{FolderMode, Settings};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A program of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// The shortcut, program or `.desktop` file that is opened.
    pub path: PathBuf,
    pub category: Category,
    /// The subgroup of the category, if any.
    pub sub: Option<&'static str>,
    /// The category comes from the folder the program is in.
    pub placed: bool,
    /// Linux: the command of a `.desktop` file.
    pub exec: Vec<String>,
    /// What the user chose (see [`crate::items`]).
    pub icon: Option<PathBuf>,
    pub favourite: bool,
    pub hidden: bool,
    pub admin: bool,
    /// Added one by one, not found in a folder.
    pub added: bool,
}

impl Entry {
    pub fn new(name: String, path: PathBuf, category: Category) -> Entry {
        Entry {
            name,
            path,
            category,
            sub: None,
            placed: false,
            exec: Vec::new(),
            icon: None,
            favourite: false,
            hidden: false,
            admin: false,
            added: false,
        }
    }

    /// Where it is in the menu.
    pub fn group(&self) -> Group {
        Group { category: self.category, sub: self.sub }
    }

    pub fn set_group(&mut self, group: Group) {
        (self.category, self.sub) = (group.category, group.sub);
    }

    /// The file whose icon stands for the program.
    pub fn icon_source(&self) -> &Path {
        self.icon.as_deref().unwrap_or(&self.path)
    }
}

/// The entry of a file the user added: any file, named after it; a `.desktop`
/// file gives its own name and command.
pub fn added_entry(path: &Path) -> Option<Entry> {
    let own = path
        .extension()
        .is_some_and(|e| e == "desktop")
        .then(|| std::fs::read_to_string(path).ok().and_then(|t| desktop::parse(&t, path)))
        .flatten();
    let entry = match own.or_else(|| unix_program(path)) {
        Some(e) => e,
        None => {
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let category = category::guess(&[&name]);
            Entry::new(name, path.to_path_buf(), category)
        }
    };
    Some(Entry { added: true, ..entry })
}

/// A folder to look through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub dir: PathBuf,
    /// Folders inside it that are not looked into.
    pub skip: Vec<PathBuf>,
    /// Whether its folders say the categories.
    pub mode: FolderMode,
}

impl Root {
    fn plain(dir: PathBuf, skip: Vec<PathBuf>) -> Root {
        Root { dir, skip, mode: FolderMode::Names }
    }
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
    Some(Entry::new(name, path.to_path_buf(), category))
}

/// A program of Linux in a folder: executable, and a binary (an AppImage too)
/// or a script. On drives where every file is "executable" (FAT, NTFS), the
/// first bytes still tell.
#[cfg(unix)]
fn unix_program(path: &Path) -> Option<Entry> {
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if ext == "desktop" {
        return std::fs::read_to_string(path).ok().and_then(|t| desktop::parse(&t, path));
    }
    if !matches!(ext.as_str(), "" | "sh" | "appimage" | "run" | "bin" | "x86_64" | "py") {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
        return None;
    }
    let mut magic = [0u8; 4];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)).ok()?;
    if !(magic == *b"\x7fELF" || magic.starts_with(b"#!")) {
        return None;
    }
    // `Tool-1.2.AppImage` and `run.sh` are called without the extension.
    let name = path.file_stem()?.to_string_lossy().into_owned();
    if is_junk(&name) {
        return None;
    }
    let category = category::guess(&[&name]);
    let exec = vec![path.to_string_lossy().into_owned()];
    Some(Entry { exec, ..Entry::new(name, path.to_path_buf(), category) })
}

#[cfg(not(unix))]
fn unix_program(_: &Path) -> Option<Entry> {
    None
}

fn walk(dir: &Path, skip: &[PathBuf], depth: usize, found: &mut Vec<Entry>) {
    // An app of the PortableApps format says what starts it: nothing else of
    // its folder is a program of the menu.
    if let Some(apps) = portableapps::app(dir) {
        found.extend(apps);
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<_> = read.flatten().collect();
    items.sort_by_key(|e| e.file_name());
    // Suites of tools ship `x.exe` beside `x64.exe` and `x64a.exe` (as
    // Sysinternals does): only the first is listed.
    let files: HashSet<String> = items.iter().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
    let variant = |name: &str| {
        let n = name.to_lowercase();
        ["64.exe", "64a.exe"].iter().any(|end| n.strip_suffix(end).is_some_and(|base| files.contains(&format!("{base}.exe"))))
    };
    // Among the apps of a suite: its platform and shared files are not apps,
    // and a folder in another format gives only what is at its top.
    let suite = dir.join(portableapps::PLATFORM).is_dir();
    for item in items {
        if found.len() >= MOST {
            return;
        }
        let path = item.path();
        // `file_type` does not follow links, so a junction is never entered.
        let Ok(kind) = item.file_type() else { continue };
        if kind.is_dir() {
            let name = item.file_name().to_string_lossy().into_owned();
            let hidden = name.starts_with(['.', '$']);
            let shared = suite && [portableapps::PLATFORM, "CommonFiles"].iter().any(|s| name.eq_ignore_ascii_case(s));
            if depth < DEPTH && !hidden && !shared && !skip.contains(&path) {
                walk(&path, skip, if suite { DEPTH } else { depth + 1 }, found);
            }
        } else if variant(&item.file_name().to_string_lossy()) {
            continue;
        } else if let Some(e) = entry_of(&path).or_else(|| unix_program(&path)) {
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
        out.extend(scoop.iter().map(|d| Root::plain(d.clone(), Vec::new())));
    }
    out.extend(st.folders.iter().map(|d| Root { dir: d.clone(), skip: Vec::new(), mode: st.mode_of(d) }));
    if st.start_menu {
        out.extend(menus.iter().map(|d| Root::plain(d.clone(), scoop.clone())));
    }
    out
}

/// The names of the folders between `root` and the program.
fn folders_below(root: &Path, program: &Path) -> Vec<String> {
    let inside = program.strip_prefix(root).ok().and_then(Path::parent);
    inside.map_or_else(Vec::new, |p| p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect())
}

/// Gives the programs of a folder the categories its mode says.
fn place(root: &Root, found: &mut [Entry]) {
    let mode = match root.mode {
        // Shortcuts arranged in folders say the categories; portable programs
        // each in its folder, and suites, do not.
        FolderMode::Auto => {
            let arranged = found.iter().any(|e| {
                e.path.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) && !folders_below(&root.dir, &e.path).is_empty()
            });
            if arranged {
                FolderMode::Subfolders
            } else {
                FolderMode::Names
            }
        }
        mode => mode,
    };
    // A Scoop package is in `apps\<name>\current`: it is called `<name>`.
    let named = if root.dir.file_name().is_some_and(|n| n.eq_ignore_ascii_case("current")) { root.dir.parent() } else { Some(&*root.dir) };
    let own = named.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned());
    for e in found {
        let mut folders = folders_below(&root.dir, &e.path);
        match mode {
            FolderMode::Subfolders => {}
            FolderMode::One => {
                if let Some(own) = &own {
                    folders.insert(0, own.clone());
                }
            }
            FolderMode::Auto | FolderMode::Names => continue,
        }
        let mut folders = folders.into_iter();
        let Some(top) = folders.next() else { continue };
        e.set_group(Group { category: Category::named(&top), sub: folders.next().map(|s| category::intern(&s)) });
        e.placed = true;
    }
}

/// Every program of the folders (and of the `.desktop` files given), one of
/// each name, sorted by name.
pub fn collect(roots: &[Root], desktop_dirs: &[PathBuf]) -> Vec<Entry> {
    let mut found = Vec::new();
    for r in roots {
        let from = found.len();
        walk(&r.dir, &r.skip, 0, &mut found);
        place(r, &mut found[from..]);
    }
    for dir in desktop_dirs {
        desktop::read_dir(dir, &mut found);
    }
    // The first of a name stays, but takes the category a folder gave another:
    // a Scoop program arranged in the user's folders goes where it was put.
    let mut kept: Vec<Entry> = Vec::new();
    let mut at = HashMap::new();
    for e in found {
        match at.get(&e.name.to_lowercase()) {
            None => {
                at.insert(e.name.to_lowercase(), kept.len());
                kept.push(e);
            }
            Some(&i) => {
                let first: &mut Entry = &mut kept[i];
                if e.placed && !first.placed {
                    first.set_group(e.group());
                    first.placed = true;
                }
            }
        }
    }
    kept.sort_by_cached_key(|e| e.name.to_lowercase());
    kept
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

    /// The program of a `.desktop` file's text, if it belongs in a menu, with
    /// the menu's language and this desktop.
    pub fn parse(text: &str, path: &Path) -> Option<Entry> {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        parse_for(text, path, crate::i18n::lang().tag(), &desktop)
    }

    /// [`parse`] for the language `lang` (`pt-PT`) on `desktop`.
    pub fn parse_for(text: &str, path: &Path, lang: &str, desktop: &str) -> Option<Entry> {
        let mut name = None;
        // `Name[pt_PT]` is better than `Name[pt]`.
        let full = format!("Name[{}]", lang.replace('-', "_"));
        let short = format!("Name[{}]", lang.split('-').next().unwrap_or(lang));
        let (mut by_full, mut by_short) = (None, None);
        let (mut only, mut not) = ("", "");
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
                k if k == full => by_full = Some(v.trim().to_string()),
                k if k == short => by_short = Some(v.trim().to_string()),
                "OnlyShowIn" => only = v.trim(),
                "NotShowIn" => not = v.trim(),
                "Exec" => exec = v.trim(),
                "Categories" => categories = v.trim(),
                "Type" => kind = v.trim(),
                "NoDisplay" | "Hidden" | "Terminal" if v.trim() == "true" => return None,
                _ => {}
            }
        }
        let name = by_full.or(by_short).or(name)?;
        let exec = command(exec);
        if kind != "Application" || exec.is_empty() || !crate::xdg::shown_on(only, not, desktop) {
            return None;
        }
        let category = category_of(categories).unwrap_or_else(|| super::category::guess(&[&name]));
        Some(Entry { exec, ..Entry::new(name, path.to_path_buf(), category) })
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
    fn folders_say_the_categories() {
        let d = scratch("groups");
        let link = rt_sys::lnk::make("C:\\x\\x.exe", None);
        put(&d, "menu/Scoop Apps/Writer.lnk", &link);
        put(&d, "links/Loose.lnk", &link);
        put(&d, "links/Office/LibreOffice/Writer.lnk", &link);
        put(&d, "links/Editors/deeper/more/Pad.lnk", &link);
        put(&d, "links/multimedia/Player.lnk", &link);
        for f in ["procexp.exe", "procexp64.exe", "procexp64a.exe", "base64.exe"] {
            put(&d, &format!("SysInternals/{f}"), b"x");
        }
        let links = d.join("links");
        let tools = d.join("SysInternals");
        let mut st = Settings { folders: vec![links.clone(), tools.clone()], start_menu: false, ..Settings::default() };
        st.modes.insert(tools.clone(), FolderMode::One);
        let places = |st: &Settings| -> Vec<(String, String)> {
            collect(&roots_in(st, &[d.join("menu")]), &[]).iter().map(|e| (e.name.clone(), e.group().key())).collect()
        };
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            places(&st),
            [
                pair("base64", "SysInternals"),
                pair("Loose", "Other"),
                pair("Pad", "Editors/deeper"),
                pair("Player", "Multimedia"),
                pair("procexp", "SysInternals"),
                // Scoop's shortcut, in the category of the user's folder.
                pair("Writer", "Office/LibreOffice"),
            ]
        );
        let all = collect(&roots_in(&st, &[d.join("menu")]), &[]);
        assert_eq!(all[5].path, d.join("menu").join("Scoop Apps").join("Writer.lnk"));
        // Turned off: names alone.
        st.modes.insert(links, FolderMode::Names);
        st.modes.insert(tools, FolderMode::Names);
        assert!(places(&st).iter().all(|(_, g)| !g.contains('/') && g != "SysInternals"));
    }

    #[test]
    fn a_suite_gives_its_apps_and_not_their_insides() {
        let d = crate::portableapps::tests::suite("pa-walk");
        let all = collect(&[Root::plain(d, Vec::new())], &[]);
        let names: Vec<&str> = all.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Git Bash", "Git GUI", "Mozilla Firefox, Edition", "Tool"]);
    }

    #[test]
    fn desktop_names_follow_the_language_and_the_desktop() {
        let text = "[Desktop Entry]\nType=Application\nName=Files\nName[pt]=Ficheiros\nName[pt_BR]=Arquivos\nExec=x\nNotShowIn=KDE;\n";
        let name = |lang, on| desktop::parse_for(text, Path::new("x"), lang, on).map(|e| e.name);
        assert_eq!(name("pt-PT", "GNOME").as_deref(), Some("Ficheiros"));
        assert_eq!(name("pt-BR", "GNOME").as_deref(), Some("Arquivos"));
        assert_eq!(name("de-DE", "").as_deref(), Some("Files"));
        assert_eq!(name("pt-PT", "KDE"), None);
    }

    #[cfg(unix)]
    #[test]
    fn folders_give_linux_programs() {
        use std::os::unix::fs::PermissionsExt;
        let d = scratch("unix");
        for (file, bytes, mode) in [
            ("Tool.AppImage", &b"\x7fELF...."[..], 0o755),
            ("run.sh", b"#!/bin/sh\n", 0o755),
            ("plain.sh", b"#!/bin/sh\n", 0o644),
            ("photo", b"JFIF....", 0o755),
            ("lib.so", b"\x7fELF....", 0o755),
        ] {
            put(&d, file, bytes);
            std::fs::set_permissions(d.join(file), std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let all = collect(&[Root::plain(d.clone(), Vec::new())], &[]);
        let names: Vec<&str> = all.iter().map(|e| e.name.as_str()).collect();
        // `target` may be on a drive without modes: then all are executable.
        let modes = std::fs::metadata(d.join("plain.sh")).unwrap().permissions().mode() & 0o111 == 0;
        assert_eq!(names, if modes { vec!["run", "Tool"] } else { vec!["plain", "run", "Tool"] });
        assert!(all.iter().all(|e| e.exec == [e.path.to_string_lossy()]));
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
