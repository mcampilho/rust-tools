//! Loading and saving files safely.
//!
//! Saving writes to a temporary file in the same folder and then replaces
//! the target (`ReplaceFileW` on Windows keeps the original's attributes and
//! permissions). If that is not possible, it falls back to writing in place.
//! A file is never modified except by an explicit save.

use crate::i18n::{tf, tr};
use super::encoding::{decode, system_legacy, Decoded, EncodeError, Enc, Eol, Writer};
use ropey::Rope;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Files larger than this are refused (memory use is ~3x the size while loading).
pub const MAX_FILE_SIZE: u64 = 256 * 1024 * 1024;
/// Above this size, syntax highlighting, folding and word wrap start disabled.
pub const LARGE_FILE_SIZE: u64 = 16 * 1024 * 1024;

/// What we know about the file on disk, to detect external changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub modified: Option<SystemTime>,
    pub len: u64,
}

pub fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    Some(Stamp { modified: m.modified().ok(), len: m.len() })
}

#[derive(Debug)]
pub enum LoadError {
    NotFound,
    PermissionDenied,
    TooLarge(u64),
    IsDirectory,
    Other(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::NotFound => write!(f, "{}", tr("The file does not exist.")),
            LoadError::PermissionDenied => write!(f, "{}", tr("Access to the file was denied.")),
            LoadError::TooLarge(n) => write!(
                f,
                "{}",
                tf(
                    "The file is {} MB. Files larger than {} MB are not supported.",
                    &[&format!("{:.0}", *n as f64 / 1048576.0), &(MAX_FILE_SIZE / 1048576).to_string()]
                )
            ),
            LoadError::IsDirectory => write!(f, "{}", tr("The path is a folder, not a file.")),
            LoadError::Other(e) => write!(f, "{e}"),
        }
    }
}

fn io_err(e: std::io::Error) -> LoadError {
    match e.kind() {
        std::io::ErrorKind::NotFound => LoadError::NotFound,
        std::io::ErrorKind::PermissionDenied => LoadError::PermissionDenied,
        _ => LoadError::Other(e.to_string()),
    }
}

pub struct Loaded {
    pub decoded: Decoded,
    pub stamp: Option<Stamp>,
    pub read_only: bool,
}

/// Reads and decodes a file. `forced` reopens with a specific encoding.
pub fn load(path: &Path, forced: Option<Enc>) -> Result<Loaded, LoadError> {
    let meta = std::fs::metadata(path).map_err(io_err)?;
    if meta.is_dir() {
        return Err(LoadError::IsDirectory);
    }
    if meta.len() > MAX_FILE_SIZE {
        return Err(LoadError::TooLarge(meta.len()));
    }
    let bytes = std::fs::read(path).map_err(io_err)?;
    let decoded = decode(&bytes, forced, system_legacy());
    Ok(Loaded { decoded, stamp: stamp(path), read_only: meta.permissions().readonly() })
}

#[derive(Debug)]
pub enum SaveError {
    Encode(EncodeError),
    ReadOnly,
    PermissionDenied,
    Io(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Encode(e) => write!(f, "{}", tf("The text cannot be saved in this encoding: {}. Choose another encoding (for example UTF-8).", &[&e.to_string()])),
            SaveError::ReadOnly => write!(f, "{}", tr("The file is read-only.")),
            SaveError::PermissionDenied => write!(f, "{}", tr("Access was denied. You may need to save it in another folder.")),
            SaveError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Encodes the whole rope (nothing touches the disk if encoding fails).
pub fn encode_rope(text: &Rope, enc: Enc, eol: Eol) -> Result<Vec<u8>, EncodeError> {
    let mut w = Writer::new(enc, eol);
    let mut out = Vec::with_capacity(text.len_bytes() + text.len_lines() + 4);
    out.extend_from_slice(w.bom());
    for chunk in text.chunks() {
        w.write(chunk, &mut out)?;
    }
    w.finish(&mut out);
    Ok(out)
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(format!(".{name}.rpad-{}.tmp", std::process::id()))
}

/// Saves `text` to `path`. Returns the new stamp.
pub fn save(path: &Path, text: &Rope, enc: Enc, eol: Eol) -> Result<Option<Stamp>, SaveError> {
    let bytes = encode_rope(text, enc, eol).map_err(SaveError::Encode)?;
    write_bytes(path, &bytes)?;
    Ok(stamp(path))
}

pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), SaveError> {
    if let Ok(m) = std::fs::metadata(path) {
        if m.permissions().readonly() {
            return Err(SaveError::ReadOnly);
        }
        // Renaming over a file only takes rights on the folder.
        #[cfg(not(windows))]
        if std::fs::OpenOptions::new().write(true).open(path).is_err_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied) {
            return Err(SaveError::ReadOnly);
        }
    }
    let tmp = temp_path(path);
    let staged = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if staged.is_ok() && replace(&tmp, path).is_ok() {
        return Ok(());
    }
    let _ = std::fs::remove_file(&tmp);
    // Fallback: write in place (e.g. no permission to create files in the folder).
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::PermissionDenied => SaveError::PermissionDenied,
        _ => SaveError::Io(e.to_string()),
    })?;
    f.write_all(bytes).map_err(|e| SaveError::Io(e.to_string()))?;
    f.sync_all().map_err(|e| SaveError::Io(e.to_string()))
}

#[cfg(windows)]
fn replace(tmp: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
    if !target.exists() {
        return std::fs::rename(tmp, target);
    }
    let w = |p: &Path| p.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (t, r) = (w(target), w(tmp));
    // SAFETY: valid NUL-terminated wide paths; no backup file; reserved args null.
    let ok = unsafe { ReplaceFileW(t.as_ptr(), r.as_ptr(), std::ptr::null(), 0, std::ptr::null(), std::ptr::null()) };
    if ok != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
fn replace(tmp: &Path, target: &Path) -> std::io::Result<()> {
    if let Ok(m) = std::fs::metadata(target) {
        let _ = std::fs::set_permissions(tmp, m.permissions());
    }
    // A symbolic link stays one: the file it points to is what changes.
    let real = std::fs::canonicalize(target).unwrap_or_else(|_| target.to_path_buf());
    std::fs::rename(tmp, real)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("rpad-io-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(not(windows))]
    #[test]
    fn saving_keeps_symbolic_links_and_respects_permissions() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let d = dir();
        let (real, link) = (d.join("real.txt"), d.join("link.txt"));
        std::fs::write(&real, "old").unwrap();
        let _ = std::fs::remove_file(&link);
        symlink(&real, &link).unwrap();
        write_bytes(&link, b"new").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
        // Writable folder, file without write permission for its owner.
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o000)).unwrap();
        // SAFETY: geteuid has no preconditions; root ignores permissions.
        if unsafe { libc::geteuid() } != 0 {
            assert!(matches!(write_bytes(&real, b"x"), Err(SaveError::ReadOnly)));
        }
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
    }

    #[test]
    fn save_and_reload_all_encodings_and_eols() {
        let d = dir();
        let text = Rope::from_str("Héllo wörld €\n2nd line\n");
        for enc in [Enc::Utf8, Enc::Utf8Bom, Enc::Utf16Le, Enc::Utf16Be, Enc::Legacy(encoding_rs::WINDOWS_1252)] {
            for eol in [Eol::Lf, Eol::CrLf, Eol::Cr] {
                let p = d.join("t.txt");
                save(&p, &text, enc, eol).unwrap();
                let back = load(&p, if enc.is_unicode() { None } else { Some(enc) }).unwrap();
                assert_eq!(back.decoded.text, text.to_string(), "{enc:?} {eol:?}");
                assert_eq!(back.decoded.eol, eol);
                assert_eq!(back.decoded.enc, enc);
            }
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn exact_bytes_are_preserved() {
        let d = dir();
        let p = d.join("crlf.txt");
        std::fs::write(&p, b"a\r\nb\r\n").unwrap();
        let l = load(&p, None).unwrap();
        save(&p, &Rope::from_str(&l.decoded.text), l.decoded.enc, l.decoded.eol).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"a\r\nb\r\n");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn errors() {
        let d = dir();
        assert!(matches!(load(&d.join("missing.txt"), None), Err(LoadError::NotFound)));
        assert!(matches!(load(&d, None), Err(LoadError::IsDirectory)));
        let p = d.join("ro.txt");
        std::fs::write(&p, "x").unwrap();
        let mut perm = std::fs::metadata(&p).unwrap().permissions();
        perm.set_readonly(true);
        std::fs::set_permissions(&p, perm.clone()).unwrap();
        assert!(load(&p, None).unwrap().read_only);
        assert!(matches!(save(&p, &Rope::from_str("y"), Enc::Utf8, Eol::Lf), Err(SaveError::ReadOnly)));
        assert_eq!(std::fs::read(&p).unwrap(), b"x");
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        std::fs::set_permissions(&p, perm).unwrap();
        // Unencodable text never reaches the disk.
        let r = save(&p, &Rope::from_str("😀"), Enc::Legacy(encoding_rs::WINDOWS_1252), Eol::Lf);
        assert!(matches!(r, Err(SaveError::Encode(_))));
        assert_eq!(std::fs::read(&p).unwrap(), b"x");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn stamps_change_on_write() {
        let d = dir();
        let p = d.join("s.txt");
        std::fs::write(&p, "one").unwrap();
        let a = stamp(&p).unwrap();
        std::fs::write(&p, "three").unwrap();
        let b = stamp(&p).unwrap();
        assert_ne!(a, b);
        let _ = std::fs::remove_dir_all(d);
    }
}
