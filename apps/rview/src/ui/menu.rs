//! Menus drawn in the window, for systems without Win32 menus. They come from
//! the same menu tree; `rt-gfx` holds their state and paints them, `main`
//! feeds them input.

use crate::commands::Command;

pub use rt_gfx::menu::{build, parse_label, Title};

pub type Act = rt_gfx::menu::Act<Command>;
pub type Menu = rt_gfx::menu::Menu<Command>;
