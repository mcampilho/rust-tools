//! Menus drawn in the window, for systems without Win32 menus. They come from
//! the same menu tree; `rt-gfx` holds their state and paints them, `main`
//! feeds them input through `rt_ui::menus`.

use crate::commands::Command;

pub use rt_gfx::menu::{build, parse_label, Title};

pub type Menu = rt_gfx::menu::Menu<Command>;
