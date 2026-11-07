//! Menus drawn in the window, for systems without Win32 menus. They come from
//! the same command table: `rt-gfx` holds their state and navigation, `view`
//! paints them and `main` feeds them input.

use crate::app::commands::Command;

pub use rt_gfx::menu::{build, parse_label, Title};

pub type Level = rt_gfx::menu::Level<Command>;
pub type Act = rt_gfx::menu::Act<Command>;
pub type Menu = rt_gfx::menu::Menu<Command>;
