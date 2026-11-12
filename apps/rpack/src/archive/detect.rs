//! Finds an archive's format by its first bytes, whatever its name: a file
//! renamed, or with no extension, still opens. Only lzma and brotli, which
//! have no mark of their own, go by the extension.

use super::Codec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Found {
    Zip,
    SevenZ,
    Tar,
    /// Compressed whole: what is inside is found after decompressing.
    Codec(Codec),
}

/// The format of a file that starts with `head` (up to 1 KB) and is named
/// `name` (in lower case).
pub fn sniff(head: &[u8], name: &str) -> Option<Found> {
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") || head.starts_with(b"PK\x07\x08") {
        return Some(Found::Zip);
    }
    if head.starts_with(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C]) {
        return Some(Found::SevenZ);
    }
    if let Some(c) = codec_of(head, name) {
        return Some(Found::Codec(c));
    }
    is_tar(head).then_some(Found::Tar)
}

/// A compressed stream, by its mark (or its extension for the two without one).
fn codec_of(head: &[u8], name: &str) -> Option<Codec> {
    let ext = |e: &str| name.ends_with(e);
    if head.starts_with(&[0x1F, 0x8B]) {
        Some(Codec::Gzip)
    } else if head.len() >= 4 && head.starts_with(b"BZh") && (b'1'..=b'9').contains(&head[3]) {
        Some(Codec::Bzip2)
    } else if head.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
        Some(Codec::Xz)
    } else if head.starts_with(b"LZIP") {
        Some(Codec::Lzip)
    } else if head.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) || (head.len() >= 4 && head[0] & 0xF0 == 0x50 && head[1..4] == [0x2A, 0x4D, 0x18]) {
        // A frame, or a skippable frame first (pzstd).
        Some(Codec::Zstd)
    } else if head.starts_with(&[0x04, 0x22, 0x4D, 0x18]) {
        Some(Codec::Lz4)
    } else if (ext(".lzma") || ext(".tlzma")) && head.first().is_some_and(|&p| p < 225) {
        Some(Codec::Lzma)
    } else if ext(".br") || ext(".tbr") {
        Some(Codec::Brotli)
    } else {
        None
    }
}

/// A tar header: `ustar` at 257, or an old header whose checksum adds up.
pub fn is_tar(head: &[u8]) -> bool {
    if head.len() < 512 {
        return false;
    }
    if &head[257..262] == b"ustar" {
        return true;
    }
    // Old tar: the checksum (octal at 148..156) is the sum of the header's
    // bytes, with the checksum's own field counted as spaces.
    let field = &head[148..156];
    let digits: String = field.iter().map(|&b| b as char).filter(|c| c.is_ascii_digit()).collect();
    let Ok(stored) = u32::from_str_radix(&digits, 8) else { return false };
    let sum: u32 = head[..512].iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u32 }).sum();
    // An empty block would add up too (all spaces in the field).
    stored == sum && head[0] != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks() {
        assert_eq!(sniff(b"PK\x03\x04rest", "x.bin"), Some(Found::Zip));
        assert_eq!(sniff(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C, 0, 4], "x"), Some(Found::SevenZ));
        assert_eq!(sniff(&[0x1F, 0x8B, 8, 0], "a"), Some(Found::Codec(Codec::Gzip)));
        assert_eq!(sniff(b"BZh9", "a"), Some(Found::Codec(Codec::Bzip2)));
        assert_eq!(sniff(b"BZhx", "a"), None);
        assert_eq!(sniff(&[0xFD, b'7', b'z', b'X', b'Z', 0], "a"), Some(Found::Codec(Codec::Xz)));
        assert_eq!(sniff(b"LZIP\x01", "a"), Some(Found::Codec(Codec::Lzip)));
        assert_eq!(sniff(&[0x28, 0xB5, 0x2F, 0xFD], "a"), Some(Found::Codec(Codec::Zstd)));
        assert_eq!(sniff(&[0x04, 0x22, 0x4D, 0x18], "a"), Some(Found::Codec(Codec::Lz4)));
        assert_eq!(sniff(&[0x5D, 0, 0, 0x80, 0], "a.lzma"), Some(Found::Codec(Codec::Lzma)));
        assert_eq!(sniff(&[0x5D, 0, 0, 0x80, 0], "a.bin"), None);
        assert_eq!(sniff(b"anything", "a.br"), Some(Found::Codec(Codec::Brotli)));
        assert_eq!(sniff(b"plain text", "a.txt"), None);
    }

    #[test]
    fn tar_headers() {
        let mut h = [0u8; 512];
        h[..5].copy_from_slice(b"a.txt");
        h[257..262].copy_from_slice(b"ustar");
        assert!(is_tar(&h));
        // An old header: no `ustar`, a checksum that adds up.
        let mut old = [0u8; 512];
        old[..5].copy_from_slice(b"a.txt");
        old[156] = b'0';
        let sum: u32 = old.iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u32 }).sum();
        old[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        assert!(is_tar(&old));
        old[0] = b'b';
        assert!(!is_tar(&old));
        assert!(!is_tar(&[0u8; 512]));
        assert!(!is_tar(&[0u8; 100]));
    }
}
