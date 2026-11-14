//! What can be cleaned: the temporary files of the system and the caches of
//! the browsers, each with the folders it is in and what goes from them.
//!
//! Only files that are there to be thrown away are listed: temporary files
//! (those older than a day, so nothing in use goes), downloads already used,
//! error reports, caches that are made again when needed. The browsers'
//! history, cookies and passwords are not touched.

use std::path::{Path, PathBuf};

/// Where a target belongs in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    /// Windows' or Linux' own.
    System,
    /// A browser, by its name (a product name: not translated).
    Browser(&'static str),
    /// A package manager, by its name.
    Tool(&'static str),
    /// Broken shortcuts.
    Shortcuts,
}

/// Which files of a folder go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    /// Those not changed for this many hours (temporary files in use are
    /// newer).
    OlderThanHours(u64),
    /// Those whose name starts so, in the folder itself only.
    Prefix(&'static str),
    /// One file of this name, in the folder itself.
    Named(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub dir: PathBuf,
    pub filter: Filter,
}

impl Place {
    fn new(dir: PathBuf, filter: Filter) -> Place {
        Place { dir, filter }
    }

    /// Whether the folders inside are looked into.
    pub fn deep(&self) -> bool {
        matches!(self.filter, Filter::All | Filter::OlderThanHours(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Files,
    /// Windows' Recycle Bin, through the shell (it has a folder per drive).
    RecycleBin,
    /// Its own commands clean it (a package manager's); the places only say
    /// how much it holds.
    Command,
    /// Shortcuts that open something gone, to the Recycle Bin.
    Shortcuts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Stable, for the settings: `win.temp`, `chrome.cache`.
    pub id: String,
    pub group: Group,
    /// Translation keys.
    pub name: &'static str,
    pub about: &'static str,
    pub places: Vec<Place>,
    pub kind: Kind,
    /// Only with administrator rights (Windows' own folders).
    pub admin: bool,
    /// Ticked unless the user said otherwise.
    pub default_on: bool,
    /// The program that may hold its files open: its process names.
    pub process: &'static [&'static str],
    /// `Kind::Command`: what cleans it.
    pub commands: Vec<crate::managers::Cmd>,
    /// Where it is, when only its manager knows (asked while analysing).
    pub locate: Option<crate::managers::Locate>,
    /// Its size can be measured (Flatpak's unused runtimes cannot).
    pub measure: bool,
}

/// The folders the targets are found from (from the environment, or made up
/// in the tests).
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub home: PathBuf,
    /// Windows: `%LOCALAPPDATA%`, `%APPDATA%`, `%TEMP%`, `%SystemRoot%`,
    /// `%ProgramData%`.
    pub local: PathBuf,
    pub roaming: PathBuf,
    pub temp: PathBuf,
    pub windows: PathBuf,
    pub program_data: PathBuf,
    /// Linux: `$XDG_CACHE_HOME` (`~/.cache`), `$XDG_CONFIG_HOME` (`~/.config`),
    /// `$XDG_DATA_HOME` (`~/.local/share`).
    pub cache: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    /// The user's desktop folder (it may have moved, to OneDrive for one).
    pub desktop: PathBuf,
}

/// The desktop folder: Windows' `User Shell Folders`, or Linux'
/// `XDG_DESKTOP_DIR` in `user-dirs.dirs`; else `~/Desktop`.
fn desktop_of(home: &Path, config: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let _ = config;
        use rt_sys::win::reg::{Key, Root, View};
        const FOLDERS: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders";
        if let Some(d) = Key::open(Root::CurrentUser, FOLDERS, View::Bits64).and_then(|k| k.string("Desktop")) {
            return PathBuf::from(rt_sys::win::reg::expand(&d));
        }
    }
    #[cfg(not(windows))]
    if let Some(d) = std::fs::read_to_string(config.join("user-dirs.dirs")).ok().and_then(|t| {
        t.lines().find_map(|l| l.strip_prefix("XDG_DESKTOP_DIR=")).map(|v| v.trim().trim_matches('"').replace("$HOME", &home.to_string_lossy()))
    }) {
        return PathBuf::from(d);
    }
    home.join("Desktop")
}

impl Env {
    /// This computer's.
    pub fn here() -> Env {
        let var = |v: &str| std::env::var_os(v).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty());
        let home = var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap_or_default();
        let config = var("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        Env {
            desktop: desktop_of(&home, &config),
            local: var("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local")),
            roaming: var("APPDATA").unwrap_or_else(|| home.join("AppData").join("Roaming")),
            temp: if cfg!(windows) { var("TEMP").unwrap_or_else(std::env::temp_dir) } else { PathBuf::from("/tmp") },
            windows: var("SystemRoot").unwrap_or_else(|| PathBuf::from("C:\\Windows")),
            program_data: var("ProgramData").unwrap_or_else(|| PathBuf::from("C:\\ProgramData")),
            cache: var("XDG_CACHE_HOME").unwrap_or_else(|| home.join(".cache")),
            config,
            data: var("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share")),
            home,
        }
    }
}

/// A path from parts.
pub fn at(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, s| p.join(s))
}

/// What this system has.
pub fn all(env: &Env) -> Vec<Target> {
    let mut out = if cfg!(windows) { windows(env) } else { linux(env) };
    out.extend(browsers(env, cfg!(windows)));
    out.extend(crate::managers::targets(env));
    out.extend(if cfg!(windows) { crate::shortcuts::windows(env) } else { crate::shortcuts::linux(env) });
    out
}

impl Target {
    /// A target of files, the others as given.
    pub fn files(id: &str, group: Group, name: &'static str, about: &'static str, places: Vec<Place>) -> Target {
        Target { id: id.to_string(), group, name, about, places, kind: Kind::Files, admin: false, default_on: true, process: &[], commands: Vec::new(), locate: None, measure: true }
    }
}

fn system(id: &str, name: &'static str, about: &'static str, places: Vec<Place>, admin: bool, default_on: bool) -> Target {
    Target { admin, default_on, ..Target::files(id, Group::System, name, about, places) }
}

/// Windows' own.
pub fn windows(e: &Env) -> Vec<Target> {
    use Filter::*;
    let day = OlderThanHours(24);
    vec![
        system(
            "win.user-temp",
            "Temporary files",
            "Files programs left in your temporary folder. Only those not changed for a day go, so nothing in use is touched.",
            vec![Place::new(e.temp.clone(), day)],
            false,
            true,
        ),
        system(
            "win.temp",
            "Temporary files of Windows",
            "Files Windows and its services left in its own temporary folder, not changed for a day.",
            vec![Place::new(e.windows.join("Temp"), day)],
            true,
            true,
        ),
        system(
            "win.update",
            "Windows Update downloads",
            "Updates already downloaded and installed. Windows downloads again whatever it still needs.",
            vec![Place::new(at(&e.windows, "SoftwareDistribution/Download"), All)],
            true,
            true,
        ),
        system(
            "win.errors",
            "Error reports",
            "Reports and crash dumps of programs that stopped working, kept for Microsoft and the program makers.",
            vec![Place::new(at(&e.local, "Microsoft/Windows/WER"), All), Place::new(e.local.join("CrashDumps"), All)],
            false,
            true,
        ),
        system(
            "win.errors-system",
            "Error reports of the system",
            "The same, for Windows itself and the programs of every user.",
            vec![Place::new(at(&e.program_data, "Microsoft/Windows/WER/ReportArchive"), All), Place::new(at(&e.program_data, "Microsoft/Windows/WER/ReportQueue"), All)],
            true,
            true,
        ),
        system(
            "win.dumps",
            "Memory dumps",
            "What Windows wrote from memory when it stopped with an error (blue screens). Only useful to find out why.",
            vec![Place::new(e.windows.join("Minidump"), All), Place::new(e.windows.clone(), Named("MEMORY.DMP"))],
            true,
            true,
        ),
        system(
            "win.thumbs",
            "Thumbnail cache",
            "The pictures Explorer shows of files and folders. It makes them again as you browse, which takes a while at first.",
            vec![Place::new(at(&e.local, "Microsoft/Windows/Explorer"), Prefix("thumbcache_"))],
            false,
            false,
        ),
        system(
            "win.shaders",
            "Graphics shader caches",
            "What DirectX and the graphics drivers compiled for games and programs. Made again when needed, slower the first time.",
            vec![
                Place::new(e.local.join("D3DSCache"), All),
                Place::new(at(&e.local, "NVIDIA/DXCache"), All),
                Place::new(at(&e.local, "NVIDIA/GLCache"), All),
                Place::new(at(&e.local, "AMD/DxCache"), All),
                Place::new(at(&e.local, "AMD/DxcCache"), All),
            ],
            false,
            false,
        ),
        Target {
            kind: Kind::RecycleBin,
            ..system("win.recycle", "Recycle Bin", "The files deleted to the Recycle Bin, on every drive. They cannot be brought back after.", Vec::new(), false, false)
        },
    ]
}

/// Linux' own, in the user's folders and the shared temporary ones.
pub fn linux(e: &Env) -> Vec<Target> {
    use Filter::*;
    let day = OlderThanHours(24);
    vec![
        system(
            "linux.tmp",
            "Temporary files",
            "Your files in /tmp and /var/tmp not changed for a day, so nothing in use is touched.",
            vec![Place::new(PathBuf::from("/tmp"), day), Place::new(PathBuf::from("/var/tmp"), day)],
            false,
            true,
        ),
        system(
            "linux.thumbs",
            "Thumbnail cache",
            "The pictures file managers show of files and folders. They are made again as you browse.",
            vec![Place::new(e.cache.join("thumbnails"), All)],
            false,
            true,
        ),
        system(
            "linux.shaders",
            "Graphics shader caches",
            "What Mesa and the graphics drivers compiled for games and programs. Made again when needed, slower the first time.",
            vec![
                Place::new(e.cache.join("mesa_shader_cache"), All),
                Place::new(e.cache.join("mesa_shader_cache_db"), All),
                Place::new(at(&e.cache, "nvidia/GLCache"), All),
                Place::new(at(&e.home, ".nv/GLCache"), All),
            ],
            false,
            false,
        ),
        system(
            "linux.trash",
            "Trash",
            "The files deleted to the trash. They cannot be brought back after.",
            vec![Place::new(at(&e.data, "Trash/files"), All), Place::new(at(&e.data, "Trash/info"), All)],
            false,
            false,
        ),
    ]
}

/// A browser built on Chromium: where its profiles are, and its processes.
struct Chromium {
    name: &'static str,
    id: &'static str,
    /// Windows: under `%LOCALAPPDATA%` (Opera: `%APPDATA%`, its cache under
    /// `%LOCALAPPDATA%`).
    win: &'static str,
    /// Linux: under `~/.config` and `~/.cache`.
    linux: &'static str,
    win_process: &'static [&'static str],
    linux_process: &'static [&'static str],
}

const CHROMIUM: &[Chromium] = &[
    Chromium { name: "Google Chrome", id: "chrome", win: "Google/Chrome/User Data", linux: "google-chrome", win_process: &["chrome.exe"], linux_process: &["chrome"] },
    Chromium { name: "Microsoft Edge", id: "edge", win: "Microsoft/Edge/User Data", linux: "microsoft-edge", win_process: &["msedge.exe"], linux_process: &["msedge"] },
    Chromium {
        name: "Brave",
        id: "brave",
        win: "BraveSoftware/Brave-Browser/User Data",
        linux: "BraveSoftware/Brave-Browser",
        win_process: &["brave.exe"],
        linux_process: &["brave"],
    },
    Chromium { name: "Vivaldi", id: "vivaldi", win: "Vivaldi/User Data", linux: "vivaldi", win_process: &["vivaldi.exe"], linux_process: &["vivaldi-bin"] },
    Chromium { name: "Opera", id: "opera", win: "Opera Software/Opera Stable", linux: "opera", win_process: &["opera.exe"], linux_process: &["opera"] },
    Chromium { name: "Chromium", id: "chromium", win: "Chromium/User Data", linux: "chromium", win_process: &["chrome.exe"], linux_process: &["chromium"] },
];

/// The caches inside a Chromium profile, and in its `User Data` folder.
const CHROMIUM_PROFILE_CACHES: &[&str] = &["Cache", "Code Cache", "GPUCache", "DawnGraphiteCache", "DawnWebGPUCache", "Service Worker/CacheStorage", "Service Worker/ScriptCache"];
const CHROMIUM_ROOT_CACHES: &[&str] = &["ShaderCache", "GrShaderCache", "GraphiteDawnCache", "component_crx_cache"];

/// A browser built on Firefox: where its profiles' caches are.
struct Gecko {
    name: &'static str,
    id: &'static str,
    /// Windows: under `%LOCALAPPDATA%`.
    win: &'static str,
    /// Linux: under `~/.cache` (and Snap's and Flatpak's own for Firefox).
    linux: &'static [&'static str],
    win_process: &'static [&'static str],
    linux_process: &'static [&'static str],
}

const GECKO: &[Gecko] = &[
    Gecko {
        name: "Firefox",
        id: "firefox",
        win: "Mozilla/Firefox/Profiles",
        linux: &["~cache/mozilla/firefox", "~/snap/firefox/common/.cache/mozilla/firefox", "~/.var/app/org.mozilla.firefox/cache/mozilla/firefox"],
        win_process: &["firefox.exe"],
        linux_process: &["firefox", "firefox-bin"],
    },
    Gecko { name: "LibreWolf", id: "librewolf", win: "librewolf/Profiles", linux: &["~cache/librewolf"], win_process: &["librewolf.exe"], linux_process: &["librewolf"] },
    Gecko { name: "Waterfox", id: "waterfox", win: "Waterfox/Profiles", linux: &["~cache/waterfox"], win_process: &["waterfox.exe"], linux_process: &["waterfox"] },
    Gecko { name: "Floorp", id: "floorp", win: "Floorp/Profiles", linux: &["~cache/floorp"], win_process: &["floorp.exe"], linux_process: &["floorp"] },
];

const GECKO_CACHES: &[&str] = &["cache2", "startupCache", "jumpListCache"];

/// The folders right inside `dir`.
fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
    out.sort();
    out
}

/// The caches of one Chromium browser's `root` (its `User Data`, or Opera's
/// profile): of each profile in it, and of the root.
fn chromium_places(root: &Path, out: &mut Vec<Place>) {
    let mut profiles: Vec<PathBuf> = subdirs(root).into_iter().filter(|d| CHROMIUM_PROFILE_CACHES.iter().any(|c| at(d, c).is_dir())).collect();
    // Opera keeps its one profile in the root itself.
    if CHROMIUM_PROFILE_CACHES.iter().any(|c| at(root, c).is_dir()) {
        profiles.insert(0, root.to_path_buf());
    }
    for p in profiles {
        out.extend(CHROMIUM_PROFILE_CACHES.iter().map(|c| at(&p, c)).filter(|d| d.is_dir()).map(|d| Place::new(d, Filter::All)));
    }
    out.extend(CHROMIUM_ROOT_CACHES.iter().map(|c| root.join(c)).filter(|d| d.is_dir()).map(|d| Place::new(d, Filter::All)));
}

/// The browsers found, with their caches (none: not listed).
pub fn browsers(e: &Env, windows: bool) -> Vec<Target> {
    let mut out = Vec::new();
    let target = |id: &str, name: &'static str, places: Vec<Place>, process: &'static [&'static str]| Target {
        process,
        ..Target::files(
            &format!("{id}.cache"),
            Group::Browser(name),
            "Cache",
            "Pages, pictures and scripts kept to open sites faster. They are downloaded again when needed; history, cookies and passwords stay.",
            places,
        )
    };
    for b in CHROMIUM {
        let mut places = Vec::new();
        if windows {
            let base = if b.id == "opera" { &e.roaming } else { &e.local };
            chromium_places(&at(base, b.win), &mut places);
            // Opera's cache is in `%LOCALAPPDATA%`, apart from its profile.
            if b.id == "opera" {
                chromium_places(&at(&e.local, b.win), &mut places);
            }
        } else {
            chromium_places(&at(&e.config, b.linux), &mut places);
            chromium_places(&at(&e.cache, b.linux), &mut places);
        }
        places.dedup();
        if !places.is_empty() {
            out.push(target(b.id, b.name, places, if windows { b.win_process } else { b.linux_process }));
        }
    }
    for b in GECKO {
        let roots: Vec<PathBuf> = if windows {
            vec![at(&e.local, b.win)]
        } else {
            b.linux
                .iter()
                .map(|r| match (r.strip_prefix("~cache/"), r.strip_prefix("~/")) {
                    (Some(rel), _) => at(&e.cache, rel),
                    (_, Some(rel)) => at(&e.home, rel),
                    _ => PathBuf::from(r),
                })
                .collect()
        };
        let places: Vec<Place> =
            roots.iter().flat_map(|r| subdirs(r)).flat_map(|p| GECKO_CACHES.iter().map(move |c| p.join(c))).filter(|d| d.is_dir()).map(|d| Place::new(d, Filter::All)).collect();
        if !places.is_empty() {
            out.push(target(b.id, b.name, places, if windows { b.win_process } else { b.linux_process }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join(format!("targets-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn mk(base: &Path, rel: &str) {
        std::fs::create_dir_all(at(base, rel)).unwrap();
    }

    #[test]
    fn browsers_found_by_their_caches() {
        let root = dir("browsers");
        let e = Env { local: root.join("local"), roaming: root.join("roaming"), cache: root.join("cache"), config: root.join("config"), home: root.join("home"), ..Env::default() };
        mk(&e.local, "Google/Chrome/User Data/Default/Cache/Cache_Data");
        mk(&e.local, "Google/Chrome/User Data/Profile 1/Code Cache/js");
        mk(&e.local, "Google/Chrome/User Data/ShaderCache");
        mk(&e.local, "Google/Chrome/User Data/Crashpad");
        mk(&e.local, "Mozilla/Firefox/Profiles/abc.default-release/cache2/entries");
        mk(&e.local, "Microsoft/Edge/User Data/Default/Bookmarks-only");
        mk(&e.roaming, "Opera Software/Opera Stable/Code Cache");
        mk(&e.local, "Opera Software/Opera Stable/Cache");
        let ts = browsers(&e, true);
        let ids: Vec<&str> = ts.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["chrome.cache", "opera.cache", "firefox.cache"], "Edge has no cache yet");
        let chrome: Vec<String> = ts[0].places.iter().map(|p| p.dir.strip_prefix(&e.local).unwrap().to_string_lossy().replace('\\', "/")).collect();
        assert_eq!(chrome, ["Google/Chrome/User Data/Default/Cache", "Google/Chrome/User Data/Profile 1/Code Cache", "Google/Chrome/User Data/ShaderCache"]);
        assert_eq!(ts[1].places.len(), 2, "Opera: its profile in the root, and its cache apart");
        assert_eq!(ts[0].group, Group::Browser("Google Chrome"));
        assert!(ts.iter().all(|t| t.places.iter().all(|p| p.filter == Filter::All)));
        // Linux: the config and the cache folders; Firefox from Snap too.
        mk(&e.config, "google-chrome/Default/GPUCache");
        mk(&e.cache, "google-chrome/Default/Cache");
        mk(&e.home, "snap/firefox/common/.cache/mozilla/firefox/x.default/cache2");
        let ts = browsers(&e, false);
        assert_eq!(ts.iter().map(|t| (t.id.as_str(), t.places.len())).collect::<Vec<_>>(), [("chrome.cache", 2), ("firefox.cache", 1)]);
        assert_eq!(ts[1].process, ["firefox", "firefox-bin"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn system_targets() {
        let e = Env { temp: PathBuf::from("T"), windows: PathBuf::from("W"), local: PathBuf::from("L"), program_data: PathBuf::from("P"), ..Env::default() };
        let w = windows(&e);
        let ids: Vec<&str> = w.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids.len(), ids.iter().collect::<std::collections::HashSet<_>>().len(), "ids are unique");
        let temp = &w[0];
        assert_eq!((temp.places[0].filter, temp.admin, temp.default_on), (Filter::OlderThanHours(24), false, true));
        assert!(w.iter().any(|t| t.kind == Kind::RecycleBin && !t.default_on));
        let dumps = w.iter().find(|t| t.id == "win.dumps").unwrap();
        assert!(!dumps.places[1].deep(), "never the whole Windows folder");
        assert!(linux(&e).iter().all(|t| !t.admin));
    }
}
