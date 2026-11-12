//! Hashes of files on disk and of the entries of an archive, to check them
//! against the sums published with a download: CRC-32, CRC-64, XXH64, MD5,
//! SHA-1, SHA-2 (256, 384, 512), SHA3-256, BLAKE2sp and BLAKE3.
//!
//! Each file is read once, whatever the number of hashes asked for.

use crate::archive::{self, Error, Kind, Listing, Meter};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Algo {
    Crc32,
    Crc64,
    Xxh64,
    Md5,
    Sha1,
    Sha256,
    Sha384,
    Sha512,
    Sha3_256,
    Blake2sp,
    Blake3,
}

impl Algo {
    pub const ALL: [Algo; 11] =
        [Algo::Crc32, Algo::Crc64, Algo::Xxh64, Algo::Md5, Algo::Sha1, Algo::Sha256, Algo::Sha384, Algo::Sha512, Algo::Sha3_256, Algo::Blake2sp, Algo::Blake3];

    pub fn name(self) -> &'static str {
        match self {
            Algo::Crc32 => "CRC-32",
            Algo::Crc64 => "CRC-64",
            Algo::Xxh64 => "XXH64",
            Algo::Md5 => "MD5",
            Algo::Sha1 => "SHA-1",
            Algo::Sha256 => "SHA-256",
            Algo::Sha384 => "SHA-384",
            Algo::Sha512 => "SHA-512",
            Algo::Sha3_256 => "SHA3-256",
            Algo::Blake2sp => "BLAKE2sp",
            Algo::Blake3 => "BLAKE3",
        }
    }

    /// The extension of the sum files the usual tools write (`.sha256`).
    pub fn ext(self) -> &'static str {
        match self {
            Algo::Crc32 => "sfv",
            Algo::Crc64 => "crc64",
            Algo::Xxh64 => "xxh64",
            Algo::Md5 => "md5",
            Algo::Sha1 => "sha1",
            Algo::Sha256 => "sha256",
            Algo::Sha384 => "sha384",
            Algo::Sha512 => "sha512",
            Algo::Sha3_256 => "sha3-256",
            Algo::Blake2sp => "blake2sp",
            Algo::Blake3 => "blake3",
        }
    }
}

/// CRC-64 as xz and 7-Zip compute it (ECMA-182, reflected).
const CRC64: crc::Crc<u64, crc::Table<16>> = crc::Crc::<u64, crc::Table<16>>::new(&crc::CRC_64_XZ);

enum State {
    Crc32(crc32fast::Hasher),
    Crc64(crc::Digest<'static, u64, crc::Table<16>>),
    Xxh64(twox_hash::XxHash64),
    Md5(md5::Md5),
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
    Sha384(sha2::Sha384),
    Sha512(sha2::Sha512),
    Sha3_256(sha3::Sha3_256),
    Blake2sp(Box<blake2s_simd::blake2sp::State>),
    Blake3(Box<blake3::Hasher>),
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Several hashes of the same bytes at once.
pub struct Hasher {
    states: Vec<(Algo, State)>,
}

impl Hasher {
    pub fn new(algos: &[Algo]) -> Hasher {
        use sha2::Digest;
        let states = algos
            .iter()
            .map(|&a| {
                let s = match a {
                    Algo::Crc32 => State::Crc32(crc32fast::Hasher::new()),
                    Algo::Crc64 => State::Crc64(CRC64.digest()),
                    Algo::Xxh64 => State::Xxh64(twox_hash::XxHash64::with_seed(0)),
                    Algo::Md5 => State::Md5(md5::Md5::new()),
                    Algo::Sha1 => State::Sha1(sha1::Sha1::new()),
                    Algo::Sha256 => State::Sha256(sha2::Sha256::new()),
                    Algo::Sha384 => State::Sha384(sha2::Sha384::new()),
                    Algo::Sha512 => State::Sha512(sha2::Sha512::new()),
                    Algo::Sha3_256 => State::Sha3_256(sha3::Sha3_256::new()),
                    Algo::Blake2sp => State::Blake2sp(Box::new(blake2s_simd::blake2sp::State::new())),
                    Algo::Blake3 => State::Blake3(Box::new(blake3::Hasher::new())),
                };
                (a, s)
            })
            .collect();
        Hasher { states }
    }

    pub fn update(&mut self, data: &[u8]) {
        use sha2::Digest;
        use std::hash::Hasher as _;
        for (_, s) in &mut self.states {
            match s {
                State::Crc32(h) => h.update(data),
                State::Crc64(h) => h.update(data),
                State::Xxh64(h) => h.write(data),
                State::Md5(h) => h.update(data),
                State::Sha1(h) => h.update(data),
                State::Sha256(h) => h.update(data),
                State::Sha384(h) => h.update(data),
                State::Sha512(h) => h.update(data),
                State::Sha3_256(h) => h.update(data),
                State::Blake2sp(h) => {
                    h.update(data);
                }
                State::Blake3(h) => {
                    h.update(data);
                }
            }
        }
    }

    /// The sums, as shown: CRCs and XXH64 in capitals, as 7-Zip shows them;
    /// the others in small letters, as `sha256sum` writes them.
    pub fn finish(self) -> Vec<(Algo, String)> {
        use sha2::Digest;
        use std::hash::Hasher as _;
        self.states
            .into_iter()
            .map(|(a, s)| {
                let v = match s {
                    State::Crc32(h) => format!("{:08X}", h.finalize()),
                    State::Crc64(h) => format!("{:016X}", h.finalize()),
                    State::Xxh64(h) => format!("{:016X}", h.finish()),
                    State::Md5(h) => hex(&h.finalize()),
                    State::Sha1(h) => hex(&h.finalize()),
                    State::Sha256(h) => hex(&h.finalize()),
                    State::Sha384(h) => hex(&h.finalize()),
                    State::Sha512(h) => hex(&h.finalize()),
                    State::Sha3_256(h) => hex(&h.finalize()),
                    State::Blake2sp(h) => h.finalize().to_hex().to_string(),
                    State::Blake3(h) => h.finalize().to_hex().to_string(),
                };
                (a, v)
            })
            .collect()
    }
}

/// The sums of one file, or why it could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sums {
    /// The path as shown: relative to the folder chosen, or inside the archive.
    pub name: String,
    pub size: u64,
    pub sums: Result<Vec<(Algo, String)>, Error>,
}

/// Reads `r` to the end into the hashes, counting into the meter.
pub fn read(r: &mut dyn Read, algos: &[Algo], meter: &Meter) -> Result<(u64, Vec<(Algo, String)>), Error> {
    let mut h = Hasher::new(algos);
    let mut buf = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        meter.check()?;
        let n = match r.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted && !archive::is_stop(&e) => continue,
            Err(e) => return Err(e.into()),
        };
        h.update(&buf[..n]);
        total += n as u64;
        meter.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    Ok((total, h.finish()))
}

/// The files to hash: each file chosen, and every file below each folder
/// chosen, with the name to show.
pub fn files_below(paths: &[PathBuf]) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    for p in paths {
        let base = p.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut stack = vec![p.clone()];
        while let Some(q) = stack.pop() {
            let Ok(m) = std::fs::symlink_metadata(&q) else { continue };
            if m.is_dir() {
                let Ok(rd) = std::fs::read_dir(&q) else { continue };
                let mut kids: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
                kids.sort();
                // Popped from the end: reversed, so that the files come in order.
                stack.extend(kids.into_iter().rev());
            } else if m.is_file() {
                let name = q.strip_prefix(&base).unwrap_or(&q).to_string_lossy().replace('\\', "/");
                out.push((q, name));
            }
        }
    }
    out
}

/// Hashes files on disk (folders: every file below them).
pub fn disk(paths: &[PathBuf], algos: &[Algo], meter: &Meter) -> Result<Vec<Sums>, Error> {
    let mut out = Vec::new();
    for (p, name) in files_below(paths) {
        meter.set_current(&name);
        let size = std::fs::metadata(&p).map_or(0, |m| m.len());
        let sums = match File::open(&p) {
            Ok(mut f) => match read(&mut f, algos, meter) {
                Err(Error::Stopped) => return Err(Error::Stopped),
                Err(Error::Corrupt(why)) => Err(Error::Io(why)),
                r => r.map(|(_, s)| s),
            },
            Err(e) => Err(Error::Io(e.to_string())),
        };
        meter.entries.fetch_add(1, Ordering::Relaxed);
        out.push(Sums { name, size, sums });
    }
    Ok(out)
}

/// Hashes the files of an archive for which `wanted` is true.
pub fn entries(path: &Path, listing: &Listing, wanted: &dyn Fn(usize) -> bool, algos: &[Algo], meter: &Meter) -> Result<Vec<Sums>, Error> {
    let mut out = Vec::new();
    let result = archive::each(path, listing, wanted, meter, &mut |i, contents| {
        let e = &listing.entries[i];
        meter.set_current(&e.path);
        let sums = match contents {
            Ok(r) => match read(r, algos, meter) {
                Err(Error::Stopped) => return Err(Error::Stopped),
                r => r.map(|(_, s)| s),
            },
            Err(Error::Stopped) => return Err(Error::Stopped),
            Err(e) => Err(e),
        };
        out.push(Sums { name: e.path.clone(), size: e.size.unwrap_or(0), sums });
        Ok(())
    });
    match result {
        Ok(()) => Ok(out),
        // A compressed tar broken in the middle: the sums so far, and the break.
        Err(Error::Corrupt(why)) => {
            out.push(Sums { name: String::new(), size: 0, sums: Err(Error::Corrupt(why)) });
            Ok(out)
        }
        Err(e) => Err(e),
    }
}

/// The files of an archive to hash: all of them when nothing is chosen.
pub fn count_files(listing: &Listing, wanted: Option<&[bool]>) -> usize {
    listing.entries.iter().enumerate().filter(|(i, e)| e.kind == Kind::File && wanted.is_none_or(|w| w[*i])).count()
}

/// Sums in the form `sha256sum` and its kin read back (`hash *name`), one
/// file a line; with several hashes, one block per hash. Files that could
/// not be read are left out.
pub fn sum_file(results: &[Sums], algos: &[Algo]) -> String {
    let mut out = String::new();
    for (k, a) in algos.iter().enumerate() {
        if algos.len() > 1 {
            if k > 0 {
                out.push('\n');
            }
            out.push_str(&format!("# {}\n", a.name()));
        }
        for r in results {
            if let Ok(sums) = &r.sums {
                if let Some((_, v)) = sums.iter().find(|(b, _)| b == a) {
                    out.push_str(&format!("{v} *{}\n", r.name));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sums(data: &[u8]) -> Vec<(Algo, String)> {
        let mut h = Hasher::new(&Algo::ALL);
        // In two pieces: the states carry over.
        let mid = data.len() / 2;
        h.update(&data[..mid]);
        h.update(&data[mid..]);
        h.finish()
    }

    fn get(s: &[(Algo, String)], a: Algo) -> &str {
        &s.iter().find(|(b, _)| *b == a).unwrap().1
    }

    #[test]
    fn known_values() {
        let s = sums(b"123456789");
        assert_eq!(get(&s, Algo::Crc32), "CBF43926");
        assert_eq!(get(&s, Algo::Crc64), "995DC9BBDF1939FA");
        let s = sums(b"abc");
        assert_eq!(get(&s, Algo::Crc32), "352441C2");
        assert_eq!(get(&s, Algo::Md5), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(get(&s, Algo::Sha1), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(get(&s, Algo::Sha256), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(get(&s, Algo::Sha384), "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7");
        assert_eq!(
            get(&s, Algo::Sha512),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(get(&s, Algo::Sha3_256), "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532");
        let s = sums(b"");
        assert_eq!(get(&s, Algo::Xxh64), "EF46DB3751D8E999");
        assert_eq!(get(&s, Algo::Blake3), "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262");
        assert_eq!(get(&s, Algo::Blake2sp).len(), 64);
        // BLAKE2sp is not BLAKE2s.
        assert_ne!(get(&s, Algo::Blake2sp), blake2s_simd::blake2s(b"").to_hex().as_str());
    }

    #[test]
    fn folders_files_and_entries() {
        let d = archive::tests::scratch("hash");
        std::fs::create_dir_all(d.join("f/sub")).unwrap();
        std::fs::write(d.join("f/a.txt"), b"abc").unwrap();
        std::fs::write(d.join("f/sub/b.txt"), b"").unwrap();
        std::fs::write(d.join("c.txt"), b"123456789").unwrap();
        let m = Meter::default();
        let r = disk(&[d.join("f"), d.join("c.txt")], &[Algo::Crc32], &m).unwrap();
        let names: Vec<&str> = r.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["f/a.txt", "f/sub/b.txt", "c.txt"]);
        assert_eq!(r[2].sums, Ok(vec![(Algo::Crc32, "CBF43926".to_string())]));
        assert_eq!(sum_file(&r, &[Algo::Crc32]), "352441C2 *f/a.txt\n00000000 *f/sub/b.txt\nCBF43926 *c.txt\n");
        // The entries of an archive: the same sums as the files they came from.
        let zip = d.join("s.zip");
        archive::tests::make_zip(&zip);
        let l = archive::open(&zip, &m).unwrap();
        let r = entries(&zip, &l, &|_| true, &[Algo::Sha256], &m).unwrap();
        assert_eq!(r.len(), 4);
        let readme = r.iter().find(|s| s.name == "readme.txt").unwrap();
        let mut h = Hasher::new(&[Algo::Sha256]);
        h.update(b"hello");
        assert_eq!(readme.sums, Ok(h.finish()));
        assert_eq!(count_files(&l, None), 4);
    }
}
