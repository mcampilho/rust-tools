//! What kind of drive a path is on, and how many threads suit it.

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Media {
    /// Solid state, inside the computer.
    Ssd,
    /// A turning disk inside the computer.
    Hdd,
    /// On USB (a disk, a stick, a card), whatever is inside.
    Usb,
    Network,
    Unknown,
}

/// The threads for deleting or moving on `media`, measured on 2026-11-25 with
/// thousands of small files (`bench` in `tests.rs`), seconds to delete / to
/// copy, with 1 thread and with the number chosen:
///
/// - NVMe SSD (3000 files): 7.8 / 6.1 → 6.4 / 1.4 with 8; 16 adds little.
/// - Internal hard disk (3000 files): 8.7 / 7.5 → 7.3 / 5.1 with 2; more is
///   no faster at deleting, and large files would be read with the head
///   jumping between them.
/// - USB disk (600 files): the number makes no difference, so 1.
/// - Network share (600 files): 26 / 94 → 3.8 / 7 with 16: each file waits for
///   the server, so many wait at once.
pub fn threads(media: Media) -> usize {
    match media {
        Media::Ssd => 8,
        Media::Hdd => 2,
        Media::Usb => 1,
        Media::Network => 16,
        Media::Unknown => 2,
    }
}

/// The kind of drive `path` is on.
pub fn of(path: &Path) -> Media {
    #[cfg(windows)]
    return windows::of(path);
    #[cfg(target_os = "linux")]
    return linux::of(path);
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = path;
        Media::Unknown
    }
}

#[cfg(windows)]
mod windows {
    use super::Media;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        BusTypeMmc, BusTypeSd, BusTypeUsb, CreateFileW, GetDriveTypeW, GetVolumePathNameW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Ioctl::{
        PropertyStandardQuery, StorageDeviceProperty, StorageDeviceSeekPenaltyProperty, DEVICE_SEEK_PENALTY_DESCRIPTOR,
        IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_ID, STORAGE_PROPERTY_QUERY,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;

    // GetDriveTypeW.
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_REMOTE: u32 = 4;

    /// Asks the drive behind `h` for one of its properties, into `out`; false when it does not say.
    fn query<T>(h: HANDLE, id: STORAGE_PROPERTY_ID, out: &mut T) -> bool {
        let q = STORAGE_PROPERTY_QUERY { PropertyId: id, QueryType: PropertyStandardQuery, AdditionalParameters: [0] };
        let mut got = 0u32;
        // SAFETY: a valid handle; the sizes given are those of the structures given.
        let ok = unsafe {
            DeviceIoControl(
                h,
                IOCTL_STORAGE_QUERY_PROPERTY,
                &q as *const _ as *const _,
                size_of::<STORAGE_PROPERTY_QUERY>() as u32,
                out as *mut T as *mut _,
                size_of::<T>() as u32,
                &mut got,
                std::ptr::null_mut(),
            )
        };
        ok != 0
    }

    pub fn of(path: &Path) -> Media {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        let mut root = [0u16; 261];
        // SAFETY: a terminated path and a buffer of the length given.
        if unsafe { GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0 {
            return Media::Unknown;
        }
        // SAFETY: GetVolumePathNameW ended it with NUL.
        let drive_type = unsafe { GetDriveTypeW(root.as_ptr()) };
        let len = root.iter().position(|&c| c == 0).unwrap_or(0);
        let root = String::from_utf16_lossy(&root[..len]);
        let root = root.trim_start_matches(r"\\?\").trim_end_matches('\\');
        if drive_type == DRIVE_REMOTE || root.starts_with(r"\\") || root.starts_with("UNC") {
            return Media::Network;
        }
        // The volume `C:` itself, asked with no access: no administrator rights needed.
        let device: Vec<u16> = format!(r"\\.\{root}").encode_utf16().chain([0]).collect();
        // SAFETY: a terminated name; the other arguments are plain values.
        let h = unsafe {
            CreateFileW(device.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut())
        };
        if h == INVALID_HANDLE_VALUE {
            return if drive_type == DRIVE_REMOVABLE { Media::Usb } else { Media::Unknown };
        }
        let mut media = Media::Unknown;
        // SAFETY: plain data, filled by the system.
        let mut d: STORAGE_DEVICE_DESCRIPTOR = unsafe { std::mem::zeroed() };
        let bus = query(h, StorageDeviceProperty, &mut d).then_some(d.BusType);
        if drive_type == DRIVE_REMOVABLE || bus.is_some_and(|b| b == BusTypeUsb || b == BusTypeSd || b == BusTypeMmc) {
            media = Media::Usb;
        } else {
            // SAFETY: plain data, filled by the system.
            let mut seek: DEVICE_SEEK_PENALTY_DESCRIPTOR = unsafe { std::mem::zeroed() };
            if query(h, StorageDeviceSeekPenaltyProperty, &mut seek) {
                media = if seek.IncursSeekPenalty { Media::Hdd } else { Media::Ssd };
            }
        }
        // SAFETY: the handle is ours and closed once.
        unsafe { CloseHandle(h) };
        media
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::Media;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    const NETWORK: [&str; 7] = ["nfs", "nfs4", "cifs", "smb3", "fuse.sshfs", "9p", "davfs"];

    /// The type of the filesystem mounted closest above `path`.
    fn filesystem(path: &Path) -> Option<String> {
        let mounts = std::fs::read_to_string("/proc/self/mounts").ok()?;
        mounts
            .lines()
            .filter_map(|l| {
                let mut f = l.split_ascii_whitespace().skip(1);
                // A space in a mount point is written \040.
                Some((f.next()?.replace("\\040", " "), f.next()?.to_string()))
            })
            .filter(|(at, _)| path.starts_with(at))
            .max_by_key(|(at, _)| at.len())
            .map(|(_, fs)| fs)
    }

    pub fn of(path: &Path) -> Media {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if filesystem(&path).is_some_and(|f| NETWORK.contains(&f.as_str())) {
            return Media::Network;
        }
        let Ok(meta) = std::fs::metadata(&path) else { return Media::Unknown };
        let (major, minor) = (libc::major(meta.dev()), libc::minor(meta.dev()));
        let Ok(device) = std::fs::canonicalize(format!("/sys/dev/block/{major}:{minor}")) else { return Media::Unknown };
        if device.to_string_lossy().contains("/usb") {
            return Media::Usb;
        }
        // A partition has no queue of its own: the disk above it has.
        let read = |d: &Path| std::fs::read_to_string(d.join("queue/rotational")).ok();
        match read(&device).or_else(|| device.parent().and_then(read)).as_deref().map(str::trim) {
            Some("1") => Media::Hdd,
            Some("0") => Media::Ssd,
            _ => Media::Unknown,
        }
    }
}
