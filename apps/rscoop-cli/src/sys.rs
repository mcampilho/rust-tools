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
#[cfg(windows)]
pub fn make_junction(link: &str, target: &str) -> std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::IO::DeviceIoControl;
    const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
    let target = normpath(&strip_prefix(target));
    std::fs::create_dir(link)?;
    let sub: Vec<u16> = format!(r"\??\{target}").encode_utf16().collect();
    let print: Vec<u16> = target.encode_utf16().collect();
    let mut buf: Vec<u8> = Vec::new();
    let path_bytes = (sub.len() + 1 + print.len() + 1) * 2;
    buf.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    buf.extend_from_slice(&((8 + path_bytes) as u16).to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&((sub.len() * 2) as u16).to_le_bytes());
    buf.extend_from_slice(&((sub.len() * 2 + 2) as u16).to_le_bytes());
    buf.extend_from_slice(&((print.len() * 2) as u16).to_le_bytes());
    for u in sub.iter().chain(&[0]).chain(print.iter()).chain(&[0]) {
        buf.extend_from_slice(&u.to_le_bytes());
    }
    let result = (|| {
        // GENERIC_WRITE, backup semantics + open reparse point.
        let f = std::fs::OpenOptions::new().access_mode(0x4000_0000).custom_flags(0x0200_0000 | 0x0020_0000).open(link)?;
        let mut returned = 0u32;
        // SAFETY: the buffer is a valid REPARSE_DATA_BUFFER of the given length.
        let ok = unsafe {
            DeviceIoControl(
                f.as_raw_handle(),
                FSCTL_SET_REPARSE_POINT,
                buf.as_ptr().cast(),
                buf.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir(link);
    }
    result
}

#[cfg(not(windows))]
pub fn make_junction(link: &str, target: &str) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
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
