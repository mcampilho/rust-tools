//! Local persistence in a small `key=value` text file.
//!
//! Location: next to the executable when a `<app>.portable` marker is there
//! (or, if the app asks for it, its settings file); otherwise `%APPDATA%\<app>`
//! on Windows or `$XDG_CONFIG_HOME/<app>` (`~/.config/<app>`) elsewhere. If
//! the location is not writable the app keeps working and reports that
//! nothing is saved.

use crate::i18n::{tf, tr};
use std::path::{Path, PathBuf};

/// Settings as ordered `key=value` pairs. A key may repeat (lists such as
/// history); [`Store::get`] returns the last value.
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

    /// The file text, starting with a `# header` comment line.
    pub fn serialize(&self, header: &str) -> String {
        let mut s = format!("# {header}\n");
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

    /// Sets `key`, replacing every earlier value.
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.pairs.retain(|(k, _)| k != key);
        self.pairs.push((key.to_string(), value.into()));
    }

    /// Adds another value for `key` (lists).
    pub fn push(&mut self, key: &str, value: impl Into<String>) {
        self.pairs.push((key.to_string(), value.into()));
    }
}

/// What an app stores and how it is found.
#[derive(Debug, Clone, Copy)]
pub struct Place {
    /// App name: the folder name and the `<app>.portable` marker.
    pub app: &'static str,
    /// Settings file name, e.g. `settings.ini`.
    pub file: &'static str,
    /// Whether the settings file next to the executable also means portable
    /// mode (as the marker does).
    pub file_marks_portable: bool,
}

/// Where data is stored, and whether it can be written.
#[derive(Debug, Clone)]
pub struct Location {
    pub place: Place,
    pub dir: Option<PathBuf>,
    pub portable: bool,
    /// Last error, shown in the app's settings.
    pub error: Option<String>,
}

impl Location {
    /// The data folder for `place`: portable, or the user's.
    pub fn detect(place: Place) -> Location {
        let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
        if let Some(d) = &exe_dir {
            let marker = d.join(format!("{}.portable", place.app));
            if marker.exists() || (place.file_marks_portable && d.join(place.file).exists()) {
                return Location {
                    place,
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
                place,
                dir: Some(b.join(place.app)),
                portable: false,
                error: None,
            },
            None => Location::nowhere(place),
        }
    }

    /// A fixed folder (tests, or a folder chosen by the app).
    pub fn at(place: Place, dir: PathBuf) -> Location {
        Location {
            place,
            dir: Some(dir),
            portable: true,
            error: None,
        }
    }

    /// No folder: nothing can be saved.
    pub fn nowhere(place: Place) -> Location {
        Location {
            place,
            dir: None,
            portable: false,
            error: Some(tr("No user data folder is available.").to_string()),
        }
    }

    pub fn file(&self, name: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(name))
    }

    /// The settings file.
    pub fn settings_file(&self) -> Option<PathBuf> {
        self.file(self.place.file)
    }

    pub fn load(&mut self) -> Store {
        let Some(path) = self.settings_file() else {
            return Store::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(t) => Store::parse(&t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Store::default(),
            Err(e) => {
                self.error = Some(tf("Could not read {}: {}", &[&path.display().to_string(), &e.to_string()]));
                Store::default()
            }
        }
    }

    pub fn read_text(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.file(name)?).ok()
    }

    /// Writes a file in the data folder atomically (temp file + rename).
    pub fn write_text(&self, name: &str, text: &str) -> std::io::Result<()> {
        let dir = self.dir.as_ref().ok_or_else(|| std::io::Error::other("no data folder"))?;
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)
    }

    /// Writes the settings atomically (temp file + rename).
    pub fn save(&mut self, store: &Store) -> bool {
        let Some(dir) = self.dir.clone() else { return false };
        let file = self.place.file;
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&dir)?;
            let tmp = dir.join(format!("{file}.tmp"));
            std::fs::write(&tmp, store.serialize(self.place.app))?;
            std::fs::rename(&tmp, dir.join(file))
        })();
        match result {
            Ok(()) => {
                self.error = None;
                true
            }
            Err(e) => {
                self.error = Some(tf(
                    "Settings can't be saved to {}: {}",
                    &[&dir.display().to_string(), &e.to_string()],
                ));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLACE: Place = Place {
        app: "rt-core-test",
        file: "settings.ini",
        file_marks_portable: false,
    };

    #[test]
    fn round_trip_with_escapes() {
        let mut s = Store::default();
        s.set("theme", "dark");
        s.push("history", "2 + 3 =\t5\t5");
        s.push("history", "a\\b\nc");
        s.set("dir", "C:\\a\\b\nc\t5");
        s.set("theme", "light");
        let text = s.serialize("test");
        assert!(text.starts_with("# test\n"));
        let back = Store::parse(&text);
        assert_eq!(back.get("theme"), Some("light"));
        assert_eq!(back.get("dir"), Some("C:\\a\\b\nc\t5"));
        let h: Vec<_> = back.get_all("history").collect();
        assert_eq!(h, vec!["2 + 3 =\t5\t5", "a\\b\nc"]);
        assert_eq!(back.get("missing"), None);
    }

    #[test]
    fn save_and_load_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!("rt-core-store-test-{}", std::process::id()));
        let mut loc = Location::at(PLACE, dir.clone());
        let mut s = Store::default();
        s.set("mode", "Scientific");
        assert!(loc.save(&s));
        assert_eq!(loc.load().get("mode"), Some("Scientific"));
        loc.write_text("sub/x.txt", "hello").unwrap();
        assert_eq!(loc.read_text("sub/x.txt").as_deref(), Some("hello"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unwritable_location_reports_error() {
        let mut loc = Location::nowhere(PLACE);
        assert!(loc.error.is_some());
        assert!(!loc.save(&Store::default()));
        assert_eq!(loc.load().get("x"), None);
        assert!(loc.write_text("x", "y").is_err());
    }
}
