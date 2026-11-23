//! The Repair page: Windows' own repair tools (`repair`) — SFC, DISM, a
//! chkdsk scan of each NTFS drive, the Start menu — one row each with what it
//! does and how it ended last; on the right what the tool writes, as it
//! comes, with its progress. One at a time; administrator rights.

use crate::i18n::{tf, tr};
use crate::repair::{self, CheckResult, Event, Output, Task};
use rt_gfx::gfx::{Align, Color, FontId, Gfx, Rect};
use rt_ui::page::{Dialogs, Effects};
use rt_ui::theme::Theme;
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use winit::window::CursorIcon;

/// What the page does (its buttons and menu entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Run,
    Fix,
    Copy,
    Log,
}

pub const ACTIONS: [Action; 4] = [Action::Run, Action::Fix, Action::Copy, Action::Log];

impl Action {
    /// The menu label (a translation key with its access key).
    pub fn label_key(self) -> &'static str {
        match self {
            Action::Run => "&Run",
            Action::Fix => "&Fix Errors...",
            Action::Copy => "&Copy the Output",
            Action::Log => "Open the &Log",
        }
    }
}

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Act(usize),
    Row(usize),
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub actions: [Rect; 4],
    pub list: Rect,
    pub output: Rect,
    pub row_h: f32,
    pub status: Rect,
}

#[derive(Default)]
pub struct RepairPage {
    /// The NTFS drives (None: not known yet; the system drive stands in).
    pub drives: Option<Vec<char>>,
    pub sel: usize,
    /// The first row shown.
    pub scroll: usize,
    pub admin: bool,
    /// What the last (or running) tool wrote, and which it was.
    pub output: Output,
    pub shown: Option<Task>,
    /// How each tool ended last.
    pub results: HashMap<Task, Result<i32, String>>,
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    pub note: Option<String>,
    running: Option<(Task, Receiver<Event>)>,
    reading: Option<Receiver<Option<Vec<char>>>>,
}

/// The system drive's letter.
fn system_drive() -> char {
    std::env::var("SystemDrive").ok().and_then(|d| d.chars().next()).unwrap_or('C').to_ascii_uppercase()
}

impl RepairPage {
    pub fn new(admin: bool) -> RepairPage {
        RepairPage { admin, ..RepairPage::default() }
    }

    /// The rows: SFC, DISM, a check per drive, the Start menu.
    pub fn tasks(&self) -> Vec<Task> {
        let mut out = vec![Task::Sfc, Task::Dism];
        let drives = self.drives.clone().unwrap_or_else(|| vec![system_drive()]);
        out.extend(drives.into_iter().map(Task::Check));
        out.push(Task::StartMenu);
        out
    }

    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// Asks Windows which drives are NTFS (chkdsk /scan needs it), in a thread.
    pub fn refresh(&mut self) {
        if self.reading.is_some() {
            return;
        }
        let (send, recv) = channel();
        std::thread::spawn(move || {
            let v = crate::disks::read()
                .map(|v| v.into_iter().filter(|v| v.file_system.eq_ignore_ascii_case("NTFS")).map(|v| v.letter).collect::<Vec<char>>());
            let _ = send.send(v);
        });
        self.reading = Some(recv);
    }

    /// While a tool runs (or the drives are read): takes what came, and when to look again.
    pub fn animate(&mut self) -> Option<Instant> {
        if let Some(r) = &self.reading {
            match r.try_recv() {
                Ok(v) => {
                    if let Some(mut v) = v.filter(|v| !v.is_empty()) {
                        v.sort_unstable();
                        self.drives = Some(v);
                    }
                    self.reading = None;
                    self.fx.redraw();
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.reading = None,
                Err(_) => {}
            }
        }
        if let Some((task, r)) = &self.running {
            let task = task.clone();
            let mut changed = false;
            loop {
                match r.try_recv() {
                    Ok(Event::Text(t)) => {
                        self.output.feed(&t);
                        changed = true;
                    }
                    Ok(Event::Done(code)) => {
                        self.note = Some(match &code {
                            Ok(_) => tf("{}: finished.", &[&self.name(&task)]),
                            Err(why) => tf("{} did not start: {}", &[&self.name(&task), why]),
                        });
                        self.results.insert(task.clone(), code);
                        self.running = None;
                        self.fx.menus();
                        changed = true;
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.running = None;
                        self.fx.menus();
                        changed = true;
                        break;
                    }
                }
            }
            if changed {
                self.fx.redraw();
            }
        }
        (self.running.is_some() || self.reading.is_some()).then(|| Instant::now() + Duration::from_millis(120))
    }

    /// The name of a row (translated).
    pub fn name(&self, t: &Task) -> String {
        match t {
            Task::Sfc => tr("System files (SFC)"),
            Task::Dism => tr("Windows image (DISM)"),
            Task::Check(d) | Task::Fix(d) => tf("Drive {}: (chkdsk)", &[&d.to_string()]),
            Task::StartMenu => tr("Start menu"),
        }
    }

    fn about(t: &Task) -> String {
        tr(match t {
            Task::Sfc => "Checks Windows' files and puts back those that are damaged. Takes 5 to 20 minutes.",
            Task::Dism => {
                "Repairs the store SFC takes good files from, through Windows Update. When SFC cannot fix something; 20 minutes or more."
            }
            Task::Check(_) | Task::Fix(_) => {
                "Looks for file system errors while the drive is in use. Errors found are repaired with Fix Errors."
            }
            Task::StartMenu => "Restarts Start and Search and registers Windows' own Start menu again. Nothing is deleted.",
        })
    }

    /// How a row ended last (a translated line, and whether it is good).
    fn result_text(&self, t: &Task) -> (String, Option<bool>) {
        if self.running.as_ref().is_some_and(|(r, _)| r == t || matches!((r, t), (Task::Fix(a), Task::Check(b)) if a == b)) {
            return (tr("Running…"), None);
        }
        let fix = match t {
            Task::Check(d) => self.results.get(&Task::Fix(*d)),
            _ => None,
        };
        if let Some(code) = fix {
            return match code {
                Ok(0) => (tr("Errors repaired, or scheduled for the next start."), Some(true)),
                Ok(n) => (tf("Ended with code {}.", &[&n.to_string()]), Some(false)),
                Err(e) => (e.clone(), Some(false)),
            };
        }
        match (t, self.results.get(t)) {
            (_, None) => (tr("Not run yet."), None),
            (_, Some(Err(e))) => (e.clone(), Some(false)),
            (Task::Check(_), Some(Ok(code))) => match repair::check_result(*code) {
                CheckResult::Clean => (tr("No errors."), Some(true)),
                CheckResult::Fixed => (tr("Errors were fixed."), Some(true)),
                CheckResult::NeedsFix => (tr("Errors found: Fix Errors repairs them."), Some(false)),
                CheckResult::Other(n) => (tf("Ended with code {}.", &[&n.to_string()]), Some(false)),
            },
            (_, Some(Ok(0))) => (tr("Finished."), Some(true)),
            (_, Some(Ok(n))) => (tf("Ended with code {}.", &[&n.to_string()]), Some(false)),
        }
    }

    fn selected(&self) -> Option<Task> {
        self.tasks().get(self.sel).cloned()
    }

    pub fn enabled(&self, a: Action) -> bool {
        match a {
            Action::Run => self.admin && !self.busy() && self.selected().is_some(),
            Action::Fix => {
                self.admin
                    && !self.busy()
                    && matches!(self.selected(), Some(Task::Check(d))
                        if matches!(self.results.get(&Task::Check(d)), Some(Ok(c)) if repair::check_result(*c) == CheckResult::NeedsFix))
            }
            Action::Copy => !self.output.all().is_empty(),
            Action::Log => self.log_file().is_some_and(|f| f.exists()),
        }
    }

    /// The log Windows keeps of the selected tool (SFC and DISM).
    fn log_file(&self) -> Option<std::path::PathBuf> {
        let win = std::path::PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()));
        match self.selected()? {
            Task::Sfc => Some(win.join("Logs\\CBS\\CBS.log")),
            Task::Dism => Some(win.join("Logs\\DISM\\dism.log")),
            _ => None,
        }
    }

    fn start(&mut self, task: Task) {
        let (send, recv) = channel();
        let send = std::sync::Mutex::new(send);
        repair::run(&task, move |e| {
            if let Ok(s) = send.lock() {
                let _ = s.send(e);
            }
        });
        self.output = Output::default();
        self.output.feed(&format!("> {}\n\n", task.command_line()));
        self.shown = Some(task.clone());
        self.results.remove(&task);
        if let Task::Check(d) = task {
            self.results.remove(&Task::Fix(d));
        }
        self.running = Some((task, recv));
        self.note = None;
    }

    /// Does what a button or its menu entry asks.
    pub fn act(&mut self, a: Action, dialogs: &mut dyn Dialogs) {
        if !self.enabled(a) {
            if matches!(a, Action::Run | Action::Fix) && !self.admin {
                self.fx.info(
                    &tr("Repair"),
                    &tr("Windows' repair tools need administrator rights: restart rcare as administrator (File menu)."),
                );
            }
            return;
        }
        match a {
            Action::Run => {
                let Some(t) = self.selected() else { return };
                if t == Task::StartMenu
                    && !dialogs.confirm(&tr("Repair"), &tr("Start and Search close for a moment and come back on their own. Go on?"))
                {
                    return;
                }
                self.start(t);
            }
            Action::Fix => {
                let Some(Task::Check(d)) = self.selected() else { return };
                let text = if d == system_drive() {
                    tf("Drive {}: is the system's: Windows repairs it at the next start, in a few seconds. Schedule it?", &[&d.to_string()])
                } else {
                    tf(
                        "Drive {}: goes offline for a moment while it is repaired: programs using it may be interrupted. Go on?",
                        &[&d.to_string()],
                    )
                };
                if dialogs.confirm(&tr("Repair"), &text) {
                    self.start(Task::Fix(d));
                }
            }
            Action::Copy => {
                let text = self.output.all().join("\r\n");
                #[cfg(windows)]
                let ok = rt_sys::win::clipboard::set_text(&text);
                #[cfg(not(windows))]
                let ok = {
                    let _ = &text;
                    false
                };
                self.note = Some(if ok { tr("Copied to the clipboard.") } else { tr("The clipboard could not be used.") });
            }
            Action::Log => {
                if let Some(f) = self.log_file() {
                    rt_sys::reveal(&f);
                }
            }
        }
        self.fx.menus();
        self.fx.redraw();
    }

    // ------------------------------------------------------------ drawing

    pub fn layout_in(area: Rect, s: f32) -> Layout {
        let pad = (24.0 * s).round();
        let inner = Rect::new(area.x + pad, area.y + pad, (area.w - 2.0 * pad).max(0.0), (area.h - pad).max(0.0));
        let bh = (30.0 * s).round();
        let top = inner.y + (62.0 * s).round();
        let wants = ACTIONS.map(|a| match a {
            Action::Run => 100.0,
            _ => 160.0,
        });
        let gap = 8.0 * s;
        let fit = ((inner.w - 3.0 * gap) / (wants.iter().sum::<f32>() * s)).min(1.0);
        let mut actions = [Rect::default(); 4];
        let mut x = inner.x;
        for (i, want) in wants.iter().enumerate() {
            let w = (want * s * fit).floor();
            actions[i] = Rect::new(x, top, w, bh);
            x += w + gap;
        }
        let status_h = (24.0 * s).round();
        let y = top + bh + (16.0 * s).round();
        let h = (area.bottom() - status_h - (12.0 * s).round() - y).max(0.0);
        let list_w = (inner.w * 0.42).clamp((220.0 * s).min(inner.w * 0.6), 420.0 * s);
        let list = Rect::new(inner.x, y, list_w, h);
        let output = Rect::new(list.right() + (16.0 * s).round(), y, (inner.right() - list.right() - (16.0 * s).round()).max(0.0), h);
        Layout {
            area,
            actions,
            list,
            output,
            row_h: (66.0 * s).round(),
            status: Rect::new(area.x, area.bottom() - status_h, area.w, status_h),
        }
    }

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        use rt_core::i18n::strip_mnemonic;
        use rt_gfx::dialog::ellipsize;
        let l = Self::layout_in(area, s);
        g.fill_rect(area, th.bg);
        let pad = (24.0 * s).round();
        let (x, w) = (area.x + pad, (area.w - 2.0 * pad).max(0.0));
        g.text_in(&tr("Repair"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about =
            ellipsize(g, &tr("Windows' own repair tools, one at a time, with what they write as it comes."), w, 12.5 * s, FontId::Regular);
        g.text_in(&about, Rect::new(x, area.y + pad + 32.0 * s, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
        for (i, a) in ACTIONS.iter().enumerate() {
            let r = l.actions[i];
            let on = self.enabled(*a);
            let primary = *a == Action::Run && on;
            g.fill_rrect(r, 5.0 * s, if primary { th.accent } else { th.bg });
            if on && !primary && self.hot == Some(Hit::Act(i)) {
                g.fill_rrect(r, 5.0 * s, th.hover);
            }
            if !primary {
                g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
            }
            let label = strip_mnemonic(&tr(a.label_key()));
            let label = ellipsize(g, label.trim_end_matches("..."), r.w - 10.0 * s, 12.5 * s, FontId::Regular);
            let color = if primary {
                if th.dark {
                    Color::rgb(0x000000)
                } else {
                    Color::rgb(0xFFFFFF)
                }
            } else if on {
                th.text
            } else {
                th.dim
            };
            g.text_in(&label, r, 12.5 * s, FontId::Regular, color, Align::Center);
        }
        // The tools.
        g.stroke_rrect(l.list, 4.0 * s, 1.0, th.border);
        g.set_clip(Some(l.list));
        for (i, t) in self.tasks().iter().enumerate().skip(self.scroll) {
            let y = l.list.y + (i - self.scroll) as f32 * l.row_h;
            let r = Rect::new(l.list.x + 1.0, y, l.list.w - 2.0, l.row_h);
            if self.sel == i {
                g.fill_rect(r, th.selection);
            } else if self.hot == Some(Hit::Row(i)) {
                g.fill_rect(r, th.hover);
            }
            let ip = 12.0 * s;
            let tw = (r.w - 2.0 * ip).max(0.0);
            let name = ellipsize(g, &self.name(t), tw, 13.0 * s, FontId::Semibold);
            g.text_in(&name, Rect::new(r.x + ip, y + 5.0 * s, tw, 20.0 * s), 13.0 * s, FontId::Semibold, th.text, Align::Left);
            let ab = ellipsize(g, &Self::about(t), tw, 11.0 * s, FontId::Regular);
            g.text_in(&ab, Rect::new(r.x + ip, y + 24.0 * s, tw, 18.0 * s), 11.0 * s, FontId::Regular, th.dim, Align::Left);
            let (res, ok) = self.result_text(t);
            let color = match ok {
                Some(true) => th.good,
                Some(false) => th.warn,
                None => th.dim,
            };
            let rt = ellipsize(g, &res, tw, 11.5 * s, FontId::Regular);
            g.text_in(&rt, Rect::new(r.x + ip, y + 42.0 * s, tw, 18.0 * s), 11.5 * s, FontId::Regular, color, Align::Left);
        }
        g.set_clip(None);
        // What the tool writes.
        let o = l.output;
        g.fill_rrect(o, 4.0 * s, th.panel);
        g.stroke_rrect(o, 4.0 * s, 1.0, th.border);
        let ip = 10.0 * s;
        let mut top_y = o.y + ip;
        if self.running.is_some() {
            if let Some(p) = self.output.progress() {
                let bar = Rect::new(o.x + ip, top_y, o.w - 2.0 * ip, 6.0 * s);
                g.fill_rrect(bar, 3.0 * s, th.bar_track);
                g.fill_rrect(Rect::new(bar.x, bar.y, bar.w * p / 100.0, bar.h), 3.0 * s, th.accent);
                top_y += 14.0 * s;
            }
        }
        let lines = self.output.all();
        if lines.is_empty() {
            let m = if self.admin { tr("Choose a tool and press Run.") } else { tr("Windows' repair tools need administrator rights.") };
            g.text_in(&m, o, 12.5 * s, FontId::Regular, th.dim, Align::Center);
        } else {
            let lh = 17.0 * s;
            let rows = (((o.bottom() - ip - top_y) / lh).floor() as usize).max(1);
            let first = lines.len().saturating_sub(rows);
            g.set_clip(Some(o));
            for (k, line) in lines[first..].iter().enumerate() {
                let r = Rect::new(o.x + ip, top_y + k as f32 * lh, o.w - 2.0 * ip, lh);
                let t = ellipsize(g, line, r.w, 11.5 * s, FontId::Regular);
                let c = if line.starts_with("> ") { th.accent } else { th.text };
                g.text_in(&t, r, 11.5 * s, FontId::Regular, c, Align::Left);
            }
            g.set_clip(None);
        }
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let left = self.note.clone().unwrap_or_else(|| match &self.running {
            Some((t, _)) => match self.output.progress() {
                Some(p) => tf("{}: {}%", &[&self.name(t), &format!("{p:.0}")]),
                None => tf("{}: running…", &[&self.name(t)]),
            },
            None if !self.admin => tr("Windows' repair tools need administrator rights."),
            None => String::new(),
        });
        let sr = Rect::new(l.status.x + 10.0 * s, l.status.y, (l.status.w - 20.0 * s).max(0.0), l.status.h);
        let left = ellipsize(g, &left, sr.w, 12.0 * s, FontId::Regular);
        g.text_in(&left, sr, 12.0 * s, FontId::Regular, th.dim, Align::Left);
        self.layout = l;
    }

    // ------------------------------------------------------------ input

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let l = &self.layout;
        if let Some(i) = l.actions.iter().position(|r| r.contains(x, y)) {
            return Some(Hit::Act(i));
        }
        if !l.list.contains(x, y) || l.row_h <= 0.0 {
            return None;
        }
        let i = self.scroll + ((y - l.list.y) / l.row_h) as usize;
        (i < self.tasks().len()).then_some(Hit::Row(i))
    }

    /// What the pointer is on; `blocked` while a message is open.
    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        let hot = if blocked { None } else { self.hit(x, y) };
        if hot != self.hot {
            self.hot = hot;
            self.fx.redraw();
        }
        self.cursor_icon = match hot {
            Some(Hit::Act(i)) if self.enabled(ACTIONS[i]) => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        };
    }

    pub fn pointer_left(&mut self) {
        if self.hot.take().is_some() {
            self.fx.redraw();
        }
    }

    pub fn release(&mut self) {}

    /// A click: a row is selected; a button gives its action for the window to do.
    pub fn press(&mut self) -> Option<Action> {
        let out = match self.hot {
            Some(Hit::Act(i)) => Some(ACTIONS[i]),
            Some(Hit::Row(i)) => {
                self.sel = i;
                self.note = None;
                None
            }
            None => None,
        };
        self.fx.menus();
        self.fx.redraw();
        out
    }

    /// The arrows choose the tool; Enter runs it.
    pub fn key(&mut self, key: &winit::keyboard::Key) -> Option<Action> {
        use winit::keyboard::{Key, NamedKey};
        let n = self.tasks().len();
        match key {
            Key::Named(NamedKey::Enter) => return Some(Action::Run),
            Key::Named(NamedKey::ArrowDown) => self.sel = (self.sel + 1).min(n.saturating_sub(1)),
            Key::Named(NamedKey::ArrowUp) => self.sel = self.sel.saturating_sub(1),
            _ => return None,
        }
        // Keep the selection in view.
        let rows = self.rows_shown();
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + rows {
            self.scroll = self.sel + 1 - rows;
        }
        self.fx.menus();
        self.fx.redraw();
        None
    }

    /// How many rows the list shows.
    fn rows_shown(&self) -> usize {
        if self.layout.row_h <= 0.0 {
            return 1;
        }
        ((self.layout.list.h / self.layout.row_h).floor() as usize).max(1)
    }

    /// The mouse wheel scrolls the list.
    pub fn wheel(&mut self, delta: winit::event::MouseScrollDelta) {
        let rows = match delta {
            winit::event::MouseScrollDelta::LineDelta(_, l) => (-l).round() as isize,
            winit::event::MouseScrollDelta::PixelDelta(p) => (-p.y as f32 / self.layout.row_h.max(20.0)).round() as isize,
        };
        let max = self.tasks().len().saturating_sub(self.rows_shown());
        self.scroll = (self.scroll as isize + rows).clamp(0, max as isize) as usize;
        self.fx.redraw();
    }

    /// The page's nodes for the screen readers: the tools are 100 + their index.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role};
        let mut list = Node::new(Role::List);
        list.set_label(tr("Repair"));
        let mut ids = Vec::new();
        for (i, t) in self.tasks().iter().enumerate() {
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::ListItem);
            n.set_label(format!("{}: {}", self.name(t), self.result_text(t).0));
            n.set_selected(self.sel == i);
            n.add_action(accesskit::Action::Click);
            ids.push(nid);
            nodes.push((nid, n));
        }
        list.set_children(ids);
        nodes.push((NodeId(1), list));
        children.push(NodeId(1));
        Some(NodeId(100 + self.sel as u64))
    }

    /// A screen reader clicked a tool: it is selected.
    pub fn accessibility_click(&mut self, i: usize) {
        if i < self.tasks().len() {
            self.sel = i;
            self.fx.menus();
            self.fx.redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_results_and_rights() {
        let mut p = RepairPage::new(false);
        let t = p.tasks();
        assert!(t[0] == Task::Sfc && t[1] == Task::Dism && *t.last().unwrap() == Task::StartMenu);
        assert!(matches!(t[2], Task::Check(_)), "the system drive stands in until the drives are known");
        p.drives = Some(vec!['C', 'E']);
        assert_eq!(p.tasks().len(), 5);
        assert!(!p.enabled(Action::Run), "administrator rights");
        p.admin = true;
        assert!(p.enabled(Action::Run) && !p.enabled(Action::Fix));
        p.sel = 3;
        p.results.insert(Task::Check('E'), Ok(3));
        assert!(p.enabled(Action::Fix));
        assert_eq!(p.result_text(&Task::Check('E')).1, Some(false));
        p.results.insert(Task::Fix('E'), Ok(0));
        assert_eq!(p.result_text(&Task::Check('E')).1, Some(true), "the fix is what counts after it");
        p.results.insert(Task::Sfc, Ok(0));
        assert_eq!(p.result_text(&Task::Sfc), (tr("Finished."), Some(true)));
        assert_eq!(p.result_text(&Task::Dism).1, None);
    }

    #[test]
    fn layout_and_pointer() {
        let mut p = RepairPage::new(true);
        p.layout = RepairPage::layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        let l = p.layout.clone();
        assert!(l.list.right() < l.output.x && l.output.right() <= 224.0 + 956.0);
        p.pointer(l.list.x + 30.0, l.list.y + l.row_h * 1.5, false);
        assert_eq!(p.press(), None);
        assert_eq!(p.sel, 1);
        p.pointer(l.actions[0].x + 3.0, l.actions[0].y + 3.0, false);
        assert_eq!(p.press(), Some(Action::Run));
        assert_eq!(p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter)), Some(Action::Run));
    }
}
