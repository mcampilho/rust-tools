//! Text buffer and editing operations.

pub mod buffer;
pub mod ops;

pub use buffer::{After, Buffer, Edit, Kind, Sel};
pub use ops::IndentCfg;
