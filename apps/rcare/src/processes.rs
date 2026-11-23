//! Processes and Auto Shutdown (the user's choice, 2026-11-23, from what
//! was left for later).
//!
//! - **Processes**: every process (`CreateToolhelp32Snapshot`), with its
//!   program, memory (working set) and processor time; the page takes two
//!   readings to show the processor use. A process can be ended
//!   (`TerminateProcess`) after a question, except the ones Windows cannot
//!   live without (ending them stops the computer) and rcare itself.
//! - **Auto Shutdown**: Windows' own `shutdown.exe /s` or `/r` with a delay,
//!   so it happens even with rcare closed, and Windows says it is coming;
//!   `shutdown.exe /a` calls it off. What was asked is kept in a small file so
//!   rcare shows it when opened again.

use std::path::{Path, PathBuf};

/// One process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    pub name: String,
    /// Its program, when it can be asked (not for the system's own, without administrator rights).
    pub path: Option<PathBuf>,
    /// Working set, bytes.
    pub memory: u64,
    /// Processor time used so far (kernel and user), in 100 ns.
    pub cpu_time: u64,
    /// When it started (a FILETIME): with the pid, which process it is across readings.
    pub created: u64,
}

/// The processes Windows cannot live without: ending one stops the computer (or the session).
const VITAL: [&str; 11] = [
    "system",
    "registry",
    "memory compression",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "fontdrvhost.exe",
];

/// Whether a process may not be ended from here: Windows' vital ones, the idle and system processes, rcare itself.
pub fn protected(name: &str, pid: u32, own_pid: u32) -> bool {
    pid <= 4 || pid == own_pid || VITAL.contains(&name.to_lowercase().as_str())
}

/// The processor use of a process between two readings, in percent of the whole computer (0–100).
pub fn cpu_percent(before: u64, after: u64, wall_100ns: u64, cpus: u32) -> f32 {
    if wall_100ns == 0 || cpus == 0 {
        return 0.0;
    }
    let used = after.saturating_sub(before) as f64;
    ((used / (wall_100ns as f64 * f64::from(cpus))) * 100.0).clamp(0.0, 100.0) as f32
}

/// How many logical processors there are.
pub fn cpus() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
}

#[cfg(windows)]
mod win {
    use super::Proc;
    use std::path::PathBuf;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    fn ft(f: FILETIME) -> u64 {
        (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
    }

    /// Program, memory, processor time and start of a process, as far as it can be asked.
    fn details(pid: u32) -> (Option<PathBuf>, u64, u64, u64) {
        // SAFETY: a handle with the least rights, closed below.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return (None, 0, 0, 0);
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        // SAFETY: `len` is the buffer's size in characters.
        let path = (unsafe { QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) } != 0)
            .then(|| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])));
        let mut pmc: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        // SAFETY: `cb` says the size of the structure Windows fills.
        let mem = if unsafe { K32GetProcessMemoryInfo(h, &mut pmc, pmc.cb) } != 0 { pmc.WorkingSetSize as u64 } else { 0 };
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut c, mut e, mut k, mut u) = (zero, zero, zero, zero);
        // SAFETY: four output FILETIMEs.
        let (cpu, created) =
            if unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) } != 0 { (ft(k) + ft(u), ft(c)) } else { (0, 0) };
        // SAFETY: the handle opened above.
        unsafe { CloseHandle(h) };
        (path, mem, cpu, created)
    }

    pub fn read() -> Vec<Proc> {
        let mut out = Vec::new();
        // SAFETY: a snapshot of the processes, closed below.
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut e: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        // SAFETY: `dwSize` says the size of the structure; the snapshot is valid.
        let mut more = unsafe { Process32FirstW(snap, &mut e) } != 0;
        while more {
            let n = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
            let name = String::from_utf16_lossy(&e.szExeFile[..n]);
            let pid = e.th32ProcessID;
            let (path, memory, cpu_time, created) = if pid == 0 { (None, 0, 0, 0) } else { details(pid) };
            out.push(Proc { pid, name, path, memory, cpu_time, created });
            // SAFETY: as above.
            more = unsafe { Process32NextW(snap, &mut e) } != 0;
        }
        // SAFETY: the snapshot opened above.
        unsafe { CloseHandle(snap) };
        out
    }

    pub fn end(pid: u32) -> Result<(), String> {
        // SAFETY: a handle with the right to end the process, closed below.
        let h = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
        if h.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: the handle opened above.
        let ok = unsafe { TerminateProcess(h, 1) } != 0;
        let err = std::io::Error::last_os_error();
        // SAFETY: the handle opened above.
        unsafe { CloseHandle(h) };
        if ok {
            Ok(())
        } else {
            Err(err.to_string())
        }
    }
}

/// Every process (the idle one included, without details).
#[cfg(windows)]
pub fn read() -> Vec<Proc> {
    win::read()
}

#[cfg(not(windows))]
pub fn read() -> Vec<Proc> {
    Vec::new()
}

/// Ends a process. Err: why not.
#[cfg(windows)]
pub fn end(pid: u32) -> Result<(), String> {
    win::end(pid)
}

#[cfg(not(windows))]
pub fn end(_pid: u32) -> Result<(), String> {
    Err("Windows only".into())
}

// ---------------------------------------------------------------- Auto Shutdown

/// What Auto Shutdown does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    Shutdown,
    Restart,
}

impl Power {
    pub fn word(self) -> &'static str {
        match self {
            Power::Shutdown => "shutdown",
            Power::Restart => "restart",
        }
    }

    pub fn from_word(w: &str) -> Option<Power> {
        match w {
            "shutdown" => Some(Power::Shutdown),
            "restart" => Some(Power::Restart),
            _ => None,
        }
    }

    /// What the page calls it (a translation key).
    pub fn label(self) -> &'static str {
        match self {
            Power::Shutdown => "Shut Down",
            Power::Restart => "Restart",
        }
    }
}

/// The longest delay `shutdown.exe` accepts: 10 years, in seconds.
pub const MAX_DELAY: u64 = 315_360_000;

/// The arguments of `shutdown.exe` for `p` in `secs` seconds.
pub fn shutdown_args(p: Power, secs: u64) -> Vec<String> {
    let flag = match p {
        Power::Shutdown => "/s",
        Power::Restart => "/r",
    };
    vec![flag.to_string(), "/t".to_string(), secs.min(MAX_DELAY).to_string()]
}

/// Seconds from `now` (hour, minute, second) to the next `hour:minute` (tomorrow when it has passed; never 0).
pub fn seconds_until(hour: u32, minute: u32, now: (u32, u32, u32)) -> u64 {
    let target = u64::from(hour) * 3600 + u64::from(minute) * 60;
    let cur = u64::from(now.0) * 3600 + u64::from(now.1) * 60 + u64::from(now.2);
    if target > cur {
        target - cur
    } else {
        target + 86_400 - cur
    }
}

/// Seconds since 1970 now.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// What is planned: what, and when (seconds since 1970).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Planned {
    pub power: Power,
    pub at: u64,
}

/// The file that keeps what was planned: `<word> <unix seconds>`.
pub fn planned_text(p: &Planned) -> String {
    format!("{} {}\n", p.power.word(), p.at)
}

/// What the file says, if it is still to come.
pub fn parse_planned(text: &str, now: u64) -> Option<Planned> {
    let mut it = text.split_whitespace();
    let power = Power::from_word(it.next()?)?;
    let at: u64 = it.next()?.parse().ok()?;
    (at > now).then_some(Planned { power, at })
}

/// Asks Windows to shut down or restart in `secs` seconds. Err: what `shutdown.exe` said.
pub fn plan(p: Power, secs: u64, file: &Path) -> Result<Planned, String> {
    let args = shutdown_args(p, secs);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match crate::ps::command("shutdown.exe", &args) {
        Some((true, _)) => {
            let planned = Planned { power: p, at: unix_now() + secs };
            let _ = std::fs::write(file, planned_text(&planned));
            Ok(planned)
        }
        Some((false, text)) => Err(crate::ps::last_line(&text)),
        None => Err("shutdown.exe".into()),
    }
}

/// Calls off what was planned (`shutdown.exe /a`); the file goes. Err: what `shutdown.exe` said
/// (also when nothing was planned any more).
pub fn cancel(file: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(file);
    match crate::ps::command("shutdown.exe", &["/a"]) {
        Some((true, _)) => Ok(()),
        Some((false, text)) => Err(crate::ps::last_line(&text)),
        None => Err("shutdown.exe".into()),
    }
}

/// What the file in `file` says is still to come.
pub fn load_planned(file: &Path) -> Option<Planned> {
    parse_planned(&std::fs::read_to_string(file).ok()?, unix_now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vital_ones_are_kept() {
        assert!(protected("csrss.exe", 600, 1));
        assert!(protected("LSASS.EXE", 700, 1));
        assert!(protected("System", 4, 1));
        assert!(protected("[System Process]", 0, 1));
        assert!(protected("rcare.exe", 1234, 1234));
        assert!(!protected("notepad.exe", 1234, 1));
        assert!(!protected("explorer.exe", 800, 1));
    }

    #[test]
    fn processor_use() {
        // 1 s of one processor's time over 1 s, with 4 processors: a quarter.
        assert_eq!(cpu_percent(0, 10_000_000, 10_000_000, 4), 25.0);
        assert_eq!(cpu_percent(5, 5, 10_000_000, 4), 0.0);
        assert_eq!(cpu_percent(10, 5, 10_000_000, 4), 0.0, "a time that went back is nothing");
        assert_eq!(cpu_percent(0, 100, 0, 4), 0.0);
        assert_eq!(cpu_percent(0, u64::MAX, 1, 1), 100.0);
        assert!(cpus() >= 1);
    }

    #[test]
    fn shutdown_commands() {
        assert_eq!(shutdown_args(Power::Shutdown, 600), ["/s", "/t", "600"]);
        assert_eq!(shutdown_args(Power::Restart, u64::MAX), ["/r", "/t", "315360000"]);
        assert_eq!(seconds_until(23, 0, (22, 0, 0)), 3600);
        assert_eq!(seconds_until(1, 30, (22, 0, 30)), 3 * 3600 + 30 * 60 - 30);
        assert_eq!(seconds_until(22, 0, (22, 0, 0)), 86_400, "now is tomorrow's");
        let p = Planned { power: Power::Restart, at: 2_000 };
        assert_eq!(parse_planned(&planned_text(&p), 1_000), Some(p));
        assert_eq!(parse_planned(&planned_text(&p), 2_000), None, "already passed");
        assert_eq!(parse_planned("sleep 5000", 1), None);
        assert_eq!(parse_planned("", 1), None);
        for w in [Power::Shutdown, Power::Restart] {
            assert_eq!(Power::from_word(w.word()), Some(w));
        }
    }

    /// The processes of this PC (this test's own among them, with its program and memory).
    #[cfg(windows)]
    #[test]
    fn reads_the_processes() {
        let list = read();
        let me = list.iter().find(|p| p.pid == std::process::id()).expect("this process is listed");
        assert!(me.path.as_ref().is_some_and(|p| p.is_file()), "{me:?}");
        assert!(me.memory > 0 && me.created > 0, "{me:?}");
        assert!(list.len() > 10);
    }
}
