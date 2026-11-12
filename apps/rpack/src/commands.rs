//! Commands, their menu labels (translation keys) and keyboard shortcuts.

use crate::hash::Algo;
use crate::i18n::{tr, Lang};

/// `Command::Hash` for every hash at once.
pub const ALL_HASHES: u8 = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Asks for an archive to open.
    OpenArchive,
    CloseArchive,
    Exit,
    /// The row selected: a folder is entered, a file opened with its program.
    OpenItem,
    /// Back to the folder above.
    Up,
    /// Reads the folder shown again.
    Refresh,
    SelectAll,
    /// Opens the "Add to archive" form for the files and folders chosen.
    AddDialog,
    /// Adds what is chosen to `<name>.7z` or `<name>.zip` beside it, at once.
    AddTo7z,
    AddToZip,
    /// Each file or folder chosen into an archive of its own, as last chosen.
    AddEach,
    /// Extracts the rows selected (or everything) to the folder shown.
    Extract,
    /// The archives chosen on disk: into the folder they are in, or into a
    /// folder named after each.
    ExtractHere,
    ExtractNamed,
    /// Asks for a folder, then extracts there.
    ExtractTo,
    /// Asks for the folder to extract to, without extracting.
    ChooseFolder,
    /// Reads everything, checking it.
    Test,
    /// Stops the work running.
    Stop,
    /// What to do with a file already there: index into `Overwrite::ALL`.
    Overwrite(u8),
    /// Opens the folder after extracting (on or off).
    OpenAfter,
    /// Hashes of the files chosen: index into `Algo::ALL`, or `ALL_HASHES`.
    Hash(u8),
    /// Writes the last hashes to a sum file.
    SaveHashes,
    /// The files chosen: shown in the file manager, their paths copied, or
    /// moved to the Recycle Bin (the trash on Linux).
    Reveal,
    CopyPath,
    Recycle,
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
            OpenArchive => "&Open Archive...",
            CloseArchive => "&Close Archive",
            Exit => "E&xit",
            OpenItem => "&Open",
            Up => "&Up One Level",
            Refresh => "&Refresh",
            SelectAll => "Select &All",
            AddDialog => "&Add to Archive...",
            AddTo7z => "Add to a &7z Archive",
            AddToZip => "Add to a &Zip Archive",
            AddEach => "Add Each to &Its Own Archive",
            Extract => "&Extract",
            ExtractHere => "Extract &Here",
            ExtractNamed => "Extract Each to a &New Folder",
            ExtractTo => "Extract to &Folder...",
            ChooseFolder => "Choose the &Destination...",
            Test => "&Test",
            Stop => "&Stop",
            Overwrite(0) => "&Replace It",
            Overwrite(1) => "&Skip It",
            Overwrite(_) => "&Keep Both",
            OpenAfter => "Open the Folder Afte&r Extracting",
            Hash(ALL_HASHES) => "&All Hashes",
            Hash(i) => return Err(Algo::ALL.get(i as usize).map_or("", |a| a.name()).to_string()),
            SaveHashes => "&Save Hashes...",
            Reveal => "Show in File &Manager",
            CopyPath => "&Copy Path",
            Recycle if cfg!(windows) => "Move to Recycle &Bin",
            Recycle => "Mo&ve to Trash",
            Theme(0) => "Use &System Setting",
            Theme(1) => "&Light",
            Theme(_) => "&Dark",
            UiLanguage(0) => "Use &System Setting",
            UiLanguage(i) => return Err(Lang::ALL.get(i as usize - 1).map_or("", |l| l.native_name()).to_string()),
            Shortcuts => "&Keyboard Shortcuts",
            About => "&About rpack",
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
    ("Ctrl+O", OpenArchive),
    ("Ctrl+W", CloseArchive),
    ("Enter", OpenItem),
    ("Backspace", Up),
    ("F5", Refresh),
    ("Ctrl+Shift+A", AddDialog),
    ("Delete", Recycle),
    ("Ctrl+A", SelectAll),
    ("Ctrl+E", Extract),
    ("Ctrl+Shift+E", ExtractTo),
    ("Ctrl+T", Test),
    ("Esc", Stop),
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
    vec![
        ("&File", vec![Item(OpenArchive), Item(CloseArchive), Sep, Item(Exit)]),
        (
            "&Edit",
            vec![Item(OpenItem), Item(Up), Item(Refresh), Sep, Item(SelectAll), Sep, Item(CopyPath), Item(Reveal), Item(Recycle)],
        ),
        (
            "&Archive",
            vec![
                Item(AddDialog),
                Item(AddTo7z),
                Item(AddToZip),
                Item(AddEach),
                Sep,
                Item(Extract),
                Item(ExtractHere),
                Item(ExtractNamed),
                Item(ExtractTo),
                Item(ChooseFolder),
                Sep,
                Item(Test),
                Item(Stop),
                Sep,
                Sub("&When a File Exists", vec![Item(Overwrite(0)), Item(Overwrite(1)), Item(Overwrite(2))]),
                Item(OpenAfter),
                Sep,
                Sub("Calc&ulate Hashes", hash_menu()),
            ],
        ),
        ("&View", vec![Sub("T&heme", vec![Item(Theme(0)), Item(Theme(1)), Item(Theme(2))]), Sub("Display &Language", langs)]),
        ("&Help", vec![Item(Shortcuts), Item(About)]),
    ]
}

/// The hashes to calculate, with saving the last ones.
pub fn hash_menu() -> Vec<M> {
    use rt_core::menu::M::*;
    let mut items: Vec<M> = (0..Algo::ALL.len()).map(|i| Item(Hash(i as u8))).collect();
    items.extend([Sep, Item(Hash(ALL_HASHES)), Sep, Item(SaveHashes)]);
    items
}

/// The context menu of rows on disk. `names`: what the quick items make
/// (`photos.7z`, `photos.zip`, the folder `photos`), shown in their labels.
pub fn disk_menu(names: Option<(String, String, Option<String>)>, archives: bool, several: bool) -> Vec<M> {
    use rt_core::menu::M::*;
    // A name in a menu: `&` shows as itself.
    let esc = |s: &str| s.replace('&', "&&");
    let mut items = vec![Item(OpenItem), Sep];
    if archives {
        items.push(Item(Extract));
        items.push(Item(ExtractHere));
        match names.as_ref().and_then(|n| n.2.as_ref()) {
            Some(folder) => items.push(Dyn(ExtractNamed, crate::i18n::tf("Extract to “{}”", &[&esc(folder)]))),
            None => items.push(Item(ExtractNamed)),
        }
        items.extend([Item(ExtractTo), Item(Test), Sep]);
    }
    items.push(Item(AddDialog));
    match &names {
        Some((a, b, _)) => {
            items.push(Dyn(AddTo7z, crate::i18n::tf("Add to “{}”", &[&esc(a)])));
            items.push(Dyn(AddToZip, crate::i18n::tf("Add to “{}”", &[&esc(b)])));
        }
        None => items.extend([Item(AddTo7z), Item(AddToZip)]),
    }
    if several {
        items.push(Item(AddEach));
    }
    items.extend([Sep, Sub("Calc&ulate Hashes", hash_menu()), Sep, Item(Reveal), Item(CopyPath), Item(Recycle)]);
    items
}

/// The context menu of rows inside an archive.
pub fn item_menu() -> Vec<M> {
    use rt_core::menu::M::*;
    vec![Item(OpenItem), Sep, Item(Extract), Item(ExtractTo), Sep, Sub("Calc&ulate Hashes", hash_menu()), Sep, Item(CopyPath)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords() {
        assert_eq!(lookup(true, false, false, "o"), Some(OpenArchive));
        assert_eq!(lookup(true, false, false, "e"), Some(Extract));
        assert_eq!(lookup(true, true, false, "e"), Some(ExtractTo));
        assert_eq!(lookup(false, false, false, "backspace"), Some(Up));
        assert_eq!(lookup(false, false, false, "enter"), Some(OpenItem));
        assert_eq!(lookup(false, false, false, "esc"), Some(Stop));
        assert_eq!(lookup(false, false, false, "o"), None);
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
