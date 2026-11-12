//! What the window shows, apart from the window itself: the page, the
//! scanned tree, which folders are open in the list, the selection and the
//! folder the treemap shows. Everything here can be tested without a window.

use crate::i18n::tf;
use crate::model::{ExtStat, Id, Node, SizeMode, Tree};
use crate::scan::{Progress, Scan};
use crate::volumes::{Space, Volume};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Instant;

/// The columns of the folder list, each of which can sort it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Col {
    Name,
    /// The share of the folder: the size in the current mode.
    #[default]
    Share,
    Size,
    Alloc,
    /// Files and folders below.
    Items,
    Files,
    Folders,
    Modified,
}

impl Col {
    pub const ALL: [Col; 8] = [Col::Name, Col::Share, Col::Size, Col::Alloc, Col::Items, Col::Files, Col::Folders, Col::Modified];
}

/// The columns of the table of file types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ExtCol {
    Ext,
    Type,
    /// The share of the folder: the size in the current mode.
    #[default]
    Share,
    Size,
    Alloc,
    Files,
}

impl ExtCol {
    pub const ALL: [ExtCol; 6] = [ExtCol::Ext, ExtCol::Type, ExtCol::Share, ExtCol::Size, ExtCol::Alloc, ExtCol::Files];
}

/// A column and its direction (true: largest first, or Z to A).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort<C> {
    pub col: C,
    pub desc: bool,
}

impl<C: Copy + PartialEq + Default> Default for Sort<C> {
    fn default() -> Self {
        Sort { col: C::default(), desc: true }
    }
}

impl<C: Copy + PartialEq> Sort<C> {
    /// A click on a column's header: the same column turns the order round;
    /// another starts with `first_desc`.
    pub fn click(&mut self, col: C, first_desc: bool) {
        if self.col == col {
            self.desc = !self.desc;
        } else {
            *self = Sort { col, desc: first_desc };
        }
    }
}

/// Files and folders below a node (1 for a file).
pub fn items(n: &Node) -> u64 {
    if n.is_dir() {
        n.files + n.dirs
    } else {
        1
    }
}

/// Compares two nodes by a column, smallest (or A) first.
pub fn compare(t: &Tree, a: Id, b: Id, col: Col) -> Ordering {
    let (na, nb) = (t.node(a), t.node(b));
    match col {
        Col::Name => natural(t.name(a), t.name(b)),
        Col::Share => t.size(a).cmp(&t.size(b)),
        Col::Size => na.size.cmp(&nb.size),
        Col::Alloc => na.alloc.cmp(&nb.alloc),
        Col::Items => items(na).cmp(&items(nb)),
        Col::Files => na.files.cmp(&nb.files),
        Col::Folders => na.dirs.cmp(&nb.dirs),
        Col::Modified => na.mtime.cmp(&nb.mtime),
    }
}

/// Names as people read them: without regard to case.
fn natural(a: &str, b: &str) -> Ordering {
    let lower = |s: &str| s.chars().flat_map(char::to_lowercase).collect::<String>();
    lower(a).cmp(&lower(b)).then_with(|| a.cmp(b))
}

/// What a file type is called: the system's name for it, or "EXT File".
pub fn type_name(names: &HashMap<String, String>, ext: &str) -> String {
    if ext.is_empty() {
        return String::new();
    }
    names.get(ext).cloned().unwrap_or_else(|| tf("{} File", &[&ext.to_uppercase()]))
}

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
    /// How the list's folders order their contents.
    pub sort: Sort<Col>,
    /// The table of file types of the folder the chart shows, in its order.
    pub exts: Vec<(u32, ExtStat)>,
    pub ext_sort: Sort<ExtCol>,
    /// The system's names of the file types, by extension (lower case).
    pub type_names: HashMap<String, String>,
    /// The selected row of the table of file types (an extension id).
    pub ext_sel: Option<u32>,
    /// First row shown in the table of file types.
    pub types_scroll: usize,
    /// Rows that fit in the table of file types (set by the view).
    pub types_page: usize,
    /// The volume the scanned folder is on.
    pub space: Option<Space>,
    /// A message for the status bar (an export, a deletion).
    pub note: Option<String>,
}

impl App {
    pub fn new() -> App {
        App { show_drives: true, page: 20, types_page: 10, ..App::default() }
    }

    /// Fills the table of file types for the folder the chart shows.
    pub fn refresh_exts(&mut self) {
        let Some(scan) = &self.scan else {
            self.exts.clear();
            return;
        };
        self.exts = scan.tree.ext_totals(self.zoom);
        self.types_scroll = 0;
        // The type picked stays picked while the folder shown has some.
        if self.ext_sel.is_some_and(|e| !self.exts.iter().any(|x| x.0 == e)) {
            self.ext_sel = None;
        }
        self.sort_exts();
    }

    /// Orders the table of file types by its column.
    pub fn sort_exts(&mut self) {
        let Some(scan) = &self.scan else { return };
        let t = &scan.tree;
        let mode = t.mode();
        let names = &self.type_names;
        let Sort { col, desc } = self.ext_sort;
        let in_mode = |s: &ExtStat| match mode {
            SizeMode::Logical => s.size,
            SizeMode::Allocated => s.alloc,
        };
        self.exts.sort_by(|a, b| {
            let o = match col {
                ExtCol::Ext => natural(t.ext_name(a.0), t.ext_name(b.0)),
                ExtCol::Type => natural(&type_name(names, t.ext_name(a.0)), &type_name(names, t.ext_name(b.0))),
                ExtCol::Share => in_mode(&a.1).cmp(&in_mode(&b.1)),
                ExtCol::Size => a.1.size.cmp(&b.1.size),
                ExtCol::Alloc => a.1.alloc.cmp(&b.1.alloc),
                ExtCol::Files => a.1.files.cmp(&b.1.files),
            };
            let o = if desc { o.reverse() } else { o };
            o.then_with(|| in_mode(&b.1).cmp(&in_mode(&a.1))).then(a.0.cmp(&b.0))
        });
    }

    /// A click on a header of the table of file types.
    pub fn sort_exts_by(&mut self, col: ExtCol) {
        self.ext_sort.click(col, !matches!(col, ExtCol::Ext | ExtCol::Type));
        self.sort_exts();
    }

    /// A click on a header of the list.
    pub fn sort_by(&mut self, col: Col) {
        self.sort.click(col, !matches!(col, Col::Name));
        self.rebuild();
        self.keep_visible();
    }

    pub fn scroll_types_by(&mut self, rows: isize) {
        let max = self.exts.len().saturating_sub(self.types_page.max(1));
        self.types_scroll = (self.types_scroll as isize + rows).clamp(0, max as isize) as usize;
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
        self.types_scroll = 0;
        self.ext_sel = None;
        self.note = None;
        self.rebuild();
        self.refresh_exts();
    }

    /// Takes a deleted item out of the tree and the views; the selection
    /// goes to its folder.
    pub fn removed(&mut self, id: Id) -> bool {
        let Some(scan) = &mut self.scan else { return false };
        let Some(parent) = scan.tree.parent(id) else { return false };
        if scan.tree.is_within(self.zoom, id) {
            self.zoom = parent;
        }
        if !scan.tree.remove(id) {
            return false;
        }
        self.expanded.remove(&id);
        self.rebuild();
        self.select(parent);
        self.refresh_exts();
        true
    }

    pub fn has_tree(&self) -> bool {
        self.scan.is_some()
    }

    /// Lists the rows again after folders were opened or closed, or the order changed.
    pub fn rebuild(&mut self) {
        self.rows.clear();
        let Some(scan) = &self.scan else { return };
        let t = &scan.tree;
        let Sort { col, desc } = self.sort;
        // The tree keeps each folder's contents largest first in the current
        // mode: other orders are made for the open folders only.
        let as_tree = col == Col::Share && desc;
        let mut kids = Vec::new();
        let mut stack = vec![(t.root(), 0u16)];
        while let Some((id, depth)) = stack.pop() {
            self.rows.push((id, depth));
            if self.expanded.contains(&id) {
                kids.clear();
                kids.extend_from_slice(t.children(id));
                if !as_tree {
                    kids.sort_by(|&a, &b| {
                        let o = compare(t, a, b, col);
                        let o = if desc { o.reverse() } else { o };
                        o.then_with(|| t.size(b).cmp(&t.size(a))).then(a.cmp(&b))
                    });
                }
                stack.extend(kids.iter().rev().map(|&c| (c, depth + 1)));
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
        self.sort_exts();
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
        } else if let Some(&(c, _)) = self.row_of(id).and_then(|i| self.rows.get(i + 1)) {
            // The first row inside it, in the list's order.
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

    /// The chart shows the folder above; the list selects it, as when going down.
    pub fn zoom_out(&mut self) {
        if let Some(p) = self.scan.as_ref().and_then(|s| s.tree.parent(self.zoom)) {
            self.zoom = p;
            self.select(p);
        }
    }

    pub fn zoom_top(&mut self) {
        if let Some(root) = self.scan.as_ref().map(|s| s.tree.root()) {
            self.zoom = root;
            self.select(root);
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
        a.set_scan(Scan { tree: t, method: Method::System, elapsed: Duration::ZERO, complete: true, admin_would_help: false }, SizeMode::Logical);
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
    fn a_deleted_folder_leaves_the_list() {
        let mut a = app();
        a.select(6);
        a.zoom_in();
        assert_eq!(a.zoom, 6);
        assert!(a.removed(6));
        assert_eq!(a.zoom, 1, "the chart shows the folder above");
        assert_eq!(a.sel, Some(1));
        assert!(!a.rows.iter().any(|&(r, _)| r == 6 || r == 7));
        assert_eq!(a.scan.as_ref().unwrap().tree.node(0).size, 165);
        assert!(!a.removed(0));
    }

    #[test]
    fn every_column_sorts_the_list() {
        let mut a = app();
        a.toggle(1);
        let kids = |a: &App| -> Vec<String> {
            let t = &a.scan.as_ref().unwrap().tree;
            a.rows.iter().filter(|r| r.1 == 2).map(|&(id, _)| t.name(id).to_string()).collect()
        };
        assert_eq!(kids(&a), ["a1", "a2", "a3"], "largest first");
        a.sort_by(Col::Name);
        assert_eq!(kids(&a), ["a1", "a2", "a3"]);
        a.sort_by(Col::Name);
        assert_eq!(kids(&a), ["a3", "a2", "a1"], "a second click turns it round");
        a.sort_by(Col::Size);
        assert!(a.sort.desc);
        assert_eq!(kids(&a), ["a1", "a2", "a3"]);
        a.sort_by(Col::Items);
        assert_eq!(kids(&a), ["a1", "a2", "a3"], "1 item each: then by size");
        a.sort_by(Col::Folders);
        assert_eq!(kids(&a), ["a1", "a2", "a3"], "no folders below any: then by size");
        let top: Vec<String> = {
            a.sort_by(Col::Files);
            let t = &a.scan.as_ref().unwrap().tree;
            a.rows.iter().filter(|r| r.1 == 1).map(|&(id, _)| t.name(id).to_string()).collect()
        };
        assert_eq!(top, ["a", "b", "c.txt"], "a holds 3 files, b 1, c.txt is one");
        for col in Col::ALL {
            a.sort_by(col);
            assert_eq!(a.rows.len(), 7, "{col:?} keeps every row");
        }
        // Right arrow goes to the first row inside, in the list's order.
        a.sort = Sort { col: Col::Name, desc: true };
        a.rebuild();
        a.select(1);
        a.right();
        assert_eq!(a.sel, Some(6), "a3 is first from Z to A");
    }

    #[test]
    fn the_table_of_file_types_sorts() {
        let mut a = app();
        let names = |a: &App| -> Vec<String> {
            let t = &a.scan.as_ref().unwrap().tree;
            a.exts.iter().map(|e| t.ext_name(e.0).to_string()).collect()
        };
        assert_eq!(names(&a), ["", "txt"], "no extension: 161 bytes, txt: 5");
        a.sort_exts_by(ExtCol::Ext);
        assert_eq!(names(&a), ["", "txt"]);
        a.sort_exts_by(ExtCol::Ext);
        assert_eq!(names(&a), ["txt", ""]);
        a.sort_exts_by(ExtCol::Files);
        assert_eq!(names(&a), ["", "txt"], "4 files without one, 1 txt");
        a.type_names.insert("txt".into(), "Text Document".into());
        assert_eq!(type_name(&a.type_names, "txt"), "Text Document");
        assert_eq!(type_name(&a.type_names, "xyz"), "XYZ File");
        assert_eq!(type_name(&a.type_names, ""), "");
        for col in ExtCol::ALL {
            a.sort_exts_by(col);
            assert_eq!(a.exts.len(), 2);
        }
        a.select(6);
        a.zoom_in();
        a.refresh_exts();
        assert_eq!(names(&a), [""], "only the files below the folder shown");
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
        assert_eq!(a.sel, Some(1), "the list follows the chart up");
        a.zoom_top();
        assert_eq!(a.zoom, 0);
        assert_eq!(a.sel, Some(0));
        a.zoom_out();
        assert_eq!(a.zoom, 0);
    }
}
