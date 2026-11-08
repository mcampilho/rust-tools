//! User preferences in `settings.ini`. Missing or invalid values fall back
//! to defaults.

use crate::filter::{Choice, Field, KINDS};
use crate::folders::SHARES;
use crate::i18n::Lang;
use crate::storage::Store;
use std::path::PathBuf;

/// The smallest file sizes offered with one click (1: any size). Below a few
/// hundred kilobytes nearly every file shares its size with another, so each
/// one has to be opened: 1 MB leaves a fraction of the files with most of the
/// space.
pub const MIN_SIZES: [u64; 6] = [1, 4 << 10, 64 << 10, 1 << 20, 10 << 20, 100 << 20];

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 0 follows the system, 1 light, 2 dark.
    pub theme: u8,
    /// Interface language; None follows the system.
    pub language: Option<Lang>,
    /// The drives and folders chosen to search, kept for the next time.
    pub roots: Vec<PathBuf>,
    /// The folders added, ticked or not.
    pub folders: Vec<PathBuf>,
    /// The filters, as typed.
    pub choice: Choice,
    /// Last folder chosen in the folder dialog.
    pub last_dir: String,
    /// Window size (logical pixels) and maximized state.
    pub window: Option<(f64, f64, bool)>,
    /// The least share in common of the folders shown (one of `folders::SHARES`).
    pub share: u16,
    /// Look for duplicate files, and for duplicate folders.
    pub compare: [bool; 2],
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            theme: 0,
            language: None,
            roots: Vec::new(),
            folders: Vec::new(),
            choice: Choice::default(),
            last_dir: String::new(),
            window: None,
            share: DEFAULT_SHARE,
            compare: [true; 2],
        }
    }
}

/// Folders shown at first: identical ones and those with 90 % in common.
pub const DEFAULT_SHARE: u16 = 900;

/// Separates the roots in the file: a character no path can hold.
const ROOT_SEP: char = '|';

impl Settings {
    pub fn load(s: &Store) -> Settings {
        Settings {
            theme: match s.get("theme") {
                Some("light") => 1,
                Some("dark") => 2,
                _ => 0,
            },
            language: s.get("language").and_then(Lang::from_tag),
            roots: paths(s.get("roots")),
            folders: paths(s.get("folders")),
            choice: load_choice(s),
            last_dir: s.get("last_dir").unwrap_or("").to_string(),
            window: s.get("window").and_then(|v| {
                let mut it = v.split(',');
                let w: f64 = it.next()?.trim().parse().ok()?;
                let h: f64 = it.next()?.trim().parse().ok()?;
                let max = it.next() == Some("max");
                (w >= 200.0 && h >= 150.0 && w < 20000.0 && h < 20000.0).then_some((w, h, max))
            }),
            share: s.get("folder_share").and_then(|v| v.trim().parse().ok()).filter(|v| SHARES.contains(v)).unwrap_or(DEFAULT_SHARE),
            compare: [s.get("compare_files") != Some("no"), s.get("compare_folders") != Some("no")],
        }
    }

    pub fn to_store(&self) -> Store {
        let mut s = Store::default();
        s.set("theme", ["system", "light", "dark"][self.theme.min(2) as usize]);
        s.set("language", self.language.map_or("", |l| l.tag()));
        let roots: Vec<String> = self.roots.iter().map(|r| r.display().to_string()).collect();
        s.set("roots", roots.join(&ROOT_SEP.to_string()));
        let folders: Vec<String> = self.folders.iter().map(|r| r.display().to_string()).collect();
        s.set("folders", folders.join(&ROOT_SEP.to_string()));
        for f in Field::ALL {
            s.set(f.key(), self.choice.text(f));
        }
        s.set("folder_share", self.share.to_string());
        s.set("compare_files", if self.compare[0] { "yes" } else { "no" });
        s.set("compare_folders", if self.compare[1] { "yes" } else { "no" });
        s.set("last_dir", self.last_dir.clone());
        if let Some((w, h, max)) = self.window {
            s.set("window", format!("{w:.0},{h:.0},{}", if max { "max" } else { "normal" }));
        }
        s
    }
}

/// A list of paths, as saved.
fn paths(v: Option<&str>) -> Vec<PathBuf> {
    v.unwrap_or("").split(ROOT_SEP).filter(|r| !r.trim().is_empty()).map(PathBuf::from).collect()
}

/// The filters; a box never saved keeps its default.
fn load_choice(s: &Store) -> Choice {
    let mut c = Choice::default();
    for f in Field::ALL {
        if let Some(v) = s.get(f.key()) {
            c.texts[f.index()] = v.chars().filter(|c| !c.is_control()).take(500).collect();
        }
    }
    // Kinds ticked by an earlier version: their extensions.
    for id in s.get("kinds").unwrap_or("").split(',') {
        if let Some(i) = KINDS.iter().position(|k| k.id == id.trim()) {
            if !c.has_kind(i) {
                c.toggle_kind(i);
            }
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_bad_values() {
        let s = Settings {
            theme: 2,
            language: Some(Lang::PtPT),
            roots: vec![PathBuf::from("C:\\"), PathBuf::from("D:\\Fotos de família")],
            folders: vec![PathBuf::from("D:\\Fotos de família"), PathBuf::from("\\\\nas\\cópias")],
            choice: {
                let mut c = Choice::default();
                c.texts[Field::MinSize.index()] = String::new();
                c.texts[Field::SkipNames.index()] = "tmp* *cache*".into();
                c.texts[Field::To.index()] = "2024-12-31".into();
                c.toggle_kind(0);
                c.toggle_kind(2);
                c
            },
            last_dir: "C:\\data".into(),
            window: Some((900.0, 700.0, true)),
            share: 750,
            compare: [false, true],
        };
        assert_eq!(Settings::load(&Store::parse(&s.to_store().serialize("rdup"))), s);
        let bad = Settings::load(&Store::parse("theme=??\nwindow=abc\nlanguage=xx-YY\nroots=|\nkinds=nope,\nfolder_share=42\n"));
        assert_eq!(bad, Settings::default());
    }
}
