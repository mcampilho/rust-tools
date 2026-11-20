//! The binary codec of the payloads: little-endian integers and strings
//! with a length. Decoding is strict: every read is checked against the
//! bytes left, and a payload with bytes left over is an error.

use std::fmt;

/// What can be wrong with a payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The payload ends before the value does.
    Short,
    /// A string that is not UTF-8, or a length beyond what is allowed.
    Invalid,
    /// Bytes left after the last field.
    Trailing,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DecodeError::Short => "the message ends too soon",
            DecodeError::Invalid => "the message has an invalid value",
            DecodeError::Trailing => "the message has bytes left over",
        })
    }
}

impl std::error::Error for DecodeError {}

/// The longest string or byte run a payload may hold.
pub const MAX_ITEM: usize = 1 << 20;

/// Builds a payload.
#[derive(Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Writer {
        Writer::default()
    }

    pub fn u8(&mut self, v: u8) -> &mut Writer {
        self.buf.push(v);
        self
    }

    pub fn u16(&mut self, v: u16) -> &mut Writer {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn u32(&mut self, v: u32) -> &mut Writer {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn u64(&mut self, v: u64) -> &mut Writer {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn i64(&mut self, v: i64) -> &mut Writer {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn bool(&mut self, v: bool) -> &mut Writer {
        self.u8(u8::from(v))
    }

    /// A string: its length in bytes, then the bytes. Longer than
    /// [`MAX_ITEM`] it is cut at a character boundary.
    pub fn str(&mut self, s: &str) -> &mut Writer {
        let mut end = s.len().min(MAX_ITEM);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.u32(end as u32);
        self.buf.extend_from_slice(&s.as_bytes()[..end]);
        self
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

/// Reads a payload.
pub struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Reader<'a> {
        Reader { buf, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.buf.len()).ok_or(DecodeError::Short)?;
        let s = &self.buf[self.at..end];
        self.at = end;
        Ok(s)
    }

    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().map_err(|_| DecodeError::Short)?))
    }

    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| DecodeError::Short)?))
    }

    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().map_err(|_| DecodeError::Short)?))
    }

    pub fn i64(&mut self) -> Result<i64, DecodeError> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().map_err(|_| DecodeError::Short)?))
    }

    pub fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::Invalid),
        }
    }

    pub fn str(&mut self) -> Result<&'a str, DecodeError> {
        let n = self.u32()? as usize;
        if n > MAX_ITEM {
            return Err(DecodeError::Invalid);
        }
        std::str::from_utf8(self.take(n)?).map_err(|_| DecodeError::Invalid)
    }

    /// A count of items that each take at least `min_each` bytes: refuses a
    /// count the rest of the payload cannot hold, so a hostile count does not
    /// make the caller reserve memory.
    pub fn count(&mut self, min_each: usize) -> Result<usize, DecodeError> {
        let n = self.u32()? as usize;
        if n.checked_mul(min_each.max(1)).is_none_or(|b| b > self.buf.len() - self.at) {
            return Err(DecodeError::Invalid);
        }
        Ok(n)
    }

    /// Succeeds only when the whole payload was read.
    pub fn end(&self) -> Result<(), DecodeError> {
        if self.at == self.buf.len() {
            Ok(())
        } else {
            Err(DecodeError::Trailing)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip() {
        let mut w = Writer::new();
        w.u8(7).u16(513).u32(70_000).u64(u64::MAX).i64(-5).bool(true).str("olá, mundo");
        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.u8(), Ok(7));
        assert_eq!(r.u16(), Ok(513));
        assert_eq!(r.u32(), Ok(70_000));
        assert_eq!(r.u64(), Ok(u64::MAX));
        assert_eq!(r.i64(), Ok(-5));
        assert_eq!(r.bool(), Ok(true));
        assert_eq!(r.str(), Ok("olá, mundo"));
        assert_eq!(r.end(), Ok(()));
    }

    #[test]
    fn short_payloads_are_errors_not_panics() {
        let mut w = Writer::new();
        w.str("abc");
        let bytes = w.finish();
        for cut in 0..bytes.len() {
            let mut r = Reader::new(&bytes[..cut]);
            assert_eq!(r.str(), Err(DecodeError::Short), "cut at {cut}");
        }
        assert_eq!(Reader::new(&[]).u64(), Err(DecodeError::Short));
    }

    #[test]
    fn hostile_lengths_are_refused() {
        // A string that claims 4 GB.
        let mut r = Reader::new(&[0xff, 0xff, 0xff, 0xff, b'a']);
        assert_eq!(r.str(), Err(DecodeError::Invalid));
        // A count the payload cannot hold.
        let mut r = Reader::new(&[0xff, 0xff, 0xff, 0x0f]);
        assert_eq!(r.count(4), Err(DecodeError::Invalid));
        // Not UTF-8, a bool that is neither, bytes left.
        assert_eq!(Reader::new(&[1, 0, 0, 0, 0xff]).str(), Err(DecodeError::Invalid));
        assert_eq!(Reader::new(&[2]).bool(), Err(DecodeError::Invalid));
        let mut r = Reader::new(&[1, 2]);
        r.u8().unwrap();
        assert_eq!(r.end(), Err(DecodeError::Trailing));
    }

    #[test]
    fn a_long_string_is_cut_on_a_character() {
        let long = "é".repeat(MAX_ITEM);
        let mut w = Writer::new();
        w.str(&long);
        let bytes = w.finish();
        let s = Reader::new(&bytes).str().unwrap().to_string();
        assert!(s.len() <= MAX_ITEM && s.chars().all(|c| c == 'é'));
    }
}
