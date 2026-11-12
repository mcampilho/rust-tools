//! Creating archives from files and folders on disk: 7z (LZMA2, solid),
//! zip (Deflate, Zstandard, XZ, BZip2 or stored), tar alone or compressed
//! whole, and one file compressed alone (`.gz`, `.xz`, `.zst`...).
//!
//! The archive is written under a temporary name beside the place it goes,
//! and takes its name only when it is complete: stopping, or an error,
//! leaves nothing behind.

use crate::archive::{Codec, Error, Meter};
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// What kind of archive to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    SevenZ,
    Zip,
    /// A tar, alone or compressed whole.
    Tar(Option<Codec>),
    /// One file compressed alone.
    Single(Codec),
}

impl Kind {
    /// The kinds offered, in order.
    pub const ALL: [Kind; 14] = [
        Kind::SevenZ,
        Kind::Zip,
        Kind::Tar(None),
        Kind::Tar(Some(Codec::Gzip)),
        Kind::Tar(Some(Codec::Xz)),
        Kind::Tar(Some(Codec::Zstd)),
        Kind::Tar(Some(Codec::Bzip2)),
        Kind::Tar(Some(Codec::Lz4)),
        Kind::Tar(Some(Codec::Lzip)),
        Kind::Single(Codec::Gzip),
        Kind::Single(Codec::Xz),
        Kind::Single(Codec::Zstd),
        Kind::Single(Codec::Bzip2),
        Kind::Single(Codec::Lz4),
    ];

    /// The extension of the archive: `7z`, `zip`, `tar.gz`, `gz`.
    pub fn ext(self) -> String {
        match self {
            Kind::SevenZ => "7z".into(),
            Kind::Zip => "zip".into(),
            Kind::Tar(None) => "tar".into(),
            Kind::Tar(Some(c)) => format!("tar.{}", c.extensions().0[0]),
            Kind::Single(c) => c.extensions().0[0].into(),
        }
    }

    /// Only one file can be compressed alone.
    pub fn needs_one_file(self) -> bool {
        matches!(self, Kind::Single(_))
    }

    /// Whether the level means something for this kind (lz4 has one speed).
    pub fn has_levels(self) -> bool {
        !matches!(self, Kind::Tar(None) | Kind::Tar(Some(Codec::Lz4)) | Kind::Single(Codec::Lz4))
    }

    /// From the key kept in the settings (the extension).
    pub fn from_key(k: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.ext() == k)
    }
}

/// How the files of a zip are compressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ZipMethod {
    /// Every program opens it.
    #[default]
    Deflate,
    Zstd,
    Xz,
    Bzip2,
}

impl ZipMethod {
    pub const ALL: [ZipMethod; 4] = [ZipMethod::Deflate, ZipMethod::Zstd, ZipMethod::Xz, ZipMethod::Bzip2];

    pub fn label(self) -> &'static str {
        match self {
            ZipMethod::Deflate => "Deflate",
            ZipMethod::Zstd => "Zstandard",
            ZipMethod::Xz => "XZ",
            ZipMethod::Bzip2 => "BZip2",
        }
    }
}

/// How hard to compress, as 7-Zip names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Level {
    Store,
    Fastest,
    Fast,
    #[default]
    Normal,
    Maximum,
    Ultra,
}

impl Level {
    pub const ALL: [Level; 6] = [Level::Store, Level::Fastest, Level::Fast, Level::Normal, Level::Maximum, Level::Ultra];

    /// On the scale of 0 to 9 of gzip, xz and 7-Zip.
    pub fn nine(self) -> u32 {
        match self {
            Level::Store => 0,
            Level::Fastest => 1,
            Level::Fast => 3,
            Level::Normal => 5,
            Level::Maximum => 7,
            Level::Ultra => 9,
        }
    }

    /// Zstandard's own scale (1 to 22).
    fn zstd(self) -> i32 {
        match self {
            Level::Store | Level::Fastest => 1,
            Level::Fast => 3,
            Level::Normal => 6,
            Level::Maximum => 15,
            Level::Ultra => 19,
        }
    }

}

/// What to make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    pub kind: Kind,
    pub level: Level,
    pub zip_method: ZipMethod,
    /// 7z: files compressed together in blocks (smaller archives).
    pub solid: bool,
}

impl Default for Spec {
    fn default() -> Spec {
        Spec { kind: Kind::SevenZ, level: Level::Normal, zip_method: ZipMethod::Deflate, solid: true }
    }
}

/// A file or folder to put in the archive, and its name there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    /// With `/` between the names.
    pub name: String,
    pub dir: bool,
    pub size: u64,
    pub mtime: i64,
}

/// Everything below the paths chosen, each named from the folder it is in:
/// choosing `D:\a\photos` puts `photos/...` in the archive. Links are left
/// out (their count is returned).
pub fn gather(paths: &[PathBuf]) -> (Vec<Item>, u64) {
    let mut out = Vec::new();
    let mut links = 0;
    for p in paths {
        let base = p.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut stack = vec![p.clone()];
        while let Some(q) = stack.pop() {
            let Ok(m) = fs::symlink_metadata(&q) else { continue };
            let name = q.strip_prefix(&base).unwrap_or(&q).to_string_lossy().replace('\\', "/");
            let mtime = m.modified().map_or(0, crate::archive::unix_secs);
            if m.file_type().is_symlink() {
                links += 1;
            } else if m.is_dir() {
                out.push(Item { path: q.clone(), name, dir: true, size: 0, mtime });
                if let Ok(rd) = fs::read_dir(&q) {
                    let mut kids: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
                    kids.sort();
                    stack.extend(kids.into_iter().rev());
                }
            } else if m.is_file() {
                out.push(Item { path: q, name, dir: false, size: m.len(), mtime });
            }
        }
    }
    (out, links)
}

/// The archive's name for the paths chosen: the folder or file's own name
/// for one (`photos.7z`, `notes.txt.gz`), the folder they are in for several.
pub fn suggested_name(paths: &[PathBuf], kind: Kind) -> String {
    let stem = match paths {
        [one] => {
            let name = one.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if one.is_dir() || kind.needs_one_file() {
                name
            } else {
                // `report.pdf` → `report.7z`; a name with no extension stays.
                match name.rfind('.') {
                    Some(i) if i > 0 => name[..i].to_string(),
                    _ => name,
                }
            }
        }
        _ => paths.first().and_then(|p| p.parent()).and_then(|d| d.file_name()).map_or("archive".into(), |n| n.to_string_lossy().into_owned()),
    };
    format!("{stem}.{}", kind.ext())
}

/// How it went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Made {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    /// The size of the archive.
    pub size: u64,
    pub links: u64,
    /// Files that could not be read, and were left out: (path, why).
    pub skipped: Vec<(String, String)>,
}

/// Reads a file to put in, counting into the meter (done) and stopping when asked.
struct Counted<'m, R> {
    inner: R,
    meter: &'m Meter,
}

impl<R: Read> Read for Counted<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.meter.check()?;
        let n = self.inner.read(buf)?;
        self.meter.done.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// Opens a file to put in; why it cannot be read goes in the report.
fn open_item<'m>(it: &Item, meter: &'m Meter, made: &mut Made) -> Option<Counted<'m, File>> {
    meter.set_current(&it.name);
    match File::open(&it.path) {
        Ok(f) => Some(Counted { inner: f, meter }),
        Err(e) => {
            made.skipped.push((it.name.clone(), e.to_string()));
            // Counted as done, so that the progress still reaches the end.
            meter.done.fetch_add(it.size, Ordering::Relaxed);
            None
        }
    }
}

/// The temporary name beside `dest`.
fn temp_of(dest: &Path) -> PathBuf {
    let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    dest.with_file_name(format!(".{name}.rpack-part"))
}

/// Makes the archive `dest` of `paths`.
pub fn create(paths: &[PathBuf], dest: &Path, spec: &Spec, meter: &Meter) -> Result<Made, Error> {
    let (items, links) = gather(paths);
    let tmp = temp_of(dest);
    let result = write_to(&items, &tmp, spec, meter);
    match result {
        Ok(mut made) => {
            if let Err(e) = fs::rename(&tmp, dest) {
                let _ = fs::remove_file(&tmp);
                return Err(Error::Io(format!("{}: {e}", dest.display())));
            }
            made.links = links;
            made.size = fs::metadata(dest).map_or(0, |m| m.len());
            Ok(made)
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(if meter.stopped() { Error::Stopped } else { e })
        }
    }
}

fn write_err(e: io::Error) -> Error {
    if crate::archive::is_stop(&e) {
        Error::Stopped
    } else {
        Error::Io(e.to_string())
    }
}

fn write_to(items: &[Item], tmp: &Path, spec: &Spec, meter: &Meter) -> Result<Made, Error> {
    let file = File::create(tmp).map_err(|e| Error::Io(format!("{}: {e}", tmp.display())))?;
    let mut made = Made::default();
    match spec.kind {
        Kind::SevenZ => sevenz(items, file, spec, meter, &mut made)?,
        Kind::Zip => zip(items, file, spec, meter, &mut made)?,
        Kind::Tar(codec) => {
            let out = BufWriter::with_capacity(1 << 20, file);
            let mut w = encoder(codec, spec.level, out)?;
            tar(items, &mut w, meter, &mut made)?;
            w.finish().map_err(write_err)?;
        }
        Kind::Single(c) => {
            let files: Vec<&Item> = items.iter().filter(|i| !i.dir).collect();
            let [it] = files[..] else { return Err(Error::Unsupported("one file".into())) };
            let mut w = encoder(Some(c), spec.level, BufWriter::with_capacity(1 << 20, file))?;
            if let Some(mut r) = open_item(it, meter, &mut made) {
                made.bytes += io::copy(&mut r, &mut w).map_err(write_err)?;
                made.files += 1;
            }
            w.finish().map_err(write_err)?;
        }
    }
    Ok(made)
}

/// A compressed stream to write into, finished explicitly (each codec ends
/// its stream its own way).
pub trait Finish: Write {
    fn finish(self: Box<Self>) -> io::Result<()>;
}

struct Plain<W: Write>(W);
impl<W: Write> Write for Plain<W> {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
impl<W: Write> Finish for Plain<W> {
    fn finish(mut self: Box<Self>) -> io::Result<()> {
        self.0.flush()
    }
}

macro_rules! finisher {
    ($name:ident, $t:ty, $done:expr) => {
        struct $name<W: Write>($t);
        impl<W: Write> Write for $name<W> {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                self.0.write(b)
            }
            fn flush(&mut self) -> io::Result<()> {
                self.0.flush()
            }
        }
        impl<W: Write> Finish for $name<W> {
            fn finish(self: Box<Self>) -> io::Result<()> {
                let f: fn($t) -> io::Result<()> = $done;
                f(self.0)
            }
        }
    };
}

finisher!(Gz, flate2::write::GzEncoder<W>, |e| e.finish().and_then(|mut w| w.flush()));
finisher!(Bz, bzip2::write::BzEncoder<W>, |e| e.finish().and_then(|mut w| w.flush()));
finisher!(Xz, lzma_rust2::XzWriter<W>, |e| e.finish().map_err(io::Error::other).and_then(|mut w| w.flush()));
finisher!(XzMt, lzma_rust2::XzWriterMt<W>, |e| e.finish().map_err(io::Error::other).and_then(|mut w| w.flush()));
finisher!(Lz, lzma_rust2::LzipWriter<W>, |e| e.finish().map_err(io::Error::other).and_then(|mut w| w.flush()));
finisher!(Zst, zstd::Encoder<'static, W>, |e| e.finish().and_then(|mut w| w.flush()));
finisher!(L4, lz4_flex::frame::FrameEncoder<W>, |e| e.finish().map_err(io::Error::other).and_then(|mut w| w.flush()));


fn threads() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
}

/// The stream of a codec (or none) over `out`.
fn encoder<W: Write + Send + 'static>(c: Option<Codec>, level: Level, out: W) -> Result<Box<dyn Finish>, Error> {
    let n = level.nine();
    Ok(match c {
        None => Box::new(Plain(out)),
        Some(Codec::Gzip) => Box::new(Gz(flate2::write::GzEncoder::new(out, flate2::Compression::new(n.max(1))))),
        Some(Codec::Bzip2) => Box::new(Bz(bzip2::write::BzEncoder::new(out, bzip2::Compression::new(n.clamp(1, 9))))),
        Some(Codec::Xz) => {
            let mut opts = lzma_rust2::XzOptions::with_preset(n);
            if threads() > 1 {
                // Blocks compressed side by side: three times the dictionary, as xz does.
                let dict = opts.lzma_options.dict_size as u64;
                opts.set_block_size(std::num::NonZeroU64::new((dict * 3).max(1 << 20)));
                Box::new(XzMt(lzma_rust2::XzWriterMt::new(out, opts, threads()).map_err(|e| Error::Io(e.to_string()))?))
            } else {
                Box::new(Xz(lzma_rust2::XzWriter::new(out, opts).map_err(|e| Error::Io(e.to_string()))?))
            }
        }
        Some(Codec::Lzip) => Box::new(Lz(lzma_rust2::LzipWriter::new(out, lzma_rust2::LzipOptions::with_preset(n)))),
        Some(Codec::Lzma) => return Err(Error::Unsupported("lzma".into())),
        Some(Codec::Zstd) => {
            let mut e = zstd::Encoder::new(out, level.zstd()).map_err(|e| Error::Io(e.to_string()))?;
            let _ = e.include_checksum(true);
            Box::new(Zst(e))
        }
        Some(Codec::Lz4) => Box::new(L4(lz4_flex::frame::FrameEncoder::new(out))),
        // Reading brotli stays; writing it would add 1 MB to the program.
        Some(Codec::Brotli) => return Err(Error::Unsupported("brotli".into())),
    })
}

fn tar(items: &[Item], w: &mut dyn Write, meter: &Meter, made: &mut Made) -> Result<(), Error> {
    let mut b = ::tar::Builder::new(w);
    b.mode(::tar::HeaderMode::Complete);
    for it in items {
        meter.check()?;
        let mut h = ::tar::Header::new_gnu();
        h.set_mtime(it.mtime.max(0) as u64);
        if it.dir {
            h.set_entry_type(::tar::EntryType::Directory);
            h.set_mode(0o755);
            h.set_size(0);
            b.append_data(&mut h, format!("{}/", it.name), io::empty()).map_err(write_err)?;
            made.dirs += 1;
            continue;
        }
        let Some(r) = open_item(it, meter, made) else { continue };
        h.set_entry_type(::tar::EntryType::Regular);
        h.set_mode(file_mode(&it.path));
        h.set_size(it.size);
        // A file that changed size while being read would break the tar.
        let mut r = r.take(it.size);
        b.append_data(&mut h, &it.name, &mut r).map_err(write_err)?;
        made.files += 1;
        made.bytes += it.size;
    }
    b.finish().map_err(write_err)?;
    Ok(())
}

/// The permissions to keep: the file's own on Linux, 644 elsewhere.
fn file_mode(p: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = fs::metadata(p) {
            return m.permissions().mode() & 0o777;
        }
    }
    let _ = p;
    0o644
}

/// A date for zip (local time, from 1980).
fn zip_time(secs: i64) -> Option<::zip::DateTime> {
    let local = secs + rt_scan::format::day_start(1970, 1, 1).map_or(0, |d| -d);
    let (y, m, d) = rt_scan::format::civil(local.div_euclid(86_400) as i32);
    let t = local.rem_euclid(86_400);
    ::zip::DateTime::from_date_and_time(y.clamp(1980, 2107) as u16, m as u8, d as u8, (t / 3600) as u8, (t / 60 % 60) as u8, (t % 60) as u8).ok()
}

fn zip<W: Write + Seek>(items: &[Item], out: W, spec: &Spec, meter: &Meter, made: &mut Made) -> Result<(), Error> {
    use ::zip::write::SimpleFileOptions;
    use ::zip::CompressionMethod as M;
    let mut z = ::zip::ZipWriter::new(io::BufWriter::with_capacity(1 << 20, out));
    let (method, level) = match (spec.level, spec.zip_method) {
        (Level::Store, _) => (M::Stored, None),
        (l, ZipMethod::Deflate) => (M::Deflated, Some(l.nine().max(1) as i64)),
        (l, ZipMethod::Zstd) => (M::Zstd, Some(l.zstd() as i64)),
        (l, ZipMethod::Xz) => (M::Xz, Some(l.nine() as i64)),
        (l, ZipMethod::Bzip2) => (M::Bzip2, Some(l.nine().clamp(1, 9) as i64)),
    };
    for it in items {
        meter.check()?;
        let mut opts = SimpleFileOptions::default().compression_method(method).compression_level(level).large_file(it.size >= u32::MAX as u64);
        if let Some(t) = zip_time(it.mtime) {
            opts = opts.last_modified_time(t);
        }
        #[cfg(unix)]
        {
            opts = opts.unix_permissions(if it.dir { 0o755 } else { file_mode(&it.path) });
        }
        if it.dir {
            z.add_directory(format!("{}/", it.name), opts).map_err(|e| Error::Io(e.to_string()))?;
            made.dirs += 1;
            continue;
        }
        let Some(mut r) = open_item(it, meter, made) else { continue };
        // Empty files are stored: there is nothing to compress.
        let opts = if it.size == 0 { opts.compression_method(M::Stored).compression_level(None) } else { opts };
        z.start_file(it.name.as_str(), opts).map_err(|e| Error::Io(e.to_string()))?;
        made.bytes += io::copy(&mut r, &mut z).map_err(write_err)?;
        made.files += 1;
    }
    let mut w = z.finish().map_err(|e| Error::Io(e.to_string()))?;
    w.flush().map_err(write_err)?;
    Ok(())
}

fn sevenz(items: &[Item], out: File, spec: &Spec, meter: &Meter, made: &mut Made) -> Result<(), Error> {
    use sevenz_rust2::encoder_options::Lzma2Options;
    use sevenz_rust2::{ArchiveEntry, ArchiveWriter, EncoderConfiguration, EncoderMethod, SourceReader};
    let mut w = ArchiveWriter::new(out).map_err(Error::from)?;
    let method = if spec.level == Level::Store {
        EncoderConfiguration::new(EncoderMethod::COPY)
    } else {
        let n = threads();
        let opts = if n > 1 { Lzma2Options::from_level_mt(spec.level.nine(), n, 64 << 20) } else { Lzma2Options::from_level(spec.level.nine()) };
        EncoderConfiguration::new(EncoderMethod::LZMA2).with_options(opts.into())
    };
    w.set_content_methods(vec![method]);
    // Solid: the files go in blocks of up to this much, read one after the other.
    const BLOCK: u64 = 256 << 20;
    let mut block: Vec<(ArchiveEntry, &Item)> = Vec::new();
    let mut in_block = 0u64;
    let flush = |w: &mut ArchiveWriter<File>, block: &mut Vec<(ArchiveEntry, &Item)>, made: &mut Made| -> Result<(), Error> {
        if block.is_empty() {
            return Ok(());
        }
        let mut entries = Vec::new();
        let mut readers = Vec::new();
        for (e, it) in block.drain(..) {
            // Opened one by one as the block is read (a few at once at most).
            readers.push(SourceReader::new(Lazy { path: it.path.clone(), file: None, meter, name: it.name.clone() }));
            entries.push(e);
            made.files += 1;
            made.bytes += it.size;
        }
        w.push_archive_entries(entries, readers).map_err(Error::from)?;
        Ok(())
    };
    for it in items {
        meter.check()?;
        let entry = ArchiveEntry::from_path(&it.path, it.name.clone());
        if it.dir {
            w.push_archive_entry(entry, None::<File>).map_err(Error::from)?;
            made.dirs += 1;
            continue;
        }
        if !spec.solid || it.size >= BLOCK {
            let Some(r) = open_item(it, meter, made) else { continue };
            w.push_archive_entry(entry, Some(r)).map_err(Error::from)?;
            made.files += 1;
            made.bytes += it.size;
            continue;
        }
        if in_block + it.size > BLOCK {
            flush(&mut w, &mut block, made)?;
            in_block = 0;
        }
        in_block += it.size;
        block.push((entry, it));
    }
    flush(&mut w, &mut block, made)?;
    w.finish().map_err(write_err)?;
    Ok(())
}

/// A file of a solid block, opened when its turn comes. One that cannot be
/// opened reads as empty (its name is in the stop error if the user stops).
struct Lazy<'m> {
    path: PathBuf,
    file: Option<File>,
    meter: &'m Meter,
    name: String,
}

impl Read for Lazy<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.meter.check()?;
        if self.file.is_none() {
            self.meter.set_current(&self.name);
            self.file = Some(File::open(&self.path)?);
        }
        let n = self.file.as_mut().map_or(Ok(0), |f| f.read(buf))?;
        self.meter.done.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

#[cfg(test)]
mod tests;
