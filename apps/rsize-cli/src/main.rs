//! rsize-cli: how much disk space files and folders take, like `du`. It reads
//! NTFS volumes directly (as administrator) or through the Everything index
//! when it can, folder by folder otherwise: the same reading as rsize.

mod cli;
mod report;

use cli::{Format, Opts};
use report::Entry;
use rt_cli::Texts;
use rt_scan::model::{Kind, SizeMode};
use rt_scan::{scan_with, Ways};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::AtomicBool;

const PROG: &str = "rsize-cli";
const TABLE: &str = include_str!("strings.txt");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(&args, &mut std::io::stdout().lock(), &mut std::io::stderr().lock());
    std::process::exit(code);
}

/// Runs the command line. Returns the exit status: 0 all read, 1 a path or
/// some folders could not be read, 2 wrong usage.
fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let lang = cli::lang_asked(args).and_then(|l| rt_cli::pick(&l)).unwrap_or_else(|| rt_cli::language("RSIZE_LANG"));
    let t = Texts::new(TABLE, &lang);
    let mut o = match cli::parse(args) {
        Ok(o) => o,
        Err(e) => {
            let _ = writeln!(err, "{PROG}: {}\n{}", e.describe(&t), t.fill("err.try", &[("prog", PROG)]));
            return 2;
        }
    };
    if o.help {
        let _ = write!(out, "{}", cli::help(&t, PROG));
        return 0;
    }
    if o.version {
        let _ = writeln!(out, "{PROG} {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    if o.paths.is_empty() {
        o.paths.push(".".into());
    }

    let mut status = 0;
    let mut entries = Vec::new();
    let mut total = 0;
    let mut unreadable = 0;
    for path in std::mem::take(&mut o.paths) {
        match collect(&path, &o) {
            Ok((mut found, missed)) => {
                unreadable += missed;
                if let Some(root) = found.iter().find(|e| e.path == path || e.path.is_empty()).or(found.last()) {
                    total += root.bytes(&o);
                }
                entries.append(&mut found);
            }
            Err(reason) => {
                let _ = writeln!(err, "{PROG}: {}", t.fill("err.access", &[("path", &path), ("reason", &reason)]));
                status = 1;
            }
        }
    }
    if unreadable > 0 {
        let _ = writeln!(err, "{PROG}: {}", t.fill("warn.unreadable", &[("n", &unreadable.to_string())]));
        status = 1;
    }
    if let Some(n) = o.top {
        entries = report::largest(entries, n, &o);
    }
    let total = (o.total && o.format == Format::Text).then_some(total);
    if report::write(out, &entries, total, &o).is_err() {
        // A closed pipe (`| head`) is not an error worth reporting.
        return status;
    }
    status
}

/// The entries of one path, and how many folders could not be read.
fn collect(path: &str, o: &Opts) -> Result<(Vec<Entry>, usize), String> {
    let p = Path::new(path);
    let meta = std::fs::metadata(p).map_err(|e| e.to_string())?;
    if !meta.is_dir() {
        // A single file: its length (the space it takes is not known without reading its folder).
        let mtime = meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
        let e = Entry { path: path.to_string(), kind: Kind::File, size: meta.len(), alloc: meta.len(), files: 1, folders: 0, mtime };
        return Ok((vec![e], 0));
    }
    let ways = if o.no_index { Ways::SYSTEM } else { Ways::ALL };
    let cancel = AtomicBool::new(false);
    let mut scan = scan_with(p, ways, &cancel, &|_| {})?;
    scan.tree.sort(if o.apparent { SizeMode::Logical } else { SizeMode::Allocated });
    let entries = report::select(&scan.tree, path, o);
    Ok((entries, scan.tree.unreadable()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_args(s: &[&str]) -> (i32, String, String) {
        let args: Vec<String> = s.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&args, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    /// A folder with known files, inside the project (never in the TEMP folder).
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("rsize-cli-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.join("sub").join("b.bin"), vec![0u8; 3000]).unwrap();
        dir
    }

    #[test]
    fn the_table_of_texts_is_complete() {
        assert_eq!(rt_cli::table_problems(TABLE), Vec::<String>::new());
    }

    #[test]
    fn help_and_version_need_no_scan() {
        let (code, out, _) = run_args(&["--help", "--lang", "pt"]);
        assert_eq!(code, 0);
        assert!(out.starts_with("Utilização: rsize-cli"));
        let (_, out, _) = run_args(&["--version"]);
        assert!(out.starts_with("rsize-cli 0."));
    }

    #[test]
    fn a_wrong_option_is_status_2() {
        let (code, out, err) = run_args(&["--nope", "--lang=es"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.starts_with("rsize-cli: opción desconocida '--nope'"), "{err}");
        assert!(err.contains("rsize-cli --help"));
    }

    #[test]
    fn a_missing_path_is_status_1_and_the_others_still_count() {
        let dir = scratch("missing");
        let (code, out, err) = run_args(&["--no-index", "-b", "-s", dir.to_str().unwrap(), "no-such-place"]);
        assert_eq!(code, 1);
        assert!(out.starts_with("4000\t"), "{out}");
        assert!(err.contains("no-such-place"), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_folder_is_listed_with_its_sizes() {
        let dir = scratch("listed");
        let shown = dir.to_str().unwrap().to_string();
        let (code, out, err) = run_args(&["--no-index", "-b", "-c", &shown]);
        assert_eq!((code, err.as_str()), (0, ""));
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3, "{out}");
        assert!(lines[0].starts_with("3000\t") && lines[0].ends_with("sub"), "{out}");
        assert_eq!(lines[1], format!("4000\t{shown}"));
        assert_eq!(lines[2], "4000\ttotal");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_shows_its_length() {
        let dir = scratch("file");
        let file = dir.join("a.txt");
        let (code, out, _) = run_args(&["-b", file.to_str().unwrap()]);
        assert_eq!(code, 0);
        assert!(out.starts_with("1000\t"), "{out}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn json_lists_every_path() {
        let dir = scratch("json");
        let (code, out, _) = run_args(&["--no-index", "--json", "-a", "-b", dir.to_str().unwrap()]);
        assert_eq!(code, 0);
        assert_eq!(out.matches("\"path\"").count(), 4, "{out}");
        assert!(out.starts_with("[\n") && out.trim_end().ends_with(']'));
        let _ = std::fs::remove_dir_all(dir);
    }
}
