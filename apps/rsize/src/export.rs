//! Exporting a folder of the tree to CSV: one row per folder and file below
//! it, largest first within each folder, as spreadsheets read it (UTF-8 with
//! a byte order mark, commas, CRLF line ends, quotes where needed).

use crate::format;
use crate::i18n::tr;
use crate::model::{Id, Kind, Tree};
use std::io::{self, Write};

/// A field, quoted when it holds a comma, a quote or a line break.
fn field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Writes `top` and everything below it. Returns the number of rows.
pub fn write_csv(tree: &Tree, top: Id, out: impl Write) -> io::Result<u64> {
    let mut w = io::BufWriter::new(out);
    w.write_all("\u{FEFF}".as_bytes())?;
    let head = [tr("Path"), tr("Kind"), tr("Size"), tr("Size on disk"), tr("Files"), tr("Folders"), tr("Modified")];
    w.write_all(head.iter().map(|h| field(h)).collect::<Vec<_>>().join(",").as_bytes())?;
    w.write_all(b"\r\n")?;
    let (folder, file, link) = (tr("folder"), tr("file"), tr("link"));
    let mut rows = 0;
    let mut stack: Vec<(Id, String)> = vec![(top, tree.path(top).to_string_lossy().into_owned())];
    while let Some((id, path)) = stack.pop() {
        let n = tree.node(id);
        let kind = match n.kind {
            Kind::Dir => &folder,
            Kind::File => &file,
            Kind::Link => &link,
        };
        let (files, dirs) = if n.kind == Kind::Dir { (n.files.to_string(), n.dirs.to_string()) } else { (String::new(), String::new()) };
        let line = [field(&path), field(kind), n.size.to_string(), n.alloc.to_string(), files, dirs, format::datetime(n.mtime)].join(",");
        w.write_all(line.as_bytes())?;
        w.write_all(b"\r\n")?;
        rows += 1;
        let sep = if path.ends_with(std::path::MAIN_SEPARATOR) { "" } else { std::path::MAIN_SEPARATOR_STR };
        // Reversed, so the largest comes out first.
        for &c in tree.children(id).iter().rev() {
            stack.push((c, format!("{path}{sep}{}", tree.name(c))));
        }
    }
    w.flush()?;
    Ok(rows)
}

/// A file name for the export of a folder: its name, or "rsize" for a drive.
pub fn suggested_name(tree: &Tree, top: Id) -> String {
    let path = tree.path(top);
    let name = path.file_name().map_or("rsize".to_string(), |n| n.to_string_lossy().into_owned());
    let clean: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
    format!("{clean}.csv")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{set_lang, Lang};
    use crate::model::Entry;

    #[test]
    fn rows_with_quotes_and_order() {
        set_lang(Lang::EnUS);
        let f = |n: &str, s: u64| Entry { name: n.into(), kind: Kind::File, size: s, alloc: 4096, mtime: 1_577_836_800, flags: 0 };
        let d = |n: &str| Entry { name: n.into(), kind: Kind::Dir, size: 0, alloc: 0, mtime: 0, flags: 0 };
        let mut t = Tree::new(if cfg!(windows) { "C:\\r" } else { "/r" }, 0);
        t.add_children(0, &[f("small, \"quoted\".txt", 1), d("big")]);
        t.add_children(2, &[f("x.bin", 900)]);
        t.finish();
        let mut out = Vec::new();
        assert_eq!(write_csv(&t, 0, &mut out).unwrap(), 4);
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("\u{FEFF}Path,Kind,Size,Size on disk,Files,Folders,Modified\r\n"));
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[1].contains(",folder,901,8192,2,1,"));
        let big = t.path(t.children(0)[0]).to_string_lossy().into_owned();
        assert!(lines[2].starts_with(&format!("{big},folder,900,")), "{}", lines[2]);
        assert!(lines[3].contains("x.bin,file,900,4096,,,"), "{}", lines[3]);
        // The whole path is quoted, with its quotes doubled.
        assert!(lines[4].starts_with('"') && lines[4].contains("small, \"\"quoted\"\".txt\",file,1,"), "{}", lines[4]);
        assert_eq!(suggested_name(&t, 2), "big.csv");
    }
}
