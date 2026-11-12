//! The window's state without the window: where the browser is (the
//! drives, a folder on disk, or inside an archive), the rows, the selection,
//! where to extract, and the work running.

use crate::archive::{Codec, Entry, Listing, Meter};
use crate::create::{Kind as NewKind, Spec};
use crate::extract::{Job, Overwrite};
use crate::tree::Tree;
use rt_scan::volumes::Volume;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Col {
    Name,
    Size,
    /// Archive: the size inside it.
    Packed,
    Modified,
    /// Archive: the compression method.
    Method,
    /// Disk: the kind of file.
    Type,
    /// Drives: the free space and the file system.
    Free,
    FileSystem,
}

/// Where the browser is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// The drives (This PC).
    Drives,
    /// A folder on disk.
    Folder(PathBuf),
    /// Inside the archive open (`App::opened`).
    Archive,
}

/// A file, folder or drive on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskItem {
    pub name: String,
    pub path: PathBuf,
    pub dir: bool,
    pub size: u64,
    pub mtime: i64,
    /// For a drive: its label, file system and space.
    pub volume: Option<Volume>,
}

/// A row of the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// Back to the folder above.
    Up,
    /// Archive: a folder (node of the tree).
    Dir(usize),
    /// Archive: an entry that is not a folder.
    File(usize),
    /// Disk: a drive, folder or file (index into `App::disk`).
    Disk(usize),
}

/// A place on the path shown above the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Crumb {
    Drives,
    Folder(PathBuf),
    /// A folder of the archive (node of its tree; 0 is the archive itself).
    Node(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Task {
    Opening,
    Extracting,
    Testing,
    Hashing,
    Creating,
}

/// The "Add to archive" form: what to put in, and how.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddForm {
    pub paths: Vec<PathBuf>,
    /// The folder the archive goes to.
    pub dir: PathBuf,
    /// The archive's file name, as typed.
    pub name: String,
    /// Where typing goes (a char index), and whether the whole name is
    /// selected (typing replaces it).
    pub caret: usize,
    pub all: bool,
    pub spec: Spec,
    /// Each path into an archive of its own, named after it.
    pub each: bool,
}

impl AddForm {
    pub fn new(paths: Vec<PathBuf>, dir: PathBuf, spec: Spec) -> AddForm {
        let mut spec = spec;
        // One kind of archive holds one file only.
        if spec.kind.needs_one_file() && !one_file(&paths) {
            spec.kind = NewKind::SevenZ;
        }
        let mut f = AddForm { paths, dir, name: String::new(), caret: 0, all: true, spec, each: false };
        f.name = f.free_name(&crate::create::suggested_name(&f.paths, f.spec.kind));
        f.caret = f.name.chars().count();
        f
    }

    /// `name`, or a free name like it in the folder: never a file already
    /// there (one of those being put in, perhaps).
    fn free_name(&self, name: &str) -> String {
        free_path(&self.dir.join(name)).file_name().map_or(name.to_string(), |n| n.to_string_lossy().into_owned())
    }

    pub fn one_file(&self) -> bool {
        one_file(&self.paths)
    }

    /// Changes the kind of archive; the name's extension follows.
    pub fn set_kind(&mut self, kind: NewKind) {
        if kind.needs_one_file() && !self.one_file() {
            return;
        }
        let old = format!(".{}", self.spec.kind.ext());
        let stem = self.name.strip_suffix(&old).unwrap_or(&self.name).to_string();
        self.spec.kind = kind;
        self.name = self.free_name(&format!("{stem}.{}", kind.ext()));
        self.caret = self.name.chars().count();
    }

    /// Typing in the name.
    pub fn type_text(&mut self, s: &str) {
        let s: String = s.chars().filter(|c| !c.is_control() && !"\\/:*?\"<>|".contains(*c)).collect();
        if self.all {
            self.name.clear();
            self.caret = 0;
            self.all = false;
        }
        let at = self.byte_at(self.caret);
        self.name.insert_str(at, &s);
        self.caret += s.chars().count();
    }

    /// Backspace (`forward` false) or Delete.
    pub fn delete(&mut self, forward: bool) {
        if self.all {
            self.name.clear();
            self.caret = 0;
            self.all = false;
            return;
        }
        let n = self.name.chars().count();
        let at = if forward {
            if self.caret >= n {
                return;
            }
            self.caret
        } else {
            if self.caret == 0 {
                return;
            }
            self.caret -= 1;
            self.caret
        };
        let (a, b) = (self.byte_at(at), self.byte_at(at + 1));
        self.name.replace_range(a..b, "");
    }

    pub fn move_caret(&mut self, delta: isize, to_end: bool) {
        let n = self.name.chars().count() as isize;
        self.all = false;
        self.caret = if to_end { if delta < 0 { 0 } else { n as usize } } else { (self.caret as isize + delta).clamp(0, n) as usize };
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.name.char_indices().nth(chars).map_or(self.name.len(), |(i, _)| i)
    }

    /// Where the archive (or, for each path, its archive) goes.
    pub fn targets(&self) -> Vec<(Vec<PathBuf>, PathBuf)> {
        if self.each && self.paths.len() > 1 {
            self.paths
                .iter()
                .map(|p| {
                    let dir = p.parent().map(Path::to_path_buf).unwrap_or_else(|| self.dir.clone());
                    let name = crate::create::suggested_name(std::slice::from_ref(p), self.spec.kind);
                    (vec![p.clone()], free_path(&dir.join(name)))
                })
                .collect()
        } else {
            vec![(self.paths.clone(), self.dir.join(self.name.trim()))]
        }
    }

    /// Whether it can be made as it is.
    pub fn ready(&self) -> bool {
        let name = self.name.trim();
        let into_itself = !self.each && self.paths.contains(&self.dir.join(name));
        !self.paths.is_empty()
            && (self.each && self.paths.len() > 1 || !name.is_empty() && name != "." && name != ".." && !into_itself)
            && (!self.spec.kind.needs_one_file() || self.one_file())
    }
}

/// Whether `paths` is one file (not a folder).
fn one_file(paths: &[PathBuf]) -> bool {
    matches!(paths, [p] if p.is_file())
}

/// `p`, or a free name beside it: `name (2).7z`.
pub fn free_path(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // The whole extension (`.tar.gz`) stays at the end.
    let (stem, ext) = match name.find('.').filter(|&i| i > 0) {
        Some(i) => (name[..i].to_string(), name[i..].to_string()),
        None => (name.clone(), String::new()),
    };
    (2..).map(|n| p.with_file_name(format!("{stem} ({n}){ext}"))).find(|q| !q.exists()).unwrap_or_else(|| p.to_path_buf())
}

/// Work running on a background thread.
#[derive(Debug)]
pub struct Busy {
    pub task: Task,
    pub meter: Arc<Meter>,
    pub started: Instant,
    /// What the meter's count goes up to: bytes of the archive read
    /// (`by_read`) or of the contents done; 0 when unknown.
    pub total: u64,
    pub by_read: bool,
    /// What it works on: the archive, or the first file.
    pub path: PathBuf,
    /// How many archives or files, when more than one.
    pub count: usize,
}

impl Busy {
    /// How far it got, from 0 to 1, when the total is known.
    pub fn fraction(&self) -> Option<f32> {
        use std::sync::atomic::Ordering::Relaxed;
        let at = if self.by_read { self.meter.read.load(Relaxed) } else { self.meter.done.load(Relaxed) };
        let total = self.total();
        (total > 0).then(|| (at as f64 / total as f64).min(1.0) as f32)
    }

    /// The total, known from the start or learnt by the work.
    pub fn total(&self) -> u64 {
        self.total.max(self.meter.total.load(std::sync::atomic::Ordering::Relaxed))
    }
}

/// An archive that is open.
#[derive(Clone, Debug)]
pub struct Opened {
    pub path: PathBuf,
    pub listing: Listing,
    pub tree: Tree,
}

#[derive(Debug)]
pub struct App {
    pub place: Place,
    /// The drives, files and folders of the place on disk.
    pub disk: Vec<DiskItem>,
    /// Why the folder could not be read.
    pub disk_error: Option<String>,
    pub opened: Option<Opened>,
    /// The folder of the archive shown (a node of the tree).
    pub folder: usize,
    pub rows: Vec<Row>,
    /// One per row.
    pub selected: Vec<bool>,
    /// The row the keyboard is on, and where a Shift selection starts.
    pub cursor: Option<usize>,
    pub anchor: Option<usize>,
    pub scroll: usize,
    /// Rows that fit in the list (set by the window).
    pub page: usize,
    pub sort: Col,
    pub descending: bool,
    /// Where to extract, when chosen; otherwise beside the archive.
    pub dest: Option<PathBuf>,
    pub overwrite: Overwrite,
    /// Open the folder in the file manager after extracting.
    pub open_after: bool,
    pub busy: Option<Busy>,
    /// A line for the status bar (what the last action did).
    pub note: Option<String>,
    /// The "Add to archive" form, when open.
    pub add: Option<AddForm>,
    /// How archives are made, as last chosen.
    pub spec: Spec,
}

impl Default for App {
    fn default() -> App {
        App::new()
    }
}

/// Whether a file name looks like an archive rpack opens (to offer
/// extracting it from the browser; opening still goes by the contents).
pub fn looks_like_archive(name: &str) -> bool {
    let lower = name.to_lowercase();
    let Some((_, ext)) = lower.rsplit_once('.') else { return false };
    let mut exts: Vec<&str> = vec!["zip", "7z", "tar", "jar", "apk", "epub", "cbz"];
    for c in Codec::ALL {
        let (single, tars) = c.extensions();
        exts.extend(single);
        exts.extend(tars);
    }
    exts.contains(&ext)
}

impl App {
    pub fn new() -> App {
        App {
            place: Place::Drives,
            disk: Vec::new(),
            disk_error: None,
            opened: None,
            folder: 0,
            rows: Vec::new(),
            selected: Vec::new(),
            cursor: None,
            anchor: None,
            scroll: 0,
            page: 20,
            sort: Col::Name,
            descending: false,
            dest: None,
            overwrite: Overwrite::default(),
            open_after: true,
            busy: None,
            note: None,
            add: None,
            spec: Spec::default(),
        }
    }

    pub fn in_archive(&self) -> bool {
        self.place == Place::Archive && self.opened.is_some()
    }

    // ------------------------------------------------------------ disk

    /// Shows the drives.
    pub fn show_drives(&mut self, volumes: Vec<Volume>, select: Option<&Path>) {
        self.close_archive();
        self.place = Place::Drives;
        self.disk_error = None;
        self.disk = volumes
            .into_iter()
            .map(|v| DiskItem {
                name: drive_name(&v),
                path: v.path.clone(),
                dir: true,
                size: v.total,
                mtime: 0,
                volume: Some(v),
            })
            .collect();
        self.after_move(select);
    }

    /// Shows a folder on disk; the row of `select` (a file or folder in it)
    /// is selected. Err: why it cannot be read (nothing changes).
    pub fn show_folder(&mut self, dir: &Path, select: Option<&Path>) -> Result<(), String> {
        let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
        let mut items = Vec::new();
        for e in rd.flatten() {
            let path = e.path();
            // Links to folders are followed, as the file manager does.
            let Ok(m) = std::fs::metadata(&path).or_else(|_| e.metadata()) else { continue };
            items.push(DiskItem {
                name: e.file_name().to_string_lossy().into_owned(),
                path,
                dir: m.is_dir(),
                size: if m.is_dir() { 0 } else { m.len() },
                mtime: m.modified().map_or(0, crate::archive::unix_secs),
                volume: None,
            });
        }
        self.close_archive();
        self.place = Place::Folder(dir.to_path_buf());
        self.disk = items;
        self.disk_error = None;
        if !matches!(self.sort, Col::Name | Col::Size | Col::Modified | Col::Type) {
            self.sort = Col::Name;
            self.descending = false;
        }
        self.after_move(select);
        Ok(())
    }

    /// The folder on disk shown, or that holds the archive open.
    pub fn disk_folder(&self) -> Option<PathBuf> {
        match &self.place {
            Place::Folder(d) => Some(d.clone()),
            Place::Archive => self.opened.as_ref().and_then(|o| o.path.parent().map(Path::to_path_buf)),
            Place::Drives => None,
        }
    }

    fn after_move(&mut self, select: Option<&Path>) {
        self.rebuild();
        let at = select.and_then(|s| self.rows.iter().position(|r| matches!(r, Row::Disk(i) if self.disk[*i].path == s)));
        self.cursor = at.or((!self.rows.is_empty()).then_some(0));
        self.anchor = self.cursor;
        self.scroll = 0;
        if let Some(c) = self.cursor {
            self.selected[c] = at.is_some();
            self.keep_visible(c);
        }
    }

    /// The paths of the disk rows selected (or of the cursor's, when none
    /// is selected).
    pub fn disk_chosen(&self) -> Vec<PathBuf> {
        let mut rows = self.chosen();
        if rows.is_empty() {
            rows.extend(self.cursor.and_then(|c| self.rows.get(c).copied()).filter(|r| *r != Row::Up));
        }
        rows.into_iter()
            .filter_map(|r| match r {
                Row::Disk(i) => Some(self.disk[i].path.clone()),
                _ => None,
            })
            .collect()
    }

    /// The archives among the disk rows selected (by their names).
    pub fn chosen_archives(&self) -> Vec<PathBuf> {
        self.chosen()
            .into_iter()
            .filter_map(|r| match r {
                Row::Disk(i) if !self.disk[i].dir && looks_like_archive(&self.disk[i].name) => Some(self.disk[i].path.clone()),
                _ => None,
            })
            .collect()
    }

    // ------------------------------------------------------------ archive

    /// Shows an archive just listed, from its root.
    pub fn set_opened(&mut self, path: PathBuf, listing: Listing) {
        let tree = Tree::build(&listing.entries);
        self.opened = Some(Opened { path, listing, tree });
        self.place = Place::Archive;
        self.dest = None;
        self.note = None;
        if !matches!(self.sort, Col::Name | Col::Size | Col::Packed | Col::Modified | Col::Method) {
            self.sort = Col::Name;
            self.descending = false;
        }
        self.folder = 0;
        self.enter(0);
    }

    fn close_archive(&mut self) {
        self.opened = None;
        self.dest = None;
        self.folder = 0;
    }

    /// Shows folder `n` of the archive.
    pub fn enter(&mut self, n: usize) {
        let came_from = self.folder;
        self.folder = n;
        self.rebuild();
        // Back up a level: the folder left is selected.
        let back = self.rows.iter().position(|r| *r == Row::Dir(came_from)).filter(|_| came_from != n);
        self.cursor = back.or((!self.rows.is_empty()).then_some(0));
        self.anchor = self.cursor;
        self.scroll = 0;
        if let Some(c) = self.cursor {
            self.selected[c] = back.is_some();
            self.keep_visible(c);
        }
    }

    /// Where "up" goes: a folder of the archive, the folder on disk (from
    /// the archive's root, or from a folder), or the drives.
    pub fn up_target(&self) -> Option<(Crumb, Option<PathBuf>)> {
        match &self.place {
            Place::Archive => {
                let o = self.opened.as_ref()?;
                if self.folder != 0 {
                    Some((Crumb::Node(o.tree.nodes[self.folder].parent), None))
                } else {
                    let dir = o.path.parent()?.to_path_buf();
                    Some((Crumb::Folder(dir), Some(o.path.clone())))
                }
            }
            Place::Folder(d) => match d.parent() {
                Some(p) => Some((Crumb::Folder(p.to_path_buf()), Some(d.clone()))),
                None => Some((Crumb::Drives, Some(d.clone()))),
            },
            Place::Drives => None,
        }
    }

    /// The rows of the place shown, sorted; folders first.
    pub fn rebuild(&mut self) {
        let mut dirs: Vec<Row>;
        let mut files: Vec<Row>;
        match (&self.place, &self.opened) {
            (Place::Archive, Some(o)) => {
                let node = &o.tree.nodes[self.folder];
                dirs = node.dirs.iter().map(|&d| Row::Dir(d)).collect();
                files = node.files.iter().map(|&f| Row::File(f)).collect();
            }
            _ => {
                dirs = (0..self.disk.len()).filter(|&i| self.disk[i].dir).map(Row::Disk).collect();
                files = (0..self.disk.len()).filter(|&i| !self.disk[i].dir).map(Row::Disk).collect();
            }
        }
        let cmp = |a: &Row, b: &Row| {
            let o = self.sort_key(*a).cmp(&self.sort_key(*b)).then_with(|| self.name_key(*a).cmp(&self.name_key(*b)));
            if self.descending {
                o.reverse()
            } else {
                o
            }
        };
        // Drives keep the system's order unless sorted otherwise.
        if !(self.place == Place::Drives && self.sort == Col::Name && !self.descending) {
            dirs.sort_by(cmp);
        }
        files.sort_by(cmp);
        let mut rows = Vec::with_capacity(dirs.len() + files.len() + 1);
        if self.place != Place::Drives {
            rows.push(Row::Up);
        }
        rows.extend(dirs);
        rows.extend(files);
        self.rows = rows;
        self.selected = vec![false; self.rows.len()];
        self.cursor = self.cursor.filter(|&c| c < self.rows.len());
    }

    /// Sorts by `col`; the same column again turns the order around.
    pub fn sort_by(&mut self, col: Col) {
        if self.sort == col {
            self.descending = !self.descending;
        } else {
            self.sort = col;
            // Sizes and dates: the largest and newest first.
            self.descending = matches!(col, Col::Size | Col::Packed | Col::Modified | Col::Free);
        }
        let chosen: Vec<Row> = self.rows.iter().zip(&self.selected).filter(|(_, s)| **s).map(|(r, _)| *r).collect();
        let cursor = self.cursor.and_then(|c| self.rows.get(c).copied());
        self.rebuild();
        for (i, r) in self.rows.iter().enumerate() {
            self.selected[i] = chosen.contains(r);
        }
        self.cursor = cursor.and_then(|c| self.rows.iter().position(|r| *r == c));
    }

    fn name_key(&self, r: Row) -> String {
        match (r, &self.opened) {
            (Row::Dir(n), Some(o)) => o.tree.nodes[n].name.to_lowercase(),
            (Row::File(e), Some(o)) => o.listing.entries[e].name().to_lowercase(),
            (Row::Disk(i), _) => self.disk[i].name.to_lowercase(),
            _ => String::new(),
        }
    }

    fn sort_key(&self, r: Row) -> Key {
        let text = |s: &str| Key::Text(s.to_lowercase());
        let num = |n: i128| Key::Num(n);
        match (r, &self.opened) {
            (Row::Dir(n), Some(o)) => {
                let n = &o.tree.nodes[n];
                match self.sort {
                    Col::Size => num(n.size as i128),
                    Col::Packed => num(n.packed.map_or(-1, |p| p as i128)),
                    Col::Modified => num(n.mtime as i128),
                    _ => text(&n.name),
                }
            }
            (Row::File(e), Some(o)) => {
                let e = &o.listing.entries[e];
                match self.sort {
                    Col::Size => num(e.size.unwrap_or(0) as i128),
                    Col::Packed => num(e.packed.map_or(-1, |p| p as i128)),
                    Col::Modified => num(e.mtime as i128),
                    Col::Method => text(&e.method),
                    _ => text(e.name()),
                }
            }
            (Row::Disk(i), _) => {
                let d = &self.disk[i];
                match self.sort {
                    Col::Size => num(d.size as i128),
                    Col::Modified => num(d.mtime as i128),
                    Col::Type => text(d.name.rsplit_once('.').map_or("", |(_, e)| e)),
                    Col::Free => num(d.volume.as_ref().map_or(0, |v| v.free) as i128),
                    Col::FileSystem => text(d.volume.as_ref().map_or("", |v| v.fs.as_str())),
                    _ => text(&d.name),
                }
            }
            _ => Key::Num(0),
        }
    }

    // ------------------------------------------------------------ selection

    /// A click on row `i`, with Ctrl (add or take away) or Shift (a range).
    pub fn click(&mut self, i: usize, ctrl: bool, shift: bool) {
        if i >= self.rows.len() {
            return;
        }
        if shift {
            let a = self.anchor.unwrap_or(i);
            if !ctrl {
                self.selected.iter_mut().for_each(|s| *s = false);
            }
            for s in &mut self.selected[a.min(i)..=a.max(i)] {
                *s = true;
            }
        } else if ctrl {
            self.selected[i] = !self.selected[i];
            self.anchor = Some(i);
        } else {
            self.selected.iter_mut().for_each(|s| *s = false);
            self.selected[i] = true;
            self.anchor = Some(i);
        }
        // The row "up" is never part of a selection.
        if self.rows[0] == Row::Up {
            self.selected[0] = false;
        }
        self.cursor = Some(i);
    }

    pub fn select_all(&mut self) {
        for (s, r) in self.selected.iter_mut().zip(&self.rows) {
            *s = *r != Row::Up;
        }
    }

    /// The keyboard moves the cursor by `delta` rows (Shift keeps a range).
    pub fn move_cursor(&mut self, delta: isize, shift: bool) {
        if self.rows.is_empty() {
            return;
        }
        let at = self.cursor.map_or(0, |c| (c as isize + delta).clamp(0, self.rows.len() as isize - 1) as usize);
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

    /// The rows selected (never "up").
    pub fn chosen(&self) -> Vec<Row> {
        self.rows.iter().zip(&self.selected).filter(|(r, s)| **s && **r != Row::Up).map(|(r, _)| *r).collect()
    }

    // ------------------------------------------------------------ extracting

    /// The archive's name without its extensions: `photos.tar.gz` → `photos`.
    pub fn stem(path: &Path) -> String {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut exts: Vec<&str> = vec!["zip", "7z", "tar", "jar", "apk", "epub", "cbz", "docx", "xlsx", "pptx", "odt", "ods"];
        for c in Codec::ALL {
            let (single, tars) = c.extensions();
            exts.extend(single);
            exts.extend(tars);
        }
        let mut stem = name.as_str();
        // `.tar.gz`: both go.
        for _ in 0..2 {
            let lower = stem.to_lowercase();
            if let Some(e) = exts.iter().find(|e| lower.ends_with(&format!(".{e}")) && lower.len() > e.len() + 1) {
                stem = &stem[..stem.len() - e.len() - 1];
            }
        }
        if stem != name {
            return stem.to_string();
        }
        // An extension of its own (a renamed archive): it goes too, unless
        // nothing would be left.
        match name.rfind('.') {
            Some(i) if i > 0 => name[..i].to_string(),
            _ => format!("{name} (files)"),
        }
    }

    /// Where extracting goes when no folder was chosen: beside the archive,
    /// in a folder named after it, unless the archive brings its own folder
    /// (or is one file).
    pub fn default_dest(&self) -> Option<PathBuf> {
        let o = self.opened.as_ref()?;
        Some(dest_for(&o.path, &o.tree))
    }

    pub fn dest(&self) -> Option<PathBuf> {
        self.dest.clone().or_else(|| self.default_dest())
    }

    /// The archive's entries chosen: the rows selected, with everything in
    /// the folders selected; None when nothing is selected (all of it).
    pub fn wanted(&self) -> Option<Vec<bool>> {
        let o = self.opened.as_ref()?;
        let chosen = self.chosen();
        if chosen.is_empty() {
            return None;
        }
        let mut wanted = vec![false; o.listing.entries.len()];
        for r in chosen {
            match r {
                Row::Dir(n) => o.tree.mark_below(n, &mut wanted),
                Row::File(e) => wanted[e] = true,
                _ => {}
            }
        }
        Some(wanted)
    }

    /// What extracting does now: the rows selected, from the folder shown;
    /// with none selected, the whole archive.
    pub fn job(&self, dest: PathBuf) -> Option<Job> {
        let o = self.opened.as_ref()?;
        Some(match self.wanted() {
            None => Job { dest, wanted: None, strip: String::new(), overwrite: self.overwrite },
            Some(w) => Job { dest, wanted: Some(w), strip: o.tree.path(self.folder), overwrite: self.overwrite },
        })
    }

    /// The bytes a job will write, as far as known.
    pub fn job_size(&self, wanted: Option<&[bool]>) -> u64 {
        let Some(o) = &self.opened else { return 0 };
        o.listing.entries.iter().enumerate().filter(|(i, _)| wanted.is_none_or(|w| w[*i])).filter_map(|(_, e)| e.size).sum()
    }

    /// The entry of a file row.
    pub fn entry(&self, r: Row) -> Option<&Entry> {
        match r {
            Row::File(e) => self.opened.as_ref()?.listing.entries.get(e),
            _ => None,
        }
    }

    /// The places from the drives down to where the browser is, with their
    /// names (`drives` is the name of the drives).
    pub fn trail(&self, drives: &str) -> Vec<(Crumb, String)> {
        let mut out = vec![(Crumb::Drives, drives.to_string())];
        if let Some(dir) = self.disk_folder() {
            let mut parts: Vec<&Path> = dir.ancestors().collect();
            parts.reverse();
            for p in parts {
                let name = match p.file_name() {
                    Some(n) => n.to_string_lossy().into_owned(),
                    // A drive or a share: `C:`, `\\nas\share`, `/`.
                    None => {
                        let s = p.display().to_string();
                        let t = s.trim_end_matches(['\\', '/']);
                        if t.is_empty() {
                            s
                        } else {
                            t.to_string()
                        }
                    }
                };
                out.push((Crumb::Folder(p.to_path_buf()), name));
            }
        }
        if let (Place::Archive, Some(o)) = (&self.place, &self.opened) {
            let name = o.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            for n in o.tree.trail(self.folder) {
                out.push((Crumb::Node(n), if n == 0 { name.clone() } else { o.tree.nodes[n].name.clone() }));
            }
        }
        out
    }
}

/// Where an archive is extracted by default (see `App::default_dest`).
pub fn dest_for(archive: &Path, tree: &Tree) -> PathBuf {
    let beside = archive.parent().map(Path::to_path_buf).unwrap_or_default();
    if tree.single_top() {
        beside
    } else {
        beside.join(App::stem(archive))
    }
}

/// A drive as shown: `Data (D:)`, or the path alone.
fn drive_name(v: &Volume) -> String {
    let path = v.path.display().to_string();
    let short = path.trim_end_matches(['\\', '/']);
    let short = if short.is_empty() { path.as_str() } else { short };
    if v.label.is_empty() || cfg!(not(windows)) {
        short.to_string()
    } else {
        format!("{} ({short})", v.label)
    }
}

/// Drives for the tests that draw the pages.
#[cfg(test)]
pub fn tests_volumes() -> Vec<Volume> {
    vec![Volume { path: "C:\\".into(), label: "System".into(), fs: "NTFS".into(), total: 100 << 30, free: 10 << 30, remote: false }]
}

/// The value a column sorts by.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Num(i128),
    Text(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{Container, Format, Kind};

    fn listing(paths: &[(&str, u64)]) -> Listing {
        let entries = paths
            .iter()
            .map(|&(p, s)| Entry {
                path: p.trim_end_matches('/').into(),
                kind: if p.ends_with('/') { Kind::Dir } else { Kind::File },
                size: Some(s),
                packed: Some(s / 2),
                mtime: s as i64,
                method: "Deflate".into(),
                encrypted: false,
                crc: None,
                mode: None,
                link: None,
            })
            .collect();
        Listing { format: Format { container: Container::Zip, codec: None }, entries, file_size: 100, comment: String::new() }
    }

    #[test]
    fn stems() {
        let s = |n: &str| App::stem(Path::new(n));
        assert_eq!(s("photos.zip"), "photos");
        assert_eq!(s("src-1.2.tar.gz"), "src-1.2");
        assert_eq!(s("a.TGZ"), "a");
        assert_eq!(s("notes.txt.xz"), "notes.txt");
        assert_eq!(s("data.bin"), "data");
        assert_eq!(s("noext"), "noext (files)");
        assert_eq!(s(".zip"), ".zip (files)");
        assert!(looks_like_archive("a.TAR.GZ") && looks_like_archive("b.tzst") && !looks_like_archive("c.txt") && !looks_like_archive("zip"));
    }

    #[test]
    fn browse_select_and_jobs() {
        let mut app = App::new();
        app.set_opened(PathBuf::from("/x/a.zip"), listing(&[("b/", 0), ("b/c.txt", 30), ("b/d/e.txt", 5), ("z.txt", 100), ("a.txt", 1)]));
        assert_eq!(app.default_dest(), Some(PathBuf::from("/x/a")));
        // "Up" (to the folder of the archive), folders, then files by name.
        assert_eq!(app.rows, [Row::Up, Row::Dir(1), Row::File(4), Row::File(3)]);
        app.sort_by(Col::Size);
        assert_eq!(app.rows[2..], [Row::File(3), Row::File(4)]);
        // Nothing selected: everything.
        app.selected.iter_mut().for_each(|s| *s = false);
        let j = app.job(PathBuf::from("/out")).unwrap();
        assert_eq!(j.wanted, None);
        assert_eq!(app.job_size(None), 136);
        // Into b: "up", the folder d, the file c.txt.
        app.enter(1);
        assert_eq!(app.rows, [Row::Up, Row::Dir(2), Row::File(1)]);
        app.click(1, false, false);
        let j = app.job(PathBuf::from("/out")).unwrap();
        assert_eq!(j.wanted, Some(vec![false, false, true, false, false]));
        assert_eq!(j.strip, "b");
        app.click(0, false, true);
        assert_eq!(app.chosen(), [Row::Dir(2)]);
        app.select_all();
        assert_eq!(app.chosen().len(), 2);
        // Up: the folder left is selected; from the root, the folder on disk.
        assert_eq!(app.up_target(), Some((Crumb::Node(0), None)));
        app.enter(0);
        assert_eq!(app.cursor.map(|c| app.rows[c]), Some(Row::Dir(1)));
        assert_eq!(app.chosen(), [Row::Dir(1)]);
        assert_eq!(app.up_target(), Some((Crumb::Folder(PathBuf::from("/x")), Some(PathBuf::from("/x/a.zip")))));
        let names: Vec<String> = app.trail("PC").into_iter().map(|(_, n)| n).collect();
        assert_eq!(names.first().map(String::as_str), Some("PC"));
        assert_eq!(names.last().map(String::as_str), Some("a.zip"));
        // One folder at the top: extracted beside the archive.
        app.set_opened(PathBuf::from("/x/b.zip"), listing(&[("b/c.txt", 30), ("b/d.txt", 1)]));
        assert_eq!(app.default_dest(), Some(PathBuf::from("/x")));
    }

    #[test]
    fn folders_on_disk() {
        let d = crate::archive::tests::scratch("app-disk");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("b.zip"), b"PK\x05\x06").unwrap();
        std::fs::write(d.join("a.txt"), b"hello").unwrap();
        let mut app = App::new();
        app.show_folder(&d, Some(&d.join("b.zip"))).unwrap();
        let names: Vec<&str> = app.rows[1..].iter().map(|r| if let Row::Disk(i) = r { app.disk[*i].name.as_str() } else { "" }).collect();
        assert_eq!(names, ["sub", "a.txt", "b.zip"]);
        assert_eq!(app.chosen(), [app.rows[3]]);
        assert_eq!(app.chosen_archives(), [d.join("b.zip")]);
        assert_eq!(app.disk_chosen(), [d.join("b.zip")]);
        assert_eq!(app.up_target().map(|t| t.0), d.parent().map(|p| Crumb::Folder(p.to_path_buf())));
        let trail = app.trail("PC");
        assert_eq!(trail.last().map(|t| t.1.clone()), d.file_name().map(|n| n.to_string_lossy().into_owned()));
        assert!(app.show_folder(&d.join("missing"), None).is_err());
        assert_eq!(app.place, Place::Folder(d.clone()));
        app.show_drives(rt_scan::volumes::list(), None);
        assert_eq!(app.place, Place::Drives);
        assert!(app.rows.iter().all(|r| matches!(r, Row::Disk(_))));
    }

    #[test]
    fn the_add_form() {
        let d = crate::archive::tests::scratch("app-add");
        std::fs::write(d.join("a.txt"), "x").unwrap();
        std::fs::create_dir_all(d.join("photos")).unwrap();
        // A zip into a zip: never the file itself.
        std::fs::write(d.join("old.zip"), "z").unwrap();
        let mut z = AddForm::new(vec![d.join("old.zip")], d.clone(), Spec { kind: NewKind::Zip, ..Spec::default() });
        assert_eq!(z.name, "old (2).zip");
        z.name = "old.zip".into();
        assert!(!z.ready());
        let mut f = AddForm::new(vec![d.join("photos")], d.clone(), Spec::default());
        assert_eq!(f.name, "photos.7z");
        f.set_kind(NewKind::Tar(Some(Codec::Gzip)));
        assert_eq!(f.name, "photos.tar.gz");
        // A folder cannot be compressed alone.
        f.set_kind(NewKind::Single(Codec::Gzip));
        assert_eq!(f.spec.kind, NewKind::Tar(Some(Codec::Gzip)));
        f.type_text("x");
        assert_eq!((f.name.as_str(), f.ready()), ("x", true));
        f.type_text("y:z");
        f.move_caret(-1, false);
        f.delete(false);
        assert_eq!(f.name, "xz");
        f.delete(true);
        assert_eq!(f.name, "x");
        assert_eq!(f.targets(), vec![(vec![d.join("photos")], d.join("x"))]);
        let mut g = AddForm::new(vec![d.join("a.txt"), d.join("photos")], d.clone(), Spec::default());
        g.each = true;
        std::fs::write(d.join("a.7z"), "taken").unwrap();
        let t = g.targets();
        assert_eq!(t[0].1, d.join("a (2).7z"));
        assert_eq!(t[1].1, d.join("photos.7z"));
        assert_eq!(free_path(&d.join("b.tar.gz")), d.join("b.tar.gz"));
    }

    #[test]
    fn keyboard_and_scroll() {
        let mut app = App::new();
        let many: Vec<(String, u64)> = (0..50).map(|i| (format!("f{i:02}.txt"), i)).collect();
        let refs: Vec<(&str, u64)> = many.iter().map(|(p, s)| (p.as_str(), *s)).collect();
        app.set_opened(PathBuf::from("/a.zip"), listing(&refs));
        app.page = 10;
        app.move_cursor(25, false);
        assert_eq!(app.cursor, Some(25));
        assert_eq!(app.scroll, 16);
        app.move_cursor(2, true);
        assert_eq!(app.chosen().len(), 3);
        app.move_cursor(-100, false);
        assert_eq!((app.cursor, app.scroll), (Some(0), 0));
        app.scroll_by(1000);
        assert_eq!(app.scroll, 41);
    }
}
