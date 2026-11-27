//! What the user chose, as kept in `settings.ini`.

use crate::i18n::Lang;
use crate::items::Items;
use crate::storage::Store;
use crate::ui::theme::CHOICES;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// How the programs of a folder of the user get their categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FolderMode {
    /// From its subfolders when shortcuts are arranged in them.
    #[default]
    Auto,
    /// Its subfolders are the categories, and theirs the subgroups.
    Subfolders,
    /// The folder is a category, and its subfolders the subgroups.
    One,
    /// Only the programs' names say.
    Names,
}

impl FolderMode {
    pub const ALL: [FolderMode; 4] = [FolderMode::Auto, FolderMode::Subfolders, FolderMode::One, FolderMode::Names];

    pub fn key(self) -> &'static str {
        match self {
            FolderMode::Auto => "auto",
            FolderMode::Subfolders => "subfolders",
            FolderMode::One => "one",
            FolderMode::Names => "names",
        }
    }

    /// The English name, which is also the translation key.
    pub fn name(self) -> &'static str {
        match self {
            FolderMode::Auto => "Automatic",
            FolderMode::Subfolders => "Subfolders",
            FolderMode::One => "One category",
            FolderMode::Names => "By name",
        }
    }
}

/// The key combinations offered to open the menu (Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hotkey {
    #[default]
    CtrlAltSpace,
    CtrlShiftSpace,
    AltShiftSpace,
    WinShiftSpace,
    Off,
}

impl Hotkey {
    pub const ALL: [Hotkey; 5] = [Hotkey::CtrlAltSpace, Hotkey::CtrlShiftSpace, Hotkey::AltShiftSpace, Hotkey::WinShiftSpace, Hotkey::Off];

    /// The keys as written on screen; empty when off.
    pub fn label(self) -> &'static str {
        match self {
            Hotkey::CtrlAltSpace => "Ctrl+Alt+Space",
            Hotkey::CtrlShiftSpace => "Ctrl+Shift+Space",
            Hotkey::AltShiftSpace => "Alt+Shift+Space",
            Hotkey::WinShiftSpace => "Win+Shift+Space",
            Hotkey::Off => "",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Hotkey::CtrlAltSpace => "ctrl+alt+space",
            Hotkey::CtrlShiftSpace => "ctrl+shift+space",
            Hotkey::AltShiftSpace => "alt+shift+space",
            Hotkey::WinShiftSpace => "win+shift+space",
            Hotkey::Off => "off",
        }
    }

    /// `(MOD_* flags, virtual key)`; None when off.
    pub fn codes(self) -> Option<(u32, u32)> {
        // MOD_ALT 1, MOD_CONTROL 2, MOD_SHIFT 4, MOD_WIN 8; VK_SPACE 0x20.
        match self {
            Hotkey::CtrlAltSpace => Some((2 | 1, 0x20)),
            Hotkey::CtrlShiftSpace => Some((2 | 4, 0x20)),
            Hotkey::AltShiftSpace => Some((1 | 4, 0x20)),
            Hotkey::WinShiftSpace => Some((8 | 4, 0x20)),
            Hotkey::Off => None,
        }
    }
}

/// Where on the screen the menu opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    /// The bottom right corner, by the notification area.
    #[default]
    Right,
    Left,
    Centre,
    Pointer,
}

impl Position {
    pub const ALL: [Position; 4] = [Position::Right, Position::Left, Position::Centre, Position::Pointer];

    pub fn key(self) -> &'static str {
        match self {
            Position::Right => "right",
            Position::Left => "left",
            Position::Centre => "centre",
            Position::Pointer => "pointer",
        }
    }

    /// The English name, which is also the translation key.
    pub fn name(self) -> &'static str {
        match self {
            Position::Right => "Right",
            Position::Left => "Left",
            Position::Centre => "Center",
            Position::Pointer => "At the pointer",
        }
    }

    /// The top left corner of a menu of `size` in the work area `(left, top,
    /// right, bottom)`, with the pointer at `at`.
    pub fn place(self, area: (i32, i32, i32, i32), size: (i32, i32), at: (i32, i32)) -> (i32, i32) {
        let (left, top, right, bottom) = area;
        let (width, height) = size;
        let (x, y) = match self {
            Position::Right => (right - width - 8, bottom - height - 8),
            Position::Left => (left + 8, bottom - height - 8),
            Position::Centre => ((left + right - width) / 2, (top + bottom - height) / 2),
            Position::Pointer => at,
        };
        // Always inside the work area.
        (x.min(right - width).max(left), y.min(bottom - height).max(top))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// 0 as the system, 1 light, 2 dark.
    pub theme: u8,
    pub language: Option<Lang>,
    /// The shortcuts Scoop makes in the Start menu.
    pub scoop: bool,
    /// The rest of the Start menu.
    pub start_menu: bool,
    /// The user's own folders of programs and shortcuts.
    pub folders: Vec<PathBuf>,
    /// The folders whose mode is not [`FolderMode::Auto`].
    pub modes: BTreeMap<PathBuf, FolderMode>,
    pub hotkey: Hotkey,
    /// The size of the menu, in percent of the compact one.
    pub size: u16,
    /// How much of what is behind shows through, in percent (Windows).
    pub transparency: u8,
    /// Ask for administrator rights at every start (Windows).
    pub always_admin: bool,
    /// The colour of the frame (see `ui::theme`).
    pub colour: u8,
    /// Taller rows with larger icons.
    pub large: bool,
    /// Where the menu opens.
    pub position: Position,
    /// Changes to programs and programs added.
    pub items: Items,
}

pub const SIZES: [u16; 3] = [100, 115, 130];
pub const TRANSPARENCIES: [u8; 5] = [0, 10, 20, 30, 40];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: 0,
            language: None,
            scoop: true,
            start_menu: true,
            folders: Vec::new(),
            modes: BTreeMap::new(),
            hotkey: Hotkey::default(),
            size: 100,
            transparency: 0,
            always_admin: false,
            colour: 0,
            large: false,
            position: Position::Right,
            items: Items::default(),
        }
    }
}

/// The drive of a path (`E:`), on Windows.
fn drive(p: &Path) -> Option<std::ffi::OsString> {
    match p.components().next() {
        Some(Component::Prefix(x)) => Some(x.as_os_str().to_ascii_uppercase()),
        _ => None,
    }
}

/// In portable mode a folder on rlaunch's own drive is kept as `*:\…`, so it
/// is found again when the pen drive gets another letter.
pub fn pack(folder: &Path, own: Option<&Path>) -> String {
    let text = folder.to_string_lossy().into_owned();
    match (drive(folder), own.and_then(drive)) {
        (Some(a), Some(b)) if a == b && a.len() == 2 => format!("*:{}", &text[2..]),
        _ => text,
    }
}

/// The other way: `*:` is the drive rlaunch is on now.
pub fn unpack(text: &str, own: Option<&Path>) -> PathBuf {
    match (text.strip_prefix("*:"), own.and_then(drive)) {
        (Some(rest), Some(d)) => PathBuf::from(format!("{}{rest}", d.to_string_lossy())),
        _ => PathBuf::from(text),
    }
}

impl Settings {
    pub fn mode_of(&self, folder: &Path) -> FolderMode {
        self.modes.get(folder).copied().unwrap_or_default()
    }

    pub fn set_mode(&mut self, folder: &Path, mode: FolderMode) {
        if mode == FolderMode::Auto {
            self.modes.remove(folder);
        } else {
            self.modes.insert(folder.to_path_buf(), mode);
        }
    }

    /// `own` is the folder of rlaunch in portable mode, else None.
    pub fn load(s: &Store, own: Option<&Path>) -> Settings {
        let d = Settings::default();
        let flag = |k: &str, or: bool| s.get(k).map_or(or, |v| v == "1");
        Settings {
            theme: s.get("theme").and_then(|v| v.parse().ok()).filter(|t| *t <= 2).unwrap_or(0),
            language: s.get("language").and_then(|v| Lang::ALL.into_iter().find(|l| l.tag() == v)),
            scoop: flag("scoop", d.scoop),
            start_menu: flag("start_menu", d.start_menu),
            folders: s.get_all("folder").filter(|v| !v.is_empty()).map(|v| unpack(v, own)).collect(),
            modes: s
                .get_all("folder_mode")
                .filter_map(|v| v.split_once('\t'))
                .filter_map(|(f, m)| Some((unpack(f, own), FolderMode::ALL.into_iter().find(|x| x.key() == m)?)))
                .collect(),
            hotkey: s.get("hotkey").and_then(|v| Hotkey::ALL.into_iter().find(|h| h.key() == v)).unwrap_or_default(),
            size: s.get("size").and_then(|v| v.parse().ok()).filter(|v| SIZES.contains(v)).unwrap_or(d.size),
            transparency: s.get("transparency").and_then(|v| v.parse().ok()).filter(|v| TRANSPARENCIES.contains(v)).unwrap_or(0),
            always_admin: flag("always_admin", false),
            colour: s.get("colour").and_then(|v| v.parse().ok()).filter(|c| *c < CHOICES).unwrap_or(0),
            large: flag("large", false),
            position: s.get("position").and_then(|v| Position::ALL.into_iter().find(|p| p.key() == v)).unwrap_or_default(),
            items: Items::load(s, own),
        }
    }

    pub fn to_store(&self, own: Option<&Path>) -> Store {
        let mut s = Store::default();
        let bit = |b: bool| if b { "1" } else { "0" };
        s.set("theme", self.theme.to_string());
        s.set("language", self.language.map_or("", |l| l.tag()));
        s.set("scoop", bit(self.scoop));
        s.set("start_menu", bit(self.start_menu));
        for f in &self.folders {
            s.push("folder", pack(f, own));
        }
        for (f, mode) in &self.modes {
            s.push("folder_mode", format!("{}\t{}", pack(f, own), mode.key()));
        }
        s.set("hotkey", self.hotkey.key());
        s.set("size", self.size.to_string());
        s.set("transparency", self.transparency.to_string());
        s.set("always_admin", bit(self.always_admin));
        s.set("colour", self.colour.to_string());
        s.set("large", bit(self.large));
        s.set("position", self.position.key());
        self.items.save(&mut s, own);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_stays_in_the_work_area() {
        let (area, size) = ((0, 0, 1920, 1040), (350, 480));
        assert_eq!(Position::Right.place(area, size, (0, 0)), (1562, 552));
        assert_eq!(Position::Left.place(area, size, (0, 0)), (8, 552));
        assert_eq!(Position::Centre.place(area, size, (0, 0)), (785, 280));
        assert_eq!(Position::Pointer.place(area, size, (1900, 20)), (1570, 20));
    }

    #[test]
    fn settings_come_back() {
        let mut st = Settings {
            theme: 2,
            language: Some(Lang::PtPT),
            scoop: false,
            hotkey: Hotkey::WinShiftSpace,
            size: 130,
            transparency: 20,
            always_admin: true,
            colour: 3,
            large: true,
            position: Position::Pointer,
            ..Settings::default()
        };
        st.folders = vec![PathBuf::from("D:\\Apps"), PathBuf::from("/opt/apps")];
        st.set_mode(Path::new("/opt/apps"), FolderMode::One);
        st.set_mode(Path::new("D:\\Apps"), FolderMode::Auto);
        assert_eq!(st.modes.len(), 1);
        assert_eq!(Settings::load(&st.to_store(None), None), st);
        assert_eq!(Settings::load(&Store::default(), None), Settings::default());
    }

    #[cfg(windows)]
    #[test]
    fn a_folder_on_the_pen_follows_its_letter() {
        let own = Path::new("F:\\rlaunch");
        assert_eq!(pack(Path::new("F:\\PortableApps"), Some(own)), "*:\\PortableApps");
        assert_eq!(pack(Path::new("C:\\Tools"), Some(own)), "C:\\Tools");
        assert_eq!(unpack("*:\\PortableApps", Some(Path::new("H:\\rlaunch"))), PathBuf::from("H:\\PortableApps"));
        // Not portable: kept as it is.
        assert_eq!(pack(Path::new("F:\\PortableApps"), None), "F:\\PortableApps");
    }
}
