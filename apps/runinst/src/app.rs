//! The window's state without the window: the programs read, which are
//! shown (search, for whom, system components, updates), their order, the
//! one selected and the shortcuts of each.

use crate::inventory::{Inventory, Program, Scope, Source};
use crate::leftovers::Leftover;
use crate::shortcuts::{self, Shortcut};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Col {
    #[default]
    Name,
    Publisher,
    Version,
    Size,
    Installed,
    For,
    /// Where it was found (registry, Windows Installer, Store, winget...).
    Source,
}

impl Col {
    pub const ALL: [Col; 7] = [Col::Name, Col::Publisher, Col::Version, Col::Size, Col::Installed, Col::For, Col::Source];
}

/// The lists: every program, and each package manager's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Programs,
    Scoop,
    Chocolatey,
    /// What the programs uninstalled left behind.
    Leftovers,
}

/// The leftovers found after uninstalling, and which are ticked.
#[derive(Clone, Debug, Default)]
pub struct Leftovers {
    pub items: Vec<Leftover>,
    pub ticked: Vec<bool>,
    /// Still looking.
    pub searching: bool,
    /// Removing the ticked ones now.
    pub removing: bool,
    /// How removing each went (an error), when it was tried.
    pub failed: Vec<Option<String>>,
}

impl Leftovers {
    /// What was found, ticked as its group says (not what needs rights
    /// runinst does not have).
    pub fn new(items: Vec<Leftover>) -> Leftovers {
        let ticked = items.iter().map(|l| l.group.ticked_at_first() && !l.admin && !l.doubt).collect();
        let failed = vec![None; items.len()];
        Leftovers { items, ticked, searching: false, removing: false, failed }
    }

    pub fn chosen(&self) -> Vec<usize> {
        (0..self.items.len()).filter(|&i| self.ticked[i]).collect()
    }
}

/// Whose programs are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Whose {
    #[default]
    All,
    /// Installed for every user.
    Everyone,
    /// For the user running runinst only.
    Mine,
    /// For other users only.
    Others,
}

impl Whose {
    pub const ALL: [Whose; 4] = [Whose::All, Whose::Everyone, Whose::Mine, Whose::Others];

    fn takes(self, s: &Scope) -> bool {
        matches!(
            (self, s),
            (Whose::All, _)
                | (Whose::Everyone, Scope::Machine)
                | (Whose::Mine, Scope::User { current: true, .. })
                | (Whose::Others, Scope::User { current: false, .. })
        )
    }
}

/// What was read: the programs, the shortcuts, and which belong to which;
/// and the apps of Scoop and Chocolatey, when they are there.
#[derive(Clone, Debug, Default)]
pub struct Found {
    pub inv: Inventory,
    pub shortcuts: Vec<Shortcut>,
    /// For each program, the indexes of its shortcuts.
    pub links: Vec<Vec<usize>>,
    pub scoop: Option<Vec<Program>>,
    pub chocolatey: Option<Vec<Program>>,
    /// winget has answered (or is not there).
    pub winget_done: bool,
}

impl Found {
    pub fn new(inv: Inventory, shortcuts: Vec<Shortcut>) -> Found {
        let links = shortcuts::match_programs(&inv.programs, &shortcuts);
        Found { inv, shortcuts, links, ..Found::default() }
    }

    /// The list of a tab.
    pub fn list(&self, tab: Tab) -> &[Program] {
        match tab {
            Tab::Programs => &self.inv.programs,
            Tab::Scoop => self.scoop.as_deref().unwrap_or(&[]),
            Tab::Chocolatey => self.chocolatey.as_deref().unwrap_or(&[]),
            Tab::Leftovers => &[],
        }
    }
}

#[derive(Debug, Default)]
pub struct App {
    pub found: Option<Found>,
    pub tab: Tab,
    /// Reading the programs now.
    pub loading: bool,
    /// The programs shown, by index, in order.
    pub rows: Vec<usize>,
    /// The row selected.
    pub sel: Option<usize>,
    /// The programs selected (indexes into the tab's list), with Ctrl and
    /// Shift; the row of `sel` is always one of them. A Shift range starts at
    /// `anchor`.
    pub marked: std::collections::BTreeSet<usize>,
    pub anchor: Option<usize>,
    /// The programs whose box is ticked (indexes into the tab's list); they
    /// stay ticked while the search changes. When there are any, the
    /// buttons act on them instead of on the selection.
    pub ticked: std::collections::BTreeSet<usize>,
    pub scroll: usize,
    /// Rows that fit in the list (set by the window).
    pub page: usize,
    pub sort: Col,
    pub descending: bool,
    /// What is typed to find programs (name, publisher, folder).
    pub search: rt_core::textedit::TextEdit,
    /// The search box was clicked (typing goes to it anyway): it shows it.
    pub search_focus: bool,
    pub whose: Whose,
    pub show_system: bool,
    pub show_updates: bool,
    /// How far the details panel is scrolled, in pixels.
    pub details_scroll: f32,
    /// A line for the status bar.
    pub note: Option<String>,
    /// Uninstalling now.
    pub busy: Option<Busy>,
    /// What the programs uninstalled left behind (its own tab).
    pub leftovers: Option<Leftovers>,
    /// The tab to go back to when the leftovers are closed.
    pub back_tab: Tab,
}

/// How a program in the queue is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mark {
    #[default]
    Waiting,
    Running,
    Done,
    Failed,
}

/// The uninstalling under way: the program now, its place in the queue.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Busy {
    /// Each program in the queue: key, name, how it is doing.
    pub items: Vec<(String, String, Mark)>,
    /// Moves on with time, for what turns while waiting (0..1).
    pub phase: f32,
    pub name: String,
    /// 1-based.
    pub index: usize,
    pub total: usize,
    /// The user asked to stop after this one.
    pub stopping: bool,
    /// Changing or repairing, not uninstalling.
    pub modify: bool,
}

impl App {
    pub fn new() -> App {
        App { page: 20, sort: Col::Name, ..App::default() }
    }

    /// Shows what was just read, keeping the program selected.
    pub fn set_found(&mut self, f: Found) {
        let keep = self.selected().map(|p| p.key.clone());
        self.marked.clear();
        self.ticked.clear();
        self.found = Some(f);
        self.loading = false;
        self.rebuild();
        if let Some(k) = keep {
            self.sel = self.rows.iter().position(|&i| self.program(i).is_some_and(|p| p.key == k));
        }
        if self.sel.is_none() && !self.rows.is_empty() {
            self.sel = Some(0);
        }
        self.mark_cursor();
    }

    /// A program of the tab shown.
    pub fn program(&self, i: usize) -> Option<&Program> {
        self.found.as_ref()?.list(self.tab).get(i)
    }

    /// The tabs there are: Programs, and the package managers found.
    pub fn tabs(&self) -> Vec<Tab> {
        let mut t = vec![Tab::Programs];
        if let Some(f) = &self.found {
            if f.scoop.is_some() {
                t.push(Tab::Scoop);
            }
            if f.chocolatey.is_some() {
                t.push(Tab::Chocolatey);
            }
        }
        if self.leftovers.is_some() {
            t.push(Tab::Leftovers);
        }
        t
    }

    /// A leftover, by its index.
    pub fn leftover(&self, i: usize) -> Option<&Leftover> {
        self.leftovers.as_ref()?.items.get(i)
    }

    /// Shows what was found (or that it is being looked for, in a tab of its
    /// own, staying on the tab shown). `show` goes to it; without any, back
    /// to the tab it was opened from.
    pub fn set_leftovers(&mut self, l: Option<Leftovers>, show: bool) {
        self.leftovers = l;
        if self.leftovers.is_some() {
            if show {
                self.set_tab(Tab::Leftovers);
            }
        } else if self.tab == Tab::Leftovers {
            let back = if self.tabs().contains(&self.back_tab) { self.back_tab } else { Tab::Programs };
            self.set_tab(back);
        }
        self.rebuild();
    }

    pub fn set_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            if tab == Tab::Leftovers {
                self.back_tab = self.tab;
            }
            self.tab = tab;
            self.sel = None;
            self.marked.clear();
            self.ticked.clear();
            self.scroll = 0;
            self.details_scroll = 0.0;
            self.rebuild();
        }
    }

    /// Joins what winget said to the programs.
    pub fn set_winget(&mut self, rows: Option<Vec<crate::winget::Row>>) {
        let keep = self.selected().map(|p| p.key.clone());
        if let Some(f) = &mut self.found {
            if let Some(rows) = rows {
                crate::winget::merge(&mut f.inv.programs, rows);
                f.links = shortcuts::match_programs(&f.inv.programs, &f.shortcuts);
            }
            f.winget_done = true;
        }
        self.rebuild();
        if let Some(k) = keep {
            self.sel = self.rows.iter().position(|&i| self.program(i).is_some_and(|p| p.key == k)).or(self.sel);
        }
    }

    /// The program selected.
    pub fn selected(&self) -> Option<&Program> {
        self.sel.and_then(|r| self.rows.get(r)).and_then(|&i| self.program(i))
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.sel.and_then(|r| self.rows.get(r)).copied()
    }

    /// The shortcuts of the program selected.
    pub fn selected_shortcuts(&self) -> Vec<&Shortcut> {
        let (Some(f), Some(i)) = (&self.found, self.selected_index()) else { return Vec::new() };
        if self.tab != Tab::Programs {
            return Vec::new();
        }
        f.links[i].iter().map(|&s| &f.shortcuts[s]).collect()
    }

    /// Whether a program passes the search and the choices.
    fn shown(&self, p: &Program) -> bool {
        if (p.system && !self.show_system) || (p.update && !self.show_updates) || !self.whose.takes(&p.scope) {
            return false;
        }
        let q = self.search.text.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        let hay = format!("{} {} {}", p.name, p.publisher, p.location.as_ref().map(|l| l.display().to_string()).unwrap_or_default()).to_lowercase();
        q.split_whitespace().all(|w| hay.contains(w))
    }

    /// The rows from the programs, the choices and the order.
    pub fn rebuild(&mut self) {
        let keep = self.selected_index();
        if self.tab == Tab::Leftovers {
            let q = self.search.text.trim().to_lowercase();
            let items = self.leftovers.as_ref().map_or(&[][..], |l| &l.items[..]);
            self.rows = (0..items.len())
                .filter(|&i| {
                    let hay = format!("{} {} {}", items[i].program, items[i].path, items[i].value.as_deref().unwrap_or("")).to_lowercase();
                    q.split_whitespace().all(|w| hay.contains(w))
                })
                .collect();
            self.sel = keep.and_then(|k| self.rows.iter().position(|&i| i == k)).or((!self.rows.is_empty()).then_some(0));
            self.scroll = self.scroll.min(self.rows.len().saturating_sub(1));
            return;
        }
        let Some(f) = &self.found else {
            self.rows.clear();
            return;
        };
        let ps = f.list(self.tab);
        let mut rows: Vec<usize> = (0..ps.len()).filter(|&i| self.shown(&ps[i])).collect();
        rows.sort_by(|&a, &b| {
            let (pa, pb) = (&ps[a], &ps[b]);
            let by_name = || pa.name.to_lowercase().cmp(&pb.name.to_lowercase());
            let o = match self.sort {
                Col::Name => by_name(),
                Col::Publisher => pa.publisher.to_lowercase().cmp(&pb.publisher.to_lowercase()).then_with(by_name),
                Col::Version => version_key(&pa.version).cmp(&version_key(&pb.version)).then_with(by_name),
                Col::Size => pa.size.cmp(&pb.size).then_with(by_name),
                Col::Installed => pa.date.cmp(&pb.date).then_with(by_name),
                Col::For => pa.scope.cmp(&pb.scope).then_with(by_name),
                Col::Source => pa.sources.cmp(&pb.sources).then_with(by_name),
            };
            if self.descending {
                o.reverse()
            } else {
                o
            }
        });
        self.rows = rows;
        self.sel = keep.and_then(|k| self.rows.iter().position(|&i| i == k)).or((!self.rows.is_empty()).then_some(0));
        self.scroll = self.scroll.min(self.rows.len().saturating_sub(1));
        if let Some(s) = self.sel {
            self.keep_visible(s);
        }
        // What the search or the choices hide is no longer selected.
        let shown: std::collections::BTreeSet<usize> = self.rows.iter().copied().collect();
        self.marked.retain(|i| shown.contains(i));
        if self.marked.is_empty() {
            self.mark_cursor();
        }
    }

    /// The row of the cursor alone is selected.
    fn mark_cursor(&mut self) {
        self.marked.clear();
        if let Some(i) = self.selected_index() {
            self.marked.insert(i);
        }
    }

    /// A click on row `row`, with Ctrl (add or take away) or Shift (a range).
    pub fn click(&mut self, row: usize, ctrl: bool, shift: bool) {
        if row >= self.rows.len() {
            return;
        }
        if self.sel != Some(row) {
            self.details_scroll = 0.0;
        }
        if shift {
            let a = self.anchor.filter(|&a| a < self.rows.len()).unwrap_or(row);
            if !ctrl {
                self.marked.clear();
            }
            for r in a.min(row)..=a.max(row) {
                self.marked.insert(self.rows[r]);
            }
        } else if ctrl {
            let i = self.rows[row];
            if !self.marked.remove(&i) {
                self.marked.insert(i);
            }
            self.anchor = Some(row);
        } else {
            self.marked.clear();
            self.marked.insert(self.rows[row]);
            self.anchor = Some(row);
        }
        self.sel = Some(row);
    }

    pub fn select_all(&mut self) {
        self.marked = self.rows.iter().copied().collect();
    }

    /// Whether row `row` is selected.
    pub fn is_marked(&self, row: usize) -> bool {
        self.rows.get(row).is_some_and(|i| self.marked.contains(i))
    }

    /// What the buttons act on (indexes into the tab's list): the programs
    /// ticked, those shown first in the list's order, then those the search
    /// hides; with none ticked, the programs selected.
    pub fn chosen(&self) -> Vec<usize> {
        // Leftovers: only those ticked are ever removed.
        if self.tab == Tab::Leftovers {
            return self.leftovers.as_ref().map_or(Vec::new(), Leftovers::chosen);
        }
        if self.ticked.is_empty() {
            return self.rows.iter().copied().filter(|i| self.marked.contains(i)).collect();
        }
        let mut out: Vec<usize> = self.rows.iter().copied().filter(|i| self.ticked.contains(i)).collect();
        out.extend(self.ticked.iter().copied().filter(|i| !self.rows.contains(i)));
        out
    }

    /// Whether row `row`'s box is ticked.
    pub fn is_ticked(&self, row: usize) -> bool {
        let Some(&i) = self.rows.get(row) else { return false };
        match (&self.leftovers, self.tab) {
            (Some(l), Tab::Leftovers) => l.ticked.get(i).copied().unwrap_or(false),
            _ => self.ticked.contains(&i),
        }
    }

    /// Whether the box of the list index `i` can be ticked (a leftover that
    /// needs rights runinst lacks cannot).
    pub fn can_tick(&self, i: usize) -> bool {
        self.tab != Tab::Leftovers || self.leftover(i).is_some_and(|l| !l.admin) && self.leftovers.as_ref().is_some_and(|l| !l.removing)
    }

    fn set_tick(&mut self, i: usize, on: bool) {
        if !self.can_tick(i) {
            return;
        }
        match (&mut self.leftovers, self.tab) {
            (Some(l), Tab::Leftovers) => {
                if let Some(t) = l.ticked.get_mut(i) {
                    *t = on;
                }
            }
            _ => {
                if on {
                    self.ticked.insert(i);
                } else {
                    self.ticked.remove(&i);
                }
            }
        }
    }

    fn any_ticked(&self) -> bool {
        match (&self.leftovers, self.tab) {
            (Some(l), Tab::Leftovers) => l.ticked.iter().any(|t| *t),
            _ => !self.ticked.is_empty(),
        }
    }

    /// Ticks or unticks row `row`'s box.
    pub fn toggle_tick(&mut self, row: usize) {
        let Some(&i) = self.rows.get(row) else { return };
        let on = self.is_ticked(row);
        self.set_tick(i, !on);
    }

    /// Space: ticks the rows selected, or unticks them when all are ticked.
    pub fn toggle_ticks_selected(&mut self) {
        let sel: Vec<usize> = (0..self.rows.len()).filter(|&r| self.is_marked(r)).collect();
        let on = !sel.iter().all(|&r| self.is_ticked(r) || !self.can_tick(self.rows[r]));
        for r in sel {
            self.set_tick(self.rows[r], on);
        }
    }

    /// The box in the header: unticks all, or, with none ticked, ticks
    /// every row shown.
    pub fn toggle_tick_all(&mut self) {
        let on = !self.any_ticked();
        let all: Vec<usize> = if on { self.rows.clone() } else { (0..self.list_len()).collect() };
        for i in all {
            self.set_tick(i, on);
        }
    }

    /// How many the tab's list holds.
    pub fn list_len(&self) -> usize {
        match self.tab {
            Tab::Leftovers => self.leftovers.as_ref().map_or(0, |l| l.items.len()),
            t => self.found.as_ref().map_or(0, |f| f.list(t).len()),
        }
    }

    /// The header's box: none, some or all of those shown ticked.
    pub fn ticks_shown(&self) -> (bool, bool) {
        let any = self.any_ticked();
        (any, any && (0..self.rows.len()).all(|r| self.is_ticked(r) || !self.can_tick(self.rows[r])))
    }

    /// The programs selected, in the list's order.
    pub fn chosen_programs(&self) -> Vec<&Program> {
        self.chosen().into_iter().filter_map(|i| self.program(i)).collect()
    }

    /// Sorts by `col`; the same column again turns the order around.
    pub fn sort_by(&mut self, col: Col) {
        if self.sort == col {
            self.descending = !self.descending;
        } else {
            self.sort = col;
            self.descending = matches!(col, Col::Size | Col::Installed);
        }
        self.rebuild();
    }

    /// Selects row `row` alone.
    pub fn select(&mut self, row: usize) {
        self.click(row, false, false);
    }

    /// The keyboard moves the cursor by `delta` rows (Shift keeps a range).
    pub fn move_sel(&mut self, delta: isize, shift: bool) {
        if self.rows.is_empty() {
            return;
        }
        let at = self.sel.map_or(0, |s| (s as isize + delta).clamp(0, self.rows.len() as isize - 1) as usize);
        self.click(at, false, shift);
        self.keep_visible(at);
    }

    pub fn keep_visible(&mut self, i: usize) {
        let page = self.page.max(1);
        if i < self.scroll {
            self.scroll = i;
        } else if i >= self.scroll + page {
            self.scroll = i + 1 - page;
        }
    }

    pub fn scroll_by(&mut self, rows: isize) {
        let max = self.rows.len().saturating_sub(self.page.max(1));
        self.scroll = (self.scroll as isize + rows).clamp(0, max as isize) as usize;
    }

    /// Typing (or pasting) goes to the search, at its caret.
    pub fn type_text(&mut self, s: &str) {
        self.search.insert(s);
        self.search_focus = true;
        self.rebuild();
    }

    pub fn backspace(&mut self) {
        self.edit_search(|t| t.delete(false, false));
    }

    /// Changes the search's text, caret or selection; the list follows
    /// when the text changed.
    pub fn edit_search(&mut self, f: impl FnOnce(&mut rt_core::textedit::TextEdit)) {
        let before = self.search.text.clone();
        f(&mut self.search);
        self.search_focus = true;
        if self.search.text != before {
            self.rebuild();
        }
    }

    pub fn clear_search(&mut self) {
        self.search.clear();
        self.search_focus = false;
        self.rebuild();
    }

    /// How a program is doing in the queue, while uninstalling.
    pub fn mark_of(&self, p: &Program) -> Option<Mark> {
        self.busy.as_ref()?.items.iter().find(|(k, n, _)| *k == p.key && *n == p.name).map(|i| i.2)
    }

    /// How far the queue is, 0..1, counting the one running as half done.
    pub fn progress(&self) -> f32 {
        let Some(b) = &self.busy else { return 0.0 };
        if b.items.is_empty() {
            return 0.0;
        }
        let done = b.items.iter().map(|i| match i.2 {
            Mark::Done | Mark::Failed => 1.0,
            Mark::Running => 0.5,
            Mark::Waiting => 0.0,
        });
        done.sum::<f32>() / b.items.len() as f32
    }

    /// The programs shown, as CSV (comma-separated, quoted, with a header).
    pub fn csv(&self, header: &[String], whose: &dyn Fn(&Scope) -> String, sources: &dyn Fn(&[Source]) -> String) -> String {
        let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = header.iter().map(|h| q(h)).collect::<Vec<_>>().join(",") + "\r\n";
        for &i in &self.rows {
            let Some(p) = self.program(i) else { continue };
            let date = p.date.map_or(String::new(), |(y, m, d)| format!("{y:04}-{m:02}-{d:02}"));
            let cells = [
                p.name.clone(),
                p.publisher.clone(),
                p.version.clone(),
                p.size.map_or(String::new(), |s| s.to_string()),
                date,
                whose(&p.scope),
                sources(&p.sources),
                p.winget.as_ref().map(|w| w.id.clone()).unwrap_or_default(),
                p.winget.as_ref().and_then(|w| w.available.clone()).unwrap_or_default(),
                p.location.as_ref().map(|l| l.display().to_string()).unwrap_or_default(),
                p.uninstall.clone().unwrap_or_default(),
                p.key.clone(),
            ];
            out += &(cells.iter().map(|c| q(c)).collect::<Vec<_>>().join(",") + "\r\n");
        }
        out
    }
}

/// A version compared by its numbers: `10.2` after `9.11`.
fn version_key(v: &str) -> Vec<(u64, String)> {
    v.split(['.', '-', ' ', '_'])
        .map(|part| {
            let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
            (digits.parse().unwrap_or(0), part[digits.len()..].to_lowercase())
        })
        .collect()
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::inventory::Bits;
    use std::path::PathBuf;

    pub fn program(name: &str, publisher: &str, version: &str, scope: Scope, size: Option<u64>) -> Program {
        Program {
            name: name.into(),
            publisher: publisher.into(),
            version: version.into(),
            size,
            date: Some((2024, 3, name.len() as u32)),
            location: Some(PathBuf::from(format!("C:\\Program Files\\{name}"))),
            uninstall: Some(format!("\"C:\\Program Files\\{name}\\uninstall.exe\"")),
            quiet: None,
            modify: None,
            icon: None,
            msi: None,
            system: false,
            update: false,
            no_remove: false,
            scope,
            bits: Bits::B64,
            key: format!("HKLM\\...\\{name}"),
            values: vec![("DisplayName".into(), name.into())],
            sources: vec![crate::inventory::Source::Registry],
            ..Program::default()
        }
    }

    pub fn sample() -> Found {
        let me = Scope::User { name: "me".into(), current: true };
        let ana = Scope::User { name: "Ana".into(), current: false };
        let mut programs = vec![
            program("Zeta Editor", "Zeta Ltd", "9.11", Scope::Machine, Some(300 << 20)),
            program("Alpha Player", "Alpha", "10.2", Scope::Machine, Some(40 << 20)),
            program("My Notes", "Notes Co", "1.0", me, None),
            program("Ana's Game", "Games", "2.5", ana, Some(2 << 30)),
            program("Runtime Helper", "Microsoft Corporation", "14.0", Scope::Machine, Some(1 << 20)),
            program("Security Update for Zeta (KB123456)", "Zeta Ltd", "1", Scope::Machine, None),
        ];
        programs[4].system = true;
        programs[5].update = true;
        Found::new(Inventory { programs, unread_users: vec!["Guest".into()], other_users: vec![("Ana".into(), PathBuf::from("C:\\Users\\Ana"))], ..Inventory::default() }, Vec::new())
    }

    fn names(app: &App) -> Vec<&str> {
        app.rows.iter().map(|&i| app.program(i).unwrap().name.as_str()).collect()
    }

    #[test]
    fn filters_and_order() {
        let mut app = App::new();
        app.set_found(sample());
        assert_eq!(names(&app), ["Alpha Player", "Ana's Game", "My Notes", "Zeta Editor"]);
        app.show_system = true;
        app.show_updates = true;
        app.rebuild();
        assert_eq!(app.rows.len(), 6);
        app.show_system = false;
        app.show_updates = false;
        app.whose = Whose::Mine;
        app.rebuild();
        assert_eq!(names(&app), ["My Notes"]);
        app.whose = Whose::Others;
        app.rebuild();
        assert_eq!(names(&app), ["Ana's Game"]);
        app.whose = Whose::Everyone;
        app.rebuild();
        assert_eq!(names(&app), ["Alpha Player", "Zeta Editor"]);
        app.whose = Whose::All;
        app.type_text("zeta");
        assert_eq!(names(&app), ["Zeta Editor"]);
        for _ in 0..4 {
            app.backspace();
        }
        assert_eq!(app.rows.len(), 4);
        app.type_text("A pl");
        assert_eq!(names(&app), ["Alpha Player"]);
        app.clear_search();
        app.sort_by(Col::Size);
        assert_eq!(names(&app), ["Ana's Game", "Zeta Editor", "Alpha Player", "My Notes"]);
        app.sort_by(Col::Version);
        app.sort_by(Col::Version);
        assert!(app.descending);
        app.sort_by(Col::Name);
        app.descending = false;
        app.sort_by(Col::Version);
        assert_eq!(names(&app)[..2], ["My Notes", "Ana's Game"]);
        // 9.11 before 10.2.
        let pos = |n: &str| names(&app).iter().position(|x| *x == n).unwrap();
        assert!(pos("Zeta Editor") < pos("Alpha Player"));
    }

    #[test]
    fn selecting_several() {
        let mut app = App::new();
        app.set_found(sample());
        assert_eq!(app.chosen().len(), 1);
        app.click(1, false, false);
        app.click(3, true, false);
        assert_eq!(app.chosen(), [app.rows[1], app.rows[3]]);
        app.click(0, false, true);
        // A range from the last click with Ctrl (row 3) to row 0.
        assert_eq!(app.chosen().len(), 4);
        app.select_all();
        assert_eq!(app.chosen().len(), app.rows.len());
        // The search hides some: they are no longer chosen.
        app.type_text("zeta");
        assert_eq!(app.chosen().len(), 1);
        app.clear_search();
        app.move_sel(1, true);
        assert!(!app.chosen().is_empty());
    }

    #[test]
    fn leftovers_keep_the_tab() {
        let mut app = App::new();
        let mut f = sample();
        f.scoop = Some(vec![program("ripgrep", "main", "14.1", Scope::Machine, None)]);
        app.set_found(f);
        app.set_tab(Tab::Scoop);
        // Looking, then nothing found: still on Scoop.
        app.set_leftovers(Some(Leftovers { searching: true, ..Default::default() }), false);
        assert_eq!(app.tab, Tab::Scoop);
        app.set_leftovers(None, false);
        assert_eq!(app.tab, Tab::Scoop);
        // Shown, then closed: back to Scoop.
        let l = crate::leftovers::Leftover {
            program: "rg".into(),
            group: crate::leftovers::Group::Settings,
            kind: crate::leftovers::Kind::Folder,
            path: "C:\\x\\persist\\rg".into(),
            value: None,
            data: None,
            size: None,
            admin: false,
            doubt: false,
        };
        app.set_leftovers(Some(Leftovers::new(vec![l])), true);
        assert_eq!(app.tab, Tab::Leftovers);
        // Settings are not ticked at first; a click ticks.
        assert!(app.chosen().is_empty());
        app.toggle_tick(0);
        assert_eq!(app.chosen(), [0]);
        app.set_leftovers(None, false);
        assert_eq!(app.tab, Tab::Scoop);
    }

    #[test]
    fn search_edits() {
        let mut app = App::new();
        app.set_found(sample());
        app.type_text("zeta editor");
        assert_eq!(app.rows.len(), 1);
        app.edit_search(|t| {
            t.move_caret(true, false, true, false);
            t.move_caret(false, true, false, true);
        });
        assert_eq!(app.search.selected_text(), "editor");
        app.edit_search(|t| t.delete(false, false));
        assert_eq!(app.search.text, "zeta ");
        app.type_text("x");
        assert!(app.rows.is_empty());
    }

    #[test]
    fn ticks_outlive_the_search() {
        let mut app = App::new();
        app.set_found(sample());
        let names = |app: &App| app.chosen_programs().iter().map(|p| p.name.clone()).collect::<Vec<_>>();
        app.type_text("zeta");
        app.select(0);
        app.toggle_ticks_selected();
        app.clear_search();
        app.type_text("alpha");
        app.toggle_tick(0);
        // The other one is hidden by the search, and still chosen; the
        // selection does not count while some are ticked.
        assert_eq!(names(&app), ["Alpha Player", "Zeta Editor"]);
        app.select(0);
        app.toggle_ticks_selected();
        assert_eq!(names(&app), ["Zeta Editor"]);
        app.toggle_tick_all();
        assert!(app.ticked.is_empty());
        assert_eq!(names(&app), ["Alpha Player"]);
        app.toggle_tick_all();
        assert_eq!(app.ticked.len(), app.rows.len());
    }

    #[test]
    fn tabs_and_winget() {
        let mut app = App::new();
        let mut f = sample();
        f.scoop = Some(vec![program("ripgrep", "main", "14.1", Scope::Machine, None)]);
        app.set_found(f);
        assert_eq!(app.tabs(), [Tab::Programs, Tab::Scoop]);
        app.set_tab(Tab::Scoop);
        assert_eq!(names(&app), ["ripgrep"]);
        app.set_tab(Tab::Programs);
        app.select(3);
        let rows = crate::winget::parse("Name         Id           Version  Available  Source\n----------------------------------------------------\nZeta Editor  Zeta.Editor  9.11     9.12       winget\n");
        app.set_winget(Some(rows));
        let p = app.selected().unwrap();
        assert_eq!(p.name, "Zeta Editor");
        assert!(p.sources.contains(&Source::Winget));
        assert!(app.found.as_ref().unwrap().winget_done);
    }

    #[test]
    fn selection_survives_reading_again() {
        let mut app = App::new();
        app.set_found(sample());
        app.select(3);
        assert_eq!(app.selected().map(|p| p.name.as_str()), Some("Zeta Editor"));
        app.set_found(sample());
        assert_eq!(app.selected().map(|p| p.name.as_str()), Some("Zeta Editor"));
        app.page = 2;
        app.move_sel(-10, false);
        assert_eq!(app.sel, Some(0));
        app.move_sel(3, false);
        assert_eq!(app.scroll, 2);
        let csv = app.csv(&["Name".into()], &|_| "x".into(), &|_| "y".into());
        assert_eq!(csv.lines().count(), 5);
        assert!(csv.contains("\"Ana's Game\""));
    }
}
