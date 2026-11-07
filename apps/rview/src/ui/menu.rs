//! Menus drawn in the window, for systems without Win32 menus. They come from
//! the same menu tree. `main` feeds them input.

use super::gfx::{Align, Color, FontId, Gfx, Rect};
use super::theme::Theme;
use crate::commands::{shortcut, Command, M};

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
    let (mut plain, mut mnemonic, mut mark) = (String::new(), None, None);
    let chars: Vec<char> = label.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match (chars[i], chars.get(i + 1)) {
            ('&', Some('&')) => {
                plain.push('&');
                i += 2;
            }
            // "ファイル(&F)" has the key outside the text.
            ('(', Some('&')) if chars.get(i + 3) == Some(&')') => {
                mnemonic = chars[i + 2].to_lowercase().next();
                i += 4;
            }
            ('&', Some(k)) => {
                if mnemonic.is_none() {
                    mnemonic = k.to_lowercase().next();
                    mark = Some(plain.chars().count());
                }
                i += 1;
            }
            (c, _) => {
                plain.push(c);
                i += 1;
            }
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
                M::Sub(title, children) => (crate::i18n::tr(title), None, build(children, checked, enabled), false),
                M::Sep => (String::new(), None, Vec::new(), true),
            };
            let (label, mnemonic, mark) = parse_label(&text);
            Item { label, mark, mnemonic, shortcut: cmd.and_then(shortcut), checked: cmd.is_some_and(checked), enabled: cmd.is_none_or(enabled), cmd, sub, sep }
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
    /// Title under the pointer.
    pub hot: Option<usize>,
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

    pub fn title_at(&self, x: f32, y: f32) -> Option<usize> {
        self.titles.iter().position(|t| t.rect.contains(x, y))
    }

    /// The row under `(x, y)`, in the topmost popup there.
    pub fn row_at(&self, x: f32, y: f32) -> Option<(usize, usize)> {
        let l = self.levels.iter().rposition(|l| l.rect.contains(x, y))?;
        self.levels[l].rows.iter().find(|(_, r)| r.contains(x, y)).map(|(i, _)| (l, *i))
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

    // ------------------------------------------------------------ painting

    pub fn draw_bar(&mut self, g: &mut Gfx, r: Rect, th: &Theme, s: f32) {
        g.fill_rect(r, th.status);
        g.fill_rect(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), th.border);
        let px = 13.0 * s;
        let (asc, desc) = g.metrics(px, FontId::Regular);
        let base = r.center_y() + (asc + desc) / 2.0;
        let mut x = r.x + 4.0 * s;
        for (i, title) in self.titles.iter_mut().enumerate() {
            let w = g.measure(&title.label, px, FontId::Regular) + 18.0 * s;
            let rect = Rect::new(x, r.y + 2.0 * s, w, r.h - 4.0 * s);
            title.rect = rect;
            if self.top == Some(i) || self.hot == Some(i) {
                g.fill_rrect(rect, 4.0 * s, th.dim.mix(th.status, 0.75));
            }
            let tx = rect.x + 9.0 * s;
            g.text(&title.label, tx, base, px, FontId::Regular, th.text);
            underline(g, &title.label, title.mark, tx, base, px, s, th.text);
            x += w;
        }
    }

    /// Paints the open popups, each on top of its parent.
    pub fn draw_popups(&mut self, g: &mut Gfx, full: Rect, th: &Theme, s: f32) {
        let px = 13.0 * s;
        let (asc, desc) = g.metrics(px, FontId::Regular);
        let (row_h, sep_h, pad, side) = ((26.0 * s).round(), (9.0 * s).round(), (4.0 * s).round(), (28.0 * s).round());
        let height = |sep: bool| if sep { sep_h } else { row_h };
        let bg = if th.dark { Color::rgb(0x2B2B2B) } else { Color::rgb(0xFFFFFF) };
        let selected = Color::rgba(0x3390FF, 0x48);
        let mut parent: Option<Rect> = None;
        for level in &mut self.levels {
            // Size and position: the popup stays inside the window.
            let mut width = 0.0f32;
            for it in &level.items {
                let keys = it.shortcut.map_or(0.0, |k| g.measure(k, 12.0 * s, FontId::Regular) + 28.0 * s);
                width = width.max(g.measure(&it.label, px, FontId::Regular) + keys);
            }
            let w = (width + 2.0 * side).min(full.w - 2.0 * pad);
            let total: f32 = level.items.iter().map(|i| height(i.sep)).sum::<f32>() + 2.0 * pad;
            let h = total.min(full.h - 2.0 * pad);
            let (mut x, mut y) = level.anchor;
            if x + w > full.w - pad {
                x = parent.map_or(full.w - pad - w, |p| p.x - w).max(pad);
            }
            if y + h > full.h - pad {
                y = (full.h - pad - h).max(pad);
            }
            let rect = Rect::new(x.round(), y.round(), w, h);
            level.rect = rect;
            parent = Some(rect);
            // Keep the selected row in view.
            if let Some(sel) = level.sel {
                level.scroll = level.scroll.min(sel);
                while level.scroll < sel && level.items[level.scroll..=sel].iter().map(|i| height(i.sep)).sum::<f32>() > h - 2.0 * pad {
                    level.scroll += 1;
                }
            }
            let rad = 8.0 * s;
            for i in 1..=4 {
                let d = i as f32 * s;
                g.fill_rrect(Rect::new(rect.x - d * 0.5, rect.y + d * 0.5, rect.w + d, rect.h + d), rad + d, Color::rgba(0x000000, if th.dark { 0x20 } else { 0x10 }));
            }
            g.fill_rrect(rect, rad, bg);
            g.stroke_rrect(rect, rad, 1.0, th.border);
            g.set_clip(Some(rect));
            level.rows.clear();
            let mut ry = rect.y + pad;
            for i in level.scroll..level.items.len() {
                let it = &level.items[i];
                let rh = height(it.sep);
                if ry + rh > rect.bottom() - pad + 0.5 {
                    break;
                }
                let row = Rect::new(rect.x + pad, ry, rect.w - 2.0 * pad, rh);
                ry += rh;
                if it.sep {
                    g.fill_rect(Rect::new(row.x + 6.0 * s, row.center_y().round(), row.w - 12.0 * s, 1.0), th.border);
                    continue;
                }
                level.rows.push((i, row));
                if it.enabled && level.sel == Some(i) {
                    g.fill_rrect(row, 4.0 * s, selected);
                }
                let c = if it.enabled { th.text } else { th.dim.mix(bg, 0.5) };
                let base = row.center_y() + (asc + desc) / 2.0;
                if it.checked {
                    g.text("✓", row.x + 6.0 * s, base, px, FontId::Regular, c);
                }
                let tx = rect.x + side;
                g.text(&it.label, tx, base, px, FontId::Regular, c);
                underline(g, &it.label, it.mark, tx, base, px, s, c);
                let right = Rect::new(row.x, row.y, row.w - 10.0 * s, row.h);
                if !it.sub.is_empty() || (level.sel == Some(i) && it.cmd.is_none()) {
                    right_text(g, "›", right, 15.0 * s, c);
                } else if let Some(k) = it.shortcut {
                    right_text(g, k, right, 12.0 * s, th.dim);
                }
            }
            g.set_clip(None);
        }
    }
}

fn right_text(g: &mut Gfx, text: &str, r: Rect, px: f32, c: Color) {
    let w = g.measure(text, px, FontId::Regular);
    g.text_in(text, Rect::new(r.right() - w, r.y, w, r.h), px, FontId::Regular, c, Align::Left);
}

#[allow(clippy::too_many_arguments)]
fn underline(g: &mut Gfx, text: &str, mark: Option<usize>, x: f32, base: f32, px: f32, s: f32, c: Color) {
    let Some(i) = mark else { return };
    let before: String = text.chars().take(i).collect();
    let letter: String = text.chars().skip(i).take(1).collect();
    let x0 = x + g.measure(&before, px, FontId::Regular);
    let w = g.measure(&letter, px, FontId::Regular);
    g.fill_rect(Rect::new(x0, base + 2.0 * s, w, s.round().max(1.0)), c);
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
        more.sub = vec![item("&Inner", Some(Command::About), true)];
        let items = vec![item("&Open", Some(Command::Open), true), item("&Gone", Some(Command::Reload), false), sep(), more];
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
    }

    #[test]
    fn keyboard_skips_separators_and_disabled_items() {
        let mut m = menu();
        assert_eq!(m.levels[0].sel, Some(0));
        m.up_down(1);
        assert_eq!(m.levels[0].sel, Some(3));
        m.up_down(1);
        assert_eq!(m.levels[0].sel, Some(0));
    }

    #[test]
    fn submenus_and_pointer() {
        let mut m = menu();
        assert_eq!(m.right(), Act::Switch(1), "no submenu on the selected item");
        m.up_down(-1);
        assert_eq!(m.right(), Act::None);
        assert_eq!(m.enter(), Act::Run(Command::About));
        assert_eq!(m.left(), Act::None);
        assert_eq!(m.levels[0].items[3].sub.len(), 1, "the submenu is given back");
        assert_eq!(m.mnemonic('o'), Act::Run(Command::Open));
        assert_eq!(m.click(0, 1), Act::None, "disabled");
        m.hover(0, 3);
        assert_eq!(m.levels.len(), 2);
        m.hover(0, 0);
        assert_eq!(m.levels.len(), 1);
        m.back();
        assert!(!m.is_open());
    }
}
