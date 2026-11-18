//! Minimal reader for Windows shortcut (.lnk) files, following [MS-SHLLINK]:
//! the target (LinkInfo or the environment-variable block), working folder,
//! arguments and icon location.

use crate::paths::{cp1252_decode, dirname, join, normpath, u16s};
use crate::sys::expand;

#[derive(Debug, Default, PartialEq)]
pub struct Shortcut {
    pub target: Option<String>,
    pub relative_path: Option<String>,
    pub working_dir: Option<String>,
    pub arguments: Option<String>,
    pub icon_location: Option<String>,
}

const HAS_ID_LIST: u32 = 0x01;
const HAS_LINK_INFO: u32 = 0x02;
const IS_UNICODE: u32 = 0x80;
const ENV_BLOCK: u32 = 0xA000_0001;
const HEADER_SIZE: usize = 0x4C;

fn u16_at(d: &[u8], p: usize) -> Option<usize> {
    Some(u16::from_le_bytes(d.get(p..p + 2)?.try_into().ok()?) as usize)
}

fn u32_at(d: &[u8], p: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(p..p + 4)?.try_into().ok()?))
}

fn cstr(d: &[u8], pos: usize, wide: bool) -> String {
    let d = d.get(pos..).unwrap_or(&[]);
    if wide {
        let units: Vec<u16> = u16s(d, false).into_iter().take_while(|&u| u != 0).collect();
        String::from_utf16_lossy(&units)
    } else {
        let end = d.iter().position(|&b| b == 0).unwrap_or(d.len());
        cp1252_decode(&d[..end])
    }
}

/// Parse the bytes of a .lnk file; None when it is not one.
pub fn parse(d: &[u8]) -> Option<Shortcut> {
    if d.len() < HEADER_SIZE || u32_at(d, 0)? as usize != HEADER_SIZE {
        return None;
    }
    let flags = u32_at(d, 0x14)?;
    let mut sc = Shortcut::default();
    let mut pos = HEADER_SIZE;
    if flags & HAS_ID_LIST != 0 {
        pos += 2 + u16_at(d, pos)?;
    }
    if flags & HAS_LINK_INFO != 0 {
        let size = u32_at(d, pos)? as usize;
        let header_size = u32_at(d, pos + 4)?;
        let info_flags = u32_at(d, pos + 8)?;
        if info_flags & 1 != 0 {
            let base_off = u32_at(d, pos + 16)? as usize;
            let suffix_off = u32_at(d, pos + 24)? as usize;
            let (mut base_w, mut suffix_w) = (0, 0);
            if header_size >= 0x24 {
                base_w = u32_at(d, pos + 28)? as usize;
                suffix_w = u32_at(d, pos + 32)? as usize;
            }
            let base = if base_w != 0 { cstr(d, pos + base_w, true) } else { cstr(d, pos + base_off, false) };
            let suffix = if suffix_w != 0 { cstr(d, pos + suffix_w, true) } else { cstr(d, pos + suffix_off, false) };
            sc.target = Some(base + &suffix);
        }
        pos += size;
    }
    let wide = flags & IS_UNICODE != 0;
    for (flag, slot) in [(0x04u32, 0), (0x08, 1), (0x10, 2), (0x20, 3), (0x40, 4)] {
        if flags & flag == 0 {
            continue;
        }
        let count = u16_at(d, pos)?;
        pos += 2;
        let n = if wide { count * 2 } else { count };
        let raw = d.get(pos..pos + n)?;
        pos += n;
        let text = if wide {
            String::from_utf16_lossy(&u16s(raw, false))
        } else {
            cp1252_decode(raw)
        };
        match slot {
            1 => sc.relative_path = Some(text),
            2 => sc.working_dir = Some(text),
            3 => sc.arguments = Some(text),
            4 => sc.icon_location = Some(text),
            _ => {}
        }
    }
    // Extra data blocks: the environment-variable target.
    while pos + 8 <= d.len() {
        let size = u32_at(d, pos)? as usize;
        if size < 8 {
            break;
        }
        if u32_at(d, pos + 4)? == ENV_BLOCK && size >= 8 + 260 + 520 && sc.target.as_deref().unwrap_or("").is_empty() {
            let w = cstr(d, pos + 8 + 260, true);
            let a = cstr(d, pos + 8, false);
            sc.target = Some(expand(if w.is_empty() { &a } else { &w }));
        }
        pos += size;
    }
    Some(sc)
}

/// Read a .lnk file; None when it cannot be read.
pub fn read(path: &str) -> Option<Shortcut> {
    let data = std::fs::read(path).ok()?;
    let mut sc = parse(&data[..data.len().min(1 << 20)])?;
    if sc.target.as_deref().unwrap_or("").is_empty() {
        if let Some(rel) = sc.relative_path.as_deref().filter(|r| !r.is_empty()) {
            sc.target = Some(normpath(&join(&dirname(path), rel)));
        }
    }
    sc.working_dir = sc.working_dir.map(|v| expand(&v));
    sc.icon_location = sc.icon_location.map(|v| expand(&v));
    Some(sc)
}

/// Bytes of a minimal .lnk (header, LinkInfo with LocalBasePath, strings): for tests.
#[cfg(test)]
pub fn build(target: &str, working_dir: Option<&str>, icon: Option<&str>, arguments: Option<&str>) -> Vec<u8> {
    let mut flags: u32 = 0x02 | 0x80;
    let mut strings = Vec::new();
    for (flag, value) in [(0x10u32, working_dir), (0x20, arguments), (0x40, icon)] {
        if let Some(v) = value {
            flags |= flag;
            let units: Vec<u16> = v.encode_utf16().collect();
            strings.extend_from_slice(&(units.len() as u16).to_le_bytes());
            for u in units {
                strings.extend_from_slice(&u.to_le_bytes());
            }
        }
    }
    let mut d = Vec::new();
    d.extend_from_slice(&0x4Cu32.to_le_bytes());
    d.extend_from_slice(&[0x01, 0x14, 0x02]);
    d.extend_from_slice(&[0; 13]);
    d.extend_from_slice(&flags.to_le_bytes());
    d.resize(0x4C, 0);
    let base: Vec<u8> = target.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }).chain([0]).collect();
    let volume: Vec<u8> = [16u32, 3, 0, 16].iter().flat_map(|v| v.to_le_bytes()).collect();
    let head = 0x1Cusize;
    let base_off = head + volume.len();
    let suffix_off = base_off + base.len();
    let body_len = volume.len() + base.len() + 1;
    for v in [(head + body_len) as u32, head as u32, 1, head as u32, base_off as u32, 0, suffix_off as u32] {
        d.extend_from_slice(&v.to_le_bytes());
    }
    d.extend_from_slice(&volume);
    d.extend_from_slice(&base);
    d.push(0);
    d.extend_from_slice(&strings);
    d.extend_from_slice(&0u32.to_le_bytes());
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_built() {
        let sc = parse(&build(r"C:\x\y.exe", Some(r"C:\x"), Some(r"C:\x\y.exe,0"), Some("-a"))).unwrap();
        assert_eq!(sc.target.as_deref(), Some(r"C:\x\y.exe"));
        assert_eq!(sc.working_dir.as_deref(), Some(r"C:\x"));
        assert_eq!(sc.arguments.as_deref(), Some("-a"));
        assert_eq!(sc.icon_location.as_deref(), Some(r"C:\x\y.exe,0"));
    }

    #[test]
    fn not_a_link() {
        assert!(parse(b"hello").is_none());
    }
}
