use super::*;
use std::time::Instant;

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("rt-ops-tests").join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let d = d.canonicalize().unwrap();
    shown(&d)
}

/// `folders` folders (some inside others) with `files` files of `size` bytes each.
fn tree(root: &Path, folders: usize, files: usize, size: usize) -> u64 {
    let data = vec![7u8; size];
    for i in 0..folders {
        let d = root.join(format!("d{}", i / 10)).join(format!("s{i}"));
        fs::create_dir_all(&d).unwrap();
        for j in 0..files {
            fs::write(d.join(format!("f{j}.bin")), &data).unwrap();
        }
    }
    (folders * files) as u64
}

#[test]
fn a_tree_goes_whole_with_read_only_files_and_odd_names() {
    for threads in [1, 4] {
        let d = scratch(&format!("delete-{threads}"));
        let top = d.join("top");
        let files = tree(&top, 30, 5, 10);
        let readonly = top.join("d0").join("s0").join("f0.bin");
        let mut p = fs::metadata(&readonly).unwrap().permissions();
        p.set_readonly(true);
        fs::set_permissions(&readonly, p).unwrap();
        fs::create_dir_all(top.join("empty").join("deeper")).unwrap();
        let one = d.join("one.txt");
        fs::write(&one, "x").unwrap();
        let ctl = Control::new();
        let failures = delete(&[top.clone(), one.clone()], threads, &ctl);
        assert!(failures.is_empty(), "{failures:?}");
        assert!(!top.exists() && !one.exists() && d.exists());
        assert_eq!(ctl.files(), files + 1);
    }
}

#[cfg(windows)]
#[test]
fn a_name_ending_in_a_dot_is_that_file_and_not_its_neighbour() {
    let d = scratch("dots");
    let top = d.join("top");
    fs::create_dir(&top).unwrap();
    let exact_name = exact(&top).unwrap();
    fs::write(exact_name.join("a."), "dot").unwrap();
    fs::write(exact_name.join("a"), "plain").unwrap();
    // Only the one with the dot.
    let failures = delete(&[top.join("a.")], 1, &Control::new());
    assert!(failures.is_empty(), "{failures:?}");
    assert_eq!(fs::read_to_string(top.join("a")).unwrap(), "plain");
    assert!(fs::symlink_metadata(exact_name.join("a.")).is_err());
}

#[test]
fn links_are_removed_and_what_they_lead_to_stays() {
    let d = scratch("links");
    let (top, precious) = (d.join("top"), d.join("precious"));
    fs::create_dir(&top).unwrap();
    fs::create_dir(&precious).unwrap();
    fs::write(precious.join("kept.txt"), "x").unwrap();
    let link = top.join("link");
    #[cfg(windows)]
    {
        let made = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(&link).arg(&precious).output().unwrap();
        assert!(made.status.success(), "{made:?}");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&precious, &link).unwrap();
    // Moving to another drive refuses it, and nothing changes.
    let copy_to = d.join("copy");
    let moved = copy_then_remove(&top, &copy_to, 2, &Control::new(), "holds a link").unwrap();
    assert!(matches!(moved, Moved::NotCopied(f) if f.first[0].1 == "holds a link"));
    assert!(top.exists() && !copy_to.exists());
    let failures = delete(std::slice::from_ref(&top), 2, &Control::new());
    assert!(failures.is_empty(), "{failures:?}");
    assert!(!top.exists());
    assert!(precious.join("kept.txt").exists());
}

#[test]
fn drives_and_relative_paths_are_refused() {
    let root = if cfg!(windows) { r"C:\" } else { "/" };
    for p in [root, "relative", ""] {
        assert_eq!(delete(&[PathBuf::from(p)], 1, &Control::new()).count, 1, "{p}");
    }
}

#[cfg(windows)]
#[test]
fn what_is_in_use_stays_with_the_folders_above_it() {
    use std::os::windows::fs::OpenOptionsExt;
    let d = scratch("in-use");
    let top = d.join("top");
    tree(&top, 3, 3, 1);
    let busy = top.join("d0").join("s1").join("f1.bin");
    // Opened sharing nothing: it cannot be deleted meanwhile.
    let held = fs::OpenOptions::new().read(true).share_mode(0).open(&busy).unwrap();
    let failures = delete(std::slice::from_ref(&top), 3, &Control::new());
    drop(held);
    assert_eq!(failures.count, 1, "{failures:?}");
    assert_eq!(failures.first[0].0, busy);
    assert!(busy.exists());
    assert!(!top.join("d0").join("s0").exists() && !top.join("d0").join("s2").exists());
}

#[test]
fn stopped_before_it_starts_nothing_changes() {
    let d = scratch("stopped");
    let top = d.join("top");
    tree(&top, 3, 3, 1);
    let ctl = Control::new();
    ctl.stop();
    delete(std::slice::from_ref(&top), 2, &ctl);
    assert_eq!(fs::read_dir(top.join("d0")).unwrap().count(), 3);
    let to = d.join("to");
    assert!(matches!(copy_then_remove(&top, &to, 2, &ctl, "link").unwrap(), Moved::Stopped));
    assert!(top.exists() && !to.exists());
}

#[test]
fn a_move_by_copy_takes_everything_and_counts_the_bytes() {
    let d = scratch("move");
    let (from, to) = (d.join("from"), d.join("to"));
    let files = tree(&from, 12, 4, 1000);
    let ctl = Control::new();
    assert!(matches!(copy_then_remove(&from, &to, 4, &ctl, "link").unwrap(), Moved::All));
    assert!(!from.exists());
    assert_eq!(fs::read(to.join("d1").join("s11").join("f3.bin")).unwrap(), vec![7u8; 1000]);
    assert_eq!((ctl.files(), ctl.bytes()), (files, files * 1000));
    // On the same drive it is only renamed; and never over what is there.
    let again = d.join("again");
    assert!(matches!(move_to(&to, &again, 1, &Control::new(), "link").unwrap(), Moved::All));
    fs::create_dir(&to).unwrap();
    assert_eq!(move_to(&again, &to, 1, &Control::new(), "link").unwrap_err().kind(), io::ErrorKind::AlreadyExists);
}

/// Not a test: times deleting and copying a tree with different numbers of
/// threads, to choose [`media::threads`]. `RT_OPS_BENCH_DIR` is a list of
/// folders separated by `|`, each on the drive to measure (a folder of its own
/// is made there and removed); `RT_OPS_FOLDERS` and `RT_OPS_FILES` size the tree.
#[test]
#[ignore]
fn bench() {
    let dirs = std::env::var("RT_OPS_BENCH_DIR").expect("RT_OPS_BENCH_DIR");
    let number = |name: &str, or: usize| std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(or);
    let (folders, files) = (number("RT_OPS_FOLDERS", 100), number("RT_OPS_FILES", 40));
    for dir in dirs.split('|') {
        let base = Path::new(dir).join("rt-ops-bench-tmp");
        println!("{dir}: {:?}, {} files of 4 KB in {folders} folders", media::of(Path::new(dir)), folders * files);
        for round in 0..2 {
            let mut order = vec![1usize, 2, 4, 8, 16];
            if round == 1 {
                order.reverse();
            }
            for threads in order {
                let from = base.join("from");
                tree(&from, folders, files, 4096);
                let t = Instant::now();
                let copied = base.join("copy");
                assert!(copy(&from, &copied, threads, &Control::new(), "link").is_empty());
                let copy_time = t.elapsed();
                let t = Instant::now();
                assert!(delete(std::slice::from_ref(&from), threads, &Control::new()).is_empty());
                let delete_time = t.elapsed();
                println!(
                    "  round {round} threads {threads:2}: delete {:7.2} s   copy {:7.2} s",
                    delete_time.as_secs_f64(),
                    copy_time.as_secs_f64()
                );
                delete(std::slice::from_ref(&copied), 8, &Control::new());
            }
        }
        // The system's own way, one thread, to compare.
        let from = base.join("from");
        tree(&from, folders, files, 4096);
        let t = Instant::now();
        fs::remove_dir_all(&from).unwrap();
        println!("  std remove_dir_all: {:.2} s", t.elapsed().as_secs_f64());
        fs::remove_dir(&base).unwrap();
    }
}
