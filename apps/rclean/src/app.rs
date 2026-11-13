//! The state of the window, without the window: what can be cleaned, which
//! are ticked, what was found in each and how cleaning went, the rows of the
//! list (a title for each group, then its items) and the selection.

use crate::clean::{Cleaned, Found};
use crate::targets::{Group, Target};

/// How a target is doing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum State {
    /// Not looked at yet.
    #[default]
    Waiting,
    Analysing,
    Found(Found),
    Cleaning(Found),
    /// Cleaned: how it went.
    Cleaned(Cleaned),
    /// Needs administrator rights rclean does not have.
    NeedsAdmin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub target: Target,
    pub ticked: bool,
    pub state: State,
    /// Its program is open (a browser): some files may be in use.
    pub open: bool,
}

impl Item {
    pub fn found(&self) -> Option<&Found> {
        match &self.state {
            State::Found(f) | State::Cleaning(f) => Some(f),
            _ => None,
        }
    }

    /// Whether it can be ticked and cleaned here.
    pub fn usable(&self) -> bool {
        self.state != State::NeedsAdmin
    }
}

/// A row of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// A group's title: index into `App::groups`.
    Group(usize),
    Item(usize),
}

/// Analysing or cleaning, under way.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Busy {
    pub cleaning: bool,
    pub done: usize,
    pub total: usize,
    /// The target now.
    pub name: String,
    /// Moves on with time, for what turns while working (0..1).
    pub phase: f32,
    /// The user asked to stop.
    pub stopping: bool,
}

#[derive(Debug, Default)]
pub struct App {
    pub items: Vec<Item>,
    pub groups: Vec<Group>,
    pub rows: Vec<Row>,
    /// The row selected.
    pub sel: Option<usize>,
    pub scroll: usize,
    /// Rows that fit in the list (set by the window).
    pub page: usize,
    pub busy: Option<Busy>,
    /// How far the details panel is scrolled, in pixels.
    pub details_scroll: f32,
    /// A line for the status bar.
    pub note: Option<String>,
    /// What the last cleaning freed, for the status bar.
    pub freed: Option<Cleaned>,
}

impl App {
    /// The targets, ticked as `ticks` says (`id` → ticked), else by default;
    /// those that need rights rclean does not have are marked so.
    pub fn new(targets: Vec<Target>, admin: bool, ticks: &[(String, bool)]) -> App {
        let items: Vec<Item> = targets
            .into_iter()
            .map(|t| {
                let state = if t.admin && !admin { State::NeedsAdmin } else { State::Waiting };
                // What cannot be cleaned here is not ticked (nor remembered so).
                let ticked = state != State::NeedsAdmin && ticks.iter().find(|(id, _)| *id == t.id).map_or(t.default_on, |(_, on)| *on);
                Item { target: t, ticked, state, open: false }
            })
            .collect();
        let mut app = App { items, page: 20, ..App::default() };
        app.rebuild();
        app.sel = app.rows.iter().position(|r| matches!(r, Row::Item(_)));
        app
    }

    /// The rows: each group's title, then its items, in the order found.
    pub fn rebuild(&mut self) {
        self.groups.clear();
        for it in &self.items {
            if !self.groups.contains(&it.target.group) {
                self.groups.push(it.target.group);
            }
        }
        self.rows.clear();
        for (g, group) in self.groups.iter().enumerate() {
            self.rows.push(Row::Group(g));
            self.rows.extend(self.items.iter().enumerate().filter(|(_, it)| it.target.group == *group).map(|(i, _)| Row::Item(i)));
        }
    }

    /// The items of a group.
    pub fn group_items(&self, g: usize) -> Vec<usize> {
        let Some(group) = self.groups.get(g) else { return Vec::new() };
        self.items.iter().enumerate().filter(|(_, it)| it.target.group == *group).map(|(i, _)| i).collect()
    }

    pub fn selected_item(&self) -> Option<&Item> {
        match self.rows.get(self.sel?)? {
            Row::Item(i) => self.items.get(*i),
            Row::Group(_) => None,
        }
    }

    /// The ticks to remember: those that differ from the default.
    pub fn ticks(&self) -> Vec<(String, bool)> {
        self.items.iter().filter(|it| it.usable() && it.ticked != it.target.default_on).map(|it| (it.target.id.clone(), it.ticked)).collect()
    }

    // ------------------------------------------------------------ ticking

    /// Ticks a row's box: an item, or every item of a group (all off when
    /// all were on).
    pub fn toggle_row(&mut self, row: usize) {
        if self.busy.is_some() {
            return;
        }
        match self.rows.get(row).copied() {
            Some(Row::Item(i)) => {
                if self.items[i].usable() {
                    self.items[i].ticked = !self.items[i].ticked;
                }
            }
            Some(Row::Group(g)) => {
                let its: Vec<usize> = self.group_items(g).into_iter().filter(|&i| self.items[i].usable()).collect();
                let on = !its.iter().all(|&i| self.items[i].ticked);
                for i in its {
                    self.items[i].ticked = on;
                }
            }
            None => {}
        }
    }

    /// The tick of a group: none, some, or all of its items.
    pub fn group_tick(&self, g: usize) -> (bool, bool) {
        let its: Vec<usize> = self.group_items(g).into_iter().filter(|&i| self.items[i].usable()).collect();
        let n = its.iter().filter(|&&i| self.items[i].ticked).count();
        (n > 0, n == its.len() && n > 0)
    }

    pub fn tick_all(&mut self, on: bool) {
        if self.busy.is_none() {
            for it in self.items.iter_mut().filter(|it| it.usable()) {
                it.ticked = on;
            }
        }
    }

    pub fn tick_defaults(&mut self) {
        if self.busy.is_none() {
            for it in self.items.iter_mut() {
                it.ticked = it.target.default_on && it.usable();
            }
        }
    }

    /// The items ticked that can be cleaned here.
    pub fn chosen(&self) -> Vec<usize> {
        (0..self.items.len()).filter(|&i| self.items[i].ticked && self.items[i].usable()).collect()
    }

    /// What the items ticked hold: (files, bytes), from the last analysis.
    pub fn ticked_total(&self) -> (u64, u64) {
        self.chosen().iter().filter_map(|&i| self.items[i].found()).fold((0, 0), |(n, b), f| (n + f.count(), b + f.bytes))
    }

    /// Something was found to clean among the ticked ones.
    pub fn anything_to_clean(&self) -> bool {
        self.chosen().iter().any(|&i| self.items[i].found().is_some_and(|f| f.count() > 0))
    }

    // ------------------------------------------------------------ progress

    /// How far the work is (0..1).
    pub fn progress(&self) -> f32 {
        self.busy.as_ref().map_or(0.0, |b| if b.total == 0 { 0.0 } else { b.done as f32 / b.total as f32 })
    }

    /// An item was looked at.
    pub fn set_found(&mut self, i: usize, f: Found) {
        if let Some(it) = self.items.get_mut(i) {
            it.state = State::Found(f);
        }
        if let Some(b) = &mut self.busy {
            b.done += 1;
        }
    }

    /// An item was cleaned.
    pub fn set_cleaned(&mut self, i: usize, c: Cleaned) {
        if let Some(it) = self.items.get_mut(i) {
            it.state = State::Cleaned(c.clone());
        }
        let total = self.freed.get_or_insert_with(Cleaned::default);
        total.bytes += c.bytes;
        total.files += c.files;
        total.failed += c.failed;
        if let Some(b) = &mut self.busy {
            b.done += 1;
        }
    }

    // ------------------------------------------------------------ the list

    pub fn select(&mut self, row: usize) {
        if row < self.rows.len() {
            self.sel = Some(row);
            self.details_scroll = 0.0;
            self.keep_visible(row);
        }
    }

    pub fn move_sel(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let cur = self.sel.unwrap_or(0) as isize;
        let to = (cur + delta).clamp(0, self.rows.len() as isize - 1) as usize;
        self.select(to);
    }

    pub fn keep_visible(&mut self, row: usize) {
        let page = self.page.max(1);
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + page {
            self.scroll = row + 1 - page;
        }
    }

    pub fn scroll_by(&mut self, rows: isize) {
        let max = self.rows.len().saturating_sub(self.page.max(1));
        self.scroll = (self.scroll as isize + rows).clamp(0, max as isize) as usize;
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::targets::{Filter, Kind, Place};

    pub fn target(id: &str, group: Group, admin: bool, on: bool) -> Target {
        Target {
            id: id.into(),
            group,
            name: "Cache",
            about: "Pages, pictures and scripts kept to open sites faster. They are downloaded again when needed; history, cookies and passwords stay.",
            places: vec![Place { dir: std::path::PathBuf::from(format!("C:\\x\\{id}")), filter: Filter::All }],
            kind: Kind::Files,
            admin,
            default_on: on,
            process: &[],
        }
    }

    /// A list as the screenshots and the translation test show it.
    pub fn sample() -> App {
        let ts = vec![
            Target { name: "Temporary files", ..target("win.user-temp", Group::System, false, true) },
            Target { name: "Temporary files of Windows", ..target("win.temp", Group::System, true, true) },
            Target { name: "Thumbnail cache", ..target("win.thumbs", Group::System, false, false) },
            Target { name: "Recycle Bin", kind: Kind::RecycleBin, ..target("win.recycle", Group::System, false, false) },
            target("chrome.cache", Group::Browser("Google Chrome"), false, true),
            target("firefox.cache", Group::Browser("Firefox"), false, true),
        ];
        let mut app = App::new(ts, false, &[]);
        let found = |n: usize, b: u64| Found { files: (0..n).map(|i| (std::path::PathBuf::from(format!("f{i}")), b / n as u64)).collect(), bytes: b, ..Found::default() };
        app.set_found(0, found(1234, 812_000_000));
        app.set_found(2, found(40, 96_000_000));
        app.set_found(3, Found { bytes: 2_400_000_000, items: 310, ..Found::default() });
        app.set_found(4, found(5210, 1_310_000_000));
        app.set_cleaned(5, Cleaned { bytes: 250_000_000, files: 900, failed: 12 });
        app.items[4].open = true;
        app
    }

    #[test]
    fn groups_rows_and_ticks() {
        let mut app = sample();
        assert_eq!(app.groups, [Group::System, Group::Browser("Google Chrome"), Group::Browser("Firefox")]);
        assert_eq!(app.rows.len(), 3 + 6);
        assert_eq!(app.rows[0], Row::Group(0));
        assert_eq!(app.sel, Some(1), "the first item, not a title");
        assert_eq!(app.items[1].state, State::NeedsAdmin);
        // The defaults: temp, Chrome and Firefox; Windows' temp needs rights.
        assert_eq!(app.chosen(), [0, 4, 5]);
        assert_eq!(app.ticked_total(), (1234 + 5210, 812_000_000 + 1_310_000_000));
        // A group's box: all of it on, then off; the one needing rights stays off.
        app.toggle_row(0);
        assert_eq!(app.group_tick(0), (true, true));
        assert!(!app.items[1].ticked);
        app.toggle_row(0);
        assert_eq!(app.group_tick(0), (false, false));
        app.toggle_row(1);
        assert_eq!(app.group_tick(0), (true, false));
        // Only what differs from the defaults is remembered.
        assert!(app.ticks().is_empty());
        app.toggle_row(3);
        assert_eq!(app.ticks(), [("win.thumbs".to_string(), true)]);
        let again = App::new(app.items.iter().map(|it| it.target.clone()).collect(), true, &app.ticks());
        assert!(again.items[2].ticked && again.items[1].ticked && again.items[1].usable(), "with rights, Windows' temp can be cleaned");
    }
}
