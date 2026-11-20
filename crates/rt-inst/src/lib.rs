//! The programs installed on a computer, without a window: the inventory
//! (registry, Windows Installer, the Store, dpkg and rpm). rinst shows it;
//! the privileged service reads it in the background.

pub mod appx;
pub mod inventory;
pub mod linux_pkgs;
pub mod msi;
