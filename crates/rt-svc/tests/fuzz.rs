//! Hostile bytes into every decoder of the protocol: random ones, and good
//! messages with bytes changed, cut and repeated. Nothing may panic or
//! reserve a lot of memory; the answer is an `Ok` or an `Err`. The sequence
//! is fixed, so a failure can be repeated.

use rt_svc::frame::read_frame;
use rt_svc::proto::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// Every decoder, on the same bytes.
fn all_decoders(b: &[u8]) {
    let _ = Hello::decode(b);
    let _ = HelloOk::decode(b);
    let _ = Status::decode(b);
    let _ = decode_error(b);
    let _ = ScanRequest::decode(b);
    let _ = ScanStart::decode(b);
    let _ = ScanProgress::decode(b);
    let _ = decode_batch(b);
    let _ = decode_done(b);
    let _ = SnapshotRequest::decode(b);
    let _ = SnapshotData::decode(b);
    let _ = JobRequest::decode(b);
    let _ = JobInfo::decode(b);
    let _ = decode_job_id(b);
    let mut r = b;
    let _ = read_frame(&mut r);
}

fn good_messages() -> Vec<Vec<u8>> {
    let entry = |n: &str, k: u8| WireEntry { name: n.into(), kind: k, flags: 1, size: 5, alloc: 4096, mtime: 9 };
    vec![
        Hello { protocol: 1, app: "rsize".into(), app_version: "0.2.0".into() }.encode(),
        HelloOk { protocol: 1, service_version: "0.1.0".into(), level: Level::User, caps: 1 }.encode(),
        Status { uptime_secs: 1, clients: 2, scans: 3, snapshots: vec![("programs".into(), 4)] }.encode(),
        encode_error(code::DENIED, "no"),
        ScanRequest { root: r"C:\data".into() }.encode(),
        ScanStart { root: r"C:\data".into(), mtime: 5 }.encode(),
        ScanProgress { files: 1, dirs: 2, bytes: 3, errors: 4, percent: 50 }.encode(),
        encode_batch(&[Chunk { dir: 0, last: true, entries: vec![entry("a", 0), entry("b", 1), entry("c", 2)] }]),
        SnapshotRequest { name: "programs".into(), max_age_secs: 60 }.encode(),
        SnapshotData { made_at: 1, age_secs: 2, data: vec![1, 2, 3, 4] }.encode(),
        JobRequest { kind: JobKind::Clean, items: vec!["a".into(), "b".into()] }.encode(),
        JobInfo { id: 1, kind: JobKind::Clean, state: JobState::Running, done: 1, total: 2, log: vec!["x".into()] }.encode(),
    ]
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..20_000 {
        let n = rng.below(200);
        let b: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        all_decoders(&b);
    }
}

#[test]
fn good_messages_with_changes_never_panic() {
    let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
    for good in good_messages() {
        all_decoders(&good);
        for _ in 0..4000 {
            let mut b = good.clone();
            for _ in 0..1 + rng.below(4) {
                match rng.below(5) {
                    0 if !b.is_empty() => {
                        let i = rng.below(b.len());
                        b[i] = rng.next() as u8;
                    }
                    1 if !b.is_empty() => {
                        let i = rng.below(b.len());
                        b[i] ^= 1 << rng.below(8);
                    }
                    2 => b.truncate(rng.below(b.len() + 1)),
                    3 => b.push(rng.next() as u8),
                    _ if !b.is_empty() => {
                        // A length field made huge.
                        let i = rng.below(b.len());
                        b[i] = 0xff;
                    }
                    _ => {}
                }
            }
            all_decoders(&b);
        }
    }
}

#[test]
fn frames_with_hostile_headers_are_errors_not_allocations() {
    // The longest frame the protocol allows is a few megabytes; these claim gigabytes.
    for len in [u32::MAX, u32::MAX / 2, 1 << 30, (8 << 20) + 8] {
        let mut b = len.to_le_bytes().to_vec();
        b.extend_from_slice(&[1, 2, 0, 0, 0, 0, 0]);
        assert!(read_frame(&mut b.as_slice()).is_err(), "{len}");
    }
}
