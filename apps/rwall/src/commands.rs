//! The menu of the notification-area icon.

use crate::i18n::tr;
pub use rt_core::menu::{MenuCommand, M};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum TrayCmd {
    Next = 1,
    About,
    Settings,
    ShowFile,
    Exit,
    /// Pauses or resumes the dynamic wallpaper (Windows).
    LivePause,
}

/// The text of [`TrayCmd::LivePause`] while paused.
pub const RESUME: &str = "&Resume the dynamic wallpaper";

impl TrayCmd {
    pub const ALL: [TrayCmd; 6] = [TrayCmd::Next, TrayCmd::About, TrayCmd::Settings, TrayCmd::ShowFile, TrayCmd::Exit, TrayCmd::LivePause];

    /// The untranslated label, with its `&` access key.
    pub fn key(self) -> &'static str {
        match self {
            TrayCmd::Next => "&Next picture",
            TrayCmd::About => "&About this picture",
            TrayCmd::Settings => "&Settings…",
            TrayCmd::ShowFile => "Show the picture &file",
            TrayCmd::Exit => "E&xit",
            TrayCmd::LivePause => "&Pause the dynamic wallpaper",
        }
    }

    pub fn id(self) -> u32 {
        self as u32
    }

    pub fn from_id(id: u32) -> Option<TrayCmd> {
        TrayCmd::ALL.into_iter().find(|c| c.id() == id)
    }
}

impl MenuCommand for TrayCmd {
    fn label(self) -> String {
        tr(self.key())
    }

    fn shortcut(self) -> Option<&'static str> {
        None
    }
}

/// The tray menu, top to bottom. The dynamic wallpaper plays on Windows only.
pub fn tray_menu() -> Vec<M<TrayCmd>> {
    let mut m = vec![M::Item(TrayCmd::Next), M::Item(TrayCmd::About)];
    if cfg!(windows) {
        m.push(M::Item(TrayCmd::LivePause));
    }
    m.extend([M::Item(TrayCmd::Settings), M::Item(TrayCmd::ShowFile), M::Sep, M::Item(TrayCmd::Exit)]);
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for c in TrayCmd::ALL {
            assert_eq!(TrayCmd::from_id(c.id()), Some(c));
        }
        assert_eq!(TrayCmd::from_id(0), None);
        let mut n = 0;
        M::each_command(&tray_menu(), &mut |_| n += 1);
        assert_eq!(n, if cfg!(windows) { TrayCmd::ALL.len() } else { TrayCmd::ALL.len() - 1 });
    }
}
