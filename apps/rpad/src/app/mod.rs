//! Application state and commands, independent of the window system.
//! User interaction that needs dialogs goes through the [`Host`] trait so
//! workflows can be tested without a window.

pub mod commands;
pub mod doc;
pub mod find;
pub mod input;
pub mod session;
pub mod settings;

use crate::doc::{After, Buffer, Edit, IndentCfg, Kind, Sel};
use crate::i18n::{tf, tr};
use crate::io::encoding::{Enc, Eol};
use crate::io::file::{self, LoadError};
use crate::search::files::Hit;
use crate::storage::Location;
use crate::syntax::{self, Lang};
use crate::tools::{text as tt, xml};
use crate::ui::gfx::Rect;
use crate::ui::lineedit::LineEdit;
use commands::{Command, EOLS};
use doc::{Doc, Msg, Notice};
use settings::{LastTabClose, Settings, ThemePref};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    Cancel,
}

/// Dialogs and system services the application needs.
pub trait Host {
    fn ask_save(&self, name: &str) -> Answer;
    fn confirm(&self, title: &str, text: &str) -> bool;
    fn info(&self, title: &str, text: &str);
    fn error(&self, title: &str, text: &str);
    fn open_dialog(&self, dir: Option<&Path>) -> Vec<PathBuf>;
    fn save_dialog(&self, name: &str, dir: Option<&Path>) -> Option<PathBuf>;
    fn pick_folder(&self) -> Option<PathBuf>;
    fn choose_font(&self, face: &str, size: f32) -> Option<(String, f32)>;
    fn clipboard_get(&self) -> Option<String>;
    fn clipboard_set(&self, text: &str);
    fn reveal(&self, path: &Path);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Focus {
    Editor,
    Find,
    Replace,
    Palette,
    FifQuery,
    FifDir,
    FifFilter,
    Results,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptKind {
    PrefixSuffix,
    SnippetName,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteMode {
    Commands,
    GotoLine,
    Symbols,
    Snippets,
    History,
    Prompt(PromptKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PAct {
    Cmd(Command),
    Line(usize),
    Text(String),
    Path(PathBuf),
}

#[derive(Clone, Debug)]
pub struct PItem {
    pub label: String,
    pub detail: String,
    pub act: PAct,
}

pub struct Palette {
    pub mode: PaletteMode,
    pub title: String,
    pub edit: LineEdit,
    pub items: Vec<PItem>,
    pub shown: Vec<usize>,
    pub sel: usize,
    pub scroll: usize,
}

#[derive(Clone, Debug)]
pub struct ResultItem {
    pub doc: Option<u64>,
    pub path: Option<PathBuf>,
    pub line: usize,
    pub text: String,
    pub col: (usize, usize),
}

pub struct Results {
    pub title: String,
    pub items: Vec<ResultItem>,
    pub sel: usize,
    pub scroll: usize,
    pub status: String,
    /// Shows the find-in-files form above the list.
    pub fif: bool,
}

pub struct FifForm {
    pub query: LineEdit,
    pub dir: LineEdit,
    pub filter: LineEdit,
    pub cancel: Option<Arc<AtomicBool>>,
    rx: Option<mpsc::Receiver<(Vec<Hit>, usize, bool)>>,
}

/// One visual row of the editor as laid out in the last frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct RowInfo {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub y: f32,
}

/// Editor geometry from the last frame (for hit testing and paging).
#[derive(Clone, Debug, Default)]
pub struct EditorLayout {
    pub rect: Rect,
    pub text_x: f32,
    pub char_w: f32,
    pub line_h: f32,
    pub rows: Vec<RowInfo>,
    pub rows_visible: usize,
    pub wrap_cells: Option<usize>,
    pub gutter: Rect,
    pub fold_x: f32,
    pub vscroll: Rect,
    pub hscroll: Rect,
    pub max_line_cells: usize,
}

pub struct App {
    pub docs: Vec<Doc>,
    pub active: usize,
    pub settings: Settings,
    pub location: Location,
    pub find: find::FindState,
    pub palette: Option<Palette>,
    pub results: Option<Results>,
    pub fif: FifForm,
    pub focus: Focus,
    pub closed: Vec<(PathBuf, usize)>,
    untitled: usize,
    pub overwrite: bool,
    pub toast: Option<(String, Instant)>,
    /// Menus drawn in the window.
    #[cfg(not(windows))]
    pub menu: crate::ui::menu::Menu,
    pub layout: EditorLayout,
    pub quit: bool,
    pub menu_dirty: bool,
    /// Menu labels changed (language): rebuild the menu bar.
    pub menu_rebuild: bool,
    /// Closing the last tab asked to close the window.
    pub close_window: bool,
    pub fullscreen_request: bool,
    pub font_dirty: bool,
    /// Scroll the active editor so the primary caret is visible.
    pub reveal_caret: bool,
    pub last_edit: Instant,
    pub recovery_due: Option<Instant>,
    pub waker: Option<Arc<dyn Fn() + Send + Sync>>,
    pub input: input::InputState,
    pub preview: crate::ui::markdown::Preview,
    pub tab_scroll: usize,
    pub outline_scroll: usize,
    /// Positions of the bracket pair under the caret (for highlighting).
    pub bracket_pair: Option<(usize, usize)>,
}

pub const RECOVERY_DELAY: Duration = Duration::from_secs(2);

impl App {
    pub fn new(location: Location, settings: Settings) -> App {
        let fif = FifForm {
            query: LineEdit::default(),
            dir: LineEdit::new(&settings.fif_dir),
            filter: LineEdit::new(&settings.fif_filter),
            cancel: None,
            rx: None,
        };
        let mut app = App {
            docs: Vec::new(),
            active: 0,
            settings,
            location,
            find: find::FindState::default(),
            palette: None,
            results: None,
            fif,
            focus: Focus::Editor,
            closed: Vec::new(),
            untitled: 0,
            overwrite: false,
            toast: None,
            layout: EditorLayout::default(),
            quit: false,
            menu_dirty: true,
            menu_rebuild: false,
            #[cfg(not(windows))]
            menu: crate::ui::menu::Menu::default(),
            close_window: false,
            fullscreen_request: false,
            font_dirty: true,
            reveal_caret: true,
            last_edit: Instant::now(),
            recovery_due: None,
            waker: None,
            input: input::InputState::default(),
            preview: crate::ui::markdown::Preview::default(),
            tab_scroll: 0,
            outline_scroll: 0,
            bracket_pair: None,
        };
        app.new_doc();
        app
    }

    // ------------------------------------------------------------ accessors

    pub fn doc(&self) -> &Doc {
        &self.docs[self.active]
    }

    pub fn doc_mut(&mut self) -> &mut Doc {
        &mut self.docs[self.active]
    }

    pub fn indent_cfg(&self) -> IndentCfg {
        IndentCfg { use_tabs: self.settings.use_tabs, width: self.settings.tab_width, auto: self.settings.auto_indent }
    }

    /// Applies the interface language from the settings (or Windows).
    pub fn apply_language(&mut self) {
        let l = self.settings.language.unwrap_or_else(crate::i18n::system_lang);
        crate::i18n::set_lang(l);
        self.menu_rebuild = true;
        self.menu_dirty = true;
    }

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    // ------------------------------------------------------------ documents

    pub fn new_doc(&mut self) {
        self.untitled += 1;
        let d = Doc::untitled(self.untitled, self.settings.new_enc, self.settings.new_eol);
        self.docs.push(d);
        self.active = self.docs.len() - 1;
        self.after_switch();
    }

    fn after_switch(&mut self) {
        self.focus = Focus::Editor;
        self.reveal_caret = true;
        self.menu_dirty = true;
        self.find.invalidate();
    }

    pub fn activate(&mut self, i: usize) {
        if i < self.docs.len() {
            self.active = i;
            self.after_switch();
        }
    }

    pub fn find_doc_by_path(&self, p: &Path) -> Option<usize> {
        let canon = |x: &Path| std::fs::canonicalize(x).unwrap_or_else(|_| x.to_path_buf());
        let target = canon(p);
        self.docs.iter().position(|d| d.path.as_ref().is_some_and(|dp| canon(dp) == target))
    }

    /// Opens a file (or switches to it if already open).
    pub fn open_path(&mut self, path: &Path, host: &dyn Host) -> bool {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(i) = self.find_doc_by_path(&path) {
            self.activate(i);
            return true;
        }
        match file::load(&path, None) {
            Ok(l) => {
                let doc = Doc::from_loaded(&path, l);
                self.insert_doc(doc);
                self.settings.add_recent(&path);
                self.menu_dirty = true;
                true
            }
            Err(e) => {
                host.error(&tr("Open"), &tf("Cannot open {}:\n\n{}", &[&path.display().to_string(), &e.to_string()]));
                if matches!(e, LoadError::NotFound) {
                    self.settings.recent.retain(|r| r != &path);
                    self.menu_dirty = true;
                }
                false
            }
        }
    }

    /// Opens a command-line argument. A path that does not exist yet (but
    /// whose folder does) becomes an empty tab that creates the file on save.
    pub fn open_arg(&mut self, path: &Path, host: &dyn Host) -> bool {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let parent_ok = path.parent().is_some_and(|p| p.as_os_str().is_empty() || p.is_dir());
        if path.exists() || !parent_ok {
            return self.open_path(&path, host);
        }
        if let Some(i) = self.find_doc_by_path(&path) {
            self.activate(i);
            return true;
        }
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let lang = syntax::detect(Some(&path), "");
        let mut d = Doc::with_buffer(Buffer::new(""), Some(path), name, self.settings.new_enc, self.settings.new_eol, lang);
        d.notice = Some(Notice::Info(Msg::new("This file does not exist yet. It will be created when you save.")));
        self.insert_doc(d);
        true
    }

    /// Adds a document, replacing a pristine "Untitled" tab if that is active.
    fn insert_doc(&mut self, doc: Doc) {
        if self.docs.len() == 1 && self.docs[0].is_pristine() {
            self.docs[0] = doc;
            self.active = 0;
        } else {
            self.docs.insert(self.active + 1, doc);
            self.active += 1;
        }
        self.after_switch();
    }

    /// Asks to save a modified document. Returns false if cancelled.
    fn confirm_close(&mut self, i: usize, host: &dyn Host) -> bool {
        if !self.docs[i].is_modified() {
            return true;
        }
        self.active = i;
        match host.ask_save(&self.docs[i].display_name()) {
            Answer::Cancel => false,
            Answer::No => true,
            Answer::Yes => self.save_doc(i, false, host),
        }
    }

    pub fn close_doc(&mut self, i: usize, host: &dyn Host) -> bool {
        if i >= self.docs.len() || !self.confirm_close(i, host) {
            return false;
        }
        let d = self.docs.remove(i);
        if let Some(p) = &d.path {
            self.closed.push((p.clone(), d.buf.primary_sel().head));
            self.closed.truncate(50);
        }
        self.delete_recovery(&d);
        if self.docs.is_empty() {
            match self.settings.last_tab_close {
                LastTabClose::KeepEmpty => self.new_doc(),
                LastTabClose::CloseWindow => {
                    self.new_doc();
                    self.close_window = true;
                }
            }
        }
        if self.active >= self.docs.len() || (self.active > i) {
            self.active = self.active.saturating_sub(1).min(self.docs.len() - 1);
        }
        self.after_switch();
        true
    }

    fn delete_recovery(&self, d: &Doc) {
        if let Some(dir) = self.location.file(session::RECOVERY_DIR) {
            let _ = std::fs::remove_file(dir.join(session::recovery_name(d.id)));
        }
    }

    /// Saves document `i`; `force_dialog` for Save As. Returns success.
    pub fn save_doc(&mut self, i: usize, force_dialog: bool, host: &dyn Host) -> bool {
        let d = &self.docs[i];
        if d.view_only && !force_dialog {
            host.info(&tr("Save"), &tr("This document is read-only. Use Save As to save a copy."));
            return false;
        }
        let path = match (&d.path, force_dialog || d.read_only) {
            (Some(p), false) => p.clone(),
            (p, _) => {
                let name = p.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| format!("{}.txt", d.display_name()));
                let dir = p.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf);
                match host.save_dialog(&name, dir.as_deref()) {
                    Some(p) => p,
                    None => return false,
                }
            }
        };
        let same_file = d.path.as_deref() == Some(path.as_path());
        if same_file {
            if d.changed_on_disk() == Some(true) && path.exists() {
                let ok = host.confirm(
                    &tr("File changed on disk"),
                    &tf("{} was changed by another program since it was opened.\n\nOverwrite it with your version?", &[&path.display().to_string()]),
                );
                if !ok {
                    return false;
                }
            }
            if d.lossy {
                let ok = host.confirm(
                    &tr("Replacement characters"),
                    &tr("This document contains bytes that could not be decoded and are shown as \u{FFFD}.\n\nSaving will write \u{FFFD} in their place. Continue?"),
                );
                if !ok {
                    return false;
                }
            }
        }
        if self.settings.local_history && path.exists() {
            let _ = session::snapshot_before_save(&self.location, &path, &timestamp(), self.settings.history_keep);
        }
        let d = &mut self.docs[i];
        match d.save_to(&path) {
            Ok(()) => {
                d.view_only = false;
                let doc_snapshot = d.id;
                self.settings.add_recent(&path);
                self.menu_dirty = true;
                let dd = self.docs.iter().find(|x| x.id == doc_snapshot).expect("doc");
                self.delete_recovery(dd);
                self.notify(tf("Saved {}", &[&path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()]));
                true
            }
            Err(e) => {
                host.error(&tr("Save"), &tf("Could not save {}:\n\n{}", &[&path.display().to_string(), &e.to_string()]));
                false
            }
        }
    }

    pub fn reload(&mut self, i: usize, forced: Option<Enc>, host: &dyn Host) {
        let Some(path) = self.docs[i].path.clone() else { return };
        if self.docs[i].is_modified() && !host.confirm(&tr("Reload"), &tf("Discard your changes to {} and reload it from disk?\n\n(You can undo the reload.)", &[&self.docs[i].display_name()])) {
            return;
        }
        match file::load(&path, forced) {
            Ok(l) => {
                let fresh = Doc::from_loaded(&path, l);
                let d = &mut self.docs[i];
                let caret = d.buf.primary_sel().head;
                d.buf.replace_all_text(&fresh.buf.text.to_string());
                d.buf.mark_saved();
                d.buf.set_caret(caret.min(d.buf.len()));
                d.enc = fresh.enc;
                d.eol = fresh.eol;
                d.stamp = fresh.stamp;
                d.lossy = fresh.lossy;
                d.read_only = fresh.read_only;
                d.view_only = fresh.view_only;
                d.notice = fresh.notice;
                d.hl.invalidate(0);
                d.folds_version = u64::MAX;
                self.find.invalidate();
            }
            Err(e) => host.error(&tr("Reload"), &tf("Cannot reload {}:\n\n{}", &[&path.display().to_string(), &e.to_string()])),
        }
    }

    /// Checks open files for external changes (on window focus).
    pub fn check_external_changes(&mut self) {
        for d in self.docs.iter_mut() {
            let Some(p) = &d.path else { continue };
            if d.notice == Some(Notice::ChangedOnDisk) || d.notice == Some(Notice::Deleted) {
                continue;
            }
            if !p.exists() {
                if d.stamp.is_some() {
                    d.notice = Some(Notice::Deleted);
                }
                continue;
            }
            if d.changed_on_disk() == Some(true) {
                d.notice = Some(Notice::ChangedOnDisk);
            }
        }
    }

    /// Keep the user's version after an external change.
    pub fn keep_mine(&mut self) {
        let d = self.doc_mut();
        d.notice = None;
        // Saving later will still ask before overwriting the newer file.
    }

    /// Asks about all modified documents (exit without recovery). False if cancelled.
    pub fn confirm_all(&mut self, host: &dyn Host) -> bool {
        for i in 0..self.docs.len() {
            if !self.confirm_close(i, host) {
                return false;
            }
        }
        true
    }

    // ------------------------------------------------------------ editing helpers

    /// Runs an edit on the active buffer if it is editable, then updates caches.
    pub fn edit(&mut self, f: impl FnOnce(&mut Buffer)) {
        if !self.doc().editable() {
            self.notify(tr("This document is read-only"));
            return;
        }
        let lines_before = self.doc().buf.line_count();
        let v = self.doc().buf.version;
        f(&mut self.doc_mut().buf);
        self.after_edit(lines_before, v);
    }

    fn after_edit(&mut self, lines_before: usize, version_before: u64) {
        let d = self.doc_mut();
        if let Some(line) = d.buf.take_dirty() {
            d.hl.invalidate(line);
            let delta = d.buf.line_count() as isize - lines_before as isize;
            d.shift_folds(line, delta);
            let caret_line = d.buf.line_of(d.buf.primary_sel().head);
            d.reveal(caret_line);
        }
        d.goals.clear();
        if d.buf.version != version_before {
            self.last_edit = Instant::now();
            self.find.invalidate();
            if self.settings.recovery && self.recovery_due.is_none() {
                self.recovery_due = Some(Instant::now() + RECOVERY_DELAY);
            }
        }
        self.reveal_caret = true;
        self.menu_dirty = true;
    }

    /// Runs a line transformation, confirming first when it changes many lines.
    fn line_tool(&mut self, name: &str, f: &dyn Fn(Vec<String>) -> Vec<String>, host: &dyn Host) {
        let name = &tr(name);
        if !self.doc().editable() {
            self.notify(tr("This document is read-only"));
            return;
        }
        let (total, changed) = self.doc().buf.preview_lines(f);
        if changed == 0 {
            self.notify(tf("{}: nothing to change", &[name]));
            return;
        }
        let whole = !self.doc().buf.has_selection();
        if (whole && changed > 50) || changed > 1000 {
            let msg = if whole {
                "{} will change {} of {} lines in the whole document.\n\nYou can undo this with Ctrl+Z. Continue?"
            } else {
                "{} will change {} of {} lines in the selected lines.\n\nYou can undo this with Ctrl+Z. Continue?"
            };
            let ok = host.confirm(name, &tf(msg, &[name, &changed.to_string(), &total.to_string()]));
            if !ok {
                return;
            }
        }
        let mut n = 0;
        self.edit(|b| n = b.transform_lines(f));
        self.notify(tf("{}: {} line(s) changed", &[name, &n.to_string()]));
    }

    /// Applies a string transformation to the selections (or whole document
    /// when nothing is selected, after confirmation).
    fn text_tool(&mut self, name: &str, f: &dyn Fn(&str) -> Result<String, String>, host: &dyn Host) {
        let name = &tr(name);
        if !self.doc().editable() {
            self.notify(tr("This document is read-only"));
            return;
        }
        let has_sel = self.doc().buf.has_selection();
        if !has_sel {
            let n = self.doc().buf.line_count();
            if n > 1 && !host.confirm(name, &tf("No text is selected. Apply \"{}\" to the whole document ({} lines)?\n\nYou can undo this with Ctrl+Z.", &[name, &n.to_string()])) {
                return;
            }
            self.edit(|b| b.select_all());
        }
        // Validate all selections first so nothing changes on error.
        for t in self.doc().buf.selected_texts() {
            if let Err(e) = f(&t) {
                host.error(name, &e);
                return;
            }
        }
        let mut changed = 0;
        self.edit(|b| changed = b.transform_selections(&|s: &str| f(s).unwrap_or_else(|_| s.to_string())));
        if changed == 0 {
            self.notify(tf("{}: nothing to change", &[name]));
        }
    }

    // ------------------------------------------------------------ clipboard

    pub fn copy(&mut self, cut: bool, host: &dyn Host) {
        let b = &self.doc().buf;
        let text = if b.has_selection() {
            b.selected_texts().into_iter().filter(|t| !t.is_empty() || b.sels.len() > 1).collect::<Vec<_>>().join("\n")
        } else {
            // No selection: copy/cut the current line(s), like most editors.
            let lines: Vec<String> = b.sel_line_ranges().iter().flat_map(|(a, z)| (*a..=*z).map(|l| b.line_text(l))).collect();
            format!("{}\n", lines.join("\n"))
        };
        host.clipboard_set(&text);
        if cut {
            if self.doc().buf.has_selection() {
                self.edit(|b| b.insert_text("", false));
            } else {
                self.edit(|b| b.delete_lines());
            }
        }
    }

    pub fn paste(&mut self, host: &dyn Host) {
        let Some(text) = host.clipboard_get() else { return };
        let (text, _) = crate::io::encoding::normalize_eol(&text);
        let n = self.doc().buf.sels.len();
        let lines: Vec<String> = text.trim_end_matches('\n').split('\n').map(str::to_string).collect();
        if n > 1 && lines.len() == n {
            self.edit(|b| b.insert_per_selection(&lines));
        } else {
            let ov = self.overwrite;
            self.edit(|b| {
                b.seal();
                b.insert_text(&text, ov && false);
                b.seal();
            });
        }
    }

    // ------------------------------------------------------------ dispatch

    pub fn dispatch(&mut self, cmd: Command, host: &dyn Host) {
        use Command::*;
        self.menu_dirty = true;
        let tool_cfg = self.indent_cfg();
        match cmd {
            New => self.new_doc(),
            Open => {
                let dir = self.doc().path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf);
                for p in host.open_dialog(dir.as_deref()) {
                    self.open_path(&p, host);
                }
            }
            OpenRecent(i) => {
                if let Some(p) = self.settings.recent.get(i as usize).cloned() {
                    self.open_path(&p, host);
                }
            }
            ClearRecent => self.settings.recent.clear(),
            ReopenClosed => {
                if let Some((p, caret)) = self.closed.pop() {
                    if self.open_path(&p, host) {
                        let d = self.doc_mut();
                        let c = caret.min(d.buf.len());
                        d.buf.set_caret(c);
                        self.reveal_caret = true;
                    }
                } else {
                    self.notify(tr("No closed tabs to reopen"));
                }
            }
            Save => {
                self.save_doc(self.active, false, host);
            }
            SaveAs => {
                self.save_doc(self.active, true, host);
            }
            SaveAll => {
                for i in 0..self.docs.len() {
                    if self.docs[i].is_modified() {
                        self.save_doc(i, false, host);
                    }
                }
            }
            Reload => self.reload(self.active, None, host),
            ReopenWithEncoding(i) => {
                if let Some(e) = Enc::all().get(i as usize).copied() {
                    if self.doc().path.is_none() {
                        self.notify(tr("Save the document first; reopening needs a file on disk"));
                    } else {
                        self.reload(self.active, Some(e), host);
                    }
                }
            }
            LocalHistory => self.open_history_list(),
            CloseTab => {
                self.close_doc(self.active, host);
            }
            CloseOthers => {
                let keep = self.doc().id;
                let mut i = 0;
                while i < self.docs.len() {
                    if self.docs[i].id != keep {
                        if !self.close_doc(i, host) {
                            break;
                        }
                    } else {
                        i += 1;
                    }
                }
                if let Some(k) = self.docs.iter().position(|d| d.id == keep) {
                    self.activate(k);
                }
            }
            CloseAll => {
                while !self.docs.is_empty() {
                    let before = self.docs.len();
                    if !self.close_doc(0, host) || (self.docs.len() == 1 && self.docs[0].is_pristine() && before == 1) {
                        break;
                    }
                }
            }
            CopyPath => match &self.doc().path {
                Some(p) => {
                    host.clipboard_set(&p.display().to_string());
                    self.notify(tr("Path copied"));
                }
                None => self.notify(tr("This document has not been saved yet")),
            },
            RevealInExplorer => {
                if let Some(p) = self.doc().path.clone() {
                    host.reveal(&p);
                }
            }
            Exit => self.quit = true,
            Undo => self.edit(|b| {
                b.undo();
            }),
            Redo => self.edit(|b| {
                b.redo();
            }),
            Cut => self.copy(true, host),
            Copy => self.copy(false, host),
            Paste => self.paste(host),
            Delete => self.edit(|b| b.delete_forward()),
            SelectAll => self.edit(|b| b.select_all()),
            Duplicate => self.edit(|b| b.duplicate()),
            DeleteLine => self.edit(|b| b.delete_lines()),
            MoveLineUp => self.edit(|b| b.move_lines(true)),
            MoveLineDown => self.edit(|b| b.move_lines(false)),
            JoinLines => self.edit(|b| b.join_lines()),
            Indent => self.edit(|b| b.indent_lines(&tool_cfg)),
            Outdent => self.edit(|b| b.outdent_lines(&tool_cfg)),
            ToggleComment => match syntax::def(self.doc().lang).comment_token() {
                Some(t) => {
                    let t = t.trim_end().to_string();
                    self.edit(|b| b.toggle_comment(&t));
                }
                None => self.notify(tr("This language has no line comments")),
            },
            CaretAbove => {
                let tab = self.settings.tab_width;
                self.edit(|b| b.add_caret_vertical(false, tab));
            }
            CaretBelow => {
                let tab = self.settings.tab_width;
                self.edit(|b| b.add_caret_vertical(true, tab));
            }
            AddNextOccurrence => self.edit(|b| {
                b.add_next_occurrence();
            }),
            SelectAllOccurrences => {
                let mut n = 0;
                self.edit(|b| n = b.select_all_occurrences());
                self.notify(format!("{n} occurrence(s) selected"));
            }
            SplitSelectionIntoLines => self.edit(|b| {
                let mut sels = Vec::new();
                for s in b.sels.clone() {
                    let (a, z) = (b.line_of(s.start()), b.line_of(s.end()));
                    if a == z {
                        sels.push(s);
                        continue;
                    }
                    for l in a..=z {
                        let st = if l == a { s.start() } else { b.line_start(l) };
                        let en = if l == z { s.end() } else { b.line_end(l) };
                        sels.push(Sel::new(st, en));
                    }
                }
                b.set_sels(sels, 0);
            }),
            InsertDateTime => {
                let t = timestamp_display();
                self.edit(|b| b.insert_text(&t, false));
            }
            InsertSnippet => self.open_snippets_list(host),
            SaveSnippet => {
                if self.doc().buf.has_selection() {
                    self.open_prompt(PromptKind::SnippetName, &tr("Snippet name"), "");
                } else {
                    self.notify(tr("Select the text to save as a snippet first"));
                }
            }
            EditSnippets => self.edit_snippets_file(host),
            ToggleOverwrite => self.overwrite = !self.overwrite,
            Find => self.open_find(false),
            Replace => self.open_find(true),
            FindNext => self.find_step(true, host),
            FindPrev => self.find_step(false, host),
            FindInDocs => self.find_in_docs(host),
            FindInFiles => self.open_fif(),
            GotoLine => {
                let n = self.doc().buf.line_count();
                self.open_palette(PaletteMode::GotoLine, &tf("Go to line (1 - {}); use line:column for a column", &[&n.to_string()]), Vec::new());
            }
            GotoBracket => self.goto_bracket(),
            GotoSymbol => self.open_symbols(),
            WordWrap => {
                self.settings.word_wrap = !self.settings.word_wrap;
                self.doc_mut().scroll_x = 0.0;
                self.reveal_caret = true;
            }
            LineNumbers => self.settings.line_numbers = !self.settings.line_numbers,
            HighlightLine => self.settings.highlight_line = !self.settings.highlight_line,
            ShowWhitespace => self.settings.show_whitespace = !self.settings.show_whitespace,
            ShowEol => self.settings.show_eol = !self.settings.show_eol,
            ShowRuler => self.settings.show_ruler = !self.settings.show_ruler,
            FoldMargin => self.settings.fold_margin = !self.settings.fold_margin,
            FoldCurrent | UnfoldCurrent => {
                self.ensure_folds(true);
                let d = self.doc_mut();
                let line = d.buf.line_of(d.buf.primary_sel().head);
                if let Some((s, _)) = d.fold_containing(line) {
                    if cmd == FoldCurrent {
                        d.folded.insert(s);
                        d.buf.set_caret(d.buf.line_start(s));
                    } else {
                        d.folded.remove(&s);
                    }
                }
            }
            FoldAll => {
                self.ensure_folds(true);
                let d = self.doc_mut();
                d.folded = d.folds.iter().map(|f| f.0).collect();
                let line = d.buf.line_of(d.buf.primary_sel().head);
                if d.is_hidden(line) {
                    let top = d.folds.iter().filter(|(s, e)| *s < line && line <= *e).map(|f| f.0).min().unwrap_or(0);
                    d.buf.set_caret(d.buf.line_start(top));
                }
                self.reveal_caret = true;
            }
            UnfoldAll => self.doc_mut().folded.clear(),
            ZoomIn => self.settings.zoom = (self.settings.zoom + 10).min(400),
            ZoomOut => self.settings.zoom = (self.settings.zoom - 10).max(30),
            ZoomReset => self.settings.zoom = 100,
            Toolbar => self.settings.toolbar = !self.settings.toolbar,
            StatusBar => self.settings.status_bar = !self.settings.status_bar,
            Outline => self.settings.outline = !self.settings.outline,
            MarkdownPreview => self.settings.markdown_preview = !self.settings.markdown_preview,
            ResultsPanel => {
                if self.results.is_some() {
                    self.results = None;
                    self.focus = Focus::Editor;
                } else {
                    self.results = Some(Results { title: tr("Search results"), items: Vec::new(), sel: 0, scroll: 0, status: tr("No search yet"), fif: false });
                }
            }
            Theme(i) => {
                self.settings.theme = match i {
                    1 => ThemePref::Light,
                    2 => ThemePref::Dark,
                    _ => ThemePref::System,
                }
            }
            UiLanguage(i) => {
                self.settings.language = (i as usize).checked_sub(1).and_then(|k| crate::i18n::Lang::ALL.get(k)).copied();
                self.apply_language();
            }
            FullScreen => self.fullscreen_request = true,
            CommandPalette => self.open_commands(),
            NextTab | PrevTab => {
                let n = self.docs.len();
                let i = if cmd == NextTab { (self.active + 1) % n } else { (self.active + n - 1) % n };
                self.activate(i);
            }
            LangAuto => {
                let d = self.doc_mut();
                let first = d.buf.line_text(0);
                let l = syntax::detect(d.path.as_deref(), &first);
                d.set_lang(l, false);
            }
            SetLang(i) => {
                if let Some(def) = syntax::lang::DEFS.get(i as usize) {
                    let l = def.lang;
                    let d = self.doc_mut();
                    d.set_lang(l, true);
                    if l != Lang::Plain && d.large {
                        d.hl.enabled = true;
                    }
                }
            }
            SetEncoding(i) => {
                if let Some(e) = Enc::all().get(i as usize).copied() {
                    self.set_encoding(e, host);
                }
            }
            SetEol(i) => {
                if let Some((_, e)) = EOLS.get(i as usize) {
                    let d = self.doc_mut();
                    if d.eol != *e {
                        d.eol = *e;
                        d.buf.mark_unsaved();
                        if matches!(&d.notice, Some(Notice::MixedEol(_))) {
                            d.notice = None;
                        }
                        self.notify(tf("Line endings will be saved as {}", &[e.name()]));
                    }
                }
            }
            UseSpaces => self.settings.use_tabs = false,
            UseTabs => self.settings.use_tabs = true,
            TabWidth(w) => self.settings.tab_width = w as usize,
            IndentToSpaces => {
                let w = self.settings.tab_width;
                self.line_tool("Convert indentation to spaces", &move |v| {
                    v.into_iter()
                        .map(|l| {
                            let ws: String = l.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
                            let exp = tt::tabs_to_spaces(vec![ws.clone()], w).remove(0);
                            format!("{exp}{}", &l[ws.len()..])
                        })
                        .collect()
                }, host);
                self.settings.use_tabs = false;
            }
            IndentToTabs => {
                let w = self.settings.tab_width;
                self.line_tool("Convert indentation to tabs", &move |v| tt::leading_spaces_to_tabs(v, w), host);
                self.settings.use_tabs = true;
            }
            AutoIndent => self.settings.auto_indent = !self.settings.auto_indent,
            Font => {
                let face = if self.settings.font_face.is_empty() { "Consolas".to_string() } else { self.settings.font_face.clone() };
                if let Some((f, size)) = host.choose_font(&face, self.settings.font_size) {
                    self.settings.font_face = f;
                    self.settings.font_size = size.clamp(6.0, 72.0);
                    self.font_dirty = true;
                }
            }
            Upper => self.text_tool("UPPERCASE", &|s| Ok(tt::upper(s)), host),
            Lower => self.text_tool("lowercase", &|s| Ok(tt::lower(s)), host),
            TitleCase => self.text_tool("Title Case", &|s| Ok(tt::title(s)), host),
            SentenceCase => self.text_tool("Sentence case", &|s| Ok(tt::sentence(s)), host),
            InvertCase => self.text_tool("Invert case", &|s| Ok(tt::invert(s)), host),
            SortAsc => self.line_tool("Sort ascending", &|v| tt::sort(v, tt::SortMode::Ascending), host),
            SortDesc => self.line_tool("Sort descending", &|v| tt::sort(v, tt::SortMode::Descending), host),
            SortCi => self.line_tool("Sort ignoring case", &|v| tt::sort(v, tt::SortMode::CaseInsensitive), host),
            SortNatural => self.line_tool("Natural sort", &|v| tt::sort(v, tt::SortMode::Natural), host),
            Reverse => self.line_tool("Reverse lines", &tt::reverse, host),
            Dedupe => self.line_tool("Remove duplicate lines", &tt::remove_duplicates, host),
            RemoveBlank => self.line_tool("Remove blank lines", &tt::remove_blank, host),
            TrimTrailing => self.line_tool("Trim trailing whitespace", &tt::trim_trailing, host),
            TrimLeading => self.line_tool("Trim leading whitespace", &tt::trim_leading, host),
            PrefixSuffix => self.open_prompt(PromptKind::PrefixSuffix, &tr("Prefix|suffix for each line (text before | is added at the start, after | at the end)"), ""),
            TabsToSpaces => {
                let w = self.settings.tab_width;
                self.line_tool("Convert tabs to spaces", &move |v| tt::tabs_to_spaces(v, w), host);
            }
            SpacesToTabs => {
                let w = self.settings.tab_width;
                self.line_tool("Convert leading spaces to tabs", &move |v| tt::leading_spaces_to_tabs(v, w), host);
            }
            JsonEscape => self.text_tool("JSON escape", &|s| Ok(tt::json_escape(s)), host),
            JsonUnescape => self.text_tool("JSON unescape", &tt::json_unescape, host),
            XmlEscape => self.text_tool("XML escape", &|s| Ok(tt::xml_escape(s)), host),
            XmlUnescape => self.text_tool("XML unescape", &|s| Ok(tt::xml_unescape(s)), host),
            UrlEncode => self.text_tool("URL encode", &|s| Ok(tt::url_encode(s)), host),
            UrlDecode => self.text_tool("URL decode", &tt::url_decode, host),
            B64Encode => self.text_tool("Base64 encode", &|s| Ok(tt::base64_encode(s)), host),
            B64Decode => self.text_tool("Base64 decode", &tt::base64_decode, host),
            XmlCheck => self.xml_check(host),
            XmlFormat => self.xml_format(host),
            OptRestoreSession => self.settings.restore_session = !self.settings.restore_session,
            OptRecovery => {
                self.settings.recovery = !self.settings.recovery;
                if !self.settings.recovery {
                    session::prune_recovery(&self.location, &[]);
                    for d in self.docs.iter_mut() {
                        d.recovery_version = None;
                    }
                    self.notify(tr("Recovery is off: unsaved changes are no longer kept, and you will be asked to save on exit"));
                } else {
                    self.recovery_due = Some(Instant::now());
                }
            }
            OptAutosave => self.settings.autosave_on_blur = !self.settings.autosave_on_blur,
            OptLocalHistory => self.settings.local_history = !self.settings.local_history,
            OptSingleInstance => self.settings.single_instance = !self.settings.single_instance,
            LastTabKeep => self.settings.last_tab_close = LastTabClose::KeepEmpty,
            LastTabCloseWindow => self.settings.last_tab_close = LastTabClose::CloseWindow,
            OpenDataFolder => match self.location.dir.clone() {
                Some(d) => {
                    let _ = std::fs::create_dir_all(&d);
                    host.reveal(&d.join(crate::storage::FILE_NAME));
                }
                None => host.info(&tr("Settings folder"), &tr("No settings folder is available on this system.")),
            },
            Shortcuts => self.open_shortcuts_doc(),
            About => host.info(
                &tr("About rpad"),
                &tf(
                    if self.location.portable {
                        "rpad {}\n\nA portable text and code editor written in Rust.\nNo telemetry, no network access.\n\nSettings and recovery data (portable mode): {}\n\nMIT License."
                    } else {
                        "rpad {}\n\nA portable text and code editor written in Rust.\nNo telemetry, no network access.\n\nSettings and recovery data: {}\n\nMIT License."
                    },
                    &[env!("CARGO_PKG_VERSION"), &self.location.dir.as_ref().map_or_else(|| tr("(not available)"), |d| d.display().to_string())],
                ),
            ),
        }
    }

    /// Menu check marks.
    pub fn checked(&self, cmd: Command) -> bool {
        use Command::*;
        let s = &self.settings;
        let d = self.doc();
        match cmd {
            WordWrap => s.word_wrap,
            LineNumbers => s.line_numbers,
            HighlightLine => s.highlight_line,
            ShowWhitespace => s.show_whitespace,
            ShowEol => s.show_eol,
            ShowRuler => s.show_ruler,
            FoldMargin => s.fold_margin,
            Toolbar => s.toolbar,
            StatusBar => s.status_bar,
            Outline => s.outline,
            MarkdownPreview => s.markdown_preview,
            ResultsPanel => self.results.is_some(),
            Theme(i) => matches!((i, s.theme), (0, ThemePref::System) | (1, ThemePref::Light) | (2, ThemePref::Dark)),
            UiLanguage(0) => s.language.is_none(),
            UiLanguage(i) => s.language == crate::i18n::Lang::ALL.get(i as usize - 1).copied(),
            LangAuto => !d.lang_override,
            SetLang(i) => syntax::lang::DEFS.get(i as usize).is_some_and(|x| x.lang == d.lang),
            SetEncoding(i) => Enc::all().get(i as usize) == Some(&d.enc),
            SetEol(i) => EOLS.get(i as usize).is_some_and(|e| e.1 == d.eol),
            UseSpaces => !s.use_tabs,
            UseTabs => s.use_tabs,
            TabWidth(w) => s.tab_width == w as usize,
            AutoIndent => s.auto_indent,
            ToggleOverwrite => self.overwrite,
            OptRestoreSession => s.restore_session,
            OptRecovery => s.recovery,
            OptAutosave => s.autosave_on_blur,
            OptLocalHistory => s.local_history,
            OptSingleInstance => s.single_instance,
            LastTabKeep => s.last_tab_close == LastTabClose::KeepEmpty,
            LastTabCloseWindow => s.last_tab_close == LastTabClose::CloseWindow,
            _ => false,
        }
    }

    /// Menu enabled state.
    pub fn enabled(&self, cmd: Command) -> bool {
        use Command::*;
        let d = self.doc();
        let editable = d.editable();
        match cmd {
            Undo => editable && d.buf.can_undo(),
            Redo => editable && d.buf.can_redo(),
            Cut | Paste | Delete | Duplicate | DeleteLine | MoveLineUp | MoveLineDown | JoinLines | Indent | Outdent | ToggleComment
            | InsertDateTime | InsertSnippet | Upper | Lower | TitleCase | SentenceCase | InvertCase | SortAsc | SortDesc | SortCi
            | SortNatural | Reverse | Dedupe | RemoveBlank | TrimTrailing | TrimLeading | PrefixSuffix | TabsToSpaces | SpacesToTabs
            | JsonEscape | JsonUnescape | XmlEscape | XmlUnescape | UrlEncode | UrlDecode | B64Encode | B64Decode | XmlFormat
            | IndentToSpaces | IndentToTabs => editable,
            Reload | CopyPath | RevealInExplorer | LocalHistory => d.path.is_some(),
            ReopenWithEncoding(_) => d.path.is_some(),
            ReopenClosed => !self.closed.is_empty(),
            SaveSnippet => d.buf.has_selection(),
            ClearRecent => !self.settings.recent.is_empty(),
            _ => true,
        }
    }

    fn set_encoding(&mut self, e: Enc, host: &dyn Host) {
        let d = self.doc();
        if d.enc == e {
            return;
        }
        // Make sure the text can be represented before switching.
        if let Err(err) = file::encode_rope(&d.buf.text, e, d.eol) {
            host.error(&tr("Encoding"), &tf("The document cannot be saved as {}: {}.\n\nThe encoding was not changed.", &[&e.name(), &err.to_string()]));
            return;
        }
        let d = self.doc_mut();
        d.enc = e;
        d.buf.mark_unsaved();
        self.notify(tf("The document will be saved as {}", &[&e.name()]));
    }

    // ------------------------------------------------------------ folding / outline caches

    /// Recomputes fold ranges if stale (`force` ignores the edit debounce).
    pub fn ensure_folds(&mut self, force: bool) {
        let settle = self.last_edit.elapsed() > Duration::from_millis(300);
        let d = self.doc_mut();
        if d.large || d.folds_version == d.buf.version || !(force || settle) {
            return;
        }
        d.folds = syntax::fold::compute(&d.buf.text, d.lang);
        let starts: std::collections::BTreeSet<usize> = d.folds.iter().map(|f| f.0).collect();
        d.folded.retain(|s| starts.contains(s));
        d.folds_version = d.buf.version;
    }

    pub fn ensure_outline(&mut self) {
        let d = self.doc_mut();
        if d.outline_version == d.buf.version || d.large {
            return;
        }
        let text = d.buf.text.to_string();
        d.outline = syntax::outline::outline(d.lang, text.lines());
        d.outline_version = d.buf.version;
    }

    // ------------------------------------------------------------ navigation

    pub fn goto_line(&mut self, line: usize, col: Option<usize>) {
        let d = self.doc_mut();
        let l = line.min(d.buf.line_count().saturating_sub(1));
        let start = d.buf.line_start(l);
        let len = d.buf.line_end(l) - start;
        let p = start + col.unwrap_or(0).min(len);
        d.reveal(l);
        d.buf.set_caret(p);
        self.reveal_caret = true;
        self.focus = Focus::Editor;
    }

    /// Bracket at/before the caret and its match, skipping brackets in
    /// strings and comments. Scans at most `limit` characters.
    pub fn matching_bracket_within(&mut self, limit: usize) -> Option<(usize, usize)> {
        let d = &mut self.docs[self.active];
        let head = d.buf.primary_sel().head;
        let markup = matches!(d.lang, Lang::Xml | Lang::Html);
        let pairs = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];
        let mut found = None;
        for p in [Some(head), head.checked_sub(1)].into_iter().flatten() {
            if let Some(c) = d.buf.char_at(p) {
                if let Some(&(o, cl)) = pairs.iter().find(|(o, cl)| (*o == c || *cl == c) && (markup || *o != '<')) {
                    found = Some((p, c == o, o, cl));
                    break;
                }
            }
        }
        let (pos, forward, open, close) = found?;
        let mut tokens = TokenProbe::default();
        if !tokens.is_code(d, pos) {
            return None;
        }
        let len = d.buf.len();
        let mut depth = 0i64;
        let mut p = pos;
        for _ in 0..limit {
            let ch = d.buf.text.char(p);
            if (ch == open || ch == close) && tokens.is_code(d, p) {
                depth += if (ch == open) == forward { 1 } else { -1 };
                if depth == 0 {
                    return Some((pos, p));
                }
            }
            if forward {
                p += 1;
                if p >= len {
                    return None;
                }
            } else {
                p = p.checked_sub(1)?;
            }
        }
        None
    }

    pub fn matching_bracket(&mut self) -> Option<(usize, usize)> {
        self.matching_bracket_within(4_000_000)
    }

    fn goto_bracket(&mut self) {
        match self.matching_bracket() {
            Some((_, m)) => {
                let d = self.doc_mut();
                d.buf.set_caret(m);
                let l = d.buf.line_of(m);
                d.reveal(l);
                self.reveal_caret = true;
            }
            None => self.notify(tr("No matching bracket")),
        }
    }

    // ------------------------------------------------------------ palette

    pub fn open_palette(&mut self, mode: PaletteMode, title: &str, items: Vec<PItem>) {
        let shown = (0..items.len()).collect();
        self.palette = Some(Palette { mode, title: title.to_string(), edit: LineEdit::default(), items, shown, sel: 0, scroll: 0 });
        self.focus = Focus::Palette;
    }

    fn open_prompt(&mut self, kind: PromptKind, title: &str, initial: &str) {
        self.open_palette(PaletteMode::Prompt(kind), title, Vec::new());
        if let Some(p) = self.palette.as_mut() {
            p.edit.set(initial);
        }
    }

    fn open_commands(&mut self) {
        let items = Command::palette()
            .into_iter()
            .map(|(c, cat)| PItem {
                label: format!("{}: {}", tr(cat), c.plain_label()),
                detail: commands::shortcut(c).unwrap_or("").to_string(),
                act: PAct::Cmd(c),
            })
            .collect();
        self.open_palette(PaletteMode::Commands, &tr("Type a command"), items);
    }

    fn open_symbols(&mut self) {
        self.ensure_outline();
        let items: Vec<PItem> = self
            .doc()
            .outline
            .iter()
            .map(|s| PItem { label: format!("{}{}", "  ".repeat(s.depth.min(6)), s.name), detail: tf("{} · line {}", &[&tr(s.kind), &(s.line + 1).to_string()]), act: PAct::Line(s.line) })
            .collect();
        if items.is_empty() {
            self.notify(tf("No symbols found for {}", &[&tr(self.doc().lang.name())]));
            return;
        }
        self.open_palette(PaletteMode::Symbols, &tr("Go to symbol"), items);
    }

    fn snippets(&self) -> Vec<(String, String)> {
        self.location.read_text(session::SNIPPETS_FILE).map(|t| session::parse_snippets(&t)).unwrap_or_default()
    }

    fn open_snippets_list(&mut self, host: &dyn Host) {
        let snips = self.snippets();
        if snips.is_empty() {
            if host.confirm(&tr("Snippets"), &tr("You have no snippets yet. Open the snippets file to add some?")) {
                self.edit_snippets_file(host);
            }
            return;
        }
        let items = snips
            .into_iter()
            .map(|(n, body)| PItem { detail: body.lines().next().unwrap_or("").chars().take(60).collect(), label: n, act: PAct::Text(body) })
            .collect();
        self.open_palette(PaletteMode::Snippets, &tr("Insert snippet"), items);
    }

    fn edit_snippets_file(&mut self, host: &dyn Host) {
        let Some(path) = self.location.file(session::SNIPPETS_FILE) else {
            host.info(&tr("Snippets"), &tr("No settings folder is available, so snippets cannot be stored."));
            return;
        };
        if !path.exists() {
            if let Err(e) = self.location.write_text(session::SNIPPETS_FILE, session::SNIPPETS_TEMPLATE) {
                host.error(&tr("Snippets"), &tf("Cannot create {}: {}", &[&path.display().to_string(), &e.to_string()]));
                return;
            }
        }
        self.open_path(&path, host);
    }

    fn open_history_list(&mut self) {
        let Some(p) = self.doc().path.clone() else { return };
        let snaps = session::list_snapshots(&self.location, &p);
        if snaps.is_empty() {
            self.notify(if self.settings.local_history {
                tr("No local history for this file.")
            } else {
                tr("No local history for this file. Turn on Tools › Preferences › Keep Local History Snapshots on Save.")
            });
            return;
        }
        let items = snaps
            .into_iter()
            .map(|(path, stamp)| {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                PItem { label: stamp.replacen('_', "  ", 1).replace('-', ":").replacen(':', "-", 2), detail: tf("{} bytes", &[&size.to_string()]), act: PAct::Path(path) }
            })
            .collect();
        self.open_palette(PaletteMode::History, &tr("Open a snapshot of this file (read-only)"), items);
    }

    /// Filters palette items by the typed text (subsequence match).
    pub fn palette_filter(&mut self) {
        let Some(p) = self.palette.as_mut() else { return };
        if matches!(p.mode, PaletteMode::GotoLine | PaletteMode::Prompt(_)) {
            return;
        }
        let q = p.edit.text.to_lowercase();
        let mut scored: Vec<(usize, i32)> = p
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| fuzzy_score(&q, &it.label.to_lowercase()).map(|s| (i, s)))
            .collect();
        if !q.is_empty() {
            scored.sort_by_key(|(i, s)| (-*s, *i));
        }
        p.shown = scored.into_iter().map(|(i, _)| i).collect();
        p.sel = 0;
        p.scroll = 0;
    }

    /// Enter in the palette.
    pub fn palette_accept(&mut self, host: &dyn Host) {
        let Some(p) = self.palette.take() else { return };
        self.focus = Focus::Editor;
        match &p.mode {
            PaletteMode::GotoLine => {
                let t = p.edit.text.trim();
                let mut parts = t.split([':', ',']);
                let line = parts.next().and_then(|x| x.trim().parse::<usize>().ok());
                let col = parts.next().and_then(|x| x.trim().parse::<usize>().ok());
                match line {
                    Some(l) if l >= 1 => self.goto_line(l - 1, col.map(|c| c.saturating_sub(1))),
                    _ => self.notify(tr("Type a line number")),
                }
            }
            PaletteMode::Prompt(kind) => self.prompt_done(kind.clone(), &p.edit.text, host),
            _ => {
                let Some(&i) = p.shown.get(p.sel) else { return };
                match p.items[i].act.clone() {
                    PAct::Cmd(c) => self.dispatch(c, host),
                    PAct::Line(l) => self.goto_line(l, None),
                    PAct::Text(t) => self.edit(|b| b.insert_text(&t, false)),
                    PAct::Path(path) => self.open_snapshot(&path, host),
                }
            }
        }
    }

    fn prompt_done(&mut self, kind: PromptKind, text: &str, host: &dyn Host) {
        match kind {
            PromptKind::PrefixSuffix => {
                let (pre, suf) = text.split_once('|').unwrap_or((text, ""));
                let (pre, suf) = (pre.to_string(), suf.to_string());
                self.line_tool("Add prefix/suffix", &move |v| tt::prefix_suffix(v, &pre, &suf, false), host);
            }
            PromptKind::SnippetName => {
                let name = text.trim().replace('\n', " ");
                if name.is_empty() {
                    return;
                }
                let body = self.doc().buf.selected_texts().join("\n");
                let mut existing = self.location.read_text(session::SNIPPETS_FILE).unwrap_or_else(|| session::SNIPPETS_TEMPLATE.to_string());
                if !existing.ends_with('\n') {
                    existing.push('\n');
                }
                existing.push_str(&format!("\n## {name}\n{body}\n"));
                match self.location.write_text(session::SNIPPETS_FILE, &existing) {
                    Ok(()) => self.notify(tf("Snippet \"{}\" saved", &[&name])),
                    Err(e) => host.error(&tr("Snippets"), &tf("Could not save the snippet: {}", &[&e.to_string()])),
                }
            }
        }
    }

    fn open_snapshot(&mut self, path: &Path, host: &dyn Host) {
        match file::load(path, None) {
            Ok(l) => {
                let base = self.doc().display_name();
                let stamp = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let mut d = Doc::from_loaded(path, l);
                d.path = None;
                d.title = tf("{} (snapshot {})", &[&base, &stamp]);
                d.view_only = true;
                d.notice = Some(Notice::Info(Msg::new("Read-only snapshot from local history. Copy what you need, or use Save As.")));
                self.insert_doc(d);
            }
            Err(e) => host.error(&tr("Local history"), &e.to_string()),
        }
    }

    fn open_shortcuts_doc(&mut self) {
        let title = tr("rpad keyboard shortcuts");
        let underline = |t: &str| "=".repeat(t.chars().count().max(3));
        let mut text = format!("{title}\n{}\n\n", underline(&title));
        for (k, c) in commands::BINDINGS {
            text.push_str(&format!("{k:<20} {}\n", c.plain_label()));
        }
        let editor = tr("Editor");
        text.push_str(&format!("\n{editor}\n{}\n", "-".repeat(editor.chars().count().max(3))));
        for (k, what) in [
            ("Ctrl+Click", "Add a caret"),
            ("Alt+Drag", "Column (rectangular) selection"),
            ("Double/triple click", "Select word / line"),
            ("Ctrl+Backspace/Del", "Delete word"),
            ("Ctrl+Left/Right", "Move by word"),
            ("Ctrl+Home/End", "Document start/end"),
            ("Esc", "Back to a single caret / close panels"),
            ("Tab / Shift+Tab", "Indent / outdent selected lines"),
        ] {
            text.push_str(&format!("{k:<20} {}\n", tr(what)));
        }
        let mut d = Doc::with_buffer(Buffer::new(&text), None, tr("Keyboard shortcuts"), Enc::Utf8, Eol::platform(), Lang::Plain);
        d.view_only = true;
        self.insert_doc(d);
    }

    // ------------------------------------------------------------ XML

    fn xml_check(&mut self, host: &dyn Host) {
        let text = self.doc().buf.text.to_string();
        match xml::check(&text) {
            Ok(n) => host.info("XML", &tf("The document is well-formed ({} elements).", &[&n.to_string()])),
            Err(e) => {
                self.goto_line(e.line - 1, Some(e.col - 1));
                host.error(&tr("XML is not well-formed"), &e.to_string());
            }
        }
    }

    fn xml_format(&mut self, host: &dyn Host) {
        let text = self.doc().buf.text.to_string();
        let unit = self.indent_cfg().unit();
        match xml::format(&text, &unit) {
            Ok(out) if out == text => self.notify(tr("The XML is already formatted")),
            Ok(out) => {
                let ok = host.confirm(
                    &tr("Format XML"),
                    &tr("Formatting re-indents elements that contain only other elements. Text content, CDATA, comments and elements with xml:space=\"preserve\" are kept exactly as they are.\n\nYou can undo this with Ctrl+Z. Continue?"),
                );
                if ok {
                    self.edit(|b| {
                        let len = b.len();
                        b.apply(vec![Edit::new(0, len, out)], After::Sels(vec![Sel::caret(0)]), Kind::Other);
                    });
                }
            }
            Err(e) => {
                self.goto_line(e.line - 1, Some(e.col - 1));
                host.error(&tr("Format XML"), &tf("The document is not well-formed, so it was not changed.\n\n{}", &[&e.to_string()]));
            }
        }
    }

    // ------------------------------------------------------------ find in files / docs

    fn open_fif(&mut self) {
        let sel = self.doc().buf.selected_texts().into_iter().next().unwrap_or_default();
        if !sel.is_empty() && !sel.contains('\n') {
            self.fif.query.set(&sel);
        } else if self.fif.query.is_empty() {
            self.fif.query.set(&self.find.find.text.clone());
        }
        if self.fif.dir.is_empty() {
            if let Some(dir) = self.doc().path.as_ref().and_then(|p| p.parent()) {
                self.fif.dir.set(&dir.display().to_string());
            }
        }
        self.results = Some(Results { title: tr("Find in files"), items: Vec::new(), sel: 0, scroll: 0, status: String::new(), fif: true });
        self.fif.query.select_all();
        self.focus = Focus::FifQuery;
    }

    pub fn start_fif(&mut self, host: &dyn Host) {
        let q = self.find.query_with(&self.fif.query.text);
        let re = match q.build() {
            Ok(r) => r,
            Err(e) => {
                host.error(&tr("Find in files"), &e);
                return;
            }
        };
        let dir = PathBuf::from(self.fif.dir.text.trim());
        if !dir.is_dir() {
            host.error(&tr("Find in files"), &tf("The folder \"{}\" does not exist.", &[&dir.display().to_string()]));
            return;
        }
        self.stop_fif();
        settings::Settings::push_history(&mut self.settings.find_history, &self.fif.query.text);
        self.settings.fif_dir = self.fif.dir.text.clone();
        self.settings.fif_filter = self.fif.filter.text.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let filter = self.fif.filter.text.clone();
        let c2 = cancel.clone();
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            crate::search::files::search(&dir, &filter, &re, &c2, |hits, p| {
                let _ = tx.send((hits, p.files_searched, p.done));
                if let Some(w) = &waker {
                    w();
                }
            });
        });
        self.fif.cancel = Some(cancel);
        self.fif.rx = Some(rx);
        if let Some(r) = self.results.as_mut() {
            r.items.clear();
            r.sel = 0;
            r.scroll = 0;
            r.status = tr("Searching…");
            r.title = tf("Find in files: {}", &[&self.fif.query.text]);
        }
    }

    pub fn stop_fif(&mut self) {
        if let Some(c) = self.fif.cancel.take() {
            c.store(true, Ordering::Relaxed);
        }
        self.fif.rx = None;
    }

    pub fn fif_running(&self) -> bool {
        self.fif.rx.is_some()
    }

    /// Collects background results (called when woken).
    pub fn pump(&mut self) {
        let Some(rx) = &self.fif.rx else { return };
        let mut done = false;
        let mut files = 0;
        let mut new_items = Vec::new();
        while let Ok((hits, f, d)) = rx.try_recv() {
            files = f;
            done |= d;
            new_items.extend(hits.into_iter().map(|h| ResultItem { doc: None, path: Some(h.path), line: h.line, text: h.text, col: h.col }));
        }
        if let Some(r) = self.results.as_mut() {
            r.items.extend(new_items);
            let n = r.items.len();
            let (ns, fs) = (n.to_string(), files.to_string());
            r.status = if !done {
                tf("Searching… {} match(es) so far, {} file(s)", &[&ns, &fs])
            } else if n >= crate::search::files::MAX_HITS {
                tf("{} match(es) in {} file(s) searched (limit reached)", &[&ns, &fs])
            } else {
                tf("{} match(es) in {} file(s) searched", &[&ns, &fs])
            };
        }
        if done {
            self.fif.rx = None;
            self.fif.cancel = None;
        }
    }

    fn find_in_docs(&mut self, host: &dyn Host) {
        if self.find.find.is_empty() {
            let w = self.doc().buf.selected_texts().into_iter().next().unwrap_or_default();
            self.find.find.set(&w);
        }
        let q = self.find.query();
        if q.is_empty() {
            self.open_find(false);
            self.notify(tr("Type what to find, then use Search › Find in Open Documents"));
            return;
        }
        let re = match q.build() {
            Ok(r) => r,
            Err(e) => {
                host.error(&tr("Find"), &e);
                return;
            }
        };
        let mut items = Vec::new();
        for d in &self.docs {
            let text = d.buf.text.to_string();
            for (i, line) in text.lines().enumerate() {
                if let Some(m) = re.find(line) {
                    let pre = line[..m.start()].chars().count();
                    let len = line[m.start()..m.end()].chars().count();
                    items.push(ResultItem { doc: Some(d.id), path: d.path.clone(), line: i, text: line.chars().take(300).collect(), col: (pre, pre + len) });
                }
            }
        }
        let n = items.len();
        self.results = Some(Results {
            title: tf("Find in open documents: {}", &[&q.text]),
            status: tf("{} matching line(s) in {} document(s)", &[&n.to_string(), &self.docs.len().to_string()]),
            items,
            sel: 0,
            scroll: 0,
            fif: false,
        });
        self.focus = Focus::Results;
    }

    /// Opens a search result.
    pub fn open_result(&mut self, i: usize, host: &dyn Host) {
        let Some(item) = self.results.as_ref().and_then(|r| r.items.get(i)).cloned() else { return };
        if let Some(r) = self.results.as_mut() {
            r.sel = i;
        }
        let ok = match (item.doc.and_then(|id| self.docs.iter().position(|d| d.id == id)), &item.path) {
            (Some(di), _) => {
                self.activate(di);
                true
            }
            (None, Some(p)) => self.open_path(p, host),
            _ => false,
        };
        if ok {
            self.goto_line(item.line, None);
            let d = self.doc_mut();
            let start = d.buf.line_start(item.line.min(d.buf.line_count() - 1));
            let len = d.buf.line_end(d.buf.line_of(start)) - start;
            let (a, b) = (item.col.0.min(len), item.col.1.min(len));
            if item.text.starts_with('…') {
                d.buf.set_caret(start);
            } else {
                d.buf.set_sels(vec![Sel::new(start + a, start + b)], 0);
            }
            self.reveal_caret = true;
        }
    }

    // ------------------------------------------------------------ session / recovery

    pub fn session_tabs(&self, include_saved: bool) -> Vec<session::SessionTab> {
        self.docs
            .iter()
            .enumerate()
            .filter(|(_, d)| !d.view_only || d.path.is_some())
            .filter(|(_, d)| !(d.path.is_none() && d.buf.is_empty()))
            .filter_map(|(i, d)| {
                let recovered = self.settings.recovery && d.is_modified();
                if !recovered && !(include_saved && d.path.is_some()) {
                    return None;
                }
                Some(session::SessionTab {
                    path: d.path.clone(),
                    recovery: recovered.then(|| session::recovery_name(d.id)),
                    title: d.title.clone(),
                    enc: d.enc,
                    eol: d.eol,
                    lang: d.lang_override.then_some(d.lang),
                    caret: d.buf.primary_sel().head,
                    top: d.top,
                    active: i == self.active,
                })
            })
            .collect()
    }

    /// Writes recovery files for changed documents and the session list.
    /// `blocking` waits for the writes (used on exit).
    pub fn write_recovery(&mut self, blocking: bool) {
        self.recovery_due = None;
        let Some(rdir) = self.location.file(session::RECOVERY_DIR) else { return };
        let mut handles = Vec::new();
        if self.settings.recovery {
            for d in self.docs.iter_mut() {
                if d.is_modified() && d.recovery_version != Some(d.buf.version) {
                    let snapshot = d.buf.text.clone();
                    let name = session::recovery_name(d.id);
                    let dir = rdir.clone();
                    d.recovery_version = Some(d.buf.version);
                    handles.push(std::thread::spawn(move || session::write_recovery(&dir, &name, snapshot)));
                }
            }
        }
        if blocking {
            for h in handles {
                let _ = h.join();
            }
        }
        let tabs = self.session_tabs(self.settings.restore_session);
        let keep: Vec<String> = tabs.iter().filter_map(|t| t.recovery.clone()).collect();
        let _ = self.location.write_text(session::SESSION_FILE, &session::serialize(&tabs));
        if blocking {
            session::prune_recovery(&self.location, &keep);
        }
    }

    /// Restores tabs from the last session. Returns true if any were restored.
    pub fn restore_session(&mut self, host: &dyn Host) -> bool {
        if !self.settings.restore_session && !self.settings.recovery {
            return false;
        }
        let Some(text) = self.location.read_text(session::SESSION_FILE) else { return false };
        let tabs = session::parse(&text);
        let mut restored = false;
        let mut active_id = None;
        for t in tabs {
            let recovered = t.recovery.as_ref().filter(|_| self.settings.recovery).and_then(|r| session::read_recovery(&self.location, r));
            if recovered.is_none() && !self.settings.restore_session {
                continue;
            }
            let doc = match (recovered, &t.path) {
                (Some(text), path) => {
                    let mut d = Doc::with_buffer(Buffer::new(&text), path.clone(), t.title.clone(), t.enc, t.eol, Lang::Plain);
                    let first = d.buf.line_text(0);
                    d.set_lang(t.lang.unwrap_or_else(|| syntax::detect(path.as_deref(), &first)), t.lang.is_some());
                    d.buf.mark_unsaved();
                    if let Some(p) = path {
                        d.stamp = file::stamp(p);
                        d.notice = Some(Notice::Info(Msg::new("Unsaved changes were recovered from the last session.")));
                    }
                    if d.title.is_empty() && path.is_none() {
                        self.untitled += 1;
                        d.title = format!("Untitled {}", self.untitled);
                    }
                    if let Some(n) = d.title.strip_prefix("Untitled ").and_then(|n| n.parse::<usize>().ok()) {
                        self.untitled = self.untitled.max(n);
                    }
                    d
                }
                (None, Some(p)) => match file::load(p, None) {
                    Ok(l) => {
                        let mut d = Doc::from_loaded(p, l);
                        if let Some(lang) = t.lang {
                            d.set_lang(lang, true);
                        }
                        d
                    }
                    Err(_) => continue,
                },
                (None, None) => continue,
            };
            let mut doc = doc;
            let c = t.caret.min(doc.buf.len());
            doc.buf.set_caret(c);
            doc.top = t.top.min(doc.buf.line_count().saturating_sub(1));
            if t.active {
                active_id = Some(doc.id);
            }
            if self.docs.len() == 1 && self.docs[0].is_pristine() {
                self.docs.clear();
            }
            self.docs.push(doc);
            restored = true;
        }
        if self.docs.is_empty() {
            self.new_doc();
        }
        self.active = active_id.and_then(|id| self.docs.iter().position(|d| d.id == id)).unwrap_or(self.docs.len() - 1);
        let _ = host;
        self.after_switch();
        restored
    }

    /// Autosave on focus loss (if enabled) for documents with a path.
    pub fn autosave(&mut self, host: &dyn Host) {
        if !self.settings.autosave_on_blur {
            return;
        }
        for i in 0..self.docs.len() {
            let d = &self.docs[i];
            if d.is_modified() && d.path.is_some() && !d.read_only && !d.view_only && d.changed_on_disk() != Some(true) {
                self.save_doc(i, false, host);
            }
        }
    }

    /// Persists settings. Errors are recorded in `location.error`.
    pub fn save_settings(&mut self) {
        let store = self.settings.to_store();
        self.location.save(&store);
    }
}

/// Caches the token spans of one line while scanning for brackets.
#[derive(Default)]
struct TokenProbe {
    line: Option<usize>,
    start: usize,
    spans: Vec<syntax::Span>,
    byte_of: Vec<u32>,
}

impl TokenProbe {
    fn is_code(&mut self, d: &mut Doc, p: usize) -> bool {
        if d.lang == Lang::Plain || !d.hl.enabled {
            return true;
        }
        let line = d.buf.line_of(p);
        if self.line != Some(line) {
            self.line = Some(line);
            self.start = d.buf.line_start(line);
            d.hl.line_spans(&d.buf.text, line, &mut self.spans);
            let text = d.buf.line_text(line);
            self.byte_of = text.char_indices().map(|(b, _)| b as u32).collect();
        }
        let col = p - self.start;
        let byte = self.byte_of.get(col).copied().unwrap_or(u32::MAX) as usize;
        !self.spans.iter().any(|s| s.start <= byte && byte < s.end && matches!(s.tok, syntax::Tok::String | syntax::Tok::Comment))
    }
}

/// Subsequence match score (higher is better); None if no match.
pub fn fuzzy_score(q: &str, s: &str) -> Option<i32> {
    if q.is_empty() {
        return Some(0);
    }
    if let Some(p) = s.find(q) {
        return Some(1000 - p as i32);
    }
    let mut it = s.chars();
    let mut score = 0;
    let mut last = 0;
    for (i, qc) in q.chars().enumerate() {
        let mut pos = 0;
        loop {
            let c = it.next()?;
            pos += 1;
            if c == qc {
                break;
            }
        }
        score += if pos == 1 && i > 0 { 5 } else { 1 };
        last += pos;
    }
    Some(score * 10 - last)
}

/// File-name-safe timestamp: `2026-10-19_14-03-22`.
pub fn timestamp() -> String {
    let t = rt_sys::local_time();
    format!("{:04}-{:02}-{:02}_{:02}-{:02}-{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
}

/// Human timestamp for Insert Date/Time: `2026-10-19 14:03`.
pub fn timestamp_display() -> String {
    let t = rt_sys::local_time();
    format!("{:04}-{:02}-{:02} {:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute)
}

#[cfg(test)]
mod tests;
