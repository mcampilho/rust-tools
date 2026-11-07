//! Reading an NTFS volume directly: its Master File Table (MFT), the table
//! with one 1 KB record per file and folder.
//!
//! Instead of opening folders one by one, the whole MFT is read in large
//! sequential chunks, and the tree is put together from the records: each
//! file name records its parent folder. That is how WizTree and Everything
//! read a drive in seconds. It needs administrator rights on Windows (the
//! volume is opened as `\\.\C:`).
//!
//! The reader takes any source of bytes ([`Disk`]): a volume, or an image
//! file in the tests. What it reads from each record:
//!
//! - `$STANDARD_INFORMATION`: the last change.
//! - `$FILE_NAME`: the name and the parent folder; every name but the DOS
//!   short name, so a file with hard links has several. It is counted once,
//!   where it is met first.
//! - `$DATA`: the length (unnamed stream) and the space on the disk (all
//!   streams; compressed and sparse files count what they really take).
//! - `$INDEX_ALLOCATION`: the space a folder's own index takes.
//! - `$REPARSE_POINT`: junctions and symbolic links, not followed.
//!
//! Attributes in extension records (files with many names or fragments)
//! are added to their base record.

use super::Progress;
use crate::model::{Entry, Kind, Tree, F_HARDLINK};
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};

/// The root folder's record.
pub const ROOT: u32 = 5;
/// `$BadClus`: its `$Bad` stream is as large as the volume and not real data.
const BAD_CLUS: u32 = 8;

const ATTR_STANDARD: u32 = 0x10;
const ATTR_LIST: u32 = 0x20;
const ATTR_FILE_NAME: u32 = 0x30;
const ATTR_DATA: u32 = 0x80;
const ATTR_INDEX_ALLOC: u32 = 0xA0;
const ATTR_REPARSE: u32 = 0xC0;
const ATTR_END: u32 = 0xFFFF_FFFF;

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

/// Bytes read at a time.
const CHUNK: u64 = 4 << 20;

/// Something to read a volume from, at any offset that is a multiple of
/// the sector size, for a whole number of sectors.
pub trait Disk {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
}

impl<T: Read + Seek> Disk for T {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.seek(SeekFrom::Start(offset))?;
        self.read_exact(buf)
    }
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

/// The geometry from the boot sector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub sector: u64,
    pub cluster: u64,
    pub record: u64,
    pub mft_lcn: u64,
}

impl Geometry {
    pub fn parse(boot: &[u8]) -> Option<Geometry> {
        if boot.get(3..11)? != b"NTFS    " {
            return None;
        }
        let sector = u16_at(boot, 0x0B)? as u64;
        let spc = *boot.get(0x0D)?;
        // Above 0x80 the value is a negative power of two (64 KB clusters and up).
        let sectors = if spc > 0x80 { 1u64 << (256 - spc as u32) } else { spc as u64 };
        let cluster = sector * sectors;
        let mft_lcn = u64_at(boot, 0x30)?;
        let cpr = *boot.get(0x40)? as i8;
        let record = if cpr < 0 { 1u64 << (-(cpr as i32)) } else { cpr as u64 * cluster };
        let ok = sector.is_power_of_two() && (512..=4096).contains(&sector) && cluster > 0 && (256..=65536).contains(&record);
        ok.then_some(Geometry { sector, cluster, record, mft_lcn })
    }
}

/// Applies the update sequence: the last two bytes of each 512-byte block
/// were replaced on disk by a check value. False when the record is torn.
fn fixup(rec: &mut [u8]) -> bool {
    let (Some(off), Some(count)) = (u16_at(rec, 4), u16_at(rec, 6)) else { return false };
    let (off, count) = (off as usize, count as usize);
    if count == 0 || off + count * 2 > rec.len() || (count - 1) * 512 > rec.len() {
        return false;
    }
    let usn = [rec[off], rec[off + 1]];
    for i in 1..count {
        let end = i * 512 - 2;
        if rec[end..end + 2] != usn {
            return false;
        }
        rec[end] = rec[off + i * 2];
        rec[end + 1] = rec[off + i * 2 + 1];
    }
    true
}

/// Decodes a run list into (first cluster, number of clusters) pairs; a
/// sparse run has no cluster (None).
fn runs(b: &[u8]) -> Vec<(Option<u64>, u64)> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut lcn: i64 = 0;
    while let Some(&head) = b.get(i) {
        if head == 0 {
            break;
        }
        let (nl, no) = ((head & 0x0F) as usize, (head >> 4) as usize);
        if nl == 0 || nl > 8 || no > 8 || i + 1 + nl + no > b.len() {
            break;
        }
        let mut len = 0u64;
        for k in 0..nl {
            len |= (b[i + 1 + k] as u64) << (8 * k);
        }
        if no == 0 {
            out.push((None, len));
        } else {
            let mut delta: i64 = 0;
            for k in 0..no {
                delta |= (b[i + 1 + nl + k] as i64) << (8 * k);
            }
            // Sign-extend.
            let shift = 64 - 8 * no as u32;
            delta = (delta << shift) >> shift;
            lcn += delta;
            out.push((Some(lcn as u64), len));
        }
        i += 1 + nl + no;
    }
    out
}

/// What is kept of a file record.
#[derive(Clone, Debug, Default)]
struct Rec {
    in_use: bool,
    dir: bool,
    link: bool,
    size: u64,
    alloc: u64,
    mtime: i64,
}

/// A name of a file: in folder `parent`, of record `frn`.
#[derive(Clone, Debug)]
struct Name {
    parent: u32,
    frn: u32,
    name: String,
}

/// One attribute's header.
struct Attr<'a> {
    kind: u32,
    named: bool,
    non_resident: bool,
    flags: u16,
    /// The value of a resident attribute.
    value: &'a [u8],
    /// The whole attribute (for a non-resident one).
    raw: &'a [u8],
}

fn attrs(rec: &[u8]) -> Vec<Attr<'_>> {
    let mut out = Vec::new();
    let Some(first) = u16_at(rec, 0x14) else { return out };
    let used = u32_at(rec, 0x18).map_or(rec.len(), |u| (u as usize).min(rec.len()));
    let mut off = first as usize;
    while off + 16 <= used {
        let Some(kind) = u32_at(rec, off) else { break };
        if kind == ATTR_END {
            break;
        }
        let Some(len) = u32_at(rec, off + 4).map(|l| l as usize) else { break };
        if len < 16 || off + len > used {
            break;
        }
        let raw = &rec[off..off + len];
        let non_resident = raw[8] != 0;
        let named = raw[9] != 0;
        let flags = u16_at(raw, 12).unwrap_or(0);
        let value = if non_resident {
            &[][..]
        } else {
            let vlen = u32_at(raw, 0x10).unwrap_or(0) as usize;
            let voff = u16_at(raw, 0x14).unwrap_or(0) as usize;
            raw.get(voff..voff + vlen).unwrap_or(&[])
        };
        out.push(Attr { kind, named, non_resident, flags, value, raw });
        off += len;
    }
    out
}

/// (lowest VCN, data size, space on the disk) of a non-resident attribute.
fn non_resident_sizes(a: &Attr) -> (u64, u64, u64) {
    let lowest = u64_at(a.raw, 0x10).unwrap_or(0);
    let allocated = u64_at(a.raw, 0x28).unwrap_or(0);
    let size = u64_at(a.raw, 0x30).unwrap_or(0);
    // Compressed (0x00FF) or sparse (0x8000): the total really allocated.
    let disk = if a.flags & 0x80FF != 0 { u64_at(a.raw, 0x40).unwrap_or(allocated) } else { allocated };
    (lowest, size, disk)
}

/// The record number of a file reference (its low 48 bits).
fn frn_of(reference: u64) -> u64 {
    reference & 0xFFFF_FFFF_FFFF
}

/// Reads one record's attributes into `recs` (at its base record) and `names`.
fn parse_record(n: u32, rec: &[u8], recs: &mut [Rec], names: &mut Vec<Name>) {
    let flags = u16_at(rec, 0x16).unwrap_or(0);
    if flags & 1 == 0 {
        return;
    }
    let base = u64_at(rec, 0x20).map_or(0, frn_of);
    let id = if base != 0 { base as usize } else { n as usize };
    let Some(r) = recs.get_mut(id) else { return };
    if base == 0 {
        r.in_use = true;
        r.dir = flags & 2 != 0;
    }
    for a in attrs(rec) {
        match a.kind {
            ATTR_STANDARD if a.value.len() >= 0x10 => {
                let t = u64_at(a.value, 8).unwrap_or(0) as i64;
                r.mtime = t / 10_000_000 - 11_644_473_600;
            }
            ATTR_FILE_NAME if a.value.len() >= 0x42 => {
                let space = a.value[0x41];
                if space == 2 {
                    continue; // the DOS short name
                }
                let len = a.value[0x40] as usize;
                let Some(bytes) = a.value.get(0x42..0x42 + len * 2) else { continue };
                let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                let parent = frn_of(u64_at(a.value, 0).unwrap_or(0)) as u32;
                let name = String::from_utf16_lossy(&units);
                if !names.iter().rev().take(4).any(|x| x.frn == id as u32 && x.parent == parent && x.name == name) {
                    names.push(Name { parent, frn: id as u32, name });
                }
            }
            ATTR_DATA if id as u32 == BAD_CLUS && a.named => {}
            ATTR_DATA => {
                if a.non_resident {
                    let (lowest, size, disk) = non_resident_sizes(&a);
                    if lowest == 0 {
                        r.alloc += disk;
                        if !a.named {
                            r.size = size;
                        }
                    }
                } else if !a.named {
                    r.size = a.value.len() as u64;
                }
            }
            ATTR_INDEX_ALLOC if a.non_resident => {
                let (lowest, _, disk) = non_resident_sizes(&a);
                if lowest == 0 {
                    r.alloc += disk;
                }
            }
            ATTR_REPARSE => {
                let tag = u32_at(a.value, 0).unwrap_or(0);
                if tag == IO_REPARSE_TAG_MOUNT_POINT || tag == IO_REPARSE_TAG_SYMLINK {
                    r.link = true;
                }
            }
            _ => {}
        }
    }
}

/// Pieces of a run list, by their first VCN.
type Extents = Vec<(u64, Vec<(Option<u64>, u64)>)>;

/// Where the MFT's data lies: its runs in bytes (offset, length), and how
/// many records it holds.
fn mft_layout(disk: &mut dyn Disk, g: &Geometry) -> io::Result<(Vec<(u64, u64)>, u64)> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "the MFT's own record cannot be read");
    let mut rec0 = vec![0u8; g.record.max(g.sector) as usize];
    disk.read_at(g.mft_lcn * g.cluster, &mut rec0)?;
    rec0.truncate(g.record as usize);
    if rec0.get(0..4) != Some(b"FILE") || !fixup(&mut rec0) {
        return Err(bad());
    }
    let mut extents: Extents = Vec::new();
    let mut size = 0;
    let mut list = Vec::new();
    for a in attrs(&rec0) {
        if a.kind == ATTR_DATA && !a.named && a.non_resident {
            let (lowest, s, _) = non_resident_sizes(&a);
            if lowest == 0 {
                size = s;
            }
            let roff = u16_at(a.raw, 0x20).unwrap_or(0) as usize;
            extents.push((lowest, runs(a.raw.get(roff..).unwrap_or(&[]))));
        } else if a.kind == ATTR_LIST && !a.non_resident {
            list = a.value.to_vec();
        }
    }
    let to_bytes = |extents: &Extents| -> Vec<(u64, u64)> {
        let mut sorted: Vec<_> = extents.iter().collect();
        sorted.sort_by_key(|e| e.0);
        sorted.iter().flat_map(|e| e.1.iter()).map(|&(lcn, n)| (lcn.unwrap_or(0) * g.cluster, n * g.cluster)).collect()
    };
    // A fragmented MFT continues its run list in other records, named in its
    // attribute list.
    let mut i = 0;
    while i + 0x1A <= list.len() {
        let (Some(kind), Some(len)) = (u32_at(&list, i), u16_at(&list, i + 4)) else { break };
        if len == 0 {
            break;
        }
        let other = u64_at(&list, i + 0x10).map_or(0, frn_of);
        if kind == ATTR_DATA && other != 0 {
            let layout = to_bytes(&extents);
            if let Some(off) = record_offset(&layout, other, g.record) {
                let mut rec = vec![0u8; g.record.max(g.sector) as usize];
                disk.read_at(off - off % g.sector, &mut rec)?;
                let mut rec = rec[(off % g.sector) as usize..].to_vec();
                rec.truncate(g.record as usize);
                if rec.get(0..4) == Some(b"FILE") && fixup(&mut rec) {
                    for a in attrs(&rec) {
                        if a.kind == ATTR_DATA && !a.named && a.non_resident {
                            let (lowest, _, _) = non_resident_sizes(&a);
                            let roff = u16_at(a.raw, 0x20).unwrap_or(0) as usize;
                            if !extents.iter().any(|e| e.0 == lowest) {
                                extents.push((lowest, runs(a.raw.get(roff..).unwrap_or(&[]))));
                            }
                        }
                    }
                }
            }
        }
        i += len as usize;
    }
    if size == 0 || extents.is_empty() {
        return Err(bad());
    }
    Ok((to_bytes(&extents), size / g.record))
}

/// The disk offset of record `n`, given the MFT's runs.
fn record_offset(layout: &[(u64, u64)], n: u64, record: u64) -> Option<u64> {
    let mut at = n * record;
    for &(off, len) in layout {
        if at < len {
            return Some(off + at);
        }
        at -= len;
    }
    None
}

/// All the records of the volume, read in large chunks.
struct Mft {
    recs: Vec<Rec>,
    names: Vec<Name>,
}

fn read_mft(disk: &mut dyn Disk, cancel: &AtomicBool, progress: &dyn Fn(&Progress), label: &dyn Fn(u64) -> String) -> io::Result<Mft> {
    let mut boot = vec![0u8; 4096];
    disk.read_at(0, &mut boot[..512])?;
    let g = Geometry::parse(&boot).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not an NTFS volume"))?;
    let (layout, count) = mft_layout(disk, &g)?;
    let count = count.min(u32::MAX as u64);
    let mut recs = vec![Rec::default(); count as usize];
    let mut names = Vec::new();
    let rs = g.record as usize;
    let mut pending: Vec<u8> = Vec::with_capacity(CHUNK as usize + rs);
    let mut n: u64 = 0;
    let mut p = Progress::default();
    let mut last = std::time::Instant::now();
    'runs: for &(off, len) in &layout {
        let mut done = 0;
        while done < len {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "stopped"));
            }
            let take = (len - done).min(CHUNK);
            let start = pending.len();
            pending.resize(start + take as usize, 0);
            disk.read_at(off + done, &mut pending[start..])?;
            done += take;
            let whole = pending.len() / rs * rs;
            for rec in pending[..whole].chunks_exact_mut(rs) {
                if n >= count {
                    break 'runs;
                }
                if rec.starts_with(b"FILE") && fixup(rec) {
                    parse_record(n as u32, rec, &mut recs, &mut names);
                }
                n += 1;
            }
            pending.drain(..whole);
            if last.elapsed().as_millis() >= 100 {
                last = std::time::Instant::now();
                p.files = n;
                p.current = label(n * 100 / count.max(1));
                progress(&p);
            }
        }
    }
    Ok(Mft { recs, names })
}

/// The tree below record `top`, named `root` (its full path).
fn build(m: &Mft, top: u32, root: &str) -> Tree {
    let mut tree = Tree::new(root, m.recs.get(top as usize).map_or(0, |r| r.mtime));
    // Names grouped by folder.
    let mut order: Vec<u32> = (0..m.names.len() as u32).filter(|&i| m.names[i as usize].parent != m.names[i as usize].frn).collect();
    order.sort_unstable_by_key(|&i| m.names[i as usize].parent);
    let parents: Vec<u32> = order.iter().map(|&i| m.names[i as usize].parent).collect();
    let mut counted = vec![false; m.recs.len()];
    let mut queue = std::collections::VecDeque::from([(top, 0u32)]);
    while let Some((frn, id)) = queue.pop_front() {
        let from = parents.partition_point(|&p| p < frn);
        let to = parents.partition_point(|&p| p <= frn);
        let mut entries = Vec::with_capacity(to - from);
        let mut dirs = Vec::new();
        for &ni in &order[from..to] {
            let name = &m.names[ni as usize];
            let Some(r) = m.recs.get(name.frn as usize).filter(|r| r.in_use) else { continue };
            let kind = match (r.dir, r.link) {
                (_, true) => Kind::Link,
                (true, false) => Kind::Dir,
                (false, false) => Kind::File,
            };
            let first = !std::mem::replace(&mut counted[name.frn as usize], true);
            let (size, alloc, flags) = match (kind, first) {
                (Kind::File, false) => (0, 0, F_HARDLINK),
                (Kind::File, true) => (r.size, r.alloc, 0),
                // A folder's own space is its index; it has no length.
                _ => (0, r.alloc, 0),
            };
            if kind == Kind::Dir {
                dirs.push((entries.len(), name.frn));
            }
            entries.push(Entry { name: name.name.clone(), kind, size, alloc, mtime: r.mtime, flags });
        }
        let first = tree.add_children(id, &entries);
        for (i, frn) in dirs {
            queue.push_back((frn, first + i as u32));
        }
    }
    tree
}

/// The record of the folder at `parts` below the root (names compared
/// without case, as NTFS does on Windows).
fn find(m: &Mft, parts: &[String]) -> Option<u32> {
    let mut cur = ROOT;
    for part in parts {
        let want = part.to_lowercase();
        cur = m.names.iter().find(|n| n.parent == cur && n.frn != cur && n.name.to_lowercase() == want)?.frn;
    }
    m.recs.get(cur as usize).filter(|r| r.in_use && r.dir).map(|_| cur)
}

/// Reads the tree of the folder at `parts` (empty: the whole volume) from
/// `disk`, naming its root `root`.
pub fn scan_disk(
    disk: &mut dyn Disk,
    parts: &[String],
    root: &str,
    cancel: &AtomicBool,
    progress: &dyn Fn(&Progress),
    label: &dyn Fn(u64) -> String,
) -> io::Result<Tree> {
    let m = read_mft(disk, cancel, progress, label)?;
    let top = find(&m, parts).ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    let mut tree = build(&m, top, root);
    tree.finish();
    Ok(tree)
}

/// Why a volume was not read directly.
#[derive(Debug)]
pub enum Skip {
    /// Not a local NTFS volume.
    NotNtfs,
    /// No administrator rights to open the volume.
    Denied,
    Stopped,
    Failed(io::Error),
}

impl From<io::Error> for Skip {
    fn from(e: io::Error) -> Skip {
        match e.kind() {
            io::ErrorKind::Interrupted => Skip::Stopped,
            io::ErrorKind::PermissionDenied => Skip::Denied,
            _ => Skip::Failed(e),
        }
    }
}

/// Reads the folder `root` (normalized, see `scan::normalize`) through its
/// volume's MFT, when it is on a local NTFS volume and the volume can be
/// opened.
#[cfg(windows)]
pub fn scan_volume(root: &std::path::Path, cancel: &AtomicBool, progress: &dyn Fn(&Progress), label: &dyn Fn(u64) -> String) -> Result<Tree, Skip> {
    use rt_sys::win::{from_wide, wide};
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetVolumeInformationW, GetVolumeNameForVolumeMountPointW, GetVolumePathNameW};
    use windows_sys::Win32::System::WindowsProgramming::DRIVE_REMOTE;
    let text = root.to_string_lossy().into_owned();
    let w = wide(&text);
    let mut mount = vec![0u16; 1024];
    // SAFETY: NUL-terminated input; the output buffer is as long as stated.
    if unsafe { GetVolumePathNameW(w.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) } == 0 {
        return Err(Skip::NotNtfs);
    }
    let mount_s = from_wide(&mount);
    let mw = wide(&mount_s);
    // SAFETY: NUL-terminated string.
    if unsafe { GetDriveTypeW(mw.as_ptr()) } == DRIVE_REMOTE {
        return Err(Skip::NotNtfs);
    }
    let mut fs = vec![0u16; 64];
    // SAFETY: the buffer is as long as stated; the other outputs are null.
    let ok = unsafe {
        GetVolumeInformationW(mw.as_ptr(), std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), fs.as_mut_ptr(), fs.len() as u32)
    };
    if ok == 0 || from_wide(&fs) != "NTFS" {
        return Err(Skip::NotNtfs);
    }
    let mut guid = vec![0u16; 64];
    // SAFETY: NUL-terminated mount point ending in a backslash; buffer as stated.
    if unsafe { GetVolumeNameForVolumeMountPointW(mw.as_ptr(), guid.as_mut_ptr(), guid.len() as u32) } == 0 {
        return Err(Skip::NotNtfs);
    }
    // "\\?\Volume{…}\" names the root folder; without the backslash, the volume.
    let device = from_wide(&guid).trim_end_matches('\\').to_string();
    let mut disk = std::fs::File::open(&device)?;
    let parts: Vec<String> = text.get(mount_s.len()..).unwrap_or("").split('\\').filter(|p| !p.is_empty()).map(str::to_string).collect();
    Ok(scan_disk(&mut disk, &parts, &text, cancel, progress, label)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// An 8 MB volume made by `mkntfs`, with files copied in by `ntfscp`
    /// (see the comment in the fixture's README).
    fn image() -> Cursor<Vec<u8>> {
        let z = include_bytes!("../../tests/fixtures/ntfs-small.img.zlib");
        Cursor::new(miniz_oxide::inflate::decompress_to_vec_zlib(z).expect("fixture"))
    }

    fn scan_image(parts: &[&str]) -> Tree {
        let parts: Vec<String> = parts.iter().map(|s| s.to_string()).collect();
        scan_disk(&mut image(), &parts, "T:\\", &AtomicBool::new(false), &|_| {}, &|p| format!("{p}%")).unwrap()
    }

    fn child(t: &Tree, parent: u32, name: &str) -> u32 {
        *t.children(parent).iter().find(|&&c| t.name(c) == name).unwrap_or_else(|| panic!("{name} not found"))
    }

    #[test]
    fn the_boot_sector_gives_the_geometry() {
        let mut d = image();
        let mut boot = vec![0u8; 512];
        d.read_at(0, &mut boot).unwrap();
        let g = Geometry::parse(&boot).unwrap();
        assert_eq!((g.sector, g.cluster, g.record), (512, 4096, 1024));
        assert!(Geometry::parse(&[0u8; 512]).is_none());
    }

    #[test]
    fn files_folders_and_sizes() {
        let t = scan_image(&[]);
        let small = child(&t, 0, "small.txt");
        assert_eq!(t.node(small).size, 35, "resident data");
        assert!(t.node(small).alloc >= 4096, "its named stream takes clusters");
        let big = child(&t, 0, "big.bin");
        assert_eq!(t.node(big).size, 50_000);
        assert_eq!(t.node(big).alloc, 53_248, "13 clusters");
        assert_eq!(t.node(child(&t, 0, "larger.dat")).size, 300_000);
        assert_eq!(t.node(child(&t, 0, "ação ü.txt")).size, 6);
        let ext = child(&t, 0, "$Extend");
        assert_eq!(t.node(ext).kind, Kind::Dir);
        assert_eq!(t.node(child(&t, ext, "one.txt")).size, 1);
        let mft = child(&t, 0, "$MFT");
        assert!(t.node(mft).size >= 64 * 1024);
        // $BadClus's stream as large as the volume is not counted.
        assert_eq!(t.node(child(&t, 0, "$BadClus")).alloc, 0);
        assert!(t.node(0).alloc < 8 << 20);
        assert!(t.node(0).mtime > 1_700_000_000);
        // No DOS short names, and the root is not its own child.
        assert!(t.children(0).iter().all(|&c| !t.name(c).contains('~') && t.name(c) != "."));
    }

    #[test]
    fn a_folder_of_the_volume() {
        let t = scan_image(&["$EXTEND"]);
        assert_eq!(t.name(0), "T:\\");
        assert!(t.children(0).iter().any(|&c| t.name(c) == "one.txt"));
        assert!(!t.children(0).iter().any(|&c| t.name(c) == "big.bin"), "only what is inside");
        let parts = vec!["nope".to_string()];
        assert!(scan_disk(&mut image(), &parts, "x", &AtomicBool::new(false), &|_| {}, &|_| String::new()).is_err());
    }

    #[test]
    fn run_lists_and_fixups() {
        // 0x21: 1 byte of length, 2 of offset; then a negative jump; then sparse.
        let r = runs(&[0x21, 0x10, 0x00, 0x01, 0x11, 0x08, 0xF0, 0x01, 0x05, 0x00]);
        assert_eq!(r, vec![(Some(0x100), 0x10), (Some(0xF0), 0x08), (None, 0x05)]);
        let mut rec = vec![0u8; 1024];
        rec[4] = 0x30; // update sequence at 0x30, 3 entries
        rec[6] = 3;
        rec[0x30..0x32].copy_from_slice(&[0xAB, 0xCD]);
        rec[0x32..0x36].copy_from_slice(&[1, 2, 3, 4]);
        rec[510..512].copy_from_slice(&[0xAB, 0xCD]);
        rec[1022..1024].copy_from_slice(&[0xAB, 0xCD]);
        assert!(fixup(&mut rec));
        assert_eq!((&rec[510..512], &rec[1022..1024]), (&[1, 2][..], &[3, 4][..]));
        rec[510] = 0;
        rec[0x30] = 0x77;
        assert!(!fixup(&mut rec), "a torn record");
    }

    #[test]
    fn stopping_ends_the_reading() {
        let r = scan_disk(&mut image(), &[], "T:\\", &AtomicBool::new(true), &|_| {}, &|_| String::new());
        assert_eq!(r.err().map(|e| e.kind()), Some(io::ErrorKind::Interrupted));
    }
}
