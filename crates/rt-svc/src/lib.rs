//! The protocol and the client of the rust-tools privileged service (see
//! `docs/service.md`).
//!
//! - [`codec`], [`frame`], [`proto`]: what is sent on the pipe, the same on
//!   every system and tested without one.
//! - [`access`], [`pipe`], [`server`], [`client`]: the Windows named pipe, how
//!   the service decides the level of a client, and how a client checks that
//!   the server is the service. On other systems [`client::connect`] simply
//!   finds no service.

pub mod codec;
pub mod frame;
pub mod proto;

#[cfg(windows)]
pub mod access;
pub mod client;
#[cfg(windows)]
pub mod pipe;
#[cfg(windows)]
pub mod secure;
#[cfg(windows)]
pub mod server;

pub use frame::{kind, Frame, PROTOCOL};
pub use proto::{Level, Visibility};

/// The name of the pipe of the installed service.
pub const PIPE_NAME: &str = r"\\.\pipe\rust-tools-svc";
/// The name of the pipe of `rtsvc console` (development): a client that checks
/// the server never connects to it.
pub const DEV_PIPE_NAME: &str = r"\\.\pipe\rust-tools-svc-dev";
/// The name of the Windows service.
pub const SERVICE_NAME: &str = "RustToolsSvc";
