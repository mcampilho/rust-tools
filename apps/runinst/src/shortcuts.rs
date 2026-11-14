//! Shortcuts on the Start menu and on the desktop: for every user (the
//! shared folders, readable by anyone), for the user running runinst, and,
//! with administrator rights, in the other users' profiles. Each is read
//! from its `.lnk` file (the Shell Link format, read here without COM), and
//! matched to the program it belongs to.

use crate::inventory::{command_file, Me, Program, Scope};
use std::path::{Path, PathBuf};

/// Where a shortcut is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Place {
    StartMenu,
    Desktop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    /// The `.lnk` file.
    pub path: PathBuf,
    /// Its name, without `.lnk`.
    pub name: String,
    pub place: Place,
    /// For whom: everyone, or one user.
    pub scope: Scope,
    /// What it opens; None for an "advertised" Windows Installer shortcut,
    /// which names a product instead of a file.
    pub target: Option<PathBuf>,
}

impl Shortcut {
    /// Whether what it opens is gone.
    pub fn broken(&self) -> bool {
        self.target.as_ref().is_some_and(|t| !t.exists() && t.is_absolute())
    }
}

/// What a shortcut opens (see `rt_sys::lnk`).
pub use rt_sys::lnk::target_of;

/// Every `.lnk` below `dir`.
fn walk(dir: &Path, place: Place, scope: &Scope, out: &mut Vec<Shortcut>) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                let target = std::fs::read(&p).ok().and_then(|b| target_of(&b, &d));
                let name = p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                out.push(Shortcut { path: p, name, place, scope: scope.clone(), target });
            }
        }
    }
}

/// The shortcuts of everyone, of the desktop's user, and of the users given
/// (their name and profile folder).
pub fn read(me: &Me, others: &[(String, PathBuf)]) -> Vec<Shortcut> {
    let mut out = Vec::new();
    let env = |v: &str| std::env::var_os(v).map(PathBuf::from);
    if let Some(pd) = env("ProgramData") {
        walk(&pd.join("Microsoft\\Windows\\Start Menu\\Programs"), Place::StartMenu, &Scope::Machine, &mut out);
    }
    if let Some(public) = env("PUBLIC") {
        walk(&public.join("Desktop"), Place::Desktop, &Scope::Machine, &mut out);
    }
    let mine = Scope::User { name: me.name.clone(), current: true };
    // runinst may run as another account: the desktop user's folders then
    // come from their profile, not from this process's variables.
    let appdata = if me.runs_as { env("APPDATA") } else { me.home.as_ref().map(|h| h.join("AppData\\Roaming")) };
    if let Some(ad) = appdata {
        walk(&ad.join("Microsoft\\Windows\\Start Menu\\Programs"), Place::StartMenu, &mine, &mut out);
    }
    if let Some(desk) = my_desktop(me) {
        walk(&desk, Place::Desktop, &mine, &mut out);
    }
    for (name, home) in others {
        let scope = Scope::User { name: name.clone(), current: false };
        walk(&home.join("AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs"), Place::StartMenu, &scope, &mut out);
        walk(&home.join("Desktop"), Place::Desktop, &scope, &mut out);
    }
    out
}

/// The desktop user's desktop folder (it may have moved, to OneDrive for one).
fn my_desktop(me: &Me) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use rt_sys::win::reg::{Key, Root, View};
        const FOLDERS: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\User Shell Folders";
        if me.runs_as {
            return Key::open(Root::CurrentUser, FOLDERS, View::Bits64)?.string("Desktop").map(PathBuf::from);
        }
        // Their value as stored (`%USERPROFILE%\Desktop`), with their profile.
        let home = me.home.as_ref()?;
        let raw = match Key::open(Root::Users, &format!("{}\\{FOLDERS}", me.sid), View::Bits64).and_then(|k| k.value("Desktop")) {
            Some(rt_sys::win::reg::Value::Expand(s)) | Some(rt_sys::win::reg::Value::Str(s)) => s,
            _ => return Some(home.join("Desktop")),
        };
        let lower = raw.to_lowercase();
        Some(match lower.find("%userprofile%") {
            Some(i) => PathBuf::from(format!("{}{}{}", &raw[..i], home.display(), &raw[i + 13..])),
            None => PathBuf::from(rt_sys::win::reg::expand(&raw)),
        })
    }
    #[cfg(not(windows))]
    {
        me.home.as_ref().map(|h| h.join("Desktop"))
    }
}

fn lower(p: &Path) -> String {
    p.to_string_lossy().to_lowercase().trim_end_matches(['\\', '/']).to_string()
}

/// The folder a program lives in: its install folder, or the folder of its
/// icon or of its uninstaller (when not the system's own folder).
pub fn home_of(p: &Program) -> Option<String> {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()).to_lowercase();
    let usable = |d: String| (d.len() > 3 && !d.starts_with(&windir) && !d.ends_with("\\installer")).then_some(d);
    if let Some(l) = p.location.as_ref().map(|l| lower(l)).and_then(usable) {
        return Some(l);
    }
    let from_file = |f: &Path| f.parent().map(lower).and_then(usable);
    p.icon.as_ref().and_then(|(f, _)| from_file(f)).or_else(|| p.uninstall.as_deref().and_then(command_file).and_then(|f| from_file(&f)))
}

/// Simplified for comparing names: lower case, letters and digits only.
fn plain(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Which shortcuts belong to which program (indexes into `shortcuts`, one
/// list per program): by the folder of what they open, or else by name.
pub fn match_programs(programs: &[Program], shortcuts: &[Shortcut]) -> Vec<Vec<usize>> {
    let homes: Vec<Option<String>> = programs.iter().map(home_of).collect();
    let names: Vec<String> = programs.iter().map(|p| plain(&p.name)).collect();
    let mut out = vec![Vec::new(); programs.len()];
    for (si, s) in shortcuts.iter().enumerate() {
        let by_folder = s.target.as_ref().map(|t| lower(t)).and_then(|t| {
            // The program with the longest folder holding the target.
            homes
                .iter()
                .enumerate()
                .filter_map(|(i, h)| h.as_ref().filter(|h| t.starts_with(h.as_str()) && t[h.len()..].starts_with('\\')).map(|h| (i, h.len())))
                .max_by_key(|&(_, n)| n)
                .map(|(i, _)| i)
        });
        let found = by_folder.or_else(|| {
            let n = plain(&s.name);
            (n.len() >= 4).then(|| names.iter().position(|p| *p == n || p.starts_with(&n) && n.len() * 2 > p.len())).flatten()
        });
        if let Some(i) = found {
            out[i].push(si);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::Bits;

    fn program(name: &str, location: Option<&str>, icon: Option<&str>) -> Program {
        Program {
            name: name.into(),
            publisher: String::new(),
            version: String::new(),
            size: None,
            date: None,
            location: location.map(PathBuf::from),
            uninstall: None,
            quiet: None,
            modify: None,
            icon: icon.and_then(crate::inventory::parse_icon),
            msi: None,
            system: false,
            update: false,
            no_remove: false,
            scope: Scope::Machine,
            bits: Bits::B64,
            key: "k".into(),
            values: Vec::new(),
            ..Program::default()
        }
    }

    #[test]
    fn shortcuts_find_their_program() {
        let programs = vec![
            program("Big Editor 3", Some("D:\\Apps\\Big"), None),
            program("Big Editor Tools", Some("D:\\Apps\\Big\\Tools\\"), None),
            program("Viewer", None, Some("\"D:\\Viewer\\viewer.exe\",0")),
            program("Something Else", None, None),
        ];
        let s = |name: &str, t: Option<&str>| Shortcut { path: PathBuf::from(format!("{name}.lnk")), name: name.into(), place: Place::StartMenu, scope: Scope::Machine, target: t.map(PathBuf::from) };
        let shortcuts = vec![
            s("Big", Some("D:\\Apps\\Big\\big.exe")),
            s("Tools", Some("D:\\Apps\\Big\\Tools\\t.exe")),
            s("Viewer", Some("d:\\viewer\\viewer.exe")),
            s("Something Else", None),
            s("Other", Some("D:\\Apps\\Bigger\\x.exe")),
        ];
        let m = match_programs(&programs, &shortcuts);
        assert_eq!(m, vec![vec![0], vec![1], vec![2], vec![3]]);
    }

    /// What this computer has: `cargo test -p runinst --lib report -- --ignored --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn report() {
        let t = std::time::Instant::now();
        let inv = crate::inventory::read();
        let shortcuts = read(&inv.me, &inv.other_users);
        let m = match_programs(&inv.programs, &shortcuts);
        let visible = inv.programs.iter().filter(|p| p.visible()).count();
        let mine = inv.programs.iter().filter(|p| matches!(p.scope, Scope::User { current: true, .. })).count();
        let linked: usize = m.iter().map(|v| v.len()).sum();
        let broken = shortcuts.iter().filter(|s| s.broken()).count();
        println!("{} entries ({visible} shown, {mine} this user's), other users read {:?}, not read {:?}", inv.programs.len(), inv.other_users, inv.unread_users);
        println!("{} shortcuts, {linked} matched to a program, {broken} broken; {:?}", shortcuts.len(), t.elapsed());
        for (p, s) in inv.programs.iter().zip(&m).filter(|(p, _)| p.visible() && !p.sources.contains(&crate::inventory::Source::Registry)) {
            println!("  {} | {} | {:?} | {:?} | {} shortcuts", p.name, p.publisher, p.scope, p.sources, s.len());
        }
        for p in inv.programs.iter().filter(|p| !p.problems.is_empty()) {
            println!("  problem: {} {:?} | {:?}", p.name, p.problems, p.uninstall);
        }
        let scoop = crate::managers::scoop_all(&inv.me);
        for p in scoop.iter().filter(|p| !p.problems.is_empty()) {
            println!("  scoop: {} {:?}", p.name, p.problems);
        }
        let mut programs = inv.programs.clone();
        let t = std::time::Instant::now();
        if let Some(text) = crate::winget::run() {
            let rows = crate::winget::parse(&text);
            let before = programs.len();
            crate::winget::merge(&mut programs, rows.clone());
            println!("winget: {} rows in {:?}, {} new", rows.len(), t.elapsed(), programs.len() - before);
            for p in &programs[before..] {
                println!("  only winget: {} | {:?}", p.name, p.winget.as_ref().map(|w| &w.id));
            }
            for p in programs.iter().filter(|p| p.winget.as_ref().is_some_and(|w| w.available.is_some())) {
                println!("  update: {} {} → {:?}", p.name, p.version, p.winget.as_ref().and_then(|w| w.available.clone()));
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn reads_this_computer() {
        let all = read(&crate::inventory::me(), &[]);
        // Windows has shortcuts on the shared Start menu.
        assert!(all.iter().any(|s| s.scope == Scope::Machine && s.place == Place::StartMenu));
        assert!(all.iter().filter(|s| s.target.is_some()).count() * 2 >= all.len().min(2));
    }
}
