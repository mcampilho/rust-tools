//! The uninstaller page: the programs of every source and user read on
//! threads of their own (then their icons, winget and the tools' tabs), the
//! search, the selection and the ticks, uninstalling one after another, the
//! leftovers, and the page's drawing, keyboard and mouse. The window of rinst
//! (`main.rs`) and the Uninstall Apps of rcare both hold one.
//!
//! The page does not own the window. What it needs from it (draw again,
//! show a message, rebuild the menus) it notes in `fx`; the dialogs are the
//! ones the window passes in ([`Dialogs`]); what the threads report comes
//! back as an [`Event`] through the [`Sender`]; the menus that pop up at the
//! pointer are asked for with [`Popup`] and shown by the window.

use crate::app::{App, Col, Found, Whose};
use crate::commands::{self, Command};
use crate::i18n::{tf, tr};
use crate::settings::Settings;
use crate::ui::gfx::{Gfx, Rect};
use crate::ui::theme::Theme;
use crate::ui::view::{self, Hot, Layout};
use rt_ui::page::{Dialogs, Effects, Mods};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tiny_skia::Pixmap;
use winit::event::MouseScrollDelta;
use winit::window::CursorIcon;

mod input;
mod uninstalling;

pub use uninstalling::JobMsg;

/// An icon read: the program's index, and its pixels (width, height, RGBA).
pub type Icon = (usize, u32, u32, Vec<u8>);

/// What the threads report.
pub enum Event {
    /// The programs read, for the reading of this generation.
    Read(u64, Box<Found>),
    /// Icons read (only Windows reads them yet).
    Icons(u64, Vec<Icon>),
    /// The uninstalling queue.
    Job(JobMsg),
    /// What the programs uninstalled left behind.
    Leftovers(Vec<crate::leftovers::Leftover>),
    /// How removing the leftovers ticked went (their indexes), and the
    /// `.reg` file with the keys.
    Removed(Vec<(usize, crate::leftovers::Removed)>, PathBuf),
    /// What `winget list` said (None: winget is not there, or failed).
    Winget(u64, Option<Vec<crate::winget::Row>>),
    /// The tabs of DISM and the development tools that are there.
    Tools(u64, Vec<(crate::app::Tab, Vec<crate::inventory::Program>)>),
}

/// Where the threads send their events (to the window's event loop).
pub type Sender = Arc<dyn Fn(Event) + Send + Sync>;

/// A menu that pops up at the pointer; the window shows it and runs the command chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popup {
    /// What can be done with the programs selected (`commands::program_menu`).
    Program,
    /// The search box's (`commands::search_menu`).
    Search,
}

impl Popup {
    pub fn items(self) -> Vec<commands::M> {
        match self {
            Popup::Program => commands::program_menu(),
            Popup::Search => commands::search_menu(),
        }
    }
}

pub struct InstPage {
    pub app: App,
    pub layout: Layout,
    pub hot: Hot,
    /// Dragging the scroll bar's thumb.
    pub dragging: bool,
    /// Selecting the search's text with the pointer held down.
    pub selecting: bool,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    /// Running with administrator rights: every user can be read.
    pub admin: bool,
    /// The last left click in the search (when, where), to see a double click.
    last_click: Option<(std::time::Instant, f32)>,
    generation: u64,
    /// The next reading is the first: it may come from the privileged service.
    first_load: bool,
    /// The icons read so far, by program index.
    icons: HashMap<usize, Pixmap>,
    /// winget's answer when it came before the list.
    pending_winget: Option<Option<Vec<crate::winget::Row>>>,
    /// The tools' tabs when they came before the list.
    pending_tools: Option<Vec<(crate::app::Tab, Vec<crate::inventory::Program>)>>,
    /// Uninstalling now, and, once it is done, the report waiting for the
    /// list read again.
    job: Option<uninstalling::Job>,
    report: Option<uninstalling::Job>,
    /// When the queue started, for what turns while it runs.
    job_started: Option<std::time::Instant>,
    /// Where the program keeps its data: the leftovers' `.reg` copies go in `backups` there.
    data_dir: PathBuf,
    /// The last place of the pointer, to look at again what it is on.
    pointer_at: Option<(f32, f32)>,
    scale: f32,
    mods: Mods,
    send: Sender,
}

/// Straight RGBA to a pixmap (premultiplied).
fn pixmap(w: u32, h: u32, rgba: &[u8]) -> Option<Pixmap> {
    let mut pm = Pixmap::new(w, h)?;
    for (d, s) in pm.pixels_mut().iter_mut().zip(rgba.as_chunks::<4>().0) {
        *d = tiny_skia::ColorU8::from_rgba(s[0], s[1], s[2], s[3]).premultiply();
    }
    Some(pm)
}

impl InstPage {
    /// The page with what the settings kept (whose programs, what is shown).
    pub fn new(settings: &Settings, admin: bool, data_dir: PathBuf, send: Sender) -> InstPage {
        let mut app = App::new();
        app.whose = settings.whose;
        app.show_system = settings.show_system;
        app.show_updates = settings.show_updates;
        app.sort = Col::Name;
        InstPage {
            app,
            layout: Layout::default(),
            hot: Hot::None,
            dragging: false,
            selecting: false,
            cursor_icon: CursorIcon::Default,
            fx: Effects::default(),
            admin,
            last_click: None,
            generation: 0,
            first_load: true,
            icons: HashMap::new(),
            pending_winget: None,
            pending_tools: None,
            job: None,
            report: None,
            job_started: None,
            data_dir,
            pointer_at: None,
            scale: 1.0,
            mods: Mods::default(),
            send,
        }
    }

    /// Puts what the page keeps in the settings.
    pub fn store_into(&self, s: &mut Settings) {
        s.whose = self.app.whose;
        s.show_system = self.app.show_system;
        s.show_updates = self.app.show_updates;
    }

    /// Whether nothing is going on (no uninstalling, no leftovers being
    /// looked for or removed): the list can be read again.
    pub fn idle(&self) -> bool {
        let leftovers_busy = self.app.leftovers.as_ref().is_some_and(|l| l.searching || l.removing);
        self.job.is_none() && self.app.busy.is_none() && !leftovers_busy
    }

    /// Reads the programs and their shortcuts on a thread, then their icons.
    pub fn refresh(&mut self) {
        self.generation += 1;
        let gen = self.generation;
        self.app.loading = true;
        self.pending_winget = None;
        // winget takes seconds: on a thread of its own, joined when it answers.
        let send = self.send.clone();
        std::thread::spawn(move || {
            let rows = crate::winget::run().map(|t| crate::winget::parse(&t));
            send(Event::Winget(gen, rows));
        });
        // DISM, npm and pip take a second or more too.
        self.pending_tools = None;
        let send = self.send.clone();
        let admin = self.admin;
        // The first reading may come from the privileged service, if it runs and is not elevated
        // already: a user who asks again (or uninstalls something) wants what is there now.
        let use_snapshot = std::mem::take(&mut self.first_load) && !admin;
        std::thread::spawn(move || {
            let tools = crate::tools::read_all(&crate::inventory::me(), admin);
            send(Event::Tools(gen, tools));
        });
        let send = self.send.clone();
        std::thread::spawn(move || {
            let (inv, service_age) = match use_snapshot.then(|| rt_inst::snapshot::fetch(24 * 3600)).flatten() {
                Some((mut inv, age)) => {
                    inv.me = crate::inventory::me();
                    (inv, Some(age))
                }
                None => (crate::inventory::read(), None),
            };
            let shortcuts = crate::shortcuts::read(&inv.me, &inv.other_users);
            let mut found = Found::new(inv, shortcuts);
            found.service_age = service_age;
            let scoop = crate::managers::scoop_all(&found.inv.me);
            found.scoop = (!scoop.is_empty()).then_some(scoop);
            found.chocolatey = crate::managers::choco_root().map(|r| crate::managers::choco_packages(&r));
            #[cfg(not(windows))]
            {
                found.flatpak = crate::linux_pkgs::flatpak(&found.inv.me.name);
                found.snap = crate::linux_pkgs::snap();
            }
            let wanted: Vec<IconOf> = found
                .inv
                .programs
                .iter()
                .enumerate()
                .filter_map(|(i, p)| match (&p.icon, &p.app_id) {
                    (Some((f, n)), _) => Some((i, IconFrom::File(f.clone(), *n))),
                    (None, Some(a)) => Some((i, IconFrom::App(a.clone()))),
                    _ => None,
                })
                .collect();
            send(Event::Read(gen, Box::new(found)));
            read_icons(gen, wanted, &send);
        });
        self.fx.menus();
        self.fx.redraw();
    }

    /// What a work thread reported.
    pub fn event(&mut self, event: Event) {
        match event {
            Event::Read(gen, found) if gen == self.generation => {
                self.icons.clear();
                self.app.set_found(*found);
                if let Some(rows) = self.pending_winget.take() {
                    self.app.set_winget(rows);
                }
                if let Some(tools) = self.pending_tools.take() {
                    self.app.set_tools(tools);
                }
                // The report waits for every list (an npm package may be gone).
                if self.app.found.as_ref().is_some_and(|f| f.tools_done) {
                    self.show_report();
                }
                self.fx.menus();
                self.fx.redraw();
            }
            Event::Read(..) => {}
            Event::Icons(gen, icons) if gen == self.generation => {
                for (i, w, h, px) in icons {
                    if let Some(pm) = pixmap(w, h, &px) {
                        self.icons.insert(i, pm);
                    }
                }
                self.fx.redraw();
            }
            Event::Icons(..) => {}
            Event::Winget(gen, rows) if gen == self.generation => {
                // The list may not be there yet: then it waits for it.
                if self.app.found.is_some() {
                    self.app.set_winget(rows);
                } else {
                    self.pending_winget = Some(rows);
                }
                self.fx.redraw();
            }
            Event::Winget(..) => {}
            Event::Tools(gen, tools) if gen == self.generation => {
                if self.app.found.as_ref().is_some_and(|f| !f.tools_done) && !self.app.loading {
                    self.app.set_tools(tools);
                    self.show_report();
                } else {
                    self.pending_tools = Some(tools);
                }
                self.fx.menus();
                self.fx.redraw();
            }
            Event::Tools(..) => {}
            Event::Job(m) => self.job_event(m),
            Event::Leftovers(items) => self.leftovers_found(items),
            Event::Removed(results, backup) => self.leftovers_removed(results, &backup),
        }
    }

    /// While uninstalling, the bar and the marks move, so it shows it is
    /// working even when the uninstaller shows nothing. True while it does:
    /// the window then asks to wake up in a few milliseconds.
    pub fn animate(&mut self) -> bool {
        let (Some(t0), Some(b)) = (self.job_started, &mut self.app.busy) else { return false };
        let phase = (t0.elapsed().as_secs_f32() / 1.6).fract();
        if (phase - b.phase).abs() > 0.01 {
            b.phase = phase;
            self.fx.redraw();
        }
        true
    }

    // ------------------------------------------------------------ commands

    pub fn checked(&self, c: Command) -> bool {
        match c {
            Command::Whose(i) => Whose::ALL.get(i as usize) == Some(&self.app.whose),
            Command::ShowSystem => self.app.show_system,
            Command::ShowUpdates => self.app.show_updates,
            _ => false,
        }
    }

    /// Whether the command can be used now (menus and buttons).
    pub fn enabled(&self, c: Command) -> bool {
        let sel = self.app.selected();
        let leftovers_tab = self.app.tab == crate::app::Tab::Leftovers;
        let lo_idle = self.app.leftovers.as_ref().is_some_and(|l| !l.searching && !l.removing);
        match c {
            #[cfg(windows)]
            Command::RestartAdmin => !self.admin,
            Command::Refresh => !self.app.loading && !self.busy(),
            Command::RemoveLeftovers => leftovers_tab && lo_idle && !self.app.chosen().is_empty(),
            Command::FindLeftovers => {
                !self.busy() && self.app.found.is_some() && self.app.leftovers.as_ref().is_none_or(|l| !l.searching && !l.removing)
            }
            Command::CloseLeftovers => lo_idle,
            Command::OpenLeftover | Command::OpenFolder if leftovers_tab => self.leftover_folder().is_some(),
            Command::OpenLeftover => false,
            Command::CopyDetails if leftovers_tab => !self.app.rows.is_empty(),
            Command::Uninstall | Command::UninstallQuiet if leftovers_tab => lo_idle && !self.app.chosen().is_empty(),
            Command::Modify | Command::CopyName | Command::CopyKey | Command::CopyCommand | Command::Export if leftovers_tab => false,
            Command::Uninstall | Command::UninstallQuiet => !self.busy() && !self.app.chosen().is_empty(),
            // On for one program: when it has nothing to change with, a click
            // says why (a button always off says nothing).
            Command::Modify => !self.busy() && self.app.chosen_programs().len() == 1,
            Command::TextCut | Command::TextCopy | Command::TextDelete => self.app.search.selection().is_some(),
            Command::TextPaste => rt_sys::clipboard_text().is_some_and(|t| !t.is_empty()),
            Command::TextSelectAll => !self.app.search.text.is_empty(),
            Command::SelectAll => !self.app.rows.is_empty(),
            Command::Export => !self.app.rows.is_empty(),
            Command::OpenFolder => sel.and_then(folder_of).is_some(),
            Command::CopyName | Command::CopyKey | Command::CopyDetails => sel.is_some(),
            Command::CopyCommand => sel.is_some_and(|p| p.uninstall.is_some()),
            Command::ClearSearch => !self.app.search.text.is_empty(),
            _ => true,
        }
    }

    /// Does the commands that are the page's own; false for the others
    /// (Exit, restart, Explorer menu, autostart, theme, language, help…
    /// belong to the window).
    pub fn command(&mut self, cmd: Command, dialogs: &mut dyn Dialogs) -> bool {
        // Delete on the leftovers removes them.
        let leftovers_tab = self.app.tab == crate::app::Tab::Leftovers;
        let cmd = match cmd {
            Command::Uninstall | Command::UninstallQuiet if leftovers_tab => Command::RemoveLeftovers,
            Command::OpenFolder if leftovers_tab => Command::OpenLeftover,
            c => c,
        };
        #[cfg(windows)]
        if matches!(cmd, Command::RestartAdmin | Command::ExplorerMenu) {
            return false;
        }
        if matches!(
            cmd,
            Command::Autostart
                | Command::Exit
                | Command::Theme(_)
                | Command::UiLanguage(_)
                | Command::Shortcuts
                | Command::CheckUpdates
                | Command::About
        ) {
            return false;
        }
        if !self.enabled(cmd) {
            return true;
        }
        let copy = |text: String, app: &mut App| {
            if !text.is_empty() {
                rt_sys::set_clipboard_text(&text);
                app.note = Some(tr("Copied to the clipboard."));
            }
        };
        match cmd {
            Command::Refresh => self.refresh(),
            Command::Export => self.export(dialogs),
            Command::Uninstall => self.uninstall(crate::uninstall::How::Normal, dialogs),
            Command::UninstallQuiet => self.uninstall(crate::uninstall::How::Quiet, dialogs),
            Command::Modify => self.uninstall(crate::uninstall::How::Modify, dialogs),
            Command::SelectAll => self.app.select_all(),
            Command::OpenLeftover => {
                if let Some(dir) = self.leftover_folder() {
                    open_path(&dir);
                }
            }
            Command::OpenFolder => {
                if let Some(dir) = self.app.selected().and_then(folder_of) {
                    open_path(&dir);
                }
            }
            Command::TextCut => {
                let mut cut = String::new();
                self.app.edit_search(|t| cut = t.cut());
                rt_sys::set_clipboard_text(&cut);
            }
            Command::TextCopy => {
                rt_sys::set_clipboard_text(self.app.search.selected_text());
            }
            Command::TextPaste => self.paste(),
            Command::TextDelete => self.app.edit_search(|t| t.delete(false, false)),
            Command::TextSelectAll => self.app.edit_search(|t| t.select_all()),
            Command::FindLeftovers => self.look_for_orphans(),
            Command::RemoveLeftovers => self.remove_leftovers(dialogs),
            Command::CloseLeftovers => self.app.set_leftovers(None, false),
            Command::CopyName => {
                let t = self.app.selected().map(|p| p.name.clone()).unwrap_or_default();
                copy(t, &mut self.app);
            }
            Command::CopyCommand => {
                let t = self.app.selected().and_then(|p| p.uninstall.clone()).unwrap_or_default();
                copy(t, &mut self.app);
            }
            Command::CopyKey => {
                let t = self.app.selected().map(|p| p.key.clone()).unwrap_or_default();
                copy(t, &mut self.app);
            }
            Command::CopyDetails => {
                let t = view::details_text(&self.app);
                copy(t, &mut self.app);
            }
            Command::ClearSearch => self.app.clear_search(),
            Command::Whose(i) => {
                self.app.whose = Whose::ALL[(i as usize).min(3)];
                self.app.rebuild();
            }
            Command::ShowSystem => {
                self.app.show_system = !self.app.show_system;
                self.app.rebuild();
            }
            Command::ShowUpdates => {
                self.app.show_updates = !self.app.show_updates;
                self.app.rebuild();
            }
            _ => {}
        }
        self.fx.menus();
        self.fx.redraw();
        true
    }

    /// Writes the list shown to a CSV file the user chooses.
    fn export(&mut self, dialogs: &mut dyn Dialogs) {
        let Some(path) = dialogs.save_file("rinst-programs.csv", (&tr("CSV files"), "*.csv"), None) else { return };
        let header: Vec<String> = [
            "Name",
            "Publisher",
            "Version",
            "Size",
            "Installed",
            "For",
            "Source",
            "winget id",
            "Newer version (winget)",
            "Install folder",
            "Uninstall command",
            "Registry key",
        ]
        .iter()
        .map(|h| tr(h))
        .collect();
        let csv = self.app.csv(&header, &view::scope_text, &view::sources_text);
        self.app.note = Some(match std::fs::write(&path, csv) {
            Ok(()) => tf("The list was saved to {}.", &[&path.display().to_string()]),
            Err(e) => tf("Could not save {}: {}", &[&path.display().to_string(), &e.to_string()]),
        });
        self.fx.redraw();
    }

    // ------------------------------------------------------------ drawing

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32, menu_bar: bool) {
        self.scale = s;
        self.layout = view::layout(g, &self.app, area, s, menu_bar, self.admin);
        self.app.page = self.layout.page_rows();
        // Which buttons work, asked before drawing borrows the canvas.
        let on: Vec<(Command, bool)> = commands::actions(self.app.tab).iter().map(|&c| (c, self.enabled(c))).collect();
        let enabled = |c: Command| on.iter().any(|&(x, e)| x == c && e);
        view::draw(g, &self.app, &self.layout, th, s, self.hot, &self.icons, &enabled);
    }

    /// The mouse wheel, with the pointer at `at`.
    pub fn wheel(&mut self, delta: MouseScrollDelta, at: Option<(f32, f32)>) {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, l) => -l * 3.0,
            MouseScrollDelta::PixelDelta(p) => -p.y as f32 / self.layout.row_h.max(20.0),
        };
        let s = self.scale;
        let over_details = at.is_some_and(|(x, y)| self.layout.details.contains(x, y));
        if over_details {
            let max = (view::details_height(&self.app, s) - self.layout.details.h).max(0.0);
            self.app.details_scroll = (self.app.details_scroll + lines * 18.0 * s).clamp(0.0, max);
        } else {
            self.app.scroll_by(lines.round() as isize);
        }
        self.fx.redraw();
        if let Some((x, y)) = at {
            self.pointer(x, y, false);
        }
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
        let mut list = Node::new(Role::List);
        list.set_label(tr("Programs"));
        let mut rows = Vec::new();
        let end = (self.app.scroll + self.layout.page_rows()).min(self.app.rows.len());
        for i in self.app.scroll..end {
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::ListItem);
            let p = self.app.program(self.app.rows[i]);
            n.set_label(p.map_or(String::new(), |p| format!("{} {} {}", p.name, p.version, p.publisher)));
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

/// Where a program's icon comes from: a file and an index, or an app of a package.
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(not(windows), allow(dead_code))]
enum IconFrom {
    File(PathBuf, i32),
    App(String),
}

type IconOf = (usize, IconFrom);

/// Reads the icons on this thread and sends them in batches.
fn read_icons(gen: u64, wanted: Vec<IconOf>, send: &Sender) {
    #[cfg(windows)]
    {
        type Pixels = Option<(u32, u32, Vec<u8>)>;
        let mut cache: HashMap<IconFrom, Pixels> = HashMap::new();
        let mut batch = Vec::new();
        for (i, from) in wanted {
            let got = cache
                .entry(from.clone())
                .or_insert_with(|| match &from {
                    IconFrom::File(f, n) => rt_sys::win::icon::extract(f, *n),
                    IconFrom::App(a) => crate::appx::icon(a, 32),
                })
                .clone();
            if let Some((w, h, px)) = got {
                batch.push((i, w, h, px));
            }
            if batch.len() >= 16 {
                send(Event::Icons(gen, std::mem::take(&mut batch)));
            }
        }
        if !batch.is_empty() {
            send(Event::Icons(gen, batch));
        }
    }
    #[cfg(not(windows))]
    let _ = (gen, wanted, send);
}

/// The folder of a program: its install folder, or its uninstaller's.
fn folder_of(p: &crate::inventory::Program) -> Option<PathBuf> {
    p.location.clone().filter(|l| l.is_dir()).or_else(|| {
        p.uninstall
            .as_deref()
            .and_then(crate::inventory::command_file)
            .and_then(|f| f.parent().map(Path::to_path_buf))
            .filter(|d| d.is_absolute() && d.is_dir())
    })
}

/// Opens a folder in the file manager.
fn open_path(p: &Path) {
    #[cfg(windows)]
    rt_sys::win::shell_open(&p.to_string_lossy());
    #[cfg(not(windows))]
    rt_sys::linux::xdg_open(p.as_os_str().to_os_string());
}

/// The keys of the page, for the Keyboard shortcuts panel.
pub fn shortcuts_text() -> String {
    let rows = [
        (tr("Letters"), tr("Find a program by its name, publisher or folder")),
        ("Esc".to_string(), tr("Clear the search")),
        ("↑ ↓  PgUp PgDn  Home End".to_string(), tr("Move in the list")),
        ("Delete".to_string(), tr("Uninstall the programs selected")),
        ("Shift+Delete".to_string(), tr("Uninstall them quietly")),
        ("Ctrl+A".to_string(), tr("Select every program shown")),
        (tr("Ctrl+click, Shift+click"), tr("Select several programs")),
        (tr("Ctrl+V"), tr("Paste into the search")),
        (tr("Space"), tr("Tick the programs selected: the buttons act on those ticked, whatever the search shows")),
        ("F5".to_string(), tr("Read the programs again")),
        ("Ctrl+E".to_string(), tr("Open the install folder")),
        ("Ctrl+C".to_string(), tr("Copy the name")),
        ("Ctrl+Shift+C".to_string(), tr("Copy all the details")),
        ("Ctrl+S".to_string(), tr("Save the list as CSV")),
    ];
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
}
