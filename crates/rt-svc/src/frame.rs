//! Frames: what is sent on the pipe. `u32` length of what follows, `u8`
//! protocol version, `u16` kind, `u32` request number, then the payload.

use std::io::{self, Read, Write};

/// The protocol this build speaks.
pub const PROTOCOL: u8 = 1;
/// The oldest protocol it still understands.
pub const PROTOCOL_MIN: u8 = 1;

/// The bytes of a frame after the length: version, kind, number.
pub const HEADER: usize = 1 + 2 + 4;
/// The longest payload of a frame.
pub const MAX_PAYLOAD: usize = 1 << 20;
/// The longest payload of the frames that carry the entries of a scan.
pub const MAX_BULK: usize = 8 << 20;

/// What a frame is. Requests are below 100, answers from 100.
pub mod kind {
    pub const HELLO: u16 = 1;
    pub const PING: u16 = 2;
    pub const STATUS: u16 = 3;
    pub const SCAN_VOLUME: u16 = 4;
    /// A snapshot kept by the service (programs, what can be cleaned).
    pub const GET_SNAPSHOT: u16 = 5;
    /// Asks the service to make a snapshot again.
    pub const REFRESH: u16 = 6;

    pub const HELLO_OK: u16 = 100;
    pub const PONG: u16 = 101;
    pub const STATUS_OK: u16 = 102;
    pub const SCAN_BATCH: u16 = 103;
    pub const SCAN_DONE: u16 = 104;
    pub const ERROR: u16 = 105;
    /// The root and the time of what a scan reads, before its entries.
    pub const SCAN_START: u16 = 106;
    /// How far a scan has got.
    pub const SCAN_PROGRESS: u16 = 107;
    pub const SNAPSHOT: u16 = 108;
    pub const REFRESH_OK: u16 = 109;

    /// Frames that may be longer than `MAX_PAYLOAD`.
    pub fn is_bulk(kind: u16) -> bool {
        kind == SCAN_BATCH || kind == SNAPSHOT
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub version: u8,
    pub kind: u16,
    /// The number of the request: an answer carries the one it answers.
    pub id: u32,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(kind: u16, id: u32, payload: Vec<u8>) -> Frame {
        Frame { version: PROTOCOL, kind, id, payload }
    }
}

fn limit(kind: u16) -> usize {
    if kind::is_bulk(kind) {
        MAX_BULK
    } else {
        MAX_PAYLOAD
    }
}

/// Writes a frame. A payload over the limit of its kind is an error.
pub fn write_frame(out: &mut dyn Write, f: &Frame) -> io::Result<()> {
    if f.payload.len() > limit(f.kind) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the frame is too long"));
    }
    let mut head = Vec::with_capacity(4 + HEADER + f.payload.len());
    head.extend_from_slice(&((HEADER + f.payload.len()) as u32).to_le_bytes());
    head.push(f.version);
    head.extend_from_slice(&f.kind.to_le_bytes());
    head.extend_from_slice(&f.id.to_le_bytes());
    head.extend_from_slice(&f.payload);
    out.write_all(&head)?;
    out.flush()
}

/// Reads a frame. A length beyond the limits, a version that is not spoken
/// and a frame cut short are errors; nothing is reserved before the length
/// has been checked.
pub fn read_frame(input: &mut dyn Read) -> io::Result<Frame> {
    let bad = |m: &'static str| io::Error::new(io::ErrorKind::InvalidData, m);
    let mut len = [0u8; 4];
    input.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if !(HEADER..=HEADER + MAX_BULK).contains(&len) {
        return Err(bad("the frame has an impossible length"));
    }
    let mut head = [0u8; HEADER];
    input.read_exact(&mut head)?;
    let version = head[0];
    let kind = u16::from_le_bytes([head[1], head[2]]);
    let id = u32::from_le_bytes([head[3], head[4], head[5], head[6]]);
    if !(PROTOCOL_MIN..=PROTOCOL).contains(&version) {
        return Err(bad("the protocol version is not spoken here"));
    }
    let n = len - HEADER;
    if n > limit(kind) {
        return Err(bad("the frame is too long for its kind"));
    }
    let mut payload = vec![0u8; n];
    input.read_exact(&mut payload)?;
    Ok(Frame { version, kind, id, payload })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_round_trips() {
        let f = Frame::new(kind::PING, 9, vec![1, 2, 3]);
        let mut wire = Vec::new();
        write_frame(&mut wire, &f).unwrap();
        assert_eq!(read_frame(&mut wire.as_slice()).unwrap(), f);
        let empty = Frame::new(kind::STATUS, 0, Vec::new());
        let mut wire = Vec::new();
        write_frame(&mut wire, &empty).unwrap();
        assert_eq!(read_frame(&mut wire.as_slice()).unwrap(), empty);
    }

    #[test]
    fn bad_frames_are_errors() {
        let read = |bytes: &[u8]| read_frame(&mut &bytes[..]);
        // Length below the header, and far above any limit.
        assert!(read(&[3, 0, 0, 0, 1, 2, 3]).is_err());
        assert!(read(&[0xff, 0xff, 0xff, 0xff]).is_err());
        // A version that is not spoken.
        assert!(read(&[7, 0, 0, 0, 9, 2, 0, 0, 0, 0, 0]).is_err());
        // Cut short, at every point of a good frame.
        let mut wire = Vec::new();
        write_frame(&mut wire, &Frame::new(kind::PING, 1, vec![5; 10])).unwrap();
        for cut in 0..wire.len() {
            assert!(read(&wire[..cut]).is_err(), "cut at {cut}");
        }
    }

    #[test]
    fn only_bulk_kinds_may_be_long() {
        let long = vec![0u8; MAX_PAYLOAD + 1];
        assert!(write_frame(&mut Vec::new(), &Frame::new(kind::PING, 0, long.clone())).is_err());
        assert!(write_frame(&mut Vec::new(), &Frame::new(kind::SCAN_BATCH, 0, long)).is_ok());
        // The same on reading: a long frame of a kind that is not bulk.
        let mut wire = Vec::new();
        wire.extend_from_slice(&((HEADER + MAX_PAYLOAD + 1) as u32).to_le_bytes());
        wire.extend_from_slice(&[PROTOCOL, kind::PING as u8, 0, 0, 0, 0, 0]);
        wire.extend(std::iter::repeat_n(0u8, MAX_PAYLOAD + 1));
        assert!(read_frame(&mut wire.as_slice()).is_err());
    }
}
