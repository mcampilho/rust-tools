//! Commands, their menu labels (translation keys) and keyboard shortcuts.

use crate::i18n::{tr, Lang};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Asks for a folder (local or on the network) to add to the search.
    AddFolder,
    /// Searches the drives and folders chosen.
    Search,
    /// Back to the drives and folders, to change them.
    NewSearch,
    /// Stops the search in progress.
    Stop,
    /// Starts rdup again with administrator rights, to read NTFS directly.
    #[cfg(windows)]
    RestartAdmin,
    /// "Find duplicates with rdup" on the Explorer menu of folders and drives (on or off).
    #[cfg(windows)]
    ExplorerMenu,
    Exit,
    CopyPath,
    /// Opens the selected file with its default program.
    OpenItem,
    Reveal,
    /// Marks the selected copy, or unmarks it.
    ToggleMark,
    /// Marks all copies of each group but the one kept by a rule.
    KeepNewest,
    KeepOldest,
    KeepShortest,
    /// Asks for a folder: the copies in it are kept.
    KeepInFolder,
    UnmarkAll,
    /// Moves the marked copies to the Recycle Bin (the trash on Linux).
    Recycle,
    /// Asks for a folder and moves the marked copies into it.
    MoveTo,
    /// Writes the list shown to a CSV file.
    Export,
    /// The result: the copies of files, or the folders that hold copies.
    ShowFiles,
    ShowFolders,
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
            AddFolder => "&Add Folder...",
            Search => "Find &Duplicates",
            NewSearch => "&New Search",
            Stop => "&Stop",
            #[cfg(windows)]
            RestartAdmin => "Restart as Ad&ministrator",
            #[cfg(windows)]
            ExplorerMenu => "&Explorer Context Menu",
            Exit => "E&xit",
            CopyPath => "&Copy Path",
            OpenItem => "&Open",
            Reveal => "Show in File &Manager",
            ToggleMark => "Mar&k or Unmark",
            KeepNewest => "Keep the &Newest",
            KeepOldest => "Keep the &Oldest",
            KeepShortest => "Keep the &Shortest Path",
            KeepInFolder => "Keep Those in a &Folder...",
            UnmarkAll => "&Unmark All",
            Recycle if cfg!(windows) => "Move to &Recycle Bin",
            Recycle => "Move to &Trash",
            MoveTo => "Mo&ve To...",
            Export => "&Export to CSV...",
            ShowFiles => "&Files",
            ShowFolders => "F&olders",
            Theme(0) => "Use &System Setting",
            Theme(1) => "&Light",
            Theme(_) => "&Dark",
            UiLanguage(0) => "Use &System Setting",
            UiLanguage(i) => return Err(Lang::ALL.get(i as usize - 1).map_or("", |l| l.native_name()).to_string()),
            Shortcuts => "&Keyboard Shortcuts",
            About => "&About rdup",
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
    ("Ctrl+O", AddFolder),
    ("F5", Search),
    ("Ctrl+N", NewSearch),
    ("Esc", Stop),
    ("Ctrl+C", CopyPath),
    ("Ctrl+Enter", OpenItem),
    ("Ctrl+E", Reveal),
    ("Space", ToggleMark),
    ("Delete", Recycle),
    ("Ctrl+M", MoveTo),
    ("Ctrl+S", Export),
    ("Ctrl+1", ShowFiles),
    ("Ctrl+2", ShowFolders),
    ("F1", Shortcuts),
];

pub fn shortcut(cmd: Command) -> Option<&'static str> {
    BINDINGS.iter().find(|(_, c)| *c == cmd).map(|(k, _)| *k)
}

/// Finds the command for a key chord such as `Ctrl+O` (`key` in lower case).
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
    let mut file = vec![Item(AddFolder), Item(Search), Item(NewSearch), Item(Stop), Sep, Item(Export), Sep];
    #[cfg(windows)]
    file.extend([Item(RestartAdmin), Sub("Windows &Integration", vec![Item(ExplorerMenu)]), Sep]);
    file.push(Item(Exit));
    vec![
        ("&File", file),
        ("&Edit", vec![Item(CopyPath), Sep, Item(OpenItem), Item(Reveal), Sep, Item(Recycle), Item(MoveTo)]),
        (
            "&Mark",
            vec![Item(ToggleMark), Sep, Item(KeepNewest), Item(KeepOldest), Item(KeepShortest), Item(KeepInFolder), Sep, Item(UnmarkAll)],
        ),
        (
            "&View",
            vec![Item(ShowFiles), Item(ShowFolders), Sep, Sub("T&heme", vec![Item(Theme(0)), Item(Theme(1)), Item(Theme(2))]), Sub("Display &Language", langs)],
        ),
        ("&Help", vec![Item(Shortcuts), Item(About)]),
    ]
}

/// The context menu of a file.
pub fn item_menu() -> Vec<M> {
    use rt_core::menu::M::*;
    vec![Item(ToggleMark), Sep, Item(OpenItem), Item(Reveal), Item(CopyPath)]
}

/// The rules to mark copies, for the button under the list.
pub fn rule_menu() -> Vec<M> {
    use rt_core::menu::M::*;
    vec![Item(KeepNewest), Item(KeepOldest), Item(KeepShortest), Item(KeepInFolder), Sep, Item(UnmarkAll)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords() {
        assert_eq!(lookup(true, false, false, "o"), Some(AddFolder));
        assert_eq!(lookup(false, false, false, "f5"), Some(Search));
        assert_eq!(lookup(true, false, false, "n"), Some(NewSearch));
        assert_eq!(lookup(true, false, false, "enter"), Some(OpenItem));
        assert_eq!(lookup(false, false, false, "esc"), Some(Stop));
        assert_eq!(lookup(true, false, false, "2"), Some(ShowFolders));
        assert_eq!(lookup(false, false, false, "space"), Some(ToggleMark));
        assert_eq!(lookup(false, false, false, "delete"), Some(Recycle));
        assert_eq!(lookup(false, false, false, "o"), None);
        assert_eq!(lookup(true, true, false, "c"), None);
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
