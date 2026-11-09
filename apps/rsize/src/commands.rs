//! Commands, their menu labels (translation keys) and keyboard shortcuts.

use crate::i18n::{tr, Lang};
use crate::settings::ChartType;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Asks for a folder (local or on the network) and scans it.
    ChooseFolder,
    /// Back to the list of drives.
    Drives,
    Rescan,
    /// Stops the scan in progress.
    Stop,
    /// Starts rsize again with administrator rights, to read NTFS directly.
    #[cfg(windows)]
    RestartAdmin,
    /// "Analyze with rsize" on the Explorer menu of folders and drives (on or off).
    #[cfg(windows)]
    ExplorerMenu,
    /// Writes the folder shown to a CSV file.
    ExportCsv,
    Exit,
    CopyPath,
    /// Moves the selection to the Recycle Bin (trash), after asking.
    Delete,
    /// Opens the selected file or folder with its default program.
    OpenItem,
    Reveal,
    /// The treemap shows the selected folder.
    ZoomIn,
    ZoomOut,
    ZoomTop,
    /// Which chart is shown below the list.
    Chart(ChartType),
    SizeLogical,
    SizeAllocated,
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
            ChooseFolder => "Choose &Folder...",
            Drives => "Show &Drives",
            Rescan => "&Rescan",
            Stop => "&Stop",
            #[cfg(windows)]
            RestartAdmin => "Restart as &Administrator",
            #[cfg(windows)]
            ExplorerMenu => "&Explorer Context Menu",
            ExportCsv => "&Export to CSV...",
            Exit => "E&xit",
            CopyPath => "&Copy Path",
            Delete => "&Delete...",
            OpenItem => "&Open",
            Reveal => "Show in File &Manager",
            ZoomIn => "Zoom &In",
            ZoomOut => "Zoom O&ut",
            ZoomTop => "Zoom to &Top",
            Chart(ChartType::Treemap) => "Tree&map",
            Chart(ChartType::Pie) => "&Pie Chart",
            Chart(ChartType::Rings) => "&Rings",
            Chart(ChartType::Types) => "File T&ypes",
            SizeLogical => "File &Size",
            SizeAllocated => "Size on &Disk",
            Theme(0) => "Use &System Setting",
            Theme(1) => "&Light",
            Theme(_) => "&Dark",
            UiLanguage(0) => "Use &System Setting",
            UiLanguage(i) => return Err(Lang::ALL.get(i as usize - 1).map_or("", |l| l.native_name()).to_string()),
            Shortcuts => "&Keyboard Shortcuts",
            About => "&About rsize",
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
    ("Ctrl+O", ChooseFolder),
    ("Ctrl+D", Drives),
    ("F5", Rescan),
    ("Esc", Stop),
    ("Ctrl+C", CopyPath),
    ("Ctrl+Enter", OpenItem),
    ("Ctrl+E", Reveal),
    ("Enter", ZoomIn),
    ("Backspace", ZoomOut),
    ("Ctrl+Backspace", ZoomTop),
    ("F6", Chart(ChartType::Treemap)),
    ("F7", Chart(ChartType::Pie)),
    ("F8", Chart(ChartType::Rings)),
    ("F9", Chart(ChartType::Types)),
    ("Ctrl+S", ExportCsv),
    ("Delete", Delete),
    ("Ctrl+1", SizeLogical),
    ("Ctrl+2", SizeAllocated),
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
    let mut file = vec![Item(ChooseFolder), Item(Drives), Sep, Item(Rescan), Item(Stop), Sep, Item(ExportCsv), Sep];
    #[cfg(windows)]
    file.extend([Item(RestartAdmin), Sub("Windows &Integration", vec![Item(ExplorerMenu)]), Sep]);
    file.push(Item(Exit));
    vec![
        ("&File", file),
        ("&Edit", vec![Item(CopyPath), Sep, Item(OpenItem), Item(Reveal), Sep, Item(Delete)]),
        (
            "&View",
            vec![
                Item(ZoomIn),
                Item(ZoomOut),
                Item(ZoomTop),
                Sep,
                Item(Chart(ChartType::Treemap)),
                Item(Chart(ChartType::Pie)),
                Item(Chart(ChartType::Rings)),
                Item(Chart(ChartType::Types)),
                Sep,
                Item(SizeLogical),
                Item(SizeAllocated),
                Sep,
                Sub("T&heme", vec![Item(Theme(0)), Item(Theme(1)), Item(Theme(2))]),
                Sub("Display &Language", langs),
            ],
        ),
        ("&Help", vec![Item(Shortcuts), Item(About)]),
    ]
}

/// The context menu of a file or folder.
pub fn item_menu() -> Vec<M> {
    use rt_core::menu::M::*;
    vec![Item(OpenItem), Item(Reveal), Item(CopyPath), Sep, Item(ZoomIn), Item(ZoomOut), Sep, Item(Delete)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords() {
        assert_eq!(lookup(true, false, false, "o"), Some(ChooseFolder));
        assert_eq!(lookup(false, false, false, "f5"), Some(Rescan));
        assert_eq!(lookup(false, false, false, "enter"), Some(ZoomIn));
        assert_eq!(lookup(true, false, false, "enter"), Some(OpenItem));
        assert_eq!(lookup(false, false, false, "backspace"), Some(ZoomOut));
        assert_eq!(lookup(true, false, false, "backspace"), Some(ZoomTop));
        assert_eq!(lookup(false, false, false, "esc"), Some(Stop));
        assert_eq!(lookup(true, false, false, "2"), Some(SizeAllocated));
        assert_eq!(lookup(false, false, false, "f8"), Some(Chart(ChartType::Rings)));
        assert_eq!(lookup(false, false, false, "delete"), Some(Delete));
        assert_eq!(lookup(true, false, false, "s"), Some(ExportCsv));
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
