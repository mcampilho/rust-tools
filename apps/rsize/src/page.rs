//! The disk space page: the drives, the scan on a thread of its own, the
//! folders and file types, the treemap and the charts (made again only when
//! what they show changed), the selection, and the page's drawing, keyboard
//! and mouse. The window of rsize (`main.rs`) and the Large Files of rcare
//! both hold one.
//!
//! The page does not own the window. What it needs from it (draw again,
//! show a message, save the settings) it notes in `fx`; the dialogs are the
//! ones the window passes in ([`Dialogs`]); what the threads report comes
//! back as an [`Event`] through the [`Sender`]; the menu that pops up at the
//! pointer is asked for with [`Popup`] and shown by the window.
//!
//! The page keeps the settings that are its own (the chart, the layout, the
//! size shown, the splitters); the window keeps the theme, the language and
//! the size of its window in the same [`Settings`], which the page holds.

use crate::app::{App, Scanning};
use crate::chart::{Chart, Hit};
use crate::commands::{self, Command};
use crate::i18n::{self, tf, tr};
use crate::model::{Id, SizeMode};
use crate::scan::{self, Event as ScanEvent};
use crate::settings::{ChartType, Settings, ViewMode, SPLIT_MAX, SPLIT_MIN, SPLIT_X_MAX, SPLIT_X_MIN};
use crate::treemap::Map;
use crate::ui::gfx::{Color, Gfx, Rect};
use crate::ui::theme::Theme;
use crate::ui::view::{self, Hot, Page};
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
    Scan(u64, ScanEvent),
    Volumes(Vec<Volume>),
    /// The space of the scanned folder's volume (for the scan of that generation).
    Space(u64, Option<volumes::Space>),
    /// The system's names of the scanned file types, by extension (for that generation).
    TypeNames(u64, std::collections::HashMap<String, String>),
    /// Deleting ended: how many things stayed, and the first ones with the
    /// reason (empty when the user said no).
    Removed(u64, Vec<(PathBuf, String)>),
}

/// What is being deleted, in the background.
struct Removing {
    ctl: Arc<rt_ops::Control>,
    /// The tree it was picked in, and where.
    tree_gen: u64,
    id: Id,
    path: PathBuf,
    name: String,
    /// The files in it, from the reading (nothing is counted again).
    files: u64,
    /// For good, not to the Recycle Bin.
    permanent: bool,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Where the threads send their events (to the window's event loop).
pub type Sender = Arc<dyn Fn(Event) + Send + Sync>;

/// A menu that pops up at the pointer; the window shows it and runs the command chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popup {
    /// The context menu of the selection.
    Item,
}

#[derive(Clone, Copy)]
enum Drag {
    Splitter,
    VSplitter,
    Thumb,
    ExtThumb,
}

const DOUBLE_CLICK: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_SLOP: f32 = 4.0;
/// How long the window size must stay the same before the treemap is made again.
const RESIZE_PAUSE: Duration = Duration::from_millis(150);

/// What the cached treemap was drawn for: zoom, size, order, tree.
type MapKey = (Id, u32, u32, SizeMode, u64);
/// What the cached pie or rings were drawn for: the same, plus the chart,
/// the background and the scale.
type ChartKey = (ChartType, MapKey, u32, u32);

pub struct SizePage {
    pub app: App,
    /// The settings (the window keeps its theme, language and size in them too).
    pub settings: Settings,
    pub layout: view::Layout,
    pub hot: Hot,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    /// Running with administrator rights (NTFS can be read directly).
    pub elevated: bool,
    map: Option<Map>,
    map_key: Option<MapKey>,
    /// When to make the treemap again at the new window size.
    map_due: Option<Instant>,
    /// The pie or the rings, and what they were drawn for.
    chart: Option<Chart>,
    chart_key: Option<ChartKey>,
    /// What the pointer is on in the pie or the rings.
    hover_seg: Option<Hit>,
    /// The treemap with the files of the picked type standing out, and
    /// what it was made for: the treemap and the type.
    lit: Option<((MapKey, u32), tiny_skia::Pixmap)>,
    /// What the table of file types was made for: folder, order, tree.
    exts_key: Option<(Id, SizeMode, u64)>,
    /// The last change of the window size.
    resized: Instant,
    /// The node under the pointer (list or treemap).
    hover: Option<Id>,
    drag: Option<Drag>,
    handle: Option<scan::Handle>,
    removing: Option<Removing>,
    generation: u64,
    /// Which tree the treemap was made from (changes with each scan).
    tree_gen: u64,
    last_click: Option<(Instant, f32, f32, u32)>,
    scale: f32,
    send: Sender,
}

impl SizePage {
    pub fn new(settings: Settings, elevated: bool, send: Sender) -> SizePage {
        SizePage {
            app: App::new(),
            settings,
            layout: view::Layout::default(),
            hot: Hot::None,
            cursor_icon: CursorIcon::Default,
            fx: Effects::default(),
            elevated,
            map: None,
            map_key: None,
            map_due: None,
            chart: None,
            chart_key: None,
            hover_seg: None,
            lit: None,
            exts_key: None,
            resized: Instant::now(),
            hover: None,
            drag: None,
            handle: None,
            removing: None,
            generation: 0,
            tree_gen: 0,
            last_click: None,
            scale: 1.0,
            send,
        }
    }

    /// The window changed size: the treemap is made again once it settles.
    pub fn resized(&mut self) {
        self.resized = Instant::now();
    }

    /// Lets go of the drawings (the window is hidden).
    pub fn release_drawings(&mut self) {
        self.drag = None;
        self.map = None;
        self.map_key = None;
        self.chart = None;
        self.chart_key = None;
    }

    /// The title of the window: what is being read, or the folder shown.
    pub fn title(&self) -> Option<String> {
        match (&self.app.scanning, &self.app.scan) {
            (Some(s), _) => Some(s.root.display().to_string()),
            (None, Some(s)) if !self.app.show_drives => Some(s.tree.name(s.tree.root()).to_string()),
            _ => None,
        }
    }

    /// Whether a scan is going on.
    pub fn scanning(&self) -> bool {
        self.app.scanning.is_some()
    }

    // ------------------------------------------------------------ scans

    /// Reads the drives, to show them.
    pub fn load_volumes(&self) {
        let send = self.send.clone();
        std::thread::spawn(move || send(Event::Volumes(volumes::list())));
    }

    /// Asks for the space of the volume `root` is on, in the background (a
    /// network drive can be slow to answer).
    fn load_space(&self, root: &Path) {
        let (send, gen, root) = (self.send.clone(), self.generation, root.to_path_buf());
        std::thread::spawn(move || send(Event::Space(gen, volumes::space_of(&root))));
    }

    /// Asks the system what the scanned file types are called, in the
    /// background: thousands of them take a moment.
    fn load_type_names(&self) {
        let Some(t) = self.app.scan.as_ref().map(|s| &s.tree) else { return };
        let exts: Vec<String> = t.ext_ranking().iter().map(|&e| t.ext_name(e).to_string()).collect();
        let (send, gen) = (self.send.clone(), self.generation);
        std::thread::spawn(move || {
            #[cfg(windows)]
            let names = exts.into_iter().filter_map(|e| Some((e.clone(), rt_sys::win::file_type_name(&e)?))).collect();
            #[cfg(not(windows))]
            let names = {
                let _ = exts;
                std::collections::HashMap::new()
            };
            send(Event::TypeNames(gen, names));
        });
    }

    pub fn start_scan(&mut self, path: &Path) {
        if let Some(h) = self.handle.take() {
            h.cancel();
        }
        self.generation += 1;
        let gen = self.generation;
        let root = scan::normalize(path);
        self.app.scanning = Some(Scanning { root: root.clone(), progress: Default::default(), started: Instant::now(), generation: gen });
        self.app.error = None;
        let send = self.send.clone();
        let lang = i18n::lang();
        self.handle = Some(scan::start(
            root,
            Box::new(move |e| {
                i18n::set_lang(lang);
                send(Event::Scan(gen, e));
            }),
        ));
        self.drag = None;
        self.fx.menus();
        self.fx.redraw();
    }

    /// Stops the scan under way and the deleting (the window is closing); a
    /// deletion for good is waited for, so it ends on a whole file.
    pub fn cancel(&mut self) {
        if let Some(h) = &self.handle {
            h.cancel();
        }
        if let Some(r) = &mut self.removing {
            r.ctl.stop();
            if let (true, Some(t)) = (r.permanent, r.thread.take()) {
                let _ = t.join();
            }
        }
    }

    /// Before the app exits: with something still being deleted, asks whether
    /// to stop it. True to go on.
    pub fn may_close(&self, dialogs: &mut dyn Dialogs) -> bool {
        self.removing.is_none() || dialogs.confirm(&tr("Delete"), &tr("Something is still being deleted. Stop and exit?"))
    }

    /// Folders from a later launch (or the Explorer menu): the first is read.
    pub fn open_paths(&mut self, paths: &[PathBuf]) {
        if let Some(p) = paths.first() {
            self.start_scan(p);
        }
    }

    /// A file or folder dropped on the window: its folder is read.
    pub fn drop_file(&mut self, p: PathBuf) {
        let dir = if p.is_dir() { p } else { p.parent().map(Path::to_path_buf).unwrap_or(p) };
        self.start_scan(&dir);
    }

    /// The arguments of a new instance with administrator rights: the same
    /// folder, now read directly, without its last backslash, which would
    /// escape the closing quote ("C:" is read as C:\ again, see `scan::normalize`).
    pub fn restart_args(&self) -> String {
        let root = self
            .app
            .scanning
            .as_ref()
            .map(|s| s.root.clone())
            .or_else(|| self.app.scan.as_ref().map(|s| PathBuf::from(s.tree.name(s.tree.root()))));
        root.map_or(String::new(), |r| format!("\"{}\"", r.display().to_string().trim_end_matches('\\')))
    }

    /// What a work thread reported.
    pub fn event(&mut self, event: Event) {
        match event {
            Event::Scan(gen, ev) => self.on_scan(gen, ev),
            Event::Volumes(v) => {
                self.app.volumes = Some(v);
                self.fx.redraw();
            }
            Event::Space(gen, space) if gen == self.generation => {
                self.app.space = space;
                self.fx.redraw();
            }
            Event::TypeNames(gen, names) if gen == self.generation => {
                self.app.type_names = names;
                self.app.sort_exts();
                self.fx.redraw();
            }
            Event::Space(..) | Event::TypeNames(..) => {}
            Event::Removed(count, first) => self.removed(count, first),
        }
    }

    fn on_scan(&mut self, gen: u64, ev: ScanEvent) {
        if gen != self.generation {
            return;
        }
        match ev {
            ScanEvent::Progress(p) => {
                if let Some(s) = &mut self.app.scanning {
                    s.progress = p;
                }
            }
            ScanEvent::Done(result) => {
                let root = self.app.scanning.take().map(|s| s.root).unwrap_or_default();
                self.handle = None;
                match result {
                    Ok(scan) => {
                        self.app.set_scan(*scan, self.settings.size_mode);
                        self.tree_gen += 1;
                        self.app.space = None;
                        self.load_space(&root);
                        self.load_type_names();
                        self.map = None;
                        self.hover = None;
                    }
                    Err(e) => {
                        self.app.error = Some(tf("Could not read {}: {}", &[&root.to_string_lossy(), &e]));
                        self.app.show_drives = true;
                    }
                }
                self.fx.menus();
            }
        }
        self.fx.redraw();
    }

    /// The time the window should wake up at: the elapsed time on the scan
    /// page moves even between reports, and a stretched treemap is made
    /// again once the resizing pauses.
    pub fn animate(&mut self) -> Option<Instant> {
        // The progress of the deleting, drawn again only when it changed.
        let tick = self.removing.is_some().then(|| {
            self.show_progress();
            Instant::now() + Duration::from_millis(200)
        });
        if self.app.scanning.is_some() {
            self.fx.redraw();
            return Some(tick.unwrap_or_else(|| Instant::now() + Duration::from_millis(500)));
        }
        let Some(due) = self.map_due else { return tick };
        if Instant::now() >= due {
            self.fx.redraw();
            tick
        } else {
            Some(tick.map_or(due, |t| t.min(due)))
        }
    }

    fn show_progress(&mut self) {
        let Some(r) = &self.removing else { return };
        let text = if r.permanent {
            let done = crate::format::count(r.ctl.files().min(r.files));
            tf("Deleting “{}”: {} of {} files (Esc stops)", &[&r.name, &done, &crate::format::count(r.files)])
        } else if cfg!(windows) {
            tf("Moving “{}” to the Recycle Bin…", &[&r.name])
        } else {
            tf("Moving “{}” to the trash…", &[&r.name])
        };
        if self.app.working.as_ref() != Some(&text) {
            self.app.working = Some(text);
            self.fx.redraw();
        }
    }

    // ------------------------------------------------------------ commands

    pub fn checked(&self, c: Command) -> bool {
        match c {
            Command::Chart(kind) => self.settings.chart_shown() == kind,
            Command::View(mode) => self.settings.view == mode,
            Command::SizeLogical => self.settings.size_mode == SizeMode::Logical,
            Command::SizeAllocated => self.settings.size_mode == SizeMode::Allocated,
            _ => false,
        }
    }

    /// Whether the command can be used now (menus and buttons).
    pub fn enabled(&self, c: Command) -> bool {
        let result = view::page(&self.app) == Page::Result;
        match c {
            Command::Rescan => self.app.has_tree() && self.app.scanning.is_none(),
            Command::Stop => self.app.scanning.is_some() || self.removing.as_ref().is_some_and(|r| r.permanent),
            #[cfg(windows)]
            Command::RestartAdmin => !self.elevated,
            Command::CopyPath | Command::OpenItem | Command::Reveal => result && self.app.sel.is_some(),
            Command::ExportCsv => result,
            // Anything but the folder that was read.
            Command::Delete | Command::DeletePermanently => result && self.removing.is_none() && self.app.sel.is_some_and(|s| s != 0),
            Command::ZoomIn => result && self.app.sel.is_some(),
            Command::ZoomOut | Command::ZoomTop => result && self.app.scan.as_ref().is_some_and(|s| s.tree.parent(self.app.zoom).is_some()),
            _ => true,
        }
    }

    /// Does the commands that are the page's own; false for the others
    /// (Exit, theme, language, help… belong to the window).
    pub fn command(&mut self, cmd: Command, dialogs: &mut dyn Dialogs) -> bool {
        if !matches!(
            cmd,
            Command::ChooseFolder
                | Command::Drives
                | Command::Rescan
                | Command::Stop
                | Command::ExportCsv
                | Command::Delete
                | Command::DeletePermanently
                | Command::CopyPath
                | Command::OpenItem
                | Command::Reveal
                | Command::ZoomIn
                | Command::ZoomOut
                | Command::ZoomTop
                | Command::Chart(_)
                | Command::View(_)
                | Command::SizeLogical
                | Command::SizeAllocated
        ) {
            return false;
        }
        if !self.enabled(cmd) {
            return true;
        }
        match cmd {
            Command::ChooseFolder => {
                if let Some(p) = dialogs.pick_folder(&tr("Choose a folder to analyze")) {
                    self.settings.last_dir = p.display().to_string();
                    self.start_scan(&p);
                }
            }
            Command::Drives => {
                self.app.show_drives = true;
                self.app.volumes = None;
                self.load_volumes();
            }
            Command::Rescan => {
                if let Some(root) = self.app.scan.as_ref().map(|s| PathBuf::from(s.tree.name(s.tree.root()))) {
                    self.start_scan(&root);
                }
            }
            // The scan first; with none, the deleting.
            Command::Stop => match (&self.handle, &self.removing) {
                (Some(h), _) => h.cancel(),
                (None, Some(r)) => r.ctl.stop(),
                (None, None) => {}
            },
            Command::ExportCsv => self.export_csv(dialogs),
            Command::Delete => self.delete(false, dialogs),
            Command::DeletePermanently => self.delete(true, dialogs),
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
            Command::ZoomIn => self.app.zoom_in(),
            Command::ZoomOut => self.app.zoom_out(),
            Command::ZoomTop => self.app.zoom_top(),
            Command::Chart(kind) => {
                self.settings.chart = kind;
                // The pie and the rings need the larger area of the map layout.
                if kind != ChartType::Treemap {
                    self.settings.view = ViewMode::Map;
                }
                self.hover = None;
                self.hover_seg = None;
            }
            Command::View(mode) => {
                self.settings.view = mode;
                self.hover = None;
                self.hover_seg = None;
            }
            Command::SizeLogical | Command::SizeAllocated => {
                let mode = if cmd == Command::SizeLogical { SizeMode::Logical } else { SizeMode::Allocated };
                self.settings.size_mode = mode;
                self.app.set_mode(mode);
            }
            _ => {}
        }
        self.fx.menus();
        self.fx.redraw();
        true
    }

    /// Writes the folder shown, with all it holds, to a CSV file.
    fn export_csv(&mut self, dialogs: &mut dyn Dialogs) {
        let Some(scan) = &self.app.scan else { return };
        let name = crate::export::suggested_name(&scan.tree, self.app.zoom);
        let dir = (!self.settings.last_dir.is_empty()).then(|| PathBuf::from(&self.settings.last_dir));
        let Some(path) = dialogs.save_file(&name, (&tr("CSV files"), "*.csv"), dir.as_deref()) else { return };
        let result = std::fs::File::create(&path).and_then(|f| crate::export::write_csv(&scan.tree, self.app.zoom, f));
        let shown = path.display().to_string();
        self.app.note = Some(match result {
            Ok(rows) => tf("{} rows written to {}", &[&crate::format::count(rows), &shown]),
            Err(e) => tf("Could not write {}: {}", &[&shown, &e.to_string()]),
        });
        if let Some(d) = path.parent() {
            self.settings.last_dir = d.display().to_string();
        }
    }

    /// Deletes the selection, after asking: to the Recycle Bin (the trash on
    /// Linux), or for good. In the background; `removed` takes it out of the
    /// tree when it ends.
    fn delete(&mut self, permanent: bool, dialogs: &mut dyn Dialogs) {
        let Some(scan) = &self.app.scan else { return };
        let Some(id) = self.app.sel.filter(|&s| s != scan.tree.root()) else { return };
        if self.removing.is_some() {
            return;
        }
        let t = &scan.tree;
        let path = t.path(id);
        let name = t.name(id).to_string();
        let n = t.node(id);
        let files = if n.is_dir() { n.files } else { 1 };
        let what = if n.is_dir() {
            tf("{} in {} files", &[&crate::format::size(n.size), &crate::format::count(n.files)])
        } else {
            crate::format::size(n.size)
        };
        let shown = path.display().to_string();
        let question = match (permanent, cfg!(windows)) {
            (true, _) => tf("Delete “{}” permanently?\n\n{}\n{}\n\nIt cannot be brought back.", &[&name, &shown, &what]),
            (false, true) => tf("Move “{}” to the Recycle Bin?\n\n{}\n{}", &[&name, &shown, &what]),
            (false, false) => tf("Move “{}” to the trash?\n\n{}\n{}", &[&name, &shown, &what]),
        };
        if !dialogs.confirm(&tr("Delete"), &question) {
            return;
        }
        let ctl = Arc::new(rt_ops::Control::new());
        let (send, theirs, target) = (self.send.clone(), ctl.clone(), path.clone());
        let lang = i18n::lang();
        let thread = std::thread::spawn(move || {
            i18n::set_lang(lang);
            let target = std::slice::from_ref(&target);
            if permanent {
                let f = rt_ops::delete_auto(target, &theirs);
                send(Event::Removed(f.count, f.first));
            } else {
                let stayed = rt_sys::recycle_many(target);
                send(Event::Removed(stayed.len() as u64, stayed));
            }
        });
        self.removing = Some(Removing { ctl, tree_gen: self.tree_gen, id, path, name, files, permanent, thread: Some(thread) });
        self.app.note = None;
        self.show_progress();
    }

    /// Deleting ended.
    fn removed(&mut self, count: u64, first: Vec<(PathBuf, String)>) {
        let Some(r) = self.removing.take() else { return };
        self.app.working = None;
        // The drive's used and free space changed (also when only part went);
        // what is shown stays until the new figures come.
        if let Some(root) = self.app.scan.as_ref().map(|s| PathBuf::from(s.tree.name(s.tree.root()))) {
            self.load_space(&root);
        }
        let gone = r.path.symlink_metadata().is_err();
        // A newer reading already shows how things are.
        if gone && r.tree_gen == self.tree_gen {
            self.app.removed(r.id);
            self.tree_gen += 1;
            self.hover = None;
            self.hover_seg = None;
        }
        self.app.note = if gone {
            Some(match (r.permanent, cfg!(windows)) {
                (true, _) => tf("“{}” was deleted.", &[&r.name]),
                (false, true) => tf("“{}” was moved to the Recycle Bin.", &[&r.name]),
                (false, false) => tf("“{}” was moved to the trash.", &[&r.name]),
            })
        } else if r.ctl.stopped() {
            Some(tf("Stopped: part of “{}” is still there. Press F5 to read again.", &[&r.name]))
        } else {
            match first.first() {
                // An empty reason: the user said no to deleting it for good.
                Some((_, why)) if why.is_empty() => None,
                Some((p, why)) if count > 1 => {
                    let why = format!("{} ({}): {why}", p.display(), crate::format::count(count));
                    Some(tf("Could not delete “{}”: {}", &[&r.name, &why]))
                }
                Some((_, why)) => Some(tf("Could not delete “{}”: {}", &[&r.name, why])),
                None => None,
            }
        };
        self.fx.menus();
        self.fx.redraw();
    }

    // ------------------------------------------------------------ drawing

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32, menu_bar: bool) {
        self.scale = s;
        self.layout = view::layout(&self.app, self.settings.view, self.settings.panes(), area, s, menu_bar);
        self.app.page = self.layout.page_rows();
        self.app.types_page = self.layout.ext_rows();
        self.ensure_map(th.bg);
        self.ensure_lit();
        let frame = view::Frame {
            mode: self.settings.size_mode,
            kind: self.settings.chart_shown(),
            map: self.map.as_ref(),
            lit: self.lit.as_ref().map(|l| &l.1),
            chart: self.chart.as_ref(),
            hover_seg: self.hover_seg,
            hot: self.hot,
            hover: self.hover,
        };
        view::draw(g, &self.app, &self.layout, th, s, &frame);
    }

    /// Makes the treemap again when the folder, the size or the order
    /// changed. While the window is being resized, the old one is stretched
    /// and the new one waits until the size has stayed the same for a moment:
    /// a large tree takes a fraction of a second to lay out.
    fn ensure_map(&mut self, bg: Color) {
        if view::page(&self.app) != Page::Result {
            return;
        }
        let r = self.layout.map;
        let (w, h) = (r.w.max(1.0) as u32, r.h.max(1.0) as u32);
        let key = (self.app.zoom, w, h, self.settings.size_mode, self.tree_gen);
        // The table of file types follows the folder the chart shows.
        let ekey = (self.app.zoom, self.settings.size_mode, self.tree_gen);
        if self.exts_key != Some(ekey) {
            self.exts_key = Some(ekey);
            self.app.refresh_exts();
        }
        let Some(scan) = &self.app.scan else { return };
        if self.settings.chart_shown() != ChartType::Treemap {
            // A few thousand segments at most: quick enough to redo at once.
            let s = self.scale;
            let bg = (bg.r as u32) << 16 | (bg.g as u32) << 8 | bg.b as u32;
            let ckey = (self.settings.chart_shown(), key, bg, s.to_bits());
            if self.chart_key != Some(ckey) || self.chart.is_none() {
                let t = &scan.tree;
                self.chart = Some(match self.settings.chart_shown() {
                    ChartType::Pie => Chart::pie(t, self.app.zoom, w, h, s, bg),
                    _ => Chart::rings(t, self.app.zoom, w, h, s, bg),
                });
                self.chart_key = Some(ckey);
                self.hover_seg = None;
            }
            return;
        }
        if self.map_key == Some(key) && self.map.is_some() {
            self.map_due = None;
            return;
        }
        let only_size = self.map_key.is_some_and(|k| (k.0, k.3, k.4) == (key.0, key.3, key.4)) && self.map.is_some();
        if only_size && self.resized.elapsed() < RESIZE_PAUSE {
            self.map_due = Some(self.resized + RESIZE_PAUSE);
            return;
        }
        let head = (16.0 * self.scale).round();
        self.map = Some(Map::new(&scan.tree, self.app.zoom, w, h, head));
        self.map_key = Some(key);
        self.map_due = None;
    }

    /// Makes the treemap with the files of the picked type standing out,
    /// when a type is picked and the treemap or the type changed.
    fn ensure_lit(&mut self) {
        let want = match (self.app.ext_sel, &self.map, self.map_key, &self.app.scan) {
            (Some(e), Some(m), Some(k), Some(scan)) if self.settings.chart_shown() == ChartType::Treemap => Some((k, e, m, &scan.tree)),
            _ => None,
        };
        match want {
            Some((k, e, m, t)) => {
                if self.lit.as_ref().map(|l| l.0) != Some((k, e)) {
                    self.lit = Some(((k, e), m.highlighted(t, e)));
                }
            }
            None => self.lit = None,
        }
    }

    /// A point of the window in the treemap's own pixels (it may be
    /// stretched while the window is resized).
    fn map_point(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let (m, r) = (self.map.as_ref()?, self.layout.map);
        (r.contains(x, y) && r.w > 0.0 && r.h > 0.0).then(|| ((x - r.x) * m.w as f32 / r.w, (y - r.y) * m.h as f32 / r.h))
    }

    // ------------------------------------------------------------ input

    fn count_click(&mut self, x: f32, y: f32) -> u32 {
        let now = Instant::now();
        let n = match self.last_click {
            Some((t, cx, cy, n)) if now - t < DOUBLE_CLICK && (cx - x).abs() < DOUBLE_CLICK_SLOP && (cy - y).abs() < DOUBLE_CLICK_SLOP => {
                n % 2 + 1
            }
            _ => 1,
        };
        self.last_click = Some((now, x, y, n));
        n
    }

    /// A key: the command of a chord (for the window to run), or the arrows
    /// and Space in the list.
    pub fn key(&mut self, event: &winit::event::KeyEvent, mods: Mods) -> Option<Command> {
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
                NamedKey::F6 => "f6".into(),
                NamedKey::F7 => "f7".into(),
                NamedKey::F8 => "f8".into(),
                NamedKey::F9 => "f9".into(),
                NamedKey::Delete => "delete".into(),
                NamedKey::Enter => "enter".into(),
                NamedKey::Backspace => "backspace".into(),
                NamedKey::Escape => "esc".into(),
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
            WKey::Named(NamedKey::ArrowLeft) => self.app.left(),
            WKey::Named(NamedKey::ArrowRight) => self.app.right(),
            WKey::Named(NamedKey::Space) => {
                if let Some(id) = self.app.sel {
                    self.app.toggle(id);
                }
            }
            _ => return None,
        }
        self.fx.menus();
        self.fx.redraw();
        None
    }

    /// Updates what the pointer is on; `blocked` while a message is open.
    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        let l = &self.layout;
        let mut seg = None;
        let (hot, hover) = match view::page(&self.app) {
            Page::Start if !l.body.contains(x, y) => (Hot::None, None),
            Page::Start => {
                let card = l.cards.iter().position(|r| r.contains(x, y));
                (card.map_or(if l.choose.contains(x, y) { Hot::Choose } else { Hot::None }, Hot::Card), None)
            }
            Page::Scanning => (if l.stop.contains(x, y) { Hot::Stop } else { Hot::None }, None),
            Page::Result => {
                if l.splitter.contains(x, y) {
                    (Hot::Splitter, None)
                } else if l.vsplit.contains(x, y) {
                    (Hot::VSplitter, None)
                } else if let Some(&(kind, _)) = l.tabs.iter().find(|(_, r)| r.contains(x, y)) {
                    (Hot::Tab(kind), None)
                } else if let Some(&(mode, _)) = l.modes.iter().find(|(_, r)| r.contains(x, y)) {
                    (Hot::Mode(mode), None)
                } else if let Some(c) = l.head_at(x, y) {
                    (Hot::Head(c), None)
                } else if let Some(c) = l.ext_head_at(x, y) {
                    (Hot::ExtHead(c), None)
                } else if let Some(i) = l.row_at(&self.app, x, y) {
                    (Hot::Row(i), Some(self.app.rows[i].0))
                } else if let Some(i) = l.ext_row_at(&self.app, x, y) {
                    (Hot::ExtRow(i), None)
                } else if let (Some(c), true) = (&self.chart, self.settings.chart_shown() != ChartType::Treemap && l.map.contains(x, y)) {
                    seg = c.hit(x - l.map.x, y - l.map.y);
                    let node = match seg {
                        Some(Hit::Segment(i)) => c.segments[i].node,
                        Some(Hit::Center) => Some(c.root),
                        None => None,
                    };
                    (Hot::None, node)
                } else if let Some((mx, my)) = self.map_point(x, y) {
                    (Hot::None, self.map.as_ref().and_then(|m| m.hit(mx, my)))
                } else {
                    (Hot::None, None)
                }
            }
        };
        let hot = if blocked { Hot::None } else { hot };
        if hot != self.hot || hover != self.hover || seg != self.hover_seg {
            self.hot = hot;
            self.hover = hover;
            self.hover_seg = seg;
            self.fx.redraw();
        }
        self.cursor_icon = match (self.drag, hot) {
            (Some(Drag::Splitter), _) | (_, Hot::Splitter) => CursorIcon::NsResize,
            (Some(Drag::VSplitter), _) | (_, Hot::VSplitter) => CursorIcon::EwResize,
            (_, Hot::Card(_) | Hot::Choose | Hot::Stop | Hot::Tab(_) | Hot::Mode(_) | Hot::Head(_) | Hot::ExtHead(_)) => {
                CursorIcon::Pointer
            }
            _ if seg == Some(Hit::Center) => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        };
    }

    /// The pointer left the page.
    pub fn pointer_left(&mut self) {
        if self.drag.is_none() && (self.hot != Hot::None || self.hover.is_some()) {
            self.hot = Hot::None;
            self.hover = None;
            self.fx.redraw();
        }
    }

    /// A click at (x, y): the command of a button, for the window to run.
    pub fn press(&mut self, x: f32, y: f32) -> Option<Command> {
        let clicks = self.count_click(x, y);
        let mut out = None;
        match self.hot {
            Hot::Card(i) => {
                if let Some(p) = self.app.volumes.as_ref().and_then(|v| v.get(i)).map(|v| v.path.clone()) {
                    self.start_scan(&p);
                }
            }
            Hot::Choose => out = Some(Command::ChooseFolder),
            Hot::Stop => out = Some(Command::Stop),
            Hot::Splitter => self.drag = Some(Drag::Splitter),
            Hot::VSplitter => self.drag = Some(Drag::VSplitter),
            Hot::Tab(kind) => out = Some(Command::Chart(kind)),
            Hot::Mode(mode) => out = Some(Command::View(mode)),
            Hot::Head(c) => self.app.sort_by(c),
            Hot::ExtHead(c) => self.app.sort_exts_by(c),
            Hot::ExtRow(i) => {
                let ext = self.app.exts.get(i).map(|e| e.0);
                self.app.ext_sel = if self.app.ext_sel == ext { None } else { ext };
            }
            Hot::Row(i) => {
                let (id, depth) = self.app.rows[i];
                if self.layout.on_arrow(x, depth) || clicks == 2 {
                    self.app.toggle(id);
                }
                self.app.select(id);
            }
            Hot::None => {
                let l = &self.layout;
                if l.track.contains(x, y) && self.app.rows.len() > l.page_rows() {
                    self.drag = Some(Drag::Thumb);
                    self.drag_to(x, y);
                } else if l.ext_track.contains(x, y) && self.app.exts.len() > l.ext_rows() {
                    self.drag = Some(Drag::ExtThumb);
                    self.drag_to(x, y);
                } else if self.settings.chart_shown() != ChartType::Treemap && l.map.contains(x, y) {
                    self.chart_press(clicks);
                } else if let Some((mx, my)) = self.map_point(x, y) {
                    if let Some(id) = self.map.as_ref().and_then(|m| m.hit(mx, my)) {
                        self.app.select(id);
                        if clicks == 2 {
                            let t = &self.app.scan.as_ref().expect("a treemap has a tree").tree;
                            // A folder's header shows that folder; a file, one folder down toward it.
                            if t.node(id).is_dir() && !t.children(id).is_empty() && id != self.app.zoom {
                                self.app.zoom = id;
                            } else {
                                self.zoom_toward(id);
                            }
                        }
                    }
                }
            }
        }
        self.fx.menus();
        self.fx.redraw();
        out
    }

    /// A click on the pie or the rings selects; a double click on a folder
    /// shows it; a click in the middle of the rings goes up.
    fn chart_press(&mut self, clicks: u32) {
        match self.hover_seg {
            Some(Hit::Center) => self.app.zoom_out(),
            Some(Hit::Segment(i)) => {
                let Some(id) = self.chart.as_ref().and_then(|c| c.segments.get(i)).and_then(|s| s.node) else { return };
                self.app.select(id);
                let t = &self.app.scan.as_ref().expect("a chart has a tree").tree;
                if clicks == 2 && t.node(id).is_dir() && !t.children(id).is_empty() {
                    self.app.zoom = id;
                }
            }
            None => {}
        }
    }

    /// A double click in the treemap shows the folder one level down
    /// toward `id`.
    fn zoom_toward(&mut self, id: Id) {
        let Some(t) = self.app.scan.as_ref().map(|s| &s.tree) else { return };
        let mut cur = id;
        while let Some(p) = t.parent(cur) {
            if p == self.app.zoom {
                if t.node(cur).is_dir() && !t.children(cur).is_empty() {
                    self.app.zoom = cur;
                    self.app.select(cur);
                }
                return;
            }
            cur = p;
        }
    }

    /// The pointer moved while the left button is down: a splitter or a
    /// scroll bar's thumb follows it.
    pub fn drag(&mut self, x: f32, y: f32) {
        if self.drag.is_some() {
            self.drag_to(x, y);
        }
    }

    fn drag_to(&mut self, x: f32, y: f32) {
        match self.drag {
            Some(Drag::Splitter) => {
                // A share of the height below the block on top.
                let (top, h) = (self.layout.info.bottom(), self.layout.body.bottom() - self.layout.info.bottom());
                if h > 0.0 {
                    self.settings.panes_mut().split = ((y - top) / h).clamp(SPLIT_MIN, SPLIT_MAX);
                }
            }
            Some(Drag::VSplitter) => {
                let body = self.layout.body;
                if body.w > 0.0 {
                    self.settings.panes_mut().split_x = ((x - body.x) / body.w).clamp(SPLIT_X_MIN, SPLIT_X_MAX);
                }
            }
            Some(Drag::Thumb) => {
                let page = self.layout.page_rows();
                self.app.scroll = view::scroll_for(self.layout.track, self.app.rows.len(), page, y);
            }
            Some(Drag::ExtThumb) => {
                let page = self.layout.ext_rows();
                self.app.types_scroll = view::scroll_for(self.layout.ext_track, self.app.exts.len(), page, y);
            }
            None => return,
        }
        self.fx.redraw();
    }

    /// The left button was released: after a drag, the settings are saved.
    pub fn release(&mut self) {
        if self.drag.take().is_some() {
            self.fx.save();
        }
    }

    /// A click with the right button: in the chart it goes one folder up, as
    /// Backspace (a double click goes down, as Enter); Shift+right click
    /// opens the item's menu there. Elsewhere, the menu of the item under the pointer.
    pub fn right_click(&mut self, at: Option<(f32, f32)>, mods: Mods) -> Option<Popup> {
        let in_chart = at.is_some_and(|(x, y)| self.layout.map.contains(x, y));
        if view::page(&self.app) == Page::Result && in_chart && !mods.shift {
            self.app.zoom_out();
            self.hover = None;
            self.hover_seg = None;
            self.fx.menus();
            self.fx.redraw();
        } else if view::page(&self.app) == Page::Result && self.hover.is_some() {
            if let Some(id) = self.hover {
                self.app.select(id);
            }
            self.fx.menus();
            self.fx.redraw();
            return Some(Popup::Item);
        }
        None
    }

    /// The mouse wheel, with the pointer at `at`.
    pub fn wheel(&mut self, delta: MouseScrollDelta, at: Option<(f32, f32)>) {
        let Some((x, y)) = at else { return };
        let rows = match delta {
            MouseScrollDelta::LineDelta(_, l) => (-l * 3.0).round() as isize,
            MouseScrollDelta::PixelDelta(p) => (-p.y as f32 / self.layout.row_h.max(1.0)).round() as isize,
        };
        let l = &self.layout;
        match view::page(&self.app) {
            Page::Result if l.list.contains(x, y) || l.track.contains(x, y) || l.header.contains(x, y) => self.app.scroll_by(rows),
            Page::Result if l.ext_list.contains(x, y) || l.ext_track.contains(x, y) || l.ext_header.contains(x, y) => {
                self.app.scroll_types_by(rows)
            }
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
        if let (Page::Result, Some(scan)) = (view::page(&self.app), &self.app.scan) {
            let t = &scan.tree;
            let mut list = Node::new(Role::Tree);
            list.set_label(tr("Folders"));
            let mut rows = Vec::new();
            let end = (self.app.scroll + self.layout.page_rows()).min(self.app.rows.len());
            for (i, &(id, depth)) in self.app.rows.iter().enumerate().take(end).skip(self.app.scroll) {
                let nid = NodeId(100 + i as u64);
                let mut n = Node::new(Role::TreeItem);
                n.set_label(format!("{}, {}", t.name(id), crate::format::size(t.size(id))));
                n.set_level(depth as usize + 1);
                if !t.children(id).is_empty() {
                    n.set_expanded(self.app.expanded.contains(&id));
                }
                n.set_selected(self.app.sel == Some(id));
                n.add_action(accesskit::Action::Click);
                if self.app.sel == Some(id) {
                    focus = Some(nid);
                }
                rows.push(nid);
                nodes.push((nid, n));
            }
            list.set_children(rows);
            nodes.push((NodeId(1), list));
            let mut map = Node::new(Role::Image);
            map.set_label(view::chart_name(self.settings.chart_shown()));
            nodes.push((NodeId(2), map));
            children.extend([NodeId(1), NodeId(2)]);
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
        if let Some(&(node, _)) = self.app.rows.get(i) {
            self.app.select(node);
        }
        self.fx.redraw();
    }
}

/// Opens a file or folder with its default program (an explicit command).
fn open_item(p: &Path) {
    #[cfg(windows)]
    rt_sys::win::shell_open(&p.to_string_lossy());
    #[cfg(not(windows))]
    rt_sys::linux::xdg_open(p.as_os_str().to_os_string());
}

/// The keys of the page, for the Keyboard shortcuts panel.
pub fn shortcuts_text() -> String {
    let rows = [
        ("Ctrl+O", tr("Choose a folder to analyze")),
        ("Ctrl+D", tr("Show the drives")),
        ("F5", tr("Read the same folder again")),
        ("Esc", tr("Stop reading or deleting")),
        ("↑ ↓  PgUp PgDn  Home End", tr("Move in the list")),
        ("← →  Space", tr("Close or open a folder")),
        ("Enter", tr("Show the selected folder in the treemap")),
        ("Backspace", tr("Treemap: one folder up")),
        ("Ctrl+Backspace", tr("Treemap: the whole scan")),
        ("Double click", tr("Treemap: one folder down")),
        ("Right click", tr("Treemap: one folder up")),
        ("F3 / F4", tr("Folder view / map view")),
        ("F6 / F7 / F8", tr("Treemap / pie / rings")),
        ("Ctrl+Enter", tr("Open the selected file or folder")),
        ("Ctrl+E", tr("Show it in the file manager")),
        ("Ctrl+C", tr("Copy its path")),
        ("Ctrl+1 / Ctrl+2", tr("Show file sizes / sizes on disk")),
        ("Ctrl+S", tr("Export the folder shown to CSV")),
        ("Delete", tr("Move it to the Recycle Bin or trash, after asking")),
        ("Shift+Delete", tr("Delete it permanently, after asking")),
    ];
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entry, Kind, Tree};
    use std::sync::mpsc;

    struct Answer(bool, Vec<String>);

    impl Dialogs for Answer {
        fn confirm(&mut self, _title: &str, text: &str) -> bool {
            self.1.push(text.to_string());
            self.0
        }
        fn pick_folder(&mut self, _: &str) -> Option<PathBuf> {
            None
        }
        fn save_file(&mut self, _: &str, _: (&str, &str), _: Option<&Path>) -> Option<PathBuf> {
            None
        }
    }

    #[test]
    fn deleting_for_good_asks_runs_in_the_background_and_leaves_the_tree() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("rsize-delete-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("a.bin"), [0u8; 100]).unwrap();
        let dir = scan::normalize(&dir.canonicalize().unwrap());
        let dir = PathBuf::from(dir.to_string_lossy().trim_start_matches(r"\\?\"));

        let entry = |name: &str, kind, size| Entry { name: name.into(), kind, size, alloc: size, mtime: 0, flags: 0 };
        let mut tree = Tree::new(&dir.to_string_lossy(), 0);
        tree.add_children(0, &[entry("sub", Kind::Dir, 0)]);
        tree.add_children(1, &[entry("a.bin", Kind::File, 100)]);
        tree.finish();

        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let send: Sender = Arc::new(move |e| {
            let _ = tx.lock().unwrap().send(e);
        });
        let mut page = SizePage::new(Settings::default(), false, send);
        let scan = crate::scan::Scan {
            tree,
            method: crate::scan::Method::System,
            elapsed: Duration::ZERO,
            complete: true,
            admin_would_help: false,
        };
        page.app.set_scan(scan, SizeMode::Logical);
        page.app.show_drives = false;
        page.app.select(1);

        // Said no: nothing happens.
        let mut no = Answer(false, Vec::new());
        page.command(Command::DeletePermanently, &mut no);
        assert!(no.1[0].contains("sub") && page.removing.is_none() && dir.join("sub").exists());

        page.command(Command::DeletePermanently, &mut Answer(true, Vec::new()));
        assert!(page.removing.is_some() && page.app.working.is_some());
        assert!(!page.enabled(Command::Delete), "one at a time");
        let ended = rx.recv_timeout(Duration::from_secs(30)).unwrap();
        page.event(ended);
        assert!(page.removing.is_none() && page.app.working.is_none());
        assert!(!dir.join("sub").exists() && dir.exists());
        let t = &page.app.scan.as_ref().unwrap().tree;
        assert!(t.children(t.root()).is_empty(), "taken out of the tree");
        // The drive's space is asked for again.
        let space = rx.recv_timeout(Duration::from_secs(30)).unwrap();
        assert!(matches!(space, Event::Space(_, Some(_))));
    }
}
