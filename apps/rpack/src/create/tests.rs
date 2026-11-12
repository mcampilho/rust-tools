use super::*;
use crate::archive::tests::{sample_files, scratch};
use crate::archive::{open, Container, Format};
use crate::extract::{extract, test, Job, Overwrite};

/// The sample files in a folder `src` inside `d`, plus an empty folder.
fn source(d: &Path) -> PathBuf {
    let src = d.join("src");
    for (name, data) in sample_files() {
        let p = src.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }
    fs::create_dir_all(src.join("nothing")).unwrap();
    src
}

fn check(dest: &Path) {
    for (name, data) in sample_files() {
        assert!(fs::read(dest.join("src").join(name)).unwrap() == data, "{name}");
    }
    assert!(dest.join("src/nothing").is_dir());
}

#[test]
fn every_kind_reads_back() {
    let d = scratch("create");
    let src = source(&d);
    for kind in Kind::ALL.into_iter().filter(|k| !k.needs_one_file()) {
        for level in [Level::Store, Level::Fastest, Level::Normal] {
            for solid in [true, false] {
                if solid && kind != Kind::SevenZ {
                    continue;
                }
                let name = format!("a-{}-{level:?}-{solid}.{}", kind.ext().replace('.', "-"), kind.ext());
                let out = d.join(&name);
                let spec = Spec { kind, level, solid, ..Spec::default() };
                let made = create(std::slice::from_ref(&src), &out, &spec, &Meter::default()).unwrap_or_else(|e| panic!("{name}: {e:?}"));
                assert_eq!((made.files, made.dirs), (4, 4), "{name}");
                assert!(made.size > 0 && made.skipped.is_empty());
                let l = open(&out, &Meter::default()).unwrap_or_else(|e| panic!("{name}: {e:?}"));
                let want = match kind {
                    Kind::SevenZ => Format { container: Container::SevenZ, codec: None },
                    Kind::Zip => Format { container: Container::Zip, codec: None },
                    Kind::Tar(c) => Format { container: Container::Tar, codec: c },
                    Kind::Single(_) => unreachable!(),
                };
                assert_eq!(l.format, want, "{name}");
                let x = d.join(format!("x-{name}"));
                let r = extract(&out, &l, &Job { dest: x.clone(), wanted: None, strip: String::new(), overwrite: Overwrite::Replace }, &Meter::default()).unwrap();
                assert!(r.errors.is_empty(), "{name}: {r:?}");
                check(&x);
                assert!(test(&out, &l, &Meter::default()).unwrap().errors.is_empty(), "{name}");
                // No temporary file is left beside it.
                assert!(!temp_of(&out).exists());
            }
        }
    }
}

#[test]
fn zip_methods_and_single_files() {
    let d = scratch("create-zip");
    let src = source(&d);
    for m in ZipMethod::ALL {
        let out = d.join(format!("{}.zip", m.label()));
        create(std::slice::from_ref(&src), &out, &Spec { kind: Kind::Zip, zip_method: m, ..Spec::default() }, &Meter::default()).unwrap();
        let l = open(&out, &Meter::default()).unwrap();
        assert!(test(&out, &l, &Meter::default()).unwrap().errors.is_empty(), "{m:?}");
    }
    let file = src.join("docs/text.txt");
    for kind in Kind::ALL.into_iter().filter(|k| k.needs_one_file()) {
        let out = d.join(suggested_name(std::slice::from_ref(&file), kind));
        create(std::slice::from_ref(&file), &out, &Spec { kind, ..Spec::default() }, &Meter::default()).unwrap();
        let l = open(&out, &Meter::default()).unwrap();
        assert_eq!(l.entries[0].path, "text.txt", "{kind:?}");
        let x = d.join(format!("x-{}", kind.ext()));
        extract(&out, &l, &Job { dest: x.clone(), wanted: None, strip: String::new(), overwrite: Overwrite::Replace }, &Meter::default()).unwrap();
        assert!(fs::read(x.join("text.txt")).unwrap() == sample_files()[1].1);
    }
    assert!(create(&[src.clone(), file.clone()], &d.join("two.gz"), &Spec { kind: Kind::Single(Codec::Gzip), ..Spec::default() }, &Meter::default()).is_err());
}

#[test]
fn names_and_stopping() {
    let d = scratch("create-names");
    let src = source(&d);
    assert_eq!(suggested_name(std::slice::from_ref(&src), Kind::SevenZ), "src.7z");
    assert_eq!(suggested_name(&[src.join("readme.txt")], Kind::Zip), "readme.zip");
    assert_eq!(suggested_name(&[src.join("readme.txt")], Kind::Single(Codec::Xz)), "readme.txt.xz");
    assert_eq!(suggested_name(&[src.join("readme.txt"), src.join("docs")], Kind::Tar(Some(Codec::Zstd))), "src.tar.zst");
    assert_eq!(Kind::from_key("tar.gz"), Some(Kind::Tar(Some(Codec::Gzip))));
    let m = Meter::default();
    m.stop();
    let out = d.join("stopped.7z");
    assert_eq!(create(std::slice::from_ref(&src), &out, &Spec::default(), &m), Err(Error::Stopped));
    assert!(!out.exists() && !temp_of(&out).exists());
}
