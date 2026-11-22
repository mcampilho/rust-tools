//! The cleaning page: what is cleaned, analysing and cleaning on threads of
//! their own, and the page's drawing, keyboard and mouse. The window of
//! rclean (`main.rs`) and the Disk Cleaner of rcare both hold one, so the
//! work is the same in both.
//!
//! The page does not own the window. What it needs from it (draw again,
//! show a message, rebuild the menus) it notes in `fx`; asking the user goes
//! through the `Dialogs` the window passes in; what the threads report comes back as an
//! [`Event`] through the [`Sender`] the page was made with.

use crate::app::{App, Busy, Row, State};
use crate::clean::{self, Cleaned, Found};
use crate::commands::{self, Command};
use crate::format;
use crate::i18n::{tf, tr};
use crate::ui::gfx::{Gfx, Rect};
use crate::ui::theme::Theme;
use crate::ui::view::{self, Hot, Layout};
use rt_ui::page::{Dialogs, Effects};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use winit::event::MouseScrollDelta;
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::CursorIcon;

/// How many targets are looked at at the same time.
const ANALYSE_THREADS: usize = 4;

/// What the work threads report.
pub enum Event {
    /// The work of this generation starts on an item.
    Starting(u64, usize),
    /// An item looked at, and whether its program is open.
    Analysed(u64, usize, Box<Found>, bool),
    Cleaned(u64, usize, Cleaned),
    /// The work of this generation is done (true: it was cleaning).
    Finished(u64, bool),
}

/// Where the threads send their events (to the window's event loop).
pub type Sender = Arc<dyn Fn(Event) + Send + Sync>;

pub struct CleanPage {
    pub app: App,
    pub layout: Layout,
    pub hot: Hot,
    pub dragging: bool,
    pub cursor_icon: CursorIcon,
    /// Running with administrator rights: Windows' own folders can be cleaned.
    pub admin: bool,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    generation: u64,
    /// Asks the work under way to stop after the item now.
    stop: Arc<AtomicBool>,
    /// When the work started, for what turns while it runs.
    started: Option<std::time::Instant>,
    /// Where this cleaning saved registry entries.
    backups: Option<PathBuf>,
    /// Where the program keeps its data: the registry copies go in `backups` there.
    data_dir: PathBuf,
    /// The last place of the pointer, to look at again what it is on.
    pointer_at: Option<(f32, f32)>,
    send: Sender,
}

impl CleanPage {
    pub fn new(app: App, admin: bool, data_dir: PathBuf, send: Sender) -> CleanPage {
        CleanPage {
            app,
            layout: Layout::default(),
            hot: Hot::None,
            dragging: false,
            cursor_icon: CursorIcon::Default,
            admin,
            fx: Effects::default(),
            generation: 0,
            stop: Arc::new(AtomicBool::new(false)),
            started: None,
            backups: None,
            data_dir,
            pointer_at: None,
            send,
        }
    }

    // ------------------------------------------------------------ work

    /// Looks at what every target holds, on a thread.
    pub fn analyse(&mut self) {
        if self.app.busy.is_some() {
            return;
        }
        self.generation += 1;
        let gen = self.generation;
        self.stop = Arc::new(AtomicBool::new(false));
        let work: Vec<(usize, crate::targets::Target)> =
            self.app.items.iter().enumerate().filter(|(_, it)| it.usable()).map(|(i, it)| (i, it.target.clone())).collect();
        for &(i, _) in &work {
            self.app.items[i].state = State::Analysing;
        }
        self.start_busy(false, work.len());
        let (send, stop) = (self.send.clone(), self.stop.clone());
        std::thread::spawn(move || {
            let running = Arc::new(crate::procs::running());
            // The targets are independent (their own folders, or their own program): a few at a
            // time, because most of the time goes to waiting for the disk and for commands.
            let workers = work.len().clamp(1, ANALYSE_THREADS);
            let queue = Arc::new(std::sync::Mutex::new(work.into_iter()));
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    let (queue, running, send, stop) = (queue.clone(), running.clone(), send.clone(), stop.clone());
                    std::thread::spawn(move || loop {
                        let next = queue.lock().ok().and_then(|mut q| q.next());
                        let Some((i, t)) = next else { break };
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        send(Event::Starting(gen, i));
                        let found = clean::analyse(&t, &stop);
                        let open = crate::procs::any_running(&running, t.process);
                        send(Event::Analysed(gen, i, Box::new(found), open));
                    })
                })
                .collect();
            for h in handles {
                let _ = h.join();
            }
            send(Event::Finished(gen, false));
        });
    }

    /// Asks, then deletes what the ticked targets hold, on a thread.
    pub fn clean(&mut self, dialogs: &mut dyn Dialogs) {
        if self.app.busy.is_some() {
            return;
        }
        let work: Vec<(usize, crate::targets::Target, Found)> = self
            .app
            .chosen()
            .into_iter()
            .filter_map(|i| {
                self.app.items[i].found().filter(|f| f.count() > 0 || f.unknown).map(|f| (i, self.app.items[i].target.clone(), f.clone()))
            })
            .collect();
        if work.is_empty() {
            return;
        }
        let (n, bytes) = work.iter().fold((0, 0), |(n, b), (_, _, f)| (n + f.count(), b + f.bytes));
        let shortcuts = work.iter().any(|(_, t, _)| t.kind == crate::targets::Kind::Shortcuts);
        let mut text = if shortcuts && work.iter().all(|(_, t, _)| t.kind == crate::targets::Kind::Shortcuts) {
            tf("{} broken shortcuts will go to the Recycle Bin:", &[&format::count(n)])
        } else if shortcuts {
            tf(
                "{} in {} files will be cleaned (broken shortcuts to the Recycle Bin, the rest for good):",
                &[&format::size(bytes), &format::count(n)],
            )
        } else {
            tf("{} in {} files will be deleted for good:", &[&format::size(bytes), &format::count(n)])
        };
        text += "\n\n";
        text += &work
            .iter()
            .map(|(i, t, f)| {
                let size = if f.unknown { "?".to_string() } else { format::size(f.bytes) };
                format!("  {} — {}: {size}", view::group_label(t.group), tr(t.name)) + if self.app.items[*i].open { " *" } else { "" }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if work.iter().any(|(i, _, _)| self.app.items[*i].open) {
            text += "\n\n";
            text += &tr("* Open now: close it first to clean everything; files in use are left as they are.");
        }
        if work.iter().any(|(_, t, _)| t.kind == crate::targets::Kind::Registry) {
            text += "\n\n";
            text += &tr("Registry entries are saved to a .reg file first, to put them back if need be.");
        }
        if !dialogs.confirm(&tr("Clean"), &text) {
            return;
        }
        self.generation += 1;
        let gen = self.generation;
        self.stop = Arc::new(AtomicBool::new(false));
        self.app.freed = None;
        for (i, _, f) in &work {
            self.app.items[*i].state = State::Cleaning(f.clone());
        }
        self.start_busy(true, work.len());
        // Registry entries are saved first: `backups\registry <date>\<id>.reg`
        // beside the settings.
        let stamp = {
            let t = rt_sys::local_time();
            format!("{:04}-{:02}-{:02} {:02}{:02}{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
        };
        let backups = self.data_dir.join("backups").join(format!("registry {stamp}"));
        self.backups = work.iter().any(|(_, t, _)| t.kind == crate::targets::Kind::Registry).then(|| backups.clone());
        let (send, stop) = (self.send.clone(), self.stop.clone());
        std::thread::spawn(move || {
            for (i, t, f) in work {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                send(Event::Starting(gen, i));
                let c = clean::clean(&t, &f, &stop, &backups);
                send(Event::Cleaned(gen, i, c));
            }
            send(Event::Finished(gen, true));
        });
    }

    fn start_busy(&mut self, cleaning: bool, total: usize) {
        self.app.busy = Some(Busy { cleaning, total, ..Busy::default() });
        self.started = Some(std::time::Instant::now());
        self.fx.menus();
        self.fx.redraw();
    }

    fn stop_work(&mut self) {
        if let Some(b) = &mut self.app.busy {
            b.stopping = true;
            self.stop.store(true, Ordering::Relaxed);
            self.fx.redraw();
        }
    }

    /// Asks the work under way to stop (the window is closing).
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// The work is done: what cleaning freed, then a new look.
    fn finished(&mut self, cleaning: bool) {
        self.app.busy = None;
        self.started = None;
        // What was not reached goes back to how it was.
        for it in &mut self.app.items {
            match &it.state {
                State::Analysing => it.state = State::Waiting,
                State::Cleaning(f) => it.state = State::Found(f.clone()),
                _ => {}
            }
        }
        if cleaning {
            let c = self.app.freed.clone().unwrap_or_default();
            let mut text = tf("{} freed: {} files deleted.", &[&format::size(c.bytes), &format::count(c.files)]);
            if c.failed > 0 {
                text += "\n\n";
                text += &tf("{} files were in use or could not be deleted, and stayed.", &[&format::count(c.failed)]);
            }
            if let Some(dir) = self.backups.take().filter(|d| d.is_dir()) {
                text += "\n\n";
                text += &tf(
                    "The registry entries were saved in {} first: open the .reg files there to put them back.",
                    &[&dir.display().to_string()],
                );
            }
            // What a package manager's command said when it did not work.
            let errors: Vec<String> = self
                .app
                .items
                .iter()
                .filter_map(|it| match &it.state {
                    State::Cleaned(Cleaned { error: Some(e), .. }) => {
                        Some(format!("  {} — {}: {e}", view::group_label(it.target.group), tr(it.target.name)))
                    }
                    _ => None,
                })
                .collect();
            if !errors.is_empty() {
                text += "\n\n";
                text += &tf("Did not work ({}):", &[&errors.len().to_string()]);
                text += "\n";
                text += &errors.join("\n");
            }
            self.fx.info(&tr("Clean"), &text);
            self.analyse();
        }
        self.fx.menus();
        self.update_pointer();
        self.fx.redraw();
    }

    /// What a work thread reported.
    pub fn event(&mut self, event: Event) {
        match event {
            Event::Starting(gen, i) if gen == self.generation => {
                let name = self
                    .app
                    .items
                    .get(i)
                    .map(|it| format!("{} — {}", view::group_label(it.target.group), tr(it.target.name)))
                    .unwrap_or_default();
                if let Some(b) = &mut self.app.busy {
                    b.name = name;
                }
                self.fx.redraw();
            }
            Event::Analysed(gen, i, found, open) if gen == self.generation => {
                if let Some(it) = self.app.items.get_mut(i) {
                    it.open = open;
                }
                self.app.set_found(i, *found);
                self.fx.menus();
                self.fx.redraw();
            }
            Event::Cleaned(gen, i, c) if gen == self.generation => {
                self.app.set_cleaned(i, c);
                self.fx.redraw();
            }
            Event::Finished(gen, cleaning) if gen == self.generation => self.finished(cleaning),
            Event::Starting(..) | Event::Analysed(..) | Event::Cleaned(..) | Event::Finished(..) => {}
        }
    }

    /// While working, the arc turns, so it shows it is busy. True while it
    /// does: the window then asks to wake up in a few milliseconds.
    pub fn animate(&mut self) -> bool {
        let (Some(t0), Some(b)) = (self.started, &mut self.app.busy) else { return false };
        let phase = (t0.elapsed().as_secs_f32() / 1.6).fract();
        if (phase - b.phase).abs() > 0.01 {
            b.phase = phase;
            self.fx.redraw();
        }
        true
    }

    // ------------------------------------------------------------ commands

    /// Whether the command can be used now (menus and buttons).
    pub fn enabled(&self, c: Command) -> bool {
        let idle = self.app.busy.is_none();
        match c {
            Command::Analyse => idle,
            Command::Clean => idle && self.app.anything_to_clean(),
            Command::Stop => self.app.busy.as_ref().is_some_and(|b| !b.stopping),
            #[cfg(windows)]
            Command::RestartAdmin => !self.admin && idle,
            Command::TickAll | Command::UntickAll | Command::TickDefaults => idle,
            Command::OpenFolder => self.folder().is_some(),
            Command::CopyDetails => self.app.sel.is_some(),
            _ => true,
        }
    }

    /// Does the commands that are the page's own; false for the others
    /// (Exit, theme, language, help… belong to the window).
    pub fn command(&mut self, cmd: Command, dialogs: &mut dyn Dialogs) -> bool {
        if !matches!(
            cmd,
            Command::Analyse
                | Command::Clean
                | Command::Stop
                | Command::TickAll
                | Command::UntickAll
                | Command::TickDefaults
                | Command::OpenFolder
                | Command::CopyDetails
        ) {
            return false;
        }
        if !self.enabled(cmd) {
            return true;
        }
        match cmd {
            Command::Analyse => self.analyse(),
            Command::Clean => self.clean(dialogs),
            Command::Stop => self.stop_work(),
            Command::TickAll => self.app.tick_all(true),
            Command::UntickAll => self.app.tick_all(false),
            Command::TickDefaults => self.app.tick_defaults(),
            Command::OpenFolder => {
                if let Some(dir) = self.folder() {
                    open_path(&dir);
                }
            }
            Command::CopyDetails => {
                rt_sys::set_clipboard_text(&view::details_text(&self.app));
                self.app.note = Some(tr("Copied to the clipboard."));
            }
            _ => {}
        }
        self.fx.menus();
        self.fx.redraw();
        true
    }

    /// The first folder of the target selected that is there.
    fn folder(&self) -> Option<PathBuf> {
        self.app.selected_item()?.target.places.iter().map(|p| p.dir.clone()).find(|d| d.is_dir())
    }

    // ------------------------------------------------------------ drawing and input

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32, menu_bar: bool) {
        self.layout = view::layout(g, &self.app, area, s, menu_bar);
        self.app.page = self.layout.page_rows();
        // Which buttons work, asked before drawing borrows the canvas.
        let on: Vec<(Command, bool)> = commands::ACTIONS.iter().map(|&c| (c, self.enabled(c))).collect();
        let enabled = |c: Command| on.iter().any(|&(x, e)| x == c && e);
        view::draw(g, &self.app, &self.layout, th, s, self.hot, &enabled);
    }

    /// A key: the command of a chord (for the window to run), or the arrows and Space in the list.
    /// `logical` is the key without the modifiers when Ctrl or Alt are down.
    pub fn key(&mut self, ctrl: bool, shift: bool, alt: bool, logical: WKey) -> Option<Command> {
        let name = match &logical {
            WKey::Character(c) if ctrl || alt => c.to_lowercase(),
            WKey::Named(n) => match n {
                NamedKey::F1 => "f1".into(),
                NamedKey::F5 => "f5".into(),
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
        let page = self.layout.page_rows() as isize;
        match logical {
            WKey::Named(NamedKey::Space) if !ctrl && !alt => {
                if let Some(r) = self.app.sel {
                    self.app.toggle_row(r);
                }
            }
            WKey::Named(NamedKey::ArrowDown) => self.app.move_sel(1),
            WKey::Named(NamedKey::ArrowUp) => self.app.move_sel(-1),
            WKey::Named(NamedKey::PageDown) => self.app.move_sel(page),
            WKey::Named(NamedKey::PageUp) => self.app.move_sel(-page),
            WKey::Named(NamedKey::Home) => self.app.move_sel(isize::MIN / 2),
            WKey::Named(NamedKey::End) => self.app.move_sel(isize::MAX / 2),
            _ => return None,
        }
        self.app.note = None;
        self.fx.menus();
        self.fx.redraw();
        None
    }

    /// What the pointer is on; `blocked` while a message is open.
    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        self.pointer_at = Some((x, y));
        let l = &self.layout;
        let hot = if blocked {
            Hot::None
        } else if let Some(i) = l.actions.iter().position(|r| r.w > 0.0 && r.contains(x, y)) {
            Hot::Action(i)
        } else if l.stop.w > 0.0 && l.stop.contains(x, y) && self.app.busy.as_ref().is_some_and(|b| !b.stopping) {
            Hot::Stop
        } else if l.banner.h > 0.0 && l.restart.contains(x, y) {
            Hot::Restart
        } else if l.details.contains(x, y) {
            Hot::Details
        } else if let Some(r) = l.row_at(&self.app, x, y) {
            let (x0, x1) = l.check_area(&self.app, r);
            if x >= x0 && x < x1 && self.app.busy.is_none() {
                Hot::Check(r)
            } else {
                Hot::Row(r)
            }
        } else {
            Hot::None
        };
        if hot != self.hot {
            self.hot = hot;
            self.fx.redraw();
        }
        self.cursor_icon = match hot {
            h if h.clickable() => CursorIcon::Pointer,
            // Working, but the window still answers.
            _ if self.app.busy.is_some() => CursorIcon::Progress,
            _ => CursorIcon::Default,
        };
    }

    /// The pointer left the page.
    pub fn pointer_left(&mut self) {
        self.pointer_at = None;
        if !self.dragging && self.hot != Hot::None {
            self.hot = Hot::None;
            self.fx.redraw();
        }
    }

    /// The pointer's shape follows whether it is working.
    pub fn update_pointer(&mut self) {
        if let Some((x, y)) = self.pointer_at {
            self.pointer(x, y, false);
        }
    }

    /// A click at (x, y): the command of a button, for the window to run.
    pub fn press(&mut self, x: f32, y: f32) -> Option<Command> {
        self.app.note = None;
        match self.hot {
            Hot::Action(i) => return Some(commands::ACTIONS[i]),
            Hot::Stop => return Some(Command::Stop),
            #[cfg(windows)]
            Hot::Restart => return Some(Command::RestartAdmin),
            Hot::Check(r) => {
                self.app.select(r);
                self.app.toggle_row(r);
            }
            Hot::Row(r) => self.app.select(r),
            Hot::None => {
                let l = &self.layout;
                if l.track.contains(x, y) && self.app.rows.len() > l.page_rows() {
                    self.dragging = true;
                    self.drag_to(y);
                }
            }
            _ => {}
        }
        self.fx.menus();
        self.fx.redraw();
        None
    }

    pub fn release(&mut self) {
        self.dragging = false;
    }

    pub fn drag_to(&mut self, y: f32) {
        let page = self.layout.page_rows();
        self.app.scroll = view::scroll_for(&self.layout, self.app.rows.len(), page, y);
        self.fx.redraw();
    }

    /// The mouse wheel, with the pointer at `at`.
    pub fn wheel(&mut self, g: &mut Gfx, s: f32, delta: MouseScrollDelta, at: Option<(f32, f32)>) {
        let lines = match delta {
            MouseScrollDelta::LineDelta(_, l) => -l * 3.0,
            MouseScrollDelta::PixelDelta(p) => -p.y as f32 / self.layout.row_h.max(20.0),
        };
        let over_details = at.is_some_and(|(x, y)| self.layout.details.contains(x, y));
        if over_details {
            let max = (view::details_height(g, &self.app, &self.layout, s) - self.layout.details.h).max(0.0);
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
        use accesskit::{Node, NodeId, Role, Toggled};
        let mut focus = None;
        let mut list = Node::new(Role::List);
        list.set_label(tr("What"));
        let mut rows = Vec::new();
        let end = (self.app.scroll + self.layout.page_rows()).min(self.app.rows.len());
        for i in self.app.scroll..end {
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::CheckBox);
            let (label, on) = match self.app.rows[i] {
                Row::Group(g) => (view::group_label(self.app.groups[g]), self.app.group_tick(g).1),
                Row::Item(k) => {
                    let it = &self.app.items[k];
                    let size = it.found().map_or(String::new(), |f| format!(", {}", format::size(f.bytes)));
                    (format!("{}{size}. {}", tr(it.target.name), view::note_text(it)), it.ticked)
                }
            };
            n.set_label(label);
            n.set_toggled(if on { Toggled::True } else { Toggled::False });
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
        let (left, right) = view::status_text(&self.app);
        let mut st = Node::new(Role::Label);
        st.set_value(format!("{left} {right}"));
        nodes.push((NodeId(3), st));
        children.push(NodeId(3));
        focus
    }

    /// A screen reader clicked the row `i`.
    pub fn accessibility_click(&mut self, i: usize) {
        self.app.select(i);
        self.app.toggle_row(i);
        self.fx.redraw();
    }
}

/// Opens a folder in the file manager.
fn open_path(p: &std::path::Path) {
    #[cfg(windows)]
    rt_sys::win::shell_open(&p.to_string_lossy());
    #[cfg(not(windows))]
    rt_sys::linux::xdg_open(p.as_os_str().to_os_string());
}

/// The keys of the page, for the Keyboard shortcuts panel.
pub fn shortcuts_text() -> String {
    let rows = [
        ("F5".to_string(), tr("Analyze again")),
        ("Ctrl+L".to_string(), tr("Clean what is ticked")),
        ("Esc".to_string(), tr("Stop")),
        ("↑ ↓  PgUp PgDn  Home End".to_string(), tr("Move in the list")),
        (tr("Space"), tr("Tick or untick the row selected")),
        ("Ctrl+A".to_string(), tr("Tick everything")),
        ("Ctrl+E".to_string(), tr("Open the folder")),
        ("Ctrl+Shift+C".to_string(), tr("Copy the details")),
    ];
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
}
