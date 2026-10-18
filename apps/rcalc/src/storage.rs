//! Local persistence in a small `key=value` text file.
//!
//! Location: next to the executable when a `rcalc.portable` marker (or an
//! existing `rcalc.ini`) is there; otherwise `%APPDATA%\rcalc` on Windows or
//! `$XDG_CONFIG_HOME/rcalc` (`~/.config/rcalc`) elsewhere. If the location is
//! not writable the app keeps working and reports that nothing is saved.

use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "rcalc.ini";
pub const PORTABLE_MARKER: &str = "rcalc.portable";
pub const CURRENCY_FILE: &str = "currency.txt";

#[derive(Default, Debug, Clone)]
pub struct Store {
    pairs: Vec<(String, String)>,
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o
}

fn unescape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => o.push('\n'),
                Some('r') => o.push('\r'),
                Some('t') => o.push('\t'),
                Some(x) => o.push(x),
                None => {}
            }
        } else {
            o.push(c);
        }
    }
    o
}

impl Store {
    pub fn parse(text: &str) -> Store {
        let pairs = text
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim().to_string(), unescape(v)))
            .collect();
        Store { pairs }
    }

    pub fn serialize(&self) -> String {
        let mut s = String::from("# rcalc settings and history\n");
        for (k, v) in &self.pairs {
            s.push_str(k);
            s.push('=');
            s.push_str(&escape(v));
            s.push('\n');
        }
        s
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn get_all<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.pairs.iter().filter(move |(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.pairs.retain(|(k, _)| k != key);
        self.pairs.push((key.to_string(), value.into()));
    }

    pub fn push(&mut self, key: &str, value: impl Into<String>) {
        self.pairs.push((key.to_string(), value.into()));
    }
}

/// Where data is stored, and whether it can be written.
#[derive(Debug, Clone)]
pub struct Location {
    pub dir: Option<PathBuf>,
    pub portable: bool,
    /// Last error, shown in Settings.
    pub error: Option<String>,
}

impl Location {
    pub fn detect() -> Location {
        let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
        if let Some(d) = &exe_dir {
            if d.join(PORTABLE_MARKER).exists() || d.join(FILE_NAME).exists() {
                return Location {
                    dir: Some(d.clone()),
                    portable: true,
                    error: None,
                };
            }
        }
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        };
        match base {
            Some(b) => Location {
                dir: Some(b.join("rcalc")),
                portable: false,
                error: None,
            },
            None => Location {
                dir: None,
                portable: false,
                error: Some("No user data folder is available.".into()),
            },
        }
    }

    pub fn file(&self, name: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(name))
    }

    pub fn load(&mut self) -> Store {
        let Some(path) = self.file(FILE_NAME) else {
            return Store::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(t) => Store::parse(&t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Store::default(),
            Err(e) => {
                self.error = Some(format!("Could not read {}: {e}", path.display()));
                Store::default()
            }
        }
    }

    pub fn read_text(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.file(name)?).ok()
    }

    /// Writes atomically (temp file + rename).
    pub fn save(&mut self, store: &Store) -> bool {
        let Some(dir) = self.dir.clone() else { return false };
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&dir)?;
            let tmp = dir.join(format!("{FILE_NAME}.tmp"));
            std::fs::write(&tmp, store.serialize())?;
            std::fs::rename(&tmp, dir.join(FILE_NAME))
        })();
        match result {
            Ok(()) => {
                self.error = None;
                true
            }
            Err(e) => {
                self.error = Some(format!("Settings can't be saved to {}: {e}", dir.display()));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_escapes() {
        let mut s = Store::default();
        s.set("theme", "dark");
        s.push("history", "2 + 3 =\t5\t5");
        s.push("history", "a\\b\nc");
        s.set("theme", "light");
        let back = Store::parse(&s.serialize());
        assert_eq!(back.get("theme"), Some("light"));
        let h: Vec<_> = back.get_all("history").collect();
        assert_eq!(h, vec!["2 + 3 =\t5\t5", "a\\b\nc"]);
        assert_eq!(back.get("missing"), None);
    }

    #[test]
    fn save_and_load_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!("rcalc-test-{}", std::process::id()));
        let mut loc = Location {
            dir: Some(dir.clone()),
            portable: true,
            error: None,
        };
        let mut s = Store::default();
        s.set("mode", "Scientific");
        assert!(loc.save(&s));
        assert_eq!(loc.load().get("mode"), Some("Scientific"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unwritable_location_reports_error() {
        let mut loc = Location {
            dir: None,
            portable: false,
            error: None,
        };
        assert!(!loc.save(&Store::default()));
        assert_eq!(loc.load().get("x"), None);
    }
}
