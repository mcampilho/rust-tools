//! The Privacy Sweep page: Windows' traces (`privacy`), one row each with a
//! box, what it is and how many entries it has. Sweeping asks with the list;
//! registry traces are saved to `backups\privacy <date>\<id>.reg` first.

use crate::i18n::{tf, tr};
use crate::privacy::{self, Found, Trace};
use rt_gfx::gfx::{Align, Color, FontId, Gfx, Rect};
use rt_ui::page::{Dialogs, Effects};
use rt_ui::theme::Theme;
use std::path::PathBuf;
use winit::window::CursorIcon;

/// What the page does (its buttons and menu entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Scan,
    Sweep,
    Copies,
}

pub const ACTIONS: [Action; 3] = [Action::Scan, Action::Sweep, Action::Copies];

impl Action {
    /// The menu label (a translation key with its access key).
    pub fn label_key(self) -> &'static str {
        match self {
            Action::Scan => "&Scan",
            Action::Sweep => "S&weep the Ticked Traces...",
            Action::Copies => "Open the C&opies Folder",
        }
    }
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
    pub actions: [Rect; 3],
    pub list: Rect,
    pub row_h: f32,
    pub status: Rect,
}

/// A trace, ticked or not, and what was found.
#[derive(Clone, Debug)]
pub struct Row {
    pub trace: Trace,
    pub ticked: bool,
    pub found: Option<Found>,
}

#[derive(Default)]
pub struct PrivacyPage {
    pub rows: Vec<Row>,
    pub sel: usize,
    pub data_dir: PathBuf,
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    pub note: Option<String>,
    /// Counting the traces, in a thread (one result a row).
    scanning: crate::bg::Job<Vec<Found>>,
    /// Sweeping: (done, failed, the first error, the copies' folder).
    sweeping: crate::bg::Job<(usize, usize, Option<String>, PathBuf)>,
}

impl PrivacyPage {
    pub fn new(data_dir: PathBuf) -> PrivacyPage {
        let rows = privacy::recent_dir()
            .map(|r| privacy::traces(&r))
            .unwrap_or_default()
            .into_iter()
            .map(|trace| Row { ticked: trace.default_on, trace, found: None })
            .collect();
        PrivacyPage { rows, data_dir, ..PrivacyPage::default() }
    }

    pub fn copies_dir(&self) -> PathBuf {
        self.data_dir.join("backups")
    }

    /// Looks at every trace again, in a thread (folders and a few keys: quick, unless Windows is busy).
    pub fn scan(&mut self) {
        if self.sweeping.busy() {
            return;
        }
        let traces: Vec<Trace> = self.rows.iter().map(|r| r.trace.clone()).collect();
        self.scanning.start(move || traces.iter().map(privacy::measure).collect());
        self.fx.menus();
        self.fx.redraw();
    }

    pub fn busy(&self) -> bool {
        self.scanning.busy() || self.sweeping.busy()
    }

    /// While counting or sweeping: takes what came, and when to look again.
    pub fn animate(&mut self) -> Option<std::time::Instant> {
        if let Some(found) = self.scanning.take() {
            for (r, f) in self.rows.iter_mut().zip(found) {
                r.found = Some(f);
            }
            self.fx.menus();
            self.fx.redraw();
        }
        if let Some((done, failed, error, dir)) = self.sweeping.take() {
            let mut text = tr("The ticked traces were swept.");
            if failed > 0 {
                text += "\n\n";
                text += &tf("{} could not be removed (in use?): {}", &[&failed.to_string(), error.as_deref().unwrap_or("")]);
            }
            if dir.is_dir() {
                text += "\n\n";
                text += &tf(
                    "The registry entries were saved in {} first: open the .reg files there to put them back.",
                    &[&dir.display().to_string()],
                );
            }
            self.fx.info(&tr("Privacy Sweep"), &text);
            self.note = Some(tf("{} traces removed.", &[&done.to_string()]));
            self.scan();
        }
        self.busy().then(crate::bg::soon)
    }

    /// Whether a row has something to sweep.
    fn has(r: &Row) -> bool {
        r.found.as_ref().is_some_and(|f| f.count > 0 || f.clipboard)
    }

    fn to_sweep(&self) -> Vec<usize> {
        (0..self.rows.len()).filter(|&i| self.rows[i].ticked && Self::has(&self.rows[i])).collect()
    }

    pub fn enabled(&self, a: Action) -> bool {
        match a {
            Action::Scan => !self.busy(),
            Action::Sweep => !self.busy() && !self.to_sweep().is_empty(),
            Action::Copies => self.copies_dir().is_dir(),
        }
    }

    fn count_text(r: &Row) -> String {
        match &r.found {
            None => String::new(),
            Some(f) if f.clipboard => "–".into(),
            Some(f) => f.count.to_string(),
        }
    }

    /// Does what a button or its menu entry asks.
    pub fn act(&mut self, a: Action, dialogs: &mut dyn Dialogs) {
        if !self.enabled(a) {
            return;
        }
        match a {
            Action::Scan => self.scan(),
            Action::Copies => rt_sys::reveal(&self.copies_dir()),
            Action::Sweep => {
                let work = self.to_sweep();
                let list: Vec<String> = work
                    .iter()
                    .map(|&i| {
                        let r = &self.rows[i];
                        let n = Self::count_text(r);
                        if n == "–" {
                            format!("  {}", tr(r.trace.name))
                        } else {
                            format!("  {} ({n})", tr(r.trace.name))
                        }
                    })
                    .collect();
                let text = format!(
                    "{}\n\n{}\n\n{}",
                    tr("Sweep these traces?"),
                    list.join("\n"),
                    tr("Registry traces are saved to a .reg file first: opening it puts them back. Files go for good.")
                );
                if !dialogs.confirm(&tr("Privacy Sweep"), &text) {
                    return;
                }
                let stamp = {
                    let t = rt_sys::local_time();
                    format!("{:04}-{:02}-{:02} {:02}{:02}{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
                };
                let dir = self.copies_dir().join(format!("privacy {stamp}"));
                // reg.exe once a key, PowerShell for the clipboard: in a thread.
                let work: Vec<(Found, String)> =
                    work.iter().filter_map(|&i| self.rows[i].found.clone().map(|f| (f, self.rows[i].trace.id.to_string()))).collect();
                self.sweeping.start(move || {
                    let (mut done, mut failed, mut error) = (0, 0, None);
                    for (f, id) in &work {
                        let s = privacy::sweep(f, &dir.join(format!("{id}.reg")));
                        done += s.done;
                        failed += s.failed;
                        if error.is_none() {
                            error = s.error;
                        }
                    }
                    (done, failed, error, dir)
                });
                self.note = Some(tr("Sweeping…"));
            }
        }
        self.fx.menus();
        self.fx.redraw();
    }

    fn toggle(&mut self, i: usize) {
        if let Some(r) = self.rows.get_mut(i) {
            r.ticked = !r.ticked;
            self.fx.menus();
            self.fx.redraw();
        }
    }

    // ------------------------------------------------------------ drawing

    pub fn layout_in(area: Rect, s: f32) -> Layout {
        let pad = (24.0 * s).round();
        let inner = Rect::new(area.x + pad, area.y + pad, (area.w - 2.0 * pad).max(0.0), (area.h - pad).max(0.0));
        let bh = (30.0 * s).round();
        let top = inner.y + (62.0 * s).round();
        let wants = ACTIONS.map(|a| match a {
            Action::Scan => 100.0,
            Action::Sweep => 200.0,
            Action::Copies => 180.0,
        });
        let gap = 8.0 * s;
        let fit = ((inner.w - 2.0 * gap) / (wants.iter().sum::<f32>() * s)).min(1.0);
        let mut actions = [Rect::default(); 3];
        let mut x = inner.x;
        for (i, want) in wants.iter().enumerate() {
            let w = (want * s * fit).floor();
            actions[i] = Rect::new(x, top, w, bh);
            x += w + gap;
        }
        let status_h = (24.0 * s).round();
        let y = top + bh + (16.0 * s).round();
        let list = Rect::new(inner.x, y, inner.w, (area.bottom() - status_h - (12.0 * s).round() - y).max(0.0));
        Layout { area, actions, list, row_h: (52.0 * s).round(), status: Rect::new(area.x, area.bottom() - status_h, area.w, status_h) }
    }

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        use rt_core::i18n::strip_mnemonic;
        use rt_gfx::dialog::ellipsize;
        let l = Self::layout_in(area, s);
        g.fill_rect(area, th.bg);
        let pad = (24.0 * s).round();
        let (x, w) = (area.x + pad, (area.w - 2.0 * pad).max(0.0));
        g.text_in(&tr("Privacy Sweep"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(
            g,
            &tr("The traces Windows keeps of what was done. Never the browsers' history, cookies or passwords."),
            w,
            12.5 * s,
            FontId::Regular,
        );
        g.text_in(&about, Rect::new(x, area.y + pad + 32.0 * s, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
        for (i, a) in ACTIONS.iter().enumerate() {
            let r = l.actions[i];
            let on = self.enabled(*a);
            let primary = *a == Action::Sweep && on;
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
        g.stroke_rrect(l.list, 4.0 * s, 1.0, th.border);
        g.set_clip(Some(l.list));
        for (i, row) in self.rows.iter().enumerate() {
            let y = l.list.y + i as f32 * l.row_h;
            let r = Rect::new(l.list.x + 1.0, y, l.list.w - 2.0, l.row_h);
            if self.sel == i {
                g.fill_rect(r, th.selection);
            } else if matches!(self.hot, Some(Hit::Row(k) | Hit::Tick(k)) if k == i) {
                g.fill_rect(r, th.hover);
            }
            let bx = Rect::new(r.x + 12.0 * s, y + (l.row_h - 16.0 * s) / 2.0, 16.0 * s, 16.0 * s);
            if row.ticked {
                g.fill_rrect(bx, 3.0 * s, th.accent);
                let k = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
                g.polyline(
                    &[(bx.x + 3.5 * s, bx.y + 8.0 * s), (bx.x + 7.0 * s, bx.y + 11.5 * s), (bx.x + 12.5 * s, bx.y + 4.5 * s)],
                    2.0 * s,
                    k,
                    false,
                );
            } else {
                g.stroke_rrect(bx, 3.0 * s, 1.2 * s, th.dim);
            }
            let count_w = 90.0 * s;
            let tx = bx.right() + 12.0 * s;
            let tw = (r.right() - tx - count_w - 12.0 * s).max(0.0);
            let name = ellipsize(g, &tr(row.trace.name), tw, 13.0 * s, FontId::Semibold);
            g.text_in(&name, Rect::new(tx, y + 6.0 * s, tw, 20.0 * s), 13.0 * s, FontId::Semibold, th.text, Align::Left);
            let ab = ellipsize(g, &tr(row.trace.about), tw, 11.5 * s, FontId::Regular);
            g.text_in(&ab, Rect::new(tx, y + 26.0 * s, tw, 20.0 * s), 11.5 * s, FontId::Regular, th.dim, Align::Left);
            let n = Self::count_text(row);
            let color = if Self::has(row) && n != "–" { th.warn } else { th.dim };
            g.text_in(&n, Rect::new(r.right() - count_w - 12.0 * s, y, count_w, l.row_h), 12.5 * s, FontId::Regular, color, Align::Right);
        }
        g.set_clip(None);
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let left = self.note.clone().unwrap_or_else(|| {
            if self.rows.iter().all(|r| r.found.is_none()) {
                tr("Not scanned yet: press Scan (F5).")
            } else {
                let n: usize = self.to_sweep().iter().filter_map(|&i| self.rows[i].found.as_ref()).map(|f| f.count).sum();
                tf("Traces ticked to sweep: {}.", &[&n.to_string()])
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
        let i = ((y - l.list.y) / l.row_h) as usize;
        if i >= self.rows.len() {
            return None;
        }
        Some(if x < l.list.x + 40.0 * (l.row_h / 52.0) { Hit::Tick(i) } else { Hit::Row(i) })
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
            Some(Hit::Tick(_)) => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        };
    }

    pub fn pointer_left(&mut self) {
        if self.hot.take().is_some() {
            self.fx.redraw();
        }
    }

    pub fn release(&mut self) {}

    /// A click: a box ticks, a row is selected; a button gives its action for the window to do.
    pub fn press(&mut self) -> Option<Action> {
        self.note = None;
        let out = match self.hot {
            Some(Hit::Act(i)) => Some(ACTIONS[i]),
            Some(Hit::Tick(i)) => {
                self.sel = i;
                self.toggle(i);
                None
            }
            Some(Hit::Row(i)) => {
                self.sel = i;
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
        let n = self.rows.len();
        match key {
            Key::Named(NamedKey::F5) => return self.scan(),
            Key::Named(NamedKey::ArrowDown) if n > 0 => self.sel = (self.sel + 1).min(n - 1),
            Key::Named(NamedKey::ArrowUp) => self.sel = self.sel.saturating_sub(1),
            Key::Named(NamedKey::Space) => return self.toggle(self.sel),
            _ => return,
        }
        self.note = None;
        self.fx.menus();
        self.fx.redraw();
    }

    /// The page's nodes for the screen readers: the traces are 100 + their index.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role, Toggled};
        let mut list = Node::new(Role::List);
        list.set_label(tr("Privacy Sweep"));
        let mut ids = Vec::new();
        for (i, r) in self.rows.iter().enumerate() {
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::CheckBox);
            n.set_label(format!("{}: {}", tr(r.trace.name), Self::count_text(r)));
            n.set_toggled(if r.ticked { Toggled::True } else { Toggled::False });
            n.set_selected(self.sel == i);
            n.add_action(accesskit::Action::Click);
            ids.push(nid);
            nodes.push((nid, n));
        }
        list.set_children(ids);
        nodes.push((NodeId(1), list));
        children.push(NodeId(1));
        (!self.rows.is_empty()).then(|| NodeId(100 + self.sel as u64))
    }

    /// A screen reader clicked a trace: it is ticked or not.
    pub fn accessibility_click(&mut self, i: usize) {
        self.sel = i.min(self.rows.len().saturating_sub(1));
        self.toggle(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::Place;

    fn page_with(dir: &std::path::Path) -> PrivacyPage {
        let mut p = PrivacyPage::new(dir.to_path_buf());
        p.rows = privacy::traces(dir).into_iter().map(|trace| Row { ticked: trace.default_on, trace, found: None }).collect();
        p
    }

    #[test]
    fn ticks_and_what_can_be_swept() {
        let dir = std::env::temp_dir().join(format!("rcare-privpage-{}", std::process::id()));
        let mut p = page_with(&dir);
        assert!(!p.enabled(Action::Sweep) && p.enabled(Action::Scan));
        p.rows[0].found = Some(Found { count: 3, ..Found::default() });
        p.rows[5].found = Some(Found { clipboard: true, ..Found::default() });
        assert_eq!(p.to_sweep(), vec![0], "the clipboard is not ticked unless asked");
        p.toggle(5);
        assert_eq!(p.to_sweep(), vec![0, 5]);
        assert_eq!(PrivacyPage::count_text(&p.rows[5]), "–");
        struct No;
        impl Dialogs for No {
            fn confirm(&mut self, _: &str, _: &str) -> bool {
                false
            }
            fn pick_folder(&mut self, _: &str) -> Option<PathBuf> {
                None
            }
            fn save_file(&mut self, _: &str, _: (&str, &str), _: Option<&std::path::Path>) -> Option<PathBuf> {
                None
            }
            fn recycle(&mut self, _: &std::path::Path) -> Result<(), String> {
                Ok(())
            }
        }
        p.act(Action::Sweep, &mut No);
        assert!(!dir.exists() && p.rows[0].found.as_ref().unwrap().count == 3, "said no: nothing done");
        assert!(matches!(p.rows[4].trace.places[0], Place::Files(_)));
    }

    #[test]
    fn rows_under_the_pointer() {
        let mut p = page_with(&std::env::temp_dir());
        p.layout = PrivacyPage::layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        let l = p.layout.clone();
        p.pointer(l.list.x + 14.0, l.list.y + l.row_h * 4.5, false);
        assert_eq!(p.hot, Some(Hit::Tick(4)));
        let before = p.rows[4].ticked;
        assert_eq!(p.press(), None);
        assert_eq!((p.sel, p.rows[4].ticked), (4, !before));
        p.pointer(l.actions[1].x + 3.0, l.actions[1].y + 3.0, false);
        assert_eq!(p.press(), Some(Action::Sweep));
        p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowDown));
        assert_eq!(p.sel, 5);
    }
}
