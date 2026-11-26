//! What the user chose, as kept in `settings.ini`.

use crate::i18n::Lang;
use crate::items::Items;
use crate::storage::Store;
use std::path::{Component, Path, PathBuf};

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
    pub hotkey: Hotkey,
    /// The size of the menu, in percent of the compact one.
    pub size: u16,
    /// How much of what is behind shows through, in percent (Windows).
    pub transparency: u8,
    /// Ask for administrator rights at every start (Windows).
    pub always_admin: bool,
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
            hotkey: Hotkey::default(),
            size: 100,
            transparency: 0,
            always_admin: false,
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
            hotkey: s.get("hotkey").and_then(|v| Hotkey::ALL.into_iter().find(|h| h.key() == v)).unwrap_or_default(),
            size: s.get("size").and_then(|v| v.parse().ok()).filter(|v| SIZES.contains(v)).unwrap_or(d.size),
            transparency: s.get("transparency").and_then(|v| v.parse().ok()).filter(|v| TRANSPARENCIES.contains(v)).unwrap_or(0),
            always_admin: flag("always_admin", false),
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
        s.set("hotkey", self.hotkey.key());
        s.set("size", self.size.to_string());
        s.set("transparency", self.transparency.to_string());
        s.set("always_admin", bit(self.always_admin));
        self.items.save(&mut s, own);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            ..Settings::default()
        };
        st.folders = vec![PathBuf::from("D:\\Apps"), PathBuf::from("/opt/apps")];
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
