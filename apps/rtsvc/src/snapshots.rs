//! The snapshots the service keeps (programs, what can be cleaned): made in
//! the background when the computer is idle, when something they depend on
//! changes, or when a client asks; kept on disk; and shown to each client
//! as much as its level allows.

use rt_svc::proto::{Level, SnapshotData};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Who is asking, for the part of a snapshot a client may see.
pub struct Viewer {
    pub level: Level,
    /// The name of the account (without the domain).
    pub account: String,
}

pub type Produce = Box<dyn Fn(&AtomicBool) -> Vec<u8> + Send + Sync>;
pub type View = Box<dyn Fn(&Viewer, &[u8]) -> Option<Vec<u8>> + Send + Sync>;

/// One kind of snapshot.
pub struct Kind {
    pub name: &'static str,
    /// It is made again when it is older than this (seconds), if the computer is idle.
    pub max_age: u64,
    /// At most one try in this time (seconds), whatever asks.
    pub min_gap: u64,
    pub produce: Produce,
    pub view: View,
}

/// Why a snapshot was not given.
#[derive(Debug, PartialEq, Eq)]
pub enum Miss {
    /// There is no such kind.
    Unknown,
    /// It is not made yet.
    NotMade,
    /// It is older than the client wants (its age).
    TooOld(u64),
    /// The client may not see it.
    Hidden,
}

#[derive(Default)]
struct State {
    data: Option<(u64, Arc<Vec<u8>>)>,
    dirty: bool,
    requested: bool,
    last_try: Option<u64>,
}

pub struct Store {
    kinds: Vec<Kind>,
    states: Mutex<Vec<State>>,
    dir: Option<PathBuf>,
    wake: (Mutex<bool>, Condvar),
}

const MAGIC: &[u8; 4] = b"RTSN";

fn checksum(data: &[u8]) -> u64 {
    // FNV-1a: to notice a file cut short or damaged, not to resist anyone.
    data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3))
}

pub fn encode_file(made_at: u64, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 25);
    out.extend_from_slice(MAGIC);
    out.push(1);
    out.extend_from_slice(&made_at.to_le_bytes());
    out.extend_from_slice(&checksum(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

pub fn decode_file(b: &[u8]) -> Option<(u64, Vec<u8>)> {
    if b.len() < 25 || &b[..4] != MAGIC || b[4] != 1 {
        return None;
    }
    let made_at = u64::from_le_bytes(b[5..13].try_into().ok()?);
    let sum = u64::from_le_bytes(b[13..21].try_into().ok()?);
    let n = u32::from_le_bytes(b[21..25].try_into().ok()?) as usize;
    let data = b.get(25..)?;
    (data.len() == n && checksum(data) == sum).then(|| (made_at, data.to_vec()))
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Store {
    /// A store of `kinds`, with what was kept in `dir` (if any) from before.
    pub fn new(kinds: Vec<Kind>, dir: Option<PathBuf>) -> Store {
        let mut states: Vec<State> = kinds.iter().map(|_| State::default()).collect();
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d);
            for (i, k) in kinds.iter().enumerate() {
                if let Some((at, data)) = std::fs::read(d.join(format!("{}.snap", k.name))).ok().and_then(|b| decode_file(&b)) {
                    states[i].data = Some((at, Arc::new(data)));
                }
            }
        }
        Store { kinds, states: Mutex::new(states), dir, wake: (Mutex::new(false), Condvar::new()) }
    }

    fn index(&self, name: &str) -> Option<usize> {
        self.kinds.iter().position(|k| k.name == name)
    }

    /// Wakes the scheduler.
    pub fn poke(&self) {
        if let Ok(mut g) = self.wake.0.lock() {
            *g = true;
            self.wake.1.notify_all();
        }
    }

    /// Sleeps until something wants the scheduler, or `d` has passed.
    pub fn wait(&self, d: Duration) {
        if let Ok(g) = self.wake.0.lock() {
            if let Ok((mut g, _)) = self.wake.1.wait_timeout_while(g, d, |poked| !*poked) {
                *g = false;
            }
        }
    }

    /// The snapshot `name` as `viewer` may see it, if it is at most `max_age` seconds old.
    pub fn get(&self, name: &str, max_age: u64, viewer: &Viewer, now: u64) -> Result<SnapshotData, Miss> {
        let i = self.index(name).ok_or(Miss::Unknown)?;
        let (at, data) = {
            let mut st = self.states.lock().map_err(|_| Miss::NotMade)?;
            match &st[i].data {
                None => {
                    st[i].requested = true;
                    drop(st);
                    self.poke();
                    return Err(Miss::NotMade);
                }
                Some((at, d)) => {
                    let age = now.saturating_sub(*at);
                    if age > max_age {
                        st[i].requested = true;
                        drop(st);
                        self.poke();
                        return Err(Miss::TooOld(age));
                    }
                    (*at, d.clone())
                }
            }
        };
        let shown = (self.kinds[i].view)(viewer, &data).ok_or(Miss::Hidden)?;
        Ok(SnapshotData { made_at: at, age_secs: now.saturating_sub(at), data: shown })
    }

    /// A client asks for the snapshot to be made again.
    pub fn refresh(&self, name: &str) -> bool {
        let Some(i) = self.index(name) else { return false };
        if let Ok(mut st) = self.states.lock() {
            st[i].requested = true;
        }
        self.poke();
        true
    }

    /// Something the snapshot depends on changed.
    pub fn mark_dirty(&self, name: &str) {
        if let Some(i) = self.index(name) {
            if let Ok(mut st) = self.states.lock() {
                st[i].dirty = true;
            }
            self.poke();
        }
    }

    /// The names of the snapshots made and their age in seconds.
    pub fn status(&self, now: u64) -> Vec<(String, u64)> {
        let st = self.states.lock().map(|s| s.iter().map(|x| x.data.as_ref().map(|(at, _)| *at)).collect::<Vec<_>>()).unwrap_or_default();
        self.kinds.iter().zip(st).filter_map(|(k, at)| at.map(|a| (k.name.to_string(), now.saturating_sub(a)))).collect()
    }

    /// One pass of the scheduler: makes the snapshots that are due. A snapshot is made when a client
    /// asked for it, or when it is dirty, old or missing and the computer is `idle`; never twice
    /// within its `min_gap`. Returns how many were made.
    pub fn tick(&self, now: u64, idle: bool, stop: &AtomicBool) -> usize {
        let mut made = 0;
        for (i, k) in self.kinds.iter().enumerate() {
            let go = {
                let Ok(st) = self.states.lock() else { return made };
                let s = &st[i];
                let gap_ok = s.last_try.is_none_or(|t| now.saturating_sub(t) >= k.min_gap);
                let due = s.dirty || s.data.as_ref().is_none_or(|(at, _)| now.saturating_sub(*at) > k.max_age);
                gap_ok && (s.requested || (due && idle))
            };
            if !go {
                continue;
            }
            if let Ok(mut st) = self.states.lock() {
                st[i].last_try = Some(now);
                st[i].requested = false;
                st[i].dirty = false;
            }
            let data = (k.produce)(stop);
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            if let Some(d) = &self.dir {
                let tmp = d.join(format!("{}.snap.tmp", k.name));
                if std::fs::write(&tmp, encode_file(now, &data)).is_ok() {
                    let _ = std::fs::rename(tmp, d.join(format!("{}.snap", k.name)));
                }
            }
            if let Ok(mut st) = self.states.lock() {
                st[i].data = Some((now, Arc::new(data)));
            }
            made += 1;
        }
        made
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn kind(counter: Arc<AtomicU32>) -> Kind {
        Kind {
            name: "k",
            max_age: 100,
            min_gap: 10,
            produce: Box::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                vec![1, 2, 3]
            }),
            view: Box::new(|v, d| (v.level >= Level::User).then(|| d.to_vec())),
        }
    }

    fn user() -> Viewer {
        Viewer { level: Level::User, account: "ana".into() }
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("target")
            .join("rtsvc-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn nothing_is_made_until_the_computer_is_idle_or_a_client_asks() {
        let n = Arc::new(AtomicU32::new(0));
        let s = Store::new(vec![kind(n.clone())], None);
        let stop = AtomicBool::new(false);
        assert_eq!(s.tick(1000, false, &stop), 0);
        assert_eq!(s.get("k", 50, &user(), 1000), Err(Miss::NotMade));
        // The miss asked for it: made at the next pass even if busy.
        assert_eq!(s.tick(1001, false, &stop), 1);
        assert_eq!(s.get("k", 50, &user(), 1002).unwrap().data, vec![1, 2, 3]);
        assert_eq!(n.load(Ordering::SeqCst), 1);
        assert_eq!(s.status(1011), vec![("k".to_string(), 10)]);
    }

    #[test]
    fn an_old_snapshot_is_made_again_when_idle_but_not_too_often() {
        let n = Arc::new(AtomicU32::new(0));
        let s = Store::new(vec![kind(n.clone())], None);
        let stop = AtomicBool::new(false);
        assert_eq!(s.tick(1000, true, &stop), 1);
        assert_eq!(s.tick(1050, true, &stop), 0, "not old yet");
        assert_eq!(s.tick(1101, true, &stop), 1, "older than max_age");
        s.mark_dirty("k");
        assert_eq!(s.tick(1105, true, &stop), 0, "within min_gap");
        assert_eq!(s.tick(1112, false, &stop), 0, "dirty but busy");
        assert_eq!(s.tick(1113, true, &stop), 1);
        assert_eq!(n.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn a_client_may_want_it_younger_and_the_view_may_hide_it() {
        let n = Arc::new(AtomicU32::new(0));
        let s = Store::new(vec![kind(n)], None);
        let stop = AtomicBool::new(false);
        s.tick(1000, true, &stop);
        assert_eq!(s.get("k", 30, &user(), 1040), Err(Miss::TooOld(40)));
        assert!(s.refresh("k") && !s.refresh("nope"));
        assert_eq!(s.tick(1041, false, &stop), 1);
        assert_eq!(s.get("k", 30, &user(), 1050).unwrap().age_secs, 9);
        assert_eq!(s.get("x", 30, &user(), 1050), Err(Miss::Unknown));
        let nobody = Viewer { level: Level::Denied, account: String::new() };
        assert_eq!(s.get("k", 30, &nobody, 1050), Err(Miss::Hidden));
    }

    #[test]
    fn snapshots_survive_a_restart_and_damage_is_noticed() {
        let dir = scratch("keep");
        let n = Arc::new(AtomicU32::new(0));
        {
            let s = Store::new(vec![kind(n.clone())], Some(dir.clone()));
            s.tick(5000, true, &AtomicBool::new(false));
        }
        let again = Store::new(vec![kind(n.clone())], Some(dir.clone()));
        assert_eq!(again.status(5060), vec![("k".to_string(), 60)]);
        assert_eq!(n.load(Ordering::SeqCst), 1, "not made again");
        // A file cut short, or changed, is ignored.
        let file = dir.join("k.snap");
        let b = std::fs::read(&file).unwrap();
        std::fs::write(&file, &b[..b.len() - 1]).unwrap();
        assert!(Store::new(vec![kind(n.clone())], Some(dir.clone())).status(5060).is_empty());
        let mut bad = b.clone();
        *bad.last_mut().unwrap() ^= 1;
        std::fs::write(&file, bad).unwrap();
        assert!(Store::new(vec![kind(n)], Some(dir.clone())).status(5060).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_stop_during_the_work_keeps_the_old_snapshot() {
        let s = Store::new(
            vec![Kind {
                name: "k",
                max_age: 1,
                min_gap: 0,
                produce: Box::new(|stop| {
                    stop.store(true, Ordering::SeqCst);
                    vec![9]
                }),
                view: Box::new(|_, d| Some(d.to_vec())),
            }],
            None,
        );
        assert_eq!(s.tick(10, true, &AtomicBool::new(false)), 0);
        assert!(s.status(11).is_empty());
    }
}
