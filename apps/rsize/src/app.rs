//! What the window shows, apart from the window itself: the page, the
//! scanned tree, which folders are open in the list, the selection and the
//! folder the treemap shows. Everything here can be tested without a window.

use crate::model::{Id, SizeMode};
use crate::scan::{Progress, Scan};
use crate::volumes::Volume;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

/// A scan in progress.
pub struct Scanning {
    pub root: PathBuf,
    pub progress: Progress,
    pub started: Instant,
    /// Increases with each scan, so late events of an old one are ignored.
    pub generation: u64,
}

#[derive(Default)]
pub struct App {
    /// The drives, once read (None: still looking).
    pub volumes: Option<Vec<Volume>>,
    pub scanning: Option<Scanning>,
    /// The last result.
    pub scan: Option<Scan>,
    /// The list of drives is shown instead of the result.
    pub show_drives: bool,
    /// Why the last scan could not start.
    pub error: Option<String>,
    /// Open folders in the list.
    pub expanded: HashSet<Id>,
    /// The list's rows: each node shown and its depth.
    pub rows: Vec<(Id, u16)>,
    pub sel: Option<Id>,
    /// The folder the treemap shows.
    pub zoom: Id,
    /// First row shown in the list.
    pub scroll: usize,
    /// Rows that fit in the list (set by the view).
    pub page: usize,
    /// How far the start page is scrolled, in pixels.
    pub start_scroll: f32,
}

impl App {
    pub fn new() -> App {
        App { show_drives: true, page: 20, ..App::default() }
    }

    /// Shows a finished scan.
    pub fn set_scan(&mut self, mut scan: Scan, mode: SizeMode) {
        scan.tree.sort(mode);
        let root = scan.tree.root();
        self.scan = Some(scan);
        self.show_drives = false;
        self.error = None;
        self.expanded = HashSet::from([root]);
        self.zoom = root;
        self.sel = Some(root);
        self.scroll = 0;
        self.rebuild();
    }

    pub fn has_tree(&self) -> bool {
        self.scan.is_some()
    }

    /// Lists the rows again after folders were opened or closed, or the order changed.
    pub fn rebuild(&mut self) {
        self.rows.clear();
        let Some(scan) = &self.scan else { return };
        let t = &scan.tree;
        let mut stack = vec![(t.root(), 0u16)];
        while let Some((id, depth)) = stack.pop() {
            self.rows.push((id, depth));
            if self.expanded.contains(&id) {
                stack.extend(t.children(id).iter().rev().map(|&c| (c, depth + 1)));
            }
        }
        self.scroll = self.scroll.min(self.rows.len().saturating_sub(1));
    }

    pub fn set_mode(&mut self, mode: SizeMode) {
        if let Some(s) = &mut self.scan {
            s.tree.sort(mode);
        }
        self.rebuild();
        self.keep_visible();
    }

    fn row_of(&self, id: Id) -> Option<usize> {
        self.rows.iter().position(|&(r, _)| r == id)
    }

    pub fn can_expand(&self, id: Id) -> bool {
        self.scan.as_ref().is_some_and(|s| !s.tree.children(id).is_empty())
    }

    pub fn toggle(&mut self, id: Id) {
        if !self.expanded.remove(&id) && self.can_expand(id) {
            self.expanded.insert(id);
        }
        self.rebuild();
    }

    /// Selects `id`, opening the folders above it and scrolling to it.
    pub fn select(&mut self, id: Id) {
        let Some(scan) = &self.scan else { return };
        let mut p = scan.tree.parent(id);
        let mut changed = false;
        while let Some(d) = p {
            changed |= self.expanded.insert(d);
            p = scan.tree.parent(d);
        }
        if changed {
            self.rebuild();
        }
        self.sel = Some(id);
        self.keep_visible();
    }

    /// Scrolls so that the selection is in view.
    pub fn keep_visible(&mut self) {
        let Some(i) = self.sel.and_then(|s| self.row_of(s)) else { return };
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

    /// Moves the selection `delta` rows (page keys: a page).
    pub fn move_sel(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let cur = self.sel.and_then(|s| self.row_of(s)).unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, self.rows.len() as isize - 1) as usize;
        self.sel = Some(self.rows[next].0);
        self.keep_visible();
    }

    /// Left arrow: closes the folder, or goes to its parent.
    pub fn left(&mut self) {
        let Some(id) = self.sel else { return };
        if self.expanded.contains(&id) {
            self.expanded.remove(&id);
            self.rebuild();
            self.keep_visible();
        } else if let Some(p) = self.scan.as_ref().and_then(|s| s.tree.parent(id)) {
            self.select(p);
        }
    }

    /// Right arrow: opens the folder, or goes to its first child.
    pub fn right(&mut self) {
        let Some(id) = self.sel else { return };
        if !self.can_expand(id) {
            return;
        }
        if self.expanded.insert(id) {
            self.rebuild();
        } else if let Some(&c) = self.scan.as_ref().and_then(|s| s.tree.children(id).first()) {
            self.select(c);
        }
    }

    /// The folder to zoom into for the selection: itself, or a file's folder.
    fn folder_of_selection(&self) -> Option<Id> {
        let scan = self.scan.as_ref()?;
        let id = self.sel?;
        if scan.tree.node(id).is_dir() && !scan.tree.children(id).is_empty() {
            Some(id)
        } else {
            scan.tree.parent(id)
        }
    }

    pub fn zoom_in(&mut self) {
        if let Some(d) = self.folder_of_selection() {
            self.zoom = d;
        }
    }

    pub fn zoom_out(&mut self) {
        if let Some(p) = self.scan.as_ref().and_then(|s| s.tree.parent(self.zoom)) {
            self.zoom = p;
        }
    }

    pub fn zoom_top(&mut self) {
        if let Some(s) = &self.scan {
            self.zoom = s.tree.root();
        }
    }

    /// The full path of the selection.
    pub fn selected_path(&self) -> Option<PathBuf> {
        Some(self.scan.as_ref()?.tree.path(self.sel?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entry, Kind, Tree};
    use crate::scan::Method;
    use std::time::Duration;

    fn app() -> App {
        let f = |n: &str, s: u64| Entry { name: n.into(), kind: Kind::File, size: s, alloc: s, mtime: 0, flags: 0 };
        let d = |n: &str| Entry { name: n.into(), kind: Kind::Dir, size: 0, alloc: 0, mtime: 0, flags: 0 };
        let mut t = Tree::new("/r", 0);
        // 1 a (dir), 2 b (dir), 3 c.txt
        t.add_children(0, &[d("a"), d("b"), f("c.txt", 5)]);
        // 4..6 in a
        t.add_children(1, &[f("a1", 100), f("a2", 50), d("a3")]);
        t.add_children(6, &[f("deep", 1)]);
        t.add_children(2, &[f("b1", 10)]);
        t.finish();
        let mut a = App::new();
        a.page = 3;
        a.set_scan(Scan { tree: t, method: Method::System, elapsed: Duration::ZERO, complete: true }, SizeMode::Logical);
        a
    }

    fn names(a: &App) -> Vec<String> {
        let t = &a.scan.as_ref().unwrap().tree;
        a.rows.iter().map(|&(id, d)| format!("{}{}", " ".repeat(d as usize), t.name(id))).collect()
    }

    #[test]
    fn rows_follow_open_folders() {
        let mut a = app();
        assert_eq!(names(&a), ["/r", " a", " b", " c.txt"]);
        a.toggle(1);
        assert_eq!(names(&a), ["/r", " a", "  a1", "  a2", "  a3", " b", " c.txt"]);
        a.toggle(1);
        assert_eq!(a.rows.len(), 4);
        a.toggle(3); // a file: nothing to open
        assert_eq!(a.rows.len(), 4);
    }

    #[test]
    fn selecting_deep_opens_and_scrolls() {
        let mut a = app();
        a.select(7);
        assert_eq!(a.sel, Some(7));
        assert!(a.expanded.contains(&1) && a.expanded.contains(&6));
        let row = a.rows.iter().position(|&(r, _)| r == 7).unwrap();
        assert!(row >= a.scroll && row < a.scroll + a.page);
        assert_eq!(a.selected_path(), Some(PathBuf::from("/r").join("a").join("a3").join("deep")));
    }

    #[test]
    fn keyboard_moves() {
        let mut a = app();
        a.move_sel(1);
        assert_eq!(a.sel, Some(1));
        a.right();
        assert!(a.expanded.contains(&1));
        a.right();
        assert_eq!(a.sel, Some(4), "first child");
        a.left();
        assert_eq!(a.sel, Some(1), "a file goes to its parent");
        a.left();
        assert!(!a.expanded.contains(&1));
        a.move_sel(100);
        assert_eq!(a.sel, Some(3));
        a.move_sel(-100);
        assert_eq!(a.sel, Some(0));
    }

    #[test]
    fn zoom_follows_the_selection() {
        let mut a = app();
        a.select(4);
        a.zoom_in();
        assert_eq!(a.zoom, 1, "a file zooms into its folder");
        a.select(6);
        a.zoom_in();
        assert_eq!(a.zoom, 6);
        a.zoom_out();
        assert_eq!(a.zoom, 1);
        a.zoom_top();
        assert_eq!(a.zoom, 0);
        a.zoom_out();
        assert_eq!(a.zoom, 0);
    }
}
