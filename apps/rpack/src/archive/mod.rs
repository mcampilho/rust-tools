//! What is inside an archive, read the same way whatever its format.
//!
//! - [`open`] finds the format by the first bytes (`detect`) and lists the
//!   entries ([`Listing`]).
//! - [`each`] reads the contents of the entries asked for, in the archive's
//!   order: extracting and testing (`crate::extract`) share it.
//!
//! Formats: zip (`zip`), 7z (`sevenz`), tar (`tar`), and files compressed whole with gzip,
//! bzip2, xz, lzma, lzip, zstd, lz4 or brotli (`codec`), alone or around a tar.
//! Work runs on a background thread; a [`Meter`] tells the window how far it
//! got and carries the request to stop.

pub mod codec;
pub mod detect;
mod sevenz;
mod tar;
mod zip;

pub use codec::Codec;

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// How the entries are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    Zip,
    SevenZ,
    Tar,
    /// One file compressed whole (`notes.txt.gz`).
    Single,
}

/// An archive's format: its container, and the compression around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub container: Container,
    pub codec: Option<Codec>,
}

impl Format {
    /// As shown: `ZIP`, `TAR`, `TAR + XZ`, `GZIP`.
    pub fn label(&self) -> String {
        match (self.container, self.codec) {
            (Container::Zip, _) => "ZIP".into(),
            (Container::SevenZ, _) => "7Z".into(),
            (Container::Tar, None) => "TAR".into(),
            (Container::Tar, Some(c)) => format!("TAR + {}", c.label()),
            (Container::Single, Some(c)) => c.label().into(),
            (Container::Single, None) => String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    /// A symbolic or hard link: not extracted.
    Link,
    /// A device, a pipe...: listed, never extracted.
    Other,
}

/// One entry of an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The path as stored, with `/` between the names and no `/` at the
    /// start or the end.
    pub path: String,
    pub kind: Kind,
    /// Unknown for some compressed files (bzip2, brotli).
    pub size: Option<u64>,
    /// The size inside the archive; unknown for the entries of a compressed
    /// tar, which is compressed as a whole.
    pub packed: Option<u64>,
    /// Seconds since 1970 (UTC); 0 when unknown.
    pub mtime: i64,
    /// The compression method, as shown (`Deflate`, `LZMA`, `Store`).
    pub method: String,
    pub encrypted: bool,
    pub crc: Option<u32>,
    /// Unix permissions, when the archive keeps them.
    pub mode: Option<u32>,
    /// Where a link points.
    pub link: Option<String>,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }

    /// The last name of the path.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// What an archive holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing {
    pub format: Format,
    pub entries: Vec<Entry>,
    /// The size of the archive file.
    pub file_size: u64,
    pub comment: String,
}

impl Listing {
    /// Total size of the files, as far as known.
    pub fn size(&self) -> u64 {
        self.entries.iter().filter_map(|e| e.size).sum()
    }

    pub fn files(&self) -> usize {
        self.entries.iter().filter(|e| e.kind == Kind::File).count()
    }
}

/// Why an archive could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The system's reason (the file is gone, in use, no rights...).
    Io(String),
    /// Not an archive that rpack knows.
    Unknown,
    /// Damaged or cut short: what was found wrong.
    Corrupt(String),
    /// A method or a feature rpack does not read.
    Unsupported(String),
    /// Encrypted: needs a password.
    Password,
    /// Stopped by the user.
    Stopped,
}

/// The error a metered reader returns when the user stops. Not
/// `Interrupted`, which readers retry.
#[derive(Debug)]
struct Stop;

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(STOPPED)
    }
}

impl std::error::Error for Stop {}

const STOPPED: &str = "rpack: stopped";

/// Whether `e` is the error of a reader that the user stopped.
pub(crate) fn is_stop(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|i| i.is::<Stop>()) || e.to_string().contains(STOPPED)
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        if is_stop(&e) {
            return Error::Stopped;
        }
        // The decoders report damaged data in their own words; an archive cut
        // short ends in the middle of something.
        match e.kind() {
            io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput | io::ErrorKind::UnexpectedEof | io::ErrorKind::Other => {
                Error::Corrupt(e.to_string())
            }
            _ => Error::Io(e.to_string()),
        }
    }
}

/// How far the work on an archive got, read by the window while it runs,
/// and the request to stop.
#[derive(Debug, Default)]
pub struct Meter {
    pub cancel: AtomicBool,
    /// Bytes of the archive file read.
    pub read: AtomicU64,
    /// Bytes of contents written (or checked).
    pub done: AtomicU64,
    /// Entries done.
    pub entries: AtomicU64,
    /// What `done` goes up to, when the work learns it as it goes (0: unknown).
    pub total: AtomicU64,
    /// The entry being read.
    pub current: Mutex<String>,
}

impl Meter {
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// An error for the readers to return when the user stopped.
    pub fn check(&self) -> io::Result<()> {
        if self.stopped() {
            Err(io::Error::other(Stop))
        } else {
            Ok(())
        }
    }

    pub fn set_current(&self, s: &str) {
        if let Ok(mut c) = self.current.lock() {
            c.clear();
            c.push_str(s);
        }
    }

    pub fn current(&self) -> String {
        self.current.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

/// Reads the archive file, counting into the meter and stopping when asked.
pub(crate) struct Metered<'m, R> {
    inner: R,
    meter: &'m Meter,
}

impl<'m, R> Metered<'m, R> {
    pub fn new(inner: R, meter: &'m Meter) -> Self {
        Metered { inner, meter }
    }
}

impl<R: Read> Read for Metered<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.meter.check()?;
        let n = self.inner.read(buf)?;
        self.meter.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

impl<R: io::BufRead> io::BufRead for Metered<'_, R> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.meter.check()?;
        self.inner.fill_buf()
    }

    fn consume(&mut self, n: usize) {
        self.meter.read.fetch_add(n as u64, Ordering::Relaxed);
        self.inner.consume(n);
    }
}

impl<R: Seek> Seek for Metered<'_, R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// Reads up to `buf.len()` bytes: fewer only at the end.
pub(crate) fn read_up_to(r: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted && !is_stop(&e) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// The archive file, buffered and metered.
pub(crate) fn input<'m>(path: &Path, meter: &'m Meter) -> io::Result<Metered<'m, BufReader<File>>> {
    Ok(Metered::new(BufReader::with_capacity(256 << 10, File::open(path)?), meter))
}

/// Lists what the archive at `path` holds.
pub fn open(path: &Path, meter: &Meter) -> Result<Listing, Error> {
    // Whatever a decoder made of the stop, it is a stop.
    open_inner(path, meter).map_err(|e| if meter.stopped() { Error::Stopped } else { e })
}

fn open_inner(path: &Path, meter: &Meter) -> Result<Listing, Error> {
    let mut f = File::open(path)?;
    let file_size = f.metadata()?.len();
    let mut head = [0u8; 1024];
    let n = read_up_to(&mut f, &mut head)?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut listing = match detect::sniff(&head[..n], &name) {
        Some(detect::Found::Zip) => zip::list(path, meter)?,
        Some(detect::Found::SevenZ) => sevenz::list(path, meter)?,
        Some(detect::Found::Tar) => tar::list(path, None, meter)?,
        Some(detect::Found::Codec(c)) => {
            // A tar inside, or one file.
            let mut d = codec::decoder(c, BufReader::new(File::open(path)?))?;
            let mut inner = [0u8; 512];
            let tar_inside = read_up_to(&mut d, &mut inner).is_ok_and(|k| detect::is_tar(&inner[..k]));
            drop(d);
            if tar_inside {
                tar::list(path, Some(c), meter)?
            } else {
                single(path, c, &mut f, file_size)?
            }
        }
        // A zip after something else (a self-extracting program), or nothing known.
        None => zip::list(path, meter).map_err(|e| if matches!(e, Error::Stopped | Error::Io(_)) { e } else { Error::Unknown })?,
    };
    listing.file_size = file_size;
    Ok(listing)
}

/// A file compressed whole: one entry, named after the archive.
fn single(path: &Path, c: Codec, f: &mut File, file_size: u64) -> Result<Listing, Error> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let entry = Entry {
        path: codec::inner_name(&name, c),
        kind: Kind::File,
        size: codec::size(c, f, file_size),
        packed: Some(file_size),
        mtime: std::fs::metadata(path).ok().and_then(|m| m.modified().ok()).map_or(0, unix_secs),
        method: c.label().to_string(),
        encrypted: false,
        crc: None,
        mode: None,
        link: None,
    };
    Ok(Listing { format: Format { container: Container::Single, codec: Some(c) }, entries: vec![entry], file_size, comment: String::new() })
}

pub(crate) fn unix_secs(t: std::time::SystemTime) -> i64 {
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// What [`each`] hands over for an entry: its contents, or why they cannot
/// be read.
pub type Contents<'a> = Result<&'a mut dyn Read, Error>;

/// Reads the entries for which `wanted` is true (files only), in the
/// archive's order, and hands each to `f` with its index in the listing.
/// An error from `f` stops the reading and is returned (the user stopped, the
/// disk is full); `f` keeps the errors of single entries itself.
pub fn each(path: &Path, listing: &Listing, wanted: &dyn Fn(usize) -> bool, meter: &Meter, f: &mut dyn FnMut(usize, Contents) -> Result<(), Error>) -> Result<(), Error> {
    each_inner(path, listing, wanted, meter, f).map_err(|e| if meter.stopped() { Error::Stopped } else { e })
}

fn each_inner(path: &Path, listing: &Listing, wanted: &dyn Fn(usize) -> bool, meter: &Meter, f: &mut dyn FnMut(usize, Contents) -> Result<(), Error>) -> Result<(), Error> {
    let wanted_file = |i: usize| listing.entries.get(i).is_some_and(|e| e.kind == Kind::File) && wanted(i);
    match listing.format.container {
        Container::Zip => zip::each(path, listing, &wanted_file, meter, f),
        Container::SevenZ => sevenz::each(path, listing, &wanted_file, meter, f),
        Container::Tar => tar::each(path, listing, &wanted_file, meter, f),
        Container::Single => {
            if !wanted_file(0) {
                return Ok(());
            }
            let c = listing.format.codec.ok_or(Error::Unknown)?;
            let mut d = codec::decoder(c, input(path, meter)?)?;
            f(0, Ok(&mut d))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
