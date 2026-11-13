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

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A NUL-terminated string at `at`: UTF-16 or one byte a character.
fn str_at(b: &[u8], at: usize, wide: bool) -> Option<String> {
    let rest = b.get(at..)?;
    if wide {
        let w: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|&c| c != 0).collect();
        Some(String::from_utf16_lossy(&w))
    } else {
        // The system's code page; Latin-1 keeps the common accented letters.
        Some(rest.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect())
    }
}

/// The file a shortcut opens, from its bytes (MS-SHLLINK): the LinkInfo's
/// local path, else the relative path from the shortcut's folder, else the
/// target with environment variables (`EnvironmentVariableDataBlock`).
pub fn target_of(bytes: &[u8], lnk_dir: &Path) -> Option<PathBuf> {
    if u32_at(bytes, 0)? != 0x4C {
        return None;
    }
    let flags = u32_at(bytes, 0x14)?;
    let wide = flags & 0x80 != 0;
    let mut at = 0x4C;
    if flags & 0x01 != 0 {
        at += 2 + u16_at(bytes, at)? as usize;
    }
    let mut local: Option<String> = None;
    if flags & 0x02 != 0 {
        let info = at;
        let size = u32_at(bytes, info)? as usize;
        let header = u32_at(bytes, info + 4)? as usize;
        let li_flags = u32_at(bytes, info + 8)?;
        if li_flags & 1 != 0 {
            let base = u32_at(bytes, info + 16)? as usize;
            let suffix = u32_at(bytes, info + 24)? as usize;
            let (base_w, suffix_w) = if header >= 0x24 { (u32_at(bytes, info + 28)? as usize, u32_at(bytes, info + 32)? as usize) } else { (0, 0) };
            let b = if base_w > 0 { str_at(bytes, info + base_w, true)? } else { str_at(bytes, info + base, false)? };
            let s = if suffix_w > 0 { str_at(bytes, info + suffix_w, true)? } else { str_at(bytes, info + suffix, false).unwrap_or_default() };
            local = Some(format!("{b}{s}"));
        }
        at = info + size;
    }
    // StringData: name, relative path, working dir, arguments, icon.
    let mut relative = None;
    for (bit, keep) in [(0x04, false), (0x08, true), (0x10, false), (0x20, false), (0x40, false)] {
        if flags & bit == 0 {
            continue;
        }
        let n = u16_at(bytes, at)? as usize;
        let len = if wide { n * 2 } else { n };
        if keep {
            let s = bytes.get(at + 2..at + 2 + len)?;
            relative = Some(if wide { String::from_utf16_lossy(&s.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>()) } else { s.iter().map(|&c| c as char).collect() });
        }
        at += 2 + len;
    }
    if let Some(l) = local.filter(|l| !l.is_empty()) {
        return Some(PathBuf::from(l));
    }
    // Extra data blocks: the target with environment variables.
    while let (Some(size), Some(sig)) = (u32_at(bytes, at), u32_at(bytes, at + 4)) {
        if size < 8 {
            break;
        }
        if sig == 0xA000_0001 {
            let t = str_at(bytes, at + 8 + 260, true).filter(|s| !s.is_empty()).or_else(|| str_at(bytes, at + 8, false))?;
            return Some(PathBuf::from(expand(&t)));
        }
        at += size as usize;
    }
    relative.map(|r| lnk_dir.join(r))
}

#[cfg(windows)]
fn expand(s: &str) -> String {
    rt_sys::win::reg::expand(s)
}

#[cfg(not(windows))]
fn expand(s: &str) -> String {
    s.to_string()
}

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

    /// A shortcut file with a LinkInfo local path (and Unicode strings).
    pub fn lnk(target: &str, relative: Option<&str>) -> Vec<u8> {
        let mut b = vec![0u8; 0x4C];
        b[0..4].copy_from_slice(&0x4Cu32.to_le_bytes());
        let flags: u32 = 0x02 | 0x80 | if relative.is_some() { 0x08 } else { 0 };
        b[0x14..0x18].copy_from_slice(&flags.to_le_bytes());
        // LinkInfo: header of 0x1C, the local path after it, an empty suffix.
        let mut li = vec![0u8; 0x1C];
        let path_at = li.len();
        li.extend(target.as_bytes());
        li.push(0);
        let suffix_at = li.len();
        li.push(0);
        let size = li.len() as u32;
        li[0..4].copy_from_slice(&size.to_le_bytes());
        li[4..8].copy_from_slice(&0x1Cu32.to_le_bytes());
        li[8..12].copy_from_slice(&1u32.to_le_bytes());
        li[16..20].copy_from_slice(&(path_at as u32).to_le_bytes());
        li[24..28].copy_from_slice(&(suffix_at as u32).to_le_bytes());
        b.extend(li);
        if let Some(r) = relative {
            let w: Vec<u16> = r.encode_utf16().collect();
            b.extend((w.len() as u16).to_le_bytes());
            for c in w {
                b.extend(c.to_le_bytes());
            }
        }
        b.extend([0u8; 4]);
        b
    }

    #[test]
    fn link_targets() {
        let d = Path::new("C:\\Menu");
        assert_eq!(target_of(&lnk("C:\\Program Files\\App\\app.exe", None), d), Some(PathBuf::from("C:\\Program Files\\App\\app.exe")));
        assert_eq!(target_of(&lnk("", Some("..\\x.exe")), d), Some(d.join("..\\x.exe")));
        assert_eq!(target_of(b"not a shortcut at all, no", d), None);
        assert_eq!(target_of(&[0x4C, 0, 0, 0], d), None);
    }

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
