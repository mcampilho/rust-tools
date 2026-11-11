//! Pictures in the user's folders, at random, passing over the ones shown
//! lately. Links and junctions are not followed.

use super::{seed, Pick, Request, Source};
use crate::picture::has_picture_ext;
use std::path::{Path, PathBuf};

/// Pictures looked at, at most, across all the folders.
const MAX_FILES: usize = 100_000;
/// Folder depth below each chosen folder.
const MAX_DEPTH: usize = 16;

/// The pictures under `folders`.
pub fn list(folders: &[PathBuf], subfolders: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = folders.iter().map(|f| (f.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(t) = e.file_type() else { continue };
            let p = e.path();
            if t.is_dir() {
                if subfolders && depth < MAX_DEPTH && !hidden(&p) {
                    stack.push((p, depth + 1));
                }
            } else if t.is_file() && has_picture_ext(&p) {
                out.push(p);
                if out.len() >= MAX_FILES {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

/// Folders starting with '.' (Linux) are skipped.
fn hidden(p: &Path) -> bool {
    p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

fn id_of(p: &Path) -> String {
    format!("file:{}", p.display())
}

pub fn pick(req: &Request) -> Result<Pick, String> {
    if req.folders.is_empty() {
        return Err("no folder chosen".into());
    }
    let all = list(&req.folders, req.subfolders);
    if all.is_empty() {
        return Err("no pictures in the chosen folders".into());
    }
    let new: Vec<&PathBuf> = all.iter().filter(|p| !req.history.contains(&id_of(p))).collect();
    let pool: Vec<&PathBuf> = if new.is_empty() { all.iter().collect() } else { new };
    let file = pool[(seed() % pool.len() as u64) as usize].clone();
    Ok(Pick {
        source: Source::Folders,
        id: id_of(&file),
        title: file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        credit: file.parent().map(|p| p.display().to_string()).unwrap_or_default(),
        link: String::new(),
        file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_pictures_and_avoids_repeats() {
        let d = std::env::temp_dir().join(format!("rwall-folders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub/deeper")).unwrap();
        std::fs::create_dir_all(d.join(".hidden")).unwrap();
        for f in ["a.jpg", "b.PNG", "notes.txt", "sub/c.webp", "sub/deeper/d.jpeg", ".hidden/e.jpg"] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        assert_eq!(list(std::slice::from_ref(&d), true).len(), 4);
        assert_eq!(list(std::slice::from_ref(&d), false).len(), 2);
        let all = list(std::slice::from_ref(&d), true);
        let history: Vec<String> = all.iter().skip(1).map(|p| id_of(p)).collect();
        let req = Request { folders: vec![d.clone()], subfolders: true, history, ..Request::default() };
        for _ in 0..5 {
            assert_eq!(pick(&req).unwrap().file, all[0]);
        }
        assert!(pick(&Request::default()).is_err());
        let _ = std::fs::remove_dir_all(d);
    }
}
