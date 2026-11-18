//! rdup-cli: finds files with the same contents, like `fdupes`. It reads the
//! drives and compares the files as rdup does (size, then the first and last
//! bytes, then BLAKE3 of the whole contents), and only lists: it never
//! deletes anything.

mod cli;
mod report;

use cli::Opts;
use rt_cli::Texts;
use rt_core::store::{Location, Place};
use rt_dups::search::{find_with, Failure, Options, Progress, Stage};
use rt_scan::Ways;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const PROG: &str = "rdup-cli";
const TABLE: &str = include_str!("strings.txt");

/// Where rdup keeps its data: the hashes are shared with it.
const PLACE: Place = Place { app: "rdup", file: "settings.ini", file_marks_portable: false };

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(&args, &mut std::io::stdout().lock(), &mut std::io::stderr().lock());
    std::process::exit(code);
}

/// Runs the command line. Returns the exit status: 0 searched (with or
/// without duplicates), 1 something could not be read, 2 wrong usage.
fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let lang = cli::lang_asked(args).and_then(|l| rt_cli::pick(&l)).unwrap_or_else(|| rt_cli::language("RDUP_LANG"));
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
    search(&o, out, err, &t)
}

fn cache_file(o: &Opts) -> Option<PathBuf> {
    if o.no_cache {
        return None;
    }
    match &o.cache {
        Some(f) => Some(PathBuf::from(f)),
        None => Location::detect(PLACE).file("hashes.bin"),
    }
}

fn search(o: &Opts, out: &mut dyn Write, err: &mut dyn Write, t: &Texts) -> i32 {
    let roots: Vec<PathBuf> = o.paths.iter().map(PathBuf::from).collect();
    let opts = Options { filter: o.filter(), files: !o.folders, folders: o.folders, cache: cache_file(o), hard_links: o.hard_links };
    let ways = if o.no_index { Ways::SYSTEM } else { Ways::ALL };
    let shown = !o.quiet && std::io::stderr().is_terminal();
    let width = AtomicUsize::new(0);
    let progress = |p: &Progress| {
        if shown {
            show(p, t, &width);
        }
    };
    let found = find_with(&roots, ways, opts, &AtomicBool::new(false), &progress);
    if shown {
        // Wipes the progress line.
        let _ = write!(std::io::stderr(), "\r{}\r", " ".repeat(width.load(Ordering::Relaxed)));
    }
    let found = match found {
        Ok(f) => f,
        Err(Failure::Unreadable(reason)) => {
            let _ = writeln!(err, "{PROG}: {}", t.fill("err.access", &[("path", &o.paths.join(", ")), ("reason", &reason)]));
            return 1;
        }
        Err(Failure::Stopped) => return 1,
    };

    let mut status = 0;
    for (path, reason) in &found.failed {
        let _ = writeln!(err, "{PROG}: {}", t.fill("err.access", &[("path", &path.to_string_lossy()), ("reason", reason)]));
        status = 1;
    }
    let folders: usize = found.roots.iter().map(|r| r.unreadable).sum();
    if folders > 0 {
        let _ = writeln!(err, "{PROG}: {}", t.fill("warn.folders", &[("n", &folders.to_string())]));
        status = 1;
    }
    if found.unreadable > 0 {
        let _ = writeln!(err, "{PROG}: {}", t.fill("warn.files", &[("n", &found.unreadable.to_string())]));
        status = 1;
    }
    // A closed pipe (`| head`) is not an error worth reporting.
    let shown = report::Shown::new(&o.paths);
    let _ = if o.folders { report::write_folders(out, &found, o, t, &shown) } else { report::write(out, &found.groups, o, t, &shown) };
    status
}

/// The progress on one line of the error output.
fn show(p: &Progress, t: &Texts, width: &AtomicUsize) {
    let (done, total) = (p.done.to_string(), p.total.to_string());
    let line = match p.stage {
        Stage::Reading | Stage::Folders => t.fill("progress.read", &[("files", &p.files.to_string())]),
        Stage::Edges => t.fill("progress.edges", &[("done", &done), ("total", &total)]),
        Stage::Contents => {
            t.fill("progress.contents", &[("done", &rt_cli::human(p.done_bytes, false)), ("total", &rt_cli::human(p.total_bytes, false))])
        }
    };
    let old = width.swap(line.chars().count(), Ordering::Relaxed);
    let pad = old.saturating_sub(line.chars().count());
    let _ = write!(std::io::stderr(), "\r{line}{}", " ".repeat(pad));
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

    #[test]
    fn the_table_of_texts_is_complete() {
        assert_eq!(rt_cli::table_problems(TABLE), Vec::<String>::new());
    }

    #[test]
    fn help_and_version_need_no_search() {
        let (code, out, _) = run_args(&["--help", "--lang", "es"]);
        assert_eq!(code, 0);
        assert!(out.starts_with("Uso: rdup-cli"), "{out}");
        let (_, out, _) = run_args(&["--version"]);
        assert!(out.starts_with("rdup-cli 0."));
    }

    #[test]
    fn a_wrong_option_is_status_2() {
        let (code, out, err) = run_args(&["--nope", "--lang=pt"]);
        assert_eq!((code, out.as_str()), (2, ""));
        assert!(err.starts_with("rdup-cli: opção desconhecida '--nope'"), "{err}");
    }

    #[test]
    fn a_missing_folder_is_status_1() {
        let (code, out, err) = run_args(&["--no-index", "--no-cache", "no-such-place"]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(err.contains("no-such-place"), "{err}");
    }

    #[test]
    fn copies_are_listed_in_groups() {
        let tt = rt_dups::testing::sample("rdup-cli-list");
        let root = tt.0.to_string_lossy().into_owned();
        let (code, out, err) = run_args(&["--no-index", "--no-cache", &root]);
        assert_eq!((code, err.as_str()), (0, ""));
        let groups: Vec<&str> = out.trim_end().split("\n\n").collect();
        // The three big copies waste the most, then the two notes.
        assert_eq!(groups.len(), 2, "{out}");
        assert_eq!(groups[0].lines().count(), 3);
        assert_eq!(groups[1].lines().count(), 2);
        assert!(groups[1].lines().all(|l| l.ends_with("notes.txt")), "{out}");
    }

    #[test]
    fn the_summary_and_the_filters_apply() {
        let tt = rt_dups::testing::sample("rdup-cli-filter");
        let root = tt.0.to_string_lossy().into_owned();
        let (_, out, _) = run_args(&["--no-index", "--no-cache", "-m", &root]);
        assert_eq!(out, "3 duplicate files (in 2 sets), occupying 601000 bytes\n");
        // Only the big files.
        let (_, out, _) = run_args(&["--no-index", "--no-cache", "-m", "--min-size", "100KB", &root]);
        assert_eq!(out, "2 duplicate files (in 1 sets), occupying 600000 bytes\n");
        // Only .txt files.
        let (_, out, _) = run_args(&["--no-index", "--no-cache", "-m", "--ext", "txt", &root]);
        assert_eq!(out, "1 duplicate files (in 1 sets), occupying 1000 bytes\n");
        let (_, out, _) = run_args(&["--no-index", "--no-cache", "-m", "--exclude", "docs backup other", &root]);
        assert_eq!(out, "No duplicates found.\n");
    }

    #[test]
    fn json_and_csv_cover_every_copy() {
        let tt = rt_dups::testing::sample("rdup-cli-formats");
        let root = tt.0.to_string_lossy().into_owned();
        let (_, json, _) = run_args(&["--no-index", "--no-cache", "--json", &root]);
        assert_eq!(json.matches("\"path\"").count(), 5, "{json}");
        let (_, csv, _) = run_args(&["--no-index", "--no-cache", "--csv", &root]);
        assert_eq!(csv.lines().count(), 6, "{csv}");
    }

    #[test]
    fn duplicate_folders_are_listed() {
        let tt = rt_dups::testing::folder_sample("rdup-cli-folders");
        let root = tt.0.to_string_lossy().into_owned();
        let (code, out, err) = run_args(&["--no-index", "--no-cache", "-D", &root]);
        assert_eq!((code, err.as_str()), (0, ""));
        let sets: Vec<&str> = out.trim_end().split("\n\n").collect();
        assert_eq!(sets.len(), 1, "{out}");
        // The folders that hold the identical ones say it all: `photos` and `backup\photos`.
        assert!(sets[0].lines().all(|l| l.ends_with("photos")) && sets[0].lines().count() == 2, "{out}");
        let (_, out, _) = run_args(&["--no-index", "--no-cache", "-D", "-m", &root]);
        assert_eq!(out, "1 duplicate folders (in 1 sets), occupying 7100 bytes\n");
        // The older folder has most, but not all, in common.
        let (_, out, _) = run_args(&["--no-index", "--no-cache", "--share=75", "--csv", &root]);
        assert!(out.starts_with("group,share,common,path,files,size,modified\n"), "{out}");
        assert!(out.lines().skip(1).any(|l| l.contains(",1000,")) && out.lines().any(|l| l.contains("old")), "{out}");
        assert!(run_args(&["--share=20"]).2.contains("--share"));
    }

    #[test]
    fn hard_links_count_as_copies_only_when_asked() {
        let tt = rt_dups::testing::sample("rdup-cli-links");
        if std::fs::hard_link(tt.0.join("docs").join("notes.txt"), tt.0.join("docs").join("notes-link.txt")).is_err() {
            return; // a file system without hard links
        }
        let root = tt.0.to_string_lossy().into_owned();
        let (_, plain, _) = run_args(&["--no-index", "--no-cache", "-m", &root]);
        let (_, with, _) = run_args(&["--no-index", "--no-cache", "-m", "-H", &root]);
        assert_eq!(plain, "3 duplicate files (in 2 sets), occupying 601000 bytes\n");
        assert_eq!(with, "4 duplicate files (in 2 sets), occupying 602000 bytes\n");
    }

    #[test]
    fn paths_are_shown_as_typed() {
        let tt = rt_dups::testing::sample("rdup-cli-typed");
        let typed = format!("{}{}.", tt.0.to_string_lossy(), std::path::MAIN_SEPARATOR);
        let (_, out, _) = run_args(&["--no-index", "--no-cache", &typed]);
        assert!(out.lines().filter(|l| !l.is_empty()).all(|l| l.starts_with(&typed)), "{out}");
    }

    #[test]
    fn the_cache_file_is_the_one_asked_for() {
        let tt = rt_dups::testing::sample("rdup-cli-cache");
        let root = tt.0.to_string_lossy().into_owned();
        let file = tt.0.join("hashes.bin");
        let (code, _, _) = run_args(&["--no-index", "--cache", file.to_str().unwrap(), &root]);
        assert_eq!(code, 0);
        assert!(file.exists());
    }
}
