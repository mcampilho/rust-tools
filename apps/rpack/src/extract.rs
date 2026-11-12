//! Extracting and testing.
//!
//! Every path from an archive goes through [`safe_path`] first: no `..`, no
//! drive or root, no name Windows cannot hold. Nothing is ever written
//! outside the folder chosen. Links are not extracted (a link could lead the
//! next files out of that folder); they are counted in the report.

use crate::archive::{self, Error, Kind, Listing, Meter};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

/// What to do with a file that is already there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Overwrite {
    Replace,
    Skip,
    /// The new one gets a free name: `name (2).ext`.
    #[default]
    KeepBoth,
}

impl Overwrite {
    pub const ALL: [Overwrite; 3] = [Overwrite::Replace, Overwrite::Skip, Overwrite::KeepBoth];

    pub fn key(self) -> &'static str {
        match self {
            Overwrite::Replace => "replace",
            Overwrite::Skip => "skip",
            Overwrite::KeepBoth => "keep_both",
        }
    }

    pub fn from_key(k: &str) -> Option<Overwrite> {
        Overwrite::ALL.into_iter().find(|o| o.key() == k)
    }
}

/// What to extract, and where.
#[derive(Clone, Debug)]
pub struct Job {
    pub dest: PathBuf,
    /// Which entries (by index); all when None.
    pub wanted: Option<Vec<bool>>,
    /// A folder of the archive to leave out of the paths: extracting
    /// `docs/a.txt` while in `docs` writes `a.txt`.
    pub strip: String,
    pub overwrite: Overwrite,
}

/// How it went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    /// Already there, and left as they were.
    pub skipped: u64,
    /// Already there: written under a new name.
    pub renamed: u64,
    /// Links, devices and paths that could not be made safe: not written.
    pub left_out: u64,
    /// Entries that could not be read or written: (path, why).
    pub errors: Vec<(String, Error)>,
}

/// The path to write an entry to, inside `dest`; None when nothing of the
/// path is left once made safe.
pub fn safe_path(dest: &Path, entry: &str) -> Option<PathBuf> {
    let mut out = dest.to_path_buf();
    let mut any = false;
    for part in entry.split(['/', '\\']) {
        match part {
            "" | "." => {}
            // Up a level: dropped, as the other archivers do, so that the
            // rest stays inside `dest`.
            ".." => {}
            // `C:` at the start of a path stored by a careless archiver.
            p if p.len() == 2 && p.ends_with(':') => {}
            p => {
                let name = safe_name(p);
                if !name.is_empty() {
                    out.push(name);
                    any = true;
                }
            }
        }
    }
    any.then_some(out)
}

/// A name every system can hold: Windows forbids some characters, names
/// ending in a dot or a space, and the names of devices.
fn safe_name(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c }).collect();
    while s.ends_with(['.', ' ']) {
        s.pop();
    }
    let stem = s.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    if device {
        s.insert(0, '_');
    }
    s
}

/// A free name beside `p`: `name (2).ext`, `name (3).ext`...
fn free_name(p: &Path) -> PathBuf {
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 2.. {
        let q = p.with_file_name(format!("{stem} ({n}){ext}"));
        if fs::symlink_metadata(&q).is_err() {
            return q;
        }
    }
    unreachable!()
}

fn set_mtime(f: &File, secs: i64) {
    if secs > 0 {
        let _ = f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64));
    }
}

/// Copies `r` to `w`, counting into the meter; stops when asked.
fn copy(r: &mut dyn Read, w: &mut dyn Write, meter: &Meter) -> Result<u64, Error> {
    let mut buf = vec![0u8; 256 << 10];
    let mut total = 0u64;
    loop {
        meter.check()?;
        let n = match r.read(&mut buf) {
            Ok(0) => return Ok(total),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted && !archive::is_stop(&e) => continue,
            // Reading the archive: damaged, unless the user stopped.
            Err(e) if archive::is_stop(&e) => return Err(Error::Stopped),
            Err(e) => return Err(Error::Corrupt(e.to_string())),
        };
        // Writing: the disk is full, or gone.
        w.write_all(&buf[..n]).map_err(|e| Error::Io(e.to_string()))?;
        total += n as u64;
        meter.done.fetch_add(n as u64, Ordering::Relaxed);
    }
}

/// The relative path of an entry once the folder being shown is left out;
/// None for an entry outside that folder.
fn relative<'a>(path: &'a str, strip: &str) -> Option<&'a str> {
    if strip.is_empty() {
        return Some(path);
    }
    path.strip_prefix(strip)?.strip_prefix('/')
}

/// Extracts the job's entries; errors of single entries go in the report.
pub fn extract(path: &Path, listing: &Listing, job: &Job, meter: &Meter) -> Result<Report, Error> {
    let mut report = Report::default();
    let wanted = |i: usize| job.wanted.as_ref().is_none_or(|w| w.get(i).copied().unwrap_or(false));
    fs::create_dir_all(&job.dest).map_err(|e| Error::Io(format!("{}: {e}", job.dest.display())))?;
    // Folders first, so that empty ones are there too; links and the rest
    // are counted out.
    for (i, e) in listing.entries.iter().enumerate() {
        if !wanted(i) {
            continue;
        }
        let Some(rel) = relative(&e.path, &job.strip) else { continue };
        match e.kind {
            Kind::Dir => {
                if let Some(p) = safe_path(&job.dest, rel) {
                    if fs::create_dir_all(&p).is_ok() {
                        report.dirs += 1;
                    } else {
                        report.errors.push((e.path.clone(), Error::Io(format!("{}", p.display()))));
                    }
                }
            }
            Kind::Link | Kind::Other => report.left_out += 1,
            Kind::File => {}
        }
    }
    let wanted_here = |i: usize| wanted(i) && relative(&listing.entries[i].path, &job.strip).is_some();
    let result = archive::each(path, listing, &wanted_here, meter, &mut |i, contents| {
        let e = &listing.entries[i];
        meter.set_current(&e.path);
        let rel = relative(&e.path, &job.strip).unwrap_or(&e.path);
        let Some(mut target) = safe_path(&job.dest, rel) else {
            report.left_out += 1;
            return Ok(());
        };
        let r = match contents {
            Ok(r) => r,
            Err(Error::Stopped) => return Err(Error::Stopped),
            Err(why) => {
                report.errors.push((e.path.clone(), why));
                return Ok(());
            }
        };
        if fs::symlink_metadata(&target).is_ok() {
            match job.overwrite {
                Overwrite::Skip => {
                    report.skipped += 1;
                    meter.done.fetch_add(e.size.unwrap_or(0), Ordering::Relaxed);
                    return Ok(());
                }
                Overwrite::KeepBoth => {
                    target = free_name(&target);
                    report.renamed += 1;
                }
                Overwrite::Replace => {
                    // A folder in the way is not replaced by a file.
                    if target.is_dir() {
                        report.errors.push((e.path.clone(), Error::Io(format!("{}: is a folder", target.display()))));
                        return Ok(());
                    }
                }
            }
        }
        if let Some(parent) = target.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                report.errors.push((e.path.clone(), Error::Io(format!("{}: {err}", parent.display()))));
                return Ok(());
            }
        }
        let mut out = match File::create(&target) {
            Ok(f) => f,
            Err(err) => {
                report.errors.push((e.path.clone(), Error::Io(format!("{}: {err}", target.display()))));
                return Ok(());
            }
        };
        match copy(r, &mut out, meter) {
            Ok(n) => {
                set_mtime(&out, e.mtime);
                #[cfg(unix)]
                if let Some(mode) = e.mode {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = out.set_permissions(fs::Permissions::from_mode(mode & 0o777));
                }
                report.files += 1;
                report.bytes += n;
                meter.entries.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(why) => {
                // Half a file is worse than none.
                drop(out);
                let _ = fs::remove_file(&target);
                match why {
                    // The user stopped, or writing failed (the disk is full or
                    // gone): the next files would fail the same way.
                    Error::Stopped | Error::Io(_) => Err(why),
                    why => {
                        report.errors.push((e.path.clone(), why));
                        Ok(())
                    }
                }
            }
        }
    });
    broken_stream(result, report)
}

/// A compressed tar that breaks cannot be read further: what was done is
/// kept, and the break is one more error.
fn broken_stream(result: Result<(), Error>, mut report: Report) -> Result<Report, Error> {
    match result {
        Ok(()) => Ok(report),
        Err(Error::Corrupt(why)) => {
            report.errors.push((String::new(), Error::Corrupt(why)));
            Ok(report)
        }
        Err(e) => Err(e),
    }
}

/// Reads every file to the end, checking what the format checks (the CRC of
/// zip entries, the checksums of the compressed streams).
pub fn test(path: &Path, listing: &Listing, meter: &Meter) -> Result<Report, Error> {
    let mut report = Report::default();
    for e in &listing.entries {
        match e.kind {
            Kind::Dir => report.dirs += 1,
            Kind::Link | Kind::Other => report.left_out += 1,
            Kind::File => {}
        }
    }
    let result = archive::each(path, listing, &|_| true, meter, &mut |i, contents| {
        let e = &listing.entries[i];
        meter.set_current(&e.path);
        let r = match contents {
            Ok(r) => r,
            Err(Error::Stopped) => return Err(Error::Stopped),
            Err(why) => {
                report.errors.push((e.path.clone(), why));
                return Ok(());
            }
        };
        match copy(r, &mut io::sink(), meter) {
            Ok(n) => {
                if e.size.is_some_and(|s| s != n) && listing.format.container != archive::Container::Single {
                    report.errors.push((e.path.clone(), Error::Corrupt(format!("{n} bytes instead of {}", e.size.unwrap_or(0)))));
                } else {
                    report.files += 1;
                    report.bytes += n;
                }
                meter.entries.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(Error::Stopped) => Err(Error::Stopped),
            Err(why) => {
                report.errors.push((e.path.clone(), why));
                Ok(())
            }
        }
    });
    broken_stream(result, report)
}

/// How one archive of several went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Each {
    pub archive: PathBuf,
    /// Where it was extracted (empty when testing).
    pub dest: PathBuf,
    pub result: Result<Report, Error>,
}

/// Where each of several archives is extracted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EachDest {
    /// Beside it, in a folder named after it unless it brings its own (see
    /// `app::dest_for`).
    Smart,
    /// Into the folder it is in.
    Here,
    /// Beside it, always in a folder named after it.
    Named,
    /// Into this folder, as `Smart` would beside the archive.
    Into(PathBuf),
}

/// Extracts each archive, one after the other; stops at the first stop.
pub fn extract_each(paths: &[PathBuf], to: &EachDest, overwrite: Overwrite, meter: &Meter) -> Vec<Each> {
    let mut out = Vec::new();
    for p in paths {
        let result = archive::open(p, meter).and_then(|listing| {
            let tree = crate::tree::Tree::build(&listing.entries);
            let beside = p.parent().map(Path::to_path_buf).unwrap_or_default();
            let dest = match to {
                EachDest::Smart => crate::app::dest_for(p, &tree),
                EachDest::Here => beside,
                EachDest::Named => beside.join(crate::app::App::stem(p)),
                EachDest::Into(dir) => {
                    let smart = crate::app::dest_for(p, &tree);
                    dir.join(smart.strip_prefix(&beside).unwrap_or(Path::new("")))
                }
            };
            let job = Job { dest: dest.clone(), wanted: None, strip: String::new(), overwrite };
            extract(p, &listing, &job, meter).map(|r| (dest, r))
        });
        let stopped = result == Err(Error::Stopped);
        out.push(match result {
            Ok((dest, r)) => Each { archive: p.clone(), dest, result: Ok(r) },
            Err(e) => Each { archive: p.clone(), dest: PathBuf::new(), result: Err(e) },
        });
        if stopped {
            break;
        }
    }
    out
}

/// Tests each archive, one after the other.
pub fn test_each(paths: &[PathBuf], meter: &Meter) -> Vec<Each> {
    let mut out = Vec::new();
    for p in paths {
        let result = archive::open(p, meter).and_then(|listing| test(p, &listing, meter));
        let stopped = result == Err(Error::Stopped);
        out.push(Each { archive: p.clone(), dest: PathBuf::new(), result });
        if stopped {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests;
