//! Commands, their menu labels (translation keys) and keyboard shortcuts.

use crate::i18n::{tr, Lang};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    Open,
    Reload,
    OpenFolder,
    /// Closes the window (hides it in resident mode).
    Close,
    Exit,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    FitWidth,
    WordWrap,
    FullScreen,
    /// Dark HTML and PDF pages in the dark theme.
    DarkPages,
    /// 0 system, 1 light, 2 dark.
    Theme(u8),
    /// 0 follows Windows, 1.. index into `Lang::ALL` + 1.
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
            Open => "&Open...",
            Reload => "&Reload",
            OpenFolder => "Show in &Folder",
            Close => "&Close",
            Exit => "E&xit",
            ZoomIn => "Zoom &In",
            ZoomOut => "Zoom &Out",
            ZoomReset => "&Actual Size",
            FitWidth => "Fit &Width",
            WordWrap => "Word W&rap",
            FullScreen => "&Full Screen",
            DarkPages => "Dark &Pages in Dark Theme",
            Theme(0) => "Use &System Setting",
            Theme(1) => "&Light",
            Theme(_) => "&Dark",
            UiLanguage(0) => "Use &System Setting",
            UiLanguage(i) => return Err(Lang::ALL.get(i as usize - 1).map_or("", |l| l.native_name()).to_string()),
            Shortcuts => "&Keyboard Shortcuts",
            About => "&About rview",
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
    ("Ctrl+O", Open),
    ("F5", Reload),
    ("Ctrl+W", Close),
    ("Ctrl+=", ZoomIn),
    ("Ctrl++", ZoomIn),
    ("Ctrl+-", ZoomOut),
    ("Ctrl+0", ZoomReset),
    ("Ctrl+9", FitWidth),
    ("Alt+Z", WordWrap),
    ("F11", FullScreen),
    ("F1", Shortcuts),
];

pub fn shortcut(cmd: Command) -> Option<&'static str> {
    BINDINGS.iter().find(|(_, c)| *c == cmd).map(|(k, _)| *k)
}

/// Finds the command for a key chord such as `Ctrl+O`.
pub fn lookup(ctrl: bool, shift: bool, alt: bool, key: &str) -> Option<Command> {
    BINDINGS.iter().find(|(k, _)| {
        let mut parts: Vec<&str> = k.split('+').collect();
        let last = if k.ends_with("++") {
            parts.retain(|p| !p.is_empty());
            "+"
        } else {
            parts.pop().unwrap_or("")
        };
        let has = |m: &str| parts.contains(&m);
        has("Ctrl") == ctrl && has("Shift") == shift && has("Alt") == alt && last.eq_ignore_ascii_case(key)
    })
    .map(|(_, c)| *c)
}

pub enum M {
    Item(Command),
    Sep,
    Sub(&'static str, Vec<M>),
}

pub fn menu_tree() -> Vec<(&'static str, Vec<M>)> {
    use M::*;
    let mut langs = vec![Item(UiLanguage(0)), Sep];
    langs.extend((1..=Lang::ALL.len()).map(|i| Item(UiLanguage(i as u8))));
    vec![
        ("&File", vec![Item(Open), Item(Reload), Item(OpenFolder), Sep, Item(Close), Item(Exit)]),
        (
            "&View",
            vec![
                Item(ZoomIn),
                Item(ZoomOut),
                Item(ZoomReset),
                Item(FitWidth),
                Sep,
                Item(WordWrap),
                Item(FullScreen),
                Sep,
                Sub("T&heme", vec![Item(Theme(0)), Item(Theme(1)), Item(Theme(2))]),
                Item(DarkPages),
                Sub("Display &Language", langs),
            ],
        ),
        ("&Help", vec![Item(Shortcuts), Item(About)]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords() {
        assert_eq!(lookup(true, false, false, "o"), Some(Open));
        assert_eq!(lookup(true, false, false, "="), Some(ZoomIn));
        assert_eq!(lookup(true, false, false, "+"), Some(ZoomIn));
        assert_eq!(lookup(true, false, false, "-"), Some(ZoomOut));
        assert_eq!(lookup(false, false, false, "f5"), Some(Reload));
        assert_eq!(lookup(false, false, true, "z"), Some(WordWrap));
        assert_eq!(lookup(false, false, false, "o"), None);
    }
}
