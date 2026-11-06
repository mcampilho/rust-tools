//! Menus drawn in the window, for systems without Win32 menus. They come from
//! the same command table: this module holds the state and the navigation,
//! `view` paints them and `main` feeds them input.

use super::gfx::Rect;
use crate::app::commands::{shortcut, Command, M};

pub struct Item {
    pub label: String,
    /// Index of the character to underline in `label`.
    pub mark: Option<usize>,
    pub mnemonic: Option<char>,
    pub shortcut: Option<&'static str>,
    pub cmd: Option<Command>,
    pub sub: Vec<Item>,
    pub sep: bool,
    pub checked: bool,
    pub enabled: bool,
}

/// Splits a label with `&` marks into its text, access key and marked index.
pub fn parse_label(label: &str) -> (String, Option<char>, Option<usize>) {
    let plain = crate::i18n::strip_mnemonic(label);
    let (mut mnemonic, mut mark, mut shown) = (None, None, 0);
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            shown += 1;
            continue;
        }
        match chars.next() {
            Some('&') => shown += 1,
            Some(k) if mnemonic.is_none() => {
                mnemonic = k.to_lowercase().next();
                // "ファイル(&F)" has the key outside the text.
                mark = (plain.chars().nth(shown) == Some(k)).then_some(shown);
                shown += 1;
            }
            Some(_) => shown += 1,
            None => {}
        }
    }
    (plain, mnemonic, mark)
}

/// Builds the menu items, with their state as of now.
pub fn build(items: Vec<M>, checked: &dyn Fn(Command) -> bool, enabled: &dyn Fn(Command) -> bool) -> Vec<Item> {
    items
        .into_iter()
        .map(|m| {
            let (text, cmd, sub, sep) = match m {
                M::Item(c) => (c.label(), Some(c), Vec::new(), false),
                M::Dyn(c, label) => (label, Some(c), Vec::new(), false),
                M::Sub(title, children) => (crate::i18n::tr(title), None, build(children, checked, enabled), false),
                M::Sep => (String::new(), None, Vec::new(), true),
            };
            let (label, mnemonic, mark) = parse_label(&text);
            Item {
                label,
                mark,
                mnemonic,
                shortcut: cmd.and_then(shortcut),
                checked: cmd.is_some_and(checked),
                enabled: cmd.is_none_or(enabled),
                cmd,
                sub,
                sep,
            }
        })
        .collect()
}

pub struct Title {
    pub label: String,
    pub mnemonic: Option<char>,
    pub mark: Option<usize>,
    /// Where it was last painted.
    pub rect: Rect,
}

/// One open popup.
pub struct Level {
    pub items: Vec<Item>,
    pub sel: Option<usize>,
    /// First item shown, when the popup does not fit the window.
    pub scroll: usize,
    /// Requested top-left corner.
    pub anchor: (f32, f32),
    /// Where it was last painted, and its rows as (item, rectangle).
    pub rect: Rect,
    pub rows: Vec<(usize, Rect)>,
}

impl Level {
    fn new(items: Vec<Item>, anchor: (f32, f32)) -> Level {
        Level { items, sel: None, scroll: 0, anchor, rect: Rect::new(anchor.0, anchor.1, 0.0, 0.0), rows: Vec::new() }
    }

    fn selectable(&self, i: usize) -> bool {
        let it = &self.items[i];
        !it.sep && it.enabled
    }

    fn step(&mut self, dir: isize) {
        let n = self.items.len() as isize;
        let mut i = self.sel.map_or(if dir > 0 { -1 } else { n }, |s| s as isize);
        for _ in 0..n {
            i = (i + dir).rem_euclid(n);
            if self.selectable(i as usize) {
                self.sel = Some(i as usize);
                return;
            }
        }
    }
}

/// What the caller has to do after a menu input.
#[derive(Debug, PartialEq, Eq)]
pub enum Act {
    None,
    Run(Command),
    /// Move to the top-level menu `dir` places away.
    Switch(isize),
}

#[derive(Default)]
pub struct Menu {
    pub titles: Vec<Title>,
    /// The top-level menu that is open, unless this is a context menu.
    pub top: Option<usize>,
    pub levels: Vec<Level>,
}

impl Menu {
    pub fn is_open(&self) -> bool {
        !self.levels.is_empty()
    }

    pub fn close(&mut self) {
        self.levels.clear();
        self.top = None;
    }

    pub fn open(&mut self, top: Option<usize>, items: Vec<Item>, anchor: (f32, f32), keyboard: bool) {
        self.close();
        self.top = top;
        let mut level = Level::new(items, anchor);
        if keyboard {
            level.step(1);
        }
        self.levels.push(level);
    }

    /// Whether `(x, y)` is on an open popup, including its disabled rows.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.levels.iter().any(|l| l.rect.contains(x, y))
    }

    /// Opens the submenu of the selected item of level `l`.
    fn open_sub(&mut self, l: usize, keyboard: bool) {
        let level = &mut self.levels[l];
        let Some(sel) = level.sel else { return };
        let items = std::mem::take(&mut level.items[sel].sub);
        if items.is_empty() {
            return;
        }
        let row = level.rows.iter().find(|(i, _)| *i == sel).map_or(level.rect, |(_, r)| *r);
        let mut sub = Level::new(items, (level.rect.right(), row.y));
        if keyboard {
            sub.step(1);
        }
        self.levels.push(sub);
    }

    /// Closes the levels above `l`, giving their items back.
    fn close_above(&mut self, l: usize) {
        while self.levels.len() > l + 1 {
            let Some(top) = self.levels.pop() else { return };
            let last = self.levels.len() - 1;
            let parent = &mut self.levels[last];
            if let Some(sel) = parent.sel {
                parent.items[sel].sub = top.items;
            }
        }
    }

    /// The pointer moved over row `i` of level `l`.
    pub fn hover(&mut self, l: usize, i: usize) {
        if self.levels.get(l).is_none_or(|lv| lv.sel == Some(i)) {
            return;
        }
        self.close_above(l);
        let level = &mut self.levels[l];
        level.sel = level.selectable(i).then_some(i);
        self.open_sub(l, false);
    }

    /// Row `i` of level `l` was clicked.
    pub fn click(&mut self, l: usize, i: usize) -> Act {
        if self.levels.get(l).is_none() {
            return Act::None;
        }
        if self.levels[l].sel != Some(i) {
            self.hover(l, i);
        }
        let Some(item) = self.levels[l].items.get(i) else { return Act::None };
        match (item.enabled, item.cmd) {
            (true, Some(c)) => Act::Run(c),
            _ => Act::None,
        }
    }

    pub fn wheel(&mut self, l: usize, rows: isize) {
        if let Some(level) = self.levels.get_mut(l) {
            let max = level.items.len().saturating_sub(level.rows.len());
            level.scroll = (level.scroll as isize + rows).clamp(0, max as isize) as usize;
        }
    }

    pub fn up_down(&mut self, dir: isize) {
        if let Some(level) = self.levels.last_mut() {
            level.step(dir);
        }
    }

    pub fn left(&mut self) -> Act {
        if self.levels.len() > 1 {
            self.close_above(self.levels.len() - 2);
            Act::None
        } else if self.top.is_some() {
            Act::Switch(-1)
        } else {
            Act::None
        }
    }

    pub fn right(&mut self) -> Act {
        let l = self.levels.len().saturating_sub(1);
        let has_sub = self.levels.last().is_some_and(|lv| lv.sel.is_some_and(|s| !lv.items[s].sub.is_empty()));
        if has_sub {
            self.open_sub(l, true);
            Act::None
        } else if self.top.is_some() {
            Act::Switch(1)
        } else {
            Act::None
        }
    }

    pub fn enter(&mut self) -> Act {
        let l = self.levels.len().saturating_sub(1);
        let Some(level) = self.levels.last() else { return Act::None };
        let Some(sel) = level.sel else { return Act::None };
        let item = &level.items[sel];
        if !item.sub.is_empty() {
            self.open_sub(l, true);
            return Act::None;
        }
        match (item.enabled, item.cmd) {
            (true, Some(c)) => Act::Run(c),
            _ => Act::None,
        }
    }

    /// Esc: closes one level.
    pub fn back(&mut self) {
        if self.levels.len() > 1 {
            self.close_above(self.levels.len() - 2);
        } else {
            self.close();
        }
    }

    /// An access key in the deepest popup.
    pub fn mnemonic(&mut self, c: char) -> Act {
        let Some(level) = self.levels.last_mut() else { return Act::None };
        let key = c.to_lowercase().next();
        let Some(i) = (0..level.items.len()).find(|i| level.selectable(*i) && level.items[*i].mnemonic == key) else { return Act::None };
        level.sel = Some(i);
        self.enter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, cmd: Option<Command>, enabled: bool) -> Item {
        let (label, mnemonic, mark) = parse_label(label);
        Item { label, mnemonic, mark, shortcut: None, cmd, sub: Vec::new(), sep: false, checked: false, enabled }
    }

    fn sep() -> Item {
        Item { label: String::new(), mnemonic: None, mark: None, shortcut: None, cmd: None, sub: Vec::new(), sep: true, checked: false, enabled: true }
    }

    fn menu() -> Menu {
        let mut more = item("&More", None, true);
        more.sub = vec![item("&Inner", Some(Command::Redo), true)];
        let items = vec![item("&Open", Some(Command::Open), true), item("&Gone", Some(Command::Save), false), sep(), more];
        let mut m = Menu::default();
        m.open(Some(1), items, (0.0, 0.0), true);
        m
    }

    #[test]
    fn labels() {
        assert_eq!(parse_label("&File"), ("File".to_string(), Some('f'), Some(0)));
        assert_eq!(parse_label("Sa&ve && go"), ("Save & go".to_string(), Some('v'), Some(2)));
        let (text, key, mark) = parse_label("ファイル(&F)");
        assert_eq!((text.as_str(), key, mark), ("ファイル", Some('f'), None));
        assert_eq!(parse_label("Plain"), ("Plain".to_string(), None, None));
    }

    #[test]
    fn keyboard_skips_separators_and_disabled_items() {
        let mut m = menu();
        assert_eq!(m.levels[0].sel, Some(0));
        m.up_down(1);
        assert_eq!(m.levels[0].sel, Some(3));
        m.up_down(1);
        assert_eq!(m.levels[0].sel, Some(0));
        m.up_down(-1);
        assert_eq!(m.levels[0].sel, Some(3));
    }

    #[test]
    fn submenus_open_and_close() {
        let mut m = menu();
        assert_eq!(m.right(), Act::Switch(1), "no submenu on the selected item");
        m.up_down(-1);
        assert_eq!(m.right(), Act::None);
        assert_eq!(m.levels.len(), 2);
        assert_eq!(m.enter(), Act::Run(Command::Redo));
        assert_eq!(m.left(), Act::None);
        assert_eq!(m.levels.len(), 1);
        assert_eq!(m.levels[0].items[3].sub.len(), 1, "the submenu is given back");
        assert_eq!(m.left(), Act::Switch(-1));
    }

    #[test]
    fn access_keys_and_pointer() {
        let mut m = menu();
        assert_eq!(m.mnemonic('O'), Act::Run(Command::Open));
        assert_eq!(m.mnemonic('g'), Act::None, "disabled");
        assert_eq!(m.click(0, 1), Act::None, "disabled");
        m.hover(0, 3);
        assert_eq!(m.levels.len(), 2);
        m.hover(0, 3);
        assert_eq!(m.levels.len(), 2, "hovering again keeps it");
        m.hover(0, 0);
        assert_eq!(m.levels.len(), 1);
        assert_eq!(m.click(0, 0), Act::Run(Command::Open));
        m.back();
        assert!(!m.is_open());
    }
}
