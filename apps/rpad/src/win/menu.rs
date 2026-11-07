//! Native Win32 menus built from rpad's command table (see `rt-sys`).

use crate::app::commands::Command;

pub use rt_sys::win::menu::popup;

pub type MenuBar = rt_sys::win::menu::MenuBar<Command>;
