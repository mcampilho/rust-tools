//! A Scoop installation, read the way Scoop reads it. The specification is the
//! official Scoop (v0.6.0, `lib\*.ps1`); each function names the one it follows.
//! It also has the pieces that change an installation (`reset`, `uninstall`).
//!
//! - [`install::Scoop`]: where the roots, the cache and the configuration are.
//! - [`manifest::Manifest`]: a manifest, with the architecture rules.
//! - [`version::compare`]: Scoop's `Compare-Version`.
//! - [`buckets`]: the local buckets and the manifests in them.
//! - [`apps`]: what is installed and in which state.
//! - [`resolve`]: from what the user typed to a manifest, and the dependencies.
//! - [`download`]: the names in the cache, the hashes and the request headers.
//! - [`config`]: changing `config.json` and the install information.
//! - [`disk`]: deleting without following links, junctions.
//! - [`system`]: the registry, the Start menu and the processes ([`system::FakeSystem`] for tests).
//! - [`env`]: environment variables and `PATH`.
//! - [`shims`], [`links`] (`current`, `persist`, modules), [`shortcuts`]: what an installed app puts around itself.
//! - [`hooks`]: the scripts of manifests, run by Scoop's own code.

pub mod apps;
pub mod buckets;
pub mod config;
pub mod disk;
pub mod download;
pub mod env;
pub mod hooks;
pub mod install;
pub mod json;
pub mod links;
pub mod manifest;
pub mod resolve;
pub mod shims;
pub mod shortcuts;
pub mod system;
pub mod version;

#[cfg(test)]
mod testing;
