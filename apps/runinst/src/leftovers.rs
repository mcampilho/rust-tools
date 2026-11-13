//! What an uninstaller leaves behind: the install folder, folders named
//! after the program (or its publisher) in Program Files, ProgramData and
//! AppData, shortcuts to what is gone, the program's keys under `Software`,
//! its startup entries, an `Uninstall` entry whose uninstaller is gone, and,
//! with Everything running, folders of the same name anywhere else.
//!
//! Nothing here is removed without the user ticking it and saying yes: keys
//! and values are first saved to a `.reg` file (double-click it to put them
//! back), files and folders go to the Recycle Bin. A folder that holds, or is
//! held by, a program still installed, or one of Windows' own folders, is
//! never offered.

use crate::inventory::{Me, Program};
use crate::shortcuts::Shortcut;
use std::path::{Path, PathBuf};

/// Where it was found, which says how sure it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    /// The install folder, or a folder of its name in Program Files or
    /// ProgramData.
    Folder,
    Shortcut,
    /// Started with Windows (`Run`).
    Startup,
    /// Keys under `HKEY_LOCAL_MACHINE\SOFTWARE`, and an `Uninstall` entry left.
    Registry,
    /// The user's own: AppData, and keys under the user's `Software`. Not
    /// ticked at first: settings are often worth keeping.
    Settings,
    /// Found by Everything outside the usual places. Not ticked at first.
    Elsewhere,
    /// A service that runs something it had.
    Service,
    /// A scheduled task that runs something it had.
    Task,
    /// A folder with no file in it.
    Empty,
}

impl Group {
    /// Ticked when found.
    pub fn ticked_at_first(self) -> bool {
        !matches!(self, Group::Settings | Group::Elsewhere)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Folder,
    File,
    Key,
    Value,
    /// A service: `path` is its key, `value` its name.
    Service,
    /// A scheduled task: `path` is its name (`\Folder\Task`), `value` the
    /// file that holds it.
    Task,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leftover {
    /// The program it belonged to.
    pub program: String,
    pub group: Group,
    pub kind: Kind,
    /// A file or folder; or a key as `regedit` writes it
    /// (`HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\X`), always in the 64-bit view.
    pub path: String,
    /// A value's name (`Kind::Value`; empty: the default value).
    pub value: Option<String>,
    /// What the value holds.
    pub data: Option<String>,
    /// Bytes, for files and folders.
    pub size: Option<u64>,
    /// Removing it needs administrator rights.
    pub admin: bool,
    /// Less sure (found from a hint only): not ticked at first.
    pub doubt: bool,
}

/// Words that say nothing about a program: versions, architectures.
fn noise(word: &str) -> bool {
    let w = word.trim_matches(|c: char| "()[],-".contains(c)).to_lowercase();
    if w.is_empty() {
        return true;
    }
    // 10, v2, 1.5.0.1423b.
    let v = w.trim_start_matches('v');
    let numeric = v.chars().all(|c| c.is_ascii_digit() || c == '.');
    if v.starts_with(|c: char| c.is_ascii_digit()) && (numeric || v.contains('.') && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')) {
        return true;
    }
    matches!(w.as_str(), "x64" | "x86" | "x32" | "64-bit" | "32-bit" | "64bit" | "32bit" | "amd64" | "arm64" | "win64" | "win32")
}

/// A name as compared: lower case, letters and digits only.
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Names of folders and keys that belong to no program in particular.
const GENERIC: &[&str] = &[
    "microsoft", "windows", "commonfiles", "programfiles", "programfilesx86", "programdata", "appdata", "local", "locallow", "roaming", "temp", "tmp",
    "cache", "caches", "packages", "programs", "startmenu", "desktop", "documents", "users", "public", "default", "system", "system32", "drivers", "data",
    "config", "settings", "bin", "lib", "app", "apps", "tools", "update", "updater", "updates", "setup", "install", "installer", "software", "classes",
    "policies", "wow6432node", "clients", "registeredapplications", "logs", "log", "uninstall", "runtime", "shared", "common", "plugins", "user",
];

fn generic(n: &str) -> bool {
    n.len() < 4 || GENERIC.contains(&n)
}

/// The names its folders and keys may have, as written (for Everything) and
/// compared: the name without version words, and the install folder's.
pub fn names(p: &Program) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut add = |raw: String| {
        let n = norm(&raw);
        if !generic(&n) && !out.iter().any(|(_, m)| *m == n) {
            out.push((raw, n));
        }
    };
    add(p.name.split_whitespace().filter(|w| !noise(w)).collect::<Vec<_>>().join(" "));
    // The install folder's name (split by hand: a Windows path on any system).
    if let Some(l) = &p.location {
        let s = l.to_string_lossy();
        if let Some(last) = s.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next() {
            add(last.to_string());
        }
    }
    out
}

/// The publisher, as a folder of theirs would be named: no "Inc.", "Ltd"...
pub fn publisher(p: &Program) -> String {
    let words: Vec<&str> = p
        .publisher
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| {
            let n = norm(w);
            !n.is_empty() && !matches!(n.as_str(), "inc" | "ltd" | "llc" | "corp" | "corporation" | "gmbh" | "co" | "sa" | "srl" | "limited" | "ag" | "bv" | "oy" | "ab" | "sro" | "pty")
        })
        .collect();
    let n = norm(&words.join(" "));
    if n.len() < 3 || GENERIC.contains(&n.as_str()) {
        String::new()
    } else {
        n
    }
}

/// Whether `a` is `b` or inside it (paths compared without case).
pub fn within(a: &str, b: &str) -> bool {
    let a = a.trim_end_matches(['\\', '/']).to_lowercase();
    let b = b.trim_end_matches(['\\', '/']).to_lowercase();
    !b.is_empty() && (a == b || a.starts_with(&format!("{b}\\")) || a.starts_with(&format!("{b}/")))
}

/// What must stay: folders of programs still installed (nothing inside
/// them, and nothing holding them, is offered) and the places searched,
/// Windows, the profiles (nothing holding them).
#[derive(Clone, Debug, Default)]
pub struct Keep {
    /// Neither these, nor what they hold, nor what holds them.
    pub programs: Vec<String>,
    /// Neither these nor what holds them.
    pub places: Vec<String>,
}

impl Keep {
    pub fn protects(&self, path: &str) -> bool {
        // A drive, or a folder right below one.
        let parts = path.trim_end_matches(['\\', '/']).split(['\\', '/']).filter(|s| !s.is_empty()).count();
        parts < 3
            || self.programs.iter().any(|k| within(path, k) || within(k, path))
            || self.places.iter().any(|k| within(k, path))
    }
}

/// One program's leftovers among the folders below `bases`: folders named
/// after it, or its publisher's folder holding them (that one only when
/// nothing else of the publisher is installed and it holds nothing else).
pub fn in_folders(p: &Program, bases: &[(PathBuf, Group)], keep: &Keep, publisher_in_use: bool, admin_places: &[String]) -> Vec<Leftover> {
    let names = names(p);
    let publ = publisher(p);
    let mut out = Vec::new();
    let push = |path: &Path, group: Group, out: &mut Vec<Leftover>| {
        let s = path.to_string_lossy().into_owned();
        if keep.protects(&s) || out.iter().any(|l: &Leftover| within(&s, &l.path)) {
            return;
        }
        let kind = if path.is_dir() { Kind::Folder } else { Kind::File };
        out.push(Leftover { program: p.name.clone(), group, kind, admin: admin_places.iter().any(|a| within(&s, a)), path: s, value: None, data: None, size: None, doubt: false });
    };
    let matches = |n: &str| names.iter().any(|(_, m)| m == n);
    for (base, group) in bases {
        let Ok(rd) = std::fs::read_dir(base) else { continue };
        for e in rd.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let n = norm(&e.file_name().to_string_lossy());
            if matches(&n) {
                push(&e.path(), *group, &mut out);
            } else if !publ.is_empty() && n == publ {
                let kids: Vec<_> = std::fs::read_dir(e.path()).map(|r| r.flatten().collect()).unwrap_or_default();
                let mine: Vec<_> = kids.iter().filter(|k| k.file_type().is_ok_and(|t| t.is_dir()) && matches(&norm(&k.file_name().to_string_lossy()))).collect();
                if mine.is_empty() {
                    continue;
                }
                if !publisher_in_use && mine.len() == kids.len() {
                    push(&e.path(), *group, &mut out);
                } else {
                    for k in mine {
                        push(&k.path(), *group, &mut out);
                    }
                }
            }
        }
    }
    out
}

/// Shortcuts that open something in the install folder, or something gone
/// with the program's name.
pub fn in_shortcuts(p: &Program, shortcuts: &[Shortcut], admin_places: &[String]) -> Vec<Leftover> {
    let names = names(p);
    let loc = p.location.as_ref().map(|l| l.to_string_lossy().into_owned()).filter(|l| !l.is_empty());
    shortcuts
        .iter()
        .filter(|s| s.path.exists())
        .filter(|s| {
            let into_folder = loc.as_deref().zip(s.target.as_ref()).is_some_and(|(l, t)| within(&t.to_string_lossy(), l) && !t.exists());
            let named = s.broken() && names.iter().any(|(_, n)| norm(&s.name).contains(n.as_str()));
            into_folder || named
        })
        .map(|s| {
            let path = s.path.to_string_lossy().into_owned();
            Leftover {
                program: p.name.clone(),
                group: Group::Shortcut,
                kind: Kind::File,
                admin: admin_places.iter().any(|a| within(&path, a)),
                path,
                value: None,
                data: s.target.as_ref().map(|t| t.to_string_lossy().into_owned()),
                size: None,
                doubt: false,
            }
        })
        .collect()
}

/// Whether a folder Everything found elsewhere may be the program's: on the
/// system's drive or the program's, not in another user's profile, not in
/// a place where folders of that name are someone else's (package managers,
/// projects, caches, backups, recycle bins); and with a name of more than
/// one word, or inside its publisher's folder (a name like "Steam" or
/// "Everything" alone is everywhere).
pub fn elsewhere_ok(path: &str, raw_name: &str, publisher: &str, drives: &[String], home: Option<&str>) -> bool {
    let lower = path.to_lowercase();
    let drive = lower.split('\\').next().unwrap_or("");
    if !drives.iter().any(|d| d.trim_end_matches('\\').eq_ignore_ascii_case(drive)) {
        return false;
    }
    if lower.contains("\\users\\") && home.is_none_or(|h| !within(path, h)) {
        return false;
    }
    const NOT: &[&str] = &[
        "\\$recycle.bin", "\\#recycle", "\\scoop\\", "\\chocolatey\\", "\\site-packages\\", "\\node_modules\\", "\\.git\\", "\\temp\\", "\\cache\\",
        "\\caches\\", "\\backup", "\\src\\", "\\projects\\", "\\winget\\", "\\installer\\", "\\.vscode\\", "\\steamapps\\",
    ];
    if NOT.iter().any(|n| lower.contains(n)) {
        return false;
    }
    let parent = path.rsplit_once('\\').map_or("", |(p, _)| p);
    let parent_name = parent.rsplit('\\').next().unwrap_or("");
    raw_name.trim().contains(' ') || (!publisher.is_empty() && norm(parent_name) == publisher)
}

/// A folder's size, counting at most `limit` entries.
pub fn size_of(path: &Path, limit: usize) -> u64 {
    let mut total = 0;
    let mut seen = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > limit {
                return total;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(t) if t.is_file() => total += e.metadata().map_or(0, |m| m.len()),
                _ => {}
            }
        }
    }
    total
}

/// A value saved as a `.reg` line: `"name"="text"`, or `hex(2):` for an
/// expandable one.
pub fn reg_value_line(name: &str, data: &str, expand: bool) -> String {
    let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let n = if name.is_empty() { "@".to_string() } else { format!("\"{}\"", esc(name)) };
    if expand {
        let bytes: Vec<String> = data.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).map(|b| format!("{b:02x}")).collect();
        format!("{n}=hex(2):{}", bytes.join(","))
    } else {
        format!("{n}=\"{}\"", esc(data))
    }
}

/// What the search needs to know.
pub struct Context<'a> {
    pub me: &'a Me,
    pub admin: bool,
    /// The programs still installed (after reading again).
    pub installed: &'a [Program],
    pub shortcuts: &'a [Shortcut],
}

/// Looks for what the programs uninstalled left behind.
pub fn find(gone: &[Program], cx: &Context, stop: &std::sync::atomic::AtomicBool) -> Vec<Leftover> {
    #[cfg(windows)]
    {
        win::find(gone, cx, stop)
    }
    #[cfg(not(windows))]
    {
        lin::find(gone, cx, stop)
    }
}

/// Looks for what programs removed before left behind, without uninstalling
/// anything: entries whose uninstaller is gone (as if just uninstalled), and
/// shortcuts, startup entries, services and tasks that run what is gone,
/// the folders they point into, and empty folders where programs go.
pub fn find_orphans(cx: &Context, stop: &std::sync::atomic::AtomicBool) -> Vec<Leftover> {
    #[cfg(windows)]
    {
        win::find_orphans(cx, stop)
    }
    #[cfg(not(windows))]
    {
        lin::find_orphans(cx, stop)
    }
}

/// How removing one went.
pub type Removed = Result<(), String>;

/// Removes the leftovers: keys and values saved first to `backup` (a `.reg`
/// file), then deleted; files and folders to the Recycle Bin.
pub fn remove(items: &[Leftover], backup: &Path) -> Vec<Removed> {
    #[cfg(windows)]
    {
        win::remove(items, backup)
    }
    #[cfg(not(windows))]
    {
        // Only files and folders of the user's on Linux: to the trash.
        let _ = backup;
        items
            .iter()
            .map(|l| match l.kind {
                Kind::Folder | Kind::File => rt_sys::recycle(Path::new(&l.path)),
                _ => Err(format!("{:?}", l.kind)),
            })
            .collect()
    }
}

/// The name of a package as its folders are named: no architecture
/// (`htop:amd64`, `firefox.x86_64`).
pub fn package_base(p: &Program) -> String {
    let id = crate::linux_pkgs::package_id(p);
    let base = id.split(':').next().unwrap_or(id);
    let base = if p.sources.first() == Some(&crate::inventory::Source::Rpm) { base.rsplit_once('.').map_or(base, |(a, _)| a) } else { base };
    norm(base)
}

/// The program a `.desktop` file starts (`Exec=`), when it names one by its
/// full path.
pub fn desktop_exec(text: &str) -> Option<PathBuf> {
    let line = text.lines().find_map(|l| l.strip_prefix("Exec="))?;
    let first = line.split_whitespace().next()?.trim_matches('"');
    first.starts_with('/').then(|| PathBuf::from(first))
}

/// Linux: the user's own folders (`~/.config`, `~/.local/share`, `~/.cache`,
/// `~/.local/state`, `~/.<name>`), Flatpak's (`~/.var/app/<id>`) and Snap's
/// (`~/snap/<name>`) data, and their menu entries (`.desktop`). The system's
/// files are the package manager's: its settings left in `/etc` show in the
/// list ("Only settings left"), and removing that entry purges them.
#[cfg(not(windows))]
mod lin {
    use super::*;
    use crate::inventory::Source;
    use crate::linux_pkgs::value;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// XDG folders that are no program's.
    const NEVER: &[&str] = &[
        "applications", "icons", "fonts", "trash", "keyrings", "sounds", "themes", "mime", "systemd", "autostart", "recentlyused", "gvfsmetadata",
        "flatpak", "snap", "dbus1", "pulse", "userdirs", "gtk30", "gtk40", "dconf", "nautilus", "backgrounds",
    ];

    struct Places {
        home: PathBuf,
        /// (folder, group): where a folder of the program's name may be.
        bases: Vec<(PathBuf, Group)>,
    }

    fn places(cx: &Context) -> Option<Places> {
        let home = cx.me.home.clone().filter(|h| h.is_dir())?;
        let bases = vec![
            (home.join(".config"), Group::Settings),
            (home.join(".local/share"), Group::Settings),
            (home.join(".local/state"), Group::Settings),
            (home.join(".cache"), Group::Folder),
        ];
        Some(Places { home, bases })
    }

    fn leftover(program: &str, path: &Path, group: Group) -> Leftover {
        let kind = if path.is_dir() { Kind::Folder } else { Kind::File };
        let size = Some(if kind == Kind::Folder { size_of(path, 200_000) } else { std::fs::metadata(path).map_or(0, |m| m.len()) });
        Leftover { program: program.to_string(), group, kind, path: path.to_string_lossy().into_owned(), value: None, data: None, size, admin: false, doubt: false }
    }

    /// The names (compared) of what is still installed.
    fn installed_names(cx: &Context) -> HashSet<String> {
        cx.installed.iter().filter(|p| !p.problems.contains(&crate::inventory::Problem::ConfigOnly)).flat_map(|p| names(p).into_iter().map(|(_, n)| n).chain([package_base(p)])).collect()
    }

    use std::collections::HashSet;

    pub fn find(gone: &[Program], cx: &Context, stop: &AtomicBool) -> Vec<Leftover> {
        let Some(pl) = places(cx) else { return Vec::new() };
        let keep = installed_names(cx);
        let mut out: Vec<Leftover> = Vec::new();
        for p in gone {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let mut push = |path: PathBuf, group: Group| {
                if path.exists() && !out.iter().any(|l| l.path == path.to_string_lossy()) {
                    out.push(leftover(&p.name, &path, group));
                }
            };
            match p.sources.first() {
                Some(Source::Flatpak) => {
                    if let Some(id) = value(p, "Application") {
                        push(pl.home.join(".var/app").join(id), Group::Settings);
                    }
                    continue;
                }
                Some(Source::Snap) => {
                    push(pl.home.join("snap").join(&p.name), Group::Settings);
                    continue;
                }
                _ => {}
            }
            // Folders named after the package or the program.
            let mut wanted: Vec<String> = names(p).into_iter().map(|(_, n)| n).collect();
            let base = package_base(p);
            if !generic(&base) && !wanted.contains(&base) {
                wanted.push(base);
            }
            wanted.retain(|n| !keep.contains(n) && !NEVER.contains(&n.as_str()));
            if wanted.is_empty() {
                continue;
            }
            let hit = |name: &str| wanted.contains(&norm(name.trim_start_matches('.')));
            for (dir, group) in &pl.bases {
                for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                    if hit(&e.file_name().to_string_lossy()) {
                        push(e.path(), *group);
                    }
                }
            }
            // ~/.name
            for e in std::fs::read_dir(&pl.home).into_iter().flatten().flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with('.') && e.path().is_dir() && hit(&n) {
                    push(e.path(), Group::Settings);
                }
            }
            // Its menu entries of the user's, if they start what is gone.
            for e in std::fs::read_dir(pl.home.join(".local/share/applications")).into_iter().flatten().flatten() {
                let path = e.path();
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let gone_exec = std::fs::read_to_string(&path).ok().and_then(|t| desktop_exec(&t)).is_some_and(|f| !f.exists());
                if gone_exec && wanted.iter().any(|w| norm(&stem).contains(w.as_str())) {
                    push(path, Group::Shortcut);
                }
            }
        }
        out
    }

    pub fn find_orphans(cx: &Context, stop: &AtomicBool) -> Vec<Leftover> {
        let Some(pl) = places(cx) else { return Vec::new() };
        let mut out = Vec::new();
        let listed = |s: Source| cx.installed.iter().filter(move |p| p.sources.first() == Some(&s));
        // Flatpak's and Snap's data of apps no longer there (only when the
        // list could be read, or the manager is gone altogether).
        let flatpak_ids: HashSet<&str> = listed(Source::Flatpak).filter_map(|p| value(p, "Application")).collect();
        if !flatpak_ids.is_empty() || crate::linux_pkgs::on_path(&["flatpak"]).is_none() {
            for e in std::fs::read_dir(pl.home.join(".var/app")).into_iter().flatten().flatten() {
                let id = e.file_name().to_string_lossy().into_owned();
                if !flatpak_ids.contains(id.as_str()) {
                    out.push(leftover(&id, &e.path(), Group::Folder));
                }
            }
        }
        let snaps: HashSet<&str> = listed(Source::Snap).map(|p| p.name.as_str()).collect();
        if !snaps.is_empty() || crate::linux_pkgs::on_path(&["snap"]).is_none() {
            for e in std::fs::read_dir(pl.home.join("snap")).into_iter().flatten().flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if e.path().is_dir() && !snaps.contains(name.as_str()) {
                    out.push(leftover(&name, &e.path(), Group::Folder));
                }
            }
        }
        if stop.load(Ordering::Relaxed) {
            return out;
        }
        // The user's menu entries that start what is gone.
        for e in std::fs::read_dir(pl.home.join(".local/share/applications")).into_iter().flatten().flatten() {
            let path = e.path();
            if path.extension().is_some_and(|x| x == "desktop") {
                if let Some(exec) = std::fs::read_to_string(&path).ok().and_then(|t| desktop_exec(&t)).filter(|f| !f.exists()) {
                    let mut l = leftover(&path.file_stem().unwrap_or_default().to_string_lossy(), &path, Group::Shortcut);
                    l.data = Some(exec.to_string_lossy().into_owned());
                    out.push(l);
                }
            }
        }
        // Empty folders right below the user's XDG folders (not those of a
        // program still installed).
        let keep = installed_names(cx);
        for (dir, _) in &pl.bases {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = e.path();
                let n = norm(&e.file_name().to_string_lossy());
                let empty = std::fs::read_dir(&path).is_ok_and(|mut r| r.next().is_none());
                if empty && e.file_type().is_ok_and(|t| t.is_dir()) && !NEVER.contains(&n.as_str()) && !keep.contains(&n) {
                    out.push(leftover(&e.file_name().to_string_lossy(), &path, Group::Empty));
                }
            }
        }
        out
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use crate::inventory::Source;
    use rt_sys::win::reg::{Key, Root, Value, View};
    use std::sync::atomic::{AtomicBool, Ordering};

    fn env(v: &str) -> Option<PathBuf> {
        std::env::var_os(v).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
    }

    /// `HKEY_LOCAL_MACHINE\SOFTWARE\X` → the root and the rest.
    pub fn split_key(path: &str) -> Option<(Root, &str)> {
        let (root, rest) = path.split_once('\\')?;
        let r = match root.to_uppercase().as_str() {
            "HKEY_LOCAL_MACHINE" | "HKLM" => Root::LocalMachine,
            "HKEY_CURRENT_USER" | "HKCU" => Root::CurrentUser,
            "HKEY_USERS" | "HKU" => Root::Users,
            _ => return None,
        };
        Some((r, rest))
    }

    fn key_exists(path: &str) -> bool {
        split_key(path).is_some_and(|(r, rest)| Key::open(r, rest, View::Bits64).is_some())
    }

    /// The desktop user's `Software` key, as written.
    fn user_software(me: &Me) -> String {
        format!("HKEY_USERS\\{}\\Software", me.sid)
    }

    /// Keys named after the program, or its publisher's key holding them,
    /// below each of `roots`.
    fn in_keys(p: &Program, roots: &[(String, Group, bool)], publisher_in_use: bool, out: &mut Vec<Leftover>) {
        let names = names(p);
        let publ = publisher(p);
        let matches = |n: &str| names.iter().any(|(_, m)| m == n);
        for (root, group, admin) in roots {
            let Some((r, rest)) = split_key(root) else { continue };
            let Some(k) = Key::open(r, rest, View::Bits64) else { continue };
            let mut push = |path: String| {
                if !out.iter().any(|l| within(&path, &l.path)) {
                    out.push(Leftover { program: p.name.clone(), group: *group, kind: Kind::Key, path, value: None, data: None, size: None, doubt: false, admin: *admin });
                }
            };
            for sub in k.subkeys() {
                let n = norm(&sub);
                if matches(&n) {
                    push(format!("{root}\\{sub}"));
                } else if !publ.is_empty() && n == publ {
                    let Some(pk) = k.sub(&sub, View::Bits64) else { continue };
                    let kids = pk.subkeys();
                    let mine: Vec<&String> = kids.iter().filter(|c| matches(&norm(c))).collect();
                    if mine.is_empty() {
                        continue;
                    }
                    if !publisher_in_use && mine.len() == kids.len() && pk.values().is_empty() {
                        push(format!("{root}\\{sub}"));
                    } else {
                        for c in mine {
                            push(format!("{root}\\{sub}\\{c}"));
                        }
                    }
                }
            }
        }
    }

    /// Startup entries that start something in the install folder, or
    /// something gone with the program's name.
    fn in_startup(p: &Program, roots: &[(String, bool)], out: &mut Vec<Leftover>) {
        let names = names(p);
        let loc = p.location.as_ref().map(|l| l.to_string_lossy().into_owned()).filter(|l| l.len() > 3);
        for (key, admin) in roots {
            let Some((r, rest)) = split_key(key) else { continue };
            let Some(k) = Key::open(r, rest, View::Bits64) else { continue };
            for (name, v) in k.values() {
                let data = match &v {
                    Value::Str(s) | Value::Expand(s) => s.clone(),
                    _ => continue,
                };
                let file = crate::inventory::command_file(&rt_sys::win::reg::expand(&data));
                let into = loc.as_deref().zip(file.as_ref()).is_some_and(|(l, f)| within(&f.to_string_lossy(), l) && !f.exists());
                let named = file.as_ref().is_some_and(|f| f.is_absolute() && !f.exists()) && names.iter().any(|(_, n)| norm(&name).contains(n.as_str()));
                if into || named {
                    out.push(Leftover {
                        program: p.name.clone(),
                        group: Group::Startup,
                        kind: Kind::Value,
                        path: key.clone(),
                        value: Some(name),
                        data: Some(data),
                        size: None,
                        admin: *admin,
                        doubt: false,
                    });
                }
            }
        }
    }

    /// A service, from its key under `Services`.
    struct Service {
        name: String,
        display: String,
        image: String,
        file: Option<PathBuf>,
    }

    /// A scheduled task, from its file under `System32\Tasks`.
    struct Task {
        /// `\Folder\Task`.
        name: String,
        xml: PathBuf,
        command: String,
        file: Option<PathBuf>,
    }

    const SERVICES: &str = "HKEY_LOCAL_MACHINE\\SYSTEM\\CurrentControlSet\\Services";

    /// What a service's `ImagePath` runs, as a full path.
    pub fn image_file(image: &str) -> Option<PathBuf> {
        let root = env("SystemRoot").map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "C:\\Windows".into());
        let t = rt_sys::win::reg::expand(image.trim());
        let t = t.strip_prefix("\\??\\").unwrap_or(&t).to_string();
        let lower = t.to_lowercase();
        let t = if let Some(rest) = lower.strip_prefix("\\systemroot\\") {
            format!("{root}\\{}", &t[t.len() - rest.len()..])
        } else if lower.starts_with("system32\\") || lower.starts_with("syswow64\\") {
            format!("{root}\\{t}")
        } else {
            t
        };
        crate::inventory::command_file(&t)
    }

    fn services() -> Vec<Service> {
        let Some((r, rest)) = split_key(SERVICES) else { return Vec::new() };
        let Some(k) = Key::open(r, rest, View::Bits64) else { return Vec::new() };
        let mut out = Vec::new();
        for name in k.subkeys() {
            let Some(s) = k.sub(&name, View::Bits64) else { continue };
            let Some(image) = s.string("ImagePath") else { continue };
            // Services of their own process or shared ones, and drivers.
            let display = s.string("DisplayName").filter(|d| !d.starts_with('@')).unwrap_or_default();
            out.push(Service { file: image_file(&image), name, display, image });
        }
        out
    }

    /// The text between `<tag>` and `</tag>`, XML entities undone.
    fn xml_text(xml: &str, tag: &str) -> String {
        let open = format!("<{tag}>");
        let Some(a) = xml.find(&open) else { return String::new() };
        let rest = &xml[a + open.len()..];
        let t = rest.split(&format!("</{tag}>")).next().unwrap_or("");
        t.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&").trim().to_string()
    }

    /// The tasks whose files can be read (all of them as administrator).
    fn tasks() -> Vec<Task> {
        let Some(dir) = env("SystemRoot").map(|r| r.join("System32\\Tasks")) else { return Vec::new() };
        let mut out = Vec::new();
        let mut stack = vec![dir.clone()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    stack.push(p);
                    continue;
                }
                let Ok(b) = std::fs::read(&p) else { continue };
                let text = if b.starts_with(&[0xFF, 0xFE]) {
                    String::from_utf16_lossy(&b[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>())
                } else {
                    String::from_utf8_lossy(&b).into_owned()
                };
                let command = xml_text(&text, "Command");
                if command.is_empty() {
                    continue;
                }
                let file = crate::inventory::command_file(&format!("\"{}\"", rt_sys::win::reg::expand(command.trim_matches('"'))));
                let args = xml_text(&text, "Arguments");
                let name = format!("\\{}", p.strip_prefix(&dir).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default());
                out.push(Task { name, xml: p, command: if args.is_empty() { command } else { format!("{command} {args}") }, file });
            }
        }
        out
    }

    /// Whether `f` names a file that is gone from a drive that is there (a
    /// drive not plugged in says nothing).
    fn gone_file(f: &Path) -> bool {
        let s = f.to_string_lossy();
        f.is_absolute() && !s.starts_with("\\\\") && !f.exists() && s.get(..3).is_some_and(|d| Path::new(d).exists())
    }

    /// Everything a search needs, read once.
    struct Scan<'a> {
        cx: &'a Context<'a>,
        bases: Vec<(PathBuf, Group)>,
        admin_places: Vec<String>,
        keep: Keep,
        home: Option<PathBuf>,
        machine_soft: Vec<(String, Group, bool)>,
        run_keys: Vec<(String, bool)>,
        services: Vec<Service>,
        tasks: Vec<Task>,
    }

    impl<'a> Scan<'a> {
        /// The places to look in; what stays: the folders of `installed`.
        fn new(cx: &'a Context<'a>, installed: &[&Program]) -> Scan<'a> {
            let mut pf: Vec<PathBuf> = ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"].iter().filter_map(|v| env(v)).collect();
            pf.dedup();
            let pd = env("ProgramData");
            let home = cx.me.home.clone().or_else(|| env("USERPROFILE"));
            let mut bases: Vec<(PathBuf, Group)> = pf.iter().map(|p| (p.clone(), Group::Folder)).collect();
            // Shared files of a publisher: `Common Files\<publisher>\<program>`.
            bases.extend(pf.iter().map(|p| (p.join("Common Files"), Group::Folder)));
            let mut admin_places: Vec<String> = pf.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            if let Some(pd) = &pd {
                bases.push((pd.clone(), Group::Folder));
                bases.push((pd.join("Microsoft\\Windows\\Start Menu\\Programs"), Group::Shortcut));
                admin_places.push(pd.to_string_lossy().into_owned());
            }
            if let Some(h) = &home {
                for sub in ["AppData\\Local", "AppData\\Roaming", "AppData\\LocalLow"] {
                    bases.push((h.join(sub), Group::Settings));
                }
                bases.push((h.join("AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs"), Group::Shortcut));
                bases.push((h.join("AppData\\Local\\Programs"), Group::Folder));
            }
            if let Some(w) = env("SystemRoot") {
                admin_places.push(w.to_string_lossy().into_owned());
            }
            let mut keep = Keep::default();
            for p in installed {
                if let Some(l) = &p.location {
                    let s = l.to_string_lossy().into_owned();
                    if s.len() > 3 {
                        keep.programs.push(s);
                    }
                }
            }
            keep.places.extend(bases.iter().map(|(b, _)| b.to_string_lossy().into_owned()));
            keep.places.extend(admin_places.iter().cloned());
            keep.places.extend(home.iter().map(|h| h.to_string_lossy().into_owned()));
            keep.places.extend(env("SystemRoot").iter().map(|w| w.to_string_lossy().into_owned()));
            if let Ok(me) = std::env::current_exe() {
                keep.programs.push(me.to_string_lossy().into_owned());
            }
            let machine_soft = vec![
                ("HKEY_LOCAL_MACHINE\\SOFTWARE".to_string(), Group::Registry, !cx.admin),
                ("HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node".to_string(), Group::Registry, !cx.admin),
                (user_software(cx.me), Group::Settings, false),
            ];
            let run_keys = vec![
                ("HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run".to_string(), !cx.admin),
                ("HKEY_LOCAL_MACHINE\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run".to_string(), !cx.admin),
                (format!("{}\\Microsoft\\Windows\\CurrentVersion\\Run", user_software(cx.me)), false),
            ];
            Scan { cx, bases, admin_places, keep, home, machine_soft, run_keys, services: services(), tasks: tasks() }
        }

        fn admin_of(&self, s: &str) -> bool {
            !self.cx.admin && self.admin_places.iter().any(|a| within(s, a))
        }

        /// Services and tasks that run something in the program's folder,
        /// or something gone with its name.
        fn in_services_and_tasks(&self, p: &Program, out: &mut Vec<Leftover>) {
            let names = names(p);
            let loc = p.location.as_ref().map(|l| l.to_string_lossy().into_owned()).filter(|l| l.len() > 3);
            let hits = |file: &Option<PathBuf>, label: &str| {
                let into = loc.as_deref().zip(file.as_ref()).is_some_and(|(l, f)| within(&f.to_string_lossy(), l));
                let named = file.as_ref().is_some_and(|f| gone_file(f)) && names.iter().any(|(_, n)| norm(label).contains(n.as_str()));
                into || named
            };
            for s in &self.services {
                if hits(&s.file, &format!("{} {}", s.name, s.display)) {
                    out.push(service_leftover(&p.name, s, self.cx.admin, false));
                }
            }
            for t in &self.tasks {
                if hits(&t.file, &t.name) {
                    out.push(task_leftover(&p.name, t, false));
                }
            }
        }

        /// One program's leftovers.
        fn program(&self, p: &Program, stop: &AtomicBool) -> Vec<Leftover> {
            let cx = self.cx;
            let mut mine: Vec<Leftover> = Vec::new();
            let leftover = |group, s: String, size| Leftover {
                program: p.name.clone(),
                group,
                kind: Kind::Folder,
                admin: self.admin_of(&s),
                path: s,
                value: None,
                data: None,
                size,
                doubt: false,
            };
            // Scoop keeps what an app wants to keep in `persist`.
            if p.sources.first() == Some(&Source::Scoop) {
                let root = Path::new(&p.key).parent().and_then(Path::parent);
                if let Some(dir) = root.map(|r| r.join("persist").join(&p.name)).filter(|d| d.exists()) {
                    mine.push(leftover(Group::Settings, dir.to_string_lossy().into_owned(), None));
                }
                if let Some(dir) = Some(PathBuf::from(&p.key)).filter(|d| d.exists()) {
                    mine.push(leftover(Group::Folder, dir.to_string_lossy().into_owned(), None));
                }
                return mine;
            }
            // Store packages are removed whole by Windows.
            if p.package.is_some() || p.sources.first() == Some(&Source::Chocolatey) {
                return mine;
            }
            // The install folder.
            if let Some(l) = p.location.as_ref().filter(|l| l.is_dir()) {
                let s = l.to_string_lossy().into_owned();
                if !self.keep.protects(&s) {
                    mine.push(leftover(Group::Folder, s, None));
                }
            }
            let publ = publisher(p);
            let publ_in_use = !publ.is_empty() && cx.installed.iter().any(|q| q.key != p.key && publisher(q) == publ);
            for l in in_folders(p, &self.bases, &self.keep, publ_in_use, &self.admin_places) {
                if !mine.iter().any(|m| within(&l.path, &m.path)) {
                    mine.push(Leftover { admin: l.admin && !cx.admin, ..l });
                }
            }
            for l in in_shortcuts(p, cx.shortcuts, &self.admin_places) {
                if !mine.iter().any(|m| within(&l.path, &m.path)) {
                    mine.push(Leftover { admin: l.admin && !cx.admin, ..l });
                }
            }
            in_keys(p, &self.machine_soft, publ_in_use, &mut mine);
            in_startup(p, &self.run_keys, &mut mine);
            self.in_services_and_tasks(p, &mut mine);
            // Its Uninstall entry, when its uninstaller is gone.
            let gone_cmd = p.uninstall.as_deref().and_then(crate::inventory::command_file).is_none_or(|f| f.is_absolute() && !f.exists());
            if p.key.starts_with("HKEY_") && gone_cmd && key_exists(&p.key) {
                let admin = p.key.starts_with("HKEY_LOCAL_MACHINE") && !cx.admin;
                mine.push(Leftover { program: p.name.clone(), group: Group::Registry, kind: Kind::Key, path: p.key.clone(), value: None, data: None, size: None, doubt: false, admin });
            }
            // Everything: folders of its name anywhere else.
            let mut drives: Vec<String> = env("SystemDrive").map(|d| d.to_string_lossy().into_owned()).into_iter().collect();
            if let Some(d) = p.location.as_ref().and_then(|l| l.to_string_lossy().get(..2).map(str::to_string)) {
                drives.push(d);
            }
            let home_s = self.home.as_ref().map(|h| h.to_string_lossy().into_owned());
            for (raw, n) in names(p) {
                let q = format!("folder:wfn:\"{raw}\"");
                let Ok(items) = rt_scan::everything::search(&q, stop) else { break };
                for it in items.iter().filter(|i| i.folder && norm(&i.name) == n) {
                    let s = format!("{}\\{}", it.path.trim_end_matches('\\'), it.name);
                    if self.keep.protects(&s) || !elsewhere_ok(&s, &raw, &publ, &drives, home_s.as_deref()) || mine.iter().any(|m| within(&s, &m.path) || within(&m.path, &s)) {
                        continue;
                    }
                    mine.push(Leftover { group: Group::Elsewhere, ..leftover(Group::Elsewhere, s, it.size) });
                }
            }
            mine
        }

        /// The folder right below one of the places looked in that holds
        /// `f` and is still there (`C:\Program Files\X` for `…\X\bin\x.exe`).
        fn folder_of(&self, f: &Path) -> Option<(String, Group)> {
            let s = f.to_string_lossy();
            for (base, group) in &self.bases {
                let b = base.to_string_lossy();
                if !within(&s, &b) || s.len() <= b.len() + 1 {
                    continue;
                }
                let first = s[b.len() + 1..].split('\\').next()?;
                let dir = format!("{}\\{first}", b.trim_end_matches('\\'));
                if Path::new(&dir).is_dir() && !self.keep.protects(&dir) && !GENERIC.contains(&norm(first).as_str()) {
                    return Some((dir, *group));
                }
            }
            None
        }

        /// What programs removed before left, without a program to start from.
        fn orphans(&self, stop: &AtomicBool) -> Vec<Leftover> {
            let cx = self.cx;
            let mut out: Vec<Leftover> = Vec::new();
            // Entries whose uninstaller is gone: as if just uninstalled.
            for p in cx.installed.iter().filter(|p| p.problems.contains(&crate::inventory::Problem::UninstallerMissing)) {
                if stop.load(Ordering::Relaxed) {
                    return out;
                }
                out.extend(self.program(p, stop));
            }
            // Folders of programs whose files are gone, from what still points at them.
            let hinted = |out: &mut Vec<Leftover>, f: &Path, who: &str| {
                if let Some((dir, group)) = self.folder_of(f) {
                    if !out.iter().any(|l| within(&dir, &l.path) || within(&l.path, &dir)) {
                        let admin = self.admin_of(&dir);
                        out.push(Leftover { program: who.to_string(), group, kind: Kind::Folder, path: dir, value: None, data: None, size: None, admin, doubt: true });
                    }
                }
            };
            // Shortcuts to what is gone.
            for s in cx.shortcuts.iter().filter(|s| s.path.exists() && s.target.as_ref().is_some_and(|t| gone_file(t))) {
                let path = s.path.to_string_lossy().into_owned();
                let t = s.target.clone().unwrap_or_default();
                out.push(Leftover {
                    program: s.name.clone(),
                    group: Group::Shortcut,
                    kind: Kind::File,
                    admin: self.admin_of(&path),
                    path,
                    value: None,
                    data: Some(t.to_string_lossy().into_owned()),
                    size: None,
                    doubt: false,
                });
                hinted(&mut out, &t, &s.name);
            }
            // Startup entries that start what is gone.
            for (key, admin) in &self.run_keys {
                let Some((r, rest)) = split_key(key) else { continue };
                let Some(k) = Key::open(r, rest, View::Bits64) else { continue };
                for (name, v) in k.values() {
                    let (Value::Str(data) | Value::Expand(data)) = v else { continue };
                    let Some(f) = crate::inventory::command_file(&rt_sys::win::reg::expand(&data)) else { continue };
                    if gone_file(&f) {
                        out.push(Leftover { program: name.clone(), group: Group::Startup, kind: Kind::Value, path: key.clone(), value: Some(name.clone()), data: Some(data), size: None, admin: *admin, doubt: false });
                        hinted(&mut out, &f, &name);
                    }
                }
            }
            // Services and tasks that run what is gone (not Windows' own).
            for s in &self.services {
                if s.file.as_ref().is_some_and(|f| gone_file(f)) {
                    let who = if s.display.is_empty() { s.name.clone() } else { s.display.clone() };
                    out.push(service_leftover(&who, s, cx.admin, true));
                }
            }
            for t in self.tasks.iter().filter(|t| !t.name.to_lowercase().starts_with("\\microsoft\\")) {
                if let Some(f) = t.file.as_ref().filter(|f| gone_file(f)) {
                    let who = t.name.rsplit('\\').next().unwrap_or(&t.name).to_string();
                    out.push(task_leftover(&who, t, false));
                    hinted(&mut out, f, &who);
                }
            }
            // Empty folders right below the places programs use.
            for (base, _) in &self.bases {
                let Ok(rd) = std::fs::read_dir(base) else { continue };
                for e in rd.flatten() {
                    let p = e.path();
                    let s = p.to_string_lossy().into_owned();
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    if !plain_dir(&p) || never_empty_leftover(&name) || self.keep.protects(&s) || !empty_tree(&p, 2_000) {
                        continue;
                    }
                    if !out.iter().any(|l| within(&s, &l.path)) {
                        out.push(Leftover { program: e.file_name().to_string_lossy().into_owned(), group: Group::Empty, kind: Kind::Folder, admin: self.admin_of(&s), path: s, value: None, data: None, size: Some(0), doubt: false });
                    }
                }
            }
            out
        }
    }

    /// A folder, not a link to one, nor hidden by the system.
    fn plain_dir(p: &Path) -> bool {
        use std::os::windows::fs::MetadataExt;
        const HIDDEN: u32 = 0x2;
        const SYSTEM: u32 = 0x4;
        std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink() && m.file_attributes() & (HIDDEN | SYSTEM) != (HIDDEN | SYSTEM))
    }

    /// Folders of Windows that are empty on purpose.
    fn never_empty_leftover(name: &str) -> bool {
        const KEEP: &[&str] = &["windows", "microsoft", "modifiablewindowsapps", "windowsapps", "uninstall information", "reference assemblies", "internet explorer", "common files", "msbuild", "packages", "temp", "crashdumps", "virtualstore", "desktop", "documents", "start menu", "templates", "application data", "favorites", "ssh", "regid.1991-06.com.microsoft", "package cache", "usoshared", "usoprivate", "sophia", "comms", "d3dscache", "history", "programs", "startup", "accessibility", "administrative tools", "maintenance", "system tools", "softwaredistribution", "placeholdertilelogofolder", "packagemanagement"];
        KEEP.iter().any(|k| name == *k || name.starts_with(&format!("{k} "))) || name.starts_with("windows") || name.starts_with("microsoft") || GENERIC.contains(&norm(name).as_str())
    }

    /// Whether a folder holds no file at any depth (looking at most at `limit` entries).
    fn empty_tree(p: &Path, limit: usize) -> bool {
        let mut seen = 0;
        let mut stack = vec![p.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else { return false };
            for e in rd.flatten() {
                seen += 1;
                if seen > limit {
                    return false;
                }
                match e.file_type() {
                    Ok(t) if t.is_dir() && !t.is_symlink() => stack.push(e.path()),
                    _ => return false,
                }
            }
        }
        true
    }

    fn service_leftover(program: &str, s: &Service, admin: bool, doubt: bool) -> Leftover {
        Leftover {
            program: program.to_string(),
            group: Group::Service,
            kind: Kind::Service,
            path: format!("{SERVICES}\\{}", s.name),
            value: Some(s.name.clone()),
            data: Some(s.image.clone()),
            size: None,
            admin: !admin,
            doubt,
        }
    }

    fn task_leftover(program: &str, t: &Task, doubt: bool) -> Leftover {
        Leftover {
            program: program.to_string(),
            group: Group::Task,
            kind: Kind::Task,
            path: t.name.clone(),
            value: Some(t.xml.to_string_lossy().into_owned()),
            data: Some(t.command.clone()),
            size: None,
            admin: false,
            doubt,
        }
    }

    /// The same thing found twice: once; then the sizes of files and folders.
    fn finish(mut out: Vec<Leftover>) -> Vec<Leftover> {
        let mut seen = std::collections::HashSet::new();
        out.retain(|l| seen.insert((l.path.to_lowercase(), l.value.clone())));
        for l in &mut out {
            if l.size.is_none() && matches!(l.kind, Kind::Folder | Kind::File) {
                let p = Path::new(&l.path);
                l.size = Some(if p.is_dir() { size_of(p, 200_000) } else { p.metadata().map_or(0, |m| m.len()) });
            }
        }
        out
    }

    pub fn find(gone: &[Program], cx: &Context, stop: &AtomicBool) -> Vec<Leftover> {
        let installed: Vec<&Program> = cx.installed.iter().collect();
        let scan = Scan::new(cx, &installed);
        let mut out = Vec::new();
        for p in gone {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            out.extend(scan.program(p, stop));
        }
        finish(out)
    }

    pub fn find_orphans(cx: &Context, stop: &AtomicBool) -> Vec<Leftover> {
        // Entries whose uninstaller is gone are not "installed": their
        // folders are not kept.
        let installed: Vec<&Program> = cx.installed.iter().filter(|p| !p.problems.contains(&crate::inventory::Problem::UninstallerMissing)).collect();
        let scan = Scan::new(cx, &installed);
        finish(scan.orphans(stop))
    }

    fn hidden(args: &[&str]) -> Result<(), String> {
        run_hidden("reg.exe", args)
    }

    /// Runs a tool of Windows without a window; its message when it fails.
    fn run_hidden(exe: &str, args: &[&str]) -> Result<(), String> {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x0800_0000;
        let o = std::process::Command::new(exe).args(args).creation_flags(NO_WINDOW).stdin(std::process::Stdio::null()).output().map_err(|e| e.to_string())?;
        if o.status.success() {
            Ok(())
        } else {
            let mut t = String::from_utf8_lossy(&o.stderr).trim().to_string();
            if t.is_empty() {
                t = String::from_utf8_lossy(&o.stdout).trim().to_string();
            }
            Err(t)
        }
    }

    /// A `.reg` file as `reg export` writes it: UTF-16 with its header.
    fn read_reg(path: &Path) -> Option<String> {
        let b = std::fs::read(path).ok()?;
        let u: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        let s = String::from_utf16_lossy(&u);
        Some(s.trim_start_matches('\u{feff}').to_string())
    }

    const HEADER: &str = "Windows Registry Editor Version 5.00";

    pub fn remove(items: &[Leftover], backup: &Path) -> Vec<Removed> {
        let mut results: Vec<Removed> = vec![Ok(()); items.len()];
        // First the copy of every key and value.
        let mut text = format!("{HEADER}\r\n");
        let tmp = std::env::temp_dir().join(format!("runinst-{}.reg", std::process::id()));
        let mut saved = vec![false; items.len()];
        // Tasks: their files beside the `.reg` (`schtasks /Create /XML` puts one back).
        let task_dir = backup.with_extension("tasks");
        for (i, l) in items.iter().enumerate() {
            match l.kind {
                Kind::Task => {
                    let file = task_dir.join(format!("{}.xml", l.path.trim_start_matches('\\').replace('\\', " - ")));
                    let copied = std::fs::create_dir_all(&task_dir).and_then(|_| std::fs::copy(l.value.as_deref().unwrap_or(""), &file));
                    match copied {
                        Ok(_) => saved[i] = true,
                        Err(e) => results[i] = Err(e.to_string()),
                    }
                }
                Kind::Key | Kind::Service => match hidden(&["export", &l.path, &tmp.to_string_lossy(), "/y", "/reg:64"]).and_then(|_| read_reg(&tmp).ok_or_else(|| "export".to_string())) {
                    Ok(t) => {
                        text += t.trim_start().trim_start_matches(HEADER).trim_start_matches(['\r', '\n']);
                        text += "\r\n";
                        saved[i] = true;
                    }
                    Err(e) => results[i] = Err(e),
                },
                Kind::Value => {
                    let (Some((r, rest)), Some(name)) = (split_key(&l.path), &l.value) else { continue };
                    let expand = matches!(Key::open(r, rest, View::Bits64).and_then(|k| k.value(name)), Some(Value::Expand(_)));
                    text += &format!("[{}]\r\n{}\r\n\r\n", l.path, reg_value_line(name, l.data.as_deref().unwrap_or(""), expand));
                    saved[i] = true;
                }
                _ => {}
            }
        }
        let _ = std::fs::remove_file(&tmp);
        if items.iter().zip(&saved).any(|(l, s)| *s && l.kind != Kind::Task) {
            let mut bytes = vec![0xFF, 0xFE];
            bytes.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
            let written = backup.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(backup, bytes));
            if let Err(e) = written {
                // No copy, no deleting.
                for (i, s) in saved.iter_mut().enumerate() {
                    if *s && items[i].kind != Kind::Task {
                        results[i] = Err(e.to_string());
                        *s = false;
                    }
                }
            }
        }
        for (i, l) in items.iter().enumerate() {
            results[i] = match l.kind {
                Kind::Key if saved[i] => hidden(&["delete", &l.path, "/f", "/reg:64"]),
                // Its key went with another one ticked.
                Kind::Value if saved[i] && !key_exists(&l.path) => Ok(()),
                Kind::Value if saved[i] => match l.value.as_deref() {
                    Some("") => hidden(&["delete", &l.path, "/ve", "/f", "/reg:64"]),
                    Some(n) => hidden(&["delete", &l.path, "/v", n, "/f", "/reg:64"]),
                    None => Err("no value".into()),
                },
                // Stopped first (it may not be running), then deleted; one
                // already gone counts as removed.
                Kind::Service if saved[i] => {
                    let name = l.value.as_deref().unwrap_or("");
                    let _ = run_hidden("sc.exe", &["stop", name]);
                    match run_hidden("sc.exe", &["delete", name]) {
                        Err(e) if e.contains("1060") => Ok(()),
                        r => r,
                    }
                }
                Kind::Task if saved[i] => run_hidden("schtasks.exe", &["/Delete", "/TN", &l.path, "/F"]),
                Kind::Key | Kind::Value | Kind::Service | Kind::Task => results[i].clone(),
                Kind::Folder | Kind::File => rt_sys::recycle(Path::new(&l.path)),
            };
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Scope;

    fn prog(name: &str, publisher: &str, location: Option<&Path>) -> Program {
        Program { name: name.into(), publisher: publisher.into(), location: location.map(Path::to_path_buf), scope: Scope::Machine, ..Program::default() }
    }

    #[test]
    fn names_and_publishers() {
        assert!(noise("1.2.3") && noise("(x64)") && noise("v10") && noise("64-bit") && noise("1.5.0.1423b") && !noise("Notes") && !noise("7-Zip") && !noise("3D"));
        let p = prog("Everything 1.5.0.1423b (x64)", "voidtools", Some(Path::new("C:\\Program Files\\Everything 1.5a")));
        let n: Vec<String> = names(&p).into_iter().map(|x| x.1).collect();
        assert_eq!(n, ["everything", "everything15a"]);
        let p = prog("Bitdefender Total Security", "Bitdefender", None);
        assert_eq!(names(&p)[0], ("Bitdefender Total Security".to_string(), "bitdefendertotalsecurity".to_string()));
        assert_eq!(publisher(&p), "bitdefender");
        assert_eq!(publisher(&prog("x", "IObit Information Technology, Inc.", None)), "iobitinformationtechnology");
        assert_eq!(publisher(&prog("x", "Microsoft Corporation", None)), "");
        // Too generic to be a program's folder.
        assert!(names(&prog("Update", "", Some(Path::new("C:\\x\\bin")))).is_empty());
    }

    #[test]
    fn what_stays() {
        let keep = Keep { programs: vec!["C:\\Program Files\\Other".into()], places: vec!["C:\\Program Files".into(), "C:\\Users\\me".into()] };
        assert!(keep.protects("C:\\"));
        assert!(keep.protects("C:\\Games"));
        assert!(keep.protects("C:\\Program Files"));
        assert!(keep.protects("C:\\Program Files\\Other\\plugins"));
        assert!(keep.protects("C:\\Users"));
        assert!(!keep.protects("C:\\Program Files\\Gone"));
        assert!(!keep.protects("C:\\Users\\me\\AppData\\Local\\Gone"));
        assert!(within("c:\\a\\B", "C:\\A") && !within("C:\\AB", "C:\\A"));
    }

    #[test]
    fn folders_found() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("leftovers-test");
        let _ = std::fs::remove_dir_all(&root);
        let pf = root.join("Program Files");
        let data = root.join("AppData");
        for d in [pf.join("Zeta Editor"), pf.join("Acme").join("Notes"), pf.join("Acme").join("Notes Helper"), data.join("Acme").join("Notes"), pf.join("Other")] {
            std::fs::create_dir_all(&d).unwrap();
        }
        let bases = [(pf.clone(), Group::Folder), (data.clone(), Group::Settings)];
        let keep = Keep { programs: vec![pf.join("Other").to_string_lossy().into_owned()], places: bases.iter().map(|b| b.0.to_string_lossy().into_owned()).collect() };
        let admin = [pf.to_string_lossy().into_owned()];
        let z = in_folders(&prog("Zeta Editor 2.1", "Zeta", None), &bases, &keep, false, &admin);
        assert_eq!(z.len(), 1);
        assert!(z[0].path.ends_with("Zeta Editor") && z[0].admin && z[0].group == Group::Folder);
        // The publisher's folder holds something else: only the program's.
        let n = in_folders(&prog("Notes", "Acme Ltd", None), &bases, &keep, false, &admin);
        let paths: Vec<&str> = n.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths.len(), 2, "{paths:?}");
        assert!(paths[0].ends_with("Acme\\Notes") || paths[0].ends_with("Acme/Notes"));
        // Under AppData, Acme holds only Notes: the whole Acme folder, unless
        // another Acme program is installed.
        assert!(paths[1].ends_with("AppData\\Acme") || paths[1].ends_with("AppData/Acme"), "{paths:?}");
        assert_eq!(n[1].group, Group::Settings);
        let n = in_folders(&prog("Notes", "Acme", None), &bases, &keep, true, &admin);
        assert!(n[1].path.ends_with("Notes"));
        // A program still installed: never.
        assert!(in_folders(&prog("Other", "", None), &bases, &keep, false, &admin).is_empty());
        assert!(size_of(&root, 1000) == 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// What would be offered for each program of this PC, were it gone
    /// (its own folders count, since it is not installed then):
    /// `cargo test -p runinst --lib leftovers_here -- --ignored --nocapture`.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn leftovers_here() {
        let inv = crate::inventory::read();
        let sc = crate::shortcuts::read(&inv.me, &inv.other_users);
        let stop = std::sync::atomic::AtomicBool::new(false);
        let filter = std::env::var("RUNINST_ONLY").unwrap_or_default().to_lowercase();
        for (i, p) in inv.programs.iter().enumerate() {
            if !p.visible() || !p.name.to_lowercase().contains(&filter) {
                continue;
            }
            let others: Vec<Program> = inv.programs.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, q)| q.clone()).collect();
            let cx = Context { me: &inv.me, admin: false, installed: &others, shortcuts: &sc };
            let t = std::time::Instant::now();
            let found = find(std::slice::from_ref(p), &cx, &stop);
            println!("{} ({} ms)", p.name, t.elapsed().as_millis());
            for l in found {
                println!("    {:?} {:?} {}{} {}", l.group, l.kind, l.path, l.value.map(|v| format!(" › {v}")).unwrap_or_default(), if l.admin { "(admin)" } else { "" });
            }
        }
    }

    /// Removes a key, a value and a folder made for the test, and puts the
    /// registry back from the copy (writes to HKCU\Software\runinst-test):
    /// `cargo test -p runinst --lib removes_and_saves -- --ignored`.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn removes_and_saves() {
        use rt_sys::win::reg::{Key, Root, View};
        let reg = |args: &[&str]| assert!(std::process::Command::new("reg.exe").args(args).output().unwrap().status.success(), "{args:?}");
        reg(&["add", "HKCU\\Software\\runinst-test\\Sub", "/v", "Path", "/d", "C:\\a \"b\"", "/f"]);
        reg(&["add", "HKCU\\Software\\runinst-test", "/v", "Run", "/t", "REG_EXPAND_SZ", "/d", "%SystemRoot%\\x.exe", "/f"]);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("leftovers-remove");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f.txt"), "x").unwrap();
        let backup = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("leftovers-backup.reg");
        let _ = std::fs::remove_file(&backup);
        let l = |kind, path: &str, value: Option<&str>, data: Option<&str>| Leftover {
            program: "t".into(),
            group: Group::Registry,
            kind,
            path: path.into(),
            value: value.map(Into::into),
            data: data.map(Into::into),
            size: None,
            admin: false,
            doubt: false,
        };
        let items = [
            l(Kind::Value, "HKEY_CURRENT_USER\\Software\\runinst-test", Some("Run"), Some("%SystemRoot%\\x.exe")),
            l(Kind::Key, "HKEY_CURRENT_USER\\Software\\runinst-test\\Sub", None, None),
            l(Kind::Folder, &dir.to_string_lossy(), None, None),
        ];
        let r = remove(&items, &backup);
        assert!(r.iter().all(Result::is_ok), "{r:?}");
        assert!(!dir.exists());
        let k = Key::open(Root::CurrentUser, "Software\\runinst-test", View::Bits64).unwrap();
        assert!(k.subkeys().is_empty() && k.values().is_empty());
        // The copy puts them back.
        reg(&["import", &backup.to_string_lossy()]);
        let k = Key::open(Root::CurrentUser, "Software\\runinst-test", View::Bits64).unwrap();
        assert_eq!(k.sub("Sub", View::Bits64).and_then(|s| s.string("Path")).as_deref(), Some("C:\\a \"b\""));
        assert!(matches!(k.value("Run"), Some(rt_sys::win::reg::Value::Expand(s)) if s == "%SystemRoot%\\x.exe"));
        reg(&["delete", "HKCU\\Software\\runinst-test", "/f"]);
    }

    #[test]
    fn elsewhere() {
        let d = ["C:".to_string()];
        let home = Some("C:\\Users\\me");
        assert!(elsewhere_ok("C:\\Games\\Advanced SystemCare", "Advanced SystemCare", "iobit", &d, home));
        assert!(elsewhere_ok("C:\\Tools\\voidtools\\Everything", "Everything", "voidtools", &d, home));
        // One word, not in its publisher's folder: anyone's.
        assert!(!elsewhere_ok("C:\\Users\\me\\.cache\\claude", "Claude", "anthropic", &d, home));
        assert!(!elsewhere_ok("S:\\Installed\\IObit\\Smart Defrag", "Smart Defrag", "iobit", &d, home));
        assert!(!elsewhere_ok("C:\\Users\\Admin\\AppData\\Roaming\\IObit\\Smart Defrag", "Smart Defrag", "iobit", &d, home));
        assert!(!elsewhere_ok("C:\\x\\scoop\\apps\\Smart Defrag", "Smart Defrag", "", &d, home));
    }

    /// What "Find leftovers of removed programs" offers on this PC:
    /// `cargo test -p runinst --lib orphans_here -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn orphans_here() {
        let inv = crate::inventory::read();
        let sc = crate::shortcuts::read(&inv.me, &inv.other_users);
        let mut installed = inv.programs.clone();
        installed.extend(crate::managers::scoop_all(&inv.me));
        #[cfg(not(windows))]
        {
            installed.extend(crate::linux_pkgs::flatpak(&inv.me.name).unwrap_or_default());
            installed.extend(crate::linux_pkgs::snap().unwrap_or_default());
        }
        let admin = std::env::var("RUNINST_ADMIN").is_ok();
        let cx = Context { me: &inv.me, admin, installed: &installed, shortcuts: &sc };
        let t = std::time::Instant::now();
        let found = find_orphans(&cx, &std::sync::atomic::AtomicBool::new(false));
        println!("{} in {} ms", found.len(), t.elapsed().as_millis());
        for l in found {
            println!(
                "    {:?} {:?} {} | {}{} {}{}",
                l.group,
                l.kind,
                l.program,
                l.path,
                l.value.map(|v| format!(" › {v}")).unwrap_or_default(),
                if l.admin { "(admin)" } else { "" },
                if l.doubt { "(doubt)" } else { "" }
            );
        }
    }

    #[test]
    fn reg_lines() {
        assert_eq!(reg_value_line("App", "\"C:\\A\\a.exe\" -s", false), "\"App\"=\"\\\"C:\\\\A\\\\a.exe\\\" -s\"");
        assert_eq!(reg_value_line("", "x", false), "@=\"x\"");
        assert_eq!(reg_value_line("E", "a", true), "\"E\"=hex(2):61,00,00,00");
    }
}
