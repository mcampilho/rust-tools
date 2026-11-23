//! The Context Menu Manager page: what programs add to Explorer's
//! right-click menu (`ctxmenu`), one row each with a switch, where it shows
//! and whose it is. Turning an entry off writes a value Explorer honours
//! (nothing is deleted), so it can always be turned on again. Shell
//! extensions, and the commands of every user, need administrator rights.

use crate::ctxmenu::{self, Item};
use crate::i18n::{tf, tr};
use rt_gfx::gfx::{Align, Color, FontId, Gfx, Rect};
use rt_ui::page::{Dialogs, Effects};
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
    /// Columns: name, where, whose (x, width); the switch is before the name.
    pub name: (f32, f32),
    pub place: (f32, f32),
    pub program: (f32, f32),
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
pub struct CtxMenuPage {
    pub items: Vec<Item>,
    pub sel: Option<usize>,
    pub scroll: usize,
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    /// Running with administrator rights: everything can be changed.
    pub admin: bool,
    /// The list was read at least once.
    pub read: bool,
    /// A line for the status bar (what was just done).
    pub note: Option<String>,
    scale: f32,
    /// Reading the menu's entries, in a thread.
    loading: crate::bg::Job<Vec<Item>>,
}

impl CtxMenuPage {
    pub fn new(admin: bool) -> CtxMenuPage {
        CtxMenuPage { admin, scale: 1.0, ..CtxMenuPage::default() }
    }

    /// Reads the list again in a thread; `animate` takes it, keeping the selection on the same entry.
    pub fn refresh(&mut self) {
        self.loading.start(ctxmenu::read);
        self.fx.redraw();
    }

    /// While reading: takes the list when it comes, and when to look again.
    pub fn animate(&mut self) -> Option<std::time::Instant> {
        if let Some(list) = self.loading.take() {
            let keep = self.sel.and_then(|i| self.items.get(i)).map(|e| e.how.clone());
            self.items = list;
            self.sel = keep.and_then(|h| self.items.iter().position(|e| e.how == h));
            self.scroll = self.scroll.min(self.items.len().saturating_sub(1));
            self.read = true;
            self.fx.menus();
            self.fx.redraw();
        }
        self.loading.busy().then(crate::bg::soon)
    }

    /// Whether this entry can be changed from here now.
    pub fn changeable(&self, i: usize) -> bool {
        self.items.get(i).is_some_and(|e| self.admin || !e.machine())
    }

    pub fn enabled(&self, a: Action) -> bool {
        let sel = self.sel.and_then(|i| self.items.get(i).map(|e| (i, e)));
        match a {
            Action::Off => sel.is_some_and(|(i, e)| e.enabled && self.changeable(i)),
            Action::On => sel.is_some_and(|(i, e)| !e.enabled && self.changeable(i)),
            Action::Open => sel.is_some_and(|(_, e)| e.program.as_ref().is_some_and(|f| f.parent().is_some_and(|d| d.is_dir()))),
            Action::Refresh => !self.loading.busy(),
        }
    }

    /// Turns the entry `i` on or off; says why not when it cannot.
    pub fn set(&mut self, i: usize, on: bool) {
        let Some(e) = self.items.get(i).cloned() else { return };
        if e.enabled == on {
            return;
        }
        if !self.changeable(i) {
            self.fx.info(
                &tr("Context Menu Manager"),
                &tr("Shell extensions, and the commands of every user, need administrator rights: restart rcare as administrator (File menu)."),
            );
            return;
        }
        match ctxmenu::set_enabled(&e, on) {
            Ok(()) => {
                self.items[i].enabled = on;
                self.note = Some(if on {
                    tf("“{}” shows in the menu again.", &[&e.name])
                } else {
                    tf("“{}” no longer shows in the menu.", &[&e.name])
                });
            }
            Err(why) => self.fx.info(&tr("Context Menu Manager"), &tf("Could not change “{}”: {}", &[&e.name, &why])),
        }
        self.fx.menus();
        self.fx.redraw();
    }

    /// Does what a button or its menu entry asks (nothing here needs a question: all can be undone).
    pub fn act(&mut self, a: Action, _dialogs: &mut dyn Dialogs) {
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
                if let Some(f) = self.sel.and_then(|i| self.items.get(i)).and_then(|e| e.program.clone()) {
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
        let name = (inner.x + switch_w, (rest * 0.32).max(120.0 * s).min(rest));
        let place = (name.0 + name.1, (rest * 0.26).min(rest - name.1).max(0.0));
        let program = (place.0 + place.1, (inner.right() - place.0 - place.1).max(0.0));
        Layout { area, actions, header, list, row_h: (30.0 * s).round(), name, place, program, status }
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
        g.text_in(&tr("Context Menu Manager"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(
            g,
            &tr("What programs add to Explorer's right-click menu. Turning an entry off deletes nothing: it can be turned on again."),
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
        for (title, (cx, cw)) in [("Name", l.name), ("Shows on", l.place), ("Program", l.program)] {
            let r = Rect::new(cx + 6.0 * s, hd.y, (cw - 12.0 * s).max(0.0), hd.h);
            let t = ellipsize(g, &tr(title), r.w, p, FontId::Semibold);
            g.text_in(&t, r, p, FontId::Semibold, th.dim, Align::Left);
        }
        // Rows.
        let page = l.page_rows();
        let end = (self.scroll + page).min(self.items.len());
        g.set_clip(Some(l.list));
        for i in self.scroll..end {
            let e = &self.items[i];
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
            let name = ellipsize(g, &e.name, name_r.w, p, FontId::Regular);
            g.text_in(&name, name_r, p, FontId::Regular, dim_if_off, Align::Left);
            let place_r = Rect::new(l.place.0 + 6.0 * s, y, (l.place.1 - 12.0 * s).max(0.0), l.row_h);
            let places = e.places.iter().map(|k| tr(k)).collect::<Vec<_>>().join(", ");
            let place = ellipsize(g, &places, place_r.w, 11.5 * s, FontId::Regular);
            g.text_in(&place, place_r, 11.5 * s, FontId::Regular, th.dim, Align::Left);
            let prog_r = Rect::new(l.program.0 + 6.0 * s, y, (l.program.1 - 12.0 * s).max(0.0), l.row_h);
            // Whose it is: the company, or else the program's file name (its folder: Open File Location).
            let who =
                e.company.clone().or_else(|| e.program.as_ref().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().into_owned()));
            let whose = match who {
                Some(w) => format!("{} · {w}", tr(e.kind_label())),
                None => tr(e.kind_label()),
            };
            let (text, color) = if e.missing {
                (format!("{} — {whose}", tr("Program not found")), th.warn)
            } else if !can {
                (format!("{} — {whose}", tr("Needs administrator rights")), th.dim)
            } else {
                (whose, th.dim)
            };
            let text = ellipsize(g, &text, prog_r.w, 11.5 * s, FontId::Regular);
            g.text_in(&text, prog_r, 11.5 * s, FontId::Regular, color, Align::Left);
        }
        g.set_clip(None);
        if self.items.is_empty() {
            let t = if self.read { tr("No program adds anything to the menu.") } else { tr("Reading the menu…") };
            g.text_in(&t, l.list, p, FontId::Regular, th.dim, Align::Center);
        }
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let off = self.items.iter().filter(|e| !e.enabled).count();
        let left = self.note.clone().unwrap_or_else(|| {
            tf("In the menu: {}. Turned off: {}.", &[&self.items.len().saturating_sub(off).to_string(), &off.to_string()])
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
        if i >= self.items.len() {
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

    /// A click: a button gives its action, a switch turns its entry on or off, a row is selected.
    pub fn press(&mut self) -> Option<Action> {
        self.note = None;
        let mut out = None;
        match self.hot {
            Some(Hit::Action(i)) => out = Some(ACTIONS[i]),
            Some(Hit::Switch(i)) => {
                self.sel = Some(i);
                let on = !self.items[i].enabled;
                self.set(i, on);
            }
            Some(Hit::Row(i)) => self.sel = Some(i),
            None => {}
        }
        self.fx.menus();
        self.fx.redraw();
        out
    }

    /// The arrows move, Space turns the selected entry on or off; F5 reads again.
    pub fn key(&mut self, key: &winit::keyboard::Key) -> Option<Action> {
        use winit::keyboard::{Key, NamedKey};
        if matches!(key, Key::Named(NamedKey::F5)) {
            return Some(Action::Refresh);
        }
        let n = self.items.len();
        if n == 0 {
            return None;
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
                    let on = !self.items[i].enabled;
                    self.set(i, on);
                }
                return None;
            }
            _ => return None,
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
        None
    }

    /// The mouse wheel scrolls the list.
    pub fn wheel(&mut self, delta: winit::event::MouseScrollDelta) {
        let rows = match delta {
            winit::event::MouseScrollDelta::LineDelta(_, l) => (-l * 3.0).round() as isize,
            winit::event::MouseScrollDelta::PixelDelta(p) => (-p.y as f32 / self.layout.row_h.max(20.0)).round() as isize,
        };
        let max = self.items.len().saturating_sub(self.layout.page_rows());
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
        list.set_label(tr("Context Menu Manager"));
        let mut rows = Vec::new();
        let end = (self.scroll + self.layout.page_rows()).min(self.items.len());
        for i in self.scroll..end {
            let e = &self.items[i];
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::CheckBox);
            n.set_label(format!("{}, {}", e.name, tr(e.kind_label())));
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
        if i < self.items.len() {
            self.sel = Some(i);
            let on = !self.items[i].enabled;
            self.set(i, on);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctxmenu::How;
    use std::path::PathBuf;

    fn item(name: &str, how: How, enabled: bool) -> Item {
        Item {
            name: name.into(),
            program: Some(PathBuf::from(format!("C:\\{name}.exe"))),
            places: vec!["Files"],
            how,
            enabled,
            missing: false,
            company: None,
        }
    }

    #[test]
    fn what_can_be_done() {
        let mut p = CtxMenuPage::new(false);
        p.items = vec![
            item("a", How::Verb(vec![(false, "k".into())]), true),
            item("b", How::Handler("{00000000-0000-0000-0000-000000000000}".into()), true),
            item("c", How::Verb(vec![(false, "k2".into()), (true, "k3".into())]), false),
        ];
        assert!(!p.enabled(Action::Off) && p.enabled(Action::Refresh));
        p.sel = Some(0);
        assert!(p.enabled(Action::Off) && !p.enabled(Action::On));
        // A shell extension, and a command of every user, without administrator rights: not changeable, and it says why.
        for i in [1, 2] {
            p.sel = Some(i);
            assert!(!p.changeable(i) && !p.enabled(Action::Off) && !p.enabled(Action::On));
            let was = p.items[i].enabled;
            p.set(i, !was);
            assert!(p.items[i].enabled == was && p.fx.take().info.is_some());
        }
        p.admin = true;
        assert!(p.changeable(1) && p.changeable(2));
    }

    #[test]
    fn rows_are_found_under_the_pointer() {
        let mut p = CtxMenuPage::new(true);
        p.items = (0..30).map(|i| item(&format!("e{i}"), How::Verb(vec![(false, format!("k{i}"))]), i % 2 == 0)).collect();
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
        assert_eq!(p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::F5)), Some(Action::Refresh));
    }
}
