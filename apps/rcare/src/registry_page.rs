//! The Registry Cleaner page: the invalid registry entries of the cleaner's
//! engine (`rt_clean::registry`: one category each, ticked to clean), two
//! categories that are only reported (COM classes and services whose file
//! is gone), and the size of the registry's files (Windows cannot compact
//! them while in use, so that is only shown). Cleaning asks first and saves
//! every entry to a `.reg` file before deleting it; opening that file puts
//! the entries back.

use crate::i18n::{tf, tr};
use rt_clean::clean::Found;
use rt_clean::registry;
use rt_gfx::gfx::{Align, Color, FontId, Gfx, Rect};
use rt_sys::regfile::Entry;
use rt_ui::page::{Dialogs, Effects};
use rt_ui::theme::Theme;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use winit::window::CursorIcon;

/// What the page does (its buttons and menu entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Scan,
    Clean,
    Copies,
    Size,
}

pub const ACTIONS: [Action; 4] = [Action::Scan, Action::Clean, Action::Copies, Action::Size];

impl Action {
    /// The menu label (a translation key with its access key).
    pub fn label_key(self) -> &'static str {
        match self {
            Action::Scan => "&Scan",
            Action::Clean => "&Clean the Ticked Entries...",
            Action::Copies => "Open the C&opies Folder",
            Action::Size => "Registry &Size...",
        }
    }
}

/// One category of the list.
#[derive(Clone, Debug)]
pub struct Category {
    pub id: String,
    /// Translation keys.
    pub name: &'static str,
    pub about: &'static str,
    /// Changing it needs administrator rights.
    pub admin: bool,
    /// Only shown, never cleaned.
    pub report: bool,
    pub ticked: bool,
    /// What was found (None: not looked yet).
    pub found: Option<Vec<(Entry, String)>>,
}

impl Category {
    pub fn count(&self) -> usize {
        self.found.as_ref().map_or(0, Vec::len)
    }
}

/// Every category: the cleaner's, then the reports.
pub fn categories(admin: bool) -> Vec<Category> {
    let mut out: Vec<Category> = registry::targets()
        .into_iter()
        .map(|t| Category { ticked: admin || !t.admin, id: t.id, name: t.name, about: t.about, admin: t.admin, report: false, found: None })
        .collect();
    out.extend(registry::REPORTS.iter().map(|(id, name, about)| Category {
        id: id.to_string(),
        name,
        about,
        admin: true,
        report: true,
        ticked: false,
        found: None,
    }));
    out
}

/// The registry's files: Windows' (`config`) and the user's, with their size.
pub fn hives() -> Vec<(String, Option<u64>)> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let windows = std::env::var_os("SystemRoot").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
    for h in ["SYSTEM", "SOFTWARE", "SAM", "SECURITY", "DEFAULT", "COMPONENTS", "DRIVERS"] {
        files.push((h.to_string(), windows.join("System32\\config").join(h)));
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        files.push(("NTUSER.DAT".into(), PathBuf::from(home).join("NTUSER.DAT")));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        files.push(("UsrClass.dat".into(), PathBuf::from(local).join("Microsoft\\Windows\\UsrClass.dat")));
    }
    files.into_iter().map(|(n, p)| (n, size_of(&p))).filter(|(_, s)| s.is_some()).collect()
}

/// The size of a file, even one Windows keeps open (its attributes are read).
fn size_of(p: &Path) -> Option<u64> {
    std::fs::metadata(p).ok().map(|m| m.len())
}

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Act(usize),
    Tick(usize),
    Row(usize),
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub actions: [Rect; 4],
    /// The categories (left) and the entries of the one selected (right).
    pub list: Rect,
    pub entries: Rect,
    pub row_h: f32,
    pub entry_h: f32,
    pub status: Rect,
}

impl Layout {
    fn rows_in(h: f32, row: f32) -> usize {
        if row <= 0.0 {
            1
        } else {
            ((h / row).floor() as usize).max(1)
        }
    }

    pub fn page_rows(&self) -> usize {
        Self::rows_in(self.list.h, self.row_h)
    }

    pub fn entry_rows(&self) -> usize {
        Self::rows_in(self.entries.h - self.row_h * 2.0, self.entry_h)
    }
}

/// What the scan sends back: a category's index and what it found.
type Scanned = (usize, Vec<(Entry, String)>);

#[derive(Default)]
pub struct RegistryPage {
    pub cats: Vec<Category>,
    pub sel: usize,
    pub scroll: usize,
    /// The first entry shown of the category selected.
    pub entry_scroll: usize,
    pub admin: bool,
    /// Where the `.reg` copies go (`backups` in rcare's data folder).
    pub data_dir: PathBuf,
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    pub note: Option<String>,
    /// The scan under way: what it sends, and how many categories are left.
    scanning: Option<(Receiver<Scanned>, usize)>,
    /// Cleaning (reg.exe once a key or value): (deleted, failed, the first error, the copies' folder).
    cleaning: crate::bg::Job<(u64, u64, Option<String>, PathBuf)>,
}

impl RegistryPage {
    pub fn new(admin: bool, data_dir: PathBuf) -> RegistryPage {
        RegistryPage { cats: categories(admin), admin, data_dir, ..RegistryPage::default() }
    }

    pub fn busy(&self) -> bool {
        self.scanning.is_some() || self.cleaning.busy()
    }

    /// The folder of the `.reg` copies (the cleaner's, beside the settings).
    pub fn copies_dir(&self) -> PathBuf {
        self.data_dir.join("backups")
    }

    /// Looks at every category again, in a thread.
    pub fn scan(&mut self) {
        if self.busy() {
            return;
        }
        let (send, recv) = channel();
        let ids: Vec<(usize, String, bool)> = self.cats.iter().enumerate().map(|(i, c)| (i, c.id.clone(), c.report)).collect();
        for c in &mut self.cats {
            c.found = None;
        }
        self.scanning = Some((recv, ids.len()));
        self.note = None;
        std::thread::spawn(move || {
            let targets = registry::targets();
            for (i, id, report) in ids {
                let found = if report {
                    registry::report(&id)
                } else {
                    targets.iter().find(|t| t.id == id).map(|t| registry::analyse(t).entries).unwrap_or_default()
                };
                if send.send((i, found)).is_err() {
                    break;
                }
            }
        });
        self.fx.menus();
        self.fx.redraw();
    }

    /// While scanning or cleaning: takes what arrived, and when to look again.
    pub fn animate(&mut self) -> Option<Instant> {
        if let Some((done, failed, error, dir)) = self.cleaning.take() {
            let mut text = tf("{} registry entries were deleted.", &[&done.to_string()]);
            if failed > 0 {
                text += "\n\n";
                text += &tf("{} could not be deleted: {}", &[&failed.to_string(), error.as_deref().unwrap_or("")]);
            }
            if dir.is_dir() {
                text += "\n\n";
                text += &tf(
                    "The registry entries were saved in {} first: open the .reg files there to put them back.",
                    &[&dir.display().to_string()],
                );
            }
            self.fx.info(&tr("Registry Cleaner"), &text);
            self.note = Some(tf("{} registry entries were deleted.", &[&done.to_string()]));
            self.scan();
        }
        if self.cleaning.busy() {
            return Some(crate::bg::soon());
        }
        let (recv, left) = self.scanning.as_mut()?;
        let (mut changed, mut gone) = (false, false);
        loop {
            match recv.try_recv() {
                Ok((i, found)) => {
                    if let Some(c) = self.cats.get_mut(i) {
                        c.found = Some(found);
                    }
                    *left = left.saturating_sub(1);
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                // The thread ended (all sent, or it failed): nothing more will come.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    gone = true;
                    break;
                }
            }
        }
        let done = *left == 0 || gone;
        if done {
            self.scanning = None;
            self.fx.menus();
        }
        if changed || done {
            self.fx.redraw();
        }
        self.scanning.as_ref().map(|_| Instant::now() + Duration::from_millis(100))
    }

    /// Whether a category can be ticked and cleaned from here now.
    pub fn cleanable(&self, i: usize) -> bool {
        self.cats.get(i).is_some_and(|c| !c.report && (self.admin || !c.admin))
    }

    /// The categories to clean: ticked, cleanable, with something found.
    fn to_clean(&self) -> Vec<usize> {
        (0..self.cats.len()).filter(|&i| self.cats[i].ticked && self.cleanable(i) && self.cats[i].count() > 0).collect()
    }

    pub fn enabled(&self, a: Action) -> bool {
        match a {
            Action::Scan => !self.busy(),
            Action::Clean => !self.busy() && !self.to_clean().is_empty(),
            Action::Copies => self.copies_dir().is_dir(),
            Action::Size => true,
        }
    }

    fn toggle(&mut self, i: usize) {
        if self.cleanable(i) {
            self.cats[i].ticked = !self.cats[i].ticked;
        } else if self.cats.get(i).is_some_and(|c| !c.report) {
            self.fx.info(
                &tr("Registry Cleaner"),
                &tr("Cleaning the entries of every user needs administrator rights: restart rcare as administrator (File menu)."),
            );
        }
        self.fx.menus();
        self.fx.redraw();
    }

    /// Does what a button or its menu entry asks.
    pub fn act(&mut self, a: Action, dialogs: &mut dyn Dialogs) {
        if !self.enabled(a) {
            return;
        }
        match a {
            Action::Scan => self.scan(),
            Action::Clean => self.clean(dialogs),
            Action::Copies => rt_sys::reveal(&self.copies_dir()),
            Action::Size => {
                let list = hives();
                let total: u64 = list.iter().filter_map(|(_, s)| *s).sum();
                let lines: Vec<String> = list.iter().map(|(n, s)| format!("  {n}\t{}", rsize::format::size(s.unwrap_or(0)))).collect();
                let mut text = format!(
                    "{}\n\n{}\n\n{}",
                    tf("The registry's files take {}:", &[&rsize::format::size(total)]),
                    lines.join("\n"),
                    tr("Windows keeps these files open and tidies them itself; they cannot be compacted while in use, so rcare only shows their size.")
                );
                // Windows' own (`System32\config`) cannot even be listed without the rights.
                if !list.iter().any(|(n, _)| n == "SOFTWARE") {
                    text += "\n\n";
                    text += &tr("The system's files are shown only as administrator.");
                }
                self.fx.info(&tr("Registry Size"), &text);
            }
        }
        self.fx.menus();
        self.fx.redraw();
    }

    /// Saves the ticked categories' entries to `.reg` files and deletes them, after asking.
    fn clean(&mut self, dialogs: &mut dyn Dialogs) {
        let work = self.to_clean();
        let n: usize = work.iter().map(|&i| self.cats[i].count()).sum();
        let names: Vec<String> = work.iter().map(|&i| format!("  {} ({})", tr(self.cats[i].name), self.cats[i].count())).collect();
        let text = format!(
            "{}\n\n{}\n\n{}",
            tf("Delete {} registry entries?", &[&n.to_string()]),
            names.join("\n"),
            tr("Each is saved to a .reg file first: opening that file puts it back.")
        );
        if !dialogs.confirm(&tr("Registry Cleaner"), &text) {
            return;
        }
        let stamp = {
            let t = rt_sys::local_time();
            format!("{:04}-{:02}-{:02} {:02}{:02}{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
        };
        let dir = self.copies_dir().join(format!("registry {stamp}"));
        // reg.exe once an entry (the copy, then the delete): in a thread.
        let work: Vec<(Found, String)> = work
            .iter()
            .map(|&i| (Found { entries: self.cats[i].found.clone().unwrap_or_default(), ..Found::default() }, self.cats[i].id.clone()))
            .collect();
        self.cleaning.start(move || {
            let (mut done, mut failed, mut error) = (0u64, 0u64, None);
            for (found, id) in &work {
                let c = registry::clean(found, &dir.join(format!("{id}.reg")));
                done += c.files;
                failed += c.failed;
                if error.is_none() {
                    error = c.error;
                }
            }
            (done, failed, error, dir)
        });
        self.note = Some(tr("Cleaning…"));
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
            Action::Scan => 100.0,
            Action::Clean => 190.0,
            Action::Copies => 180.0,
            Action::Size => 140.0,
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
        let body_y = top + bh + (14.0 * s).round();
        let body_h = (area.bottom() - status_h - body_y - (12.0 * s).round()).max(0.0);
        let list_w = (inner.w * 0.42).clamp((220.0 * s).min(inner.w * 0.6), 380.0 * s);
        let list = Rect::new(inner.x, body_y, list_w, body_h);
        let entries =
            Rect::new(list.right() + (16.0 * s).round(), body_y, (inner.right() - list.right() - (16.0 * s).round()).max(0.0), body_h);
        let status = Rect::new(area.x, area.bottom() - status_h, area.w, status_h);
        Layout { area, actions, list, entries, row_h: (30.0 * s).round(), entry_h: (22.0 * s).round(), status }
    }

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        use rt_core::i18n::strip_mnemonic;
        use rt_gfx::dialog::{ellipsize, ellipsize_middle};
        let l = Self::layout_in(area, s);
        g.fill_rect(area, th.bg);
        let pad = (24.0 * s).round();
        let (x, w) = (area.x + pad, (area.w - 2.0 * pad).max(0.0));
        g.text_in(&tr("Registry Cleaner"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(
            g,
            &tr("Registry entries that name what is gone. Each is saved to a .reg file before it is deleted, to put it back."),
            w,
            12.5 * s,
            FontId::Regular,
        );
        g.text_in(&about, Rect::new(x, area.y + pad + 32.0 * s, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
        for (i, a) in ACTIONS.iter().enumerate() {
            let r = l.actions[i];
            let on = self.enabled(*a);
            let primary = *a == Action::Clean && on;
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
        // The categories.
        let p = 12.5 * s;
        let small = 11.5 * s;
        g.stroke_rrect(l.list, 4.0 * s, 1.0, th.border);
        g.set_clip(Some(l.list));
        let end = (self.scroll + l.page_rows()).min(self.cats.len());
        for i in self.scroll..end {
            let c = &self.cats[i];
            let y = l.list.y + (i - self.scroll) as f32 * l.row_h;
            let row = Rect::new(l.list.x + 1.0, y, l.list.w - 2.0, l.row_h);
            if self.sel == i {
                g.fill_rect(row, th.selection);
            } else if matches!(self.hot, Some(Hit::Row(k) | Hit::Tick(k)) if k == i) {
                g.fill_rect(row, th.hover);
            }
            let bx = Rect::new(l.list.x + 10.0 * s, y + (l.row_h - 16.0 * s) / 2.0, 16.0 * s, 16.0 * s);
            if c.report {
                // No box: a small "i" in a ring.
                g.stroke_rrect(bx, 8.0 * s, 1.2 * s, th.dim);
                g.text_in("i", bx, 11.0 * s, FontId::Semibold, th.dim, Align::Center);
            } else if c.ticked && self.cleanable(i) {
                g.fill_rrect(bx, 3.0 * s, th.accent);
                let k = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
                g.polyline(
                    &[(bx.x + 3.5 * s, bx.y + 8.0 * s), (bx.x + 7.0 * s, bx.y + 11.5 * s), (bx.x + 12.5 * s, bx.y + 4.5 * s)],
                    2.0 * s,
                    k,
                    false,
                );
            } else {
                g.stroke_rrect(bx, 3.0 * s, 1.2 * s, if self.cleanable(i) { th.dim } else { th.border });
            }
            let count_w = 70.0 * s;
            let name_r = Rect::new(bx.right() + 10.0 * s, y, (row.right() - bx.right() - 20.0 * s - count_w).max(0.0), l.row_h);
            let name = ellipsize(g, &tr(c.name), name_r.w, p, FontId::Regular);
            g.text_in(&name, name_r, p, FontId::Regular, if self.cleanable(i) || c.report { th.text } else { th.dim }, Align::Left);
            let count = match &c.found {
                None if self.busy() => "…".to_string(),
                None => String::new(),
                Some(f) => f.len().to_string(),
            };
            let color = if c.count() > 0 && !c.report { th.warn } else { th.dim };
            g.text_in(&count, Rect::new(row.right() - count_w - 8.0 * s, y, count_w, l.row_h), small, FontId::Regular, color, Align::Right);
        }
        g.set_clip(None);
        // The category selected: what it is, and its entries.
        if let Some(c) = self.cats.get(self.sel) {
            let e = l.entries;
            let title = ellipsize(g, &tr(c.name), e.w, p, FontId::Semibold);
            g.text_in(&title, Rect::new(e.x, e.y, e.w, l.row_h * 0.8), p, FontId::Semibold, th.text, Align::Left);
            let mut about = tr(c.about);
            if c.admin && !self.admin && !c.report {
                about = format!("{about} {}", tr("Cleaning it needs administrator rights."));
            }
            let about = ellipsize(g, &about, e.w, small, FontId::Regular);
            g.text_in(&about, Rect::new(e.x, e.y + l.row_h * 0.8, e.w, l.row_h * 0.9), small, FontId::Regular, th.dim, Align::Left);
            let box_r = Rect::new(e.x, e.y + l.row_h * 2.0, e.w, (e.h - l.row_h * 2.0).max(0.0));
            g.stroke_rrect(box_r, 4.0 * s, 1.0, th.border);
            let found = c.found.as_deref().unwrap_or(&[]);
            let message = match &c.found {
                None if self.busy() => Some(tr("Scanning…")),
                None => Some(tr("Not scanned yet: press Scan (F5).")),
                Some(f) if f.is_empty() => Some(tr("Nothing found.")),
                _ => None,
            };
            if let Some(m) = message {
                g.text_in(&m, box_r, p, FontId::Regular, th.dim, Align::Center);
            }
            g.set_clip(Some(box_r));
            let rows = l.entry_rows();
            let first = self.entry_scroll.min(found.len().saturating_sub(1));
            for (k, (entry, why)) in found.iter().enumerate().skip(first).take(rows) {
                let y = box_r.y + 4.0 * s + (k - first) as f32 * l.entry_h;
                let r = Rect::new(box_r.x + 10.0 * s, y, box_r.w - 20.0 * s, l.entry_h);
                let half = r.w * 0.5;
                let t = ellipsize(g, why, half - 8.0 * s, small, FontId::Regular);
                g.text_in(&t, Rect::new(r.x, y, half - 8.0 * s, l.entry_h), small, FontId::Regular, th.text, Align::Left);
                let key = match entry.value.as_deref() {
                    Some("") => format!("{} › (default)", entry.key),
                    Some(v) => format!("{} › {v}", entry.key),
                    None => entry.key.clone(),
                };
                let key = ellipsize_middle(g, &key, half, small, FontId::Regular);
                g.text_in(&key, Rect::new(r.x + half, y, half, l.entry_h), small, FontId::Regular, th.dim, Align::Left);
            }
            g.set_clip(None);
        }
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let found: usize = self.cats.iter().filter(|c| !c.report).map(Category::count).sum();
        let left = self.note.clone().unwrap_or_else(|| {
            if self.busy() {
                tr("Scanning…")
            } else if self.cats.iter().all(|c| c.found.is_none()) {
                tr("Not scanned yet: press Scan (F5).")
            } else {
                let ticked: usize = self.to_clean().iter().map(|&i| self.cats[i].count()).sum();
                tf("Invalid entries: {}. Ticked to clean: {}.", &[&found.to_string(), &ticked.to_string()])
            }
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
        if i >= self.cats.len() {
            return None;
        }
        Some(if x < l.list.x + 34.0 * (l.row_h / 30.0) { Hit::Tick(i) } else { Hit::Row(i) })
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
            Some(Hit::Tick(i)) if self.cleanable(i) => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        };
    }

    pub fn pointer_left(&mut self) {
        if self.hot.take().is_some() {
            self.fx.redraw();
        }
    }

    pub fn release(&mut self) {}

    fn select(&mut self, i: usize) {
        if i != self.sel {
            self.sel = i;
            self.entry_scroll = 0;
        }
    }

    /// A click: a box ticks, a row is selected; a button gives its action for the window to do.
    pub fn press(&mut self) -> Option<Action> {
        self.note = None;
        let out = match self.hot {
            Some(Hit::Act(i)) => Some(ACTIONS[i]),
            Some(Hit::Tick(i)) => {
                self.select(i);
                self.toggle(i);
                None
            }
            Some(Hit::Row(i)) => {
                self.select(i);
                None
            }
            None => None,
        };
        self.fx.menus();
        self.fx.redraw();
        out
    }

    /// The arrows move, Space ticks; F5 scans.
    pub fn key(&mut self, key: &winit::keyboard::Key) {
        use winit::keyboard::{Key, NamedKey};
        let n = self.cats.len();
        if n == 0 {
            return;
        }
        let page = self.layout.page_rows();
        match key {
            Key::Named(NamedKey::F5) => return self.scan(),
            Key::Named(NamedKey::ArrowDown) => self.select((self.sel + 1).min(n - 1)),
            Key::Named(NamedKey::ArrowUp) => self.select(self.sel.saturating_sub(1)),
            Key::Named(NamedKey::Home) => self.select(0),
            Key::Named(NamedKey::End) => self.select(n - 1),
            Key::Named(NamedKey::PageDown) => self.select((self.sel + page).min(n - 1)),
            Key::Named(NamedKey::PageUp) => self.select(self.sel.saturating_sub(page)),
            Key::Named(NamedKey::Space) => return self.toggle(self.sel),
            _ => return,
        }
        if self.sel < self.scroll {
            self.scroll = self.sel;
        } else if self.sel >= self.scroll + page {
            self.scroll = self.sel + 1 - page;
        }
        self.note = None;
        self.fx.menus();
        self.fx.redraw();
    }

    /// The wheel scrolls the entries (over them) or the categories.
    pub fn wheel(&mut self, delta: winit::event::MouseScrollDelta, at: Option<(f32, f32)>) {
        let rows = match delta {
            winit::event::MouseScrollDelta::LineDelta(_, l) => (-l * 3.0).round() as isize,
            winit::event::MouseScrollDelta::PixelDelta(p) => (-p.y as f32 / self.layout.entry_h.max(16.0)).round() as isize,
        };
        if at.is_some_and(|(x, y)| self.layout.entries.contains(x, y)) {
            let n = self.cats.get(self.sel).map_or(0, Category::count);
            let max = n.saturating_sub(self.layout.entry_rows());
            self.entry_scroll = (self.entry_scroll as isize + rows).clamp(0, max as isize) as usize;
        } else {
            let max = self.cats.len().saturating_sub(self.layout.page_rows());
            self.scroll = (self.scroll as isize + rows).clamp(0, max as isize) as usize;
        }
        self.fx.redraw();
    }

    /// The page's nodes for the screen readers: the categories are 100 + their index.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role, Toggled};
        let mut list = Node::new(Role::List);
        list.set_label(tr("Registry Cleaner"));
        let mut rows = Vec::new();
        for (i, c) in self.cats.iter().enumerate() {
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(if c.report { Role::ListItem } else { Role::CheckBox });
            n.set_label(format!("{}: {}", tr(c.name), c.found.as_ref().map_or(String::new(), |f| f.len().to_string())));
            if !c.report {
                n.set_toggled(if c.ticked { Toggled::True } else { Toggled::False });
                n.add_action(accesskit::Action::Click);
            }
            n.set_selected(self.sel == i);
            rows.push(nid);
            nodes.push((nid, n));
        }
        list.set_children(rows);
        nodes.push((NodeId(1), list));
        children.push(NodeId(1));
        Some(NodeId(100 + self.sel as u64))
    }

    /// A screen reader clicked a category: it is selected and ticked or not.
    pub fn accessibility_click(&mut self, i: usize) {
        if i < self.cats.len() {
            self.select(i);
            self.toggle(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(k: &str) -> (Entry, String) {
        (Entry { key: format!("HKEY_CURRENT_USER\\Software\\{k}"), value: None }, format!("{k} → C:\\gone\\{k}.exe"))
    }

    #[test]
    fn categories_and_what_can_be_cleaned() {
        let mut p = RegistryPage::new(false, std::env::temp_dir());
        let n = p.cats.len();
        assert_eq!(p.cats.iter().filter(|c| c.report).count(), 2);
        assert!(!p.enabled(Action::Clean) && p.enabled(Action::Scan));
        // Without rights the system's categories are not ticked nor cleanable; reports never are.
        let user = p.cats.iter().position(|c| c.id == "reg.run").unwrap();
        let system = p.cats.iter().position(|c| c.id == "reg.run-all").unwrap();
        let report = n - 1;
        assert!(p.cleanable(user) && !p.cleanable(system) && !p.cleanable(report));
        assert!(p.cats[user].ticked && !p.cats[system].ticked);
        p.cats[user].found = Some(vec![entry("a"), entry("b")]);
        p.cats[system].found = Some(vec![entry("c")]);
        p.cats[report].found = Some(vec![entry("d")]);
        assert_eq!(p.to_clean(), vec![user]);
        assert!(p.enabled(Action::Clean));
        p.toggle(system);
        assert!(p.fx.take().info.is_some(), "says why");
        p.toggle(user);
        assert!(!p.enabled(Action::Clean));
        // As administrator, every category but the reports.
        let admin = RegistryPage::new(true, std::env::temp_dir());
        assert!(admin.cleanable(system) && !admin.cleanable(report) && admin.cats[system].ticked);
    }

    #[test]
    fn saying_no_changes_nothing() {
        struct No;
        impl Dialogs for No {
            fn confirm(&mut self, _: &str, _: &str) -> bool {
                false
            }
            fn pick_folder(&mut self, _: &str) -> Option<PathBuf> {
                None
            }
            fn save_file(&mut self, _: &str, _: (&str, &str), _: Option<&Path>) -> Option<PathBuf> {
                None
            }
        }
        let dir = std::env::temp_dir().join(format!("rcare-reg-{}", std::process::id()));
        let mut p = RegistryPage::new(false, dir.clone());
        let user = p.cats.iter().position(|c| c.id == "reg.muicache").unwrap();
        p.cats[user].found = Some(vec![entry("x")]);
        p.act(Action::Clean, &mut No);
        assert!(!dir.exists() && p.cats[user].count() == 1 && !p.busy());
    }

    #[test]
    fn layout_and_pointer() {
        let mut p = RegistryPage::new(true, std::env::temp_dir());
        p.layout = RegistryPage::layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        let l = p.layout.clone();
        assert!(l.list.right() < l.entries.x && l.entries.right() <= 224.0 + 956.0);
        p.pointer(l.list.x + 14.0, l.list.y + l.row_h * 2.5, false);
        assert_eq!(p.hot, Some(Hit::Tick(2)));
        let before = p.cats[2].ticked;
        p.press();
        assert_eq!((p.sel, p.cats[2].ticked), (2, !before));
        p.pointer(l.list.x + 120.0, l.list.y + l.row_h * 4.5, false);
        assert_eq!(p.hot, Some(Hit::Row(4)));
        p.pointer(l.actions[3].x + 3.0, l.actions[3].y + 3.0, false);
        assert_eq!(p.press(), Some(Action::Size));
        p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::End));
        assert_eq!(p.sel, p.cats.len() - 1);
        // The narrowest page still has both panes.
        let small = RegistryPage::layout_in(Rect::new(224.0, 0.0, 536.0, 480.0), 1.0);
        assert!(small.list.w > 100.0 && small.entries.w > 60.0);
    }

    #[test]
    fn the_registrys_files() {
        // On Windows at least the user's file is there (Windows' own need administrator rights); elsewhere nothing.
        let h = hives();
        if cfg!(windows) {
            assert!(h.iter().any(|(n, s)| n == "NTUSER.DAT" && s.is_some_and(|s| s > 0)), "{h:?}");
        }
    }
}
