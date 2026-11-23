//! The Processes page: Auto Shutdown on top (shut down or restart in a
//! while or at a time, by Windows' own `shutdown.exe`, called off with
//! Cancel), and below it every process with its processor use, memory and
//! program, read again every 2 seconds while the page is shown. A process
//! is ended only after a question, and never one Windows needs.

use crate::i18n::{tf, tr};
use crate::processes::{self, Planned, Power, Proc};
use rt_gfx::gfx::{Align, Color, FontId, Gfx, Rect};
use rt_ui::page::{Dialogs, Effects};
use rt_ui::theme::Theme;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use winit::window::CursorIcon;

/// How often the list is read again.
const EVERY: Duration = Duration::from_secs(2);

/// The delays offered, in minutes.
pub const DELAYS: [u32; 12] = [5, 10, 15, 30, 45, 60, 90, 120, 180, 240, 360, 480];

/// What the buttons (and the Edit menu) do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Start,
    CancelPlan,
    End,
    Open,
}

impl Action {
    /// The menu label (a translation key with its access key).
    pub fn label_key(self) -> &'static str {
        match self {
            Action::Start => "&Start the Countdown...",
            Action::CancelPlan => "&Cancel the Shutdown",
            Action::End => "&End the Process...",
            Action::Open => "Open File &Location",
        }
    }
}

/// How the list is sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Sort {
    Name,
    Pid,
    #[default]
    Cpu,
    Memory,
}

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Power(Power),
    /// In a while (false) or at a time (true).
    Mode(bool),
    Minus,
    Plus,
    Act(Action),
    Header(Sort),
    Row(usize),
}

/// A process as shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub proc: Proc,
    /// Processor use since the reading before, percent of the computer.
    pub cpu: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub card: Rect,
    pub power: [Rect; 2],
    pub mode: [Rect; 2],
    pub minus: Rect,
    pub value: Rect,
    pub plus: Rect,
    pub start: Rect,
    pub cancel: Rect,
    pub plan_line: Rect,
    pub end: Rect,
    pub open: Rect,
    pub header: Rect,
    pub list: Rect,
    pub row_h: f32,
    /// Columns (x, width): name, pid, processor, memory, program.
    pub cols: [(f32, f32); 5],
    pub status: Rect,
}

impl Layout {
    pub fn page_rows(&self) -> usize {
        if self.row_h <= 0.0 {
            return 1;
        }
        ((self.list.h / self.row_h).floor() as usize).max(1)
    }
}

pub struct ProcessesPage {
    pub rows: Vec<Row>,
    /// The selected process, by pid.
    pub sel: Option<u32>,
    pub scroll: usize,
    pub sort: Sort,
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    pub fx: Effects,
    pub note: Option<String>,
    /// Auto Shutdown: what to do, and when (in a while, or at a time).
    pub power: Power,
    pub at_time: bool,
    /// Index into `DELAYS`.
    pub delay: usize,
    /// At this time, minutes after midnight (steps of 15).
    pub clock: u32,
    /// What Windows was asked to do, while it is still to come.
    pub planned: Option<Planned>,
    file: PathBuf,
    loading: crate::bg::Job<Vec<Proc>>,
    last_read: Option<Instant>,
    own_pid: u32,
}

impl ProcessesPage {
    /// The page; what was planned is kept in `shutdown.txt` in `data_dir`.
    pub fn new(data_dir: &Path) -> ProcessesPage {
        let file = data_dir.join("shutdown.txt");
        ProcessesPage {
            rows: Vec::new(),
            sel: None,
            scroll: 0,
            sort: Sort::Cpu,
            layout: Layout::default(),
            hot: None,
            cursor_icon: CursorIcon::Default,
            fx: Effects::default(),
            note: None,
            power: Power::Shutdown,
            at_time: false,
            delay: 5,
            clock: 23 * 60,
            planned: processes::load_planned(&file),
            file,
            loading: crate::bg::Job::default(),
            last_read: None,
            own_pid: std::process::id(),
        }
    }

    /// Reads the processes now (and then every 2 seconds, while the page is shown).
    pub fn refresh(&mut self) {
        if !self.loading.busy() {
            self.loading.start(processes::read);
        }
    }

    /// Takes a new reading: the processor use of each is what it used since the reading before.
    pub fn take(&mut self, list: Vec<Proc>, elapsed: Option<Duration>) {
        let wall = elapsed.map_or(0, |d| (d.as_nanos() / 100) as u64);
        let cpus = processes::cpus();
        let before: std::collections::HashMap<(u32, u64), u64> =
            self.rows.iter().map(|r| ((r.proc.pid, r.proc.created), r.proc.cpu_time)).collect();
        self.rows = list
            .into_iter()
            .filter(|p| p.pid != 0)
            .map(|p| {
                let cpu = before.get(&(p.pid, p.created)).map_or(0.0, |b| processes::cpu_percent(*b, p.cpu_time, wall, cpus));
                Row { proc: p, cpu }
            })
            .collect();
        self.sort_rows();
        if self.sel.is_some_and(|pid| !self.rows.iter().any(|r| r.proc.pid == pid)) {
            self.sel = None;
        }
        self.fx.menus();
        self.fx.redraw();
    }

    fn sort_rows(&mut self) {
        match self.sort {
            Sort::Name => {
                self.rows.sort_by(|a, b| a.proc.name.to_lowercase().cmp(&b.proc.name.to_lowercase()).then(a.proc.pid.cmp(&b.proc.pid)))
            }
            Sort::Pid => self.rows.sort_by_key(|r| r.proc.pid),
            Sort::Cpu => self
                .rows
                .sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal).then(b.proc.memory.cmp(&a.proc.memory))),
            Sort::Memory => self.rows.sort_by(|a, b| b.proc.memory.cmp(&a.proc.memory).then(a.proc.pid.cmp(&b.proc.pid))),
        }
    }

    /// Reads again every 2 seconds; the countdown's line changes each minute.
    pub fn animate(&mut self) -> Option<Instant> {
        if let Some(list) = self.loading.take() {
            let now = Instant::now();
            let elapsed = self.last_read.map(|t| now - t);
            self.last_read = Some(now);
            self.take(list, elapsed);
        }
        if self.planned.is_some_and(|p| p.at <= processes::unix_now()) {
            self.planned = None;
            self.fx.menus();
            self.fx.redraw();
        }
        if self.loading.busy() {
            return Some(crate::bg::soon());
        }
        let due = self.last_read.map_or_else(Instant::now, |t| t + EVERY);
        if due <= Instant::now() {
            self.refresh();
            return Some(crate::bg::soon());
        }
        Some(due)
    }

    fn selected(&self) -> Option<&Row> {
        self.sel.and_then(|pid| self.rows.iter().find(|r| r.proc.pid == pid))
    }

    /// Whether the selected process may be ended from here.
    fn endable(&self) -> bool {
        self.selected().is_some_and(|r| !processes::protected(&r.proc.name, r.proc.pid, self.own_pid))
    }

    pub fn enabled(&self, a: Action) -> bool {
        match a {
            Action::Start => self.planned.is_none(),
            Action::CancelPlan => self.planned.is_some(),
            Action::End => self.endable(),
            Action::Open => self.selected().is_some_and(|r| r.proc.path.as_ref().is_some_and(|p| p.is_file())),
        }
    }

    /// Seconds until the countdown chosen ends.
    pub fn chosen_seconds(&self) -> u64 {
        if self.at_time {
            let t = rt_sys::local_time();
            processes::seconds_until(self.clock / 60, self.clock % 60, (t.hour, t.minute, t.second))
        } else {
            u64::from(DELAYS[self.delay]) * 60
        }
    }

    pub fn act(&mut self, a: Action, dialogs: &mut dyn Dialogs) {
        if !self.enabled(a) {
            return;
        }
        match a {
            Action::Start => {
                let secs = self.chosen_seconds();
                let what = if self.power == Power::Shutdown {
                    tf("Shut down the computer in {} (at {})?", &[&duration_text(secs), &clock_in(secs)])
                } else {
                    tf("Restart the computer in {} (at {})?", &[&duration_text(secs), &clock_in(secs)])
                };
                let text = format!(
                    "{what}\n\n{}",
                    tr("Windows does it even with rcare closed, and warns before. Save your work first; Cancel the Shutdown calls it off.")
                );
                if dialogs.confirm(&tr("Auto Shutdown"), &text) {
                    match processes::plan(self.power, secs, &self.file) {
                        Ok(p) => {
                            self.planned = Some(p);
                            self.note = None;
                        }
                        Err(why) => self.fx.info(&tr("Auto Shutdown"), &tf("Windows did not take it: {}", &[&why])),
                    }
                }
            }
            Action::CancelPlan => {
                self.planned = None;
                match processes::cancel(&self.file) {
                    Ok(()) => self.note = Some(tr("Called off: the computer will not shut down nor restart.")),
                    Err(why) => self.note = Some(tf("Nothing to call off: {}", &[&why])),
                }
            }
            Action::End => {
                let Some(r) = self.selected().cloned() else { return };
                let text = tf("End “{}” (process {})? What it has not saved is lost.", &[&r.proc.name, &r.proc.pid.to_string()]);
                if dialogs.confirm(&tr("Processes"), &text) {
                    match processes::end(r.proc.pid) {
                        Ok(()) => {
                            self.note = Some(tf("“{}” was ended.", &[&r.proc.name]));
                            self.refresh();
                        }
                        Err(why) => self.fx.info(&tr("Processes"), &tf("Could not end “{}”: {}", &[&r.proc.name, &why])),
                    }
                }
            }
            Action::Open => {
                if let Some(p) = self.selected().and_then(|r| r.proc.path.clone()) {
                    rt_sys::reveal(&p);
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
        let ip = 14.0 * s;
        let bh = (28.0 * s).round();
        let gap = 8.0 * s;
        let top = inner.y + (62.0 * s).round();
        let y = top + ip + 26.0 * s;
        // The controls in a row, or in two on a narrow page; they shrink together to fit.
        let wants = [96.0, 96.0, 80.0, 80.0, 28.0, 96.0, 28.0, 150.0, 150.0];
        let room = |n: usize, groups: usize| inner.w - 2.0 * ip - (n - 1) as f32 * gap - groups as f32 * 10.0 * s;
        let one_row = room(9, 3) / (wants.iter().sum::<f32>() * s) >= 0.85;
        let split = if one_row { 9 } else { 4 };
        let mut r = [Rect::default(); 9];
        for (k, range) in [0..split, split..9].iter().filter(|r| !r.is_empty()).enumerate() {
            let groups = range.clone().filter(|i| matches!(i, 1 | 3 | 6) && *i + 1 < range.end).count();
            let sum: f32 = wants[range.clone()].iter().sum();
            let fit = (room(range.len(), groups) / (sum * s)).min(1.0);
            let mut x = inner.x + ip;
            let ry = y + k as f32 * (bh + 8.0 * s);
            for i in range.clone() {
                let w = (wants[i] * s * fit).floor();
                r[i] = Rect::new(x, ry, w, bh);
                x += w + if matches!(i, 1 | 3 | 6) { gap + 10.0 * s } else { gap };
            }
        }
        let last = r[8].bottom();
        let plan_line = Rect::new(inner.x + ip, last + 6.0 * s, inner.w - 2.0 * ip, 20.0 * s);
        let card = Rect::new(inner.x, top, inner.w, (plan_line.bottom() + 10.0 * s - top).round());
        let ty = card.bottom() + (14.0 * s).round();
        let end = Rect::new(inner.x, ty, (150.0 * s).floor(), bh);
        let open = Rect::new(end.right() + gap, ty, (170.0 * s).floor(), bh);
        let header = Rect::new(inner.x, ty + bh + (10.0 * s).round(), inner.w, (26.0 * s).round());
        let status_h = (24.0 * s).round();
        let list = Rect::new(inner.x, header.bottom(), inner.w, (area.bottom() - status_h - header.bottom()).max(0.0));
        let w = inner.w;
        let name = (inner.x, (w * 0.26).max(140.0 * s).min(w));
        let pid = (name.0 + name.1, (70.0 * s).min((w - name.1).max(0.0)));
        let cpu = (pid.0 + pid.1, (70.0 * s).min((w - name.1 - pid.1).max(0.0)));
        let mem = (cpu.0 + cpu.1, (90.0 * s).min((w - name.1 - pid.1 - cpu.1).max(0.0)));
        let prog = (mem.0 + mem.1, (inner.right() - mem.0 - mem.1).max(0.0));
        Layout {
            area,
            card,
            power: [r[0], r[1]],
            mode: [r[2], r[3]],
            minus: r[4],
            value: r[5],
            plus: r[6],
            start: r[7],
            cancel: r[8],
            plan_line,
            end,
            open,
            header,
            list,
            row_h: (26.0 * s).round(),
            cols: [name, pid, cpu, mem, prog],
            status: Rect::new(area.x, area.bottom() - status_h, area.w, status_h),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn button(&self, g: &mut Gfx, r: Rect, label: &str, on: bool, chosen: bool, hit: Hit, th: &Theme, s: f32) {
        let fill = if chosen {
            th.accent
        } else if on && self.hot == Some(hit) {
            th.hover
        } else {
            th.bg
        };
        g.fill_rrect(r, 5.0 * s, fill);
        if !chosen {
            g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
        }
        let fg = if chosen {
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
        let t = rt_gfx::dialog::ellipsize(g, label, r.w - 8.0 * s, 12.5 * s, FontId::Regular);
        g.text_in(&t, r, 12.5 * s, FontId::Regular, fg, Align::Center);
    }

    /// The value of the countdown as shown: "1 h 30 min", or the time.
    fn value_text(&self) -> String {
        if self.at_time {
            format!("{:02}:{:02}", self.clock / 60, self.clock % 60)
        } else {
            duration_text(u64::from(DELAYS[self.delay]) * 60)
        }
    }

    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        use rt_core::i18n::strip_mnemonic;
        use rt_gfx::dialog::{ellipsize, ellipsize_middle};
        let l = Self::layout_in(area, s);
        g.fill_rect(area, th.bg);
        let pad = (24.0 * s).round();
        let (x, w) = (area.x + pad, (area.w - 2.0 * pad).max(0.0));
        g.text_in(&tr("Processes"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(
            g,
            &tr("What runs now, read again every 2 seconds; and Auto Shutdown, by Windows' own timer."),
            w,
            12.5 * s,
            FontId::Regular,
        );
        g.text_in(&about, Rect::new(x, area.y + pad + 32.0 * s, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
        // Auto Shutdown.
        let c = l.card;
        g.fill_rrect(c, 8.0 * s, th.panel);
        g.stroke_rrect(c, 8.0 * s, 1.0, th.border);
        let ip = 14.0 * s;
        g.text_in(
            &tr("Auto Shutdown"),
            Rect::new(c.x + ip, c.y + ip - 2.0 * s, c.w - 2.0 * ip, 20.0 * s),
            14.5 * s,
            FontId::Semibold,
            th.text,
            Align::Left,
        );
        let free = self.planned.is_none();
        for (r, p) in l.power.iter().zip([Power::Shutdown, Power::Restart]) {
            self.button(g, *r, &tr(p.label()), free, self.power == p, Hit::Power(p), th, s);
        }
        for (r, at) in l.mode.iter().zip([false, true]) {
            let label = if at { tr("At") } else { tr("In") };
            self.button(g, *r, &label, free, self.at_time == at, Hit::Mode(at), th, s);
        }
        self.button(g, l.minus, "−", free, false, Hit::Minus, th, s);
        g.text_in(&self.value_text(), l.value, 13.0 * s, FontId::Semibold, if free { th.text } else { th.dim }, Align::Center);
        self.button(g, l.plus, "+", free, false, Hit::Plus, th, s);
        for (r, a) in [(l.start, Action::Start), (l.cancel, Action::CancelPlan)] {
            let label = strip_mnemonic(&tr(a.label_key()));
            self.button(g, r, label.trim_end_matches("..."), self.enabled(a), false, Hit::Act(a), th, s);
        }
        let (line, color) = match self.planned {
            Some(p) => {
                let left = p.at.saturating_sub(processes::unix_now());
                let key = if p.power == Power::Shutdown { "Shutdown planned for {} (in {})." } else { "Restart planned for {} (in {})." };
                (tf(key, &[&clock_in(left), &duration_text(left)]), th.warn)
            }
            None => (tr("Nothing planned."), th.dim),
        };
        let line = ellipsize(g, &line, l.plan_line.w, 12.0 * s, FontId::Regular);
        g.text_in(&line, l.plan_line, 12.0 * s, FontId::Regular, color, Align::Left);
        // The processes.
        for (r, a) in [(l.end, Action::End), (l.open, Action::Open)] {
            let label = strip_mnemonic(&tr(a.label_key()));
            self.button(g, r, label.trim_end_matches("..."), self.enabled(a), false, Hit::Act(a), th, s);
        }
        let p = 12.0 * s;
        let hd = l.header;
        g.fill_rect(hd, th.header);
        g.fill_rect(Rect::new(hd.x, hd.bottom() - 1.0, hd.w, 1.0), th.border);
        let sorts = [Some(Sort::Name), Some(Sort::Pid), Some(Sort::Cpu), Some(Sort::Memory), None];
        for (i, title) in ["Name", "PID", "CPU", "Memory", "Program"].iter().enumerate() {
            let (cx, cw) = l.cols[i];
            let r = Rect::new(cx + 6.0 * s, hd.y, (cw - 12.0 * s).max(0.0), hd.h);
            let mut t = tr(title);
            if sorts[i] == Some(self.sort) {
                t.push_str(if self.sort == Sort::Name || self.sort == Sort::Pid { " ▲" } else { " ▼" });
            }
            let t = ellipsize(g, &t, r.w, p, FontId::Semibold);
            let right = (1..=3).contains(&i);
            g.text_in(&t, r, p, FontId::Semibold, th.dim, if right { Align::Right } else { Align::Left });
        }
        let page = l.page_rows();
        let end = (self.scroll + page).min(self.rows.len());
        g.set_clip(Some(l.list));
        for (k, i) in (self.scroll..end).enumerate() {
            let row = &self.rows[i];
            let y = l.list.y + k as f32 * l.row_h;
            let rr = Rect::new(l.list.x, y, l.list.w, l.row_h);
            if self.sel == Some(row.proc.pid) {
                g.fill_rect(rr, th.selection);
            } else if self.hot == Some(Hit::Row(i)) {
                g.fill_rect(rr, th.hover);
            }
            let vital = processes::protected(&row.proc.name, row.proc.pid, self.own_pid);
            let cell = |c: (f32, f32)| Rect::new(c.0 + 6.0 * s, y, (c.1 - 12.0 * s).max(0.0), l.row_h);
            let name = ellipsize(g, &row.proc.name, cell(l.cols[0]).w, p, FontId::Regular);
            g.text_in(&name, cell(l.cols[0]), p, FontId::Regular, if vital { th.dim } else { th.text }, Align::Left);
            g.text_in(&row.proc.pid.to_string(), cell(l.cols[1]), p, FontId::Regular, th.dim, Align::Right);
            let cpu = if row.cpu >= 0.05 { format!("{:.1} %", row.cpu) } else { "0 %".into() };
            g.text_in(&cpu, cell(l.cols[2]), p, FontId::Regular, if row.cpu >= 10.0 { th.warn } else { th.text }, Align::Right);
            let mem = if row.proc.memory > 0 { rsize::format::size(row.proc.memory) } else { "—".into() };
            g.text_in(&mem, cell(l.cols[3]), p, FontId::Regular, th.text, Align::Right);
            let prog = match &row.proc.path {
                Some(path) => path.display().to_string(),
                None if vital => tr("Windows needs it"),
                None => String::new(),
            };
            let prog = ellipsize_middle(g, &prog, cell(l.cols[4]).w, 11.5 * s, FontId::Regular);
            g.text_in(&prog, cell(l.cols[4]), 11.5 * s, FontId::Regular, th.dim, Align::Left);
        }
        g.set_clip(None);
        if self.rows.is_empty() {
            g.text_in(&tr("Reading the processes…"), l.list, p, FontId::Regular, th.dim, Align::Center);
        }
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let total: f32 = self.rows.iter().map(|r| r.cpu).sum();
        let left = self
            .note
            .clone()
            .unwrap_or_else(|| tf("{} processes. Processor: {} %.", &[&self.rows.len().to_string(), &format!("{:.0}", total.min(100.0))]));
        let sr = Rect::new(l.status.x + 10.0 * s, l.status.y, (l.status.w - 20.0 * s).max(0.0), l.status.h);
        let left = ellipsize(g, &left, sr.w, 12.0 * s, FontId::Regular);
        g.text_in(&left, sr, 12.0 * s, FontId::Regular, th.dim, Align::Left);
        self.layout = l;
    }

    // ------------------------------------------------------------ input

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let l = &self.layout;
        let buttons = [
            (l.power[0], Hit::Power(Power::Shutdown)),
            (l.power[1], Hit::Power(Power::Restart)),
            (l.mode[0], Hit::Mode(false)),
            (l.mode[1], Hit::Mode(true)),
            (l.minus, Hit::Minus),
            (l.plus, Hit::Plus),
            (l.start, Hit::Act(Action::Start)),
            (l.cancel, Hit::Act(Action::CancelPlan)),
            (l.end, Hit::Act(Action::End)),
            (l.open, Hit::Act(Action::Open)),
        ];
        if let Some((_, h)) = buttons.iter().find(|(r, _)| r.contains(x, y)) {
            return Some(*h);
        }
        if l.header.contains(x, y) {
            let sorts = [Sort::Name, Sort::Pid, Sort::Cpu, Sort::Memory];
            return l.cols.iter().take(4).position(|(cx, cw)| x >= *cx && x < cx + cw).map(|i| Hit::Header(sorts[i]));
        }
        if l.list.contains(x, y) && l.row_h > 0.0 {
            let i = self.scroll + ((y - l.list.y) / l.row_h) as usize;
            if i < self.rows.len() {
                return Some(Hit::Row(i));
            }
        }
        None
    }

    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        let hot = if blocked { None } else { self.hit(x, y) };
        if hot != self.hot {
            self.hot = hot;
            self.fx.redraw();
        }
        let free = self.planned.is_none();
        self.cursor_icon = match hot {
            Some(Hit::Act(a)) if self.enabled(a) => CursorIcon::Pointer,
            Some(Hit::Power(_) | Hit::Mode(_) | Hit::Minus | Hit::Plus) if free => CursorIcon::Pointer,
            Some(Hit::Header(_)) => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        };
    }

    pub fn pointer_left(&mut self) {
        if self.hot.take().is_some() {
            self.fx.redraw();
        }
    }

    pub fn release(&mut self) {}

    /// Moves the countdown's value one step.
    pub fn step(&mut self, up: bool) {
        if self.at_time {
            self.clock = if up { (self.clock + 15) % 1440 } else { (self.clock + 1440 - 15) % 1440 };
        } else if up {
            self.delay = (self.delay + 1).min(DELAYS.len() - 1);
        } else {
            self.delay = self.delay.saturating_sub(1);
        }
    }

    /// A click: a control changes the countdown, a header sorts, a row is selected; the buttons give their action.
    pub fn press(&mut self) -> Option<Action> {
        self.note = None;
        let free = self.planned.is_none();
        let mut out = None;
        match self.hot {
            Some(Hit::Power(p)) if free => self.power = p,
            Some(Hit::Mode(at)) if free => self.at_time = at,
            Some(Hit::Minus) if free => self.step(false),
            Some(Hit::Plus) if free => self.step(true),
            Some(Hit::Act(a)) => out = Some(a),
            Some(Hit::Header(sort)) => {
                self.sort = sort;
                self.sort_rows();
            }
            Some(Hit::Row(i)) => self.sel = self.rows.get(i).map(|r| r.proc.pid),
            _ => {}
        }
        self.fx.menus();
        self.fx.redraw();
        out
    }

    /// The arrows move in the list; Delete ends the selected process (after the question).
    pub fn key(&mut self, key: &winit::keyboard::Key) -> Option<Action> {
        use winit::keyboard::{Key, NamedKey};
        if matches!(key, Key::Named(NamedKey::Delete)) {
            return self.enabled(Action::End).then_some(Action::End);
        }
        let n = self.rows.len();
        if n == 0 {
            return None;
        }
        let page = self.layout.page_rows() as isize;
        let cur = self.sel.and_then(|pid| self.rows.iter().position(|r| r.proc.pid == pid)).map_or(-1, |i| i as isize);
        let to = |d: isize| (cur + d).clamp(0, n as isize - 1) as usize;
        let i = match key {
            Key::Named(NamedKey::ArrowDown) => to(1),
            Key::Named(NamedKey::ArrowUp) => to(-1),
            Key::Named(NamedKey::PageDown) => to(page),
            Key::Named(NamedKey::PageUp) => to(-page),
            Key::Named(NamedKey::Home) => 0,
            Key::Named(NamedKey::End) => n - 1,
            _ => return None,
        };
        self.sel = Some(self.rows[i].proc.pid);
        if i < self.scroll {
            self.scroll = i;
        } else if i >= self.scroll + self.layout.page_rows() {
            self.scroll = i + 1 - self.layout.page_rows();
        }
        self.note = None;
        self.fx.menus();
        self.fx.redraw();
        None
    }

    pub fn wheel(&mut self, delta: winit::event::MouseScrollDelta) {
        let rows = match delta {
            winit::event::MouseScrollDelta::LineDelta(_, l) => (-l * 3.0).round() as isize,
            winit::event::MouseScrollDelta::PixelDelta(p) => (-p.y as f32 / self.layout.row_h.max(20.0)).round() as isize,
        };
        let max = self.rows.len().saturating_sub(self.layout.page_rows());
        self.scroll = (self.scroll as isize + rows).clamp(0, max as isize) as usize;
        self.fx.redraw();
    }

    /// The page's nodes for the screen readers: the rows are 100 + their index.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role};
        let mut focus = None;
        let mut list = Node::new(Role::List);
        list.set_label(tr("Processes"));
        let mut ids = Vec::new();
        let end = (self.scroll + self.layout.page_rows()).min(self.rows.len());
        for i in self.scroll..end {
            let r = &self.rows[i];
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::ListItem);
            n.set_label(format!("{}, {:.1} %, {}", r.proc.name, r.cpu, rsize::format::size(r.proc.memory)));
            n.set_selected(self.sel == Some(r.proc.pid));
            n.add_action(accesskit::Action::Click);
            if self.sel == Some(r.proc.pid) {
                focus = Some(nid);
            }
            ids.push(nid);
            nodes.push((nid, n));
        }
        list.set_children(ids);
        nodes.push((NodeId(1), list));
        children.push(NodeId(1));
        focus
    }

    /// A screen reader clicked the row `i`: it is selected.
    pub fn accessibility_click(&mut self, i: usize) {
        if let Some(r) = self.rows.get(i) {
            self.sel = Some(r.proc.pid);
            self.fx.menus();
            self.fx.redraw();
        }
    }
}

/// A delay as people say it: "45 min", "2 h", "1 h 30 min".
pub fn duration_text(secs: u64) -> String {
    let mins = secs.div_ceil(60);
    let (h, m) = (mins / 60, mins % 60);
    match (h, m) {
        (0, m) => tf("{} min", &[&m.to_string()]),
        (h, 0) => tf("{} h", &[&h.to_string()]),
        (h, m) => tf("{} h {} min", &[&h.to_string(), &m.to_string()]),
    }
}

/// The clock time `secs` seconds from now, "hh:mm".
pub fn clock_in(secs: u64) -> String {
    let t = rt_sys::local_time();
    let at = (u64::from(t.hour) * 3600 + u64::from(t.minute) * 60 + u64::from(t.second) + secs) % 86_400;
    format!("{:02}:{:02}", at / 3600, at % 3600 / 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, name: &str, cpu_time: u64, memory: u64) -> Proc {
        Proc { pid, name: name.into(), path: None, memory, cpu_time, created: 7 }
    }

    fn page() -> ProcessesPage {
        let dir = std::env::temp_dir().join(format!("rcare-procs-{}", std::process::id()));
        ProcessesPage::new(&dir)
    }

    #[test]
    fn processor_use_comes_from_two_readings() {
        let mut p = page();
        p.take(vec![proc(0, "Idle", 0, 0), proc(10, "a.exe", 0, 100), proc(11, "b.exe", 0, 300)], None);
        assert_eq!(p.rows.len(), 2, "the idle process is left out");
        assert!(p.rows.iter().all(|r| r.cpu == 0.0));
        let cpus = processes::cpus() as u64;
        // a.exe used one whole processor for the second; b.exe nothing.
        p.take(vec![proc(10, "a.exe", 10_000_000, 100), proc(11, "b.exe", 0, 300)], Some(Duration::from_secs(1)));
        assert_eq!(p.rows[0].proc.name, "a.exe", "sorted by processor use");
        assert!((p.rows[0].cpu - 100.0 / cpus as f32).abs() < 0.01);
        p.sort = Sort::Memory;
        p.sort_rows();
        assert_eq!(p.rows[0].proc.name, "b.exe");
        // The selection follows the process; one that is gone is not selected.
        p.sel = Some(11);
        p.take(vec![proc(10, "a.exe", 10_000_000, 100)], Some(Duration::from_secs(1)));
        assert_eq!(p.sel, None);
    }

    #[test]
    fn what_can_be_done() {
        let mut p = page();
        p.planned = None;
        p.take(vec![proc(4, "System", 0, 1), proc(600, "csrss.exe", 0, 1), proc(900, "notepad.exe", 0, 1)], None);
        assert!(p.enabled(Action::Start) && !p.enabled(Action::CancelPlan) && !p.enabled(Action::End));
        p.sel = Some(600);
        assert!(!p.enabled(Action::End), "Windows needs it");
        p.sel = Some(900);
        assert!(p.enabled(Action::End));
        assert!(!p.enabled(Action::Open), "no program known");
        p.planned = Some(Planned { power: Power::Restart, at: u64::MAX });
        assert!(!p.enabled(Action::Start) && p.enabled(Action::CancelPlan));
    }

    #[test]
    fn the_countdown_steps() {
        let mut p = page();
        p.delay = 0;
        p.step(false);
        assert_eq!(p.delay, 0);
        p.step(true);
        assert_eq!(p.chosen_seconds(), 600);
        p.delay = DELAYS.len() - 1;
        p.step(true);
        assert_eq!(p.delay, DELAYS.len() - 1);
        p.at_time = true;
        p.clock = 0;
        p.step(false);
        assert_eq!(p.clock, 23 * 60 + 45);
        p.step(true);
        assert_eq!(p.clock, 0);
        let s = p.chosen_seconds();
        assert!(s > 0 && s <= 86_400);
        assert_eq!(p.value_text(), "00:00");
    }

    #[test]
    fn durations() {
        assert_eq!(duration_text(45 * 60), "45 min");
        assert_eq!(duration_text(2 * 3600), "2 h");
        assert_eq!(duration_text(90 * 60), "1 h 30 min");
        assert_eq!(duration_text(59), "1 min");
        assert_eq!(clock_in(0).len(), 5);
    }

    #[test]
    fn rows_and_headers_are_found_under_the_pointer() {
        let mut p = page();
        p.take((1..40).map(|i| proc(i * 10, &format!("p{i}.exe"), 0, u64::from(i))).collect(), None);
        p.layout = ProcessesPage::layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        let l = p.layout.clone();
        assert_eq!(p.hit(l.cols[0].0 + 10.0, l.list.y + l.row_h * 1.5), Some(Hit::Row(1)));
        assert_eq!(p.hit(l.cols[3].0 + 10.0, l.header.y + 5.0), Some(Hit::Header(Sort::Memory)));
        assert_eq!(p.hit(l.cols[4].0 + 10.0, l.header.y + 5.0), None);
        assert_eq!(p.hit(l.plus.x + 2.0, l.plus.y + 2.0), Some(Hit::Plus));
        assert_eq!(p.hit(l.start.x + 2.0, l.start.y + 2.0), Some(Hit::Act(Action::Start)));
        // The controls do not overlap and stay inside the card.
        let row = [l.power[0], l.power[1], l.mode[0], l.mode[1], l.minus, l.value, l.plus, l.start, l.cancel];
        for w in row.windows(2) {
            assert!(w[0].right() <= w[1].x);
        }
        assert!(l.cancel.right() <= l.card.right());
        p.hot = Some(Hit::Header(Sort::Name));
        p.press();
        assert_eq!(p.rows[0].proc.name, "p1.exe");
        p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::End));
        assert_eq!(p.sel, Some(p.rows.last().unwrap().proc.pid));
    }
}
