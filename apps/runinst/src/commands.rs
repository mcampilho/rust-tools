//! Commands, their menu labels (translation keys) and keyboard shortcuts.

use crate::i18n::{tr, Lang};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Reads the programs again.
    Refresh,
    /// Starts runinst again with administrator rights, to read every user.
    #[cfg(windows)]
    RestartAdmin,
    /// Writes the list shown to a CSV file.
    Export,
    Exit,
    /// Uninstalls the programs selected, one after another.
    Uninstall,
    /// The same, without the uninstallers' questions where they allow it.
    UninstallQuiet,
    /// Changes or repairs the program selected.
    Modify,
    SelectAll,
    /// The program selected: its install folder, its uninstaller's folder.
    OpenFolder,
    CopyName,
    CopyCommand,
    CopyKey,
    /// Everything known about it, as text.
    CopyDetails,
    /// Empties the search.
    ClearSearch,
    /// Looks for what programs removed before left behind.
    FindLeftovers,
    /// Removes the leftovers ticked (after asking).
    RemoveLeftovers,
    /// Closes the list of leftovers.
    CloseLeftovers,
    /// The folder of the leftover selected.
    OpenLeftover,
    /// The search box's own: its right-click menu and its keys.
    TextCut,
    TextCopy,
    TextPaste,
    TextDelete,
    TextSelectAll,
    /// Whose programs: index into `Whose::ALL`.
    Whose(u8),
    ShowSystem,
    ShowUpdates,
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
            Refresh => "&Refresh",
            #[cfg(windows)]
            RestartAdmin => "Restart as Ad&ministrator",
            Export => "&Export to CSV...",
            Exit => "E&xit",
            Uninstall => "Un&install...",
            UninstallQuiet => "Uninstall &Quietly...",
            Modify => "&Modify or Repair...",
            SelectAll => "Select &All",
            OpenFolder => "Open the Install &Folder",
            CopyName => "Copy the &Name",
            CopyCommand => "Copy the &Uninstall Command",
            CopyKey => "Copy the Registry &Key",
            CopyDetails => "Copy All the &Details",
            ClearSearch => "&Clear the Search",
            FindLeftovers => "Find &Leftovers...",
            RemoveLeftovers => "Remove the Ticked Ones...",
            CloseLeftovers => "Close This List",
            OpenLeftover => "Open the Folder",
            TextCut => "Cu&t",
            TextCopy => "&Copy",
            TextPaste => "&Paste",
            TextDelete => "&Delete",
            TextSelectAll => "Select &All",
            Whose(0) => "&All Programs",
            Whose(1) => "For &Everyone",
            Whose(2) => "For &Me Only",
            Whose(_) => "For &Other Users",
            ShowSystem => "&System Components",
            ShowUpdates => "&Updates",
            Theme(0) => "Use &System Setting",
            Theme(1) => "&Light",
            Theme(_) => "&Dark",
            UiLanguage(0) => "Use &System Setting",
            UiLanguage(i) => return Err(Lang::ALL.get(i as usize - 1).map_or("", |l| l.native_name()).to_string()),
            Shortcuts => "&Keyboard Shortcuts",
            About => "&About runinst",
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
    ("F5", Refresh),
    ("Delete", Uninstall),
    ("Shift+Delete", UninstallQuiet),
    ("Ctrl+A", SelectAll),
    ("Ctrl+S", Export),
    ("Ctrl+E", OpenFolder),
    ("Ctrl+C", CopyName),
    ("Ctrl+Shift+C", CopyDetails),
    ("Esc", ClearSearch),
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
    let mut file = vec![Item(Refresh), Item(Export), Sep, Item(FindLeftovers), Sep];
    #[cfg(windows)]
    file.extend([Item(RestartAdmin), Sep]);
    file.push(Item(Exit));
    vec![
        ("&File", file),
        ("&Program", program_menu()),
        (
            "&View",
            vec![
                Item(Whose(0)),
                Item(Whose(1)),
                Item(Whose(2)),
                Item(Whose(3)),
                Sep,
                Item(ShowSystem),
                Item(ShowUpdates),
                Sep,
                Item(ClearSearch),
                Sep,
                Sub("T&heme", vec![Item(Theme(0)), Item(Theme(1)), Item(Theme(2))]),
                Sub("Display &Language", langs),
            ],
        ),
        ("&Help", vec![Item(Shortcuts), Item(About)]),
    ]
}

/// What can be done with the program selected (also its context menu).
pub fn program_menu() -> Vec<M> {
    use rt_core::menu::M::*;
    vec![Item(Uninstall), Item(UninstallQuiet), Item(Modify), Sep, Item(OpenFolder), Sep, Item(CopyName), Item(CopyCommand), Item(CopyKey), Item(CopyDetails), Sep, Item(SelectAll)]
}

/// The search box's right-click menu.
pub fn search_menu() -> Vec<M> {
    use rt_core::menu::M::*;
    vec![Item(TextCut), Item(TextCopy), Item(TextPaste), Item(TextDelete), Sep, Item(TextSelectAll)]
}

/// The buttons above the list.
pub const ACTIONS: [Command; 6] = [Uninstall, UninstallQuiet, Modify, OpenFolder, CopyDetails, FindLeftovers];

/// The buttons above the list of leftovers.
pub const LEFTOVER_ACTIONS: [Command; 4] = [RemoveLeftovers, OpenLeftover, CopyDetails, CloseLeftovers];

/// The buttons above a tab's list.
pub fn actions(tab: crate::app::Tab) -> &'static [Command] {
    if tab == crate::app::Tab::Leftovers {
        &LEFTOVER_ACTIONS
    } else {
        &ACTIONS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords() {
        assert_eq!(lookup(false, false, false, "f5"), Some(Refresh));
        assert_eq!(lookup(true, false, false, "c"), Some(CopyName));
        assert_eq!(lookup(true, true, false, "c"), Some(CopyDetails));
        assert_eq!(lookup(false, false, false, "esc"), Some(ClearSearch));
        assert_eq!(lookup(false, false, false, "c"), None);
        assert_eq!(lookup(false, false, false, "delete"), Some(Uninstall));
        assert_eq!(lookup(false, true, false, "delete"), Some(UninstallQuiet));
        assert_eq!(lookup(true, false, false, "a"), Some(SelectAll));
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
