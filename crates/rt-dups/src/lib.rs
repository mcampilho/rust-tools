//! Finding duplicate files and folders: what rdup and rdup-cli share.
//!
//! - [`search`]: reads the drives and folders (with `rt-scan`) and compares the
//!   files by size, then by their first and last bytes, then by their whole
//!   contents (BLAKE3).
//! - [`cache`]: the hashes of files already read, so that the next search
//!   only reads what changed.
//! - [`folders`]: folders that are copies of one another, found from the
//!   copies of files.
//! - [`filter`]: which files are compared (sizes, dates, extensions, names)
//!   and how the text of the boxes that set them is read.
//! - [`testing`]: folders with copies of every kind, for tests.

pub mod cache;
pub mod filter;
pub mod folders;
pub mod search;
pub mod testing;
