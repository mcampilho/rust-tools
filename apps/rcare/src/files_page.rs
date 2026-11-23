//! The File Tools page: two cards. Empty Folders — choose a folder, look for
//! the folders with no file inside, send the ticked ones to the Recycle Bin.
//! File Shredder — add files or a folder, then shred them: overwritten,
//! renamed, deleted, beyond the Recycle Bin and undelete tools (`files`).
//! Both work in a thread and ask first.

use crate::bg::Job;
use crate::files::{self, Shredded};
use crate::i18n::{tf, tr};
use rt_gfx::gfx::{Align, Color, FontId, Gfx, Rect};
use rt_ui::page::{Dialogs, Effects};
use rt_ui::theme::Theme;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use winit::window::CursorIcon;

/// What the page does (its buttons and menu entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    ChooseFolder,
    Scan,
    Remove,
    AddFiles,
    AddFolder,
    Clear,
    Shred,
}

/// The buttons of each card.
pub const LEFT: [Action; 2] = [Action::ChooseFolder, Action::Scan];
pub const RIGHT: [Action; 3] = [Action::AddFiles, Action::AddFolder, Action::Clear];

impl Action {
    /// The menu label (a translation key with its access key).
    pub fn label_key(self) -> &'static str {
        match self {
            Action::ChooseFolder => "Choose the &Folder...",
            Action::Scan => "&Scan for Empty Folders",
            Action::Remove => "&Remove the Ticked Folders...",
            Action::AddFiles => "&Add Files to Shred...",
            Action::AddFolder => "Add a Fol&der to Shred...",
            Action::Clear => "C&lear the List",
            Action::Shred => "S&hred...",
        }
    }
}

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Act(Action),
    /// An empty folder's box.
    Empty(usize),
    /// The × that takes an item off the shredding list.
    Drop(usize),
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub left: Rect,
    pub right: Rect,
    pub left_buttons: [Rect; 2],
    pub right_buttons: [Rect; 3],
    pub empties: Rect,
    pub items: Rect,
    pub remove: Rect,
    pub shred: Rect,
    pub row_h: f32,
    pub status: Rect,
}

/// An empty folder found: where, how many empty folders it is, ticked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Empty {
    pub path: PathBuf,
    pub count: usize,
    pub ticked: bool,
}

#[derive(Default)]
pub struct FilesPage {
    pub root: Option<PathBuf>,
    pub empties: Option<Vec<Empty>>,
    pub items: Vec<PathBuf>,
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    pub note: Option<String>,
    scanning: Job<Vec<(PathBuf, usize)>>,
    stop: Arc<AtomicBool>,
    removing: Job<(usize, Vec<String>)>,
    shredding: Job<Shredded>,
}

impl FilesPage {
    pub fn busy(&self) -> bool {
        self.scanning.busy() || self.removing.busy() || self.shredding.busy()
    }

    fn ticked(&self) -> Vec<PathBuf> {
        self.empties.iter().flatten().filter(|e| e.ticked).map(|e| e.path.clone()).collect()
    }

    pub fn enabled(&self, a: Action) -> bool {
        match a {
            Action::ChooseFolder | Action::AddFiles | Action::AddFolder => !self.busy(),
            Action::Scan => !self.busy() && self.root.is_some(),
            Action::Remove => !self.busy() && !self.ticked().is_empty(),
            Action::Clear => !self.busy() && !self.items.is_empty(),
            Action::Shred => !self.busy() && !self.items.is_empty(),
        }
    }

    fn scan(&mut self) {
        let Some(root) = self.root.clone() else { return };
        self.stop = Arc::new(AtomicBool::new(false));
        let stop = self.stop.clone();
        self.empties = None;
        self.scanning.start(move || files::empty_folders(&root, &stop));
        self.note = Some(tr("Looking for empty folders…"));
    }

    /// While working: takes what came, and when to look again.
    pub fn animate(&mut self) -> Option<std::time::Instant> {
        if let Some(found) = self.scanning.take() {
            let n: usize = found.iter().map(|(_, c)| c).sum();
            self.note = Some(tf("{} empty folders.", &[&n.to_string()]));
            self.empties = Some(found.into_iter().map(|(path, count)| Empty { path, count, ticked: true }).collect());
            self.fx.menus();
            self.fx.redraw();
        }
        if let Some((done, failed)) = self.removing.take() {
            let mut text = tf("{} folders went to the Recycle Bin.", &[&done.to_string()]);
            if !failed.is_empty() {
                text += "\n\n";
                text += &tf("{} could not be removed: {}", &[&failed.len().to_string(), &failed[0]]);
            }
            self.fx.info(&tr("File Tools"), &text);
            self.scan();
        }
        if let Some(s) = self.shredding.take() {
            let mut text = tf("{} files shredded ({}).", &[&s.done.to_string(), &rsize::format::size(s.bytes)]);
            if !s.failed.is_empty() {
                let list: Vec<String> = s.failed.iter().take(8).map(|(p, e)| format!("  {} — {e}", p.display())).collect();
                text += "\n\n";
                text += &tf("{} could not be shredded (in use?):", &[&s.failed.len().to_string()]);
                text += "\n";
                text += &list.join("\n");
            }
            self.items.retain(|p| p.exists());
            self.note = Some(tf("{} files shredded.", &[&s.done.to_string()]));
            self.fx.info(&tr("File Shredder"), &text);
            self.fx.menus();
            self.fx.redraw();
        }
        self.busy().then(crate::bg::soon)
    }

    /// Does what a button or its menu entry asks.
    pub fn act(&mut self, a: Action, dialogs: &mut dyn Dialogs) {
        if !self.enabled(a) {
            return;
        }
        match a {
            Action::ChooseFolder => {
                if let Some(d) = dialogs.pick_folder(&tr("Choose the folder to look for empty folders in")) {
                    self.root = Some(d);
                    self.scan();
                }
            }
            Action::Scan => self.scan(),
            Action::Remove => {
                let ticked = self.ticked();
                let n: usize = self.empties.iter().flatten().filter(|e| e.ticked).map(|e| e.count).sum();
                let list: Vec<String> = ticked.iter().take(10).map(|p| format!("  {}", p.display())).collect();
                let more = if ticked.len() > 10 {
                    format!("\n  … {}", tf("and {} more", &[&(ticked.len() - 10).to_string()]))
                } else {
                    String::new()
                };
                let text = format!("{}\n\n{}{more}", tf("Send {} empty folders to the Recycle Bin?", &[&n.to_string()]), list.join("\n"));
                if dialogs.confirm(&tr("File Tools"), &text) {
                    self.removing.start(move || {
                        let (mut done, mut failed) = (0, Vec::new());
                        for p in ticked {
                            match rt_sys::recycle(&p) {
                                Ok(()) => done += 1,
                                Err(e) => failed.push(format!("{}: {e}", p.display())),
                            }
                        }
                        (done, failed)
                    });
                    self.note = Some(tr("Removing…"));
                }
            }
            Action::AddFiles => {
                for f in dialogs.open_files() {
                    if !self.items.contains(&f) {
                        self.items.push(f);
                    }
                }
            }
            Action::AddFolder => {
                if let Some(d) = dialogs.pick_folder(&tr("Choose a folder to shred (all its files, then the folder)")) {
                    if !self.items.contains(&d) {
                        self.items.push(d);
                    }
                }
            }
            Action::Clear => self.items.clear(),
            Action::Shred => {
                let files: usize = self.items.iter().map(|p| if p.is_dir() { files::files_in(p).len() } else { 1 }).sum();
                let list: Vec<String> = self.items.iter().take(10).map(|p| format!("  {}", p.display())).collect();
                let text = format!(
                    "{}\n\n{}\n\n{}",
                    tf(
                        "Shred {} files? They cannot be brought back: not from the Recycle Bin, not with undelete tools.",
                        &[&files.to_string()]
                    ),
                    list.join("\n"),
                    tr("On an SSD or a USB stick the drive may keep old copies of the blocks: shredding there is not a guarantee.")
                );
                if dialogs.confirm(&tr("File Shredder"), &text) {
                    let items = self.items.clone();
                    self.shredding.start(move || files::shred_all(&items));
                    self.note = Some(tr("Shredding…"));
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
        let status_h = (24.0 * s).round();
        let y = inner.y + (62.0 * s).round();
        let h = (area.bottom() - status_h - (12.0 * s).round() - y).max(0.0);
        let gap = (12.0 * s).round();
        let cw = ((inner.w - gap) / 2.0).floor();
        let left = Rect::new(inner.x, y, cw, h);
        let right = Rect::new(left.right() + gap, y, cw, h);
        let ip = 14.0 * s;
        let bh = (28.0 * s).round();
        // Side by side, or one under the other when they would be too narrow for their labels.
        let buttons = |card: Rect, n: usize| -> Vec<Rect> {
            let w = ((card.w - 2.0 * ip - (n - 1) as f32 * 8.0 * s) / n as f32).floor();
            let y0 = card.y + ip + 46.0 * s;
            if w >= 140.0 * s {
                (0..n).map(|i| Rect::new(card.x + ip + i as f32 * (w + 8.0 * s), y0, w, bh)).collect()
            } else {
                (0..n).map(|i| Rect::new(card.x + ip, y0 + i as f32 * (bh + 6.0 * s), card.w - 2.0 * ip, bh)).collect()
            }
        };
        let lb = buttons(left, 2);
        let rb = buttons(right, 3);
        let top = lb.iter().chain(rb.iter()).map(|r| r.bottom()).fold(0.0, f32::max) + 10.0 * s;
        let bottom_btn = |card: Rect| Rect::new(card.x + ip, card.bottom() - ip - bh, card.w - 2.0 * ip, bh);
        let remove = bottom_btn(left);
        let shred = bottom_btn(right);
        let empties = Rect::new(left.x + ip, top, left.w - 2.0 * ip, (remove.y - 10.0 * s - top).max(0.0));
        let items = Rect::new(right.x + ip, top, right.w - 2.0 * ip, (shred.y - 10.0 * s - top).max(0.0));
        Layout {
            area,
            left,
            right,
            left_buttons: [lb[0], lb[1]],
            right_buttons: [rb[0], rb[1], rb[2]],
            empties,
            items,
            remove,
            shred,
            row_h: (24.0 * s).round(),
            status: Rect::new(area.x, area.bottom() - status_h, area.w, status_h),
        }
    }

    fn button(&self, g: &mut Gfx, r: Rect, a: Action, primary: bool, th: &Theme, s: f32) {
        use rt_core::i18n::strip_mnemonic;
        let on = self.enabled(a);
        let main = primary && on;
        g.fill_rrect(
            r,
            5.0 * s,
            if main {
                th.accent
            } else if on && self.hot == Some(Hit::Act(a)) {
                th.hover
            } else {
                th.bg
            },
        );
        if !main {
            g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
        }
        let label = strip_mnemonic(&tr(a.label_key()));
        let t = rt_gfx::dialog::ellipsize(g, label.trim_end_matches("..."), r.w - 8.0 * s, 12.5 * s, FontId::Regular);
        let fg = if main {
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
        g.text_in(&t, r, 12.5 * s, FontId::Regular, fg, Align::Center);
    }

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        use rt_gfx::dialog::{ellipsize, ellipsize_middle};
        let l = Self::layout_in(area, s);
        g.fill_rect(area, th.bg);
        let pad = (24.0 * s).round();
        let (x, w) = (area.x + pad, (area.w - 2.0 * pad).max(0.0));
        g.text_in(&tr("File Tools"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(g, &tr("Empty folders to the Recycle Bin, and files shredded for good."), w, 12.5 * s, FontId::Regular);
        g.text_in(&about, Rect::new(x, area.y + pad + 32.0 * s, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
        let ip = 14.0 * s;
        let small = 11.5 * s;
        for (card, title, sub) in [
            (l.left, tr("Empty Folders"), self.root.as_ref().map_or_else(|| tr("No folder chosen yet."), |r| r.display().to_string())),
            (l.right, tr("File Shredder"), tr("Overwritten and deleted: undelete tools find nothing.")),
        ] {
            g.fill_rrect(card, 8.0 * s, th.panel);
            g.stroke_rrect(card, 8.0 * s, 1.0, th.border);
            g.text_in(
                &title,
                Rect::new(card.x + ip, card.y + ip, card.w - 2.0 * ip, 20.0 * s),
                14.5 * s,
                FontId::Semibold,
                th.text,
                Align::Left,
            );
            let sr = Rect::new(card.x + ip, card.y + ip + 22.0 * s, card.w - 2.0 * ip, 18.0 * s);
            let st = ellipsize_middle(g, &sub, sr.w, small, FontId::Regular);
            g.text_in(&st, sr, small, FontId::Regular, th.dim, Align::Left);
        }
        for (r, a) in l.left_buttons.iter().zip(LEFT) {
            self.button(g, *r, a, false, th, s);
        }
        for (r, a) in l.right_buttons.iter().zip(RIGHT) {
            self.button(g, *r, a, false, th, s);
        }
        self.button(g, l.remove, Action::Remove, true, th, s);
        self.button(g, l.shred, Action::Shred, true, th, s);
        // The empty folders.
        g.stroke_rrect(l.empties, 4.0 * s, 1.0, th.border);
        g.set_clip(Some(l.empties));
        match &self.empties {
            None if self.scanning.busy() => {
                g.text_in(&tr("Looking for empty folders…"), l.empties, 12.0 * s, FontId::Regular, th.dim, Align::Center);
            }
            None => {
                g.text_in(
                    &tr("Choose a folder: its empty folders show here."),
                    l.empties,
                    12.0 * s,
                    FontId::Regular,
                    th.dim,
                    Align::Center,
                );
            }
            Some(v) if v.is_empty() => {
                g.text_in(&tr("No empty folders."), l.empties, 12.0 * s, FontId::Regular, th.good, Align::Center);
            }
            Some(v) => {
                for (i, e) in v.iter().enumerate() {
                    let y = l.empties.y + 2.0 * s + i as f32 * l.row_h;
                    if y > l.empties.bottom() {
                        break;
                    }
                    let row = Rect::new(l.empties.x + 1.0, y, l.empties.w - 2.0, l.row_h);
                    if self.hot == Some(Hit::Empty(i)) {
                        g.fill_rect(row, th.hover);
                    }
                    let bx = Rect::new(row.x + 8.0 * s, y + (l.row_h - 14.0 * s) / 2.0, 14.0 * s, 14.0 * s);
                    if e.ticked {
                        g.fill_rrect(bx, 3.0 * s, th.accent);
                        let k = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
                        g.polyline(
                            &[(bx.x + 3.0 * s, bx.y + 7.0 * s), (bx.x + 6.0 * s, bx.y + 10.0 * s), (bx.x + 11.0 * s, bx.y + 4.0 * s)],
                            1.8 * s,
                            k,
                            false,
                        );
                    } else {
                        g.stroke_rrect(bx, 3.0 * s, 1.2 * s, th.dim);
                    }
                    let name = self
                        .root
                        .as_ref()
                        .and_then(|r| e.path.strip_prefix(r).ok())
                        .map_or_else(|| e.path.display().to_string(), |p| p.display().to_string());
                    let label = if e.count > 1 { format!("{name}  ({})", tf("{} folders", &[&e.count.to_string()])) } else { name };
                    let tr_ = Rect::new(bx.right() + 8.0 * s, y, row.right() - bx.right() - 14.0 * s, l.row_h);
                    let t = ellipsize_middle(g, &label, tr_.w, small, FontId::Regular);
                    g.text_in(&t, tr_, small, FontId::Regular, th.text, Align::Left);
                }
            }
        }
        g.set_clip(None);
        // What will be shredded.
        g.stroke_rrect(l.items, 4.0 * s, 1.0, th.border);
        g.set_clip(Some(l.items));
        if self.items.is_empty() {
            g.text_in(&tr("Add the files or a folder to shred."), l.items, 12.0 * s, FontId::Regular, th.dim, Align::Center);
        }
        for (i, p) in self.items.iter().enumerate() {
            let y = l.items.y + 2.0 * s + i as f32 * l.row_h;
            if y > l.items.bottom() {
                break;
            }
            let row = Rect::new(l.items.x + 1.0, y, l.items.w - 2.0, l.row_h);
            let x_btn = Rect::new(row.right() - 24.0 * s, y + 2.0 * s, 20.0 * s, l.row_h - 4.0 * s);
            if self.hot == Some(Hit::Drop(i)) {
                g.fill_rrect(x_btn, 4.0 * s, th.hover);
            }
            g.text_in("×", x_btn, 14.0 * s, FontId::Regular, th.dim, Align::Center);
            let kind =
                if p.is_dir() { tr("folder") } else { std::fs::metadata(p).map(|m| rsize::format::size(m.len())).unwrap_or_default() };
            let tr_ = Rect::new(row.x + 8.0 * s, y, row.w - 40.0 * s, l.row_h);
            let label = if kind.is_empty() { p.display().to_string() } else { format!("{}  ({kind})", p.display()) };
            let t = ellipsize_middle(g, &label, tr_.w, small, FontId::Regular);
            g.text_in(&t, tr_, small, FontId::Regular, th.text, Align::Left);
        }
        g.set_clip(None);
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let left = self.note.clone().unwrap_or_default();
        let sr = Rect::new(l.status.x + 10.0 * s, l.status.y, (l.status.w - 20.0 * s).max(0.0), l.status.h);
        let left = ellipsize(g, &left, sr.w, 12.0 * s, FontId::Regular);
        g.text_in(&left, sr, 12.0 * s, FontId::Regular, th.dim, Align::Left);
        self.layout = l;
    }

    // ------------------------------------------------------------ input

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let l = &self.layout;
        for (r, a) in l.left_buttons.iter().zip(LEFT).chain(l.right_buttons.iter().zip(RIGHT)) {
            if r.contains(x, y) {
                return Some(Hit::Act(a));
            }
        }
        if l.remove.contains(x, y) {
            return Some(Hit::Act(Action::Remove));
        }
        if l.shred.contains(x, y) {
            return Some(Hit::Act(Action::Shred));
        }
        if l.empties.contains(x, y) && l.row_h > 0.0 {
            let i = ((y - l.empties.y - 2.0) / l.row_h) as usize;
            if self.empties.as_ref().is_some_and(|v| i < v.len()) {
                return Some(Hit::Empty(i));
            }
        }
        if l.items.contains(x, y) && l.row_h > 0.0 && x > l.items.right() - 28.0 {
            let i = ((y - l.items.y - 2.0) / l.row_h) as usize;
            if i < self.items.len() {
                return Some(Hit::Drop(i));
            }
        }
        None
    }

    /// What the pointer is on; `blocked` while a message is open.
    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        let hot = if blocked { None } else { self.hit(x, y) };
        if hot != self.hot {
            self.hot = hot;
            self.fx.redraw();
        }
        self.cursor_icon = match hot {
            Some(Hit::Act(a)) if !self.enabled(a) => CursorIcon::Default,
            Some(_) => CursorIcon::Pointer,
            None => CursorIcon::Default,
        };
    }

    pub fn pointer_left(&mut self) {
        if self.hot.take().is_some() {
            self.fx.redraw();
        }
    }

    pub fn release(&mut self) {}

    /// A click: a box ticks, a × takes an item off; a button gives its action for the window to do.
    pub fn press(&mut self) -> Option<Action> {
        let out = match self.hot? {
            Hit::Act(a) => return Some(a),
            Hit::Empty(i) => {
                if let Some(e) = self.empties.as_mut().and_then(|v| v.get_mut(i)) {
                    e.ticked = !e.ticked;
                }
                None
            }
            Hit::Drop(i) => {
                if !self.busy() && i < self.items.len() {
                    self.items.remove(i);
                }
                None
            }
        };
        self.fx.menus();
        self.fx.redraw();
        out
    }

    pub fn key(&mut self, _key: &winit::keyboard::Key) -> Option<Action> {
        None
    }

    /// The page's nodes for the screen readers: the empty folders are 100 + their index.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role, Toggled};
        let mut list = Node::new(Role::List);
        list.set_label(tr("Empty Folders"));
        let mut ids = Vec::new();
        for (i, e) in self.empties.iter().flatten().enumerate() {
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::CheckBox);
            n.set_label(e.path.display().to_string());
            n.set_toggled(if e.ticked { Toggled::True } else { Toggled::False });
            n.add_action(accesskit::Action::Click);
            ids.push(nid);
            nodes.push((nid, n));
        }
        list.set_children(ids);
        nodes.push((NodeId(1), list));
        children.push(NodeId(1));
        None
    }

    /// A screen reader clicked an empty folder: it is ticked or not.
    pub fn accessibility_click(&mut self, i: usize) {
        if let Some(e) = self.empties.as_mut().and_then(|v| v.get_mut(i)) {
            e.ticked = !e.ticked;
            self.fx.menus();
            self.fx.redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_can_be_done() {
        let mut p = FilesPage::default();
        assert!(p.enabled(Action::ChooseFolder) && !p.enabled(Action::Scan) && !p.enabled(Action::Shred));
        p.root = Some(std::env::temp_dir());
        p.empties = Some(vec![
            Empty { path: std::env::temp_dir().join("a"), count: 1, ticked: true },
            Empty { path: std::env::temp_dir().join("b"), count: 3, ticked: false },
        ]);
        assert!(p.enabled(Action::Scan) && p.enabled(Action::Remove));
        assert_eq!(p.ticked().len(), 1);
        p.items.push(std::env::temp_dir().join("x.txt"));
        assert!(p.enabled(Action::Shred) && p.enabled(Action::Clear));
        p.layout = FilesPage::layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        let l = p.layout.clone();
        assert!(l.left.right() < l.right.x && l.right.right() <= 224.0 + 956.0);
        // The second folder's box, then the item's ×.
        p.pointer(l.empties.x + 12.0, l.empties.y + 2.0 + l.row_h * 1.5, false);
        assert_eq!(p.hot, Some(Hit::Empty(1)));
        p.press();
        assert_eq!(p.ticked().len(), 2);
        p.pointer(l.items.right() - 10.0, l.items.y + 2.0 + l.row_h * 0.5, false);
        assert_eq!(p.hot, Some(Hit::Drop(0)));
        p.press();
        assert!(p.items.is_empty());
        p.pointer(l.shred.x + 3.0, l.shred.y + 3.0, false);
        assert_eq!(p.press(), Some(Action::Shred));
    }
}
