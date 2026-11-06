//! Document types and loading. Everything except PDF is read and parsed
//! here (on a loader thread); PDF pages are rendered by the media worker.

pub mod csv;
pub mod pdf;
pub mod text;

use crate::i18n::{tf, tr};
use crate::rich::{self, RichDoc};
use std::path::{Path, PathBuf};

/// Files larger than this are refused (text, CSV, Markdown, HTML).
pub const MAX_SIZE: u64 = 512 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Csv,
    Markdown,
    Html,
    Pdf,
}

impl Kind {
    pub fn of(path: &Path) -> Kind {
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "pdf" => Kind::Pdf,
            "md" | "markdown" | "mdown" | "mkd" | "mkdn" => Kind::Markdown,
            "html" | "htm" | "xhtml" | "xht" | "shtml" => Kind::Html,
            "csv" | "tsv" => Kind::Csv,
            _ => Kind::Text,
        }
    }

    pub fn name(self) -> String {
        match self {
            Kind::Text => tr("Text"),
            Kind::Csv => "CSV".into(),
            Kind::Markdown => "Markdown".into(),
            Kind::Html => "HTML".into(),
            Kind::Pdf => "PDF".into(),
        }
    }
}

pub enum Content {
    Text(text::TextDoc),
    Csv(csv::CsvDoc),
    Rich(RichDoc),
    /// PDF: opened by the media worker.
    Pdf,
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|e| io_message(path, &e))?;
    if meta.is_dir() {
        return Err(tf("{} is a folder.", &[&path.display().to_string()]));
    }
    if meta.len() > MAX_SIZE {
        return Err(tf("{} is too large ({} MB). The limit is {} MB.", &[&path.display().to_string(), &(meta.len() >> 20).to_string(), &(MAX_SIZE >> 20).to_string()]));
    }
    std::fs::read(path).map_err(|e| io_message(path, &e))
}

fn io_message(path: &Path, e: &std::io::Error) -> String {
    let p = path.display().to_string();
    match e.kind() {
        std::io::ErrorKind::NotFound => tf("{} was not found.", &[&p]),
        std::io::ErrorKind::PermissionDenied => tf("Access to {} was denied.", &[&p]),
        _ => tf("Cannot read {}: {}", &[&p, &e.to_string()]),
    }
}

/// Reads and parses a document (not PDF).
pub fn load(path: &Path, kind: Kind) -> Result<Content, String> {
    if kind == Kind::Pdf {
        return Ok(Content::Pdf);
    }
    let bytes = read(path)?;
    let base: PathBuf = path.parent().map(Path::to_path_buf).unwrap_or_default();
    Ok(match kind {
        Kind::Text => Content::Text(text::TextDoc::from_bytes(&bytes)),
        Kind::Csv => Content::Csv(csv::CsvDoc::from_bytes(&bytes)),
        Kind::Markdown => {
            let d = crate::io::encoding::decode(&bytes, crate::io::encoding::system_legacy());
            let mut doc = rich::markdown::parse(&d.text);
            doc.base = base;
            doc.encoding = d.enc.name();
            Content::Rich(doc)
        }
        Kind::Html => {
            let (text, enc) = rich::html::decode(&bytes);
            let mut doc = rich::html::parse(&text, Some(&base));
            doc.base = base;
            doc.encoding = enc;
            Content::Rich(doc)
        }
        Kind::Pdf => Content::Pdf,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension() {
        assert_eq!(Kind::of(Path::new("a.PDF")), Kind::Pdf);
        assert_eq!(Kind::of(Path::new("readme.md")), Kind::Markdown);
        assert_eq!(Kind::of(Path::new("x.htm")), Kind::Html);
        assert_eq!(Kind::of(Path::new("data.csv")), Kind::Csv);
        assert_eq!(Kind::of(Path::new("notes.txt")), Kind::Text);
        assert_eq!(Kind::of(Path::new("LICENSE")), Kind::Text);
    }

    #[test]
    fn load_errors() {
        let e = load(Path::new("Z:\\definitely\\missing.txt"), Kind::Text).err().unwrap_or_default();
        assert!(e.contains("missing.txt"), "{e}");
    }
}

/// Timing of large files: `cargo test --release -- --ignored large_files --nocapture`
/// with `RVIEW_BIG_DIR` pointing at a folder holding big.log / big.csv.
#[cfg(test)]
mod perf {
    #[test]
    #[ignore]
    fn large_files() {
        let Ok(dir) = std::env::var("RVIEW_BIG_DIR") else { return };
        for name in ["big.log", "big.csv"] {
            let p = std::path::Path::new(&dir).join(name);
            let t = std::time::Instant::now();
            let c = super::load(&p, super::Kind::of(&p)).expect("load");
            let n = match &c {
                super::Content::Text(d) => d.line_count(),
                super::Content::Csv(d) => d.rows(),
                _ => 0,
            };
            eprintln!("{name}: {} MB, {n} lines/rows, loaded in {:?}", std::fs::metadata(&p).unwrap().len() >> 20, t.elapsed());
        }
    }
}
