//! The duplicates page: the drives and folders chosen, the filters, the
//! search on a thread of its own, the groups found, the marks and what is
//! done with the marked copies, and the page's drawing, keyboard and mouse.
//! The window of rdup (`main.rs`) and the Duplicate Files of rcare both hold
//! one.
//!
//! The page does not own the window. What it needs from it (draw again,
//! show a message, save the settings) it notes in `fx`; the dialogs are the
//! ones the window passes in ([`Dialogs`]); what the threads report comes
//! back as an [`Event`] through the [`Sender`]; the menus that pop up at the
//! pointer are asked for with [`Popup`] and shown by the window.

use crate::actions::{Op, Rule};
use crate::app::{App, Row, Searching, View};
use crate::commands::{self, Command};
use crate::format;
use crate::i18n::{self, tf, tr};
use crate::search::{self, Event as SearchEvent, Failure, Options};
use crate::settings::Settings;
use crate::ui::gfx::{Gfx, Rect};
use crate::ui::theme::Theme;
use crate::ui::view::{self, Hot, Layout, Page};
use crate::volumes::{self, Volume};
use rt_ui::page::{Dialogs, Effects, Mods};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::event::MouseScrollDelta;
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::CursorIcon;

/// What the threads report.
pub enum Event {
    /// Moving the marked copies: how many done, of how many.
    Acting(usize, usize),
    /// What was moved, and what was not (with the reason).
    Acted(Vec<PathBuf>, Vec<(PathBuf, String)>),
    Search(u64, SearchEvent),
    Volumes(Vec<Volume>),
}

/// Where the threads send their events (to the window's event loop).
pub type Sender = Arc<dyn Fn(Event) + Send + Sync>;

/// A menu that pops up at the pointer; the window shows it and runs the command chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popup {
    /// The rules to mark copies, under their button.
    Rules,
    /// The context menu of the selected file.
    Item,
}

/// What a click asks of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Out {
    Command(Command),
    Popup(Popup),
}

pub struct DupPage {
    pub app: App,
    pub layout: Layout,
    pub hot: Hot,
    /// Dragging the scroll bar's thumb.
    pub dragging: bool,
    /// Selecting text in a filter box with the pointer.
    pub selecting: bool,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    /// Last folder chosen in the folder dialog.
    pub last_dir: String,
    /// Running with administrator rights (NTFS can be read directly).
    pub elevated: bool,
    handle: Option<search::Handle>,
    generation: u64,
    /// Marked copies being moved: (done, of how many).
    acting: Option<(usize, usize)>,
    /// The last click in a filter box, to see a double click.
    last_click: Option<(Instant, f32, f32)>,
    /// Where the hashes are kept between searches.
    cache_file: Option<PathBuf>,
    scale: f32,
    mods: Mods,
    send: Sender,
}

impl DupPage {
    /// The page with what the settings kept; `folders` (from the command
    /// line or the Explorer menu) are searched instead of the ones kept.
    pub fn new(settings: &Settings, folders: &[PathBuf], cache_file: Option<PathBuf>, elevated: bool, send: Sender) -> DupPage {
        let mut app = App::new();
        app.choice = settings.choice.clone();
        app.share = settings.share;
        app.compare = settings.compare;
        app.chosen = if folders.is_empty() { settings.roots.clone() } else { folders.iter().map(|a| rt_scan::normalize(a)).collect() };
        app.saved = settings.folders.clone();
        DupPage {
            app,
            layout: Layout::default(),
            hot: Hot::None,
            dragging: false,
            selecting: false,
            cursor_icon: CursorIcon::Default,
            fx: Effects::default(),
            last_dir: settings.last_dir.clone(),
            elevated,
            handle: None,
            generation: 0,
            acting: None,
            last_click: None,
            cache_file,
            scale: 1.0,
            mods: Mods::default(),
            send,
        }
    }

    /// Puts what the page keeps in the settings.
    pub fn store_into(&self, s: &mut Settings) {
        s.roots = self.app.chosen.clone();
        if self.app.volumes.is_some() {
            s.folders = self.app.folders();
        }
        s.choice = self.app.choice.clone();
        s.share = self.app.share;
        s.compare = self.app.compare;
        s.last_dir = self.last_dir.clone();
    }

    /// Whether a search or a move is going on.
    pub fn busy(&self) -> bool {
        self.app.searching.is_some() || self.acting.is_some()
    }

    /// Whether a search is going on.
    pub fn searching(&self) -> bool {
        self.app.searching.is_some()
    }

    /// The title of the window while it searches.
    pub fn title(&self) -> Option<String> {
        (view::page(&self.app) == Page::Searching).then(|| tr("Searching"))
    }

    // ------------------------------------------------------------ searches

    /// Reads the drives, to show them.
    pub fn load_volumes(&self) {
        let send = self.send.clone();
        std::thread::spawn(move || send(Event::Volumes(volumes::list())));
    }

    pub fn start_search(&mut self) {
        if self.app.chosen.is_empty() {
            return;
        }
        let Ok(filter) = self.app.choice.filter() else { return };
        self.app.focus = None;
        self.app.calendar = None;
        if let Some(h) = self.handle.take() {
            h.cancel();
        }
        self.generation += 1;
        let gen = self.generation;
        let roots = search::distinct_roots(&self.app.chosen);
        self.app.searching =
            Some(Searching { roots: roots.clone(), progress: Default::default(), started: Instant::now(), generation: gen });
        self.app.error = None;
        let send = self.send.clone();
        let lang = i18n::lang();
        self.handle = Some(search::start(
            roots,
            Options { filter, files: self.app.compare[0], folders: self.app.compare[1], cache: self.cache_file.clone(), hard_links: false },
            Box::new(move |e| {
                i18n::set_lang(lang);
                send(Event::Search(gen, e));
            }),
        ));
        self.dragging = false;
        self.fx.menus();
        self.fx.save();
        self.fx.redraw();
    }

    /// Stops the search under way (the window is closing, or the user asked).
    pub fn cancel(&self) {
        if let Some(h) = &self.handle {
            h.cancel();
        }
    }

    /// Folders from a later launch (or the Explorer menu): they are searched.
    pub fn open_paths(&mut self, paths: Vec<PathBuf>) {
        if !paths.is_empty() && self.acting.is_none() {
            self.app.chosen = paths.iter().map(|p| rt_scan::normalize(p)).collect();
            self.start_search();
        }
    }

    /// A file or folder dropped on the window: its folder is added to the search.
    pub fn drop_file(&mut self, p: PathBuf) {
        if self.app.searching.is_none() {
            let dir = if p.is_dir() { p } else { p.parent().map(Path::to_path_buf).unwrap_or(p) };
            self.add_root(&dir);
            self.fx.redraw();
        }
    }

    /// The arguments of a new instance with administrator rights: the same
    /// drives and folders, without their last backslash, which would escape the closing quote.
    pub fn restart_args(&self) -> String {
        let args: Vec<String> = self.app.chosen.iter().map(|r| format!("\"{}\"", r.display().to_string().trim_end_matches('\\'))).collect();
        args.join(" ")
    }

    /// What a work thread reported.
    pub fn event(&mut self, event: Event) {
        match event {
            Event::Search(gen, ev) => self.on_search(gen, ev),
            Event::Acting(done, total) => {
                self.acting = Some((done, total));
                self.app.note = Some(tf("Moving {} of {}…", &[&format::count(done as u64 + 1), &format::count(total as u64)]));
                self.fx.redraw();
            }
            Event::Acted(done, failed) => self.acted(done, failed),
            Event::Volumes(v) => {
                self.app.volumes = Some(v);
                self.fx.redraw();
            }
        }
    }

    fn on_search(&mut self, gen: u64, ev: SearchEvent) {
        if gen != self.generation {
            return;
        }
        match ev {
            SearchEvent::Progress(p) => {
                if let Some(s) = &mut self.app.searching {
                    s.progress = p;
                }
            }
            SearchEvent::Done(result) => {
                self.app.searching = None;
                self.handle = None;
                match result {
                    Ok(found) => self.app.set_found(*found),
                    Err(Failure::Stopped) => {
                        self.app.show_start = true;
                        self.app.error = Some(tr("The search was stopped."));
                    }
                    Err(Failure::Unreadable(why)) => {
                        self.app.show_start = true;
                        self.app.error = Some(tf("Could not read: {}", &[&why]));
                    }
                }
                self.fx.menus();
            }
        }
        self.fx.redraw();
    }

    /// The elapsed time on the search page moves even between reports: the
    /// time the window should wake up in, while it searches.
    pub fn animate(&mut self) -> Option<Duration> {
        if self.app.searching.is_some() {
            self.fx.redraw();
            Some(Duration::from_millis(500))
        } else {
            None
        }
    }

    fn add_folder(&mut self, dialogs: &mut dyn Dialogs) {
        if let Some(p) = dialogs.pick_folder(&tr("Choose a folder to search")) {
            self.last_dir = p.display().to_string();
            self.add_root(&p);
        }
    }

    /// Adds a folder to the search and shows the start page with it.
    fn add_root(&mut self, p: &Path) {
        self.app.add_folder(p);
        self.app.show_start = true;
        self.app.error = None;
    }

    // ------------------------------------------------------------ commands

    pub fn checked(&self, c: Command) -> bool {
        match c {
            Command::ShowFiles => self.app.view == View::Files,
            Command::ShowFolders => self.app.view == View::Folders,
            _ => false,
        }
    }

    /// Whether the command can be used now (menus and buttons).
    pub fn enabled(&self, c: Command) -> bool {
        let result = view::page(&self.app) == Page::Result;
        let idle = self.app.searching.is_none();
        match c {
            Command::Search => idle && !self.app.chosen.is_empty() && self.app.choice.filter().is_ok() && self.app.compare.contains(&true),
            Command::AddFolder => idle,
            Command::NewSearch => idle && self.app.has_result() && !self.app.show_start,
            Command::Stop => !idle,
            #[cfg(windows)]
            Command::RestartAdmin => !self.elevated,
            Command::CopyPath | Command::OpenItem | Command::Reveal => result && self.app.selected_path().is_some(),
            Command::ToggleMark => result && self.app.sel.is_some_and(|i| self.app.markable(i)) && self.acting.is_none(),
            Command::KeepNewest | Command::KeepOldest | Command::KeepShortest | Command::KeepInFolder => {
                result && self.acting.is_none() && (0..self.app.rows.len()).any(|i| self.app.markable(i))
            }
            Command::UnmarkAll | Command::Recycle | Command::MoveTo => result && self.acting.is_none() && !self.app.marked().is_empty(),
            Command::Export => result && !self.app.rows.is_empty(),
            Command::ShowFiles => result && self.app.found.as_ref().is_some_and(|f| !f.groups.is_empty() && f.look == [true, true]),
            Command::ShowFolders => result && self.app.found.as_ref().is_some_and(|f| !f.groups.is_empty() && f.look[1]),
            _ => true,
        }
    }

    /// Does the commands that are the page's own; false for the others
    /// (Exit, theme, language, help… belong to the window).
    pub fn command(&mut self, cmd: Command, dialogs: &mut dyn Dialogs) -> bool {
        if !matches!(
            cmd,
            Command::AddFolder
                | Command::Search
                | Command::NewSearch
                | Command::Stop
                | Command::CopyPath
                | Command::OpenItem
                | Command::Reveal
                | Command::ToggleMark
                | Command::KeepNewest
                | Command::KeepOldest
                | Command::KeepShortest
                | Command::KeepInFolder
                | Command::UnmarkAll
                | Command::Recycle
                | Command::MoveTo
                | Command::Export
                | Command::ShowFiles
                | Command::ShowFolders
        ) {
            return false;
        }
        if !self.enabled(cmd) {
            return true;
        }
        match cmd {
            Command::AddFolder => self.add_folder(dialogs),
            Command::Search => self.start_search(),
            Command::NewSearch => {
                self.app.show_start = true;
                self.app.volumes = None;
                self.load_volumes();
            }
            Command::Stop => self.cancel(),
            Command::CopyPath => {
                if let Some(p) = self.app.selected_path() {
                    rt_sys::set_clipboard_text(&p.to_string_lossy());
                }
            }
            Command::OpenItem => {
                if let Some(p) = self.app.selected_path() {
                    open_item(&p);
                }
            }
            Command::Reveal => {
                if let Some(p) = self.app.selected_path() {
                    rt_sys::reveal(&p);
                }
            }
            Command::ToggleMark => {
                if let Some(i) = self.app.sel {
                    self.toggle_mark(i);
                }
            }
            Command::KeepNewest => self.apply_rule(Rule::Newest),
            Command::KeepOldest => self.apply_rule(Rule::Oldest),
            Command::KeepShortest => self.apply_rule(Rule::Shortest),
            Command::KeepInFolder => {
                if let Some(dir) = dialogs.pick_folder(&tr("Choose the folder whose copies to keep")) {
                    self.apply_rule(Rule::InFolder(dir));
                }
            }
            Command::UnmarkAll => self.app.unmark_all(),
            Command::Recycle => self.act(Op::Recycle, dialogs),
            Command::MoveTo => {
                if let Some(dir) = dialogs.pick_folder(&tr("Choose where to move the marked copies")) {
                    self.act(Op::MoveTo(dir), dialogs);
                }
            }
            Command::Export => self.export(dialogs),
            Command::ShowFiles => self.app.set_view(View::Files),
            Command::ShowFolders => self.app.set_view(View::Folders),
            _ => {}
        }
        self.fx.menus();
        self.fx.redraw();
        true
    }

    // ------------------------------------------------------------ marks and actions

    fn toggle_mark(&mut self, row: usize) {
        match self.app.toggle_mark(row) {
            Ok(()) => self.app.note = None,
            Err(why) => self.app.note = Some(why),
        }
        self.fx.menus();
        self.fx.redraw();
    }

    fn apply_rule(&mut self, rule: Rule) {
        let n = self.app.apply_rule(&rule);
        self.app.note =
            Some(if n == 0 { tr("No copy was marked: the rule keeps them all.") } else { tf("{} marked", &[&format::count(n as u64)]) });
    }

    /// Moves the marked copies of the view to the Recycle Bin or a folder,
    /// after asking, in the background.
    fn act(&mut self, op: Op, dialogs: &mut dyn Dialogs) {
        let marked = self.app.marked();
        if marked.is_empty() || self.acting.is_some() {
            return;
        }
        let n = format::count(marked.len() as u64);
        let what = if self.app.view == View::Folders { tf("{} folders", &[&n]) } else { tf("{} files", &[&n]) };
        let size = format::size(marked.iter().map(|(_, b)| b).sum());
        let (title, question) = match &op {
            Op::Recycle => (
                view::recycle_label(),
                if cfg!(windows) {
                    tf("Move {} to the Recycle Bin?\n\n{} in all. One copy of each group stays where it is.", &[&what, &size])
                } else {
                    tf("Move {} to the trash?\n\n{} in all. One copy of each group stays where it is.", &[&what, &size])
                },
            ),
            Op::MoveTo(dir) => (
                rt_core::i18n::strip_mnemonic(&tr("Mo&ve To...")),
                tf("Move {} to {}?\n\n{} in all. One copy of each group stays where it is.", &[&what, &dir.display().to_string(), &size]),
            ),
        };
        if !dialogs.confirm(&title, &question) {
            return;
        }
        let paths: Vec<PathBuf> = marked.into_iter().map(|(p, _)| p).collect();
        self.acting = Some((0, paths.len()));
        self.dragging = false;
        let send = self.send.clone();
        let lang = i18n::lang();
        std::thread::spawn(move || {
            i18n::set_lang(lang);
            let total = paths.len();
            let (done, failed) = crate::actions::run(&op, &paths, &|i, _| send(Event::Acting(i, total)));
            send(Event::Acted(done, failed));
        });
        self.fx.menus();
        self.fx.redraw();
    }

    fn acted(&mut self, done: Vec<PathBuf>, failed: Vec<(PathBuf, String)>) {
        self.acting = None;
        self.app.removed(&done);
        let moved = format::count(done.len() as u64);
        let mut note = tf("{} moved.", &[&moved]);
        if let Some((p, why)) = failed.first() {
            note = format!(
                "{note} {}",
                tf("{} could not be moved, such as {}: {}", &[&format::count(failed.len() as u64), &p.display().to_string(), why])
            );
        }
        self.app.note = Some(note);
        self.fx.menus();
        self.fx.redraw();
    }

    /// Writes the list shown to a CSV file the user chooses.
    fn export(&mut self, dialogs: &mut dyn Dialogs) {
        let name = if self.app.view == View::Folders { "rdup-folders.csv" } else { "rdup-files.csv" };
        let Some(path) = dialogs.save_file(name, (&tr("CSV files"), "*.csv"), None) else { return };
        self.app.note = Some(match std::fs::write(&path, self.app.csv()) {
            Ok(()) => tf("The list was saved to {}.", &[&path.display().to_string()]),
            Err(e) => tf("Could not save {}: {}", &[&path.display().to_string(), &e.to_string()]),
        });
        self.fx.redraw();
    }

    // ------------------------------------------------------------ drawing and input

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32, menu_bar: bool) {
        self.scale = s;
        self.layout = view::layout(&self.app, area, s, menu_bar);
        self.app.page = self.layout.page_rows();
        view::draw(g, &self.app, &self.layout, th, s, self.hot);
    }

    /// A key: the command of a chord (for the window to run), typing in a
    /// filter box, the arrows in the list.
    pub fn key(&mut self, event: &winit::event::KeyEvent, mods: Mods) -> Option<Command> {
        self.mods = mods;
        let start = view::page(&self.app) == Page::Start;
        if let (Some(cal), true) = (&mut self.app.calendar, start) {
            match event.logical_key {
                WKey::Named(NamedKey::Escape) => self.app.calendar = None,
                WKey::Named(NamedKey::PageUp) => cal.shift(-1),
                WKey::Named(NamedKey::PageDown) => cal.shift(1),
                _ => return None,
            }
            self.fx.redraw();
            return None;
        }
        if let Some(cmd) = self.field_key(event) {
            return cmd;
        }
        let Mods { ctrl, shift, alt } = mods;
        let logical = if ctrl || alt {
            use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
            event.key_without_modifiers()
        } else {
            event.logical_key.clone()
        };
        let name = match &logical {
            WKey::Character(c) => c.to_lowercase(),
            WKey::Named(n) => match n {
                NamedKey::F1 => "f1".into(),
                NamedKey::F5 => "f5".into(),
                NamedKey::Enter => "enter".into(),
                NamedKey::Escape => "esc".into(),
                NamedKey::Space => "space".into(),
                NamedKey::Delete => "delete".into(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        if !name.is_empty() {
            if let Some(cmd) = commands::lookup(ctrl, shift, alt, &name) {
                return Some(cmd);
            }
        }
        if view::page(&self.app) != Page::Result {
            return None;
        }
        let page = self.layout.page_rows() as isize;
        match logical {
            WKey::Named(NamedKey::ArrowDown) => self.app.move_sel(1),
            WKey::Named(NamedKey::ArrowUp) => self.app.move_sel(-1),
            WKey::Named(NamedKey::PageDown) => self.app.move_sel(page),
            WKey::Named(NamedKey::PageUp) => self.app.move_sel(-page),
            WKey::Named(NamedKey::Home) => self.app.move_sel(isize::MIN / 2),
            WKey::Named(NamedKey::End) => self.app.move_sel(isize::MAX / 2),
            _ => return None,
        }
        self.fx.menus();
        self.fx.redraw();
        None
    }

    /// Typing in a filter box, and Tab to reach them. None when it did not
    /// use the key; otherwise Some, with the command it asks the window for.
    fn field_key(&mut self, event: &winit::event::KeyEvent) -> Option<Option<Command>> {
        use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
        if view::page(&self.app) != Page::Start {
            return None;
        }
        let Mods { ctrl, shift, alt } = self.mods;
        let tab = matches!(event.logical_key, WKey::Named(NamedKey::Tab)) && !ctrl && !alt;
        if self.app.focus.is_none() {
            if tab {
                self.app.focus_next(shift);
                self.fx.redraw();
                return Some(None);
            }
            return None;
        }
        let key = if ctrl { event.key_without_modifiers() } else { event.logical_key.clone() };
        let mut ask = None;
        match &key {
            WKey::Named(NamedKey::Backspace) => self.app.delete(false, ctrl),
            WKey::Named(NamedKey::Delete) => self.app.delete(true, ctrl),
            WKey::Named(NamedKey::ArrowLeft) => self.app.move_caret(-1, false, ctrl, shift),
            WKey::Named(NamedKey::ArrowRight) => self.app.move_caret(1, false, ctrl, shift),
            WKey::Named(NamedKey::Home) => self.app.move_caret(-1, true, false, shift),
            WKey::Named(NamedKey::End) => self.app.move_caret(1, true, false, shift),
            WKey::Named(NamedKey::Tab) if tab => self.app.focus_next(shift),
            WKey::Named(NamedKey::Escape) => self.app.focus = None,
            WKey::Named(NamedKey::Enter) if !ctrl && !alt => {
                self.app.focus = None;
                ask = Some(Command::Search);
            }
            // Ctrl alone: editing shortcuts (Ctrl with Alt is AltGr, which types).
            WKey::Character(c) if ctrl && !alt => {
                let text = self.app.selected_text();
                match c.to_lowercase().as_str() {
                    "a" => self.app.select_all(),
                    "c" if !text.is_empty() => {
                        rt_sys::set_clipboard_text(&text);
                    }
                    "x" if !text.is_empty() => {
                        rt_sys::set_clipboard_text(&text);
                        self.app.delete(false, false);
                    }
                    "c" | "x" => {}
                    "v" => {
                        if let Some(t) = rt_sys::clipboard_text() {
                            self.app.type_text(&t.replace(['\r', '\n', '\t'], " "));
                        }
                    }
                    _ => return None,
                }
            }
            _ => match &event.text {
                Some(t) if !(alt && !ctrl) && !t.chars().all(char::is_control) => self.app.type_text(t),
                _ => return None,
            },
        }
        self.fx.redraw();
        Some(ask)
    }

    /// Updates what the pointer is on; `blocked` while a message is open.
    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        let l = &self.layout;
        let hot = match view::page(&self.app) {
            _ if blocked => Hot::None,
            Page::Start if self.app.calendar.is_some() => l.cal.as_ref().and_then(|c| c.hot_at(x, y)).unwrap_or(Hot::None),
            Page::Start if l.body.contains(x, y) && !l.actions.contains(x, y) => {
                if let Some(i) = l.cards.iter().position(|r| r.contains(x, y)) {
                    Hot::Card(i)
                } else if let Some(i) = l.removes.iter().position(|r| r.contains(x, y)) {
                    Hot::Remove(i)
                } else if let Some(i) = l.folders.iter().position(|r| r.contains(x, y)) {
                    Hot::Folder(i)
                } else if let Some(i) = l.sizes.iter().position(|r| r.contains(x, y)) {
                    Hot::MinSize(i)
                } else if let Some(i) = l.fields.iter().position(|r| r.contains(x, y)) {
                    let f = crate::filter::Field::ALL[i];
                    if matches!(f, crate::filter::Field::From | crate::filter::Field::To)
                        && view::pick_button(l.fields[i], self.scale).contains(x, y)
                    {
                        Hot::Pick(f)
                    } else {
                        Hot::Field(f)
                    }
                } else if let Some(i) = l.kinds.iter().position(|r| r.contains(x, y)) {
                    Hot::Kind(i)
                } else if let Some(i) = l.compare.iter().position(|r| r.contains(x, y)) {
                    Hot::Compare(i)
                } else if l.clear.contains(x, y) && self.app.choice.narrows() {
                    Hot::Clear
                } else {
                    Hot::None
                }
            }
            Page::Start => {
                if l.add.contains(x, y) {
                    Hot::Add
                } else if l.find.contains(x, y) && self.enabled(Command::Search) {
                    Hot::Find
                } else {
                    Hot::None
                }
            }
            Page::Searching => {
                if l.stop.contains(x, y) {
                    Hot::Stop
                } else {
                    Hot::None
                }
            }
            Page::Result => {
                if let Some(i) = l.tabs.iter().position(|r| r.contains(x, y)) {
                    Hot::Tab(i)
                } else if let Some(i) = l.shares.iter().position(|r| r.contains(x, y)) {
                    Hot::Share(i)
                } else if l.rules.contains(x, y) && self.enabled(Command::KeepNewest) {
                    Hot::Rules
                } else if l.unmark.contains(x, y) && self.enabled(Command::UnmarkAll) {
                    Hot::Unmark
                } else if l.move_to.contains(x, y) && self.enabled(Command::MoveTo) {
                    Hot::MoveTo
                } else if l.recycle.contains(x, y) && self.enabled(Command::Recycle) {
                    Hot::Recycle
                } else if l.actions.contains(x, y) {
                    Hot::None
                } else {
                    l.row_at(&self.app, x, y).map_or(Hot::None, Hot::Row)
                }
            }
        };
        if hot != self.hot {
            self.hot = hot;
            self.fx.redraw();
        }
        self.cursor_icon = match hot {
            Hot::Card(_)
            | Hot::Remove(_)
            | Hot::Folder(_)
            | Hot::MinSize(_)
            | Hot::Kind(_)
            | Hot::Compare(_)
            | Hot::Clear
            | Hot::Pick(_)
            | Hot::CalPrev
            | Hot::CalNext
            | Hot::CalDay(_)
            | Hot::CalToday
            | Hot::CalClear
            | Hot::Add
            | Hot::Find
            | Hot::Tab(_)
            | Hot::Share(_)
            | Hot::Rules
            | Hot::Unmark
            | Hot::MoveTo
            | Hot::Recycle
            | Hot::Stop => CursorIcon::Pointer,
            Hot::Field(_) => CursorIcon::Text,
            _ => CursorIcon::Default,
        };
    }

    /// The pointer left the page.
    pub fn pointer_left(&mut self) {
        if !self.dragging && self.hot != Hot::None {
            self.hot = Hot::None;
            self.fx.redraw();
        }
    }

    /// The pointer moved while a button is down: the scroll bar's thumb or
    /// the text selected in a filter box follows it.
    pub fn drag(&mut self, g: &mut Gfx, x: f32, y: f32) {
        if self.dragging {
            self.drag_to(y);
        }
        if self.selecting {
            if let Some(f) = self.app.focus {
                let caret = view::caret_at(g, &self.app, &self.layout, f, x, self.scale);
                self.app.drag_caret(caret);
                self.fx.redraw();
            }
        }
    }

    /// A click at (x, y): what it asks of the window, if anything.
    pub fn press(&mut self, g: &mut Gfx, x: f32, y: f32, mods: Mods) -> Option<Out> {
        self.mods = mods;
        if !matches!(self.hot, Hot::Field(_)) {
            self.app.focus = None;
        }
        // A click outside an open calendar only closes it.
        if self.app.calendar.is_some()
            && !matches!(self.hot, Hot::CalPrev | Hot::CalNext | Hot::CalDay(_) | Hot::CalToday | Hot::CalClear | Hot::CalPanel)
        {
            self.app.calendar = None;
            self.fx.redraw();
            return None;
        }
        let mut out = None;
        match self.hot {
            Hot::Field(f) => {
                let s = self.scale;
                let caret = view::caret_at(g, &self.app, &self.layout, f, x, s);
                let double = self.last_click.is_some_and(|(t, cx, cy)| {
                    t.elapsed() < Duration::from_millis(500) && (cx - x).abs() < 4.0 * s && (cy - y).abs() < 4.0 * s
                });
                if double && self.app.focus == Some(f) {
                    self.app.select_word(caret);
                    self.last_click = None;
                } else {
                    if mods.shift && self.app.focus == Some(f) {
                        self.app.drag_caret(caret);
                    } else {
                        self.app.focus_field(f, Some(caret));
                    }
                    self.selecting = true;
                    self.last_click = Some((Instant::now(), x, y));
                }
            }
            Hot::Pick(f) => self.app.calendar = Some(crate::app::Calendar::open(f, self.app.choice.text(f))),
            Hot::CalPrev | Hot::CalNext => {
                if let Some(c) = &mut self.app.calendar {
                    c.shift(if self.hot == Hot::CalPrev { -1 } else { 1 });
                }
            }
            Hot::CalDay(i) => {
                let day = self.layout.cal.as_ref().and_then(|c| c.days.get(i)).map(|d| d.1);
                if let (Some((y, m, d)), Some(c)) = (day, self.app.calendar.take()) {
                    self.app.choice.texts[c.field.index()] = format!("{y:04}-{m:02}-{d:02}");
                }
            }
            Hot::CalToday => {
                if let Some(c) = self.app.calendar.take() {
                    let t = rt_sys::local_time();
                    self.app.choice.texts[c.field.index()] = format!("{:04}-{:02}-{:02}", t.year, t.month, t.day);
                }
            }
            Hot::CalClear => {
                if let Some(c) = self.app.calendar.take() {
                    self.app.choice.texts[c.field.index()].clear();
                }
            }
            Hot::CalPanel => {}
            Hot::Kind(i) => self.app.choice.toggle_kind(i),
            Hot::Compare(i) => self.app.compare[i] = !self.app.compare[i],
            Hot::Clear => self.app.choice.clear(),
            Hot::Card(i) => {
                if let Some(p) = self.app.volumes.as_ref().and_then(|v| v.get(i)).map(|v| v.path.clone()) {
                    self.app.toggle_root(&p);
                    self.app.error = None;
                }
            }
            Hot::Remove(i) => {
                if let Some(p) = self.app.folders().get(i) {
                    self.app.remove_folder(p);
                }
            }
            Hot::Folder(i) => {
                if let Some(p) = self.app.folders().get(i) {
                    self.app.toggle_folder(p);
                    self.app.error = None;
                }
            }
            Hot::MinSize(i) => {
                if let Some(&size) = crate::settings::MIN_SIZES.get(i) {
                    self.app.choice.set_min(if size <= 1 { String::new() } else { view::size_label(size) });
                }
            }
            Hot::Add => out = Some(Out::Command(Command::AddFolder)),
            Hot::Find => out = Some(Out::Command(Command::Search)),
            Hot::Stop => out = Some(Out::Command(Command::Stop)),
            Hot::Row(i) => {
                self.app.select(i);
                // The box at the start of the row.
                if x < self.layout.cols.name.0 + 32.0 * self.scale && self.app.markable(i) && self.acting.is_none() {
                    self.toggle_mark(i);
                }
            }
            Hot::Rules => out = Some(Out::Popup(Popup::Rules)),
            Hot::Unmark => out = Some(Out::Command(Command::UnmarkAll)),
            Hot::MoveTo => out = Some(Out::Command(Command::MoveTo)),
            Hot::Recycle => out = Some(Out::Command(Command::Recycle)),
            Hot::Tab(i) => self.app.set_view(if i == 1 { View::Folders } else { View::Files }),
            Hot::Share(i) => {
                if let Some(&share) = crate::folders::SHARES.get(i) {
                    self.app.set_share(share);
                }
            }
            Hot::None => {
                let l = &self.layout;
                if view::page(&self.app) == Page::Result && l.track.contains(x, y) && self.app.rows.len() > l.page_rows() {
                    self.dragging = true;
                    self.drag_to(y);
                }
            }
        }
        self.fx.menus();
        self.fx.redraw();
        out
    }

    /// The left button was released.
    pub fn release(&mut self) {
        self.dragging = false;
        self.selecting = false;
    }

    /// A click with the right button: the context menu of the row under the pointer.
    pub fn right_click(&mut self) -> Option<Popup> {
        if let Hot::Row(i) = self.hot {
            self.app.select(i);
            self.fx.menus();
            self.fx.redraw();
            if self.app.selected_path().is_some() {
                return Some(Popup::Item);
            }
        }
        None
    }

    pub fn drag_to(&mut self, y: f32) {
        let page = self.layout.page_rows();
        self.app.scroll = view::scroll_for(&self.layout, self.app.rows.len(), page, y);
        self.fx.redraw();
    }

    /// The mouse wheel, with the pointer at `at`.
    pub fn wheel(&mut self, delta: MouseScrollDelta, at: Option<(f32, f32)>) {
        let Some((x, y)) = at else { return };
        let rows = match delta {
            MouseScrollDelta::LineDelta(_, l) => (-l * 3.0).round() as isize,
            MouseScrollDelta::PixelDelta(p) => (-p.y as f32 / self.layout.row_h.max(20.0)).round() as isize,
        };
        let l = &self.layout;
        if let Some(c) = &mut self.app.calendar {
            // Over the calendar the wheel turns the months; elsewhere it closes it.
            if l.cal.as_ref().is_some_and(|cl| cl.panel.contains(x, y)) {
                c.shift(rows.signum() as i32);
            } else {
                self.app.calendar = None;
            }
            self.fx.redraw();
            return;
        }
        match view::page(&self.app) {
            Page::Result => self.app.scroll_by(rows),
            Page::Start if l.start_max > 0.0 => {
                let step = 40.0 * self.scale;
                self.app.start_scroll = (self.app.start_scroll + rows as f32 * step / 3.0).clamp(0.0, l.start_max);
            }
            _ => return,
        }
        self.fx.redraw();
        self.pointer(x, y, false);
    }

    // ------------------------------------------------------------ screen readers

    /// The page's nodes for the screen readers. Rows are 100 + their index.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role};
        let mut focus = None;
        if let (Page::Result, Some(found)) = (view::page(&self.app), &self.app.found) {
            let mut list = Node::new(Role::List);
            list.set_label(tr("Duplicates"));
            let mut rows = Vec::new();
            let end = (self.app.scroll + self.layout.page_rows()).min(self.app.rows.len());
            for (i, row) in self.app.rows.iter().enumerate().take(end).skip(self.app.scroll) {
                let nid = NodeId(100 + i as u64);
                let mut n = Node::new(Role::ListItem);
                n.set_label(match *row {
                    Row::Group(g) => view::group_text(found, g),
                    Row::File(g, f) => found.groups[g].files[f].path.display().to_string(),
                    Row::FolderGroup(g) => view::folder_group_text(&self.app, g),
                    Row::Folder(..) => self.app.path_of(i).map_or(String::new(), |p| p.display().to_string()),
                });
                n.set_selected(self.app.sel == Some(i));
                n.add_action(accesskit::Action::Click);
                if self.app.sel == Some(i) {
                    focus = Some(nid);
                }
                rows.push(nid);
                nodes.push((nid, n));
            }
            list.set_children(rows);
            nodes.push((NodeId(1), list));
            children.push(NodeId(1));
        }
        let (left, right) = view::status_text(&self.app, None);
        let mut st = Node::new(Role::Label);
        st.set_value(format!("{left} {right}"));
        nodes.push((NodeId(3), st));
        children.push(NodeId(3));
        focus
    }

    /// A screen reader clicked the row `i`.
    pub fn accessibility_click(&mut self, i: usize) {
        self.app.select(i);
        self.fx.redraw();
    }
}

/// Opens a file with its default program (an explicit command).
fn open_item(p: &Path) {
    #[cfg(windows)]
    rt_sys::win::shell_open(&p.to_string_lossy());
    #[cfg(not(windows))]
    rt_sys::linux::xdg_open(p.as_os_str().to_os_string());
}

/// The keys of the page, for the Keyboard shortcuts panel.
pub fn shortcuts_text() -> String {
    let rows = [
        ("Ctrl+O", tr("Add a folder to the search")),
        ("F5", tr("Find the duplicates")),
        ("Ctrl+N", tr("Change the drives and folders")),
        ("Esc", tr("Stop the search")),
        ("Tab", tr("Go to the next filter box")),
        ("Ctrl+1  Ctrl+2", tr("Show the files or the folders")),
        ("Space", tr("Mark or unmark the selected copy")),
        ("Delete", if cfg!(windows) { tr("Move the marked copies to the Recycle Bin") } else { tr("Move the marked copies to the trash") }),
        ("Ctrl+M", tr("Move the marked copies to a folder")),
        ("Ctrl+S", tr("Save the list as CSV")),
        ("↑ ↓  PgUp PgDn  Home End", tr("Move in the list")),
        ("Ctrl+Enter", tr("Open the selected file")),
        ("Ctrl+E", tr("Show it in the file manager")),
        ("Ctrl+C", tr("Copy its path")),
    ];
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
}
