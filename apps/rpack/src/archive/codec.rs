//! Streams compressed whole: gzip, bzip2, xz, lzma, lzip, zstd, lz4 and
//! brotli. Each is read through a decoder; the size of the contents is read
//! from the stream's own header or trailer, when it keeps one.

use super::Error;
use std::fs::File;
use std::io::{BufRead, Read, Seek, SeekFrom};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Codec {
    Gzip,
    Bzip2,
    Xz,
    Lzma,
    Lzip,
    Zstd,
    Lz4,
    Brotli,
}

impl Codec {
    pub const ALL: [Codec; 8] = [Codec::Gzip, Codec::Bzip2, Codec::Xz, Codec::Lzma, Codec::Lzip, Codec::Zstd, Codec::Lz4, Codec::Brotli];

    pub fn label(self) -> &'static str {
        match self {
            Codec::Gzip => "GZIP",
            Codec::Bzip2 => "BZIP2",
            Codec::Xz => "XZ",
            Codec::Lzma => "LZMA",
            Codec::Lzip => "LZIP",
            Codec::Zstd => "ZSTD",
            Codec::Lz4 => "LZ4",
            Codec::Brotli => "BROTLI",
        }
    }

    /// The extensions of a file compressed this way, and of a tar compressed
    /// this way (`tgz`).
    pub fn extensions(self) -> (&'static [&'static str], &'static [&'static str]) {
        match self {
            Codec::Gzip => (&["gz", "gzip"], &["tgz", "taz"]),
            Codec::Bzip2 => (&["bz2", "bz", "bzip2"], &["tbz2", "tbz", "tb2"]),
            Codec::Xz => (&["xz"], &["txz"]),
            Codec::Lzma => (&["lzma"], &["tlzma"]),
            Codec::Lzip => (&["lz"], &["tlz"]),
            Codec::Zstd => (&["zst", "zstd"], &["tzst", "tzstd"]),
            Codec::Lz4 => (&["lz4"], &["tlz4"]),
            Codec::Brotli => (&["br"], &["tbr"]),
        }
    }
}

/// The decoder of a stream; concatenated streams (`cat a.gz b.gz`) are read
/// as one, as their own tools do.
pub fn decoder<'a, R: BufRead + 'a>(c: Codec, r: R) -> Result<Box<dyn Read + 'a>, Error> {
    Ok(match c {
        Codec::Gzip => Box::new(flate2::bufread::MultiGzDecoder::new(r)),
        Codec::Bzip2 => Box::new(bzip2::bufread::MultiBzDecoder::new(r)),
        Codec::Xz => Box::new(lzma_rust2::XzReader::new(r, true)),
        Codec::Lzma => Box::new(lzma_rust2::LzmaReader::new_mem_limit(r, u32::MAX, None)?),
        Codec::Lzip => Box::new(lzma_rust2::LzipReader::new(r)),
        Codec::Zstd => {
            let mut d = zstd::stream::read::Decoder::with_buffer(r)?;
            // Streams made with `--long` use windows up to 2 GB.
            d.window_log_max(31)?;
            Box::new(d)
        }
        Codec::Lz4 => Box::new(lz4_flex::frame::FrameDecoder::new(r)),
        Codec::Brotli => Box::new(brotli::Decompressor::new(r, 64 << 10)),
    })
}

/// The name of the file inside, from the archive's: `notes.txt.gz` →
/// `notes.txt`.
pub fn inner_name(archive: &str, c: Codec) -> String {
    let lower = archive.to_lowercase();
    for ext in c.extensions().0 {
        let dot = format!(".{ext}");
        if lower.ends_with(&dot) && lower.len() > dot.len() {
            return archive[..archive.len() - dot.len()].to_string();
        }
    }
    // `notes.gzip-copy`: a name of its own.
    match archive.rfind('.') {
        Some(i) if i > 0 => archive[..i].to_string(),
        _ => format!("{archive}.out"),
    }
}

fn read_at(f: &mut File, pos: u64, buf: &mut [u8]) -> Option<()> {
    f.seek(SeekFrom::Start(pos)).ok()?;
    f.read_exact(buf).ok()
}

fn u64_le(b: &[u8]) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[..8]);
    u64::from_le_bytes(a)
}

/// The size of the contents, when the stream keeps it.
pub fn size(c: Codec, f: &mut File, file_size: u64) -> Option<u64> {
    match c {
        // The last 4 bytes: the size modulo 4 GB (of the last member only).
        Codec::Gzip if file_size >= 18 => {
            let mut b = [0u8; 4];
            read_at(f, file_size - 4, &mut b)?;
            Some(u32::from_le_bytes(b) as u64)
        }
        Codec::Xz => {
            f.seek(SeekFrom::Start(0)).ok()?;
            lzma_rust2::xz_uncompressed_size(&mut *f).ok()
        }
        Codec::Lzma => {
            let mut b = [0u8; 13];
            read_at(f, 0, &mut b)?;
            let n = u64_le(&b[5..]);
            (n != u64::MAX).then_some(n)
        }
        Codec::Lzip => lzip_size(f, file_size),
        Codec::Zstd => {
            let mut b = [0u8; 18];
            let n = {
                f.seek(SeekFrom::Start(0)).ok()?;
                super::read_up_to(f, &mut b).ok()?
            };
            zstd::zstd_safe::get_frame_content_size(&b[..n]).ok().flatten()
        }
        Codec::Lz4 => {
            // The frame descriptor: a flag for the content size, then the size.
            let mut b = [0u8; 14];
            read_at(f, 0, &mut b)?;
            (b[4] & 0x08 != 0).then(|| u64_le(&b[6..]))
        }
        _ => None,
    }
}

/// Lzip members end with their data size and member size: read from the end.
fn lzip_size(f: &mut File, file_size: u64) -> Option<u64> {
    let (mut end, mut total) = (file_size, 0u64);
    while end > 0 {
        if end < 36 {
            return None;
        }
        let mut t = [0u8; 16];
        read_at(f, end - 16, &mut t)?;
        let (data, member) = (u64_le(&t[..8]), u64_le(&t[8..]));
        if member < 36 || member > end {
            return None;
        }
        total = total.checked_add(data)?;
        end -= member;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inner_names() {
        assert_eq!(inner_name("notes.txt.gz", Codec::Gzip), "notes.txt");
        assert_eq!(inner_name("Notes.TXT.GZ", Codec::Gzip), "Notes.TXT");
        assert_eq!(inner_name("data.zst", Codec::Zstd), "data");
        assert_eq!(inner_name("data.copy", Codec::Xz), "data");
        assert_eq!(inner_name("data", Codec::Xz), "data.out");
        assert_eq!(inner_name(".gz", Codec::Gzip), ".gz.out");
    }
}
