//! Archives of every format made here, with the libraries' own writers, then
//! opened, read back and compared.

use super::*;
use std::io::Write;
use std::path::PathBuf;

/// A folder of its own for one test, emptied first.
pub fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rpack-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The files of the sample archives: (path, contents).
pub fn sample_files() -> Vec<(&'static str, Vec<u8>)> {
    let text: Vec<u8> = (0..20_000).flat_map(|i| format!("line {i} of the sample text\n").into_bytes()).collect();
    let noise: Vec<u8> = (0..70_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
    vec![("readme.txt", b"hello".to_vec()), ("docs/text.txt", text), ("docs/img/noise.bin", noise), ("docs/empty.txt", Vec::new())]
}

/// A zip with each method the writer has.
pub fn make_zip(path: &Path) {
    use ::zip::write::SimpleFileOptions;
    let mut z = ::zip::ZipWriter::new(File::create(path).unwrap());
    let methods = [CompressionMethod::Stored, CompressionMethod::Deflated, CompressionMethod::Bzip2, CompressionMethod::Zstd, CompressionMethod::Xz];
    z.add_directory("docs/", SimpleFileOptions::default()).unwrap();
    z.add_directory("docs/nothing/", SimpleFileOptions::default()).unwrap();
    for (i, (name, data)) in sample_files().into_iter().enumerate() {
        let when = ::zip::DateTime::from_date_and_time(2024, 5, 6, 7, 8, 10).unwrap();
        let opts = SimpleFileOptions::default().compression_method(methods[i % methods.len()]).last_modified_time(when);
        z.start_file(name, opts).unwrap();
        z.write_all(&data).unwrap();
    }
    z.finish().unwrap();
}
use ::zip::CompressionMethod;

/// Compresses `data` whole with `c`.
pub fn compress(c: Codec, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    match c {
        Codec::Gzip => {
            let mut e = flate2::write::GzEncoder::new(&mut out, flate2::Compression::default());
            e.write_all(data).unwrap();
            e.finish().unwrap();
        }
        Codec::Bzip2 => {
            let mut e = bzip2::write::BzEncoder::new(&mut out, bzip2::Compression::default());
            e.write_all(data).unwrap();
            e.finish().unwrap();
        }
        Codec::Xz => {
            let mut e = lzma_rust2::XzWriter::new(&mut out, lzma_rust2::XzOptions::with_preset(3)).unwrap();
            e.write_all(data).unwrap();
            e.finish().unwrap();
        }
        Codec::Lzma => {
            let mut e = lzma_rust2::LzmaWriter::new_use_header(&mut out, &lzma_rust2::LzmaOptions::with_preset(3), Some(data.len() as u64)).unwrap();
            e.write_all(data).unwrap();
            e.finish().unwrap();
        }
        Codec::Lzip => {
            let mut e = lzma_rust2::LzipWriter::new(&mut out, lzma_rust2::LzipOptions::with_preset(3));
            e.write_all(data).unwrap();
            e.finish().unwrap();
        }
        Codec::Zstd => {
            // As the zstd tool writes a file: with its size in the header.
            let mut e = zstd::Encoder::new(&mut out, 3).unwrap();
            e.set_pledged_src_size(Some(data.len() as u64)).unwrap();
            e.write_all(data).unwrap();
            e.finish().unwrap();
        }
        Codec::Lz4 => {
            let mut info = lz4_flex::frame::FrameInfo::new();
            info.content_size = Some(data.len() as u64);
            let mut e = lz4_flex::frame::FrameEncoder::with_frame_info(info, &mut out);
            e.write_all(data).unwrap();
            e.finish().unwrap();
        }
        Codec::Brotli => {
            let mut e = brotli::CompressorWriter::new(&mut out, 4096, 5, 22);
            e.write_all(data).unwrap();
            e.flush().unwrap();
            drop(e);
        }
    }
    out
}

fn ext(c: Codec) -> &'static str {
    c.extensions().0[0]
}

#[test]
fn zip_lists_every_method() {
    let d = scratch("zip-list");
    let p = d.join("sample.zip");
    make_zip(&p);
    let l = open(&p, &Meter::default()).unwrap();
    assert_eq!(l.format.container, Container::Zip);
    let names: Vec<&str> = l.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(names, ["docs", "docs/nothing", "readme.txt", "docs/text.txt", "docs/img/noise.bin", "docs/empty.txt"]);
    assert!(l.entries[0].is_dir());
    let methods: Vec<&str> = l.entries[2..].iter().map(|e| e.method.as_str()).collect();
    assert_eq!(methods, ["Store", "Deflate", "BZip2", "ZSTD"]);
    assert_eq!(l.entries[3].size, Some(sample_files()[1].1.len() as u64));
    assert!(l.entries[3].packed.unwrap() < l.entries[3].size.unwrap() / 4);
    assert_eq!(l.files(), 4);
    assert!(l.entries[2].mtime > 0);
}

#[test]
fn tar_with_each_codec() {
    let d = scratch("tar-codecs");
    let plain = make_tar_bytes();
    let p = d.join("sample.tar");
    std::fs::write(&p, &plain).unwrap();
    let l = open(&p, &Meter::default()).unwrap();
    assert_eq!(l.format, Format { container: Container::Tar, codec: None });
    assert_eq!(l.entries.len(), 5);
    assert_eq!(l.entries[4].kind, Kind::Link);
    assert_eq!(l.entries[1].mtime, 1_700_000_000);
    assert_eq!(l.entries[1].packed, l.entries[1].size);
    for c in Codec::ALL {
        let p = d.join(format!("sample.tar.{}", ext(c)));
        std::fs::write(&p, compress(c, &plain)).unwrap();
        let l = open(&p, &Meter::default()).unwrap_or_else(|e| panic!("{c:?}: {e:?}"));
        assert_eq!(l.format, Format { container: Container::Tar, codec: Some(c) }, "{c:?}");
        assert_eq!(l.entries.len(), 5, "{c:?}");
        assert_eq!(l.entries[2].size, Some(sample_files()[2].1.len() as u64));
        assert_eq!(l.entries[2].packed, None);
    }
}

/// A tar of the sample files and a link that points out of any folder.
pub fn make_tar_bytes() -> Vec<u8> {
    let mut b = ::tar::Builder::new(Vec::new());
    for (name, data) in sample_files() {
        let mut h = ::tar::Header::new_gnu();
        h.set_size(data.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(1_700_000_000);
        h.set_cksum();
        b.append_data(&mut h, name, data.as_slice()).unwrap();
    }
    let mut h = ::tar::Header::new_gnu();
    h.set_entry_type(::tar::EntryType::Symlink);
    h.set_size(0);
    h.set_cksum();
    b.append_link(&mut h, "link", "../../etc/passwd").unwrap();
    b.into_inner().unwrap()
}

#[test]
fn single_files_and_their_sizes() {
    let d = scratch("single");
    let data = &sample_files()[1].1;
    for c in Codec::ALL {
        let p = d.join(format!("text.txt.{}", ext(c)));
        std::fs::write(&p, compress(c, data)).unwrap();
        let l = open(&p, &Meter::default()).unwrap_or_else(|e| panic!("{c:?}: {e:?}"));
        assert_eq!(l.format, Format { container: Container::Single, codec: Some(c) }, "{c:?}");
        assert_eq!(l.entries.len(), 1);
        assert_eq!(l.entries[0].path, "text.txt");
        let known = !matches!(c, Codec::Bzip2 | Codec::Brotli);
        assert_eq!(l.entries[0].size, known.then_some(data.len() as u64), "{c:?}");
        let mut got = Vec::new();
        each(&p, &l, &|_| true, &Meter::default(), &mut |_, r| {
            r?.read_to_end(&mut got).map_err(Error::from)?;
            Ok(())
        })
        .unwrap();
        assert_eq!(&got, data, "{c:?}");
    }
}

#[test]
fn renamed_and_unknown_files() {
    let d = scratch("renamed");
    // A zip and a tar.gz with names that say nothing.
    make_zip(&d.join("a.dat"));
    assert_eq!(open(&d.join("a.dat"), &Meter::default()).unwrap().format.container, Container::Zip);
    std::fs::write(d.join("b.dat"), compress(Codec::Gzip, &make_tar_bytes())).unwrap();
    assert_eq!(open(&d.join("b.dat"), &Meter::default()).unwrap().format, Format { container: Container::Tar, codec: Some(Codec::Gzip) });
    // A zip after a program (self-extracting).
    let mut sfx = vec![b'M', b'Z'];
    sfx.extend(std::iter::repeat_n(0u8, 5000));
    sfx.extend(std::fs::read(d.join("a.dat")).unwrap());
    std::fs::write(d.join("setup.exe"), sfx).unwrap();
    let l = open(&d.join("setup.exe"), &Meter::default()).unwrap();
    assert_eq!(l.files(), 4);
    std::fs::write(d.join("c.txt"), "just text, not an archive at all").unwrap();
    assert_eq!(open(&d.join("c.txt"), &Meter::default()), Err(Error::Unknown));
    assert!(matches!(open(&d.join("missing.zip"), &Meter::default()), Err(Error::Io(_))));
}

#[test]
fn stopping() {
    let d = scratch("stop");
    std::fs::write(d.join("s.tar.xz"), compress(Codec::Xz, &make_tar_bytes())).unwrap();
    let m = Meter::default();
    m.stop();
    assert_eq!(open(&d.join("s.tar.xz"), &m), Err(Error::Stopped));
}
