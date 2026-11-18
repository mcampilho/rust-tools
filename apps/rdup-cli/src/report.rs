//! How the groups of copies are written: as fdupes does (a file per line, a
//! blank line between groups), a group per line, a summary, JSON or CSV.

use crate::cli::{Format, Opts};
use rt_cli::Texts;
use rt_dups::search::Group;
use std::io::{self, Write};

fn hex(hash: &[u8; 32]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

/// A size as asked: in bytes, or as `1.5K`.
fn size_text(bytes: u64, o: &Opts, t: &Texts) -> String {
    if o.human {
        rt_cli::human(bytes, false)
    } else {
        format!("{bytes} {}", t.get("unit.bytes"))
    }
}

/// The files of a group that are listed.
fn listed<'a>(g: &'a Group, o: &Opts) -> &'a [rt_dups::search::Copy] {
    if o.omit_first {
        &g.files[1.min(g.files.len())..]
    } else {
        &g.files
    }
}

/// A path for a line with several: in quotes when it holds a space.
fn quoted(path: &str) -> String {
    if path.contains(' ') {
        format!("\"{path}\"")
    } else {
        path.to_string()
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

/// Writes the groups in the format of the options.
pub fn write(out: &mut dyn Write, groups: &[Group], o: &Opts, t: &Texts) -> io::Result<()> {
    if o.summarize && o.format == Format::Text {
        return summary(out, groups, o, t);
    }
    match o.format {
        Format::Text => {
            let mut first = true;
            for g in groups {
                let files = listed(g, o);
                if files.is_empty() {
                    continue;
                }
                if o.sameline {
                    if o.size {
                        write!(out, "{} ", t.fill("group.each", &[("size", &size_text(g.size, o, t))]))?;
                    }
                    let line: Vec<String> = files.iter().map(|c| quoted(&c.path.to_string_lossy())).collect();
                    writeln!(out, "{}", line.join(" "))?;
                    continue;
                }
                if !first {
                    writeln!(out)?;
                }
                first = false;
                if o.size {
                    writeln!(out, "{}", t.fill("group.each", &[("size", &size_text(g.size, o, t))]))?;
                }
                for c in files {
                    writeln!(out, "{}", c.path.to_string_lossy())?;
                }
            }
        }
        Format::Json => {
            writeln!(out, "[")?;
            for (i, g) in groups.iter().enumerate() {
                let files: Vec<String> = listed(g, o)
                    .iter()
                    .map(|c| format!("{{\"path\": {}, \"modified\": {}}}", json_string(&c.path.to_string_lossy()), c.mtime))
                    .collect();
                let comma = if i + 1 < groups.len() { "," } else { "" };
                writeln!(
                    out,
                    "  {{\"size\": {}, \"wasted\": {}, \"hash\": \"{}\", \"files\": [{}]}}{comma}",
                    g.size,
                    g.wasted(),
                    hex(&g.hash),
                    files.join(", ")
                )?;
            }
            writeln!(out, "]")?;
        }
        Format::Csv => {
            writeln!(out, "group,size,hash,path,modified")?;
            for (i, g) in groups.iter().enumerate() {
                for c in listed(g, o) {
                    writeln!(out, "{},{},{},{},{}", i + 1, g.size, hex(&g.hash), csv_field(&c.path.to_string_lossy()), c.mtime)?;
                }
            }
        }
    }
    Ok(())
}

/// How many copies there are and the space they take: all but one file of each group.
fn summary(out: &mut dyn Write, groups: &[Group], o: &Opts, t: &Texts) -> io::Result<()> {
    if groups.is_empty() {
        return writeln!(out, "{}", t.get("summary.none"));
    }
    let files: u64 = groups.iter().map(|g| g.files.len() as u64 - 1).sum();
    let wasted: u64 = groups.iter().map(Group::wasted).sum();
    writeln!(
        out,
        "{}",
        t.fill("summary.some", &[("files", &files.to_string()), ("sets", &groups.len().to_string()), ("size", &size_text(wasted, o, t))])
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::parse;
    use rt_dups::search::Copy;
    use std::path::PathBuf;

    fn opts(s: &str) -> Opts {
        parse(&s.split_whitespace().map(String::from).collect::<Vec<_>>()).unwrap()
    }

    fn group(size: u64, paths: &[&str]) -> Group {
        Group { size, hash: [0xab; 32], files: paths.iter().map(|p| Copy { path: PathBuf::from(p), mtime: 7 }).collect() }
    }

    fn groups() -> Vec<Group> {
        vec![group(300_000, &["a/big.bin", "b/big copy.bin", "c/big.bin"]), group(1000, &["a/n.txt", "b/n.txt"])]
    }

    fn text(o: &Opts, groups: &[Group]) -> String {
        let mut out = Vec::new();
        write(&mut out, groups, o, &Texts::new(include_str!("strings.txt"), "en")).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn a_file_per_line_and_a_blank_line_between_groups() {
        assert_eq!(text(&opts(""), &groups()), "a/big.bin\nb/big copy.bin\nc/big.bin\n\na/n.txt\nb/n.txt\n");
        assert_eq!(text(&opts(""), &[]), "");
    }

    #[test]
    fn sizes_come_before_each_group() {
        assert_eq!(text(&opts("-S"), &groups()[1..]), "1000 bytes each:\na/n.txt\nb/n.txt\n");
        assert_eq!(text(&opts("-Sh"), &groups()[1..]), "1000 bytes each:\na/n.txt\nb/n.txt\n".replace("1000 bytes", "1000"));
        assert!(text(&opts("-Sh"), &groups()).starts_with("293K each:\n"));
    }

    #[test]
    fn the_first_of_each_group_can_be_left_out() {
        assert_eq!(text(&opts("-f"), &groups()), "b/big copy.bin\nc/big.bin\n\nb/n.txt\n");
    }

    #[test]
    fn a_group_per_line_quotes_spaces() {
        assert_eq!(text(&opts("-1"), &groups()), "a/big.bin \"b/big copy.bin\" c/big.bin\na/n.txt b/n.txt\n");
        assert_eq!(text(&opts("-1 -S"), &groups()[1..]), "1000 bytes each: a/n.txt b/n.txt\n");
    }

    #[test]
    fn the_summary_counts_the_copies() {
        assert_eq!(text(&opts("-m"), &groups()), "3 duplicate files (in 2 sets), occupying 601000 bytes\n");
        assert_eq!(text(&opts("-mh"), &groups()), "3 duplicate files (in 2 sets), occupying 587K\n");
        assert_eq!(text(&opts("-m"), &[]), "No duplicates found.\n");
    }

    #[test]
    fn json_lists_the_groups() {
        let o = opts("--json");
        assert_eq!(
            text(&o, &groups()[1..]),
            format!("[\n  {{\"size\": 1000, \"wasted\": 1000, \"hash\": \"{}\", \"files\": [{{\"path\": \"a/n.txt\", \"modified\": 7}}, {{\"path\": \"b/n.txt\", \"modified\": 7}}]}}\n]\n", "ab".repeat(32))
        );
        assert_eq!(text(&o, &[]), "[\n]\n");
        assert!(text(&o, &[group(1, &["a\"b\\c", "d"])]).contains("\"a\\\"b\\\\c\""));
    }

    #[test]
    fn csv_has_a_row_per_file() {
        let o = opts("--csv");
        let out = text(&o, &[group(5, &["x,y", "z"])]);
        let hash = "ab".repeat(32);
        assert_eq!(out, format!("group,size,hash,path,modified\n1,5,{hash},\"x,y\",7\n1,5,{hash},z,7\n"));
    }

    #[test]
    fn texts_come_in_the_language_asked() {
        let mut out = Vec::new();
        write(&mut out, &groups(), &opts("-m"), &Texts::new(include_str!("strings.txt"), "pt")).unwrap();
        assert!(String::from_utf8(out).unwrap().starts_with("3 ficheiros duplicados (em 2 conjuntos)"));
    }
}
