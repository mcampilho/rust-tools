//! Commands of the window, their menu labels (translation keys) and keyboard
//! shortcuts. The commands of a page are the page's own (`Cmd::Clean(…)`
//! holds one of rclean's, `Cmd::Dups(…)` one of rdup's…), so a page needs no
//! change to sit here; its menus are the small app's, minus what is the
//! window's (exit, theme, language, help), which this window has once.

use crate::i18n::{tr, Lang};
use crate::pages::{Kind, PAGES};
use rclean::commands::Command as CleanCmd;
use rdup::commands::Command as DupCmd;
use rinst::commands::Command as InstCmd;
use rsize::commands::Command as SizeCmd;
use rt_core::menu::{MenuCommand, M as Item};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cmd {
    /// A command of the cleaning page.
    Clean(CleanCmd),
    /// A command of the duplicates page.
    Dups(DupCmd),
    /// A command of the disk space page.
    Size(SizeCmd),
    /// A command of the uninstaller page.
    Inst(InstCmd),
    /// System Maintenance: every check, in one go.
    MaintScan,
    /// Speed Up: a button or menu entry of its own.
    Speed(crate::speed_page::Action),
    /// Startup Optimizer: a button of its bar.
    Startup(crate::startup_page::Action),
    /// Program Deactivator: a button of its bar.
    Services(crate::services_page::Action),
    /// Extensions: a button of its bar.
    Extensions(crate::extensions_page::Action),
    /// Registry Cleaner: a button of its bar.
    Registry(crate::registry_page::Action),
    /// Protect System: a button of its bar.
    Protect(crate::protect_page::Action),
    /// Internet Booster: a button of its page.
    Net(crate::net_page::Action),
    /// Privacy Sweep: a button of its bar.
    Privacy(crate::privacy_page::Action),
    /// System Information: a button of its bar.
    Sys(crate::sysinfo_page::Action),
    /// Repair: a button of its bar.
    Repair(crate::repair_page::Action),
    /// Scheduler: a button of its bar.
    Schedule(crate::schedule_page::Action),
    /// Processes: a button of its page.
    Procs(crate::processes_page::Action),
    /// Context Menu Manager: a button of its page.
    CtxMenu(crate::ctxmenu_page::Action),
    /// File Tools: a button of its page.
    Files(crate::files_page::Action),
    /// Custom Clean: the junk, duplicates and large files analysed together.
    CustomAnalyze,
    /// Custom Clean: the folder where duplicates and large files are looked for.
    CustomFolder,
    /// Shows the sidebar's names, or only its icons.
    Sidebar,
    /// Starts rcare again with administrator rights.
    RestartAdmin,
    /// "Analyze with rcare" on the Explorer menu of folders and drives (on or off).
    ExplorerMenu,
    /// Starts rcare resident with the session (`--tray`).
    Autostart,
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

/// Which of a page's commands are the window's, as a function of the page's command type.
macro_rules! window_level {
    ($name:ident, $t:ty) => {
        fn $name(c: $t) -> bool {
            type C = $t;
            #[cfg(windows)]
            let windows_only = matches!(c, C::RestartAdmin | C::ExplorerMenu);
            #[cfg(not(windows))]
            let windows_only = false;
            windows_only || matches!(c, C::Exit | C::Autostart | C::Theme(_) | C::UiLanguage(_) | C::Shortcuts | C::CheckUpdates | C::About)
        }
    };
}
window_level!(clean_is_window, CleanCmd);
window_level!(dups_is_window, DupCmd);
window_level!(size_is_window, SizeCmd);
window_level!(inst_is_window, InstCmd);

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
            CleanCmd::About => About,
            CleanCmd::Theme(t) => Theme(t),
            CleanCmd::UiLanguage(i) => UiLanguage(i),
            c => Clean(c),
        }
    }

    pub fn from_dups(c: DupCmd) -> Cmd {
        match c {
            DupCmd::Exit => Exit,
            #[cfg(windows)]
            DupCmd::RestartAdmin => RestartAdmin,
            DupCmd::Shortcuts => Shortcuts,
            DupCmd::CheckUpdates => CheckUpdates,
            DupCmd::About => About,
            DupCmd::Theme(t) => Theme(t),
            DupCmd::UiLanguage(i) => UiLanguage(i),
            c => Dups(c),
        }
    }

    pub fn from_size(c: SizeCmd) -> Cmd {
        match c {
            SizeCmd::Exit => Exit,
            #[cfg(windows)]
            SizeCmd::RestartAdmin => RestartAdmin,
            SizeCmd::Shortcuts => Shortcuts,
            SizeCmd::CheckUpdates => CheckUpdates,
            SizeCmd::About => About,
            SizeCmd::Theme(t) => Theme(t),
            SizeCmd::UiLanguage(i) => UiLanguage(i),
            c => Size(c),
        }
    }

    pub fn from_inst(c: InstCmd) -> Cmd {
        match c {
            InstCmd::Exit => Exit,
            #[cfg(windows)]
            InstCmd::RestartAdmin => RestartAdmin,
            InstCmd::Shortcuts => Shortcuts,
            InstCmd::CheckUpdates => CheckUpdates,
            InstCmd::About => About,
            InstCmd::Theme(t) => Theme(t),
            InstCmd::UiLanguage(i) => UiLanguage(i),
            c => Inst(c),
        }
    }

    /// English label with `&` access key (the translation key), or the final
    /// text for labels that are not translated.
    pub fn label_key(self) -> Result<&'static str, String> {
        Ok(match self {
            Clean(c) => return c.label_key(),
            Dups(c) => return c.label_key(),
            Size(c) => return c.label_key(),
            Inst(c) => return c.label_key(),
            MaintScan => "&Scan",
            Startup(a) => a.label_key(),
            Speed(a) => a.label_key(),
            Services(a) => a.label_key(),
            Extensions(a) => a.label_key(),
            Registry(a) => a.label_key(),
            Protect(a) => a.label_key(),
            Net(a) => a.label_key(),
            Privacy(a) => a.label_key(),
            Sys(a) => a.label_key(),
            Repair(a) => a.label_key(),
            Schedule(a) => a.label_key(),
            Procs(a) => a.label_key(),
            CtxMenu(a) => a.label_key(),
            Files(a) => a.label_key(),
            CustomAnalyze => "&Analyze",
            CustomFolder => "Change &Folder...",
            Sidebar => "Collapse Side&bar",
            RestartAdmin => "Restart as Ad&ministrator",
            ExplorerMenu => "&Explorer Context Menu",
            Autostart => "Start wit&h Windows",
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
        // A page's labels are translated by the page: its own table first.
        match self {
            Clean(c) => return c.label(),
            Dups(c) => return c.label(),
            Size(c) => return c.label(),
            Inst(c) => return c.label(),
            _ => {}
        }
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
        Dups(c) => rdup::commands::shortcut(c),
        Size(c) => rsize::commands::shortcut(c),
        Inst(c) => rinst::commands::shortcut(c),
        // F5 belongs to the page shown; on Custom Clean it analyses.
        CustomAnalyze
        | MaintScan
        | Startup(crate::startup_page::Action::Refresh)
        | Speed(crate::speed_page::Action::Refresh)
        | Services(crate::services_page::Action::Refresh)
        | Extensions(crate::extensions_page::Action::Refresh)
        | Registry(crate::registry_page::Action::Scan)
        | Protect(crate::protect_page::Action::Refresh)
        | Net(crate::net_page::Action::Refresh)
        | Privacy(crate::privacy_page::Action::Scan)
        | Sys(crate::sysinfo_page::Action::Refresh)
        | Schedule(crate::schedule_page::Action::Refresh) => Some("F5"),
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

impl MenuCommand for Cmd {
    fn label(self) -> String {
        Cmd::label(self)
    }
    fn shortcut(self) -> Option<&'static str> {
        shortcut(self)
    }
}

/// What a small app's menus give the window: what goes in File, in View, and
/// the other menus (Edit, Mark…) with their titles.
struct Own {
    file: Vec<M>,
    view: Vec<M>,
    rest: Vec<(&'static str, Vec<M>)>,
}

/// Takes a small app's menu tree: leaves out what is the window's, turns the
/// rest into `Cmd`, and sorts it into `Own`.
fn own_menus<C: Copy>(tree: Vec<(&'static str, Vec<rt_core::menu::M<C>>)>, is_window: fn(C) -> bool, wrap: fn(C) -> Cmd) -> Own {
    fn keep<C: Copy>(items: Vec<rt_core::menu::M<C>>, is_window: fn(C) -> bool, wrap: fn(C) -> Cmd) -> Vec<M> {
        let mut out: Vec<M> = Vec::new();
        for it in items {
            match it {
                Item::Item(c) if !is_window(c) => out.push(Item::Item(wrap(c))),
                Item::Dyn(c, s) if !is_window(c) => out.push(Item::Dyn(wrap(c), s)),
                Item::Item(_) | Item::Dyn(..) => {}
                Item::Sep => out.push(Item::Sep),
                Item::Sub(t, children) => {
                    let inner = keep(children, is_window, wrap);
                    if inner.iter().any(|m| !matches!(m, Item::Sep)) {
                        out.push(Item::Sub(t, inner));
                    }
                }
            }
        }
        // No separator first, last or twice in a row.
        let mut clean: Vec<M> = Vec::new();
        for m in out {
            if matches!(m, Item::Sep) && matches!(clean.last(), None | Some(Item::Sep)) {
                continue;
            }
            clean.push(m);
        }
        while matches!(clean.last(), Some(Item::Sep)) {
            clean.pop();
        }
        clean
    }
    let mut own = Own { file: Vec::new(), view: Vec::new(), rest: Vec::new() };
    for (title, items) in tree {
        let items = keep(items, is_window, wrap);
        match title {
            "&File" => own.file = items,
            "&View" => own.view = items,
            "&Help" => {}
            _ if items.is_empty() => {}
            t => own.rest.push((t, items)),
        }
    }
    own
}

fn page_menus(page: usize) -> Own {
    match PAGES.get(page).map(|p| p.kind) {
        Some(Kind::Clean) => own_menus(rclean::commands::menu_tree(), clean_is_window, Cmd::from_clean),
        Some(Kind::Dups) => own_menus(rdup::commands::menu_tree(), dups_is_window, Cmd::from_dups),
        Some(Kind::Size) => own_menus(rsize::commands::menu_tree(), size_is_window, Cmd::from_size),
        Some(Kind::Speed) => {
            use crate::speed_page::Action as A;
            Own {
                file: vec![Item::Item(Speed(A::Refresh))],
                view: vec![Item::Item(Speed(A::GameMode))],
                rest: vec![("&Edit", vec![Item::Item(Speed(A::CloseTicked)), Item::Sep, Item::Item(Speed(A::FreeStandby))])],
            }
        }
        Some(Kind::Startup) => {
            use crate::startup_page::Action as A;
            Own {
                file: vec![Item::Item(Startup(A::Refresh))],
                view: Vec::new(),
                rest: vec![(
                    "&Edit",
                    vec![Item::Item(Startup(A::Off)), Item::Item(Startup(A::On)), Item::Sep, Item::Item(Startup(A::Open))],
                )],
            }
        }
        Some(Kind::Services) => {
            use crate::services_page::Action as A;
            Own {
                file: vec![Item::Item(Services(A::Refresh))],
                view: Vec::new(),
                rest: vec![(
                    "&Edit",
                    vec![Item::Item(Services(A::Off)), Item::Item(Services(A::On)), Item::Sep, Item::Item(Services(A::Open))],
                )],
            }
        }
        Some(Kind::Extensions) => {
            use crate::extensions_page::Action as A;
            Own {
                file: vec![Item::Item(Extensions(A::Refresh))],
                view: Vec::new(),
                rest: vec![("&Edit", vec![Item::Item(Extensions(A::OpenBrowser)), Item::Item(Extensions(A::OpenFolder))])],
            }
        }
        Some(Kind::Registry) => {
            use crate::registry_page::Action as A;
            Own {
                file: vec![Item::Item(Registry(A::Scan))],
                view: vec![Item::Item(Registry(A::Size))],
                rest: vec![("&Edit", vec![Item::Item(Registry(A::Clean)), Item::Sep, Item::Item(Registry(A::Copies))])],
            }
        }
        Some(Kind::Protect) => {
            use crate::protect_page::Action as A;
            Own {
                file: vec![Item::Item(Protect(A::Refresh))],
                view: vec![Item::Item(Protect(A::Security)), Item::Item(Protect(A::Folders))],
                rest: vec![(
                    "&Edit",
                    vec![
                        Item::Item(Protect(A::QuickScan)),
                        Item::Item(Protect(A::UpdateDefinitions)),
                        Item::Sep,
                        Item::Item(Protect(A::TurnOn)),
                    ],
                )],
            }
        }
        Some(Kind::Net) => {
            use crate::net::Provider as P;
            use crate::net_page::Action as A;
            let mut edit = vec![Item::Item(Net(A::RestoreTcp)), Item::Sep];
            edit.extend(P::ALL.iter().map(|p| Item::Item(Net(A::Dns(*p)))));
            edit.extend([Item::Sep, Item::Item(Net(A::Copies))]);
            Own { file: vec![Item::Item(Net(A::Refresh))], view: Vec::new(), rest: vec![("&Edit", edit)] }
        }
        Some(Kind::Privacy) => {
            use crate::privacy_page::Action as A;
            Own {
                file: vec![Item::Item(Privacy(A::Scan))],
                view: Vec::new(),
                rest: vec![("&Edit", vec![Item::Item(Privacy(A::Sweep)), Item::Sep, Item::Item(Privacy(A::Copies))])],
            }
        }
        Some(Kind::Sys) => {
            use crate::sysinfo_page::Action as A;
            Own {
                file: vec![Item::Item(Sys(A::Refresh))],
                view: Vec::new(),
                rest: vec![(
                    "&Edit",
                    vec![
                        Item::Item(Sys(A::Copy)),
                        Item::Sep,
                        Item::Item(Sys(A::FastStartup)),
                        Item::Sep,
                        Item::Item(Sys(A::DefaultApps)),
                        Item::Item(Sys(A::Drivers)),
                    ],
                )],
            }
        }
        Some(Kind::Repair) => {
            use crate::repair_page::Action as A;
            Own {
                file: Vec::new(),
                view: Vec::new(),
                rest: vec![(
                    "&Edit",
                    vec![
                        Item::Item(Repair(A::Run)),
                        Item::Item(Repair(A::Fix)),
                        Item::Sep,
                        Item::Item(Repair(A::Copy)),
                        Item::Item(Repair(A::Log)),
                    ],
                )],
            }
        }
        Some(Kind::Schedule) => {
            use crate::schedule_page::Action as A;
            Own {
                file: vec![Item::Item(Schedule(A::Refresh))],
                view: Vec::new(),
                rest: vec![(
                    "&Edit",
                    vec![
                        Item::Item(Schedule(A::Create)),
                        Item::Item(Schedule(A::RunNow)),
                        Item::Item(Schedule(A::Delete)),
                        Item::Sep,
                        Item::Item(Schedule(A::Log)),
                    ],
                )],
            }
        }
        Some(Kind::Files) => {
            use crate::files_page::Action as A;
            Own {
                file: Vec::new(),
                view: Vec::new(),
                rest: vec![(
                    "&Edit",
                    vec![
                        Item::Item(Files(A::ChooseFolder)),
                        Item::Item(Files(A::Scan)),
                        Item::Item(Files(A::Remove)),
                        Item::Sep,
                        Item::Item(Files(A::AddFiles)),
                        Item::Item(Files(A::AddFolder)),
                        Item::Item(Files(A::Clear)),
                        Item::Item(Files(A::Shred)),
                    ],
                )],
            }
        }
        Some(Kind::CtxMenu) => {
            use crate::ctxmenu_page::Action as A;
            Own {
                file: vec![Item::Item(CtxMenu(A::Refresh))],
                view: Vec::new(),
                rest: vec![(
                    "&Edit",
                    vec![Item::Item(CtxMenu(A::Off)), Item::Item(CtxMenu(A::On)), Item::Sep, Item::Item(CtxMenu(A::Open))],
                )],
            }
        }
        Some(Kind::Procs) => {
            use crate::processes_page::Action as A;
            Own {
                file: Vec::new(),
                view: Vec::new(),
                rest: vec![(
                    "&Edit",
                    vec![
                        Item::Item(Procs(A::End)),
                        Item::Item(Procs(A::Open)),
                        Item::Sep,
                        Item::Item(Procs(A::Start)),
                        Item::Item(Procs(A::CancelPlan)),
                    ],
                )],
            }
        }
        Some(Kind::Maint) => Own { file: vec![Item::Item(MaintScan)], view: Vec::new(), rest: Vec::new() },
        Some(Kind::Custom) => Own { file: vec![Item::Item(CustomAnalyze), Item::Item(CustomFolder)], view: Vec::new(), rest: Vec::new() },
        Some(Kind::Inst) => own_menus(rinst::commands::menu_tree(), inst_is_window, Cmd::from_inst),
        None => Own { file: Vec::new(), view: Vec::new(), rest: Vec::new() },
    }
}

/// The menu bar of the page `page`: File (the page's entries, then the
/// window's), the page's other menus, View (the page's entries, then the
/// sidebar, the theme and the language), Help.
pub fn menu_tree(page: usize) -> Vec<(&'static str, Vec<M>)> {
    use rt_core::menu::M::*;
    let own = page_menus(page);
    let mut langs = vec![Item(UiLanguage(0)), Sep];
    langs.extend((1..=Lang::ALL.len()).map(|i| Item(UiLanguage(i as u8))));
    let mut file = own.file;
    if !file.is_empty() {
        file.push(Sep);
    }
    file.extend([Item(RestartAdmin), Sub("Windows &Integration", vec![Item(ExplorerMenu), Sep, Item(Autostart)]), Sep, Item(Exit)]);
    let mut view = own.view;
    if !view.is_empty() {
        view.push(Sep);
    }
    view.extend([Item(Sidebar), Sep, Sub("T&heme", vec![Item(Theme(0)), Item(Theme(1)), Item(Theme(2))]), Sub("Display &Language", langs)]);
    let mut bar = vec![("&File", file)];
    bar.extend(own.rest);
    bar.push(("&View", view));
    bar.push(("&Help", vec![Item(Shortcuts), Item(CheckUpdates), Sep, Item(About)]));
    bar
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
        assert_eq!(Cmd::from_dups(DupCmd::About), About);
        assert_eq!(Cmd::from_dups(DupCmd::Search), Dups(DupCmd::Search));
        assert_eq!(Cmd::from_size(SizeCmd::Theme(2)), Theme(2));
        assert_eq!(Cmd::from_size(SizeCmd::Rescan), Size(SizeCmd::Rescan));
    }

    #[test]
    fn every_command_in_the_menus_is_unique() {
        for page in 0..PAGES.len() {
            let mut seen = Vec::new();
            for (_, items) in menu_tree(page) {
                M::each_command(&items, &mut |c| seen.push(c));
            }
            let unique: std::collections::HashSet<_> = seen.iter().collect();
            assert_eq!(unique.len(), seen.len(), "page {page}");
        }
    }

    /// The page's own commands are in its menus, and the window's only once.
    #[test]
    fn the_windows_commands_are_not_repeated() {
        for page in 0..PAGES.len() {
            let mut window = Vec::new();
            for (_, items) in menu_tree(page) {
                M::each_command(&items, &mut |c| {
                    if !matches!(c, Clean(_) | Dups(_) | Size(_) | Inst(_)) {
                        window.push(c);
                    }
                });
            }
            assert!(window.contains(&Exit) && window.contains(&About) && window.contains(&Sidebar));
            assert_eq!(window.iter().filter(|c| **c == Autostart).count(), 1);
            assert_eq!(window.iter().filter(|c| **c == Exit).count(), 1);
        }
        // A page's own entries made it: the cleaner's Analyze, the duplicates' search, the scan's rescan.
        let all = |page: usize| {
            let mut v = Vec::new();
            for (_, items) in menu_tree(page) {
                M::each_command(&items, &mut |c| v.push(c));
            }
            v
        };
        let at = |id: &str| crate::pages::index_of(id);
        assert!(all(at("custom")).contains(&CustomAnalyze));
        assert!(all(at("maint")).contains(&MaintScan));
        assert!(all(at("startup")).contains(&Startup(crate::startup_page::Action::Off)));
        assert!(all(at("services")).contains(&Services(crate::services_page::Action::Off)));
        assert!(all(at("extensions")).contains(&Extensions(crate::extensions_page::Action::OpenBrowser)));
        assert!(all(at("registry")).contains(&Registry(crate::registry_page::Action::Clean)));
        assert!(all(at("protect")).contains(&Protect(crate::protect_page::Action::QuickScan)));
        assert!(all(at("net")).contains(&Net(crate::net_page::Action::Dns(crate::net::Provider::Quad9))));
        assert!(all(at("privacy")).contains(&Privacy(crate::privacy_page::Action::Sweep)));
        assert!(all(at("sysinfo")).contains(&Sys(crate::sysinfo_page::Action::FastStartup)));
        assert!(all(at("repair")).contains(&Repair(crate::repair_page::Action::Run)));
        assert!(all(at("schedule")).contains(&Schedule(crate::schedule_page::Action::Create)));
        assert!(all(at("speed")).contains(&Speed(crate::speed_page::Action::GameMode)));
        assert!(all(at("clean")).contains(&Clean(CleanCmd::Analyse)));
        assert!(all(at("dups")).contains(&Dups(DupCmd::Search)));
        assert!(all(at("size")).contains(&Size(SizeCmd::Rescan)));
        assert!(all(at("inst")).contains(&Inst(InstCmd::Uninstall)));
    }
}
