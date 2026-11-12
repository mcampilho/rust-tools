//! 7z archives (`sevenz-rust2`): LZMA, LZMA2, BZip2, PPMd, the BCJ filters,
//! solid blocks. A solid block is read from its start, so extracting one file
//! of it reads the files before it in the block too.

use super::{input, unix_secs, Contents, Entry, Error, Format, Kind, Listing, Meter};
use sevenz_rust2::{ArchiveReader, EntryDecision, Password};
use std::collections::HashMap;
use std::path::Path;

impl From<sevenz_rust2::Error> for Error {
    fn from(e: sevenz_rust2::Error) -> Error {
        use sevenz_rust2::Error as E;
        match e {
            E::PasswordRequired | E::MaybeBadPassword(_) => Error::Password,
            E::Io(e, _) | E::FileOpen(e, _) => e.into(),
            E::UnsupportedCompressionMethod(m) => Error::Unsupported(m),
            E::Unsupported(m) => Error::Unsupported(m.into_owned()),
            e => Error::Corrupt(e.to_string()),
        }
    }
}

fn clean_path(name: &str) -> String {
    name.replace('\\', "/").trim_start_matches("./").trim_matches('/').to_string()
}

pub fn list(path: &Path, meter: &Meter) -> Result<Listing, Error> {
    let reader = ArchiveReader::new(input(path, meter)?, Password::empty())?;
    let archive = reader.archive();
    let mut entries = Vec::with_capacity(archive.files.len());
    for (i, f) in archive.files.iter().enumerate() {
        meter.check()?;
        let mut methods = Vec::new();
        let method = if f.has_stream && reader.file_compression_methods(f.name(), &mut methods).is_ok() {
            methods.iter().map(|m| m.name()).collect::<Vec<_>>().join(" ")
        } else {
            String::new()
        };
        entries.push(Entry {
            path: clean_path(f.name()),
            kind: if f.is_directory() { Kind::Dir } else { Kind::File },
            size: Some(f.size()),
            // The packed size of a file in a solid block is not its own.
            packed: (!archive.is_solid).then_some(f.compressed_size),
            mtime: if f.has_last_modified_date { unix_secs(f.last_modified_date().into()) } else { 0 },
            method,
            encrypted: archive.is_file_encrypted(i),
            crc: f.has_crc.then_some(f.crc as u32),
            mode: None,
            link: None,
        });
    }
    Ok(Listing { format: Format { container: super::Container::SevenZ, codec: None }, entries, file_size: 0, comment: String::new() })
}

pub fn each(path: &Path, listing: &Listing, wanted: &dyn Fn(usize) -> bool, meter: &Meter, f: &mut dyn FnMut(usize, Contents) -> Result<(), Error>) -> Result<(), Error> {
    let mut reader = ArchiveReader::new(input(path, meter)?, Password::empty())?;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    reader.set_thread_count(threads);
    // The entries come by name, in the archive's order; the same name twice
    // takes the indexes in turn.
    let mut by_name: HashMap<String, std::collections::VecDeque<usize>> = HashMap::new();
    for (i, e) in listing.entries.iter().enumerate() {
        by_name.entry(e.path.clone()).or_default().push_back(i);
    }
    let left = std::cell::Cell::new((0..listing.entries.len()).filter(|&i| wanted(i)).count());
    let mut failed: Option<Error> = None;
    let result = reader.for_each_entries_with_decision(|entry, r| {
        if left.get() == 0 {
            return Ok(EntryDecision::Stop);
        }
        let Some(i) = by_name.get_mut(&clean_path(entry.name())).and_then(|q| q.pop_front()) else { return Ok(EntryDecision::Continue) };
        if !wanted(i) {
            return Ok(EntryDecision::Continue);
        }
        left.set(left.get() - 1);
        if let Err(e) = f(i, Ok(r)) {
            failed = Some(e);
            return Ok(EntryDecision::Stop);
        }
        Ok(EntryDecision::Continue)
    });
    if let Some(e) = failed {
        return Err(e);
    }
    result.map_err(Error::from)
}
