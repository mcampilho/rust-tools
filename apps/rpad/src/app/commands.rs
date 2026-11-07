//! Every user command, with its label, keyboard shortcut and menu placement.
//! Menus, the command palette and keyboard handling are all driven from
//! this table, so they stay consistent.

use crate::io::encoding::Enc;
use crate::syntax;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(clippy::enum_variant_names)]
pub enum Command {
    // File
    New,
    Open,
    OpenRecent(u8),
    ClearRecent,
    ReopenClosed,
    Save,
    SaveAs,
    SaveAll,
    Reload,
    ReopenWithEncoding(u8),
    LocalHistory,
    CloseTab,
    CloseOthers,
    CloseAll,
    CopyPath,
    RevealInExplorer,
    Exit,
    // Edit
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
    Duplicate,
    DeleteLine,
    MoveLineUp,
    MoveLineDown,
    JoinLines,
    Indent,
    Outdent,
    ToggleComment,
    CaretAbove,
    CaretBelow,
    AddNextOccurrence,
    SelectAllOccurrences,
    SplitSelectionIntoLines,
    InsertDateTime,
    InsertSnippet,
    SaveSnippet,
    EditSnippets,
    ToggleOverwrite,
    // Search
    Find,
    Replace,
    FindNext,
    FindPrev,
    FindInDocs,
    FindInFiles,
    GotoLine,
    GotoBracket,
    GotoSymbol,
    // View
    WordWrap,
    LineNumbers,
    HighlightLine,
    ShowWhitespace,
    ShowEol,
    ShowRuler,
    FoldMargin,
    FoldCurrent,
    UnfoldCurrent,
    FoldAll,
    UnfoldAll,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Toolbar,
    StatusBar,
    Outline,
    MarkdownPreview,
    ResultsPanel,
    Theme(u8),
    /// Interface language: 0 follows Windows, 1.. index into `Lang::ALL` + 1.
    UiLanguage(u8),
    FullScreen,
    CommandPalette,
    NextTab,
    PrevTab,
    // Format
    LangAuto,
    SetLang(u8),
    SetEncoding(u8),
    SetEol(u8),
    UseSpaces,
    UseTabs,
    TabWidth(u8),
    IndentToSpaces,
    IndentToTabs,
    AutoIndent,
    Font,
    // Tools
    Upper,
    Lower,
    TitleCase,
    SentenceCase,
    InvertCase,
    SortAsc,
    SortDesc,
    SortCi,
    SortNatural,
    Reverse,
    Dedupe,
    RemoveBlank,
    TrimTrailing,
    TrimLeading,
    PrefixSuffix,
    TabsToSpaces,
    SpacesToTabs,
    JsonEscape,
    JsonUnescape,
    XmlEscape,
    XmlUnescape,
    UrlEncode,
    UrlDecode,
    B64Encode,
    B64Decode,
    XmlCheck,
    XmlFormat,
    OptRestoreSession,
    OptRecovery,
    OptAutosave,
    OptLocalHistory,
    OptSingleInstance,
    LastTabKeep,
    LastTabCloseWindow,
    OpenDataFolder,
    // Help
    Shortcuts,
    About,
}

use Command::*;

pub const EOLS: [(&str, crate::io::encoding::Eol); 3] = [
    ("Windows (CRLF)", crate::io::encoding::Eol::CrLf),
    ("Unix (LF)", crate::io::encoding::Eol::Lf),
    ("Classic Mac (CR)", crate::io::encoding::Eol::Cr),
];

impl Command {
    /// English menu label (the translation key), or the final text for
    /// labels built at run time.
    pub fn label_key(self) -> Result<&'static str, String> {
        Ok(match self {
            New => "&New",
            Open => "&Open...",
            OpenRecent(_) => "Recent file",
            ClearRecent => "&Clear Recent Files",
            ReopenClosed => "Reopen Closed &Tab",
            Save => "&Save",
            SaveAs => "Save &As...",
            SaveAll => "Sa&ve All",
            Reload => "Reload from &Disk",
            ReopenWithEncoding(i) => return Err(Enc::all().get(i as usize).map_or_else(String::new, |e| e.name())),
            LocalHistory => "Local H&istory...",
            CloseTab => "&Close",
            CloseOthers => "Close Ot&hers",
            CloseAll => "Close A&ll",
            CopyPath => "Copy Full &Path",
            RevealInExplorer => "Show in &Explorer",
            Exit => "E&xit",
            Undo => "&Undo",
            Redo => "&Redo",
            Cut => "Cu&t",
            Copy => "&Copy",
            Paste => "&Paste",
            Delete => "&Delete",
            SelectAll => "Select &All",
            Duplicate => "D&uplicate Line or Selection",
            DeleteLine => "Delete &Line",
            MoveLineUp => "Move Line U&p",
            MoveLineDown => "Move Line Do&wn",
            JoinLines => "&Join Lines",
            Indent => "&Indent",
            Outdent => "&Outdent",
            ToggleComment => "Toggle Line Co&mment",
            CaretAbove => "Add Caret &Above",
            CaretBelow => "Add Caret &Below",
            AddNextOccurrence => "Add &Next Occurrence",
            SelectAllOccurrences => "Select All &Occurrences",
            SplitSelectionIntoLines => "&Split Selection into Lines",
            InsertDateTime => "&Insert Date/Time",
            InsertSnippet => "&Insert Snippet...",
            SaveSnippet => "&Save Selection as Snippet...",
            EditSnippets => "&Edit Snippets File",
            ToggleOverwrite => "Toggle &Overwrite Mode",
            Find => "&Find...",
            Replace => "&Replace...",
            FindNext => "Find &Next",
            FindPrev => "Find &Previous",
            FindInDocs => "Find in Open &Documents...",
            FindInFiles => "Find in F&iles...",
            GotoLine => "&Go to Line...",
            GotoBracket => "Go to Matching &Bracket",
            GotoSymbol => "Go to &Symbol...",
            WordWrap => "&Word Wrap",
            LineNumbers => "&Line Numbers",
            HighlightLine => "H&ighlight Current Line",
            ShowWhitespace => "Show White&space",
            ShowEol => "Show Line &Endings",
            ShowRuler => "Show R&uler",
            FoldMargin => "Fold Mar&gin",
            FoldCurrent => "&Fold Current Block",
            UnfoldCurrent => "Unf&old Current Block",
            FoldAll => "Fold &All",
            UnfoldAll => "U&nfold All",
            ZoomIn => "Zoom &In",
            ZoomOut => "Zoom &Out",
            ZoomReset => "Reset &Zoom",
            Toolbar => "&Toolbar",
            StatusBar => "Status &Bar",
            Outline => "Outline &Panel",
            MarkdownPreview => "&Markdown Preview",
            ResultsPanel => "Search &Results Panel",
            Theme(0) => "Use &System Setting",
            Theme(1) => "&Light",
            Theme(_) => "&Dark",
            UiLanguage(0) => "Use &System Setting",
            UiLanguage(i) => return Err(crate::i18n::Lang::ALL.get(i as usize - 1).map_or("", |l| l.native_name()).to_string()),
            FullScreen => "&Full Screen",
            CommandPalette => "Comma&nd Palette...",
            NextTab => "Next Tab",
            PrevTab => "Previous Tab",
            LangAuto => "&Detect Automatically",
            SetLang(i) => return Err(crate::i18n::tr(syntax::lang::DEFS.get(i as usize).map_or("", |d| d.name))),
            SetEncoding(i) => return Err(Enc::all().get(i as usize).map_or_else(String::new, |e| e.name())),
            SetEol(i) => return Err(crate::i18n::tr(EOLS.get(i as usize).map_or("", |e| e.0))),
            UseSpaces => "Indent with &Spaces",
            UseTabs => "Indent with &Tabs",
            TabWidth(w) => return Err(crate::i18n::tf("Tab Width: &{}", &[&w.to_string()])),
            IndentToSpaces => "Convert Indentation to Spaces",
            IndentToTabs => "Convert Indentation to Tabs",
            AutoIndent => "&Auto Indent",
            Font => "&Font...",
            Upper => "&UPPERCASE",
            Lower => "&lowercase",
            TitleCase => "&Title Case",
            SentenceCase => "&Sentence case",
            InvertCase => "&iNVERT cASE",
            SortAsc => "Sort &Ascending",
            SortDesc => "Sort &Descending",
            SortCi => "Sort Ignoring &Case",
            SortNatural => "Sort &Natural (numbers by value)",
            Reverse => "&Reverse Lines",
            Dedupe => "Remove D&uplicate Lines",
            RemoveBlank => "Remove &Blank Lines",
            TrimTrailing => "Trim &Trailing Whitespace",
            TrimLeading => "Trim &Leading Whitespace",
            PrefixSuffix => "Add &Prefix/Suffix to Lines...",
            TabsToSpaces => "Convert &Tabs to Spaces",
            SpacesToTabs => "Convert Leading &Spaces to Tabs",
            JsonEscape => "&JSON Escape",
            JsonUnescape => "JSON Unescape",
            XmlEscape => "&XML/HTML Escape",
            XmlUnescape => "XML/HTML Unescape",
            UrlEncode => "&URL Encode",
            UrlDecode => "URL Decode",
            B64Encode => "&Base64 Encode",
            B64Decode => "Base64 Decode",
            XmlCheck => "&Check Well-Formedness",
            XmlFormat => "&Format (Pretty Print)",
            OptRestoreSession => "Restore &Session at Startup",
            OptRecovery => "Keep &Unsaved Changes (Recovery)",
            OptAutosave => "&Autosave Files When Window Loses Focus",
            OptLocalHistory => "Keep Local &History Snapshots on Save",
            OptSingleInstance => "Open Files in the &Existing Window",
            LastTabKeep => "Keep an &Empty Document",
            LastTabCloseWindow => "&Close the Window",
            OpenDataFolder => "Open Settings &Folder",
            Shortcuts => "&Keyboard Shortcuts",
            About => "&About rpad",
        })
    }

    pub fn label(self) -> String {
        match self.label_key() {
            Ok(k) => crate::i18n::tr(k),
            Err(s) => s,
        }
    }

    /// Label without menu mnemonics (palette, tooltips).
    pub fn plain_label(self) -> String {
        crate::i18n::strip_mnemonic(&self.label())
    }

    /// Commands listed in the command palette (with a menu category).
    pub fn palette() -> Vec<(Command, &'static str)> {
        let mut v: Vec<(Command, &'static str)> = Vec::new();
        for (cat, items) in [
            ("File", vec![New, Open, ReopenClosed, Save, SaveAs, SaveAll, Reload, LocalHistory, CloseTab, CloseOthers, CloseAll, CopyPath, RevealInExplorer, ClearRecent]),
            (
                "Edit",
                vec![
                    Undo, Redo, Cut, Copy, Paste, SelectAll, Duplicate, DeleteLine, MoveLineUp, MoveLineDown, JoinLines, Indent, Outdent,
                    ToggleComment, CaretAbove, CaretBelow, AddNextOccurrence, SelectAllOccurrences, SplitSelectionIntoLines, InsertDateTime,
                    InsertSnippet, SaveSnippet, EditSnippets, ToggleOverwrite,
                ],
            ),
            ("Search", vec![Find, Replace, FindNext, FindPrev, FindInDocs, FindInFiles, GotoLine, GotoBracket, GotoSymbol]),
            (
                "View",
                vec![
                    WordWrap, LineNumbers, HighlightLine, ShowWhitespace, ShowEol, ShowRuler, FoldMargin, FoldCurrent, UnfoldCurrent, FoldAll,
                    UnfoldAll, ZoomIn, ZoomOut, ZoomReset, Toolbar, StatusBar, Outline, MarkdownPreview, ResultsPanel, FullScreen,
                ],
            ),
            ("Format", vec![LangAuto, UseSpaces, UseTabs, IndentToSpaces, IndentToTabs, AutoIndent, Font]),
            (
                "Tools",
                vec![
                    Upper, Lower, TitleCase, SentenceCase, InvertCase, SortAsc, SortDesc, SortCi, SortNatural, Reverse, Dedupe, RemoveBlank,
                    TrimTrailing, TrimLeading, PrefixSuffix, TabsToSpaces, SpacesToTabs, JsonEscape, JsonUnescape, XmlEscape, XmlUnescape,
                    UrlEncode, UrlDecode, B64Encode, B64Decode, XmlCheck, XmlFormat, OpenDataFolder,
                ],
            ),
            ("Help", vec![Shortcuts, About]),
        ] {
            v.extend(items.into_iter().map(|c| (c, cat)));
        }
        for i in 0..syntax::lang::DEFS.len() {
            v.push((SetLang(i as u8), "Language"));
        }
        for i in 0..Enc::all().len() {
            v.push((SetEncoding(i as u8), "Encoding"));
        }
        for i in 0..EOLS.len() {
            v.push((SetEol(i as u8), "Line Endings"));
        }
        for i in 0..3 {
            v.push((Theme(i), "Theme"));
        }
        for i in 0..=crate::i18n::Lang::ALL.len() {
            v.push((UiLanguage(i as u8), "Display Language"));
        }
        v
    }
}

/// A key combination. `key` is a lowercase character or a named key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: String,
}

impl Chord {
    pub fn parse(s: &str) -> Chord {
        let mut c = Chord { ctrl: false, shift: false, alt: false, key: String::new() };
        for part in s.split('+') {
            match part {
                "Ctrl" => c.ctrl = true,
                "Shift" => c.shift = true,
                "Alt" => c.alt = true,
                k => c.key = k.to_lowercase(),
            }
        }
        if s.ends_with("++") || s == "+" {
            c.key = "+".into();
        }
        c
    }
}

/// Default key bindings (shown in menus as typed here).
pub const BINDINGS: &[(&str, Command)] = &[
    ("Ctrl+N", New),
    ("Ctrl+O", Open),
    ("Ctrl+Shift+T", ReopenClosed),
    ("Ctrl+S", Save),
    ("Ctrl+Shift+S", SaveAs),
    ("Ctrl+Alt+S", SaveAll),
    ("Ctrl+W", CloseTab),
    ("Ctrl+F4", CloseTab),
    ("Ctrl+Z", Undo),
    ("Ctrl+Y", Redo),
    ("Ctrl+Shift+Z", Redo),
    ("Ctrl+X", Cut),
    ("Ctrl+C", Copy),
    ("Ctrl+V", Paste),
    ("Shift+Delete", Cut),
    ("Ctrl+Insert", Copy),
    ("Shift+Insert", Paste),
    ("Ctrl+A", SelectAll),
    ("Ctrl+D", Duplicate),
    ("Ctrl+Shift+K", DeleteLine),
    ("Alt+Up", MoveLineUp),
    ("Alt+Down", MoveLineDown),
    ("Ctrl+Shift+Up", MoveLineUp),
    ("Ctrl+Shift+Down", MoveLineDown),
    ("Ctrl+J", JoinLines),
    ("Ctrl+]", Indent),
    ("Ctrl+[", Outdent),
    ("Ctrl+/", ToggleComment),
    ("Ctrl+Alt+Up", CaretAbove),
    ("Ctrl+Alt+Down", CaretBelow),
    ("Ctrl+Alt+D", AddNextOccurrence),
    ("Ctrl+Shift+L", SelectAllOccurrences),
    ("Alt+Shift+I", SplitSelectionIntoLines),
    ("F5", InsertDateTime),
    ("Ctrl+Shift+J", InsertSnippet),
    ("Insert", ToggleOverwrite),
    ("Ctrl+F", Find),
    ("Ctrl+H", Replace),
    ("F3", FindNext),
    ("Shift+F3", FindPrev),
    ("Ctrl+Shift+F", FindInFiles),
    ("Ctrl+G", GotoLine),
    ("Ctrl+B", GotoBracket),
    ("Ctrl+Shift+O", GotoSymbol),
    ("Alt+Z", WordWrap),
    ("Ctrl+Shift+[", FoldCurrent),
    ("Ctrl+Shift+]", UnfoldCurrent),
    ("Alt+0", FoldAll),
    ("Alt+Shift+0", UnfoldAll),
    ("Ctrl+=", ZoomIn),
    ("Ctrl++", ZoomIn),
    ("Ctrl+-", ZoomOut),
    ("Ctrl+0", ZoomReset),
    ("Ctrl+Shift+M", MarkdownPreview),
    ("F11", FullScreen),
    ("Ctrl+Shift+P", CommandPalette),
    ("Ctrl+Tab", NextTab),
    ("Ctrl+Shift+Tab", PrevTab),
    ("Ctrl+PageDown", NextTab),
    ("Ctrl+PageUp", PrevTab),
    ("Ctrl+Shift+U", Upper),
    ("Ctrl+U", Lower),
    ("F1", Shortcuts),
];

pub fn shortcut(cmd: Command) -> Option<&'static str> {
    BINDINGS.iter().find(|(_, c)| *c == cmd).map(|(k, _)| *k)
}

pub fn lookup(chord: &Chord) -> Option<Command> {
    BINDINGS.iter().find(|(k, _)| Chord::parse(k) == *chord).map(|(_, c)| *c)
}

/// Menu structure (`Dyn` items have a label chosen at run time: recent files).
pub type M = rt_core::menu::M<Command>;

impl rt_core::menu::MenuCommand for Command {
    fn label(self) -> String {
        Command::label(self)
    }
    fn shortcut(self) -> Option<&'static str> {
        shortcut(self)
    }
}

pub fn menu_tree(recent: &[std::path::PathBuf]) -> Vec<(&'static str, Vec<M>)> {
    use rt_core::menu::M::*;
    let mut recent_items: Vec<M> = recent
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let label = format!("&{} {}", (i + 1) % 10, p.display().to_string().replace('&', "&&"));
            Dyn(OpenRecent(i as u8), label)
        })
        .collect();
    if !recent_items.is_empty() {
        recent_items.push(Sep);
    }
    recent_items.push(Item(ClearRecent));
    let encodings = |f: fn(u8) -> Command| -> Vec<M> {
        let all = Enc::all();
        let mut v: Vec<M> = all.iter().take(4).enumerate().map(|(i, _)| Item(f(i as u8))).collect();
        v.push(Sep);
        v.extend(all.iter().enumerate().skip(4).map(|(i, _)| Item(f(i as u8))));
        v
    };
    let mut langs = vec![Item(LangAuto), Sep];
    langs.extend((0..syntax::lang::DEFS.len()).map(|i| Item(SetLang(i as u8))));
    vec![
        (
            "&File",
            vec![
                Item(New),
                Item(Open),
                Sub("Open &Recent", recent_items),
                Item(ReopenClosed),
                Sep,
                Item(Save),
                Item(SaveAs),
                Item(SaveAll),
                Item(Reload),
                Sub("Reopen &with Encoding", encodings(ReopenWithEncoding)),
                Item(LocalHistory),
                Sep,
                Item(CopyPath),
                Item(RevealInExplorer),
                Sep,
                Item(CloseTab),
                Item(CloseOthers),
                Item(CloseAll),
                Sep,
                Item(Exit),
            ],
        ),
        (
            "&Edit",
            vec![
                Item(Undo),
                Item(Redo),
                Sep,
                Item(Cut),
                Item(Copy),
                Item(Paste),
                Item(Delete),
                Item(SelectAll),
                Sep,
                Sub(
                    "&Line",
                    vec![Item(Duplicate), Item(DeleteLine), Item(MoveLineUp), Item(MoveLineDown), Item(JoinLines), Sep, Item(Indent), Item(Outdent), Item(ToggleComment)],
                ),
                Sub(
                    "&Multiple Carets",
                    vec![Item(CaretAbove), Item(CaretBelow), Item(AddNextOccurrence), Item(SelectAllOccurrences), Item(SplitSelectionIntoLines)],
                ),
                Sub("S&nippets", vec![Item(InsertSnippet), Item(SaveSnippet), Item(EditSnippets)]),
                Item(InsertDateTime),
                Item(ToggleOverwrite),
            ],
        ),
        (
            "&Search",
            vec![
                Item(Find),
                Item(Replace),
                Item(FindNext),
                Item(FindPrev),
                Sep,
                Item(FindInDocs),
                Item(FindInFiles),
                Sep,
                Item(GotoLine),
                Item(GotoBracket),
                Item(GotoSymbol),
            ],
        ),
        (
            "&View",
            vec![
                Item(WordWrap),
                Item(LineNumbers),
                Item(HighlightLine),
                Item(ShowWhitespace),
                Item(ShowEol),
                Item(ShowRuler),
                Item(FoldMargin),
                Sub("F&olding", vec![Item(FoldCurrent), Item(UnfoldCurrent), Item(FoldAll), Item(UnfoldAll)]),
                Sub("&Zoom", vec![Item(ZoomIn), Item(ZoomOut), Item(ZoomReset)]),
                Sep,
                Item(Toolbar),
                Item(StatusBar),
                Item(Outline),
                Item(MarkdownPreview),
                Item(ResultsPanel),
                Sub("T&heme", vec![Item(Theme(0)), Item(Theme(1)), Item(Theme(2))]),
                Sub("&Display Language", {
                    let mut v = vec![Item(UiLanguage(0)), Sep];
                    v.extend((1..=crate::i18n::Lang::ALL.len()).map(|i| Item(UiLanguage(i as u8))));
                    v
                }),
                Sep,
                Item(FullScreen),
                Item(CommandPalette),
            ],
        ),
        (
            "F&ormat",
            vec![
                Sub("&Language", langs),
                Sub("&Encoding", encodings(SetEncoding)),
                Sub("Line E&ndings", vec![Item(SetEol(0)), Item(SetEol(1)), Item(SetEol(2))]),
                Sub(
                    "&Indentation",
                    vec![Item(UseSpaces), Item(UseTabs), Sep, Item(TabWidth(2)), Item(TabWidth(4)), Item(TabWidth(8)), Sep, Item(IndentToSpaces), Item(IndentToTabs)],
                ),
                Item(AutoIndent),
                Sep,
                Item(Font),
            ],
        ),
        (
            "&Tools",
            vec![
                Sub("Ca&se", vec![Item(Upper), Item(Lower), Item(TitleCase), Item(SentenceCase), Item(InvertCase)]),
                Sub(
                    "&Lines",
                    vec![
                        Item(SortAsc),
                        Item(SortDesc),
                        Item(SortCi),
                        Item(SortNatural),
                        Item(Reverse),
                        Sep,
                        Item(Dedupe),
                        Item(RemoveBlank),
                        Item(TrimTrailing),
                        Item(TrimLeading),
                        Sep,
                        Item(PrefixSuffix),
                    ],
                ),
                Sub("&Whitespace", vec![Item(TabsToSpaces), Item(SpacesToTabs)]),
                Sub(
                    "&Encode/Decode",
                    vec![
                        Item(JsonEscape),
                        Item(JsonUnescape),
                        Item(XmlEscape),
                        Item(XmlUnescape),
                        Item(UrlEncode),
                        Item(UrlDecode),
                        Item(B64Encode),
                        Item(B64Decode),
                    ],
                ),
                Sub("&XML", vec![Item(XmlCheck), Item(XmlFormat)]),
                Sep,
                Sub(
                    "&Preferences",
                    vec![
                        Item(OptRestoreSession),
                        Item(OptRecovery),
                        Item(OptAutosave),
                        Item(OptLocalHistory),
                        Item(OptSingleInstance),
                        Sub("When the &Last Tab Closes", vec![Item(LastTabKeep), Item(LastTabCloseWindow)]),
                    ],
                ),
                Item(OpenDataFolder),
            ],
        ),
        ("&Help", vec![Item(Shortcuts), Item(About)]),
    ]
}

/// The editor's context menu.
pub fn editor_menu() -> Vec<M> {
    use rt_core::menu::M::*;
    vec![
        Item(Undo),
        Item(Redo),
        Sep,
        Item(Cut),
        Item(Copy),
        Item(Paste),
        Item(Delete),
        Sep,
        Item(SelectAll),
        Item(ToggleComment),
        Sub("Ca&se", vec![Item(Upper), Item(Lower), Item(TitleCase), Item(SentenceCase), Item(InvertCase)]),
        Sub("&Lines", vec![Item(SortAsc), Item(SortDesc), Item(Reverse), Item(Dedupe), Item(RemoveBlank), Item(TrimTrailing)]),
        Sep,
        Item(GotoBracket),
        Item(FoldCurrent),
        Item(UnfoldCurrent),
        Sep,
        Item(CommandPalette),
    ]
}

/// The context menu of a tab.
pub fn tab_menu() -> Vec<M> {
    use rt_core::menu::M::*;
    vec![Item(Save), Item(SaveAs), Item(Reload), Sep, Item(CopyPath), Item(RevealInExplorer), Sep, Item(CloseTab), Item(CloseOthers), Item(CloseAll), Sep, Item(ReopenClosed)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_and_lookup() {
        assert_eq!(lookup(&Chord::parse("Ctrl+S")), Some(Save));
        assert_eq!(lookup(&Chord::parse("Ctrl+Shift+S")), Some(SaveAs));
        assert_eq!(lookup(&Chord::parse("Ctrl++")), Some(ZoomIn));
        assert_eq!(lookup(&Chord::parse("Alt+Shift+0")), Some(UnfoldAll));
        assert_eq!(shortcut(Copy), Some("Ctrl+C"));
        // No chord is bound twice.
        let mut seen = std::collections::HashSet::new();
        for (k, _) in BINDINGS {
            assert!(seen.insert(Chord::parse(k)), "duplicate binding {k}");
        }
    }

    #[test]
    fn every_palette_command_has_a_label() {
        for (c, _) in Command::palette() {
            assert!(!c.plain_label().trim().is_empty(), "{c:?}");
        }
    }
}
