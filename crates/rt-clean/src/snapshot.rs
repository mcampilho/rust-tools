//! What the cleaner finds that needs administrator rights to clean, as a
//! snapshot of the privileged service: only sizes and counts, which anyone
//! may see; cleaning them still needs the tool to be started as administrator.

use crate::clean::{analyse, Found};
use crate::targets::{all, Env, Target};
use rt_svc::codec::{DecodeError, Reader, Writer};
use std::sync::atomic::AtomicBool;

const VERSION: u8 = 1;

/// What one target held when it was measured.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub id: String,
    pub bytes: u64,
    /// Files and items that would go.
    pub count: u64,
    pub unreadable: u64,
    /// Its size cannot be known before cleaning.
    pub unknown: bool,
}

impl Summary {
    pub fn of(t: &Target, f: &Found) -> Summary {
        Summary { id: t.id.clone(), bytes: f.bytes, count: f.count(), unreadable: f.unreadable, unknown: f.unknown }
    }
}

/// Measures the targets that need administrator rights, until `stop` is set.
pub fn measure(targets: &[Target], stop: &AtomicBool) -> Vec<Summary> {
    let mut out = Vec::new();
    for t in targets.iter().filter(|t| t.admin) {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        out.push(Summary::of(t, &analyse(t, stop)));
    }
    out
}

/// The targets of this computer, measured.
pub fn produce(stop: &AtomicBool) -> Vec<u8> {
    encode(&measure(&all(&Env::here()), stop))
}

pub fn encode(list: &[Summary]) -> Vec<u8> {
    let mut w = Writer::new();
    w.u8(VERSION).u32(list.len() as u32);
    for s in list {
        w.str(&s.id).u64(s.bytes).u64(s.count).u64(s.unreadable).bool(s.unknown);
    }
    w.finish()
}

pub fn decode(b: &[u8]) -> Result<Vec<Summary>, DecodeError> {
    let mut r = Reader::new(b);
    if r.u8()? != VERSION {
        return Err(DecodeError::Invalid);
    }
    let mut out = Vec::new();
    for _ in 0..r.count(29)? {
        out.push(Summary { id: r.str()?.to_string(), bytes: r.u64()?, count: r.u64()?, unreadable: r.u64()?, unknown: r.bool()? });
    }
    r.end()?;
    Ok(out)
}

/// What the service measured, if at most `max_age_secs` ago, and how long ago.
pub fn fetch(max_age_secs: u64) -> Option<(Vec<Summary>, u64)> {
    let mut c = rt_svc::client::connect("rt-clean", env!("CARGO_PKG_VERSION")).ok()?;
    let s = c.snapshot(rt_svc::proto::snapshots::CLEAN, max_age_secs).ok()?;
    Some((decode(&s.data).ok()?, s.age_secs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_round_trip_and_hostile_bytes_fail() {
        let list = vec![
            Summary { id: "windows.temp".into(), bytes: 1 << 33, count: 12_345, unreadable: 2, unknown: false },
            Summary { id: "flatpak.unused".into(), unknown: true, ..Summary::default() },
        ];
        let b = encode(&list);
        assert_eq!(decode(&b), Ok(list));
        for cut in 0..b.len() {
            assert!(decode(&b[..cut]).is_err(), "cut at {cut}");
        }
        assert!(decode(&[7]).is_err());
        assert_eq!(decode(&encode(&[])), Ok(vec![]));
    }

    #[test]
    fn only_the_targets_that_need_administrator_rights_are_measured() {
        let targets = all(&Env::here());
        assert!(targets.iter().any(|t| t.admin) && targets.iter().any(|t| !t.admin));
        // A stop set from the start measures nothing.
        let stop = AtomicBool::new(true);
        assert!(measure(&targets, &stop).is_empty());
    }
}
