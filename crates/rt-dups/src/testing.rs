//! Helpers for the tests of what searches with this crate: folders with
//! copies of every kind, searched folder by folder.

use crate::filter::Filter;
use crate::search::{find_with, Failure, Found, Options};
use rt_scan::testing::TempTree;
use rt_scan::Ways;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

/// Searches folder by folder: what the tests check, whatever else this PC has.
pub fn sys(roots: &[PathBuf]) -> Result<Found, Failure> {
    find_with(roots, Ways::SYSTEM, Options::default(), &AtomicBool::new(false), &|_| {})
}

/// `len` bytes that differ from file to file by `seed`.
pub fn data(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| (i as u32 * 31 + seed as u32 * 7) as u8).collect()
}

/// A folder with copies of every kind: small and large, in subfolders,
/// alike only in size or only at the ends.
pub fn sample(tag: &str) -> TempTree {
    let tt = TempTree::new(tag);
    let big = data(300_000, 1);
    tt.write("photos/big.bin", &big);
    tt.write("backup/big copy.bin", &big);
    tt.write("backup/old/big.bin", &big);
    // Same size, same first and last bytes, different middle.
    let mut other = big.clone();
    other[150_000] ^= 0xFF;
    tt.write("other/big.bin", &other);
    let small = data(1000, 2);
    tt.write("notes.txt", &small);
    tt.write("docs/notes.txt", &small);
    // Same size, other contents.
    tt.write("docs/plan.txt", &data(1000, 3));
    tt.write("unique.dat", &data(777, 4));
    tt.write("empty1", b"");
    tt.write("docs/empty2", b"");
    tt
}

/// Photos, a backup of them, and an older copy with one file more.
pub fn folder_sample(tag: &str) -> TempTree {
    let tt = TempTree::new(tag);
    for dir in ["photos/2024", "backup/photos/2024", "old/2024"] {
        tt.write(&format!("{dir}/a.jpg"), &data(3000, 1));
        tt.write(&format!("{dir}/b.jpg"), &data(4000, 2));
    }
    tt.write("photos/2024/note.txt", &data(100, 3));
    tt.write("backup/photos/2024/renamed.txt", &data(100, 3));
    tt.write("old/2024/extra.bin", &data(500, 4));
    tt.write("unique.bin", &data(9000, 5));
    tt
}

/// Searches `tt` folder by folder for files of at least `min` bytes.
pub fn search_in(tt: &TempTree, min: u64) -> Found {
    find_with(std::slice::from_ref(&tt.0), Ways::SYSTEM, Options::with(Filter::min(min)), &AtomicBool::new(false), &|_| {}).unwrap()
}
