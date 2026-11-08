//! The drives and mounted file systems to offer on the start page, with their
//! size and free space.
//!
//! Reading them can be slow (a sleeping disk, a network drive that is not
//! connected), so the app asks for them on a background thread.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    /// Where to scan: `C:\` or a mount point.
    pub path: PathBuf,
    /// The volume label (Windows) or the device (Linux); may be empty.
    pub label: String,
    /// File system: NTFS, exFAT, ext4, btrfs...
    pub fs: String,
    pub total: u64,
    pub free: u64,
    /// A network drive or share.
    pub remote: bool,
}

impl Volume {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.free)
    }
}

/// The volumes there are now, in the system's order.
pub fn list() -> Vec<Volume> {
    imp::list()
}

#[cfg(windows)]
mod imp {
    use super::Volume;
    use rt_sys::win::{from_wide, wide};
    use std::path::PathBuf;
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW};
    use windows_sys::Win32::System::WindowsProgramming::{DRIVE_CDROM, DRIVE_NO_ROOT_DIR, DRIVE_REMOTE, DRIVE_REMOVABLE};

    pub fn list() -> Vec<Volume> {
        let mut buf = vec![0u16; 1024];
        // SAFETY: the buffer is as long as stated.
        let n = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
        if n == 0 || n > buf.len() {
            return Vec::new();
        }
        buf[..n].split(|&c| c == 0).filter(|r| !r.is_empty()).filter_map(|r| volume(&String::from_utf16_lossy(r))).collect()
    }

    fn volume(root: &str) -> Option<Volume> {
        let w = wide(root);
        // SAFETY: NUL-terminated string.
        let kind = unsafe { GetDriveTypeW(w.as_ptr()) };
        if kind == DRIVE_NO_ROOT_DIR {
            return None;
        }
        let mut label = vec![0u16; 261];
        let mut fs = vec![0u16; 261];
        // SAFETY: the buffers are as long as stated; the optional outputs are null.
        let ok = unsafe {
            GetVolumeInformationW(
                w.as_ptr(),
                label.as_mut_ptr(),
                label.len() as u32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                fs.as_mut_ptr(),
                fs.len() as u32,
            )
        } != 0;
        // An empty card reader or disc drive.
        if !ok && (kind == DRIVE_CDROM || kind == DRIVE_REMOVABLE) {
            return None;
        }
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        // SAFETY: the outputs are valid u64s.
        let sized = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) } != 0;
        if !sized && !ok {
            return None;
        }
        Some(Volume {
            path: PathBuf::from(root),
            label: from_wide(&label),
            fs: from_wide(&fs),
            total,
            free,
            remote: kind == DRIVE_REMOTE,
        })
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Volume;
    use std::collections::HashSet;
    use std::path::PathBuf;

    /// File systems that hold files people put there (not /proc, tmpfs...).
    const REAL: &[&str] = &[
        "ext2", "ext3", "ext4", "btrfs", "xfs", "zfs", "f2fs", "jfs", "reiserfs", "bcachefs", "vfat", "exfat", "ntfs", "ntfs3",
        "fuseblk", "hfsplus", "nfs", "nfs4", "cifs", "smb3", "9p", "drvfs", "virtiofs",
    ];
    const REMOTE: &[&str] = &["nfs", "nfs4", "cifs", "smb3"];

    pub fn list() -> Vec<Volume> {
        let text = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for line in text.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            let [dev, mnt, fs, ..] = f[..] else { continue };
            if !REAL.contains(&fs) {
                continue;
            }
            let path = PathBuf::from(unescape(mnt));
            // A btrfs or bind mount shows the same device more than once.
            if !seen.insert(dev.to_string()) {
                continue;
            }
            let Some((total, free)) = space(&path) else { continue };
            if total == 0 {
                continue;
            }
            out.push(Volume { path, label: unescape(dev), fs: fs.to_string(), total, free, remote: REMOTE.contains(&fs) });
        }
        out
    }

    /// Mount tables escape spaces and a few other characters as `\040`.
    pub fn unescape(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
                out.push((b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0'));
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn space(path: &std::path::Path) -> Option<(u64, u64)> {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: a NUL-terminated path and a zeroed struct that statvfs fills.
        let st = unsafe {
            let mut st: libc::statvfs = std::mem::zeroed();
            (libc::statvfs(c.as_ptr(), &mut st) == 0).then_some(st)?
        };
        let unit = st.f_frsize as u64;
        Some((st.f_blocks as u64 * unit, st.f_bavail as u64 * unit))
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn mount_names_are_unescaped() {
            assert_eq!(super::unescape("/media/my\\040disk"), "/media/my disk");
            assert_eq!(super::unescape("/plain"), "/plain");
            assert_eq!(super::unescape("/end\\04"), "/end\\04");
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_system_has_a_volume() {
        let v = super::list();
        assert!(!v.is_empty());
        assert!(v.iter().all(|v| v.free <= v.total));
    }
}
