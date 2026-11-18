//! Which entries of a scanned tree are listed, and how they are written.

use crate::cli::{Format, Opts, Order, Unit};
use rt_scan::model::{Id, Kind, Tree};
use std::io::{self, Write};

/// One line of the result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub kind: Kind,
    /// The length of the files below.
    pub size: u64,
    /// The space they take on the disk.
    pub alloc: u64,
    pub files: u64,
    pub folders: u64,
    /// Seconds since 1970 (UTC).
    pub mtime: i64,
}

impl Entry {
    /// The size the options ask for.
    pub fn bytes(&self, o: &Opts) -> u64 {
        if o.apparent {
            self.size
        } else {
            self.alloc
        }
    }
}

fn entry(tree: &Tree, id: Id, path: String) -> Entry {
    let n = tree.node(id);
    let dir = n.kind == Kind::Dir;
    Entry {
        path,
        kind: n.kind,
        size: n.size,
        alloc: n.alloc,
        files: if dir { n.files } else { 1 },
        folders: if dir { n.dirs } else { 0 },
        mtime: n.mtime,
    }
}

/// `base` and `name` as a path.
fn join(base: &str, name: &str) -> String {
    let separators: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    if base.ends_with(separators) {
        format!("{base}{name}")
    } else {
        format!("{base}{}{name}", std::path::MAIN_SEPARATOR)
    }
}

/// The entries to list for the tree of one path, folders after what is in
/// them (as `du` does), shown as `shown` plus the names below it.
pub fn select(tree: &Tree, shown: &str, o: &Opts) -> Vec<Entry> {
    /// Returns the entry (with what is left after the exclusions), or None
    /// when this one is excluded.
    fn walk(tree: &Tree, id: Id, path: String, depth: usize, o: &Opts, out: &mut Vec<Entry>) -> Option<Entry> {
        if depth > 0 && excluded(o, tree.name(id), &path) {
            return None;
        }
        let is_dir = tree.node(id).kind == Kind::Dir;
        let listed = o.depth().is_none_or(|m| depth <= m);
        let mut e = entry(tree, id, path.clone());
        // Below the depth asked for, kids are only read to know what the exclusions leave.
        if is_dir && (listed && o.depth().is_none_or(|m| depth < m) || !o.excludes.is_empty()) {
            let mut kids = tree.children(id).to_vec();
            if o.order == Order::Name {
                kids.sort_by_cached_key(|&k| tree.name(k).to_lowercase());
            }
            let left: Vec<Entry> = kids.into_iter().filter_map(|k| walk(tree, k, join(&path, tree.name(k)), depth + 1, o, out)).collect();
            if !o.excludes.is_empty() {
                e.size = left.iter().map(|k| k.size).sum();
                e.alloc = left.iter().map(|k| k.alloc).sum();
                e.files = left.iter().map(|k| k.files).sum();
                e.folders = left.iter().map(|k| k.folders + u64::from(k.kind == Kind::Dir)).sum();
                e.mtime = left.iter().map(|k| k.mtime).max().unwrap_or(e.mtime);
            }
        }
        if listed && (is_dir || o.all) {
            out.push(e.clone());
        }
        Some(e)
    }
    let mut out = Vec::new();
    walk(tree, tree.root(), shown.to_string(), 0, o, &mut out);
    out
}

/// Does an exclusion pattern match the name or the whole path?
fn excluded(o: &Opts, name: &str, path: &str) -> bool {
    o.excludes.iter().any(|p| glob(p, name) || glob(p, path))
}

/// `*` (any run) and `?` (one character) against `text`, ignoring case on Windows.
pub fn glob(pattern: &str, text: &str) -> bool {
    let fold = |s: &str| -> Vec<char> {
        if cfg!(windows) {
            s.to_lowercase().chars().collect()
        } else {
            s.chars().collect()
        }
    };
    let (p, t) = (fold(pattern), fold(text));
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Keeps the `n` largest, largest first.
pub fn largest(mut entries: Vec<Entry>, n: usize, o: &Opts) -> Vec<Entry> {
    entries.sort_by(|a, b| b.bytes(o).cmp(&a.bytes(o)).then_with(|| a.path.cmp(&b.path)));
    entries.truncate(n);
    entries
}

/// A size in the unit of the options.
pub fn size_text(bytes: u64, unit: Unit) -> String {
    match unit {
        Unit::Bytes => bytes.to_string(),
        Unit::Kib => bytes.div_ceil(1024).to_string(),
        Unit::Mib => bytes.div_ceil(1 << 20).to_string(),
        Unit::Human => rt_cli::human(bytes, false),
        Unit::Si => rt_cli::human(bytes, true),
    }
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Dir => "folder",
        Kind::File => "file",
        Kind::Link => "link",
    }
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Writes the entries in the format of the options. `total` (for `-c`) is
/// the sum of the paths, written at the end of the text.
pub fn write(out: &mut dyn Write, entries: &[Entry], total: Option<u64>, o: &Opts) -> io::Result<()> {
    match o.format {
        Format::Text => {
            for e in entries {
                writeln!(out, "{}\t{}", size_text(e.bytes(o), o.unit), e.path)?;
            }
            if let Some(t) = total {
                writeln!(out, "{}\ttotal", size_text(t, o.unit))?;
            }
        }
        Format::Json => {
            writeln!(out, "[")?;
            for (i, e) in entries.iter().enumerate() {
                let comma = if i + 1 < entries.len() { "," } else { "" };
                writeln!(
                    out,
                    "  {{\"path\": {}, \"kind\": \"{}\", \"size\": {}, \"allocated\": {}, \"files\": {}, \"folders\": {}, \"modified\": {}}}{comma}",
                    json_string(&e.path),
                    kind_name(e.kind),
                    e.size,
                    e.alloc,
                    e.files,
                    e.folders,
                    e.mtime
                )?;
            }
            writeln!(out, "]")?;
        }
        Format::Csv => {
            writeln!(out, "path,kind,size,allocated,files,folders,modified")?;
            for e in entries {
                writeln!(out, "{},{},{},{},{},{},{}", csv_field(&e.path), kind_name(e.kind), e.size, e.alloc, e.files, e.folders, e.mtime)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::parse;
    use rt_scan::model::Entry as Raw;

    fn opts(s: &str) -> Opts {
        parse(&s.split_whitespace().map(String::from).collect::<Vec<_>>()).unwrap()
    }

    /// root: a.txt (1000 bytes, 4096 on the disk), sub/ (b.bin 3000, c.log 500), empty/
    fn tree() -> Tree {
        let f = |n: &str, size: u64, alloc: u64| Raw { name: n.into(), kind: Kind::File, size, alloc, mtime: 100, flags: 0 };
        let d = |n: &str| Raw { name: n.into(), kind: Kind::Dir, size: 0, alloc: 0, mtime: 50, flags: 0 };
        let mut t = Tree::new(if cfg!(windows) { "C:\\r" } else { "/r" }, 0);
        t.add_children(0, &[f("a.txt", 1000, 4096), d("sub"), d("empty")]);
        let sub = (1..4).find(|&i| t.name(i) == "sub").unwrap();
        t.add_children(sub, &[f("b.bin", 3000, 4096), f("c.log", 500, 4096)]);
        t.finish();
        t.sort(rt_scan::model::SizeMode::Allocated);
        t
    }

    fn paths(e: &[Entry]) -> Vec<String> {
        e.iter().map(|e| e.path.replace('\\', "/")).collect()
    }

    #[test]
    fn folders_come_after_what_is_in_them() {
        let e = select(&tree(), "r", &opts(""));
        assert_eq!(paths(&e), ["r/sub", "r/empty", "r"]);
        assert_eq!(e[0].alloc, 8192);
        assert_eq!(e[2].alloc, 12288);
        assert_eq!((e[2].files, e[2].folders), (3, 2));
    }

    #[test]
    fn summarize_and_depth() {
        assert_eq!(paths(&select(&tree(), "r", &opts("-s"))), ["r"]);
        assert_eq!(paths(&select(&tree(), "r", &opts("-d 0"))), ["r"]);
        assert_eq!(paths(&select(&tree(), "r", &opts("-d 1"))), ["r/sub", "r/empty", "r"]);
        assert_eq!(paths(&select(&tree(), "r", &opts("-a -d 1"))), ["r/sub", "r/a.txt", "r/empty", "r"]);
    }

    #[test]
    fn all_lists_the_files_too() {
        let e = select(&tree(), "r", &opts("-a"));
        // The largest folder first (sub 8192 on the disk, a.txt 4096), as the tree orders them.
        assert_eq!(paths(&e), ["r/sub/b.bin", "r/sub/c.log", "r/sub", "r/a.txt", "r/empty", "r"]);
        assert_eq!(e[0].kind, Kind::File);
        assert_eq!((e[0].files, e[0].folders), (1, 0));
    }

    #[test]
    fn names_can_be_the_order() {
        let e = select(&tree(), "r", &opts("--sort name -a"));
        assert_eq!(paths(&e), ["r/a.txt", "r/empty", "r/sub/b.bin", "r/sub/c.log", "r/sub", "r"]);
    }

    #[test]
    fn the_path_is_shown_as_typed() {
        let sep = std::path::MAIN_SEPARATOR;
        let e = select(&tree(), "x", &opts(""));
        assert_eq!(e[0].path, format!("x{sep}sub"));
        let e = select(&tree(), &format!("x{sep}"), &opts(""));
        assert_eq!(e[0].path, format!("x{sep}sub"));
    }

    #[test]
    fn the_largest_come_first() {
        let o = opts("-a");
        let e = largest(select(&tree(), "r", &o), 3, &o);
        assert_eq!(paths(&e), ["r", "r/sub", "r/a.txt"]);
    }

    #[test]
    fn disk_space_or_length() {
        let disk = opts("");
        let apparent = opts("--apparent-size");
        let root = select(&tree(), "r", &disk).pop().unwrap();
        assert_eq!((root.bytes(&disk), root.bytes(&apparent)), (12288, 4500));
    }

    #[test]
    fn units() {
        assert_eq!(size_text(1, Unit::Kib), "1");
        assert_eq!(size_text(1025, Unit::Kib), "2");
        assert_eq!(size_text(0, Unit::Kib), "0");
        assert_eq!(size_text(5, Unit::Bytes), "5");
        assert_eq!(size_text(1 << 20, Unit::Mib), "1");
        assert_eq!(size_text(1536, Unit::Human), "1.5K");
        assert_eq!(size_text(1500, Unit::Si), "1.5k");
    }

    fn text(o: &Opts, entries: &[Entry], total: Option<u64>) -> String {
        let mut out = Vec::new();
        write(&mut out, entries, total, o).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn text_has_size_tab_path() {
        let o = opts("-h");
        let e = select(&tree(), "r", &o);
        assert_eq!(text(&o, &e[2..], Some(12288)), "12K\tr\n12K\ttotal\n");
    }

    #[test]
    fn json_escapes_names() {
        let o = opts("--json");
        let e = vec![Entry { path: "a\"b\\c\n".into(), kind: Kind::File, size: 1, alloc: 4096, files: 1, folders: 0, mtime: 7 }];
        assert_eq!(
            text(&o, &e, None),
            "[\n  {\"path\": \"a\\\"b\\\\c\\n\", \"kind\": \"file\", \"size\": 1, \"allocated\": 4096, \"files\": 1, \"folders\": 0, \"modified\": 7}\n]\n"
        );
        assert_eq!(text(&o, &[], None), "[\n]\n");
    }

    #[test]
    fn csv_quotes_where_needed() {
        let o = opts("--csv");
        let e = vec![Entry { path: "a,b".into(), kind: Kind::Dir, size: 1, alloc: 2, files: 3, folders: 4, mtime: 5 }];
        assert_eq!(text(&o, &e, None), "path,kind,size,allocated,files,folders,modified\n\"a,b\",folder,1,2,3,4,5\n");
    }
}
