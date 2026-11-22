//! The Startup Optimizer page: what starts with Windows (`startup`), one row
//! each with a switch, where it starts from and what it runs. Turning an
//! item off is what Task Manager does, so it can always be turned on again;
//! nothing is deleted. Items of every user need administrator rights.

use crate::i18n::{tf, tr};
use crate::startup::{self, Entry};
use rt_gfx::gfx::{Align, Color, FontId, Gfx, Rect};
use rt_ui::page::Effects;
use rt_ui::theme::Theme;
use winit::window::CursorIcon;

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A button of the bar (index into `ACTIONS`).
    Action(usize),
    /// A row's switch.
    Switch(usize),
    Row(usize),
}

/// What the buttons above the list do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Off,
    On,
    Open,
    Refresh,
}

pub const ACTIONS: [Action; 4] = [Action::Off, Action::On, Action::Open, Action::Refresh];

impl Action {
    /// The menu label (a translation key with its access key).
    pub fn label_key(self) -> &'static str {
        match self {
            Action::Off => "Turn &Off",
            Action::On => "Turn O&n",
            Action::Open => "Open File &Location",
            Action::Refresh => "&Refresh",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub actions: [Rect; 4],
    pub header: Rect,
    pub list: Rect,
    pub row_h: f32,
    /// Columns: name, where, command (x, width); the switch is before the name.
    pub name: (f32, f32),
    pub place: (f32, f32),
    pub command: (f32, f32),
    pub status: Rect,
}

impl Layout {
    pub fn page_rows(&self) -> usize {
        if self.row_h <= 0.0 {
            return 1;
        }
        ((self.list.h / self.row_h).floor() as usize).max(1)
    }

    fn switch_at(&self, row_y: f32, s: f32) -> Rect {
        Rect::new(self.list.x + 12.0 * s, row_y + (self.row_h - 18.0 * s) / 2.0, 34.0 * s, 18.0 * s)
    }
}

#[derive(Default)]
pub struct StartupPage {
    pub entries: Vec<Entry>,
    pub sel: Option<usize>,
    pub scroll: usize,
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    /// Running with administrator rights: the items of every user can be changed.
    pub admin: bool,
    /// The list was read at least once.
    pub read: bool,
    /// A line for the status bar (what was just done).
    pub note: Option<String>,
    scale: f32,
}

impl StartupPage {
    pub fn new(admin: bool) -> StartupPage {
        StartupPage { admin, scale: 1.0, ..StartupPage::default() }
    }

    /// Reads the list again, keeping the selection on the same item.
    pub fn refresh(&mut self) {
        let keep = self.sel.and_then(|i| self.entries.get(i)).map(|e| (e.place, e.name.clone()));
        self.entries = startup::read();
        self.sel = keep.and_then(|(p, n)| self.entries.iter().position(|e| e.place == p && e.name == n));
        self.scroll = self.scroll.min(self.entries.len().saturating_sub(1));
        self.read = true;
        self.fx.menus();
        self.fx.redraw();
    }

    /// Whether this item can be changed from here now.
    pub fn changeable(&self, i: usize) -> bool {
        self.entries.get(i).is_some_and(|e| self.admin || !e.place.machine())
    }

    pub fn enabled(&self, a: Action) -> bool {
        let sel = self.sel.and_then(|i| self.entries.get(i).map(|e| (i, e)));
        match a {
            Action::Off => sel.is_some_and(|(i, e)| e.enabled && self.changeable(i)),
            Action::On => sel.is_some_and(|(i, e)| !e.enabled && self.changeable(i)),
            Action::Open => sel.is_some_and(|(_, e)| e.file.as_ref().is_some_and(|f| f.parent().is_some_and(|d| d.is_dir()))),
            Action::Refresh => true,
        }
    }

    /// Turns the item `i` on or off; says why not when it cannot.
    pub fn set(&mut self, i: usize, on: bool) {
        let Some(e) = self.entries.get(i).cloned() else { return };
        if e.enabled == on {
            return;
        }
        if !self.changeable(i) {
            self.fx.info(
                &tr("Startup Optimizer"),
                &tr("Changing the items of every user needs administrator rights: restart rcare as administrator (File menu)."),
            );
            return;
        }
        match startup::set_enabled(&e, on) {
            Ok(()) => {
                self.entries[i].enabled = on;
                let name = e.shown_name().to_string();
                self.note = Some(if on {
                    tf("“{}” will start with Windows.", &[&name])
                } else {
                    tf("“{}” will not start with Windows.", &[&name])
                });
            }
            Err(why) => self.fx.info(&tr("Startup Optimizer"), &tf("Could not change “{}”: {}", &[e.shown_name(), &why])),
        }
        self.fx.menus();
        self.fx.redraw();
    }

    /// Does what a button or its menu entry asks.
    pub fn act(&mut self, a: Action) {
        if !self.enabled(a) {
            return;
        }
        match a {
            Action::Off | Action::On => {
                if let Some(i) = self.sel {
                    self.set(i, a == Action::On);
                }
            }
            Action::Open => {
                if let Some(f) = self.sel.and_then(|i| self.entries.get(i)).and_then(|e| e.file.clone()) {
                    rt_sys::reveal(&f);
                }
            }
            Action::Refresh => self.refresh(),
        }
    }

    // ------------------------------------------------------------ drawing

    pub fn layout_in(&self, area: Rect, s: f32) -> Layout {
        let pad = (24.0 * s).round();
        let inner = Rect::new(area.x + pad, area.y + pad, (area.w - 2.0 * pad).max(0.0), (area.h - pad).max(0.0));
        let bh = (30.0 * s).round();
        let top = inner.y + (62.0 * s).round();
        let mut x = inner.x;
        let mut actions = [Rect::default(); 4];
        let wants = ACTIONS.map(|a| match a {
            Action::Open => 170.0,
            Action::Refresh => 110.0,
            _ => 120.0,
        });
        // On a narrow page the buttons shrink together to fit.
        let gap = 8.0 * s;
        let fit = ((inner.w - 3.0 * gap) / (wants.iter().sum::<f32>() * s)).min(1.0);
        for (i, want) in wants.iter().enumerate() {
            let w = (want * s * fit).floor();
            actions[i] = Rect::new(x, top, w, bh);
            x += w + gap;
        }
        let header = Rect::new(inner.x, top + bh + (14.0 * s).round(), inner.w, (26.0 * s).round());
        let status_h = (24.0 * s).round();
        let list = Rect::new(inner.x, header.bottom(), inner.w, (area.bottom() - status_h - header.bottom()).max(0.0));
        let status = Rect::new(area.x, area.bottom() - status_h, area.w, status_h);
        let switch_w = 58.0 * s;
        let rest = (inner.w - switch_w).max(0.0);
        let name = (inner.x + switch_w, (rest * 0.28).max(120.0 * s).min(rest));
        let place = (name.0 + name.1, (rest * 0.22).min(rest - name.1).max(0.0));
        let command = (place.0 + place.1, (inner.right() - place.0 - place.1).max(0.0));
        Layout { area, actions, header, list, row_h: (30.0 * s).round(), name, place, command, status }
    }

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        use rt_core::i18n::strip_mnemonic;
        use rt_gfx::dialog::ellipsize;
        self.scale = s;
        let l = self.layout_in(area, s);
        g.fill_rect(area, th.bg);
        let pad = (24.0 * s).round();
        let (x, w) = (area.x + pad, (area.w - 2.0 * pad).max(0.0));
        g.text_in(&tr("Startup Optimizer"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(
            g,
            &tr("What starts with Windows. Turning an item off is what Task Manager does: it can be turned on again, and nothing is deleted."),
            w,
            12.5 * s,
            FontId::Regular,
        );
        g.text_in(&about, Rect::new(x, area.y + pad + 32.0 * s, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
        for (i, a) in ACTIONS.iter().enumerate() {
            let r = l.actions[i];
            let on = self.enabled(*a);
            g.fill_rrect(r, 5.0 * s, th.bg);
            if on && self.hot == Some(Hit::Action(i)) {
                g.fill_rrect(r, 5.0 * s, th.hover);
            }
            g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
            let label = ellipsize(g, &strip_mnemonic(&tr(a.label_key())), r.w - 10.0 * s, 12.5 * s, FontId::Regular);
            g.text_in(&label, r, 12.5 * s, FontId::Regular, if on { th.text } else { th.dim }, Align::Center);
        }
        // Header.
        let p = 12.5 * s;
        let hd = l.header;
        g.fill_rect(hd, th.header);
        g.fill_rect(Rect::new(hd.x, hd.bottom() - 1.0, hd.w, 1.0), th.border);
        for (title, (cx, cw)) in [("Name", l.name), ("Where", l.place), ("Command", l.command)] {
            let r = Rect::new(cx + 6.0 * s, hd.y, (cw - 12.0 * s).max(0.0), hd.h);
            let t = ellipsize(g, &tr(title), r.w, p, FontId::Semibold);
            g.text_in(&t, r, p, FontId::Semibold, th.dim, Align::Left);
        }
        // Rows.
        let page = l.page_rows();
        let end = (self.scroll + page).min(self.entries.len());
        g.set_clip(Some(l.list));
        for i in self.scroll..end {
            let e = &self.entries[i];
            let y = l.list.y + (i - self.scroll) as f32 * l.row_h;
            let row = Rect::new(l.list.x, y, l.list.w, l.row_h);
            if self.sel == Some(i) {
                g.fill_rect(row, th.selection);
            } else if self.hot == Some(Hit::Row(i)) || self.hot == Some(Hit::Switch(i)) {
                g.fill_rect(row, th.hover);
            }
            // The switch.
            let sw = l.switch_at(y, s);
            let can = self.changeable(i);
            let track = if e.enabled { th.accent } else { th.bar_track };
            g.fill_rrect(sw, sw.h / 2.0, if can { track } else { track.mix(th.bg, 0.5) });
            let knob = sw.h - 6.0 * s;
            let kx = if e.enabled { sw.right() - knob - 3.0 * s } else { sw.x + 3.0 * s };
            g.fill_circle(kx + knob / 2.0, sw.y + sw.h / 2.0, knob / 2.0, Color::rgb(0xFFFFFF));
            let dim_if_off = if e.enabled { th.text } else { th.dim };
            let name_r = Rect::new(l.name.0 + 6.0 * s, y, (l.name.1 - 12.0 * s).max(0.0), l.row_h);
            let name = ellipsize(g, e.shown_name(), name_r.w, p, FontId::Regular);
            g.text_in(&name, name_r, p, FontId::Regular, dim_if_off, Align::Left);
            let place_r = Rect::new(l.place.0 + 6.0 * s, y, (l.place.1 - 12.0 * s).max(0.0), l.row_h);
            let place = ellipsize(g, &tr(e.place.label()), place_r.w, 11.5 * s, FontId::Regular);
            g.text_in(&place, place_r, 11.5 * s, FontId::Regular, th.dim, Align::Left);
            let cmd_r = Rect::new(l.command.0 + 6.0 * s, y, (l.command.1 - 12.0 * s).max(0.0), l.row_h);
            let (text, color) = if e.missing {
                (format!("{} — {}", tr("Program not found"), e.command), th.warn)
            } else if !can {
                (format!("{} — {}", tr("Needs administrator rights"), e.command), th.dim)
            } else {
                (e.command.clone(), th.dim)
            };
            let text = rt_gfx::dialog::ellipsize_middle(g, &text, cmd_r.w, 11.5 * s, FontId::Regular);
            g.text_in(&text, cmd_r, 11.5 * s, FontId::Regular, color, Align::Left);
        }
        g.set_clip(None);
        if self.read && self.entries.is_empty() {
            g.text_in(&tr("Nothing starts with Windows."), l.list, p, FontId::Regular, th.dim, Align::Center);
        }
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let off = self.entries.iter().filter(|e| !e.enabled).count();
        let left = self.note.clone().unwrap_or_else(|| {
            tf("Start with Windows: {}. Turned off: {}.", &[&self.entries.len().saturating_sub(off).to_string(), &off.to_string()])
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
            return Some(Hit::Action(i));
        }
        if !l.list.contains(x, y) || l.row_h <= 0.0 {
            return None;
        }
        let i = self.scroll + ((y - l.list.y) / l.row_h) as usize;
        if i >= self.entries.len() {
            return None;
        }
        let row_y = l.list.y + (i - self.scroll) as f32 * l.row_h;
        Some(if l.switch_at(row_y, self.scale).contains(x, y) { Hit::Switch(i) } else { Hit::Row(i) })
    }

    /// What the pointer is on; `blocked` while a message is open.
    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        let hot = if blocked { None } else { self.hit(x, y) };
        if hot != self.hot {
            self.hot = hot;
            self.fx.redraw();
        }
        self.cursor_icon = match hot {
            Some(Hit::Action(i)) if self.enabled(ACTIONS[i]) => CursorIcon::Pointer,
            Some(Hit::Switch(i)) if self.changeable(i) => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        };
    }

    pub fn pointer_left(&mut self) {
        if self.hot.take().is_some() {
            self.fx.redraw();
        }
    }

    pub fn release(&mut self) {}

    /// A click: a button acts, a switch turns its item on or off, a row is selected.
    pub fn press(&mut self) {
        self.note = None;
        match self.hot {
            Some(Hit::Action(i)) => self.act(ACTIONS[i]),
            Some(Hit::Switch(i)) => {
                self.sel = Some(i);
                let on = !self.entries[i].enabled;
                self.set(i, on);
            }
            Some(Hit::Row(i)) => self.sel = Some(i),
            None => {}
        }
        self.fx.menus();
        self.fx.redraw();
    }

    /// The arrows move, Space turns the selected item on or off; F5 reads again.
    pub fn key(&mut self, key: &winit::keyboard::Key) {
        use winit::keyboard::{Key, NamedKey};
        let n = self.entries.len();
        if n == 0 && !matches!(key, Key::Named(NamedKey::F5)) {
            return;
        }
        let page = self.layout.page_rows() as isize;
        let cur = self.sel.map_or(-1, |i| i as isize);
        let to = |d: isize| (cur + d).clamp(0, n as isize - 1) as usize;
        match key {
            Key::Named(NamedKey::ArrowDown) => self.sel = Some(to(1)),
            Key::Named(NamedKey::ArrowUp) => self.sel = Some(to(-1)),
            Key::Named(NamedKey::PageDown) => self.sel = Some(to(page)),
            Key::Named(NamedKey::PageUp) => self.sel = Some(to(-page)),
            Key::Named(NamedKey::Home) => self.sel = Some(0),
            Key::Named(NamedKey::End) => self.sel = Some(n - 1),
            Key::Named(NamedKey::Space) => {
                if let Some(i) = self.sel {
                    let on = !self.entries[i].enabled;
                    self.set(i, on);
                }
                return;
            }
            Key::Named(NamedKey::F5) => return self.refresh(),
            _ => return,
        }
        // Keep the selection in view.
        if let Some(i) = self.sel {
            if i < self.scroll {
                self.scroll = i;
            } else if i >= self.scroll + self.layout.page_rows() {
                self.scroll = i + 1 - self.layout.page_rows();
            }
        }
        self.note = None;
        self.fx.menus();
        self.fx.redraw();
    }

    /// The mouse wheel scrolls the list.
    pub fn wheel(&mut self, delta: winit::event::MouseScrollDelta) {
        let rows = match delta {
            winit::event::MouseScrollDelta::LineDelta(_, l) => (-l * 3.0).round() as isize,
            winit::event::MouseScrollDelta::PixelDelta(p) => (-p.y as f32 / self.layout.row_h.max(20.0)).round() as isize,
        };
        let max = self.entries.len().saturating_sub(self.layout.page_rows());
        self.scroll = (self.scroll as isize + rows).clamp(0, max as isize) as usize;
        self.fx.redraw();
    }

    /// The page's nodes for the screen readers: the rows are 100 + their index.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role, Toggled};
        let mut focus = None;
        let mut list = Node::new(Role::List);
        list.set_label(tr("Startup Optimizer"));
        let mut rows = Vec::new();
        let end = (self.scroll + self.layout.page_rows()).min(self.entries.len());
        for i in self.scroll..end {
            let e = &self.entries[i];
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::CheckBox);
            n.set_label(format!("{}, {}", e.shown_name(), tr(e.place.label())));
            n.set_toggled(if e.enabled { Toggled::True } else { Toggled::False });
            n.set_selected(self.sel == Some(i));
            n.add_action(accesskit::Action::Click);
            if self.sel == Some(i) {
                focus = Some(nid);
            }
            rows.push(nid);
            nodes.push((nid, n));
        }
        list.set_children(rows);
        nodes.push((NodeId(1), list));
        children.push(NodeId(1));
        focus
    }

    /// A screen reader clicked the row `i`: it is turned on or off.
    pub fn accessibility_click(&mut self, i: usize) {
        if i < self.entries.len() {
            self.sel = Some(i);
            let on = !self.entries[i].enabled;
            self.set(i, on);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::Place;

    fn entry(name: &str, place: Place, enabled: bool) -> Entry {
        Entry { name: name.into(), command: format!("C:\\{name}.exe"), file: None, place, enabled, missing: false }
    }

    #[test]
    fn what_can_be_done() {
        let mut p = StartupPage::new(false);
        p.entries = vec![entry("a", Place::UserRun, true), entry("b", Place::MachineRun, true), entry("c", Place::UserFolder, false)];
        assert!(!p.enabled(Action::Off) && p.enabled(Action::Refresh));
        p.sel = Some(0);
        assert!(p.enabled(Action::Off) && !p.enabled(Action::On));
        // An item of every user, without administrator rights: not changeable, and it says why.
        p.sel = Some(1);
        assert!(!p.changeable(1) && !p.enabled(Action::Off));
        p.set(1, false);
        assert!(p.entries[1].enabled && p.fx.take().info.is_some());
        p.sel = Some(2);
        assert!(p.enabled(Action::On));
        // As administrator, the items of every user can be changed too.
        p.admin = true;
        assert!(p.changeable(1));
    }

    #[test]
    fn rows_are_found_under_the_pointer() {
        let mut p = StartupPage::new(true);
        p.entries = (0..30).map(|i| entry(&format!("e{i}"), Place::UserRun, i % 2 == 0)).collect();
        p.layout = p.layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        let l = p.layout.clone();
        let y = l.list.y + l.row_h * 2.5;
        assert_eq!(p.hit(l.name.0 + 20.0, y), Some(Hit::Row(2)));
        let sw = l.switch_at(l.list.y + l.row_h * 2.0, 1.0);
        assert_eq!(p.hit(sw.x + 2.0, sw.y + 2.0), Some(Hit::Switch(2)));
        assert_eq!(p.hit(l.actions[3].x + 2.0, l.actions[3].y + 2.0), Some(Hit::Action(3)));
        p.sel = Some(0);
        p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::End));
        assert_eq!(p.sel, Some(29));
        assert!(p.scroll + p.layout.page_rows() > 29);
    }
}
