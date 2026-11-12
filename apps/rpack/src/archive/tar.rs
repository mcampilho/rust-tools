//! Tar archives, plain or compressed whole (`.tar.gz`, `.tar.xz`...). A
//! compressed tar can only be read from the start: listing it reads it all,
//! and extracting part of it reads up to the last entry wanted.

use super::{codec, input, Codec, Contents, Entry, Error, Format, Kind, Listing, Meter};
use std::io::Read;
use std::path::Path;
use tar::EntryType;

fn reader<'m>(path: &Path, c: Option<Codec>, meter: &'m Meter) -> Result<Box<dyn Read + 'm>, Error> {
    let file = input(path, meter)?;
    Ok(match c {
        Some(c) => codec::decoder(c, file)?,
        None => Box::new(file),
    })
}

fn clean_path(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    s.trim_start_matches("./").trim_matches('/').to_string()
}

pub fn list(path: &Path, c: Option<Codec>, meter: &Meter) -> Result<Listing, Error> {
    let mut ar = tar::Archive::new(reader(path, c, meter)?);
    let mut entries = Vec::new();
    for e in ar.entries()? {
        let e = e?;
        let h = e.header();
        let kind = match h.entry_type() {
            EntryType::Regular | EntryType::Continuous | EntryType::GNUSparse => Kind::File,
            EntryType::Directory => Kind::Dir,
            EntryType::Symlink | EntryType::Link => Kind::Link,
            _ => Kind::Other,
        };
        let link = e.link_name_bytes().map(|b| String::from_utf8_lossy(&b).into_owned());
        let path = clean_path(&e.path_bytes());
        // `./` alone: the archive's own root.
        if path.is_empty() && kind == Kind::Dir {
            entries.push(Entry { path, kind: Kind::Other, size: Some(0), packed: None, mtime: 0, method: String::new(), encrypted: false, crc: None, mode: None, link: None });
            continue;
        }
        entries.push(Entry {
            path,
            kind,
            size: Some(if kind == Kind::File { e.size() } else { 0 }),
            packed: if c.is_none() && kind == Kind::File { Some(e.size()) } else { None },
            mtime: h.mtime().map_or(0, |t| t as i64),
            method: c.map_or("Store", |c| c.label()).to_string(),
            encrypted: false,
            crc: None,
            mode: h.mode().ok(),
            link,
        });
        meter.entries.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(Listing { format: Format { container: super::Container::Tar, codec: c }, entries, file_size: 0, comment: String::new() })
}

pub fn each(path: &Path, listing: &Listing, wanted: &dyn Fn(usize) -> bool, meter: &Meter, f: &mut dyn FnMut(usize, Contents) -> Result<(), Error>) -> Result<(), Error> {
    let Some(last) = (0..listing.entries.len()).rev().find(|&i| wanted(i)) else { return Ok(()) };
    let mut ar = tar::Archive::new(reader(path, listing.format.codec, meter)?);
    for (i, e) in ar.entries()?.enumerate() {
        let mut e = e?;
        if wanted(i) {
            f(i, Ok(&mut e))?;
        }
        if i >= last {
            break;
        }
    }
    Ok(())
}
