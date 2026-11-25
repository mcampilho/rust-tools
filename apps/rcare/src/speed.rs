//! What the Speed Up page reads and does, through Windows itself:
//!
//! - the memory (`GlobalMemoryStatusEx`) and, as administrator, the standby
//!   list (`NtQuerySystemInformation`, `SystemMemoryListInformation`), which
//!   can be freed on request (`MemoryPurgeStandbyList`). The programs' own
//!   memory is never emptied: that usually makes them slower.
//! - the processor's use, from two samples of `GetSystemTimes`.
//! - the programs open (top-level windows the user sees) with their memory;
//!   closing one asks it politely (`WM_CLOSE`, as its X button), never forces.
//! - Windows' Game Mode switch (`HKCU\Software\Microsoft\GameBar`).

/// The memory of the computer, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    pub total: u64,
    /// Free plus standby: what programs can have at once.
    pub available: u64,
    /// The standby list (cached pages that can be given away), when it could be read (administrator).
    pub standby: Option<u64>,
}

impl Memory {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }

    /// The share in use, 0–100.
    pub fn load(&self) -> u32 {
        if self.total == 0 {
            0
        } else {
            (self.used() as f64 * 100.0 / self.total as f64).round() as u32
        }
    }
}

/// Times of the processor since boot (100 ns), for the use between two samples.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub idle: u64,
    /// Kernel time includes the idle time, as Windows counts it.
    pub kernel: u64,
    pub user: u64,
}

/// The processor's use between two samples, 0–100.
pub fn cpu_percent(a: CpuTimes, b: CpuTimes) -> f32 {
    let idle = b.idle.saturating_sub(a.idle) as f64;
    let total = (b.kernel.saturating_sub(a.kernel) + b.user.saturating_sub(a.user)) as f64;
    if total <= 0.0 {
        0.0
    } else {
        (((total - idle) / total) * 100.0).clamp(0.0, 100.0) as f32
    }
}

/// A program open: its process, the title of its main window, its memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenApp {
    pub pid: u32,
    /// The file name of the program (`notepad.exe`).
    pub exe: String,
    pub title: String,
    /// The memory it uses (working set), in bytes.
    pub memory: u64,
    /// Its top-level windows (as numbers, to ask them to close).
    pub windows: Vec<isize>,
}

/// Programs never listed: Windows' own shell and this program.
pub fn kept_out(exe: &str, pid: u32, own_pid: u32) -> bool {
    pid == own_pid
        || [
            "explorer.exe",
            "textinputhost.exe",
            "shellexperiencehost.exe",
            "searchhost.exe",
            "startmenuexperiencehost.exe",
            "applicationframehost.exe",
        ]
        .iter()
        .any(|k| exe.eq_ignore_ascii_case(k))
}

/// Windows found (pid, title, handle) grouped by program, the first title kept, sorted by memory.
pub fn group(windows: Vec<(u32, String, isize)>, info: &dyn Fn(u32) -> Option<(String, u64)>, own_pid: u32) -> Vec<OpenApp> {
    let mut apps: Vec<OpenApp> = Vec::new();
    for (pid, title, hwnd) in windows {
        if let Some(a) = apps.iter_mut().find(|a| a.pid == pid) {
            a.windows.push(hwnd);
            continue;
        }
        let Some((exe, memory)) = info(pid) else { continue };
        if kept_out(&exe, pid, own_pid) {
            continue;
        }
        apps.push(OpenApp { pid, exe, title, memory, windows: vec![hwnd] });
    }
    apps.sort_by(|a, b| b.memory.cmp(&a.memory).then(a.title.cmp(&b.title)));
    apps
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HWND, LPARAM, LUID};
    use windows_sys::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
    };
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetSystemTimes, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowLongW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
        PostMessageW, GWL_EXSTYLE, GW_OWNER, WM_CLOSE, WS_EX_TOOLWINDOW,
    };

    #[link(name = "ntdll")]
    extern "system" {
        fn NtQuerySystemInformation(class: u32, info: *mut core::ffi::c_void, len: u32, ret: *mut u32) -> i32;
        fn NtSetSystemInformation(class: u32, info: *const core::ffi::c_void, len: u32) -> i32;
    }

    /// `SystemMemoryListInformation`, and the command that empties the standby list.
    const MEMORY_LIST: u32 = 80;
    const PURGE_STANDBY: u32 = 4;

    /// `SYSTEM_MEMORY_LIST_INFORMATION`: page counts.
    #[repr(C)]
    #[derive(Default)]
    struct MemoryList {
        zero: usize,
        free: usize,
        modified: usize,
        modified_no_write: usize,
        bad: usize,
        standby_by_priority: [usize; 8],
        repurposed_by_priority: [usize; 8],
        modified_page_file: usize,
    }

    fn standby() -> Option<u64> {
        let mut m = MemoryList::default();
        // SAFETY: the buffer is the structure Windows fills, with its size.
        let rc = unsafe {
            NtQuerySystemInformation(
                MEMORY_LIST,
                (&mut m as *mut MemoryList).cast(),
                std::mem::size_of::<MemoryList>() as u32,
                std::ptr::null_mut(),
            )
        };
        (rc == 0).then(|| m.standby_by_priority.iter().map(|&p| p as u64).sum::<u64>() * 4096)
    }

    pub fn memory() -> Option<Memory> {
        let mut s = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..unsafe { std::mem::zeroed() } };
        // SAFETY: `dwLength` says the size of the structure Windows fills.
        if unsafe { GlobalMemoryStatusEx(&mut s) } == 0 {
            return None;
        }
        Some(Memory { total: s.ullTotalPhys, available: s.ullAvailPhys, standby: standby() })
    }

    fn enable_privilege(name: &str) -> Result<(), String> {
        let w: Vec<u16> = name.encode_utf16().chain([0]).collect();
        let mut token = std::ptr::null_mut();
        // SAFETY: the current process' token, opened to adjust its privileges, closed below.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &mut token) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let mut luid = LUID { LowPart: 0, HighPart: 0 };
        // SAFETY: a NUL-terminated name and an output LUID.
        let found = unsafe { LookupPrivilegeValueW(std::ptr::null(), w.as_ptr(), &mut luid) } != 0;
        let mut tp: TOKEN_PRIVILEGES = unsafe { std::mem::zeroed() };
        tp.PrivilegeCount = 1;
        tp.Privileges[0].Luid = luid;
        tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
        // SAFETY: one privilege described in `tp`; the previous state is not wanted.
        let ok = found && unsafe { AdjustTokenPrivileges(token, 0, &tp, 0, std::ptr::null_mut(), std::ptr::null_mut()) } != 0;
        let last = std::io::Error::last_os_error();
        // SAFETY: the token opened above.
        unsafe { CloseHandle(token) };
        // AdjustTokenPrivileges succeeds without the privilege, and says so in the last error (1300).
        if ok && last.raw_os_error() != Some(1300) {
            Ok(())
        } else {
            Err(last.to_string())
        }
    }

    pub fn purge_standby() -> Result<(), String> {
        enable_privilege("SeProfileSingleProcessPrivilege")?;
        let cmd = PURGE_STANDBY;
        // SAFETY: the command is a 4-byte number, as Windows expects for this class.
        let rc = unsafe { NtSetSystemInformation(MEMORY_LIST, (&cmd as *const u32).cast(), 4) };
        if rc == 0 {
            Ok(())
        } else {
            Err(format!("NTSTATUS 0x{rc:08X}"))
        }
    }

    fn ft(f: FILETIME) -> u64 {
        (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
    }

    pub fn cpu_times() -> Option<CpuTimes> {
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut i, mut k, mut u) = (zero, zero, zero);
        // SAFETY: three output FILETIMEs.
        (unsafe { GetSystemTimes(&mut i, &mut k, &mut u) } != 0).then(|| CpuTimes { idle: ft(i), kernel: ft(k), user: ft(u) })
    }

    /// The program's file name and memory, when it can be asked.
    fn process_info(pid: u32) -> Option<(String, u64)> {
        // SAFETY: a handle with the least rights, closed below.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        // SAFETY: `len` is the buffer's size in characters.
        let name = (unsafe { QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) } != 0).then(|| {
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            full.rsplit('\\').next().unwrap_or(&full).to_string()
        });
        let mut pmc: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        // SAFETY: `cb` says the size of the structure Windows fills.
        let mem = if unsafe { K32GetProcessMemoryInfo(h, &mut pmc, pmc.cb) } != 0 { pmc.WorkingSetSize as u64 } else { 0 };
        // SAFETY: the handle opened above.
        unsafe { CloseHandle(h) };
        name.map(|n| (n, mem))
    }

    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the vector `open_apps` passes, alive for the call.
        let out = unsafe { &mut *(data as *mut Vec<(u32, String, isize)>) };
        // SAFETY: `hwnd` is a window Windows gives this callback.
        unsafe {
            if IsWindowVisible(hwnd) == 0
                || !GetWindow(hwnd, GW_OWNER).is_null()
                || (GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW) != 0
            {
                return 1;
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return 1;
            }
            let mut buf = vec![0u16; len as usize + 1];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            out.push((pid, String::from_utf16_lossy(&buf[..n.max(0) as usize]), hwnd as isize));
        }
        1
    }

    pub fn open_apps() -> Vec<OpenApp> {
        let mut found: Vec<(u32, String, isize)> = Vec::new();
        // SAFETY: the callback only uses `found` during the call.
        unsafe { EnumWindows(Some(each), &mut found as *mut _ as LPARAM) };
        // SAFETY: no arguments.
        let own = unsafe { GetCurrentProcessId() };
        group(found, &process_info, own)
    }

    pub fn close(app: &OpenApp) {
        for &w in &app.windows {
            // SAFETY: asking a window to close; a window that is gone ignores it.
            unsafe { PostMessageW(w as HWND, WM_CLOSE, 0, 0) };
        }
    }

    const GAME_BAR: &str = "Software\\Microsoft\\GameBar";

    pub fn game_mode() -> bool {
        use rt_sys::win::reg::{Key, Root, View};
        // Not set: Windows has it on.
        Key::open(Root::CurrentUser, GAME_BAR, View::Bits64).and_then(|k| k.number("AutoGameModeEnabled")).is_none_or(|v| v != 0)
    }

    pub fn set_game_mode(on: bool) -> Result<(), String> {
        use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD};
        let key: Vec<u16> = GAME_BAR.encode_utf16().chain([0]).collect();
        let name: Vec<u16> = "AutoGameModeEnabled".encode_utf16().chain([0]).collect();
        let v: u32 = u32::from(on);
        // SAFETY: NUL-terminated names; the data is a 4-byte number.
        let rc = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_DWORD, (&v as *const u32).cast(), 4) };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(rc as i32).to_string())
        }
    }
}

#[cfg(windows)]
pub use imp::{close, cpu_times, game_mode, memory, open_apps, purge_standby, set_game_mode};

#[cfg(not(windows))]
mod other {
    use super::*;
    pub fn memory() -> Option<Memory> {
        None
    }
    pub fn purge_standby() -> Result<(), String> {
        Err("Windows only".into())
    }
    pub fn cpu_times() -> Option<CpuTimes> {
        None
    }
    pub fn open_apps() -> Vec<OpenApp> {
        Vec::new()
    }
    pub fn close(_app: &OpenApp) {}
    pub fn game_mode() -> bool {
        false
    }
    pub fn set_game_mode(_on: bool) -> Result<(), String> {
        Err("Windows only".into())
    }
}

#[cfg(not(windows))]
pub use other::{close, cpu_times, game_mode, memory, open_apps, purge_standby, set_game_mode};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processor_use() {
        let a = CpuTimes { idle: 100, kernel: 200, user: 100 };
        let b = CpuTimes { idle: 175, kernel: 300, user: 100 };
        // 100 of time, 75 idle: 25 %.
        assert!((cpu_percent(a, b) - 25.0).abs() < 0.01);
        assert_eq!(cpu_percent(a, a), 0.0);
        let m = Memory { total: 16 << 30, available: 4 << 30, standby: None };
        assert_eq!((m.used(), m.load()), (12 << 30, 75));
        assert_eq!(Memory::default().load(), 0);
    }

    #[test]
    fn programs_are_grouped_and_kept_out() {
        let info = |pid: u32| match pid {
            1 => Some(("notepad.exe".to_string(), 10 << 20)),
            2 => Some(("Explorer.EXE".to_string(), 90 << 20)),
            3 => Some(("chrome.exe".to_string(), 500 << 20)),
            7 => Some(("rcare.exe".to_string(), 30 << 20)),
            _ => None,
        };
        let w = vec![
            (1, "a.txt - Notepad".into(), 11),
            (2, "Downloads".into(), 22),
            (3, "Mail".into(), 33),
            (1, "b.txt".into(), 12),
            (9, "gone".into(), 99),
            (7, "rcare".into(), 77),
        ];
        let apps = group(w, &info, 7);
        assert_eq!(apps.iter().map(|a| a.exe.as_str()).collect::<Vec<_>>(), ["chrome.exe", "notepad.exe"]);
        assert_eq!(apps[1].windows, vec![11, 12]);
        assert_eq!(apps[1].title, "a.txt - Notepad");
    }

    /// What this PC has open, its memory and processor now.
    #[test]
    #[ignore]
    fn here() {
        println!("{:?}", memory());
        let a = cpu_times().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        println!("cpu {:.1} %", cpu_percent(a, cpu_times().unwrap()));
        println!("game mode {}", game_mode());
        for app in open_apps() {
            println!("{} {} MB {:?}", app.exe, app.memory >> 20, app.title);
        }
    }
}
