//! The state of the window, without the window: the page, the drives and
//! folders chosen, the result and its list.

use crate::actions::{mark_by, Rule};
use crate::filter::{Choice, Field};
use crate::i18n::tr;
use std::collections::HashSet;
use crate::folders::Group as FolderGroup;
use crate::search::{Found, Progress};
use crate::volumes::Volume;
use rt_core::textedit::{boundary, TextEdit};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// A search in progress.
pub struct Searching {
    pub roots: Vec<PathBuf>,
    pub progress: Progress,
    pub started: Instant,
    pub generation: u64,
}

/// A row of the result list: the title of a group, or one of its files; or
/// the title of a group of folders, or one of its folders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Group(usize),
    /// (group, file in the group)
    File(usize, usize),
    /// Index into `App::folder_groups`.
    FolderGroup(usize),
    /// (group of folders, folder in the group)
    Folder(usize, usize),
}

/// A copy in a group: (path, last change, size).
pub type Item = (PathBuf, i64, u64);

/// What the result shows: the copies of files, or the folders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    Files,
    Folders,
}

#[derive(Default)]
pub struct App {
    /// The drives, once they have been listed.
    pub volumes: Option<Vec<Volume>>,
    /// The drives and folders to search, in the order chosen.
    pub chosen: Vec<PathBuf>,
    /// The folders added, kept for the next time whether ticked or not.
    pub saved: Vec<PathBuf>,
    /// The filters, as typed.
    pub choice: Choice,
    /// Look for duplicate files, and for duplicate folders.
    pub compare: [bool; 2],
    /// The filter box being typed in, the caret in it and the other end of
    /// the selection (byte offsets; the same when nothing is selected).
    pub focus: Option<Field>,
    pub caret: usize,
    pub anchor: usize,
    /// The calendar open beside a date box.
    pub calendar: Option<Calendar>,
    pub searching: Option<Searching>,
    pub found: Option<Found>,
    /// Files or folders, and the least share in common of the folders shown
    /// (one of `folders::SHARES`), with their groups.
    pub view: View,
    pub share: u16,
    pub folder_groups: Vec<FolderGroup>,
    /// The copies marked (files and folders), to move or recycle.
    pub marks: HashSet<PathBuf>,
    /// The start page is shown even though there is a result.
    pub show_start: bool,
    /// Why the last search gave nothing (shown on the start page).
    pub error: Option<String>,
    /// A message for the status bar.
    pub note: Option<String>,
    pub rows: Vec<Row>,
    /// The selected row.
    pub sel: Option<usize>,
    pub scroll: usize,
    /// Rows that fit in the list (set by the window).
    pub page: usize,
    /// How far the start page is scrolled.
    pub start_scroll: f32,
}

/// A calendar to pick a day for a date box: the box and the month shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Calendar {
    pub field: Field,
    pub year: i32,
    pub month: u32,
}

impl Calendar {
    /// The calendar for `field`, at the month of the date in it or else today's.
    pub fn open(field: Field, text: &str) -> Calendar {
        let (year, month) = crate::filter::parse_month(text).unwrap_or_else(|| {
            let t = rt_sys::local_time();
            (t.year, t.month)
        });
        Calendar { field, year, month }
    }

    /// The month `delta` months away.
    pub fn shift(&mut self, delta: i32) {
        let m = self.year * 12 + self.month as i32 - 1 + delta;
        self.year = m.div_euclid(12);
        self.month = m.rem_euclid(12) as u32 + 1;
    }
}

/// Whether two paths name the same folder (without case on Windows).
fn same(a: &Path, b: &Path) -> bool {
    let (a, b) = (rt_scan::normalize(a), rt_scan::normalize(b));
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

impl App {
    pub fn new() -> App {
        App { page: 1, share: crate::settings::DEFAULT_SHARE, compare: [true; 2], ..App::default() }
    }

    pub fn is_chosen(&self, path: &Path) -> bool {
        self.chosen.iter().any(|c| same(c, path))
    }

    /// Adds `path` to the search, or takes it out when it is there.
    pub fn toggle_root(&mut self, path: &Path) {
        if self.is_chosen(path) {
            self.chosen.retain(|c| !same(c, path));
        } else {
            self.chosen.push(rt_scan::normalize(path));
        }
    }

    /// The folders added (ticked or not), then any other chosen folder that
    /// is not a drive on the start page (from the command line, or kept by an
    /// earlier version).
    pub fn folders(&self) -> Vec<PathBuf> {
        let vols = self.volumes.as_deref().unwrap_or(&[]);
        let mut out = self.saved.clone();
        for c in &self.chosen {
            if !vols.iter().any(|v| same(&v.path, c)) && !out.iter().any(|f| same(f, c)) {
                out.push(c.clone());
            }
        }
        out
    }

    /// Adds a folder to the list, ticked.
    pub fn add_folder(&mut self, path: &Path) {
        if !self.saved.iter().any(|f| same(f, path)) {
            self.saved.push(rt_scan::normalize(path));
        }
        if !self.is_chosen(path) {
            self.chosen.push(rt_scan::normalize(path));
        }
    }

    /// Ticks a folder of the list, or unticks it: it stays on the list either
    /// way (also one chosen before there was a list, or on the command line).
    pub fn toggle_folder(&mut self, path: &Path) {
        if !self.saved.iter().any(|f| same(f, path)) {
            self.saved.push(rt_scan::normalize(path));
        }
        self.toggle_root(path);
    }

    /// Takes a folder off the list (and out of the search).
    pub fn remove_folder(&mut self, path: &Path) {
        self.saved.retain(|f| !same(f, path));
        self.chosen.retain(|c| !same(c, path));
    }

    // ------------------------------------------------------------ filter boxes

    /// Starts typing in `f`, with the caret at `caret` (or all selected).
    pub fn focus_field(&mut self, f: Field, caret: Option<usize>) {
        self.focus = Some(f);
        let text = self.choice.text(f);
        self.caret = caret.map_or(text.len(), |c| boundary(text, c));
        self.anchor = if caret.is_none() { 0 } else { self.caret };
    }

    /// The next (or previous) box, from the one being typed in.
    pub fn focus_next(&mut self, back: bool) {
        let n = Field::ALL.len();
        let i = match self.focus {
            Some(f) if back => (f.index() + n - 1) % n,
            Some(f) => (f.index() + 1) % n,
            None if back => n - 1,
            None => 0,
        };
        self.focus_field(Field::ALL[i], None);
    }

    /// Runs `op` on the box being typed in, as a `rt_core::textedit::TextEdit`
    /// (its text, the caret and the other end of the selection).
    fn edit<R>(&mut self, op: impl FnOnce(&mut TextEdit) -> R) -> Option<R> {
        let f = self.focus?;
        let text = std::mem::take(&mut self.choice.texts[f.index()]);
        let mut e = TextEdit { text, caret: self.caret, anchor: self.anchor };
        let r = op(&mut e);
        self.choice.texts[f.index()] = e.text;
        (self.caret, self.anchor) = (e.caret, e.anchor);
        Some(r)
    }

    /// The selected text of the box being typed in, as (start, end), when
    /// there is some.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let text = self.choice.text(self.focus?);
        let (a, b) = (boundary(text, self.anchor), boundary(text, self.caret));
        (a != b).then(|| (a.min(b), a.max(b)))
    }

    /// The selected text, or nothing.
    pub fn selected_text(&self) -> String {
        match (self.focus, self.selection()) {
            (Some(f), Some((a, b))) => self.choice.text(f)[a..b].to_string(),
            _ => String::new(),
        }
    }

    /// Types `s` at the caret, in place of the selection (line breaks become
    /// spaces; other control characters are dropped).
    pub fn type_text(&mut self, s: &str) {
        self.edit(|e| e.insert(s));
    }

    /// Deletes the selection, or else the character (`word`: the word) before
    /// the caret (`forward`: after it).
    pub fn delete(&mut self, forward: bool, word: bool) {
        self.edit(|e| e.delete(forward, word));
    }

    /// Moves the caret a character (`word`: a word) left or right, or to the
    /// start or end; `extend` keeps the other end of the selection where it is.
    pub fn move_caret(&mut self, delta: isize, to_end: bool, word: bool, extend: bool) {
        self.edit(|e| e.move_caret(delta < 0, to_end, word, extend));
    }

    /// Moves the caret to `caret` keeping the other end of the selection (a
    /// drag of the pointer, or Shift with a click).
    pub fn drag_caret(&mut self, caret: usize) {
        self.edit(|e| e.set_caret(caret, true));
    }

    /// Selects the word around `at` (a double click).
    pub fn select_word(&mut self, at: usize) {
        self.edit(|e| e.select_word(at));
    }

    pub fn select_all(&mut self) {
        self.edit(TextEdit::select_all);
    }
    pub fn set_found(&mut self, found: Found) {
        self.marks.clear();
        self.view = if found.look[0] { View::Files } else { View::Folders };
        self.found = Some(found);
        self.show_start = false;
        self.fill_rows();
    }

    /// Shows the files or the folders.
    pub fn set_view(&mut self, view: View) {
        if view != self.view {
            self.view = view;
            self.fill_rows();
        }
    }

    /// Shows the folders with at least `share` thousandths in common.
    pub fn set_share(&mut self, share: u16) {
        if share != self.share {
            self.share = share;
            self.fill_rows();
        }
    }

    /// The rows of the list for the view, with the first file or folder selected.
    fn fill_rows(&mut self) {
        self.rows.clear();
        self.folder_groups.clear();
        if let Some(found) = &self.found {
            self.folder_groups = crate::folders::groups(&found.folders, &found.pairs, self.share);
            match self.view {
                View::Files => {
                    for (g, group) in found.groups.iter().enumerate() {
                        self.rows.push(Row::Group(g));
                        self.rows.extend((0..group.files.len()).map(|i| Row::File(g, i)));
                    }
                }
                View::Folders => {
                    for (g, group) in self.folder_groups.iter().enumerate() {
                        self.rows.push(Row::FolderGroup(g));
                        self.rows.extend((0..group.folders.len()).map(|i| Row::Folder(g, i)));
                    }
                }
            }
        }
        self.sel = self.rows.iter().position(|r| matches!(r, Row::File(..) | Row::Folder(..)));
        self.scroll = 0;
        self.note = None;
    }

    pub fn has_result(&self) -> bool {
        self.found.is_some()
    }

    // ------------------------------------------------------------ marks

    /// The group and the copy of a row of the view.
    fn copy_of(&self, row: usize) -> Option<(usize, usize)> {
        match *self.rows.get(row)? {
            Row::File(g, i) | Row::Folder(g, i) => Some((g, i)),
            _ => None,
        }
    }

    /// How many groups the view has.
    fn group_count(&self) -> usize {
        match (self.view, &self.found) {
            (View::Files, Some(f)) => f.groups.len(),
            (View::Folders, Some(_)) => self.folder_groups.len(),
            _ => 0,
        }
    }

    /// The copies of group `g` of the view, as (path, last change, size), and
    /// whether they can be marked: similar folders cannot, as each holds files
    /// the other has not.
    pub fn group_items(&self, g: usize) -> Option<(Vec<Item>, bool)> {
        let f = self.found.as_ref()?;
        match self.view {
            View::Files => {
                let grp = f.groups.get(g)?;
                Some((grp.files.iter().map(|c| (c.path.clone(), c.mtime, grp.size)).collect(), true))
            }
            View::Folders => {
                let grp = self.folder_groups.get(g)?;
                let items = grp.folders.iter().map(|&i| (f.folders[i].path.clone(), f.folders[i].mtime, f.folders[i].bytes)).collect();
                Some((items, grp.identical()))
            }
        }
    }

    /// Whether the copy of a row can be marked.
    pub fn markable(&self, row: usize) -> bool {
        self.copy_of(row).and_then(|(g, _)| self.group_items(g)).is_some_and(|(_, ok)| ok)
    }

    /// Whether the copy of a row is marked.
    pub fn is_marked(&self, row: usize) -> bool {
        self.markable(row) && self.path_of(row).is_some_and(|p| self.marks.contains(&p))
    }

    /// Marks the copy of a row, or unmarks it. Err with the reason when it
    /// cannot be: the last copy of a group left unmarked, or a similar folder.
    pub fn toggle_mark(&mut self, row: usize) -> Result<(), String> {
        let Some((g, i)) = self.copy_of(row) else { return Ok(()) };
        let Some((items, ok)) = self.group_items(g) else { return Ok(()) };
        if !ok {
            return Err(tr("Only identical folders can be marked: similar ones hold files of their own."));
        }
        let p = &items[i].0;
        if self.marks.remove(p) {
            return Ok(());
        }
        if items.iter().filter(|x| !self.marks.contains(&x.0)).count() <= 1 {
            return Err(tr("One copy of each group is always kept: this is the last one left."));
        }
        self.marks.insert(p.clone());
        Ok(())
    }

    /// Marks all copies of each group of the view but the one `rule` keeps.
    /// Returns how many are marked.
    pub fn apply_rule(&mut self, rule: &Rule) -> usize {
        for g in 0..self.group_count() {
            let Some((items, true)) = self.group_items(g) else { continue };
            let refs: Vec<(&Path, i64)> = items.iter().map(|(p, t, _)| (p.as_path(), *t)).collect();
            let picked = mark_by(rule, &refs);
            for (p, _, _) in &items {
                self.marks.remove(p);
            }
            for i in picked {
                self.marks.insert(items[i].0.clone());
            }
        }
        self.marked().len()
    }

    /// Unmarks every copy of the view.
    pub fn unmark_all(&mut self) {
        for g in 0..self.group_count() {
            if let Some((items, _)) = self.group_items(g) {
                for (p, _, _) in items {
                    self.marks.remove(&p);
                }
            }
        }
    }

    /// The marked copies of the view, with their sizes, in the order shown.
    pub fn marked(&self) -> Vec<(PathBuf, u64)> {
        let mut out = Vec::new();
        let Some(f) = self.found.as_ref().filter(|_| !self.marks.is_empty()) else { return out };
        // Never all of a group, however the marks came to be.
        let mut add = |items: Vec<(&Path, u64)>| {
            if !items.iter().all(|(p, _)| self.marks.contains(*p)) {
                out.extend(items.into_iter().filter(|(p, _)| self.marks.contains(*p)).map(|(p, s)| (p.to_path_buf(), s)));
            }
        };
        match self.view {
            View::Files => {
                for g in &f.groups {
                    add(g.files.iter().map(|c| (c.path.as_path(), g.size)).collect());
                }
            }
            View::Folders => {
                for g in self.folder_groups.iter().filter(|g| g.identical()) {
                    add(g.folders.iter().map(|&i| (f.folders[i].path.as_path(), f.folders[i].bytes)).collect());
                }
            }
        }
        out
    }

    /// The list of the view as a CSV file: a row per copy, with its group.
    pub fn csv(&self) -> String {
        let Some(f) = &self.found else { return String::new() };
        let yes = |p: &Path| if self.marks.contains(p) { tr("Yes") } else { String::new() };
        let (header, rows): (Vec<String>, Vec<Vec<String>>) = match self.view {
            View::Files => (
                [tr("Group"), tr("Name"), tr("Folder"), tr("Size (bytes)"), tr("Modified"), tr("Marked")].into(),
                f.groups
                    .iter()
                    .enumerate()
                    .flat_map(|(g, grp)| {
                        grp.files.iter().map(move |c| {
                            let name = c.path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                            let dir = c.path.parent().map_or(String::new(), |d| d.display().to_string());
                            vec![(g + 1).to_string(), name, dir, grp.size.to_string(), crate::format::datetime(c.mtime), yes(&c.path)]
                        })
                    })
                    .collect(),
            ),
            View::Folders => (
                [tr("Group"), tr("Path"), tr("In common"), tr("Size (bytes)"), tr("Files"), tr("Modified"), tr("Marked")].into(),
                self.folder_groups
                    .iter()
                    .enumerate()
                    .flat_map(|(g, grp)| {
                        let share = if grp.identical() { tr("Identical") } else { crate::format::whole_percent(grp.permille as u64 / 10) };
                        grp.folders.iter().map(move |&i| {
                            let d = &f.folders[i];
                            let marked = if grp.identical() { yes(&d.path) } else { String::new() };
                            vec![(g + 1).to_string(), d.path.display().to_string(), share.clone(), d.bytes.to_string(), d.files.to_string(), crate::format::datetime(d.mtime), marked]
                        })
                    })
                    .collect(),
            ),
        };
        crate::actions::csv(&header, &rows)
    }

    /// Takes out of the result what is no longer where it was (`paths`, files
    /// or folders): the copies in them, the groups left with one copy, and the
    /// pairs of folders that changed.
    pub fn removed(&mut self, paths: &[PathBuf]) {
        let gone_set: HashSet<&Path> = paths.iter().map(|p| p.as_path()).collect();
        let gone = |p: &Path| p.ancestors().any(|a| gone_set.contains(a));
        // The folders that held something removed.
        let holders: HashSet<&Path> = paths.iter().flat_map(|p| p.ancestors().skip(1)).collect();
        if let Some(f) = &mut self.found {
            for g in &mut f.groups {
                g.files.retain(|c| !gone(&c.path));
            }
            f.groups.retain(|g| g.files.len() > 1);
            let changed: Vec<bool> = f.folders.iter().map(|d| gone(&d.path) || holders.contains(d.path.as_path())).collect();
            let mut new_index = vec![None; f.pairs.len()];
            let mut kept = Vec::new();
            for (i, p) in f.pairs.iter().enumerate() {
                if !changed[p.a] && !changed[p.b] {
                    new_index[i] = Some(kept.len());
                    kept.push(p.clone());
                }
            }
            for p in &mut kept {
                p.parent = p.parent.and_then(|i| new_index[i]);
            }
            f.pairs = kept;
        }
        self.marks.retain(|p| !gone(p));
        let (sel, scroll) = (self.sel, self.scroll);
        self.fill_rows();
        self.keep_one_of_each();
        if !self.rows.is_empty() {
            self.sel = sel.map(|s| s.min(self.rows.len() - 1));
            self.scroll = scroll.min(self.rows.len() - 1);
        }
    }

    /// Unmarks the groups (of both views) whose copies are all marked: after
    /// copies were removed, the one kept may be gone.
    fn keep_one_of_each(&mut self) {
        let Some(f) = &self.found else { return };
        let mut groups: Vec<Vec<PathBuf>> = f.groups.iter().map(|g| g.files.iter().map(|c| c.path.clone()).collect()).collect();
        // Identical folders are grouped differently for each share shown.
        for share in crate::folders::SHARES {
            for g in crate::folders::groups(&f.folders, &f.pairs, share).into_iter().filter(|g| g.identical()) {
                groups.push(g.folders.iter().map(|&i| f.folders[i].path.clone()).collect());
            }
        }
        // Marks of copies no longer in a group go.
        let listed: HashSet<&PathBuf> = groups.iter().flatten().collect();
        self.marks.retain(|p| listed.contains(p));
        for g in &groups {
            if g.iter().all(|p| self.marks.contains(p)) {
                for p in g {
                    self.marks.remove(p);
                }
            }
        }
    }

    /// The path of the selected file or folder.
    pub fn selected_path(&self) -> Option<PathBuf> {
        self.path_of(self.sel?)
    }

    /// The path of the file or folder of a row.
    pub fn path_of(&self, row: usize) -> Option<PathBuf> {
        let found = self.found.as_ref()?;
        match self.rows.get(row)? {
            Row::File(g, i) => Some(found.groups[*g].files[*i].path.clone()),
            Row::Folder(g, i) => Some(found.folders[self.folder_groups[*g].folders[*i]].path.clone()),
            Row::Group(_) | Row::FolderGroup(_) => None,
        }
    }

    pub fn select(&mut self, row: usize) {
        if row < self.rows.len() {
            self.sel = Some(row);
            self.ensure_visible(row);
            self.note = None;
        }
    }

    /// Moves the selection by `delta` rows (clamped).
    pub fn move_sel(&mut self, delta: isize) {
        if self.rows.is_empty() {
            return;
        }
        let cur = self.sel.unwrap_or(0) as isize;
        let to = (cur + delta).clamp(0, self.rows.len() as isize - 1) as usize;
        self.select(to);
    }

    fn ensure_visible(&mut self, row: usize) {
        let page = self.page.max(1);
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + page {
            self.scroll = row + 1 - page;
        }
    }

    pub fn scroll_by(&mut self, rows: isize) {
        let max = self.rows.len().saturating_sub(self.page.max(1)) as isize;
        self.scroll = (self.scroll as isize + rows).clamp(0, max.max(0)) as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::tests::{sample, sys};

    #[test]
    fn roots_are_chosen_and_taken_out() {
        let mut app = App::new();
        app.toggle_root(Path::new("C:/data"));
        app.toggle_root(Path::new("D:/"));
        assert_eq!(app.chosen.len(), 2);
        assert!(app.is_chosen(Path::new("C:/data")));
        app.volumes = Some(vec![Volume { path: rt_scan::normalize(Path::new("D:/")), label: String::new(), fs: String::new(), total: 0, free: 0, remote: false }]);
        assert_eq!(app.folders(), vec![rt_scan::normalize(Path::new("C:/data"))]);
        app.toggle_root(Path::new("C:/data"));
        assert!(!app.is_chosen(Path::new("C:/data")));
        assert_eq!(app.chosen.len(), 1);
        assert!(app.folders().is_empty(), "never added: gone with its tick");
        // Folders added stay on the list, ticked or not.
        app.add_folder(Path::new("C:/fotos"));
        app.add_folder(Path::new("E:/backup"));
        app.toggle_root(Path::new("C:/fotos"));
        assert_eq!(app.folders().len(), 2);
        assert!(!app.is_chosen(Path::new("C:/fotos")) && app.is_chosen(Path::new("E:/backup")));
        app.remove_folder(Path::new("E:/backup"));
        assert_eq!((app.folders().len(), app.chosen.len()), (1, 1));
        // A folder chosen before there was a list stays on it when unticked.
        app.toggle_root(Path::new("F:/old"));
        let n = app.folders().len();
        app.toggle_folder(Path::new("F:/old"));
        assert!(!app.is_chosen(Path::new("F:/old")));
        assert_eq!(app.folders().len(), n);
    }

    #[test]
    fn typing_in_the_filter_boxes() {
        let mut app = App::new();
        app.focus_field(Field::SkipExt, None);
        app.type_text("tmp lög");
        assert_eq!(app.choice.text(Field::SkipExt), "tmp lög");
        app.delete(false, false);
        app.move_caret(-1, false, false, false);
        app.move_caret(-1, false, false, false);
        // A line break inside pasted text becomes a space (a separator).
        app.type_text("\n.");
        assert_eq!(app.choice.text(Field::SkipExt), "tmp  .lö");
        app.move_caret(1, true, false, false);
        app.delete(false, false);
        assert_eq!(app.choice.text(Field::SkipExt), "tmp  .l");
        app.move_caret(-1, true, false, false);
        app.delete(true, false);
        assert_eq!(app.choice.text(Field::SkipExt), "mp  .l");
        app.select_all();
        app.type_text("obj");
        assert_eq!((app.choice.text(Field::SkipExt), app.caret), ("obj", 3));
        app.focus_next(false);
        assert_eq!((app.focus, app.selection()), (Some(Field::OnlyNames), None), "empty: nothing to select");
        app.focus_next(true);
        assert_eq!(app.selection(), Some((0, 3)));
        app.delete(true, false);
        assert_eq!(app.choice.text(Field::SkipExt), "");
        app.focus_field(Field::MinSize, Some(99));
        assert_eq!(app.caret, 4, "1 MB");
    }

    #[test]
    fn selecting_text_in_a_box() {
        let mut app = App::new();
        app.choice.texts[Field::OnlyExt.index()] = "jpg png, gif  ñu".into();
        app.focus_field(Field::OnlyExt, Some(0));
        // Shift with the arrows, then with Ctrl (a word at a time).
        app.move_caret(1, false, false, true);
        app.move_caret(1, false, false, true);
        assert_eq!(app.selected_text(), "jp");
        app.move_caret(1, false, true, true);
        assert_eq!(app.selected_text(), "jpg");
        app.move_caret(1, false, true, true);
        assert_eq!(app.selected_text(), "jpg png");
        // An arrow without Shift goes to that side of the selection.
        app.move_caret(-1, false, false, false);
        assert_eq!((app.caret, app.selection()), (0, None));
        // Shift+End, then typing replaces the selection.
        app.move_caret(1, false, true, false);
        app.move_caret(1, true, false, true);
        assert_eq!(app.selected_text(), " png, gif  ñu");
        app.type_text(" bmp");
        assert_eq!(app.choice.text(Field::OnlyExt), "jpg bmp");
        // A drag, a word, and Ctrl+Backspace.
        app.choice.texts[Field::OnlyExt.index()] = "jpg png, gif  ñu".into();
        app.focus_field(Field::OnlyExt, Some(5));
        app.drag_caret(12);
        assert_eq!(app.selected_text(), "ng, gif");
        app.delete(false, false);
        assert_eq!(app.choice.text(Field::OnlyExt), "jpg p  ñu");
        app.select_word(8);
        assert_eq!(app.selected_text(), "ñu");
        app.select_word(1);
        assert_eq!(app.selected_text(), "jpg");
        app.move_caret(1, true, false, false);
        app.delete(false, true);
        assert_eq!(app.choice.text(Field::OnlyExt), "jpg p  ");
        app.move_caret(-1, false, true, false);
        assert_eq!(app.caret, 4);
    }

    #[test]
    fn the_calendar_moves_by_months() {
        let mut c = Calendar::open(Field::From, "2024-12-05");
        assert_eq!((c.year, c.month), (2024, 12));
        c.shift(1);
        assert_eq!((c.year, c.month), (2025, 1));
        c.shift(-13);
        assert_eq!((c.year, c.month), (2023, 12));
        assert_eq!(Calendar::open(Field::From, "2019").month, 1);
        let today = Calendar::open(Field::To, "nonsense");
        assert!(today.year >= 2024 && (1..=12).contains(&today.month));
    }
    #[test]
    fn the_list_has_a_title_and_the_files_of_each_group() {
        let tt = sample("rdup-app");
        let mut app = App::new();
        app.page = 4;
        app.set_found(sys(std::slice::from_ref(&tt.0)).unwrap());
        assert_eq!(app.rows, [Row::Group(0), Row::File(0, 0), Row::File(0, 1), Row::File(0, 2), Row::Group(1), Row::File(1, 0), Row::File(1, 1)]);
        assert_eq!(app.sel, Some(1), "the first file");
        assert!(app.selected_path().unwrap().ends_with("big copy.bin"));
        app.move_sel(3);
        assert_eq!((app.sel, app.scroll), (Some(4), 1));
        assert_eq!(app.selected_path(), None, "a title");
        app.move_sel(100);
        assert_eq!(app.sel, Some(6));
        app.scroll_by(-100);
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn one_copy_of_each_group_is_kept() {
        let tt = sample("rdup-app-marks");
        let mut app = App::new();
        app.set_found(sys(std::slice::from_ref(&tt.0)).unwrap());
        // Rows: big (3 copies) at 1..=3, notes (2) at 5..=6.
        assert!(app.toggle_mark(1).is_ok());
        assert!(app.toggle_mark(2).is_ok());
        assert!(app.toggle_mark(3).is_err(), "the last copy");
        assert!(app.toggle_mark(5).is_ok());
        assert!(app.toggle_mark(6).is_err());
        assert_eq!(app.marked().len(), 3);
        assert!(app.toggle_mark(2).is_ok(), "unmarking");
        assert_eq!(app.marked().len(), 2);
        // A rule: all but the shortest path of each group.
        assert_eq!(app.apply_rule(&Rule::Shortest), 3);
        assert!(!app.is_marked(5) && app.is_marked(6) || app.is_marked(5) && !app.is_marked(6));
        let shortest = app.path_of(5).unwrap();
        assert_eq!(app.is_marked(5), shortest.as_os_str().len() > app.path_of(6).unwrap().as_os_str().len());
        app.unmark_all();
        assert!(app.marked().is_empty());
        // The folder "backup" goes: two copies of big.bin with it; one is left.
        app.toggle_mark(3).unwrap();
        app.removed(&[rt_scan::normalize(&tt.0).join("backup")]);
        assert_eq!(app.found.as_ref().unwrap().groups.len(), 1, "big.bin has one copy left");
        assert!(app.marks.is_empty(), "photos/big.bin was marked: it is the only one now");
        assert_eq!(app.rows.len(), 3);
    }

    #[test]
    fn the_folders_are_another_view() {
        let tt = crate::folders::tests::folders("rdup-app-folders");
        let mut app = App::new();
        app.set_found(crate::folders::tests::search(&tt, 1));
        assert_eq!(app.view, View::Files);
        assert_eq!(app.folder_groups.len(), 3, "identical, and two with 93 % in common");
        app.set_view(View::Folders);
        assert_eq!(app.rows[..3], [Row::FolderGroup(0), Row::Folder(0, 0), Row::Folder(0, 1)]);
        assert_eq!(app.sel, Some(1));
        assert!(app.selected_path().unwrap().ends_with("backup/photos"));
        app.set_share(1000);
        assert_eq!(app.rows.len(), 3);
        app.set_share(500);
        assert_eq!(app.folder_groups.len(), 3);
        app.set_view(View::Files);
        assert!(matches!(app.rows[0], Row::Group(_)));
        // Folders only: they are shown first.
        let opts = crate::search::Options { files: false, ..Default::default() };
        let f = crate::search::find_with(std::slice::from_ref(&tt.0), rt_scan::Ways::SYSTEM, opts, &Default::default(), &|_| {}).unwrap();
        app.set_found(f);
        assert_eq!((app.view, app.rows[0]), (View::Folders, Row::FolderGroup(0)));
    }
}
