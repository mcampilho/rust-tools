//! What the menu shows and where the selection is, apart from drawing.

use crate::category::{Category, Group};
use crate::sources::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// The left side of the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    /// The categories that have programs.
    #[default]
    Categories,
    /// The subgroups and programs of a category, or the programs of a
    /// subgroup.
    Inside(Group),
    /// The programs the user hid.
    Hidden,
    Options,
}

/// A line of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Goes up (or, from the hidden programs, back to the options).
    Back,
    /// A category or a subgroup, and how many programs it has.
    Category(Group, usize),
    /// An index into [`App::entries`].
    Entry(usize),
}

#[derive(Default)]
pub struct App {
    pub entries: Vec<Entry>,
    pub view: View,
    pub query: String,
    pub selected: usize,
    /// First row shown.
    pub scroll: usize,
    /// The folders are being read for the first time.
    pub loading: bool,
    /// The options page shows how the menu looks, not its programs.
    pub appearance: bool,
    /// Categories in `order`, then by name; else all by name.
    pub custom_order: bool,
    pub order: Vec<Group>,
    /// The programs chosen together, to act on all of them.
    pub marked: BTreeSet<PathBuf>,
    /// The row a range of marks starts at.
    anchor: Option<usize>,
}

/// Letters without their accents, in lower case, for searching.
fn plain(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

/// Where a category goes among the others.
fn order(c: Category) -> (bool, String, Category) {
    (c == Category::Other, plain(&c.title()), c)
}

impl App {
    /// The programs that match what is typed: those whose name starts with it
    /// first, then those with a word that does, then the others.
    fn matches(&self) -> Vec<usize> {
        let q = plain(self.query.trim());
        let mut found: Vec<(u8, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.hidden)
            .filter_map(|(i, e)| {
                let n = plain(&e.name);
                let rank = if n.starts_with(&q) {
                    0
                } else if n.split([' ', '-', '_', '.']).any(|w| w.starts_with(&q)) {
                    1
                } else if q.split_whitespace().all(|part| n.contains(part)) {
                    2
                } else {
                    return None;
                };
                Some((rank, i))
            })
            .collect();
        found.sort();
        found.into_iter().map(|(_, i)| i).collect()
    }

    pub fn rows(&self) -> Vec<Row> {
        if !self.query.trim().is_empty() {
            return self.matches().into_iter().map(Row::Entry).collect();
        }
        match self.view {
            // The favourites, then the categories.
            View::Categories => self
                .entries_where(|e| e.favourite && !e.hidden)
                .chain(self.categories().into_iter().map(|(c, n)| Row::Category(c.into(), n)))
                .collect(),
            // The subgroups of a category come before its other programs.
            View::Inside(g) => std::iter::once(Row::Back)
                .chain(self.subgroups(g).into_iter().map(|(g, n)| Row::Category(g, n)))
                .chain(self.entries_where(move |e| e.group() == g && !e.hidden))
                .collect(),
            View::Hidden => std::iter::once(Row::Back).chain(self.entries_where(|e| e.hidden)).collect(),
            View::Options => Vec::new(),
        }
    }

    /// The categories with programs shown, by their names as displayed, Other
    /// last; and how many programs each has.
    fn categories(&self) -> Vec<(Category, usize)> {
        let mut counts = BTreeMap::<Category, usize>::new();
        for e in self.entries.iter().filter(|e| !e.hidden) {
            *counts.entry(e.category).or_default() += 1;
        }
        let mut all: Vec<_> = counts.into_iter().collect();
        all.sort_by_cached_key(|(c, _)| (self.place_of((*c).into()), order(*c)));
        all
    }

    /// Where the user put `g`; after all of those, when not put anywhere.
    fn place_of(&self, g: Group) -> usize {
        self.order.iter().position(|x| *x == g).filter(|_| self.custom_order).unwrap_or(usize::MAX)
    }

    /// The categories shown, or the subgroups beside `g`, in their order.
    pub fn siblings(&self, g: Group) -> Vec<Group> {
        match g.sub {
            Some(_) => self.subgroups(g.top()).into_iter().map(|x| x.0).collect(),
            None => self.categories().into_iter().map(|x| x.0.into()).collect(),
        }
    }

    /// The siblings with `moved` just before `at`, or just after it.
    pub fn reordered(&self, moved: Group, at: Group, after: bool) -> Option<Vec<Group>> {
        let mut all = self.siblings(moved);
        if moved == at || !all.contains(&at) {
            return None;
        }
        all.retain(|g| *g != moved);
        let i = all.iter().position(|g| *g == at)? + usize::from(after);
        all.insert(i, moved);
        Some(all)
    }

    /// The siblings with `g` one place up or down.
    pub fn shifted(&self, g: Group, down: bool) -> Option<Vec<Group>> {
        let all = self.siblings(g);
        let i = all.iter().position(|x| *x == g)?;
        let other = *all.get(if down { i + 1 } else { i.checked_sub(1)? })?;
        self.reordered(g, other, down)
    }

    fn path_of(&self, k: usize) -> Option<PathBuf> {
        match self.rows().get(k) {
            Some(&Row::Entry(i)) => Some(self.entries[i].path.clone()),
            _ => None,
        }
    }

    /// Marks the program of row `k`, or takes its mark off.
    pub fn toggle_mark(&mut self, k: usize) {
        let Some(path) = self.path_of(k) else { return };
        if !self.marked.remove(&path) {
            self.marked.insert(path);
        }
        self.anchor = Some(k);
    }

    /// Marks the programs from the anchor to row `k`.
    pub fn mark_to(&mut self, k: usize) {
        let from = *self.anchor.get_or_insert(k);
        for i in from.min(k)..=from.max(k) {
            self.marked.extend(self.path_of(i));
        }
    }

    pub fn mark_all(&mut self) {
        for k in 0..self.rows().len() {
            self.marked.extend(self.path_of(k));
        }
    }

    pub fn clear_marks(&mut self) {
        self.marked.clear();
        self.anchor = None;
    }

    /// The subgroups of the category `g` (none for a subgroup).
    fn subgroups(&self, g: Group) -> Vec<(Group, usize)> {
        if g.sub.is_some() {
            return Vec::new();
        }
        let mut counts = BTreeMap::<&str, usize>::new();
        for e in self.entries.iter().filter(|e| !e.hidden && e.category == g.category) {
            if let Some(sub) = e.sub {
                *counts.entry(sub).or_default() += 1;
            }
        }
        let mut all: Vec<_> = counts.into_iter().map(|(sub, n)| (Group { category: g.category, sub: Some(sub) }, n)).collect();
        all.sort_by_cached_key(|(g, _)| (self.place_of(*g), plain(&g.title())));
        all
    }

    /// Every group a program can be moved to: the built-in categories and
    /// those in use, each followed by its subgroups, by name.
    pub fn groups(&self) -> Vec<Group> {
        let mut all: BTreeSet<Group> = Category::ALL.into_iter().map(Group::from).collect();
        for e in &self.entries {
            all.extend([e.group().top(), e.group()]);
        }
        let mut all: Vec<Group> = all.into_iter().collect();
        all.sort_by_cached_key(|g| (order(g.category), g.sub.map(plain)));
        all
    }

    fn entries_where<'a>(&'a self, keep: impl Fn(&Entry) -> bool + 'a) -> impl Iterator<Item = Row> + 'a {
        self.entries.iter().enumerate().filter(move |(_, e)| keep(e)).map(|(i, _)| Row::Entry(i))
    }

    pub fn hidden_count(&self) -> usize {
        self.entries.iter().filter(|e| e.hidden).count()
    }

    /// The program of the selected row.
    pub fn selected_entry(&self) -> Option<&Entry> {
        match self.rows().get(self.selected) {
            Some(&Row::Entry(i)) => self.entries.get(i),
            _ => None,
        }
    }

    pub fn open_hidden(&mut self) {
        self.clear_marks();
        self.view = View::Hidden;
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn open_category(&mut self, g: Group) {
        self.clear_marks();
        self.view = View::Inside(g);
        // On the first row, not on "back".
        self.selected = 1;
        self.scroll = 0;
    }

    /// Up a level, with the group left selected; from the hidden programs,
    /// back to the options.
    pub fn back(&mut self) {
        self.clear_marks();
        if self.view == View::Hidden {
            self.view = View::Options;
            return;
        }
        let from = match self.view {
            View::Inside(g) => Some(g),
            _ => None,
        };
        self.view = match from {
            Some(g) if g.sub.is_some() => View::Inside(g.top()),
            _ => View::Categories,
        };
        self.scroll = 0;
        self.selected = self.rows().iter().position(|r| matches!(r, Row::Category(c, _) if Some(*c) == from)).unwrap_or(0);
    }

    /// As when the menu is opened anew.
    pub fn reset(&mut self) {
        self.clear_marks();
        self.appearance = false;
        self.view = View::Categories;
        self.query.clear();
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn set_query(&mut self, query: String) {
        self.clear_marks();
        self.query = query;
        self.selected = 0;
        self.scroll = 0;
        if self.view == View::Options {
            self.view = View::Categories;
        }
    }

    pub fn set_entries(&mut self, entries: Vec<Entry>) {
        self.entries = entries;
        self.loading = false;
        let there: BTreeSet<&PathBuf> = self.entries.iter().map(|e| &e.path).collect();
        self.marked.retain(|p| there.contains(p));
        // The group shown may be gone: renamed, or its programs moved.
        while matches!(self.view, View::Inside(_)) && self.rows().len() <= 1 {
            self.back();
        }
        let n = self.rows().len();
        self.selected = self.selected.min(n.saturating_sub(1));
        self.scroll = self.scroll.min(n.saturating_sub(1));
    }

    /// Moves the selection by `by` rows, staying in the list.
    pub fn step(&mut self, by: isize) {
        let n = self.rows().len();
        if n > 0 {
            self.selected = self.selected.saturating_add_signed(by).min(n - 1);
        }
    }

    /// Scrolls so the selection is among the `visible` rows.
    pub fn reveal_selection(&mut self, visible: usize) {
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if visible > 0 && self.selected >= self.scroll + visible {
            self.scroll = self.selected + 1 - visible;
        }
    }

    pub fn scroll_by(&mut self, by: isize, visible: usize) {
        let most = self.rows().len().saturating_sub(visible);
        self.scroll = self.scroll.saturating_add_signed(by).min(most);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sample() -> App {
        let entry = |name: &str, category| Entry::new(name.into(), PathBuf::from(name), category);
        let mut app = App::default();
        app.set_entries(vec![
            entry("Audacity", Category::Multimedia),
            entry("Firefox", Category::Internet),
            entry("Google Chrome", Category::Internet),
            entry("Mozilla Thunderbird", Category::Internet),
            entry("Múltiplo", Category::Other),
        ]);
        app
    }

    fn group(c: Category, n: usize) -> Row {
        Row::Category(c.into(), n)
    }

    #[test]
    fn subgroups_come_before_programs_and_back_goes_up_one_level() {
        let mut app = sample();
        let sub = Group::parse("Internet/Browsers").unwrap();
        let mut entries = app.entries.clone();
        entries[1].set_group(sub);
        entries[2].set_group(sub);
        entries[4].set_group(Group::parse("Zed tools").unwrap());
        app.set_entries(entries);
        // The user's category among the others, Other still last.
        assert_eq!(app.rows(), [group(Category::Internet, 3), group(Category::Multimedia, 1), group(Category::named("Zed tools"), 1)]);
        app.open_category(Category::Internet.into());
        assert_eq!(app.rows(), [Row::Back, Row::Category(sub, 2), Row::Entry(3)]);
        app.open_category(sub);
        assert_eq!(app.rows(), [Row::Back, Row::Entry(1), Row::Entry(2)]);
        app.back();
        assert_eq!((app.view, app.selected), (View::Inside(Category::Internet.into()), 1));
        assert!(app.groups().windows(2).any(|w| w == [Category::Internet.into(), sub]));
        // The subgroup shown disappears: up to the category.
        app.open_category(sub);
        let mut entries = app.entries.clone();
        entries[1].sub = None;
        entries[2].sub = None;
        app.set_entries(entries);
        assert_eq!(app.view, View::Inside(Category::Internet.into()));
    }

    #[test]
    fn categories_keep_the_order_of_the_user() {
        let mut app = sample();
        let (internet, other): (Group, Group) = (Category::Internet.into(), Category::Other.into());
        let moved = app.reordered(other, internet, false).unwrap();
        assert_eq!(moved, [other, internet, Category::Multimedia.into()]);
        app.order = moved;
        // Only when asked for.
        assert_eq!(app.siblings(internet)[0], internet);
        app.custom_order = true;
        assert_eq!(app.siblings(internet)[0], other);
        assert_eq!(app.shifted(other, true).unwrap()[..2], [internet, other]);
        assert_eq!(app.shifted(other, false), None);
    }

    #[test]
    fn programs_are_marked_one_by_one_or_in_a_range() {
        let mut app = sample();
        app.open_category(Category::Internet.into());
        app.toggle_mark(1);
        app.mark_to(3);
        assert_eq!(app.marked.len(), 3);
        app.toggle_mark(2);
        assert_eq!(app.marked.len(), 2);
        // "Back" is not a program.
        app.toggle_mark(0);
        assert_eq!(app.marked.len(), 2);
        app.set_query("o".into());
        assert!(app.marked.is_empty());
        app.mark_all();
        assert_eq!(app.marked.len(), 4);
    }

    #[test]
    fn categories_then_their_programs_and_back() {
        let mut app = sample();
        assert_eq!(app.rows(), [group(Category::Internet, 3), group(Category::Multimedia, 1), group(Category::Other, 1)]);
        app.open_category(Category::Multimedia.into());
        assert_eq!(app.rows(), [Row::Back, Row::Entry(0)]);
        assert_eq!(app.selected, 1);
        app.back();
        assert_eq!((app.view, app.selected), (View::Categories, 1));
    }

    #[test]
    fn favourites_come_first_and_hidden_ones_apart() {
        let mut app = sample();
        app.entries[2].favourite = true;
        app.entries[0].hidden = true;
        assert_eq!(app.rows(), [Row::Entry(2), group(Category::Internet, 3), group(Category::Other, 1)]);
        app.set_query("aud".into());
        assert!(app.rows().is_empty());
        app.set_query(String::new());
        app.open_hidden();
        assert_eq!((app.rows(), app.hidden_count()), (vec![Row::Back, Row::Entry(0)], 1));
        app.back();
        assert_eq!(app.view, View::Options);
    }

    #[test]
    fn search_ranks_and_ignores_accents() {
        let mut app = sample();
        app.set_query("fi".into());
        assert_eq!(app.rows(), [Row::Entry(1)]);
        app.set_query("th".into());
        assert_eq!(app.rows(), [Row::Entry(3)]);
        app.set_query("mul".into());
        assert_eq!(app.rows(), [Row::Entry(4)]);
        app.set_query("o".into());
        // Nothing starts with it as a word: all that contain it, in order.
        assert_eq!(app.rows().len(), 4);
        app.set_query("zzz".into());
        assert!(app.rows().is_empty());
    }

    #[test]
    fn the_selection_stays_in_the_list_and_in_sight() {
        let mut app = sample();
        app.step(-1);
        assert_eq!(app.selected, 0);
        app.step(10);
        assert_eq!(app.selected, 2);
        app.reveal_selection(2);
        assert_eq!(app.scroll, 1);
        app.scroll_by(-5, 2);
        assert_eq!(app.scroll, 0);
        app.scroll_by(5, 2);
        assert_eq!(app.scroll, 1);
    }
}
