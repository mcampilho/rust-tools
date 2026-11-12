//! Zip archives (`zip` crate): stored, deflate, deflate64, bzip2, lzma, xz,
//! zstd and PPMd, zip64. The entries are read in the order of the central
//! directory, each from its own place in the file.

use super::{input, Contents, Entry, Error, Format, Kind, Listing, Meter};
use std::path::Path;
use zip::result::ZipError;

impl From<ZipError> for Error {
    fn from(e: ZipError) -> Error {
        match e {
            ZipError::Io(e) => e.into(),
            ZipError::InvalidPassword => Error::Password,
            ZipError::UnsupportedArchive(m) if m.to_lowercase().contains("password") => Error::Password,
            ZipError::UnsupportedArchive(m) => Error::Unsupported(m.to_string()),
            ZipError::CompressionMethodNotSupported(m) => Error::Unsupported(method_name(m)),
            e => Error::Corrupt(e.to_string()),
        }
    }
}

/// The method's number in the zip format (the crate names only those it reads).
#[allow(deprecated)]
fn code(m: zip::CompressionMethod) -> u16 {
    m.to_u16()
}

/// The method as shown, by its number in the zip format.
fn method_name(code: u16) -> String {
    match code {
        0 => "Store".into(),
        1 => "Shrink".into(),
        2..=5 => "Reduce".into(),
        6 => "Implode".into(),
        8 => "Deflate".into(),
        9 => "Deflate64".into(),
        12 => "BZip2".into(),
        14 => "LZMA".into(),
        20 | 93 => "ZSTD".into(),
        95 => "XZ".into(),
        96 => "JPEG".into(),
        97 => "WavPack".into(),
        98 => "PPMd".into(),
        99 => "AES".into(),
        n => format!("Method {n}"),
    }
}
/// A DOS date and time (local time) in seconds since 1970 (UTC).
fn dos_time(t: zip::DateTime) -> i64 {
    if !t.is_valid() || t.year() < 1981 {
        return 0;
    }
    let day = rt_scan::format::day_start(t.year() as i32, t.month() as u32, t.day() as u32).unwrap_or(0);
    day + t.hour() as i64 * 3600 + t.minute() as i64 * 60 + t.second() as i64
}

/// Names from old zips are in the system's code page; `zip` reads them as
/// CP437 unless they are marked UTF-8, and backslashes are folders too.
fn clean_path(name: &str) -> String {
    name.replace('\\', "/").trim_start_matches("./").trim_matches('/').to_string()
}

pub fn list(path: &Path, meter: &Meter) -> Result<Listing, Error> {
    let zip = zip::ZipArchive::new(input(path, meter)?)?;
    let mut entries = Vec::with_capacity(zip.len());
    for i in 0..zip.len() {
        meter.check()?;
        let e = zip.by_index_data(i)?;
        let raw = e.name().map(|n| n.into_owned()).unwrap_or_else(|_| String::from_utf8_lossy(e.name_raw()).into_owned());
        let kind = if e.is_dir() {
            Kind::Dir
        } else if e.is_symlink() {
            Kind::Link
        } else {
            Kind::File
        };
        entries.push(Entry {
            path: clean_path(&raw),
            kind,
            size: Some(e.size()),
            packed: Some(e.compressed_size()),
            mtime: e.last_modified().map_or(0, dos_time),
            method: method_name(code(e.compression())),
            encrypted: e.encrypted(),
            crc: (kind == Kind::File).then(|| e.crc32()),
            mode: e.unix_mode(),
            link: None,
        });
    }
    let comment = String::from_utf8_lossy(zip.comment()).trim().to_string();
    Ok(Listing { format: Format { container: super::Container::Zip, codec: None }, entries, file_size: 0, comment })
}

pub fn each(path: &Path, listing: &Listing, wanted: &dyn Fn(usize) -> bool, meter: &Meter, f: &mut dyn FnMut(usize, Contents) -> Result<(), Error>) -> Result<(), Error> {
    let mut zip = zip::ZipArchive::new(input(path, meter)?)?;
    let count = zip.len();
    for (i, entry) in listing.entries.iter().enumerate().take(count) {
        if !wanted(i) {
            continue;
        }
        meter.check()?;
        if entry.encrypted {
            f(i, Err(Error::Password))?;
            continue;
        }
        match zip.by_index(i) {
            Ok(mut file) => f(i, Ok(&mut file))?,
            Err(e) => {
                let e = Error::from(e);
                if e == Error::Stopped {
                    return Err(e);
                }
                f(i, Err(e))?;
            }
        }
    }
    Ok(())
}
