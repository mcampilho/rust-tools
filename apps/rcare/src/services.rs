//! Program Deactivator: the services of other companies that start with
//! Windows. Turning one off sets it to start by hand ("Manual"), as the
//! Services console does: Windows or the program can still start it when
//! it is needed, nothing is deleted, and the start it had is written to a
//! copy (`services.txt` in rcare's data folder) so it can be given back.
//! Windows' own services are never listed. Changing needs administrator rights.

use std::path::{Path, PathBuf};

/// How a service starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Start {
    /// With Windows.
    Auto,
    /// With Windows, a little after the rest.
    Delayed,
    /// When something asks for it.
    Manual,
    /// Never.
    Disabled,
}

impl Start {
    /// The word in the copy file.
    pub fn word(self) -> &'static str {
        match self {
            Start::Auto => "auto",
            Start::Delayed => "delayed",
            Start::Manual => "manual",
            Start::Disabled => "disabled",
        }
    }

    pub fn from_word(w: &str) -> Option<Start> {
        [Start::Auto, Start::Delayed, Start::Manual, Start::Disabled].into_iter().find(|s| s.word() == w)
    }

    /// Whether it starts with Windows.
    pub fn with_windows(self) -> bool {
        matches!(self, Start::Auto | Start::Delayed)
    }

    /// What the list says (a translation key).
    pub fn label(self) -> &'static str {
        match self {
            Start::Auto => "Automatic",
            Start::Delayed => "Automatic (delayed)",
            Start::Manual => "Manual",
            Start::Disabled => "Disabled",
        }
    }
}

/// A service of another company.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
    /// The service's name (the key under `Services`).
    pub name: String,
    /// The name Windows shows.
    pub display: String,
    /// The company of its program, when the program says.
    pub company: Option<String>,
    /// The command line it runs.
    pub command: String,
    pub file: Option<PathBuf>,
    pub start: Start,
    pub running: bool,
    /// How it started before rcare set it to manual (from the copy).
    pub was: Option<Start>,
}

impl Service {
    pub fn on(&self) -> bool {
        self.start.with_windows()
    }
}

/// Whether a program belongs to Windows or another Microsoft product: by
/// the company it names; with no company, by being in the Windows folder.
pub fn is_microsoft(company: Option<&str>, file: Option<&Path>, windows_dir: &Path) -> bool {
    match company.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => c.to_lowercase().contains("microsoft"),
        None => file.is_none_or(|f| starts_with_ci(f, windows_dir)),
    }
}

fn starts_with_ci(path: &Path, dir: &Path) -> bool {
    let (p, d) = (path.to_string_lossy().to_lowercase(), dir.to_string_lossy().to_lowercase());
    let d = d.trim_end_matches('\\');
    !d.is_empty() && (p == d || p.starts_with(&format!("{d}\\")))
}

// ------------------------------------------------------------ the copy

/// The copy: each service rcare changed, and how it started before.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Copy {
    pub items: Vec<(String, Start)>,
}

impl Copy {
    pub const FILE: &'static str = "services.txt";

    /// One line each: name, a tab, the start word. Other lines are left out.
    pub fn parse(text: &str) -> Copy {
        let items = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let (n, w) = l.split_once('\t')?;
                Some((n.trim().to_string(), Start::from_word(w.trim())?)).filter(|(n, _)| !n.is_empty())
            })
            .collect();
        Copy { items }
    }

    pub fn text(&self) -> String {
        let mut s = String::from("# rcare: services set to manual, and how they started before (name, tab, start)\n");
        for (n, w) in &self.items {
            s.push_str(&format!("{n}\t{}\n", w.word()));
        }
        s
    }

    pub fn get(&self, name: &str) -> Option<Start> {
        self.items.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, s)| *s)
    }

    /// Notes how `name` started (the first time only: that is the one to give back).
    pub fn remember(&mut self, name: &str, was: Start) {
        if self.get(name).is_none() {
            self.items.push((name.to_string(), was));
        }
    }

    pub fn forget(&mut self, name: &str) {
        self.items.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
    }

    pub fn load(dir: &Path) -> Copy {
        std::fs::read_to_string(dir.join(Self::FILE)).map(|t| Copy::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(Self::FILE), self.text())
    }
}

/// What the page lists: the services of other companies that start with
/// Windows, and those rcare set to manual (they can be given back).
pub fn keep(all: Vec<Service>, copy: &Copy) -> Vec<Service> {
    let mut out: Vec<Service> = all
        .into_iter()
        .map(|mut s| {
            s.was = copy.get(&s.name);
            s
        })
        .filter(|s| s.on() || (s.was.is_some() && s.start == Start::Manual))
        .collect();
    out.sort_by(|a, b| a.display.to_lowercase().cmp(&b.display.to_lowercase()).then(a.name.cmp(&b.name)));
    out
}

// ------------------------------------------------------------ Windows

#[cfg(windows)]
mod win {
    use super::{Service, Start};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    use windows_sys::Win32::System::Services::{
        ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, OpenServiceW,
        QueryServiceConfig2W, QueryServiceConfigW, ENUM_SERVICE_STATUS_PROCESSW, QUERY_SERVICE_CONFIGW, SC_ENUM_PROCESS_INFO, SC_HANDLE,
        SC_MANAGER_CONNECT, SC_MANAGER_ENUMERATE_SERVICE, SERVICE_AUTO_START, SERVICE_CHANGE_CONFIG,
        SERVICE_CONFIG_DELAYED_AUTO_START_INFO, SERVICE_DELAYED_AUTO_START_INFO, SERVICE_DEMAND_START, SERVICE_DISABLED,
        SERVICE_QUERY_CONFIG, SERVICE_RUNNING, SERVICE_STATE_ALL, SERVICE_WIN32,
    };

    const SERVICE_NO_CHANGE: u32 = 0xffff_ffff;

    struct Handle(SC_HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: the handle is ours.
            unsafe { CloseServiceHandle(self.0) };
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    fn error() -> String {
        std::io::Error::last_os_error().to_string()
    }

    /// SAFETY: `p` is null or a NUL-terminated UTF-16 string.
    unsafe fn string(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
    }

    fn manager(access: u32) -> Result<Handle, String> {
        // SAFETY: null names: this machine, the active database.
        let h = unsafe { OpenSCManagerW(null(), null(), access) };
        if h.is_null() {
            Err(error())
        } else {
            Ok(Handle(h))
        }
    }

    fn open(scm: &Handle, name: &str, access: u32) -> Result<Handle, String> {
        let n = wide(name);
        // SAFETY: valid handle and name.
        let h = unsafe { OpenServiceW(scm.0, n.as_ptr(), access) };
        if h.is_null() {
            Err(error())
        } else {
            Ok(Handle(h))
        }
    }

    /// The start type and command line of a service.
    fn config(svc: &Handle) -> Option<(Start, String)> {
        let mut need = 0u32;
        // SAFETY: asking for the size.
        unsafe { QueryServiceConfigW(svc.0, null_mut(), 0, &mut need) };
        let mut buf = vec![0u64; (need as usize).div_ceil(8).max(1)];
        // SAFETY: the buffer has `need` bytes at least.
        if unsafe { QueryServiceConfigW(svc.0, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32, &mut need) } == 0 {
            return None;
        }
        // SAFETY: the buffer holds the structure and the strings it points to.
        let (start, line) = unsafe {
            let c = &*(buf.as_ptr() as *const QUERY_SERVICE_CONFIGW);
            (c.dwStartType, string(c.lpBinaryPathName))
        };
        let start = match start {
            SERVICE_AUTO_START => {
                let mut d = SERVICE_DELAYED_AUTO_START_INFO { fDelayedAutostart: 0 };
                let mut need = 0u32;
                // SAFETY: the structure the level asks for, with its size.
                let ok = unsafe {
                    QueryServiceConfig2W(
                        svc.0,
                        SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
                        (&mut d as *mut SERVICE_DELAYED_AUTO_START_INFO).cast(),
                        size_of::<SERVICE_DELAYED_AUTO_START_INFO>() as u32,
                        &mut need,
                    )
                };
                if ok != 0 && d.fDelayedAutostart != 0 {
                    Start::Delayed
                } else {
                    Start::Auto
                }
            }
            SERVICE_DEMAND_START => Start::Manual,
            SERVICE_DISABLED => Start::Disabled,
            _ => return None,
        };
        Some((start, line))
    }

    /// The company a program names in its version information.
    pub fn company(file: &std::path::Path) -> Option<String> {
        let path = wide(&file.to_string_lossy());
        // SAFETY: a NUL-terminated path.
        let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), null_mut()) };
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        // SAFETY: the buffer has the size asked for.
        if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
            return None;
        }
        let query = |sub: &str| -> Option<(*const u8, u32)> {
            let q = wide(sub);
            let mut p: *mut core::ffi::c_void = null_mut();
            let mut len = 0u32;
            // SAFETY: `data` is the block just read; the pointer returned points into it.
            let ok = unsafe { VerQueryValueW(data.as_ptr().cast(), q.as_ptr(), &mut p, &mut len) };
            (ok != 0 && !p.is_null() && len > 0).then_some((p as *const u8, len))
        };
        // The languages the file has, then English (US) in Unicode and in Windows' code page.
        let mut langs = Vec::new();
        if let Some((p, len)) = query("\\VarFileInfo\\Translation") {
            for k in 0..(len as usize / 4) {
                // SAFETY: `len` bytes from `p` are inside `data`; pairs of u16.
                let (lang, cp) = unsafe { (p.add(k * 4).cast::<u16>().read_unaligned(), p.add(k * 4 + 2).cast::<u16>().read_unaligned()) };
                langs.push(format!("{lang:04x}{cp:04x}"));
            }
        }
        langs.extend(["040904b0".to_string(), "040904e4".to_string()]);
        for l in langs {
            if let Some((p, len)) = query(&format!("\\StringFileInfo\\{l}\\CompanyName")) {
                // SAFETY: `len` UTF-16 units from `p`, inside `data`.
                let s = unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(p.cast::<u16>(), len as usize)) };
                // The length can run past the string's end (some files say so): up to the first NUL.
                let s = s.split('\0').next().unwrap_or("").trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        None
    }

    /// Every service of Windows' own kind (not drivers), with its start and program.
    pub fn all() -> Vec<Service> {
        let Ok(scm) = manager(SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE) else { return Vec::new() };
        let mut need = 0u32;
        let mut count = 0u32;
        let mut resume = 0u32;
        // SAFETY: asking for the size.
        unsafe {
            EnumServicesStatusExW(
                scm.0,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                null_mut(),
                0,
                &mut need,
                &mut count,
                &mut resume,
                null(),
            )
        };
        let mut buf = vec![0u64; (need as usize).div_ceil(8) + 1024];
        resume = 0;
        // SAFETY: the buffer has `need` bytes at least (and room for a few more services).
        let ok = unsafe {
            EnumServicesStatusExW(
                scm.0,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32,
                SERVICE_STATE_ALL,
                buf.as_mut_ptr().cast(),
                (buf.len() * 8) as u32,
                &mut need,
                &mut count,
                &mut resume,
                null(),
            )
        };
        if ok == 0 {
            return Vec::new();
        }
        // SAFETY: `count` structures at the start of the buffer, their strings after them.
        let list = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const ENUM_SERVICE_STATUS_PROCESSW, count as usize) };
        let mut out = Vec::new();
        for e in list {
            // SAFETY: strings inside the buffer.
            let (name, display) = unsafe { (string(e.lpServiceName), string(e.lpDisplayName)) };
            let running = e.ServiceStatusProcess.dwCurrentState == SERVICE_RUNNING;
            let Ok(svc) = open(&scm, &name, SERVICE_QUERY_CONFIG) else { continue };
            let Some((start, command)) = config(&svc) else { continue };
            let file = rinst::inventory::command_file(&rt_sys::win::reg::expand(&command));
            out.push(Service { name, display, company: None, command, file, start, running, was: None });
        }
        out
    }

    /// Sets how a service starts.
    pub fn set_start(name: &str, start: Start) -> Result<(), String> {
        let scm = manager(SC_MANAGER_CONNECT)?;
        let svc = open(&scm, name, SERVICE_CHANGE_CONFIG | SERVICE_QUERY_CONFIG)?;
        let kind = match start {
            Start::Auto | Start::Delayed => SERVICE_AUTO_START,
            Start::Manual => SERVICE_DEMAND_START,
            Start::Disabled => SERVICE_DISABLED,
        };
        // SAFETY: valid handle; the rest stays as it is.
        let ok = unsafe {
            ChangeServiceConfigW(
                svc.0,
                SERVICE_NO_CHANGE,
                kind,
                SERVICE_NO_CHANGE,
                null(),
                null(),
                null_mut(),
                null(),
                null(),
                null(),
                null(),
            )
        };
        if ok == 0 {
            return Err(error());
        }
        if start.with_windows() {
            let d = SERVICE_DELAYED_AUTO_START_INFO { fDelayedAutostart: i32::from(start == Start::Delayed) };
            // SAFETY: the structure the level asks for; it outlives the call.
            unsafe {
                ChangeServiceConfig2W(svc.0, SERVICE_CONFIG_DELAYED_AUTO_START_INFO, (&d as *const SERVICE_DELAYED_AUTO_START_INFO).cast())
            };
        }
        Ok(())
    }
}

/// The services of other companies, with their company (the slow part: it
/// opens each program's version information).
#[cfg(windows)]
pub fn read(copy: &Copy) -> Vec<Service> {
    let windows_dir = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
    let all = win::all()
        .into_iter()
        .filter(|s| s.on() || copy.get(&s.name).is_some())
        .filter_map(|mut s| {
            s.company = s.file.as_deref().and_then(win::company);
            (!is_microsoft(s.company.as_deref(), s.file.as_deref(), &windows_dir)).then_some(s)
        })
        .collect();
    keep(all, copy)
}

#[cfg(not(windows))]
pub fn read(_copy: &Copy) -> Vec<Service> {
    Vec::new()
}

#[cfg(windows)]
pub fn set_start(name: &str, start: Start) -> Result<(), String> {
    win::set_start(name, start)
}

#[cfg(not(windows))]
pub fn set_start(_name: &str, _start: Start) -> Result<(), String> {
    Err("Windows only".into())
}

#[cfg(windows)]
pub fn company(file: &Path) -> Option<String> {
    win::company(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc(name: &str, start: Start) -> Service {
        Service {
            name: name.into(),
            display: name.to_uppercase(),
            company: Some("Acme".into()),
            command: format!("C:\\Acme\\{name}.exe"),
            file: None,
            start,
            running: false,
            was: None,
        }
    }

    #[test]
    fn whose_program() {
        let win = Path::new("C:\\Windows");
        assert!(is_microsoft(Some("Microsoft Corporation"), None, win));
        assert!(!is_microsoft(Some("Bitdefender"), Some(Path::new("C:\\Windows\\System32\\bd.exe")), win));
        // No company: by the folder.
        assert!(is_microsoft(None, Some(Path::new("c:\\windows\\system32\\svchost.exe")), win));
        assert!(is_microsoft(Some("  "), Some(Path::new("C:\\Windows")), win));
        assert!(!is_microsoft(None, Some(Path::new("C:\\WindowsApps\\x.exe")), win));
        assert!(!is_microsoft(None, Some(Path::new("C:\\Program Files\\X\\x.exe")), win));
        // Nothing to tell by: left out, as Windows' own.
        assert!(is_microsoft(None, None, win));
    }

    #[test]
    fn the_copy_round_trips() {
        let mut c = Copy::default();
        c.remember("AcmeUpdate", Start::Delayed);
        c.remember("Other", Start::Auto);
        c.remember("acmeupdate", Start::Manual);
        assert_eq!(c.get("ACMEUPDATE"), Some(Start::Delayed), "the first start is the one kept");
        let back = Copy::parse(&c.text());
        assert_eq!(back, c);
        assert_eq!(Copy::parse("bad line\nX\tnonsense\n\tauto\nY\tmanual\n").items, vec![("Y".to_string(), Start::Manual)]);
        c.forget("AcmeUpdate");
        assert_eq!(c.get("AcmeUpdate"), None);
        let dir = std::env::temp_dir().join(format!("rcare-services-{}", std::process::id()));
        c.save(&dir).unwrap();
        assert_eq!(Copy::load(&dir), c);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(Copy::load(&dir), Copy::default());
    }

    #[test]
    fn what_is_listed() {
        let mut copy = Copy::default();
        copy.remember("b", Start::Auto);
        copy.remember("d", Start::Delayed);
        let list = keep(
            vec![
                svc("c", Start::Delayed),
                svc("a", Start::Auto),
                svc("b", Start::Manual),
                svc("e", Start::Manual),
                svc("d", Start::Disabled),
            ],
            &copy,
        );
        let names: Vec<_> = list.iter().map(|s| (s.name.as_str(), s.was)).collect();
        // e was always manual (not ours to list); d was disabled since (by someone else).
        assert_eq!(names, vec![("a", None), ("b", Some(Start::Auto)), ("c", None)]);
        for s in [Start::Auto, Start::Delayed, Start::Manual, Start::Disabled] {
            assert_eq!(Start::from_word(s.word()), Some(s));
            assert!(!s.label().is_empty());
        }
    }

    /// The services of other companies on this PC.
    #[test]
    #[ignore]
    fn here() {
        for s in read(&Copy::default()) {
            println!("{:?} {:?} {:?} running={} {:?} {}", s.start, s.name, s.display, s.running, s.company, s.command);
        }
    }
}
