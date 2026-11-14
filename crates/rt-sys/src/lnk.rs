//! Windows shortcuts (`.lnk`, the Shell Link format of MS-SHLLINK), read
//! without COM: what a shortcut opens. Plain bytes, so it works (and is
//! tested) on any system; only expanding `%VARIABLES%` is Windows'.

use std::path::{Path, PathBuf};

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A NUL-terminated string at `at`: UTF-16 or one byte a character.
fn str_at(b: &[u8], at: usize, wide: bool) -> Option<String> {
    let rest = b.get(at..)?;
    if wide {
        let w: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|&c| c != 0).collect();
        Some(String::from_utf16_lossy(&w))
    } else {
        // The system's code page; Latin-1 keeps the common accented letters.
        Some(rest.iter().take_while(|&&c| c != 0).map(|&c| c as char).collect())
    }
}

/// The file a shortcut opens, from its bytes: the LinkInfo's local path,
/// else the relative path from the shortcut's folder (`lnk_dir`), else the
/// target with environment variables (`EnvironmentVariableDataBlock`).
/// None for what is not a shortcut, and for "advertised" Windows Installer
/// shortcuts, which name a product instead of a file.
pub fn target_of(bytes: &[u8], lnk_dir: &Path) -> Option<PathBuf> {
    if u32_at(bytes, 0)? != 0x4C {
        return None;
    }
    let flags = u32_at(bytes, 0x14)?;
    let wide = flags & 0x80 != 0;
    let mut at = 0x4C;
    if flags & 0x01 != 0 {
        at += 2 + u16_at(bytes, at)? as usize;
    }
    let mut local: Option<String> = None;
    if flags & 0x02 != 0 {
        let info = at;
        let size = u32_at(bytes, info)? as usize;
        let header = u32_at(bytes, info + 4)? as usize;
        let li_flags = u32_at(bytes, info + 8)?;
        if li_flags & 1 != 0 {
            let base = u32_at(bytes, info + 16)? as usize;
            let suffix = u32_at(bytes, info + 24)? as usize;
            let (base_w, suffix_w) = if header >= 0x24 { (u32_at(bytes, info + 28)? as usize, u32_at(bytes, info + 32)? as usize) } else { (0, 0) };
            let b = if base_w > 0 { str_at(bytes, info + base_w, true)? } else { str_at(bytes, info + base, false)? };
            let s = if suffix_w > 0 { str_at(bytes, info + suffix_w, true)? } else { str_at(bytes, info + suffix, false).unwrap_or_default() };
            local = Some(format!("{b}{s}"));
        }
        at = info + size;
    }
    // StringData: name, relative path, working dir, arguments, icon.
    let mut relative = None;
    for (bit, keep) in [(0x04, false), (0x08, true), (0x10, false), (0x20, false), (0x40, false)] {
        if flags & bit == 0 {
            continue;
        }
        let n = u16_at(bytes, at)? as usize;
        let len = if wide { n * 2 } else { n };
        if keep {
            let s = bytes.get(at + 2..at + 2 + len)?;
            relative = Some(if wide { String::from_utf16_lossy(&s.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>()) } else { s.iter().map(|&c| c as char).collect() });
        }
        at += 2 + len;
    }
    if let Some(l) = local.filter(|l| !l.is_empty()) {
        return Some(PathBuf::from(l));
    }
    // Extra data blocks: the target with environment variables.
    while let (Some(size), Some(sig)) = (u32_at(bytes, at), u32_at(bytes, at + 4)) {
        if size < 8 {
            break;
        }
        if sig == 0xA000_0001 {
            let t = str_at(bytes, at + 8 + 260, true).filter(|s| !s.is_empty()).or_else(|| str_at(bytes, at + 8, false))?;
            return Some(PathBuf::from(expand(&t)));
        }
        at += size as usize;
    }
    relative.map(|r| lnk_dir.join(r))
}

#[cfg(windows)]
fn expand(s: &str) -> String {
    crate::win::reg::expand(s)
}

#[cfg(not(windows))]
fn expand(s: &str) -> String {
    s.to_string()
}

/// A shortcut file opening `target` (a LinkInfo local path, Unicode
/// strings), with a relative path if given: for tests.
pub fn make(target: &str, relative: Option<&str>) -> Vec<u8> {
    let mut b = vec![0u8; 0x4C];
    b[0..4].copy_from_slice(&0x4Cu32.to_le_bytes());
    let flags: u32 = 0x02 | 0x80 | if relative.is_some() { 0x08 } else { 0 };
    b[0x14..0x18].copy_from_slice(&flags.to_le_bytes());
    // LinkInfo: header of 0x1C, the local path after it, an empty suffix.
    let mut li = vec![0u8; 0x1C];
    let path_at = li.len();
    li.extend(target.as_bytes());
    li.push(0);
    let suffix_at = li.len();
    li.push(0);
    let size = li.len() as u32;
    li[0..4].copy_from_slice(&size.to_le_bytes());
    li[4..8].copy_from_slice(&0x1Cu32.to_le_bytes());
    li[8..12].copy_from_slice(&1u32.to_le_bytes());
    li[16..20].copy_from_slice(&(path_at as u32).to_le_bytes());
    li[24..28].copy_from_slice(&(suffix_at as u32).to_le_bytes());
    b.extend(li);
    if let Some(r) = relative {
        let w: Vec<u16> = r.encode_utf16().collect();
        b.extend((w.len() as u16).to_le_bytes());
        for c in w {
            b.extend(c.to_le_bytes());
        }
    }
    b.extend([0u8; 4]);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_targets() {
        let d = Path::new("C:\\Menu");
        assert_eq!(target_of(&make("C:\\Program Files\\App\\app.exe", None), d), Some(PathBuf::from("C:\\Program Files\\App\\app.exe")));
        assert_eq!(target_of(&make("", Some("..\\x.exe")), d), Some(d.join("..\\x.exe")));
        assert_eq!(target_of(b"not a shortcut at all, no", d), None);
        assert_eq!(target_of(&[0x4C, 0, 0, 0], d), None);
    }
}
