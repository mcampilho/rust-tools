//! Commands, their menu labels (translation keys) and keyboard shortcuts.

use crate::i18n::{tr, Lang};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Looks again at what each target holds.
    Analyse,
    /// Deletes what the targets ticked hold (after asking).
    Clean,
    /// Stops analysing or cleaning after the target now.
    Stop,
    /// Starts rclean again with administrator rights.
    #[cfg(windows)]
    RestartAdmin,
    Exit,
    TickAll,
    UntickAll,
    TickDefaults,
    /// The folder of the target selected.
    OpenFolder,
    /// Everything about the target selected, as text.
    CopyDetails,
    /// 0 system, 1 light, 2 dark.
    Theme(u8),
    /// 0 follows the system, 1.. index into `Lang::ALL` + 1.
    UiLanguage(u8),
    Shortcuts,
    About,
}

use Command::*;

impl Command {
    /// English label with `&` access key (the translation key), or the final
    /// text for labels that are not translated.
    pub fn label_key(self) -> Result<&'static str, String> {
        Ok(match self {
            Analyse => "&Analyze",
            Clean => "&Clean...",
            Stop => "&Stop",
            #[cfg(windows)]
            RestartAdmin => "Restart as Ad&ministrator",
            Exit => "E&xit",
            TickAll => "Tick &All",
            UntickAll => "&Untick All",
            TickDefaults => "Tick the U&sual Ones",
            OpenFolder => "Open the &Folder",
            CopyDetails => "Copy the &Details",
            Theme(0) => "Use &System Setting",
            Theme(1) => "&Light",
            Theme(_) => "&Dark",
            UiLanguage(0) => "Use &System Setting",
            UiLanguage(i) => return Err(Lang::ALL.get(i as usize - 1).map_or("", |l| l.native_name()).to_string()),
            Shortcuts => "&Keyboard Shortcuts",
            About => "&About rclean",
        })
    }

    pub fn label(self) -> String {
        match self.label_key() {
            Ok(k) => tr(k),
            Err(s) => s,
        }
    }
}

/// Keyboard shortcuts (shown in menus as written here).
pub const BINDINGS: &[(&str, Command)] = &[
    ("F5", Analyse),
    ("Ctrl+L", Clean),
    ("Esc", Stop),
    ("Ctrl+A", TickAll),
    ("Ctrl+E", OpenFolder),
    ("Ctrl+Shift+C", CopyDetails),
    ("F1", Shortcuts),
];

pub fn shortcut(cmd: Command) -> Option<&'static str> {
    BINDINGS.iter().find(|(_, c)| *c == cmd).map(|(k, _)| *k)
}

/// Finds the command for a key chord such as `Ctrl+L` (`key` in lower case).
pub fn lookup(ctrl: bool, shift: bool, alt: bool, key: &str) -> Option<Command> {
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

pub type M = rt_core::menu::M<Command>;

impl rt_core::menu::MenuCommand for Command {
    fn label(self) -> String {
        Command::label(self)
    }
    fn shortcut(self) -> Option<&'static str> {
        shortcut(self)
    }
}

pub fn menu_tree() -> Vec<(&'static str, Vec<M>)> {
    use rt_core::menu::M::*;
    let mut langs = vec![Item(UiLanguage(0)), Sep];
    langs.extend((1..=Lang::ALL.len()).map(|i| Item(UiLanguage(i as u8))));
    let mut file = vec![Item(Analyse), Item(Clean), Item(Stop), Sep];
    #[cfg(windows)]
    file.extend([Item(RestartAdmin), Sep]);
    file.push(Item(Exit));
    vec![
        ("&File", file),
        ("&Edit", vec![Item(TickAll), Item(UntickAll), Item(TickDefaults), Sep, Item(OpenFolder), Item(CopyDetails)]),
        ("&View", vec![Sub("T&heme", vec![Item(Theme(0)), Item(Theme(1)), Item(Theme(2))]), Sub("Display &Language", langs)]),
        ("&Help", vec![Item(Shortcuts), Item(About)]),
    ]
}

/// The buttons above the list.
pub const ACTIONS: [Command; 3] = [Analyse, Clean, OpenFolder];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords() {
        assert_eq!(lookup(false, false, false, "f5"), Some(Analyse));
        assert_eq!(lookup(true, false, false, "l"), Some(Clean));
        assert_eq!(lookup(false, false, false, "l"), None);
        assert_eq!(lookup(false, false, false, "esc"), Some(Stop));
        assert_eq!(lookup(true, true, false, "c"), Some(CopyDetails));
    }

    #[test]
    fn every_command_in_the_menus_is_unique() {
        let mut seen = Vec::new();
        for (_, items) in menu_tree() {
            M::each_command(&items, &mut |c| seen.push(c));
        }
        let unique: std::collections::HashSet<_> = seen.iter().collect();
        assert_eq!(unique.len(), seen.len());
    }
}
