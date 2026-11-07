//! Folder listing on Windows.
//!
//! `GetFileInformationByHandleEx(FileIdBothDirectoryInfo)` fills a 64 KB
//! buffer with as many entries as fit, each with its length, its space on the
//! disk, its dates and, for a reparse point, its tag. That is one call per
//! few hundred files, with nothing opened per file.
//!
//! Paths get the `\\?\` prefix, so folders deeper than 260 characters work.
//! Shares whose server does not answer that query are listed with
//! `FindFirstFileW` (through `std::fs::read_dir`) instead.

use super::{Ctx, Raw};
use crate::model::{Entry, Kind};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_LEVEL, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, ERROR_NO_MORE_FILES, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_BOTH_DIR_INFO, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

/// Reparse tags of links: not followed.
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

/// 100 ns intervals from 1601 to 1970.
const EPOCH_DIFF: i64 = 11_644_473_600;

/// The path with the `\\?\` prefix, NUL-terminated.
pub(crate) fn long_path(path: &Path) -> Vec<u16> {
    let s = path.as_os_str();
    let text = s.to_string_lossy();
    let mut out: Vec<u16> = Vec::with_capacity(text.len() + 8);
    if text.starts_with("\\\\?\\") || text.starts_with("\\\\.\\") {
        out.extend(s.encode_wide());
    } else if let Some(unc) = text.strip_prefix("\\\\") {
        out.extend("\\\\?\\UNC\\".encode_utf16());
        out.extend(unc.encode_utf16());
    } else {
        out.extend("\\\\?\\".encode_utf16());
        out.extend(s.encode_wide());
    }
    out.push(0);
    out
}

struct Owned(HANDLE);

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: a handle this module opened and nothing else closes.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn filetime_secs(t: i64) -> i64 {
    t / 10_000_000 - EPOCH_DIFF
}

pub(crate) fn list(path: &Path, _ctx: Ctx) -> std::io::Result<Vec<Raw>> {
    let wide = long_path(path);
    // SAFETY: `wide` is NUL-terminated and outlives the call; the handle is
    // closed by `Owned`.
    let h = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    let h = Owned(h);
    // u64 elements: the entries need 8-byte alignment.
    let mut buf = vec![0u64; 64 * 1024 / 8];
    let bytes = buf.len() * 8;
    let mut out = Vec::new();
    let mut class = FileIdBothDirectoryRestartInfo;
    loop {
        // SAFETY: the buffer is `bytes` long and writable.
        let ok = unsafe { GetFileInformationByHandleEx(h.0, class, buf.as_mut_ptr().cast(), bytes as u32) };
        if ok == 0 {
            let e = std::io::Error::last_os_error();
            let code = e.raw_os_error().unwrap_or(0) as u32;
            if code == ERROR_NO_MORE_FILES {
                break;
            }
            if out.is_empty() && [ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED, ERROR_INVALID_LEVEL].contains(&code) {
                return list_std(path);
            }
            return Err(e);
        }
        class = FileIdBothDirectoryInfo;
        parse(&buf, bytes, path, &mut out);
    }
    Ok(out)
}

/// Reads the entries in a filled buffer.
fn parse(buf: &[u64], bytes: usize, dir: &Path, out: &mut Vec<Raw>) {
    let base = buf.as_ptr().cast::<u8>();
    let name_off = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
    let mut off = 0usize;
    loop {
        if off + name_off > bytes {
            break;
        }
        // SAFETY: the header lies inside the buffer (checked above).
        let info: FILE_ID_BOTH_DIR_INFO = unsafe { std::ptr::read_unaligned(base.add(off).cast()) };
        let len = info.FileNameLength as usize / 2;
        if off + name_off + len * 2 > bytes {
            break;
        }
        // SAFETY: the name lies inside the buffer (checked above); entries
        // start at 8-byte boundaries, so the u16s are aligned.
        let name16 = unsafe { std::slice::from_raw_parts(base.add(off + name_off).cast::<u16>(), len) };
        let skip = name16 == [b'.' as u16] || name16 == [b'.' as u16, b'.' as u16];
        if !skip {
            out.push(raw(&info, String::from_utf16_lossy(name16), dir));
        }
        if info.NextEntryOffset == 0 {
            break;
        }
        off += info.NextEntryOffset as usize;
    }
}

fn raw(info: &FILE_ID_BOTH_DIR_INFO, name: String, dir: &Path) -> Raw {
    let attrs = info.FileAttributes;
    let is_dir = attrs & FILE_ATTRIBUTE_DIRECTORY != 0;
    // For a reparse point, EaSize holds the reparse tag.
    let link = attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 && [IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK].contains(&info.EaSize);
    let kind = match (is_dir, link) {
        (_, true) => Kind::Link,
        (true, false) => Kind::Dir,
        (false, false) => Kind::File,
    };
    let (size, alloc) = if kind == Kind::File { (info.EndOfFile.max(0) as u64, info.AllocationSize.max(0) as u64) } else { (0, 0) };
    let descend = (kind == Kind::Dir).then(|| dir.join(&name));
    Raw { entry: Entry { name, kind, size, alloc, mtime: filetime_secs(info.LastWriteTime), flags: 0 }, descend, link_key: None }
}

/// The slower way, for shares that do not support the batch query: the
/// space on the disk is then taken to be the length.
fn list_std(path: &Path) -> std::io::Result<Vec<Raw>> {
    use std::os::windows::fs::MetadataExt;
    let mut out = Vec::new();
    for de in std::fs::read_dir(path)? {
        let Ok(de) = de else { continue };
        let Ok(m) = de.metadata() else { continue };
        let name = de.file_name().to_string_lossy().into_owned();
        let attrs = m.file_attributes();
        let kind = if m.file_type().is_symlink() || attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 && m.is_dir() {
            Kind::Link
        } else if m.is_dir() {
            Kind::Dir
        } else {
            Kind::File
        };
        let size = if kind == Kind::File { m.len() } else { 0 };
        let descend: Option<PathBuf> = (kind == Kind::Dir).then(|| de.path());
        let mtime = super::mtime_of(&m);
        out.push(Raw { entry: Entry { name, kind, size, alloc: size, mtime, flags: 0 }, descend, link_key: None });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(w: &[u16]) -> String {
        String::from_utf16_lossy(&w[..w.len() - 1])
    }

    #[test]
    fn long_paths_get_the_prefix() {
        assert_eq!(text(&long_path(Path::new("C:\\Windows"))), "\\\\?\\C:\\Windows");
        assert_eq!(text(&long_path(Path::new("\\\\server\\share\\x"))), "\\\\?\\UNC\\server\\share\\x");
        assert_eq!(text(&long_path(Path::new("\\\\?\\D:\\a"))), "\\\\?\\D:\\a");
    }

    #[test]
    fn junctions_are_not_followed() {
        let tt = super::super::tests::TempTree::new("junction");
        tt.file("real/x.bin", 1000);
        let target = tt.0.join("real");
        let link = tt.0.join("jn");
        let ok = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            return;
        }
        let s = super::super::tests::sys(&tt.0, &std::sync::atomic::AtomicBool::new(false), &|_| {}).unwrap();
        let t = &s.tree;
        assert_eq!(t.node(0).size, 1000);
        let jn = *t.children(0).iter().find(|&&c| t.name(c) == "jn").unwrap();
        assert_eq!(t.node(jn).kind, Kind::Link);
    }

    #[test]
    fn dates_convert_to_unix_time() {
        // 2020-01-01 00:00:00 UTC.
        assert_eq!(filetime_secs(132_223_104_000_000_000), 1_577_836_800);
    }
}
