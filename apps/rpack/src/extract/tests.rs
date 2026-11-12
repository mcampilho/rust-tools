use super::*;
use crate::archive::tests::{compress, make_tar_bytes, make_zip, sample_files, scratch};
use crate::archive::{open, Codec};

fn job(dest: &Path) -> Job {
    Job { dest: dest.to_path_buf(), wanted: None, strip: String::new(), overwrite: Overwrite::KeepBoth }
}

fn check_files(dest: &Path) {
    for (name, data) in sample_files() {
        let got = fs::read(dest.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(got == data, "{name} differs");
    }
}

#[test]
fn safe_paths() {
    let d = Path::new("/out");
    let p = |s: &str| safe_path(d, s).map(|p| p.strip_prefix(d).unwrap().to_string_lossy().replace('\\', "/"));
    assert_eq!(p("a/b.txt").as_deref(), Some("a/b.txt"));
    assert_eq!(p("../../etc/passwd").as_deref(), Some("etc/passwd"));
    assert_eq!(p("/abs/x").as_deref(), Some("abs/x"));
    assert_eq!(p("C:\\Windows\\x.dll").as_deref(), Some("Windows/x.dll"));
    assert_eq!(p("a/./b/../c").as_deref(), Some("a/b/c"));
    assert_eq!(p("what?.txt").as_deref(), Some("what_.txt"));
    assert_eq!(p("dots...").as_deref(), Some("dots"));
    assert_eq!(p("CON").as_deref(), Some("_CON"));
    assert_eq!(p("aux.txt").as_deref(), Some("_aux.txt"));
    assert_eq!(p("com1.log").as_deref(), Some("_com1.log"));
    assert_eq!(p("console.txt").as_deref(), Some("console.txt"));
    assert_eq!(p("../.."), None);
    assert_eq!(p(""), None);
}

#[test]
fn zip_extracts_and_tests() {
    let d = scratch("x-zip");
    let p = d.join("s.zip");
    make_zip(&p);
    let l = open(&p, &Meter::default()).unwrap();
    let r = extract(&p, &l, &job(&d.join("out")), &Meter::default()).unwrap();
    assert_eq!((r.files, r.dirs, r.errors.len()), (4, 2, 0), "{r:?}");
    check_files(&d.join("out"));
    assert!(d.join("out/docs/nothing").is_dir());
    let t = test(&p, &l, &Meter::default()).unwrap();
    assert_eq!((t.files, t.errors.len()), (4, 0));
    // The dates are kept.
    let m = fs::metadata(d.join("out/readme.txt")).unwrap().modified().unwrap();
    assert!((archive::unix_secs(m) - l.entries[2].mtime).abs() <= 2);
}

#[test]
fn every_tar_extracts_and_leaves_links_out() {
    let d = scratch("x-tar");
    let plain = make_tar_bytes();
    let mut cases = vec![("s.tar".to_string(), plain.clone())];
    cases.extend(Codec::ALL.map(|c| (format!("s.tar.{}", c.extensions().0[0]), compress(c, &plain))));
    for (name, bytes) in cases {
        let p = d.join(&name);
        fs::write(&p, bytes).unwrap();
        let l = open(&p, &Meter::default()).unwrap();
        let out = d.join(format!("out-{name}"));
        let r = extract(&p, &l, &job(&out), &Meter::default()).unwrap();
        assert_eq!((r.files, r.left_out, r.errors.len()), (4, 1, 0), "{name}: {r:?}");
        check_files(&out);
        assert!(fs::symlink_metadata(out.join("link")).is_err());
        assert!(fs::symlink_metadata(d.join("etc")).is_err());
        let t = test(&p, &l, &Meter::default()).unwrap();
        assert_eq!((t.files, t.errors.len()), (4, 0), "{name}");
    }
}

#[test]
fn part_of_an_archive_without_its_folder() {
    let d = scratch("x-part");
    let p = d.join("s.zip");
    make_zip(&p);
    let l = open(&p, &Meter::default()).unwrap();
    let tree = crate::tree::Tree::build(&l.entries);
    let docs = tree.nodes[0].dirs[0];
    let img = tree.nodes[docs].dirs.iter().copied().find(|&n| tree.nodes[n].name == "img").unwrap();
    let mut wanted = vec![false; l.entries.len()];
    tree.mark_below(img, &mut wanted);
    let j = Job { wanted: Some(wanted), strip: "docs".into(), ..job(&d.join("out")) };
    let r = extract(&p, &l, &j, &Meter::default()).unwrap();
    assert_eq!(r.files, 1);
    assert!(d.join("out/img/noise.bin").is_file());
    assert!(!d.join("out/docs").exists());
}

#[test]
fn files_already_there() {
    let d = scratch("x-there");
    let p = d.join("s.zip");
    make_zip(&p);
    let l = open(&p, &Meter::default()).unwrap();
    let out = d.join("out");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("readme.txt"), "mine").unwrap();
    let r = extract(&p, &l, &Job { overwrite: Overwrite::Skip, ..job(&out) }, &Meter::default()).unwrap();
    assert_eq!((r.skipped, r.files), (1, 3));
    assert_eq!(fs::read_to_string(out.join("readme.txt")).unwrap(), "mine");
    let r = extract(&p, &l, &job(&out), &Meter::default()).unwrap();
    assert_eq!(r.renamed, 4);
    assert_eq!(fs::read_to_string(out.join("readme (2).txt")).unwrap(), "hello");
    let r = extract(&p, &l, &Job { overwrite: Overwrite::Replace, ..job(&out) }, &Meter::default()).unwrap();
    assert_eq!((r.files, r.renamed, r.skipped), (4, 0, 0));
    assert_eq!(fs::read_to_string(out.join("readme.txt")).unwrap(), "hello");
}

#[test]
fn damage_is_found() {
    let d = scratch("x-bad");
    let p = d.join("s.zip");
    make_zip(&p);
    let mut bytes = fs::read(&p).unwrap();
    // A byte in the middle of the stored readme ("hello").
    let at = bytes.windows(5).position(|w| w == b"hello").unwrap();
    bytes[at + 1] ^= 0xFF;
    fs::write(&p, &bytes).unwrap();
    let l = open(&p, &Meter::default()).unwrap();
    let t = test(&p, &l, &Meter::default()).unwrap();
    assert_eq!(t.errors.len(), 1, "{t:?}");
    assert_eq!(t.errors[0].0, "readme.txt");
    // A compressed tar cut short: what comes before is extracted.
    let tgz = compress(Codec::Gzip, &make_tar_bytes());
    let cut = d.join("cut.tar.gz");
    fs::write(&cut, &tgz[..tgz.len() * 2 / 3]).unwrap();
    let l = open(&cut, &Meter::default());
    assert!(matches!(l, Err(Error::Corrupt(_))), "{l:?}");
    // A single file cut short.
    let gz = d.join("t.txt.gz");
    let data = compress(Codec::Gzip, &sample_files()[1].1);
    fs::write(&gz, &data[..data.len() / 2]).unwrap();
    let l = open(&gz, &Meter::default()).unwrap();
    let t = test(&gz, &l, &Meter::default()).unwrap();
    assert_eq!(t.errors.len(), 1, "{t:?}");
    let r = extract(&gz, &l, &job(&d.join("out")), &Meter::default()).unwrap();
    assert_eq!((r.files, r.errors.len()), (0, 1));
    assert!(!d.join("out/t.txt").exists());
}

#[test]
fn several_archives_each_beside_itself() {
    let d = scratch("x-each");
    make_zip(&d.join("a.zip"));
    fs::write(d.join("b.tar.gz"), compress(Codec::Gzip, &make_tar_bytes())).unwrap();
    fs::write(d.join("c.txt.xz"), compress(Codec::Xz, b"one file")).unwrap();
    fs::write(d.join("bad.zip"), b"not a zip").unwrap();
    let paths = ["a.zip", "b.tar.gz", "c.txt.xz", "bad.zip"].map(|n| d.join(n));
    let r = extract_each(&paths, &EachDest::Smart, Overwrite::KeepBoth, &Meter::default());
    assert_eq!(r.len(), 4);
    assert_eq!(r[0].dest, d.join("a"));
    check_files(&d.join("a"));
    check_files(&d.join("b"));
    // One file: beside the archive.
    assert_eq!(r[2].dest, d);
    assert_eq!(fs::read(d.join("c.txt")).unwrap(), b"one file");
    assert_eq!(r[3].result, Err(Error::Unknown));
    let r = extract_each(&paths[..1], &EachDest::Here, Overwrite::Replace, &Meter::default());
    assert_eq!(r[0].dest, d);
    assert!(d.join("docs/text.txt").is_file());
    let r = extract_each(&paths[1..2], &EachDest::Named, Overwrite::Replace, &Meter::default());
    assert_eq!(r[0].dest, d.join("b"));
    let r = extract_each(&paths[..1], &EachDest::Into(d.join("into")), Overwrite::Replace, &Meter::default());
    assert_eq!(r[0].dest, d.join("into").join("a"));
    check_files(&d.join("into/a"));
    let t = test_each(&paths[..3], &Meter::default());
    assert!(t.iter().all(|e| e.result.as_ref().is_ok_and(|r| r.errors.is_empty())), "{t:?}");
}

#[test]
fn stopping_leaves_no_half_file() {
    let d = scratch("x-stop");
    let p = d.join("s.zip");
    make_zip(&p);
    let l = open(&p, &Meter::default()).unwrap();
    let m = Meter::default();
    m.stop();
    assert_eq!(extract(&p, &l, &job(&d.join("out")), &m), Err(Error::Stopped));
    assert!(!d.join("out/readme.txt").exists());
}
