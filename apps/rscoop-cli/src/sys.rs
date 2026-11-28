//! What needs the operating system: link kinds, junctions, hard-link identity,
//! %VARIABLE% expansion, the local time, the UI language.

use crate::paths::{dirname, join, normpath, strip_prefix};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkKind {
    Junction,
    Symlink,
    Dir,
    File,
}

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

/// What is at `p`, without following links; None when nothing is there.
pub fn link_kind(p: &str) -> Option<LinkKind> {
    let md = std::fs::symlink_metadata(p).ok()?;
    match reparse_tag(p, &md) {
        Some(IO_REPARSE_TAG_MOUNT_POINT) => return Some(LinkKind::Junction),
        Some(IO_REPARSE_TAG_SYMLINK) => return Some(LinkKind::Symlink),
        _ => {}
    }
    if md.file_type().is_symlink() {
        return Some(LinkKind::Symlink);
    }
    Some(if md.is_dir() { LinkKind::Dir } else { LinkKind::File })
}

#[cfg(windows)]
fn reparse_tag(p: &str, md: &std::fs::Metadata) -> Option<u32> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::{FindClose, FindFirstFileW, FILE_ATTRIBUTE_REPARSE_POINT, WIN32_FIND_DATAW};
    if md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return None;
    }
    let wide = wide(p);
    // SAFETY: a zeroed WIN32_FIND_DATAW is valid; the handle is closed below.
    unsafe {
        let mut data: WIN32_FIND_DATAW = std::mem::zeroed();
        let h = FindFirstFileW(wide.as_ptr(), &mut data);
        if h.is_null() || h as isize == -1 {
            return None;
        }
        FindClose(h);
        Some(data.dwReserved0)
    }
}

#[cfg(not(windows))]
fn reparse_tag(_p: &str, _md: &std::fs::Metadata) -> Option<u32> {
    None
}

/// Target of a junction or symbolic link, without the `\\?\` prefix.
pub fn read_link(p: &str) -> Option<String> {
    let target = std::fs::read_link(p).ok()?;
    let target = strip_prefix(&target.to_string_lossy());
    let absolute = target.starts_with(r"\\") || target.as_bytes().get(1) == Some(&b':') || target.starts_with('/');
    Some(if absolute { target } else { normpath(&join(&dirname(p), &target)) })
}

/// Do both paths name the same file (hard links)?
#[cfg(windows)]
pub fn same_file(a: &str, b: &str) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
    fn id(p: &str) -> Option<(u32, u32, u32)> {
        use std::os::windows::fs::OpenOptionsExt;
        let f = std::fs::OpenOptions::new().access_mode(0).custom_flags(0x0200_0000).open(p).ok()?;
        // SAFETY: the handle is valid for the life of `f`; the struct is plain data.
        unsafe {
            let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
            if GetFileInformationByHandle(f.as_raw_handle(), &mut info) == 0 {
                return None;
            }
            Some((info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow))
        }
    }
    matches!((id(a), id(b)), (Some(x), Some(y)) if x == y)
}

#[cfg(not(windows))]
pub fn same_file(a: &str, b: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

#[cfg(windows)]
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Expand `%VARIABLE%` references with the current process environment.
pub fn expand(value: &str) -> String {
    if !value.contains('%') {
        return value.to_string();
    }
    expand_env(value)
}

#[cfg(windows)]
fn expand_env(value: &str) -> String {
    use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
    let src = wide(value);
    let mut buf = vec![0u16; 1024];
    loop {
        // SAFETY: the buffer is as long as we say.
        let n = unsafe { ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) } as usize;
        if n == 0 {
            return value.to_string();
        }
        if n <= buf.len() {
            return String::from_utf16_lossy(&buf[..n - 1]);
        }
        buf.resize(n, 0);
    }
}

#[cfg(not(windows))]
fn expand_env(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(a) = rest.find('%') {
        out.push_str(&rest[..a]);
        let after = &rest[a + 1..];
        match after.find('%') {
            Some(b) => match std::env::var(&after[..b]) {
                Ok(v) => {
                    out.push_str(&v);
                    rest = &after[b + 1..];
                }
                Err(_) => {
                    out.push('%');
                    rest = after;
                }
            },
            None => {
                out.push_str(&rest[a..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Create a directory junction `link` -> `target` (no admin rights needed).
pub fn make_junction(link: &str, target: &str) -> std::io::Result<()> {
    rt_scoop::disk::make_junction(Path::new(link), Path::new(&strip_prefix(target)))
}

/// Remove a junction or symbolic link without touching what it points to.
pub fn remove_link(path: &str) -> std::io::Result<()> {
    if link_kind(path) == Some(LinkKind::Symlink) && !Path::new(path).is_dir() {
        std::fs::remove_file(path)
    } else {
        std::fs::remove_dir(path)
    }
}

/// Clear the read-only attribute; true when it was set.
pub fn make_writable(path: &str) -> bool {
    let Ok(md) = std::fs::symlink_metadata(path) else { return false };
    let mut perm = md.permissions();
    if !perm.readonly() {
        return false;
    }
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(path, perm).is_ok()
}

pub fn set_read_only(path: &str) {
    if let Ok(md) = std::fs::metadata(path) {
        let mut perm = md.permissions();
        perm.set_readonly(true);
        let _ = std::fs::set_permissions(path, perm);
    }
}

/// Local time as `YYYYMMDD-HHMMSS`.
#[cfg(windows)]
pub fn stamp() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: plain out-parameter.
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

#[cfg(not(windows))]
pub fn stamp() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = (s / 86400, s % 86400);
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// A moment in local time.
pub struct LocalTime {
    /// `2026-11-27 14:05:09`
    pub short: String,
    /// `2026-11-27T14:05:09+00:00`
    pub iso: String,
}

fn local(short: [u32; 6], offset_minutes: i64) -> LocalTime {
    let [y, mo, d, h, mi, s] = short;
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let off = offset_minutes.abs();
    LocalTime {
        short: format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}"),
        iso: format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}{sign}{:02}:{:02}", off / 60, off % 60),
    }
}

#[cfg(windows)]
pub fn local_time(time: std::time::SystemTime) -> LocalTime {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime};
    // 100 ns steps since 1601.
    let unix = time.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() / 100).unwrap_or(0) as u64;
    let ticks = unix + 116_444_736_000_000_000;
    let utc = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    // SAFETY: zeroed SYSTEMTIME and FILETIME are valid out-parameters.
    unsafe {
        let mut system = std::mem::zeroed();
        let mut shown = std::mem::zeroed();
        let mut back: FILETIME = std::mem::zeroed();
        FileTimeToSystemTime(&utc, &mut system);
        SystemTimeToTzSpecificLocalTime(std::ptr::null(), &system, &mut shown);
        SystemTimeToFileTime(&shown, &mut back);
        let local_ticks = (u64::from(back.dwHighDateTime) << 32) | u64::from(back.dwLowDateTime);
        // Rounded to minutes: the milliseconds are lost on the way.
        let offset = ((local_ticks as i64 - ticks as i64) as f64 / 600_000_000.0).round() as i64;
        let t = shown;
        local([t.wYear.into(), t.wMonth.into(), t.wDay.into(), t.wHour.into(), t.wMinute.into(), t.wSecond.into()], offset)
    }
}

#[cfg(not(windows))]
pub fn local_time(_time: std::time::SystemTime) -> LocalTime {
    local([0; 6], 0)
}

/// Today as `yyyyMMdd`, for nightly versions.
pub fn today() -> String {
    local_time(std::time::SystemTime::now()).short.chars().take(10).filter(char::is_ascii_digit).collect()
}

/// The architecture of the system, as `Get-DefaultArchitecture` sees it.
pub fn system_arch() -> rt_scoop::manifest::Arch {
    use rt_scoop::manifest::Arch;
    if std::env::var_os("ProgramFiles(Arm)").is_some() {
        Arch::Arm64
    } else if cfg!(target_pointer_width = "64") || std::env::var_os("PROCESSOR_ARCHITEW6432").is_some() {
        Arch::X64
    } else {
        Arch::X86
    }
}

/// Windows 11 runs x64 programs on ARM64; Windows 10 only x86 ones.
#[cfg(windows)]
pub fn windows_11() -> bool {
    use crate::registry::{Registry, WinRegistry};
    let key = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    WinRegistry.get("HKLM", key, "CurrentBuildNumber").and_then(|(v, _)| v.parse::<u32>().ok()).is_some_and(|build| build >= 22000)
}

#[cfg(not(windows))]
pub fn windows_11() -> bool {
    false
}

/// Seconds that local time is ahead of UTC now.
pub fn utc_offset() -> i64 {
    let now = std::time::SystemTime::now();
    let iso = local_time(now).iso;
    let seconds = now.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    rt_scoop::config::parse_time(&iso[..19.min(iso.len())], 0).map_or(0, |local| ((local - seconds) as f64 / 60.0).round() as i64 * 60)
}

/// Opens a web address in the browser.
#[cfg(windows)]
pub fn open_url(url: &str) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let (verb, file) = (wide("open"), wide(url));
    // SAFETY: both strings end in NUL and outlive the call.
    let result = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), 1) };
    result as usize > 32
}

#[cfg(not(windows))]
pub fn open_url(_url: &str) -> bool {
    false
}

#[cfg(windows)]
pub fn is_admin() -> bool {
    // SAFETY: no arguments.
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

#[cfg(not(windows))]
pub fn is_admin() -> bool {
    false
}

/// A number of `HKLM`.
#[cfg(windows)]
pub fn registry_dword(path: &str, name: &str) -> Option<u32> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
    let (path, name) = (wide(path), wide(name));
    let mut value = 0u32;
    let mut size = 4u32;
    // SAFETY: `value` has the four bytes asked for.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            path.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&mut value as *mut u32).cast(),
            &mut size,
        )
    };
    (status == 0).then_some(value)
}

#[cfg(not(windows))]
pub fn registry_dword(_path: &str, _name: &str) -> Option<u32> {
    None
}

/// The file system of the drive `path` is on ("NTFS").
#[cfg(windows)]
pub fn filesystem_name(path: &std::path::Path) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;
    let root: std::path::PathBuf = path.components().take(2).collect();
    let root = wide(&root.display().to_string());
    let mut name = [0u16; 32];
    // SAFETY: only the file system name is asked for, into its buffer.
    let ok = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    let end = name.iter().position(|&c| c == 0).unwrap_or(0);
    (ok != 0).then(|| String::from_utf16_lossy(&name[..end]))
}

#[cfg(not(windows))]
pub fn filesystem_name(_path: &std::path::Path) -> Option<String> {
    None
}

/// Tell running programs (Explorer) that the environment changed.
pub fn broadcast_environment_change() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
        let what = wide("Environment");
        let mut result = 0usize;
        // SAFETY: `what` outlives the call.
        unsafe {
            SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, what.as_ptr() as isize, SMTO_ABORTIFHUNG, 5000, &mut result);
        }
    }
}

/// Lets the console show ANSI colours (`ENABLE_VIRTUAL_TERMINAL_PROCESSING`;
/// Windows Terminal does it already, the old console needs it).
pub fn enable_colors() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::{
            GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        };
        for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: a standard handle and an out-parameter.
            unsafe {
                let handle = GetStdHandle(which);
                let mut mode = 0u32;
                if GetConsoleMode(handle, &mut mode) != 0 {
                    SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
                }
            }
        }
    }
}
