//! Deleting for good and moving to another drive, on several threads: as many
//! as the drive takes well ([`media`]). Nothing is counted first (who asks
//! knows how much there is, from its own reading), progress is in atomic
//! counters a window can look at when it likes, and all of it can be stopped.
//!
//! Links and junctions are never followed: deleting removes the link, and
//! moving to another drive refuses a folder that holds one.
//!
//! The Recycle Bin is not here (`rt_sys::recycle_many`): that is Windows' work.

pub mod media;
mod pool;

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// The progress of an operation, and the way to stop it.
#[derive(Debug, Default)]
pub struct Control {
    /// Files deleted or copied (links count as files; folders do not).
    pub files: AtomicU64,
    /// Bytes of the files copied (moving to another drive).
    pub bytes: AtomicU64,
    stop: AtomicBool,
}

impl Control {
    pub fn new() -> Control {
        Control::default()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn files(&self) -> u64 {
        self.files.load(Ordering::Relaxed)
    }

    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }
}

/// How many failures are kept with their reason (all are counted).
const KEPT: usize = 50;

/// What could not be done.
#[derive(Debug, Default)]
pub struct Failures {
    pub count: u64,
    /// The first ones, with the reason.
    pub first: Vec<(PathBuf, String)>,
}

impl Failures {
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn add(list: &Mutex<Failures>, path: &Path, why: &io::Error) {
        let mut f = list.lock().unwrap_or_else(|e| e.into_inner());
        f.count += 1;
        if f.first.len() < KEPT {
            f.first.push((shown(path), why.to_string()));
        }
    }
}

/// The path as the system is to be given it: on Windows with `\\?\`, so names
/// that end in a dot or a space, and long paths, mean what they say. Refuses
/// what is not an absolute path below a drive or a share.
fn exact(path: &Path) -> io::Result<PathBuf> {
    let refused = |why: &str| io::Error::new(io::ErrorKind::InvalidInput, why.to_string());
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(refused("not a full path"));
    }
    if path.parent().is_none() {
        return Err(refused("a whole drive or share is never taken"));
    }
    #[cfg(windows)]
    {
        let s = path.to_string_lossy().replace('/', "\\");
        if s.starts_with(r"\\?\") {
            return Ok(PathBuf::from(s));
        }
        // Only a real (UTF-16) name can be rebuilt from its text.
        if path.to_str().is_none() {
            return Ok(path.to_path_buf());
        }
        Ok(PathBuf::from(match s.strip_prefix(r"\\") {
            Some(share) => format!(r"\\?\UNC\{share}"),
            None => format!(r"\\?\{s}"),
        }))
    }
    #[cfg(not(windows))]
    Ok(path.to_path_buf())
}

/// A path without the `\\?\` that [`exact`] put before it.
fn shown(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(share) = s.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{share}"))
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// What a path is, without following links.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Folder,
    File,
    /// A symbolic link or a junction: removed itself, never followed.
    Link {
        to_folder: bool,
    },
}

fn kind(meta: &fs::Metadata) -> Kind {
    let t = meta.file_type();
    if t.is_symlink() {
        #[cfg(windows)]
        let to_folder = {
            use std::os::windows::fs::MetadataExt;
            meta.file_attributes() & 0x10 != 0
        };
        #[cfg(not(windows))]
        let to_folder = false;
        Kind::Link { to_folder }
    } else if t.is_dir() {
        Kind::Folder
    } else {
        Kind::File
    }
}

/// Runs `remove`, and again without the read-only attribute if that was in the way.
fn forced(path: &Path, remove: fn(&Path) -> io::Result<()>) -> io::Result<()> {
    match remove(path) {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            let Ok(meta) = fs::symlink_metadata(path) else { return Err(e) };
            let mut p = meta.permissions();
            if !p.readonly() || meta.file_type().is_symlink() {
                return Err(e);
            }
            // The attribute on Windows; elsewhere the owner's write bit.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = p.mode() | 0o200;
                p.set_mode(mode);
            }
            // On Windows this only clears the attribute.
            #[cfg(not(unix))]
            #[allow(clippy::permissions_set_readonly_false)]
            p.set_readonly(false);
            fs::set_permissions(path, p)?;
            remove(path)
        }
        r => r,
    }
}

fn remove_file(p: &Path) -> io::Result<()> {
    fs::remove_file(p)
}

fn remove_dir(p: &Path) -> io::Result<()> {
    fs::remove_dir(p)
}

/// A folder being emptied: removed when nothing in it is left to do.
struct Folder {
    path: PathBuf,
    parent: Option<Arc<Folder>>,
    /// Its listing, plus what was found in it and is not done yet.
    pending: AtomicUsize,
    /// Something in it stayed, so it stays too.
    kept: AtomicBool,
}

impl Folder {
    fn new(path: PathBuf, parent: Option<Arc<Folder>>) -> Arc<Folder> {
        Arc::new(Folder { path, parent, pending: AtomicUsize::new(1), kept: AtomicBool::new(false) })
    }
}

enum Del {
    List(Arc<Folder>),
    One(PathBuf, Kind, Option<Arc<Folder>>),
}

/// One thing in `folder` is done: with the last one the folder goes, and so on upwards.
fn done(mut folder: Option<Arc<Folder>>, failures: &Mutex<Failures>, ctl: &Control) {
    while let Some(f) = folder {
        if f.pending.fetch_sub(1, Ordering::AcqRel) != 1 {
            return;
        }
        let mut kept = f.kept.load(Ordering::Acquire) || ctl.stopped();
        if !kept {
            if let Err(e) = forced(&f.path, remove_dir) {
                Failures::add(failures, &f.path, &e);
                kept = true;
            }
        }
        if kept {
            if let Some(p) = &f.parent {
                p.kept.store(true, Ordering::Release);
            }
        }
        folder = f.parent.clone();
    }
}

fn keep(folder: &Option<Arc<Folder>>) {
    if let Some(f) = folder {
        f.kept.store(true, Ordering::Release);
    }
}

/// Deletes for good the files and folders `paths`, with all they hold, on
/// `threads` threads. What cannot be deleted stays, with the folders above it;
/// the rest goes. Stopped, what is not deleted yet stays.
pub fn delete(paths: &[PathBuf], threads: usize, ctl: &Control) -> Failures {
    let failures = Mutex::new(Failures::default());
    let mut first = Vec::new();
    for p in paths {
        match exact(p).and_then(|p| fs::symlink_metadata(&p).map(|m| (p, kind(&m)))) {
            Ok((p, Kind::Folder)) => first.push(Del::List(Folder::new(p, None))),
            Ok((p, k)) => first.push(Del::One(p, k, None)),
            Err(e) => Failures::add(&failures, p, &e),
        }
    }
    pool::run(first, threads, ctl, |task, more| match task {
        Del::List(folder) => {
            match fs::read_dir(&folder.path) {
                Ok(entries) => {
                    for entry in entries {
                        // The listing already says what each one is.
                        match entry.and_then(|e| e.metadata().map(|m| (e.path(), kind(&m)))) {
                            Ok((path, k)) => {
                                folder.pending.fetch_add(1, Ordering::AcqRel);
                                more.push(match k {
                                    Kind::Folder => Del::List(Folder::new(path, Some(folder.clone()))),
                                    _ => Del::One(path, k, Some(folder.clone())),
                                });
                            }
                            Err(e) => {
                                Failures::add(&failures, &folder.path, &e);
                                folder.kept.store(true, Ordering::Release);
                            }
                        }
                    }
                }
                Err(e) => {
                    Failures::add(&failures, &folder.path, &e);
                    folder.kept.store(true, Ordering::Release);
                }
            }
            done(Some(folder), &failures, ctl);
        }
        Del::One(path, k, parent) => {
            let r = match k {
                // A link to a folder is removed as a folder is, on Windows: only the link goes.
                Kind::Link { to_folder: true } => forced(&path, remove_dir),
                _ => forced(&path, remove_file),
            };
            match r {
                Ok(()) => {
                    ctl.files.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    Failures::add(&failures, &path, &e);
                    keep(&parent);
                }
            }
            done(parent, &failures, ctl);
        }
    });
    failures.into_inner().unwrap_or_else(|e| e.into_inner())
}

/// [`delete`] with the threads that suit each drive: the paths are grouped by
/// the kind of drive they are on, and the groups done one after the other.
pub fn delete_auto(paths: &[PathBuf], ctl: &Control) -> Failures {
    let mut groups: Vec<(media::Media, Vec<PathBuf>)> = Vec::new();
    for p in paths {
        let m = media::of(p);
        match groups.iter_mut().find(|(g, _)| *g == m) {
            Some((_, list)) => list.push(p.clone()),
            None => groups.push((m, vec![p.clone()])),
        }
    }
    let mut all = Failures::default();
    for (m, list) in groups {
        let f = delete(&list, media::threads(m), ctl);
        all.count += f.count;
        let room = KEPT.saturating_sub(all.first.len());
        all.first.extend(f.first.into_iter().take(room));
    }
    all
}

enum Copy {
    Folder(PathBuf, PathBuf),
    File(PathBuf, PathBuf),
}

/// Copies the file or folder `from` to `to`, which must not be there.
fn copy(from: &Path, to: &Path, threads: usize, ctl: &Control, link_refused: &str) -> Failures {
    let failures = Mutex::new(Failures::default());
    let link = || io::Error::other(link_refused.to_string());
    let first = match fs::symlink_metadata(from).map(|m| kind(&m)) {
        Ok(Kind::Folder) => vec![Copy::Folder(from.to_path_buf(), to.to_path_buf())],
        Ok(Kind::File) => vec![Copy::File(from.to_path_buf(), to.to_path_buf())],
        Ok(Kind::Link { .. }) => {
            Failures::add(&failures, from, &link());
            Vec::new()
        }
        Err(e) => {
            Failures::add(&failures, from, &e);
            Vec::new()
        }
    };
    pool::run(first, threads, ctl, |task, more| match task {
        Copy::Folder(from, to) => {
            let r = fs::create_dir(&to).and_then(|()| {
                for entry in fs::read_dir(&from)? {
                    let entry = entry?;
                    let (src, dst) = (entry.path(), to.join(entry.file_name()));
                    match kind(&entry.metadata()?) {
                        Kind::Folder => more.push(Copy::Folder(src, dst)),
                        Kind::File => more.push(Copy::File(src, dst)),
                        Kind::Link { .. } => Failures::add(&failures, &src, &link()),
                    }
                }
                Ok(())
            });
            if let Err(e) = r {
                Failures::add(&failures, &from, &e);
            }
        }
        Copy::File(from, to) => match copy_file(&from, &to, ctl) {
            Ok(()) => {
                ctl.files.fetch_add(1, Ordering::Relaxed);
            }
            // Stopped in the middle of a file: that is not a failure.
            Err(_) if ctl.stopped() => {}
            Err(e) => Failures::add(&failures, &from, &e),
        },
    });
    failures.into_inner().unwrap_or_else(|e| e.into_inner())
}

/// Files this large are copied without the system's cache, which they would only fill.
#[cfg(windows)]
const UNBUFFERED_FROM: u64 = 256 << 20;

#[cfg(windows)]
fn copy_file(from: &Path, to: &Path, ctl: &Control) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::{
        CopyFileExW, COPY_FILE_FAIL_IF_EXISTS, COPY_FILE_NO_BUFFERING, LPPROGRESS_ROUTINE_CALLBACK_REASON,
    };

    struct Seen<'a> {
        ctl: &'a Control,
        bytes: u64,
    }

    unsafe extern "system" fn progress(
        _total: i64,
        transferred: i64,
        _stream_size: i64,
        _stream_transferred: i64,
        _stream: u32,
        _reason: LPPROGRESS_ROUTINE_CALLBACK_REASON,
        _source: HANDLE,
        _destination: HANDLE,
        data: *const core::ffi::c_void,
    ) -> u32 {
        // SAFETY: `data` is the `Seen` of the call below, on the same thread.
        let seen = unsafe { &mut *(data as *mut Seen) };
        let now = transferred.max(0) as u64;
        seen.ctl.bytes.fetch_add(now.saturating_sub(seen.bytes), Ordering::Relaxed);
        seen.bytes = now;
        // PROGRESS_CANCEL (the part copied is deleted) or PROGRESS_CONTINUE.
        u32::from(seen.ctl.stopped())
    }

    let wide = |p: &Path| p.as_os_str().encode_wide().chain([0]).collect::<Vec<u16>>();
    let large = fs::symlink_metadata(from)?.len() >= UNBUFFERED_FROM;
    let flags = COPY_FILE_FAIL_IF_EXISTS | if large { COPY_FILE_NO_BUFFERING } else { 0 };
    let mut seen = Seen { ctl, bytes: 0 };
    // SAFETY: both paths end in NUL; `seen` outlives the call, which is the only user of it meanwhile.
    let ok = unsafe {
        CopyFileExW(wide(from).as_ptr(), wide(to).as_ptr(), Some(progress), &mut seen as *mut Seen as *const _, std::ptr::null_mut(), flags)
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn copy_file(from: &Path, to: &Path, ctl: &Control) -> io::Result<()> {
    use std::io::{Read, Write};
    let mut src = fs::File::open(from)?;
    let meta = src.metadata()?;
    let mut dst = fs::OpenOptions::new().write(true).create_new(true).open(to)?;
    let mut buf = vec![0u8; 1 << 20];
    let copied = loop {
        if ctl.stopped() {
            break Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        match src.read(&mut buf) {
            Ok(0) => break Ok(()),
            Ok(n) => {
                if let Err(e) = dst.write_all(&buf[..n]) {
                    break Err(e);
                }
                ctl.bytes.fetch_add(n as u64, Ordering::Relaxed);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => break Err(e),
        }
    };
    if let Err(e) = copied {
        drop(dst);
        let _ = fs::remove_file(to);
        return Err(e);
    }
    // The time and the permissions go with it, where the drive keeps them.
    if let Ok(t) = meta.modified() {
        let _ = dst.set_modified(t);
    }
    let _ = dst.set_permissions(meta.permissions());
    Ok(())
}

/// How a move ended.
#[derive(Debug)]
pub enum Moved {
    /// All of it is at the new place.
    All,
    /// Stopped: it is where it was, and nothing is at the new place.
    Stopped,
    /// It could not be copied: it is where it was, and nothing is at the new place.
    NotCopied(Failures),
    /// It is whole at the new place, but not all of the old one could be removed.
    NotRemoved(Failures),
}

/// Moves the file or folder `from` to `to`, which must not be there. On the
/// same drive it is renamed; to another one it is copied, and removed only
/// once the copy is whole. `link_refused` is the reason given for a folder
/// with a link in it, which is not moved to another drive.
pub fn move_to(from: &Path, to: &Path, threads: usize, ctl: &Control, link_refused: &str) -> io::Result<Moved> {
    let (from, to) = (exact(from)?, exact(to)?);
    if to.symlink_metadata().is_ok() {
        return Err(io::Error::from(io::ErrorKind::AlreadyExists));
    }
    match fs::rename(&from, &to) {
        Ok(()) => return Ok(Moved::All),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {}
        Err(e) => return Err(e),
    }
    copy_then_remove(&from, &to, threads, ctl, link_refused)
}

fn copy_then_remove(from: &Path, to: &Path, threads: usize, ctl: &Control, link_refused: &str) -> io::Result<Moved> {
    let failures = copy(from, to, threads, ctl, link_refused);
    if ctl.stopped() || !failures.is_empty() {
        // Only what was just made is removed (`to` was not there before).
        if to.symlink_metadata().is_ok() {
            delete(std::slice::from_ref(&to.to_path_buf()), threads, &Control::new());
        }
        return Ok(if ctl.stopped() { Moved::Stopped } else { Moved::NotCopied(failures) });
    }
    // The copy is whole: from here on it is not stopped half way.
    let failures = delete(std::slice::from_ref(&from.to_path_buf()), threads, &Control::new());
    Ok(if failures.is_empty() { Moved::All } else { Moved::NotRemoved(failures) })
}

#[cfg(test)]
mod tests;
