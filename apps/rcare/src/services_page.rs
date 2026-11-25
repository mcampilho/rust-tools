//! The Program Deactivator page: the services of other companies that start
//! with Windows (`services`), one row each with a switch, the company, how
//! it starts and what it runs. Turning one off sets it to start by hand
//! (Manual) after asking; how it started is written to a copy first, and
//! turning it on gives that back. Changing needs administrator rights.

use crate::i18n::{tf, tr};
use crate::services::{self, Copy, Service, Start};
use rt_gfx::gfx::{Align, Color, FontId, Gfx, Rect};
use rt_ui::page::{Dialogs, Effects};
use rt_ui::theme::Theme;
use std::path::PathBuf;
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
    /// Columns (x, width): name, company, start, program; the switch is before the name.
    pub name: (f32, f32),
    pub company: (f32, f32),
    pub start: (f32, f32),
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
pub struct ServicesPage {
    pub services: Vec<Service>,
    pub sel: Option<usize>,
    pub scroll: usize,
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    /// Running with administrator rights: services can be changed.
    pub admin: bool,
    /// The list was read at least once.
    pub read: bool,
    /// A line for the status bar (what was just done).
    pub note: Option<String>,
    /// Where the copy is kept (rcare's data folder).
    pub dir: PathBuf,
    scale: f32,
    /// Reading the services (the Service Control Manager can be slow while Windows is busy).
    loading: crate::bg::Job<Vec<Service>>,
    /// A change under way: the row, the start it goes to, and how it ended.
    changing: crate::bg::Job<(usize, Start, Result<(), String>)>,
}

impl ServicesPage {
    pub fn new(admin: bool, dir: PathBuf) -> ServicesPage {
        ServicesPage { admin, dir, scale: 1.0, ..ServicesPage::default() }
    }

    /// Reads the list again in a thread; `animate` takes it, keeping the selection on the same service.
    pub fn refresh(&mut self) {
        let dir = self.dir.clone();
        self.loading.start(move || services::read(&Copy::load(&dir)));
        self.fx.redraw();
    }

    pub fn busy(&self) -> bool {
        self.loading.busy() || self.changing.busy()
    }

    /// While reading or changing: takes what came, and when to look again.
    pub fn animate(&mut self) -> Option<std::time::Instant> {
        if let Some(list) = self.loading.take() {
            let keep = self.sel.and_then(|i| self.services.get(i)).map(|e| e.name.clone());
            self.services = list;
            self.sel = keep.and_then(|n| self.services.iter().position(|e| e.name == n));
            self.scroll = self.scroll.min(self.services.len().saturating_sub(1));
            self.read = true;
            self.fx.menus();
            self.fx.redraw();
        }
        if let Some((i, to, res)) = self.changing.take() {
            self.changed(i, to, res);
        }
        self.busy().then(crate::bg::soon)
    }

    /// A change ended: the row and the copy follow it, or why not is said.
    fn changed(&mut self, i: usize, to: Start, res: Result<(), String>) {
        let Some(e) = self.services.get(i).cloned() else { return };
        let on = to.with_windows();
        match res {
            Ok(()) => {
                if on {
                    let mut copy = Copy::load(&self.dir);
                    copy.forget(&e.name);
                    let _ = copy.save(&self.dir);
                }
                let s = &mut self.services[i];
                s.was = if on { None } else { Some(e.start) };
                s.start = to;
                self.note = Some(if on {
                    tf("“{}” will start with Windows.", &[&e.display])
                } else {
                    tf("“{}” will start only when needed.", &[&e.display])
                });
            }
            Err(why) => self.fx.info(&tr("Program Deactivator"), &tf("Could not change “{}”: {}", &[&e.display, &why])),
        }
        self.fx.menus();
        self.fx.redraw();
    }

    pub fn enabled(&self, a: Action) -> bool {
        let sel = self.sel.and_then(|i| self.services.get(i));
        match a {
            Action::Off => self.admin && !self.busy() && sel.is_some_and(|e| e.on()),
            Action::On => self.admin && !self.busy() && sel.is_some_and(|e| !e.on()),
            Action::Open => sel.is_some_and(|e| e.file.as_ref().is_some_and(|f| f.parent().is_some_and(|d| d.is_dir()))),
            Action::Refresh => !self.busy(),
        }
    }

    /// Turns the service `i` off (after asking) or on; says why not when it cannot.
    fn set(&mut self, i: usize, on: bool, dialogs: &mut dyn Dialogs) {
        let Some(e) = self.services.get(i).cloned() else { return };
        if e.on() == on || self.busy() {
            return;
        }
        if !self.admin {
            self.fx.info(
                &tr("Program Deactivator"),
                &tr("Changing how services start needs administrator rights: restart rcare as administrator (File menu)."),
            );
            return;
        }
        let mut copy = Copy::load(&self.dir);
        let to = if on {
            e.was.filter(|w| w.with_windows()).unwrap_or(Start::Auto)
        } else {
            let text = format!(
                "{}\n\n{}",
                tf("“{}” will start only when Windows or a program asks for it, not with Windows.", &[&e.display]),
                tr("How it starts now is kept, so it can be turned on again. Services of security programs and of drivers may need to start with Windows.")
            );
            if !dialogs.confirm(&tr("Program Deactivator"), &text) {
                return;
            }
            // The copy first: if it cannot be written, nothing is changed.
            copy.remember(&e.name, e.start);
            if let Err(why) = copy.save(&self.dir) {
                self.fx.info(&tr("Program Deactivator"), &tf("Could not keep a copy: {}", &[&why.to_string()]));
                return;
            }
            Start::Manual
        };
        // The Service Control Manager can wait while Windows is busy: in a thread.
        let name = e.name.clone();
        self.changing.start(move || (i, to, services::set_start(&name, to)));
        self.note = Some(tr("Changing…"));
        self.fx.menus();
        self.fx.redraw();
    }

    /// Does what a button, a switch or a menu entry asks.
    pub fn act(&mut self, a: Action, dialogs: &mut dyn Dialogs) {
        if a == Action::Off || a == Action::On {
            // Without rights the switch still says why.
            if let Some(i) = self.sel {
                self.set(i, a == Action::On, dialogs);
            }
            return;
        }
        if !self.enabled(a) {
            return;
        }
        match a {
            Action::Open => {
                if let Some(f) = self.sel.and_then(|i| self.services.get(i)).and_then(|e| e.file.clone()) {
                    rt_sys::reveal(&f);
                }
            }
            Action::Refresh => self.refresh(),
            Action::Off | Action::On => {}
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
        let name = (inner.x + switch_w, (rest * 0.3).max(120.0 * s).min(rest));
        let company = (name.0 + name.1, (rest * 0.16).min(rest - name.1).max(0.0));
        let start = (company.0 + company.1, (rest * 0.18).min(rest - name.1 - company.1).max(0.0));
        let command = (start.0 + start.1, (inner.right() - start.0 - start.1).max(0.0));
        Layout { area, actions, header, list, row_h: (30.0 * s).round(), name, company, start, command, status }
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
        g.text_in(&tr("Program Deactivator"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(
            g,
            &tr("Services of other companies that start with Windows. Turned off, a service starts only when needed; how it started is kept to turn it on again."),
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
        for (title, (cx, cw)) in [("Name", l.name), ("Company", l.company), ("Starts", l.start), ("Program", l.command)] {
            let r = Rect::new(cx + 6.0 * s, hd.y, (cw - 12.0 * s).max(0.0), hd.h);
            let t = ellipsize(g, &tr(title), r.w, p, FontId::Semibold);
            g.text_in(&t, r, p, FontId::Semibold, th.dim, Align::Left);
        }
        // Rows.
        let end = (self.scroll + l.page_rows()).min(self.services.len());
        g.set_clip(Some(l.list));
        for i in self.scroll..end {
            let e = &self.services[i];
            let y = l.list.y + (i - self.scroll) as f32 * l.row_h;
            let row = Rect::new(l.list.x, y, l.list.w, l.row_h);
            if self.sel == Some(i) {
                g.fill_rect(row, th.selection);
            } else if self.hot == Some(Hit::Row(i)) || self.hot == Some(Hit::Switch(i)) {
                g.fill_rect(row, th.hover);
            }
            let sw = l.switch_at(y, s);
            let track = if e.on() { th.accent } else { th.bar_track };
            g.fill_rrect(sw, sw.h / 2.0, if self.admin { track } else { track.mix(th.bg, 0.5) });
            let knob = sw.h - 6.0 * s;
            let kx = if e.on() { sw.right() - knob - 3.0 * s } else { sw.x + 3.0 * s };
            g.fill_circle(kx + knob / 2.0, sw.y + sw.h / 2.0, knob / 2.0, Color::rgb(0xFFFFFF));
            let cell = |c: (f32, f32)| Rect::new(c.0 + 6.0 * s, y, (c.1 - 12.0 * s).max(0.0), l.row_h);
            let name_r = cell(l.name);
            let name = ellipsize(g, &e.display, name_r.w, p, FontId::Regular);
            g.text_in(&name, name_r, p, FontId::Regular, if e.on() { th.text } else { th.dim }, Align::Left);
            let comp_r = cell(l.company);
            let comp = ellipsize(g, e.company.as_deref().unwrap_or("–"), comp_r.w, 11.5 * s, FontId::Regular);
            g.text_in(&comp, comp_r, 11.5 * s, FontId::Regular, th.dim, Align::Left);
            let start_r = cell(l.start);
            let mut start = tr(e.start.label());
            if e.running {
                start = format!("{start}, {}", tr("running"));
            }
            let start = ellipsize(g, &start, start_r.w, 11.5 * s, FontId::Regular);
            g.text_in(&start, start_r, 11.5 * s, FontId::Regular, th.dim, Align::Left);
            let cmd_r = cell(l.command);
            let text = rt_gfx::dialog::ellipsize_middle(g, &e.command, cmd_r.w, 11.5 * s, FontId::Regular);
            g.text_in(&text, cmd_r, 11.5 * s, FontId::Regular, th.dim, Align::Left);
        }
        g.set_clip(None);
        if self.read && self.services.is_empty() {
            g.text_in(&tr("No service of another company starts with Windows."), l.list, p, FontId::Regular, th.dim, Align::Center);
        }
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let off = self.services.iter().filter(|e| !e.on()).count();
        let left = self.note.clone().unwrap_or_else(|| {
            let n = tf("Start with Windows: {}. Turned off: {}.", &[&(self.services.len() - off).to_string(), &off.to_string()]);
            if self.admin {
                n
            } else {
                format!("{n} {}", tr("Changing them needs administrator rights."))
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
            return Some(Hit::Action(i));
        }
        if !l.list.contains(x, y) || l.row_h <= 0.0 {
            return None;
        }
        let i = self.scroll + ((y - l.list.y) / l.row_h) as usize;
        if i >= self.services.len() {
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
            Some(Hit::Switch(_)) if self.admin => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        };
    }

    pub fn pointer_left(&mut self) {
        if self.hot.take().is_some() {
            self.fx.redraw();
        }
    }

    pub fn release(&mut self) {}

    /// The action of what was clicked on the selected row.
    fn flip(&self, i: usize) -> Action {
        if self.services[i].on() {
            Action::Off
        } else {
            Action::On
        }
    }

    /// A click: a row is selected; a button or a switch gives the action for
    /// the window to do (turning off asks first).
    pub fn press(&mut self) -> Option<Action> {
        self.note = None;
        let out = match self.hot {
            Some(Hit::Action(i)) => Some(ACTIONS[i]),
            Some(Hit::Switch(i)) => {
                self.sel = Some(i);
                Some(self.flip(i))
            }
            Some(Hit::Row(i)) => {
                self.sel = Some(i);
                None
            }
            None => None,
        };
        self.fx.menus();
        self.fx.redraw();
        out
    }

    /// The arrows move; Space gives the action for the selected service, F5 reads again.
    pub fn key(&mut self, key: &winit::keyboard::Key) -> Option<Action> {
        use winit::keyboard::{Key, NamedKey};
        let n = self.services.len();
        if let Key::Named(NamedKey::F5) = key {
            return Some(Action::Refresh);
        }
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
            Key::Named(NamedKey::Space) => return self.sel.map(|i| self.flip(i)),
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
        let max = self.services.len().saturating_sub(self.layout.page_rows());
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
        list.set_label(tr("Program Deactivator"));
        let mut rows = Vec::new();
        let end = (self.scroll + self.layout.page_rows()).min(self.services.len());
        for i in self.scroll..end {
            let e = &self.services[i];
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::CheckBox);
            n.set_label(format!("{}, {}, {}", e.display, e.company.as_deref().unwrap_or(""), tr(e.start.label())));
            n.set_toggled(if e.on() { Toggled::True } else { Toggled::False });
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

    /// A screen reader clicked the row `i`: it is selected (turning it off or
    /// on goes through the Edit menu, which asks first).
    pub fn accessibility_click(&mut self, i: usize) {
        if i < self.services.len() {
            self.sel = Some(i);
            self.fx.menus();
            self.fx.redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Answer(bool, usize);

    impl Dialogs for Answer {
        fn confirm(&mut self, _title: &str, _text: &str) -> bool {
            self.1 += 1;
            self.0
        }
        fn pick_folder(&mut self, _title: &str) -> Option<PathBuf> {
            None
        }
        fn save_file(&mut self, _name: &str, _kind: (&str, &str), _dir: Option<&std::path::Path>) -> Option<PathBuf> {
            None
        }
    }

    fn svc(name: &str, start: Start) -> Service {
        Service {
            name: name.into(),
            display: name.into(),
            company: Some("Acme".into()),
            command: format!("C:\\{name}.exe"),
            file: None,
            start,
            running: true,
            was: None,
        }
    }

    #[test]
    fn what_can_be_done() {
        let mut p = ServicesPage::new(false, std::env::temp_dir());
        p.services = vec![svc("a", Start::Auto), svc("b", Start::Manual)];
        assert!(!p.enabled(Action::Off) && p.enabled(Action::Refresh));
        p.sel = Some(0);
        // Without rights: nothing changes, nothing is asked, and it says why.
        assert!(!p.enabled(Action::Off));
        let mut d = Answer(true, 0);
        p.act(Action::Off, &mut d);
        assert_eq!((p.services[0].start, d.1), (Start::Auto, 0));
        assert!(p.fx.take().info.is_some());
        p.admin = true;
        assert!(p.enabled(Action::Off) && !p.enabled(Action::On));
        // Said no: nothing is written or changed.
        let dir = std::env::temp_dir().join(format!("rcare-svcpage-{}", std::process::id()));
        p.dir = dir.clone();
        let mut no = Answer(false, 0);
        p.act(Action::Off, &mut no);
        assert_eq!((p.services[0].start, no.1), (Start::Auto, 1));
        assert!(!dir.join(Copy::FILE).exists());
        p.sel = Some(1);
        assert!(p.enabled(Action::On));
    }

    #[test]
    fn rows_are_found_under_the_pointer() {
        let mut p = ServicesPage::new(true, std::env::temp_dir());
        p.services = (0..30).map(|i| svc(&format!("s{i}"), if i % 2 == 0 { Start::Auto } else { Start::Manual })).collect();
        p.layout = p.layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        let l = p.layout.clone();
        assert!(l.command.0 + l.command.1 <= l.list.right() + 0.5 && l.company.0 > l.name.0 && l.start.0 > l.company.0);
        let y = l.list.y + l.row_h * 2.5;
        assert_eq!(p.hit(l.name.0 + 20.0, y), Some(Hit::Row(2)));
        let sw = l.switch_at(l.list.y + l.row_h * 3.0, 1.0);
        p.hot = p.hit(sw.x + 2.0, sw.y + 2.0);
        assert_eq!(p.hot, Some(Hit::Switch(3)));
        assert_eq!(p.press(), Some(Action::On), "a switch that is off asks to turn on");
        assert_eq!(p.sel, Some(3));
        assert_eq!(p.hit(l.actions[3].x + 2.0, l.actions[3].y + 2.0), Some(Hit::Action(3)));
        p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::End));
        assert_eq!(p.sel, Some(29));
        assert!(p.scroll + p.layout.page_rows() > 29);
        assert_eq!(p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Space)), Some(Action::On));
        p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowUp));
        assert_eq!(p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Space)), Some(Action::Off));
    }
}
