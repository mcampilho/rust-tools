//! Privacy Sweep: the traces Windows keeps of what was done — recent
//! documents, the Run box's and Explorer's histories, the open and save
//! dialogs' lists, jump lists and the clipboard. Never the browsers' history,
//! cookies or passwords (the user's choice, 2026-11-22, as rclean's rule).
//! Registry traces are saved to a `.reg` file before they are deleted;
//! files (shortcuts and lists Windows makes again) are deleted.

use std::path::{Path, PathBuf};

/// Where a trace is kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// The files directly in these folders (not their subfolders).
    Files(Vec<PathBuf>),
    /// Keys under `HKEY_CURRENT_USER`, deleted whole (Windows makes them again).
    Keys(Vec<&'static str>),
    /// The clipboard and its history.
    Clipboard,
}

/// A kind of trace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trace {
    pub id: &'static str,
    /// Translation keys.
    pub name: &'static str,
    pub about: &'static str,
    pub places: Vec<Place>,
    /// Ticked unless the user says otherwise.
    pub default_on: bool,
}

const EXPLORER: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer";

/// Every trace; `recent` is the user's Recent folder (`%APPDATA%\Microsoft\Windows\Recent`).
pub fn traces(recent: &Path) -> Vec<Trace> {
    vec![
        Trace {
            id: "recent",
            name: "Recent documents",
            about: "The shortcuts to the files opened lately, and Explorer's list of them.",
            places: vec![Place::Files(vec![recent.to_path_buf()]), Place::Keys(vec!["RecentDocs"])],
            default_on: true,
        },
        Trace {
            id: "run",
            name: "Run history",
            about: "What was typed in the Run box (Windows+R).",
            places: vec![Place::Keys(vec!["RunMRU"])],
            default_on: true,
        },
        Trace {
            id: "explorer",
            name: "Explorer history",
            about: "The addresses typed in Explorer's address bar, and its searches.",
            places: vec![Place::Keys(vec!["TypedPaths", "WordWheelQuery"])],
            default_on: true,
        },
        Trace {
            id: "dialogs",
            name: "Open and save dialogs",
            about: "The files and folders the open and save dialogs remember.",
            places: vec![Place::Keys(vec!["ComDlg32\\OpenSavePidlMRU", "ComDlg32\\LastVisitedPidlMRU"])],
            default_on: true,
        },
        Trace {
            id: "jumplists",
            name: "Jump lists",
            about: "The recent items of the taskbar's and Start's jump lists. Items pinned in them go too.",
            places: vec![Place::Files(vec![recent.join("AutomaticDestinations"), recent.join("CustomDestinations")])],
            default_on: false,
        },
        Trace {
            id: "clipboard",
            name: "Clipboard",
            about: "What was copied last, and the clipboard's history (Windows+V).",
            places: vec![Place::Clipboard],
            default_on: false,
        },
    ]
}

/// The user's Recent folder.
pub fn recent_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Microsoft\\Windows\\Recent"))
}

/// What was found of a trace.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub files: Vec<PathBuf>,
    /// The keys that exist, as `regedit` writes them.
    pub keys: Vec<String>,
    /// How many entries: files, plus the values in the keys (their bookkeeping left out).
    pub count: usize,
    /// The clipboard: what is in it cannot be counted.
    pub clipboard: bool,
}

/// The full name of a key of `EXPLORER`.
pub fn key_name(rel: &str) -> String {
    format!("HKEY_CURRENT_USER\\{EXPLORER}\\{rel}")
}

/// Whether a value is only the list's bookkeeping (the order), not a trace.
pub fn bookkeeping(name: &str) -> bool {
    name.eq_ignore_ascii_case("MRUList") || name.eq_ignore_ascii_case("MRUListEx") || name.is_empty()
}

/// The files directly in `dir`, `desktop.ini` left out.
pub fn files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(it) = std::fs::read_dir(dir) else { return Vec::new() };
    it.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| !p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("desktop.ini")))
        .collect()
}

/// The values in a key and its subkeys, bookkeeping left out; None when the key is not there.
#[cfg(windows)]
fn values_in(rel: &str) -> Option<usize> {
    use rt_sys::win::reg::{Key, Root, View};
    fn count(k: &Key) -> usize {
        let own = k.values().iter().filter(|(n, _)| !bookkeeping(n)).count();
        own + k.subkeys().iter().filter_map(|s| k.sub(s, View::Bits64)).map(|s| count(&s)).sum::<usize>()
    }
    Key::open(Root::CurrentUser, &format!("{EXPLORER}\\{rel}"), View::Bits64).map(|k| count(&k))
}

#[cfg(not(windows))]
fn values_in(_rel: &str) -> Option<usize> {
    None
}

/// Looks at a trace.
pub fn measure(t: &Trace) -> Found {
    let mut f = Found::default();
    for p in &t.places {
        match p {
            Place::Files(dirs) => {
                for d in dirs {
                    f.files.extend(files_in(d));
                }
            }
            Place::Keys(keys) => {
                for k in keys {
                    // An empty key is nothing to sweep.
                    if let Some(n) = values_in(k).filter(|n| *n > 0) {
                        f.keys.push(key_name(k));
                        f.count += n;
                    }
                }
            }
            Place::Clipboard => f.clipboard = true,
        }
    }
    f.count += f.files.len();
    f
}

/// What a sweep did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Swept {
    pub done: usize,
    pub failed: usize,
    pub error: Option<String>,
}

/// Empties the clipboard and its history (PowerShell: Windows' own calls).
const CLIPBOARD: &str = "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.Clipboard]::Clear(); \
try { $null = [Windows.ApplicationModel.DataTransfer.Clipboard, Windows.ApplicationModel.DataTransfer, ContentType = WindowsRuntime]; \
$null = [Windows.ApplicationModel.DataTransfer.Clipboard]::ClearHistory() } catch {}; 'ok'";

/// Sweeps what `found` holds: the keys to `backup` (a `.reg` file) and deleted, the files deleted, the clipboard emptied.
pub fn sweep(found: &Found, backup: &Path) -> Swept {
    let mut s = Swept::default();
    #[cfg(windows)]
    if !found.keys.is_empty() {
        let entries: Vec<rt_sys::regfile::Entry> =
            found.keys.iter().map(|k| rt_sys::regfile::Entry { key: k.clone(), value: None }).collect();
        for r in rt_sys::regfile::save_then_delete(&entries, backup) {
            match r {
                Ok(()) => s.done += 1,
                Err(e) => {
                    s.failed += 1;
                    s.error.get_or_insert(e);
                }
            }
        }
    }
    #[cfg(not(windows))]
    let _ = backup;
    for f in &found.files {
        match std::fs::remove_file(f) {
            Ok(()) => s.done += 1,
            // Gone already: nothing to do.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                s.failed += 1;
                s.error.get_or_insert(e.to_string());
            }
        }
    }
    if found.clipboard {
        match crate::ps::run(CLIPBOARD) {
            Some(t) if t.contains("ok") => s.done += 1,
            _ => {
                s.failed += 1;
                s.error.get_or_insert_with(|| "clipboard".into());
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_traces() {
        let recent = Path::new("R");
        let t = traces(recent);
        let ids: Vec<_> = t.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec!["recent", "run", "explorer", "dialogs", "jumplists", "clipboard"]);
        // Jump lists take pinned items, the clipboard what was copied: not ticked unless asked.
        assert!(t.iter().filter(|t| !t.default_on).map(|t| t.id).eq(["jumplists", "clipboard"]));
        assert!(t.iter().all(|t| !t.name.is_empty() && !t.about.is_empty() && !t.places.is_empty()));
        assert_eq!(key_name("RunMRU"), "HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\RunMRU");
        assert!(bookkeeping("MRUListEx") && bookkeeping("mrulist") && !bookkeeping("a"));
    }

    #[test]
    fn files_are_found_and_swept() {
        let dir = std::env::temp_dir().join(format!("rcare-privacy-{}", std::process::id()));
        let sub = dir.join("AutomaticDestinations");
        std::fs::create_dir_all(&sub).unwrap();
        for n in ["a.lnk", "b.lnk", "desktop.ini"] {
            std::fs::write(dir.join(n), "x").unwrap();
        }
        std::fs::write(sub.join("c.automaticDestinations-ms"), "x").unwrap();
        let t = Trace { id: "t", name: "n", about: "a", places: vec![Place::Files(vec![dir.clone()])], default_on: true };
        let f = measure(&t);
        // Only the folder's own files, desktop.ini left out.
        assert_eq!((f.files.len(), f.count, f.keys.len(), f.clipboard), (2, 2, 0, false));
        let s = sweep(&f, &dir.join("copy.reg"));
        assert_eq!((s.done, s.failed), (2, 0));
        assert!(!dir.join("a.lnk").exists() && dir.join("desktop.ini").exists() && sub.join("c.automaticDestinations-ms").exists());
        // Swept again: what is gone already is not a failure.
        assert_eq!(sweep(&f, &dir.join("copy.reg")), Swept::default());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// What this PC has.
    #[test]
    #[ignore]
    fn here() {
        for t in traces(&recent_dir().unwrap()) {
            let f = measure(&t);
            println!("{}: {} ({} files, keys {:?})", t.id, f.count, f.files.len(), f.keys);
        }
    }
}
