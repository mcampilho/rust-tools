//! The messages of the protocol, and how a client's rights are decided.

use crate::codec::{DecodeError, Reader, Writer};

/// What a client may do, decided by the service from the client's token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Anonymous, guest, or a token that could not be read.
    Denied = 0,
    /// Any authenticated user: may read what is system-wide.
    User = 1,
    /// A member of Administrators with an elevated token: may also change
    /// things and see the data of other users.
    Elevated = 2,
}

impl Level {
    pub fn from_u8(v: u8) -> Option<Level> {
        match v {
            0 => Some(Level::Denied),
            1 => Some(Level::User),
            2 => Some(Level::Elevated),
            _ => None,
        }
    }
}

/// The facts about a client's token that decide its [`Level`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenFacts {
    pub anonymous: bool,
    pub guest: bool,
    /// `TokenElevation`: the token is elevated (or UAC is off for an admin).
    pub elevated: bool,
    /// Administrators is in the token's groups and enabled (not deny-only).
    pub admin: bool,
}

/// The level of a client, from its token and nothing else.
pub fn decide(t: TokenFacts) -> Level {
    if t.anonymous || t.guest {
        Level::Denied
    } else if t.elevated && t.admin {
        Level::Elevated
    } else {
        Level::User
    }
}

/// What the service can do, as bits.
pub mod caps {
    /// `ScanVolume`.
    pub const SCAN: u32 = 1;
}

/// Who may see the NTFS table: set when the service is installed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// Every authenticated user.
    User = 0,
    /// Only elevated clients.
    Elevated = 1,
}

impl Visibility {
    pub fn from_u8(v: u8) -> Option<Visibility> {
        match v {
            0 => Some(Visibility::User),
            1 => Some(Visibility::Elevated),
            _ => None,
        }
    }

    /// May a client of `level` read the NTFS table?
    pub fn allows(self, level: Level) -> bool {
        match self {
            Visibility::User => level >= Level::User,
            Visibility::Elevated => level >= Level::Elevated,
        }
    }
}

/// The first message of a client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hello {
    /// The newest protocol the client speaks.
    pub protocol: u8,
    pub app: String,
    pub app_version: String,
}

impl Hello {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u8(self.protocol).str(&self.app).str(&self.app_version);
        w.finish()
    }

    pub fn decode(b: &[u8]) -> Result<Hello, DecodeError> {
        let mut r = Reader::new(b);
        let h = Hello { protocol: r.u8()?, app: r.str()?.to_string(), app_version: r.str()?.to_string() };
        r.end()?;
        Ok(h)
    }
}

/// The service's answer to [`Hello`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelloOk {
    /// The protocol both will speak.
    pub protocol: u8,
    pub service_version: String,
    /// What the service made of the client's token.
    pub level: Level,
    pub caps: u32,
}

impl HelloOk {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u8(self.protocol).str(&self.service_version).u8(self.level as u8).u32(self.caps);
        w.finish()
    }

    pub fn decode(b: &[u8]) -> Result<HelloOk, DecodeError> {
        let mut r = Reader::new(b);
        let protocol = r.u8()?;
        let service_version = r.str()?.to_string();
        let level = Level::from_u8(r.u8()?).ok_or(DecodeError::Invalid)?;
        let caps = r.u32()?;
        r.end()?;
        Ok(HelloOk { protocol, service_version, level, caps })
    }
}

/// What `Status` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub uptime_secs: u64,
    pub clients: u32,
    /// Scans of volumes served since the start.
    pub scans: u64,
    /// The snapshots kept: a name and its age in seconds.
    pub snapshots: Vec<(String, u64)>,
}

impl Status {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u64(self.uptime_secs).u32(self.clients).u64(self.scans).u32(self.snapshots.len() as u32);
        for (name, age) in &self.snapshots {
            w.str(name).u64(*age);
        }
        w.finish()
    }

    pub fn decode(b: &[u8]) -> Result<Status, DecodeError> {
        let mut r = Reader::new(b);
        let (uptime_secs, clients, scans) = (r.u64()?, r.u32()?, r.u64()?);
        let n = r.count(12)?;
        let mut snapshots = Vec::with_capacity(n);
        for _ in 0..n {
            snapshots.push((r.str()?.to_string(), r.u64()?));
        }
        r.end()?;
        Ok(Status { uptime_secs, clients, scans, snapshots })
    }
}

/// What a scan of a volume asks: the folder to read, as a full path on a local drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanRequest {
    pub root: String,
}

impl ScanRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.str(&self.root);
        w.finish()
    }

    pub fn decode(b: &[u8]) -> Result<ScanRequest, DecodeError> {
        let mut r = Reader::new(b);
        let q = ScanRequest { root: r.str()?.to_string() };
        r.end()?;
        Ok(q)
    }
}

/// The first answer of a scan: the root as the service read it, and its time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanStart {
    pub root: String,
    pub mtime: i64,
}

impl ScanStart {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.str(&self.root).i64(self.mtime);
        w.finish()
    }

    pub fn decode(b: &[u8]) -> Result<ScanStart, DecodeError> {
        let mut r = Reader::new(b);
        let s = ScanStart { root: r.str()?.to_string(), mtime: r.i64()? };
        r.end()?;
        Ok(s)
    }
}

/// How far a scan has got.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanProgress {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub errors: u64,
    /// Of the table, 0 to 100; 255 when it is not known.
    pub percent: u8,
}

impl ScanProgress {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u64(self.files).u64(self.dirs).u64(self.bytes).u64(self.errors).u8(self.percent);
        w.finish()
    }

    pub fn decode(b: &[u8]) -> Result<ScanProgress, DecodeError> {
        let mut r = Reader::new(b);
        let p = ScanProgress { files: r.u64()?, dirs: r.u64()?, bytes: r.u64()?, errors: r.u64()?, percent: r.u8()? };
        r.end()?;
        Ok(p)
    }
}

/// An entry of a folder, as it goes on the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireEntry {
    pub name: String,
    /// 0 a file, 1 a folder, 2 a link.
    pub kind: u8,
    pub flags: u8,
    pub size: u64,
    pub alloc: u64,
    pub mtime: i64,
}

/// A run of the entries of one folder (`dir` is its number in the order the entries were
/// sent in); `last` closes the folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub dir: u32,
    pub last: bool,
    pub entries: Vec<WireEntry>,
}

/// The bytes one entry takes at least on the wire: name length, kind, flags, three numbers.
const ENTRY_MIN: usize = 4 + 1 + 1 + 8 + 8 + 8;

/// The payload of one `SCAN_BATCH`: several chunks.
pub fn encode_batch(chunks: &[Chunk]) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(chunks.len() as u32);
    for c in chunks {
        w.u32(c.dir).bool(c.last).u32(c.entries.len() as u32);
        for e in &c.entries {
            w.str(&e.name).u8(e.kind).u8(e.flags).u64(e.size).u64(e.alloc).i64(e.mtime);
        }
    }
    w.finish()
}

pub fn decode_batch(b: &[u8]) -> Result<Vec<Chunk>, DecodeError> {
    let mut r = Reader::new(b);
    let n = r.count(4 + 1 + 4)?;
    let mut chunks = Vec::with_capacity(n);
    for _ in 0..n {
        let (dir, last) = (r.u32()?, r.bool()?);
        let m = r.count(ENTRY_MIN)?;
        let mut entries = Vec::with_capacity(m);
        for _ in 0..m {
            let name = r.str()?.to_string();
            let (kind, flags) = (r.u8()?, r.u8()?);
            if kind > 2 {
                return Err(DecodeError::Invalid);
            }
            entries.push(WireEntry { name, kind, flags, size: r.u64()?, alloc: r.u64()?, mtime: r.i64()? });
        }
        chunks.push(Chunk { dir, last, entries });
    }
    r.end()?;
    Ok(chunks)
}

/// The last answer of a scan: how many nodes the tree has, to check nothing was lost.
pub fn encode_done(nodes: u32) -> Vec<u8> {
    nodes.to_le_bytes().to_vec()
}

pub fn decode_done(b: &[u8]) -> Result<u32, DecodeError> {
    let mut r = Reader::new(b);
    let n = r.u32()?;
    r.end()?;
    Ok(n)
}

/// The names of the snapshots the service keeps.
pub mod snapshots {
    /// The programs installed (what rinst reads at start).
    pub const PROGRAMS: &str = "programs";
    /// What the cleaner finds that needs administrator rights to clean.
    pub const CLEAN: &str = "clean";
}

/// A request for a snapshot: its name, and the oldest it may be (seconds).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRequest {
    pub name: String,
    pub max_age_secs: u64,
}

impl SnapshotRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.str(&self.name).u64(self.max_age_secs);
        w.finish()
    }

    pub fn decode(b: &[u8]) -> Result<SnapshotRequest, DecodeError> {
        let mut r = Reader::new(b);
        let q = SnapshotRequest { name: r.str()?.to_string(), max_age_secs: r.u64()? };
        r.end()?;
        Ok(q)
    }
}

/// A snapshot, as the client receives it: when it was made, how old it is,
/// and its contents (what only the snapshot's own reader understands).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotData {
    pub made_at: u64,
    pub age_secs: u64,
    pub data: Vec<u8>,
}

impl SnapshotData {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u64(self.made_at).u64(self.age_secs).u32(self.data.len() as u32);
        let mut b = w.finish();
        b.extend_from_slice(&self.data);
        b
    }

    pub fn decode(b: &[u8]) -> Result<SnapshotData, DecodeError> {
        let mut r = Reader::new(b);
        let (made_at, age_secs, n) = (r.u64()?, r.u64()?, r.u32()? as usize);
        let head = 8 + 8 + 4;
        if b.len() != head + n {
            return Err(DecodeError::Invalid);
        }
        Ok(SnapshotData { made_at, age_secs, data: b[head..].to_vec() })
    }
}

/// What a job does. The items name things the service knows (`docs/service.md`, §8): never a path or a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    /// Cleans targets of the cleaner's catalogue that need administrator rights (items: their ids).
    Clean = 0,
    /// Uninstalls programs of the machine, by Windows Installer product code (items: the codes).
    UninstallMsi = 1,
}

impl JobKind {
    pub fn from_u8(v: u8) -> Option<JobKind> {
        match v {
            0 => Some(JobKind::Clean),
            1 => Some(JobKind::UninstallMsi),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobRequest {
    pub kind: JobKind,
    pub items: Vec<String>,
}

/// The most items of a job.
pub const MAX_JOB_ITEMS: usize = 200;

impl JobRequest {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u8(self.kind as u8).u32(self.items.len() as u32);
        for i in &self.items {
            w.str(i);
        }
        w.finish()
    }

    pub fn decode(b: &[u8]) -> Result<JobRequest, DecodeError> {
        let mut r = Reader::new(b);
        let kind = JobKind::from_u8(r.u8()?).ok_or(DecodeError::Invalid)?;
        let n = r.count(4)?;
        if n > MAX_JOB_ITEMS {
            return Err(DecodeError::Invalid);
        }
        let mut items = Vec::with_capacity(n);
        for _ in 0..n {
            items.push(r.str()?.to_string());
        }
        r.end()?;
        Ok(JobRequest { kind, items })
    }
}

/// Where a job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued = 0,
    Running = 1,
    Done = 2,
    Failed = 3,
    Cancelled = 4,
}

impl JobState {
    pub fn from_u8(v: u8) -> Option<JobState> {
        match v {
            0 => Some(JobState::Queued),
            1 => Some(JobState::Running),
            2 => Some(JobState::Done),
            3 => Some(JobState::Failed),
            4 => Some(JobState::Cancelled),
            _ => None,
        }
    }

    /// Nothing more will happen to it.
    pub fn finished(self) -> bool {
        matches!(self, JobState::Done | JobState::Failed | JobState::Cancelled)
    }
}

/// What the service tells of a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobInfo {
    pub id: u64,
    pub kind: JobKind,
    pub state: JobState,
    pub done: u32,
    pub total: u32,
    /// What was done, item by item (the last lines).
    pub log: Vec<String>,
}

impl JobInfo {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u64(self.id).u8(self.kind as u8).u8(self.state as u8).u32(self.done).u32(self.total).u32(self.log.len() as u32);
        for l in &self.log {
            w.str(l);
        }
        w.finish()
    }

    pub fn decode(b: &[u8]) -> Result<JobInfo, DecodeError> {
        let mut r = Reader::new(b);
        let id = r.u64()?;
        let kind = JobKind::from_u8(r.u8()?).ok_or(DecodeError::Invalid)?;
        let state = JobState::from_u8(r.u8()?).ok_or(DecodeError::Invalid)?;
        let (done, total) = (r.u32()?, r.u32()?);
        let mut log = Vec::new();
        for _ in 0..r.count(4)? {
            log.push(r.str()?.to_string());
        }
        r.end()?;
        Ok(JobInfo { id, kind, state, done, total, log })
    }
}

/// The number of a job, in a request to look at it or stop it.
pub fn encode_job_id(id: u64) -> Vec<u8> {
    id.to_le_bytes().to_vec()
}

pub fn decode_job_id(b: &[u8]) -> Result<u64, DecodeError> {
    let mut r = Reader::new(b);
    let id = r.u64()?;
    r.end()?;
    Ok(id)
}

/// Why a request was not done.
pub mod code {
    /// The request is not known to this service.
    pub const UNKNOWN: u16 = 1;
    /// The client's level does not allow it.
    pub const DENIED: u16 = 2;
    /// The payload could not be decoded.
    pub const MALFORMED: u16 = 3;
    /// The request was understood and failed.
    pub const FAILED: u16 = 4;
    /// The first message was not `Hello`.
    pub const NO_HELLO: u16 = 5;
    /// There is no such job.
    pub const NO_JOB: u16 = 10;
    /// There is no such snapshot, or it is not made yet.
    pub const NO_SNAPSHOT: u16 = 9;
    /// The service is busy: try later.
    pub const BUSY: u16 = 6;
    /// The client asked to stop it.
    pub const CANCELLED: u16 = 7;
    /// The volume is not NTFS, or cannot be read that way.
    pub const NOT_NTFS: u16 = 8;
}

pub fn encode_error(code: u16, message: &str) -> Vec<u8> {
    let mut w = Writer::new();
    w.u16(code).str(message);
    w.finish()
}

pub fn decode_error(b: &[u8]) -> Result<(u16, String), DecodeError> {
    let mut r = Reader::new(b);
    let e = (r.u16()?, r.str()?.to_string());
    r.end()?;
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rights_come_from_the_token() {
        let t = TokenFacts::default();
        assert_eq!(decide(t), Level::User);
        assert_eq!(decide(TokenFacts { elevated: true, ..t }), Level::User, "elevated but not an administrator");
        assert_eq!(decide(TokenFacts { admin: true, ..t }), Level::User, "an administrator not elevated (UAC-filtered)");
        assert_eq!(decide(TokenFacts { elevated: true, admin: true, ..t }), Level::Elevated);
        assert_eq!(decide(TokenFacts { anonymous: true, elevated: true, admin: true, ..t }), Level::Denied);
        assert_eq!(decide(TokenFacts { guest: true, ..t }), Level::Denied);
    }

    #[test]
    fn visibility_limits_who_reads() {
        assert!(Visibility::User.allows(Level::User) && Visibility::User.allows(Level::Elevated));
        assert!(!Visibility::User.allows(Level::Denied));
        assert!(!Visibility::Elevated.allows(Level::User) && Visibility::Elevated.allows(Level::Elevated));
    }

    #[test]
    fn messages_round_trip() {
        let h = Hello { protocol: 1, app: "rsize".into(), app_version: "0.2.0".into() };
        assert_eq!(Hello::decode(&h.encode()), Ok(h));
        let ok = HelloOk { protocol: 1, service_version: "0.1.0".into(), level: Level::Elevated, caps: caps::SCAN };
        assert_eq!(HelloOk::decode(&ok.encode()), Ok(ok));
        let s = Status { uptime_secs: 5, clients: 2, scans: 7, snapshots: vec![("programs".into(), 30), ("clean".into(), 9)] };
        assert_eq!(Status::decode(&s.encode()), Ok(s));
        assert_eq!(decode_error(&encode_error(code::DENIED, "no")), Ok((code::DENIED, "no".to_string())));
    }

    fn entry(name: &str, kind: u8) -> WireEntry {
        WireEntry { name: name.into(), kind, flags: 2, size: 10, alloc: 4096, mtime: -3 }
    }

    #[test]
    fn scan_messages_round_trip() {
        let q = ScanRequest { root: r"C:\Users".into() };
        assert_eq!(ScanRequest::decode(&q.encode()), Ok(q));
        let s = ScanStart { root: r"C:\Users".into(), mtime: 99 };
        assert_eq!(ScanStart::decode(&s.encode()), Ok(s));
        let p = ScanProgress { files: 1, dirs: 2, bytes: 3, errors: 4, percent: 255 };
        assert_eq!(ScanProgress::decode(&p.encode()), Ok(p));
        let chunks = vec![
            Chunk { dir: 0, last: false, entries: vec![entry("a", 0), entry("sub", 1)] },
            Chunk { dir: 0, last: true, entries: vec![entry("ólá.txt", 2)] },
            Chunk { dir: 3, last: true, entries: vec![] },
        ];
        assert_eq!(decode_batch(&encode_batch(&chunks)), Ok(chunks));
        assert_eq!(decode_done(&encode_done(77)), Ok(77));
    }

    #[test]
    fn hostile_batches_are_errors() {
        let chunks = vec![Chunk { dir: 1, last: true, entries: vec![entry("a", 0), entry("b", 1)] }];
        let good = encode_batch(&chunks);
        for cut in 0..good.len() {
            assert!(decode_batch(&good[..cut]).is_err(), "cut at {cut}");
        }
        // A kind that does not exist.
        let mut bad = good.clone();
        bad[4 + 4 + 1 + 4 + 4 + 1] = 9;
        assert!(decode_batch(&bad).is_err());
        // A chunk count the payload cannot hold.
        assert!(decode_batch(&[0xff, 0xff, 0xff, 0x0f]).is_err());
        let mut long = good;
        long.push(0);
        assert!(decode_batch(&long).is_err());
    }

    #[test]
    fn job_messages_round_trip_and_hostile_ones_fail() {
        let q = JobRequest { kind: JobKind::UninstallMsi, items: vec!["{AAAA}".into(), "{BBBB}".into()] };
        assert_eq!(JobRequest::decode(&q.encode()), Ok(q));
        let i = JobInfo { id: 7, kind: JobKind::Clean, state: JobState::Running, done: 1, total: 3, log: vec!["a".into(), "b".into()] };
        assert_eq!(JobInfo::decode(&i.encode()), Ok(i.clone()));
        assert_eq!(decode_job_id(&encode_job_id(99)), Ok(99));
        // A kind or a state that does not exist, too many items, cut short.
        assert!(JobRequest::decode(&[9, 0, 0, 0, 0]).is_err());
        let many = JobRequest { kind: JobKind::Clean, items: (0..=MAX_JOB_ITEMS).map(|n| n.to_string()).collect() };
        assert!(JobRequest::decode(&many.encode()).is_err());
        let b = i.encode();
        for cut in 0..b.len() {
            assert!(JobInfo::decode(&b[..cut]).is_err());
        }
        assert!(JobState::Done.finished() && !JobState::Running.finished());
    }

    #[test]
    fn hostile_messages_are_errors() {
        // A level that does not exist, bytes left over, a snapshot count the payload cannot hold.
        assert!(HelloOk::decode(&[1, 0, 0, 0, 0, 9, 0, 0, 0, 0]).is_err());
        let mut h = Hello { protocol: 1, app: "a".into(), app_version: "b".into() }.encode();
        h.push(0);
        assert!(Hello::decode(&h).is_err());
        let mut s = Status::default().encode();
        let n = s.len();
        s[n - 4..].copy_from_slice(&0x0fff_ffffu32.to_le_bytes());
        assert!(Status::decode(&s).is_err());
        let full = Status { snapshots: vec![("x".into(), 1)], ..Status::default() }.encode();
        for cut in 0..full.len() {
            assert!(Status::decode(&full[..cut]).is_err());
        }
    }
}
