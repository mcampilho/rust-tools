//! What the menu shows and where the selection is, apart from drawing.

use crate::category::Category;
use crate::sources::Entry;

/// The left side of the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    /// The categories that have programs.
    #[default]
    Categories,
    /// The programs of one.
    Inside(Category),
    Options,
}

/// A line of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Goes back to the categories.
    Back(Category),
    Category(Category, usize),
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

impl App {
    /// The programs that match what is typed: those whose name starts with it
    /// first, then those with a word that does, then the others.
    fn matches(&self) -> Vec<usize> {
        let q = plain(self.query.trim());
        let mut found: Vec<(u8, usize)> = self
            .entries
            .iter()
            .enumerate()
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
            View::Categories => Category::ALL
                .into_iter()
                .filter_map(|c| {
                    let n = self.entries.iter().filter(|e| e.category == c).count();
                    (n > 0).then_some(Row::Category(c, n))
                })
                .collect(),
            View::Inside(c) => std::iter::once(Row::Back(c))
                .chain(self.entries.iter().enumerate().filter(|(_, e)| e.category == c).map(|(i, _)| Row::Entry(i)))
                .collect(),
            View::Options => Vec::new(),
        }
    }

    pub fn open_category(&mut self, c: Category) {
        self.view = View::Inside(c);
        // On the first program, not on "back".
        self.selected = 1;
        self.scroll = 0;
    }

    /// Back to the categories, with the one left selected.
    pub fn back(&mut self) {
        let from = match self.view {
            View::Inside(c) => Some(c),
            _ => None,
        };
        self.view = View::Categories;
        self.scroll = 0;
        self.selected = self.rows().iter().position(|r| matches!(r, Row::Category(c, _) if Some(*c) == from)).unwrap_or(0);
    }

    /// As when the menu is opened anew.
    pub fn reset(&mut self) {
        self.view = View::Categories;
        self.query.clear();
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn set_query(&mut self, query: String) {
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
    use std::path::PathBuf;

    pub(crate) fn sample() -> App {
        let entry = |name: &str, category| Entry { name: name.into(), path: PathBuf::from(name), category, exec: Vec::new() };
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

    #[test]
    fn categories_then_their_programs_and_back() {
        let mut app = sample();
        assert_eq!(
            app.rows(),
            [Row::Category(Category::Internet, 3), Row::Category(Category::Multimedia, 1), Row::Category(Category::Other, 1)]
        );
        app.open_category(Category::Multimedia);
        assert_eq!(app.rows(), [Row::Back(Category::Multimedia), Row::Entry(0)]);
        assert_eq!(app.selected, 1);
        app.back();
        assert_eq!((app.view, app.selected), (View::Categories, 1));
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
