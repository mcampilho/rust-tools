//! Folder listing on Linux and other Unix systems: `readdir` and `lstat`.
//!
//! Folders on another device (other mounts, `/proc`, `/sys`, network
//! shares under the scanned folder) are listed but not entered, like
//! `du -x`. A file with several names is reported with its (device, inode),
//! so the coordinator counts it once.

use super::{Ctx, Raw};
use crate::model::{Entry, Kind, F_OTHER_FS};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

pub(crate) fn list(path: &Path, ctx: Ctx) -> std::io::Result<Vec<Raw>> {
    let mut out = Vec::new();
    for de in std::fs::read_dir(path)? {
        let Ok(de) = de else { continue };
        // DirEntry::metadata does not follow symbolic links.
        let Ok(m) = de.metadata() else { continue };
        let ft = m.file_type();
        let kind = if ft.is_symlink() {
            Kind::Link
        } else if ft.is_dir() {
            Kind::Dir
        } else {
            Kind::File
        };
        let other_fs = kind == Kind::Dir && m.dev() != ctx.dev;
        let size = if kind == Kind::File { m.len() } else { 0 };
        // A folder's own blocks count on the disk, as with `du`.
        let alloc = if other_fs { 0 } else { m.blocks() * 512 };
        let link_key = (kind == Kind::File && m.nlink() > 1).then(|| (m.dev(), m.ino()));
        let entry = Entry {
            name: de.file_name().to_string_lossy().into_owned(),
            kind,
            size,
            alloc,
            mtime: m.mtime(),
            flags: if other_fs { F_OTHER_FS } else { 0 },
        };
        let descend = (kind == Kind::Dir && !other_fs).then(|| de.path());
        out.push(Raw { entry, descend, link_key });
    }
    Ok(out)
}
