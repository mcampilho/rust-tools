//! Commands of the window, their menu labels (translation keys) and keyboard
//! shortcuts. The commands of a page are the page's own (`Cmd::Clean(…)`
//! holds one of rclean's), so the page needs no change to sit here.

use crate::i18n::{tr, Lang};
use rclean::commands::Command as CleanCmd;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cmd {
    /// A command of the cleaning page.
    Clean(CleanCmd),
    /// Shows the sidebar's names, or only its icons.
    Sidebar,
    /// Starts rcare again with administrator rights.
    RestartAdmin,
    Exit,
    /// 0 system, 1 light, 2 dark.
    Theme(u8),
    /// 0 follows the system, 1.. index into `Lang::ALL` + 1.
    UiLanguage(u8),
    Shortcuts,
    /// Asks GitHub for a newer version (only when chosen).
    CheckUpdates,
    About,
}

use Cmd::*;

impl Cmd {
    /// What a command of a page means for the window: the ones that are the
    /// window's (exit, help…) turn into its own; the others stay the page's.
    pub fn from_clean(c: CleanCmd) -> Cmd {
        match c {
            CleanCmd::Exit => Exit,
            #[cfg(windows)]
            CleanCmd::RestartAdmin => RestartAdmin,
            CleanCmd::Shortcuts => Shortcuts,
            CleanCmd::CheckUpdates => CheckUpdates,
            CleanCmd::Theme(t) => Theme(t),
            CleanCmd::UiLanguage(i) => UiLanguage(i),
            c => Clean(c),
        }
    }

    /// English label with `&` access key (the translation key), or the final
    /// text for labels that are not translated.
    pub fn label_key(self) -> Result<&'static str, String> {
        Ok(match self {
            Clean(c) => return c.label_key(),
            Sidebar => "&Collapse Sidebar",
            RestartAdmin => "Restart as Ad&ministrator",
            Exit => "E&xit",
            Theme(0) => "Use &System Setting",
            Theme(1) => "&Light",
            Theme(_) => "&Dark",
            UiLanguage(0) => "Use &System Setting",
            UiLanguage(i) => return Err(Lang::ALL.get(i as usize - 1).map_or("", |l| l.native_name()).to_string()),
            Shortcuts => "&Keyboard Shortcuts",
            CheckUpdates => "Check for &Updates...",
            About => "&About rcare",
        })
    }

    pub fn label(self) -> String {
        match self.label_key() {
            Ok(k) => tr(k),
            Err(s) => s,
        }
    }
}

/// Keyboard shortcuts of the window (a page's are its own).
pub const BINDINGS: &[(&str, Cmd)] = &[("Ctrl+B", Sidebar), ("F1", Shortcuts)];

pub fn shortcut(cmd: Cmd) -> Option<&'static str> {
    match cmd {
        Clean(c) => rclean::commands::shortcut(c),
        c => BINDINGS.iter().find(|(_, x)| *x == c).map(|(k, _)| *k),
    }
}

/// The window's command for a chord such as `Ctrl+B` (`key` in lower case).
pub fn lookup(ctrl: bool, shift: bool, alt: bool, key: &str) -> Option<Cmd> {
    BINDINGS
        .iter()
        .find(|(k, _)| {
            let mut parts: Vec<&str> = k.split('+').collect();
            let last = parts.pop().unwrap_or("");
            let has = |m: &str| parts.contains(&m);
            has("Ctrl") == ctrl && has("Shift") == shift && has("Alt") == alt && last.eq_ignore_ascii_case(key)
        })
        .map(|(_, c)| *c)
}

pub type M = rt_core::menu::M<Cmd>;

impl rt_core::menu::MenuCommand for Cmd {
    fn label(self) -> String {
        Cmd::label(self)
    }
    fn shortcut(self) -> Option<&'static str> {
        shortcut(self)
    }
}

/// The menu bar with the menu of the page `page` between File and View.
pub fn menu_tree(page: usize) -> Vec<(&'static str, Vec<M>)> {
    use rt_core::menu::M::*;
    let mut langs = vec![Item(UiLanguage(0)), Sep];
    langs.extend((1..=Lang::ALL.len()).map(|i| Item(UiLanguage(i as u8))));
    let file = vec![Item(RestartAdmin), Sep, Item(Exit)];
    let mut bar = vec![("&File", file)];
    bar.extend(page_menus(page));
    bar.push((
        "&View",
        vec![Item(Sidebar), Sep, Sub("T&heme", vec![Item(Theme(0)), Item(Theme(1)), Item(Theme(2))]), Sub("Display &Language", langs)],
    ));
    bar.push(("&Help", vec![Item(Shortcuts), Item(CheckUpdates), Sep, Item(About)]));
    bar
}

/// The menus of one page: what its own commands are.
fn page_menus(page: usize) -> Vec<(&'static str, Vec<M>)> {
    use rt_core::menu::M::*;
    use CleanCmd as C;
    let own = |items: Vec<rt_core::menu::M<CleanCmd>>| -> Vec<M> { items.into_iter().map(|m| m.map(&Clean)).collect() };
    match crate::pages::PAGES.get(page).map(|p| p.id) {
        Some("clean") => vec![
            ("&Cleaner", own(vec![Item(C::Analyse), Item(C::Clean), Item(C::Stop)])),
            (
                "&Edit",
                own(vec![Item(C::TickAll), Item(C::UntickAll), Item(C::TickDefaults), Sep, Item(C::OpenFolder), Item(C::CopyDetails)]),
            ),
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords() {
        assert_eq!(lookup(true, false, false, "b"), Some(Sidebar));
        assert_eq!(lookup(false, false, false, "f1"), Some(Shortcuts));
        assert_eq!(lookup(false, false, false, "b"), None);
    }

    #[test]
    fn window_commands_of_a_page_become_the_windows() {
        assert_eq!(Cmd::from_clean(CleanCmd::Exit), Exit);
        assert_eq!(Cmd::from_clean(CleanCmd::Shortcuts), Shortcuts);
        assert_eq!(Cmd::from_clean(CleanCmd::Analyse), Clean(CleanCmd::Analyse));
    }

    #[test]
    fn every_command_in_the_menus_is_unique() {
        for page in 0..crate::pages::PAGES.len() {
            let mut seen = Vec::new();
            for (_, items) in menu_tree(page) {
                M::each_command(&items, &mut |c| seen.push(c));
            }
            let unique: std::collections::HashSet<_> = seen.iter().collect();
            assert_eq!(unique.len(), seen.len());
        }
    }
}
