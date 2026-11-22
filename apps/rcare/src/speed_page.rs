//! The Speed Up page: Windows' Game Mode switch, the memory (with freeing the
//! standby list, as administrator), the processor's use and the programs
//! open, read again every two seconds while the page is shown. Programs are
//! closed only when the user ticks them, and asked politely: each can still
//! ask to save its work.

use crate::i18n::{tf, tr};
use crate::speed::{self, CpuTimes, Memory, OpenApp};
use rt_gfx::gfx::{Align, Color, FontId, Gfx, Rect};
use rt_ui::page::{Dialogs, Effects};
use rt_ui::theme::Theme;
use std::time::{Duration, Instant};
use winit::window::CursorIcon;

/// What the page does (its buttons and menu entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    GameMode,
    FreeStandby,
    CloseTicked,
    Refresh,
}

impl Action {
    /// The menu label (a translation key with its access key).
    pub fn label_key(self) -> &'static str {
        match self {
            Action::GameMode => "&Game Mode",
            Action::FreeStandby => "&Free Standby Memory",
            Action::CloseTicked => "&Close the Ticked Programs...",
            Action::Refresh => "&Refresh",
        }
    }
}

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Act(Action),
    /// A row's box.
    Tick(usize),
    Row(usize),
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub game: Rect,
    pub switch: Rect,
    pub memory: Rect,
    pub free: Rect,
    pub cpu: Rect,
    pub close: Rect,
    pub refresh: Rect,
    pub header: Rect,
    pub list: Rect,
    pub row_h: f32,
    pub status: Rect,
}

impl Layout {
    pub fn page_rows(&self) -> usize {
        if self.row_h <= 0.0 {
            1
        } else {
            ((self.list.h / self.row_h).floor() as usize).max(1)
        }
    }
}

/// How often the page reads again while it is shown.
pub const EVERY: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct SpeedPage {
    pub memory: Option<Memory>,
    pub cpu: Option<f32>,
    pub game_mode: bool,
    pub apps: Vec<OpenApp>,
    /// The programs ticked, by process.
    pub ticked: Vec<u32>,
    pub sel: Option<usize>,
    pub scroll: usize,
    pub admin: bool,
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    pub note: Option<String>,
    last_cpu: Option<CpuTimes>,
    next: Option<Instant>,
}

impl SpeedPage {
    pub fn new(admin: bool) -> SpeedPage {
        SpeedPage { admin, ..SpeedPage::default() }
    }

    /// Reads everything again; the ticks and the selection stay on the same programs.
    pub fn refresh(&mut self) {
        let sel_pid = self.sel.and_then(|i| self.apps.get(i)).map(|a| a.pid);
        self.memory = speed::memory();
        let now = speed::cpu_times();
        self.cpu = match (self.last_cpu, now) {
            (Some(a), Some(b)) => Some(speed::cpu_percent(a, b)),
            _ => self.cpu,
        };
        self.last_cpu = now;
        self.game_mode = speed::game_mode();
        self.apps = speed::open_apps();
        self.ticked.retain(|pid| self.apps.iter().any(|a| a.pid == *pid));
        self.sel = sel_pid.and_then(|p| self.apps.iter().position(|a| a.pid == p));
        self.scroll = self.scroll.min(self.apps.len().saturating_sub(1));
        self.next = Some(Instant::now() + EVERY);
        self.fx.menus();
        self.fx.redraw();
    }

    /// While shown: the time to read again (every two seconds).
    pub fn animate(&mut self) -> Option<Instant> {
        let due = self.next?;
        if Instant::now() >= due {
            self.refresh();
        }
        self.next
    }

    /// The page is hidden: it stops reading until shown again.
    pub fn pause(&mut self) {
        self.next = None;
    }

    pub fn enabled(&self, a: Action) -> bool {
        match a {
            Action::GameMode | Action::Refresh => true,
            Action::FreeStandby => self.admin && self.memory.is_some_and(|m| m.standby.is_some_and(|s| s > 0)),
            Action::CloseTicked => !self.ticked.is_empty(),
        }
    }

    pub fn checked(&self, a: Action) -> bool {
        a == Action::GameMode && self.game_mode
    }

    /// Does what a button or its menu entry asks.
    pub fn act(&mut self, a: Action, dialogs: &mut dyn Dialogs) {
        if !self.enabled(a) {
            return;
        }
        self.note = None;
        match a {
            Action::GameMode => match speed::set_game_mode(!self.game_mode) {
                Ok(()) => self.game_mode = speed::game_mode(),
                Err(e) => self.fx.info(&tr("Speed Up"), &tf("Could not change it: {}", &[&e])),
            },
            Action::FreeStandby => {
                let before = self.memory.and_then(|m| m.standby).unwrap_or(0);
                match speed::purge_standby() {
                    Ok(()) => {
                        self.refresh();
                        let after = self.memory.and_then(|m| m.standby).unwrap_or(0);
                        self.note = Some(tf("{} of standby memory freed.", &[&rsize::format::size(before.saturating_sub(after))]));
                    }
                    Err(e) => self.fx.info(&tr("Speed Up"), &tf("Could not free the standby memory: {}", &[&e])),
                }
            }
            Action::CloseTicked => {
                let apps: Vec<OpenApp> = self.apps.iter().filter(|a| self.ticked.contains(&a.pid)).cloned().collect();
                let list = apps.iter().map(|a| format!("  {} ({})", a.title, a.exe)).collect::<Vec<_>>().join("\n");
                let text = format!(
                    "{}\n\n{list}\n\n{}",
                    tf("Ask {} programs to close?", &[&apps.len().to_string()]),
                    tr("Each is asked as its close button does: it can still ask you to save your work.")
                );
                if dialogs.confirm(&tr("Close the Ticked Programs"), &text) {
                    for a in &apps {
                        speed::close(a);
                    }
                    self.ticked.clear();
                    self.note = Some(tf("{} programs were asked to close.", &[&apps.len().to_string()]));
                }
            }
            Action::Refresh => self.refresh(),
        }
        self.fx.menus();
        self.fx.redraw();
    }

    fn toggle(&mut self, i: usize) {
        if let Some(pid) = self.apps.get(i).map(|a| a.pid) {
            if let Some(k) = self.ticked.iter().position(|p| *p == pid) {
                self.ticked.remove(k);
            } else {
                self.ticked.push(pid);
            }
            self.fx.menus();
            self.fx.redraw();
        }
    }

    // ------------------------------------------------------------ drawing

    pub fn layout_in(area: Rect, s: f32) -> Layout {
        let pad = (24.0 * s).round();
        let inner = Rect::new(area.x + pad, area.y + pad, (area.w - 2.0 * pad).max(0.0), (area.h - pad).max(0.0));
        let top = inner.y + (62.0 * s).round();
        let ch = (116.0 * s).round();
        let gap = (12.0 * s).round();
        let cw = ((inner.w - 2.0 * gap) / 3.0).floor();
        let game = Rect::new(inner.x, top, cw, ch);
        let memory = Rect::new(game.right() + gap, top, cw, ch);
        let cpu = Rect::new(memory.right() + gap, top, cw, ch);
        let bh = (30.0 * s).round();
        let switch = Rect::new(game.x + 14.0 * s, game.bottom() - 14.0 * s - 20.0 * s, 40.0 * s, 20.0 * s);
        let free = Rect::new(
            memory.x + 14.0 * s,
            memory.bottom() - (28.0 * s).round() - 12.0 * s,
            (memory.w - 28.0 * s).max(0.0),
            (28.0 * s).round(),
        );
        let bar_y = cpu.bottom() + (16.0 * s).round();
        let close = Rect::new(inner.x, bar_y, (230.0 * s).round().min(inner.w * 0.6), bh);
        let refresh = Rect::new(close.right() + 8.0 * s, bar_y, (110.0 * s).round(), bh);
        let header = Rect::new(inner.x, close.bottom() + (10.0 * s).round(), inner.w, (26.0 * s).round());
        let status_h = (24.0 * s).round();
        let list = Rect::new(inner.x, header.bottom(), inner.w, (area.bottom() - status_h - header.bottom()).max(0.0));
        let status = Rect::new(area.x, area.bottom() - status_h, area.w, status_h);
        Layout { area, game, switch, memory, free, cpu, close, refresh, header, list, row_h: (28.0 * s).round(), status }
    }

    fn card(g: &mut Gfx, r: Rect, title: &str, th: &Theme, s: f32) {
        g.fill_rrect(r, 8.0 * s, th.panel);
        g.stroke_rrect(r, 8.0 * s, 1.0, th.border);
        let t = rt_gfx::dialog::ellipsize(g, title, r.w - 28.0 * s, 13.5 * s, FontId::Semibold);
        g.text_in(
            &t,
            Rect::new(r.x + 14.0 * s, r.y + 10.0 * s, r.w - 28.0 * s, 22.0 * s),
            13.5 * s,
            FontId::Semibold,
            th.text,
            Align::Left,
        );
    }

    fn button(&self, g: &mut Gfx, r: Rect, a: Action, label: &str, th: &Theme, s: f32) {
        let on = self.enabled(a);
        g.fill_rrect(r, 5.0 * s, th.bg);
        if on && self.hot == Some(Hit::Act(a)) {
            g.fill_rrect(r, 5.0 * s, th.hover);
        }
        g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
        let t = rt_gfx::dialog::ellipsize(g, label, r.w - 10.0 * s, 12.5 * s, FontId::Regular);
        g.text_in(&t, r, 12.5 * s, FontId::Regular, if on { th.text } else { th.dim }, Align::Center);
    }

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        use rt_core::i18n::strip_mnemonic;
        use rt_gfx::dialog::ellipsize;
        let l = Self::layout_in(area, s);
        g.fill_rect(area, th.bg);
        let pad = (24.0 * s).round();
        let (x, w) = (area.x + pad, (area.w - 2.0 * pad).max(0.0));
        g.text_in(&tr("Speed Up"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(
            g,
            &tr("Windows' Game Mode, the memory, the processor and the programs open. A program is closed only if you tick it, and it is asked as its close button does."),
            w,
            12.5 * s,
            FontId::Regular,
        );
        g.text_in(&about, Rect::new(x, area.y + pad + 32.0 * s, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
        let small = 11.5 * s;
        // Game Mode.
        Self::card(g, l.game, &strip_mnemonic(&tr("&Game Mode")), th, s);
        let gm = ellipsize(g, &tr("When a game runs, Windows gives it priority."), l.game.w - 28.0 * s, small, FontId::Regular);
        g.text_in(
            &gm,
            Rect::new(l.game.x + 14.0 * s, l.game.y + 34.0 * s, l.game.w - 28.0 * s, 18.0 * s),
            small,
            FontId::Regular,
            th.dim,
            Align::Left,
        );
        let sw = l.switch;
        let track = if self.game_mode { th.accent } else { th.bar_track };
        g.fill_rrect(sw, sw.h / 2.0, if self.hot == Some(Hit::Act(Action::GameMode)) { track.mix(th.text, 0.1) } else { track });
        let knob = sw.h - 6.0 * s;
        let kx = if self.game_mode { sw.right() - knob - 3.0 * s } else { sw.x + 3.0 * s };
        g.fill_circle(kx + knob / 2.0, sw.y + sw.h / 2.0, knob / 2.0, Color::rgb(0xFFFFFF));
        let state = if self.game_mode { tr("On") } else { tr("Off") };
        g.text_in(&state, Rect::new(sw.right() + 10.0 * s, sw.y, 120.0 * s, sw.h), 12.5 * s, FontId::Regular, th.text, Align::Left);
        // Memory.
        Self::card(g, l.memory, &tr("Memory"), th, s);
        if let Some(m) = self.memory {
            let bar = Rect::new(l.memory.x + 14.0 * s, l.memory.y + 36.0 * s, l.memory.w - 28.0 * s, 8.0 * s);
            g.fill_rrect(bar, 4.0 * s, th.bar_track);
            let used_w = bar.w * (m.used() as f32 / m.total.max(1) as f32);
            g.fill_rrect(Rect::new(bar.x, bar.y, used_w.max(1.0), bar.h), 4.0 * s, th.accent);
            if let Some(sb) = m.standby {
                let sw_ = bar.w * (sb as f32 / m.total.max(1) as f32);
                g.fill_rect(Rect::new(bar.x + used_w, bar.y, sw_.min(bar.w - used_w).max(0.0), bar.h), th.bar);
            }
            let size = rsize::format::size;
            let line = match m.standby {
                Some(sb) => tf("In use {} of {} · standby {}", &[&size(m.used()), &size(m.total), &size(sb)]),
                None => tf("In use {} of {}", &[&size(m.used()), &size(m.total)]),
            };
            let line = ellipsize(g, &line, bar.w, small, FontId::Regular);
            g.text_in(&line, Rect::new(bar.x, bar.bottom() + 4.0 * s, bar.w, 18.0 * s), small, FontId::Regular, th.dim, Align::Left);
        }
        let free_label = if self.admin { strip_mnemonic(&tr("&Free Standby Memory")) } else { tr("Free standby memory (administrator)") };
        self.button(g, l.free, Action::FreeStandby, &free_label, th, s);
        // Processor.
        Self::card(g, l.cpu, &tr("Processor"), th, s);
        let pct = self.cpu.map_or_else(|| "…".to_string(), |c| format!("{c:.0} %"));
        g.text_in(
            &pct,
            Rect::new(l.cpu.x + 14.0 * s, l.cpu.y + 38.0 * s, l.cpu.w - 28.0 * s, 30.0 * s),
            24.0 * s,
            FontId::Semibold,
            th.text,
            Align::Left,
        );
        let every = ellipsize(g, &tr("Read again every two seconds."), l.cpu.w - 28.0 * s, small, FontId::Regular);
        g.text_in(
            &every,
            Rect::new(l.cpu.x + 14.0 * s, l.cpu.bottom() - 26.0 * s, l.cpu.w - 28.0 * s, 18.0 * s),
            small,
            FontId::Regular,
            th.dim,
            Align::Left,
        );
        // The programs open.
        self.button(g, l.close, Action::CloseTicked, strip_mnemonic(&tr("&Close the Ticked Programs...")).trim_end_matches("..."), th, s);
        self.button(g, l.refresh, Action::Refresh, &strip_mnemonic(&tr("&Refresh")), th, s);
        let p = 12.5 * s;
        g.fill_rect(l.header, th.header);
        g.fill_rect(Rect::new(l.header.x, l.header.bottom() - 1.0, l.header.w, 1.0), th.border);
        let mem_w = 110.0 * s;
        let exe_w = (l.header.w * 0.25).min(220.0 * s);
        let name_x = l.header.x + 40.0 * s;
        let exe_x = l.header.right() - mem_w - exe_w;
        let name_w = (exe_x - name_x - 8.0 * s).max(0.0);
        for (t, r, al) in [
            ("Programs open", Rect::new(name_x, l.header.y, name_w, l.header.h), Align::Left),
            ("File", Rect::new(exe_x, l.header.y, exe_w, l.header.h), Align::Left),
            ("Memory", Rect::new(l.header.right() - mem_w, l.header.y, mem_w - 10.0 * s, l.header.h), Align::Right),
        ] {
            let tt = ellipsize(g, &tr(t), r.w, p, FontId::Semibold);
            g.text_in(&tt, r, p, FontId::Semibold, th.dim, al);
        }
        g.set_clip(Some(l.list));
        let end = (self.scroll + l.page_rows()).min(self.apps.len());
        for i in self.scroll..end {
            let a = &self.apps[i];
            let y = l.list.y + (i - self.scroll) as f32 * l.row_h;
            let row = Rect::new(l.list.x, y, l.list.w, l.row_h);
            if self.sel == Some(i) {
                g.fill_rect(row, th.selection);
            } else if matches!(self.hot, Some(Hit::Row(k) | Hit::Tick(k)) if k == i) {
                g.fill_rect(row, th.hover);
            }
            let bx = Rect::new(l.list.x + 12.0 * s, y + (l.row_h - 16.0 * s) / 2.0, 16.0 * s, 16.0 * s);
            let on = self.ticked.contains(&a.pid);
            if on {
                g.fill_rrect(bx, 3.0 * s, th.accent);
                let c = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
                g.polyline(
                    &[(bx.x + 3.5 * s, bx.y + 8.0 * s), (bx.x + 7.0 * s, bx.y + 11.5 * s), (bx.x + 12.5 * s, bx.y + 4.5 * s)],
                    2.0 * s,
                    c,
                    false,
                );
            } else {
                g.stroke_rrect(bx, 3.0 * s, 1.2 * s, th.dim);
            }
            let nm = ellipsize(g, &a.title, name_w, p, FontId::Regular);
            g.text_in(&nm, Rect::new(name_x, y, name_w, l.row_h), p, FontId::Regular, th.text, Align::Left);
            let ex = ellipsize(g, &a.exe, exe_w, small, FontId::Regular);
            g.text_in(&ex, Rect::new(exe_x, y, exe_w, l.row_h), small, FontId::Regular, th.dim, Align::Left);
            let ms = rsize::format::size(a.memory);
            g.text_in(&ms, Rect::new(l.list.right() - mem_w, y, mem_w - 10.0 * s, l.row_h), small, FontId::Regular, th.dim, Align::Right);
        }
        g.set_clip(None);
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let left = self
            .note
            .clone()
            .unwrap_or_else(|| tf("Programs open: {}. Ticked: {}.", &[&self.apps.len().to_string(), &self.ticked.len().to_string()]));
        let sr = Rect::new(l.status.x + 10.0 * s, l.status.y, (l.status.w - 20.0 * s).max(0.0), l.status.h);
        let left = ellipsize(g, &left, sr.w, 12.0 * s, FontId::Regular);
        g.text_in(&left, sr, 12.0 * s, FontId::Regular, th.dim, Align::Left);
        self.layout = l;
    }

    // ------------------------------------------------------------ input

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let l = &self.layout;
        for (r, a) in
            [(l.switch, Action::GameMode), (l.free, Action::FreeStandby), (l.close, Action::CloseTicked), (l.refresh, Action::Refresh)]
        {
            if r.contains(x, y) {
                return Some(Hit::Act(a));
            }
        }
        if !l.list.contains(x, y) || l.row_h <= 0.0 {
            return None;
        }
        let i = self.scroll + ((y - l.list.y) / l.row_h) as usize;
        if i >= self.apps.len() {
            return None;
        }
        Some(if x < l.list.x + 36.0 * (l.row_h / 28.0) { Hit::Tick(i) } else { Hit::Row(i) })
    }

    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        let hot = if blocked { None } else { self.hit(x, y) };
        if hot != self.hot {
            self.hot = hot;
            self.fx.redraw();
        }
        self.cursor_icon = match hot {
            Some(Hit::Act(a)) if self.enabled(a) => CursorIcon::Pointer,
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

    /// A click: what a button asks (for the window, which passes its dialogs), or a box, or a row.
    pub fn press(&mut self) -> Option<Action> {
        match self.hot {
            Some(Hit::Act(a)) => return Some(a),
            Some(Hit::Tick(i)) => {
                self.sel = Some(i);
                self.toggle(i);
            }
            Some(Hit::Row(i)) => self.sel = Some(i),
            None => {}
        }
        self.fx.menus();
        self.fx.redraw();
        None
    }

    /// The arrows move, Space ticks; F5 reads again.
    pub fn key(&mut self, key: &winit::keyboard::Key) {
        use winit::keyboard::{Key, NamedKey};
        if matches!(key, Key::Named(NamedKey::F5)) {
            return self.refresh();
        }
        let n = self.apps.len();
        if n == 0 {
            return;
        }
        let cur = self.sel.map_or(-1, |i| i as isize);
        let page = self.layout.page_rows() as isize;
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
                    self.toggle(i);
                }
                return;
            }
            _ => return,
        }
        if let Some(i) = self.sel {
            if i < self.scroll {
                self.scroll = i;
            } else if i >= self.scroll + self.layout.page_rows() {
                self.scroll = i + 1 - self.layout.page_rows();
            }
        }
        self.fx.menus();
        self.fx.redraw();
    }

    pub fn wheel(&mut self, delta: winit::event::MouseScrollDelta) {
        let rows = match delta {
            winit::event::MouseScrollDelta::LineDelta(_, l) => (-l * 3.0).round() as isize,
            winit::event::MouseScrollDelta::PixelDelta(p) => (-p.y as f32 / self.layout.row_h.max(20.0)).round() as isize,
        };
        let max = self.apps.len().saturating_sub(self.layout.page_rows());
        self.scroll = (self.scroll as isize + rows).clamp(0, max as isize) as usize;
        self.fx.redraw();
    }

    /// The page's nodes for the screen readers: the programs are 100 + their index.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role, Toggled};
        let mut info = Node::new(Role::Label);
        let mem = self.memory.map_or(String::new(), |m| format!("{} {} %", tr("Memory"), m.load()));
        let cpu = self.cpu.map_or(String::new(), |c| format!("{} {c:.0} %", tr("Processor")));
        info.set_value(format!(
            "{}: {}. {mem}. {cpu}.",
            rt_core::i18n::strip_mnemonic(&tr("&Game Mode")),
            if self.game_mode { tr("On") } else { tr("Off") }
        ));
        nodes.push((NodeId(40), info));
        children.push(NodeId(40));
        let mut focus = None;
        let mut list = Node::new(Role::List);
        list.set_label(tr("Programs open"));
        let mut rows = Vec::new();
        let end = (self.scroll + self.layout.page_rows()).min(self.apps.len());
        for i in self.scroll..end {
            let a = &self.apps[i];
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::CheckBox);
            n.set_label(format!("{}, {}, {}", a.title, a.exe, rsize::format::size(a.memory)));
            n.set_toggled(if self.ticked.contains(&a.pid) { Toggled::True } else { Toggled::False });
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

    /// A screen reader clicked the program `i`: it is ticked or unticked.
    pub fn accessibility_click(&mut self, i: usize) {
        self.sel = Some(i);
        self.toggle(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(pid: u32, exe: &str) -> OpenApp {
        OpenApp { pid, exe: exe.into(), title: format!("{exe} window"), memory: u64::from(pid) << 20, windows: vec![pid as isize] }
    }

    #[test]
    fn ticks_follow_the_programs() {
        let mut p = SpeedPage::new(false);
        p.apps = vec![app(1, "a.exe"), app(2, "b.exe")];
        assert!(!p.enabled(Action::CloseTicked));
        p.toggle(1);
        assert_eq!(p.ticked, vec![2]);
        assert!(p.enabled(Action::CloseTicked));
        p.toggle(1);
        assert!(p.ticked.is_empty());
        // Without administrator rights the standby list is not freed from here.
        p.memory = Some(Memory { total: 8, available: 4, standby: Some(2) });
        assert!(!p.enabled(Action::FreeStandby));
        p.admin = true;
        assert!(p.enabled(Action::FreeStandby));
    }

    #[test]
    fn buttons_and_rows_are_found() {
        let mut p = SpeedPage::new(true);
        p.apps = (1..=20).map(|i| app(i, "x.exe")).collect();
        p.layout = SpeedPage::layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        let l = p.layout.clone();
        assert_eq!(p.hit(l.switch.x + 2.0, l.switch.y + 2.0), Some(Hit::Act(Action::GameMode)));
        assert_eq!(p.hit(l.close.x + 2.0, l.close.y + 2.0), Some(Hit::Act(Action::CloseTicked)));
        assert_eq!(p.hit(l.list.x + 14.0, l.list.y + l.row_h * 1.5), Some(Hit::Tick(1)));
        assert_eq!(p.hit(l.list.x + 300.0, l.list.y + l.row_h * 1.5), Some(Hit::Row(1)));
        assert!(l.cpu.right() <= l.area.right());
    }
}
