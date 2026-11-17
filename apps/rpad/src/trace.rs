//! A small log of files handed from one rpad to another
//! (`handoff.log` in the settings folder), to find why a file chosen from
//! Explorer's menu is sometimes not opened by the rpad already running
//! (2026-11-17). Local only; it keeps the last 256 KB.

use std::io::Write;
use std::path::Path;

const MAX: u64 = 256 * 1024;

/// Adds a line, with the time and this process's id.
pub fn note(dir: Option<&Path>, text: &str) {
    let Some(d) = dir else { return };
    let f = d.join("handoff.log");
    if std::fs::metadata(&f).is_ok_and(|m| m.len() > MAX) {
        let _ = std::fs::rename(&f, d.join("handoff.old.log"));
    }
    let t = rt_sys::local_time();
    let line = format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02} [{}] {text}\n",
        t.year,
        t.month,
        t.day,
        t.hour,
        t.minute,
        t.second,
        std::process::id()
    );
    let _ = std::fs::create_dir_all(d);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&f) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// The paths as one line.
pub fn paths(p: &[std::path::PathBuf]) -> String {
    p.iter().map(|x| x.display().to_string()).collect::<Vec<_>>().join(" | ")
}
