//! The Scheduler page: a new schedule (what runs, how often, at what time),
//! the schedules rcare made with what Task Scheduler says of each (next run,
//! last result), and the last runs with what they came to. Making, running
//! and deleting go through Windows' Task Scheduler (`schedule`), in a thread,
//! after asking; administrator rights.

use crate::bg::Job;
use crate::i18n::{tf, tr};
use crate::schedule::{self, Freq, Op, Run, Schedule, TaskStatus};
use rt_gfx::gfx::{Align, Color, FontId, Gfx, Rect};
use rt_ui::page::{Dialogs, Effects};
use rt_ui::theme::Theme;
use std::collections::HashMap;
use std::path::PathBuf;
use winit::window::CursorIcon;

/// What the page does (its buttons and menu entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Create,
    RunNow,
    Delete,
    Log,
    Refresh,
}

pub const ACTIONS: [Action; 5] = [Action::Create, Action::RunNow, Action::Delete, Action::Log, Action::Refresh];

impl Action {
    /// The menu label (a translation key with its access key).
    pub fn label_key(self) -> &'static str {
        match self {
            Action::Create => "&Create the Schedule...",
            Action::RunNow => "Run &Now",
            Action::Delete => "&Delete the Schedule...",
            Action::Log => "Open the &Log",
            Action::Refresh => "&Refresh",
        }
    }
}

/// The new schedule being set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Form {
    /// Ticked, in `Op::ALL`'s order.
    pub ops: [bool; 4],
    /// 0 daily, 1 weekly, 2 monthly.
    pub freq: u8,
    /// 0 Monday … 6 Sunday.
    pub weekday: u8,
    /// 1–28.
    pub day: u8,
    /// Minutes after midnight, in steps of 30.
    pub time: u16,
}

impl Default for Form {
    fn default() -> Self {
        // SFC and the junk, on Sunday nights.
        Form { ops: [true, false, false, true], freq: 1, weekday: 6, day: 1, time: 3 * 60 }
    }
}

impl Form {
    pub fn ops(&self) -> Vec<Op> {
        Op::ALL.iter().zip(self.ops).filter(|(_, on)| *on).map(|(o, _)| *o).collect()
    }

    pub fn freq(&self) -> Freq {
        match self.freq {
            0 => Freq::Daily,
            1 => Freq::Weekly(self.weekday.min(6)),
            _ => Freq::Monthly(self.day.clamp(1, 28)),
        }
    }

    pub fn schedule(&self, name: String) -> Schedule {
        Schedule { name, ops: self.ops(), freq: self.freq(), hour: (self.time / 60) as u8, minute: (self.time % 60) as u8 }
    }
}

/// The weekdays' short names (translation keys).
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const WEEKDAYS_LONG: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/// When a schedule runs, in words.
pub fn when_text(s: &Schedule) -> String {
    let t = s.time();
    match s.freq {
        Freq::Daily => tf("Every day at {}", &[&t]),
        Freq::Weekly(d) => tf("Every {} at {}", &[&tr(WEEKDAYS_LONG[usize::from(d.min(6))]), &t]),
        Freq::Monthly(d) => tf("On day {} of each month at {}", &[&d.to_string(), &t]),
    }
}

/// `2026-11-29T03:00:00` → `2026-11-29 03:00`.
pub fn short_time(s: &str) -> String {
    s.replace('T', " ").chars().take(16).collect()
}

/// What a schedule runs, in words.
pub fn ops_label(ops: &[Op]) -> String {
    ops.iter().map(|o| tr(o.label())).collect::<Vec<_>>().join(", ")
}

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Act(usize),
    Op(usize),
    Freq(usize),
    Weekday(usize),
    DayDown,
    DayUp,
    TimeDown,
    TimeUp,
    Row(usize),
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub actions: [Rect; 5],
    pub form: Rect,
    pub ops: [Rect; 4],
    pub freqs: [Rect; 3],
    pub weekdays: [Rect; 7],
    pub day: [Rect; 2],
    pub time: [Rect; 2],
    pub list: Rect,
    pub runs: Rect,
    pub row_h: f32,
    pub status: Rect,
}

/// What a change in a thread was, for its answer.
#[derive(Clone, Debug)]
enum Change {
    Create(Schedule),
    Delete(String),
    RunNow,
}

#[derive(Default)]
pub struct SchedulePage {
    pub list: Vec<Schedule>,
    pub status: HashMap<String, TaskStatus>,
    pub runs: Vec<Run>,
    pub form: Form,
    pub sel: Option<usize>,
    pub admin: bool,
    pub data_dir: PathBuf,
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    pub note: Option<String>,
    reading: Job<Option<Vec<(String, TaskStatus)>>>,
    changing: Job<(Change, Result<(), String>)>,
}

impl SchedulePage {
    pub fn new(admin: bool, data_dir: PathBuf) -> SchedulePage {
        SchedulePage { admin, data_dir, ..SchedulePage::default() }
    }

    pub fn busy(&self) -> bool {
        self.changing.busy()
    }

    /// Reads the schedules and the runs again, and asks Task Scheduler in a thread.
    pub fn refresh(&mut self) {
        self.list = schedule::load(&self.data_dir);
        self.runs = schedule::runs(&self.data_dir);
        self.sel = self.sel.filter(|i| *i < self.list.len());
        self.reading.start(schedule::status);
        self.fx.menus();
        self.fx.redraw();
    }

    /// While working: takes what came, and when to look again.
    pub fn animate(&mut self) -> Option<std::time::Instant> {
        if let Some(st) = self.reading.take() {
            self.status = st.unwrap_or_default().into_iter().collect();
            self.fx.redraw();
        }
        if let Some((change, res)) = self.changing.take() {
            match (change, res) {
                (Change::Create(s), Ok(())) => {
                    self.list.retain(|x| x.name != s.name);
                    self.list.push(s.clone());
                    if let Err(e) = schedule::save(&self.data_dir, &self.list) {
                        self.fx.info(&tr("Scheduler"), &tf("Could not keep the schedules: {}", &[&e.to_string()]));
                    }
                    self.note = Some(tf("Scheduled: {}.", &[&when_text(&s)]));
                }
                (Change::Delete(name), res) => {
                    // A task already gone from Task Scheduler is left out all the same.
                    if let Err(e) = &res {
                        self.note = Some(tf("Task Scheduler said: {}", &[e]));
                    } else {
                        self.note = Some(tr("The schedule was deleted."));
                    }
                    self.list.retain(|x| x.name != name);
                    let _ = schedule::save(&self.data_dir, &self.list);
                    self.sel = None;
                }
                (Change::RunNow, Ok(())) => self.note = Some(tr("Started: its result shows in the last runs when it ends.")),
                (_, Err(e)) => self.fx.info(&tr("Scheduler"), &tf("Task Scheduler did not do it: {}", &[&e])),
            }
            self.refresh();
        }
        (self.reading.busy() || self.changing.busy()).then(crate::bg::soon)
    }

    pub fn enabled(&self, a: Action) -> bool {
        match a {
            Action::Create => self.admin && !self.busy() && !self.form.ops().is_empty(),
            Action::RunNow | Action::Delete => self.admin && !self.busy() && self.sel.is_some_and(|i| i < self.list.len()),
            Action::Log => schedule::log_file(&self.data_dir).exists(),
            Action::Refresh => !self.reading.busy(),
        }
    }

    /// Does what a button or its menu entry asks.
    pub fn act(&mut self, a: Action, dialogs: &mut dyn Dialogs) {
        if !self.enabled(a) {
            if matches!(a, Action::Create | Action::RunNow | Action::Delete) && !self.admin {
                self.fx.info(&tr("Scheduler"), &tr("Scheduling needs administrator rights: restart rcare as administrator (File menu)."));
            }
            return;
        }
        match a {
            Action::Create => {
                let s = self.form.schedule(schedule::free_name(&self.list));
                let text = format!(
                    "{}\n\n  {}\n\n{}",
                    tf("{}, run:", &[&when_text(&s)]),
                    ops_label(&s.ops),
                    tr("Windows' Task Scheduler runs it with the highest rights while your session is open: the computer must be on then. The junk is cleaned as ticked in the Disk Cleaner; never the registry or shortcuts.")
                );
                if dialogs.confirm(&tr("Scheduler"), &text) {
                    self.changing.start(move || {
                        let r = schedule::create(&s);
                        (Change::Create(s), r)
                    });
                }
            }
            Action::Delete => {
                let Some(s) = self.sel.and_then(|i| self.list.get(i)).cloned() else { return };
                let text = tf("Delete the schedule “{}” ({})?", &[&ops_label(&s.ops), &when_text(&s)]);
                if dialogs.confirm(&tr("Scheduler"), &text) {
                    self.changing.start(move || {
                        let r = schedule::delete(&s);
                        (Change::Delete(s.name), r)
                    });
                }
            }
            Action::RunNow => {
                let Some(s) = self.sel.and_then(|i| self.list.get(i)).cloned() else { return };
                let text = format!(
                    "{}\n\n{}",
                    tf("Run “{}” now?", &[&ops_label(&s.ops)]),
                    tr("It runs without a window and may take a long time (SFC and DISM, 20 minutes or more).")
                );
                if dialogs.confirm(&tr("Scheduler"), &text) {
                    self.changing.start(move || (Change::RunNow, schedule::run_now(&s)));
                }
            }
            Action::Log => rt_sys::reveal(&schedule::log_file(&self.data_dir)),
            Action::Refresh => self.refresh(),
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
            Action::Create => 190.0,
            Action::RunNow => 110.0,
            Action::Delete => 180.0,
            Action::Log => 140.0,
            Action::Refresh => 100.0,
        });
        let gap = 8.0 * s;
        let fit = ((inner.w - 4.0 * gap) / (wants.iter().sum::<f32>() * s)).min(1.0);
        let mut actions = [Rect::default(); 5];
        let mut x = inner.x;
        for (i, want) in wants.iter().enumerate() {
            let w = (want * s * fit).floor();
            actions[i] = Rect::new(x, top, w, bh);
            x += w + gap;
        }
        let status_h = (24.0 * s).round();
        let y = top + bh + (16.0 * s).round();
        let h = (area.bottom() - status_h - (12.0 * s).round() - y).max(0.0);
        let form_w = (inner.w * 0.4).clamp((240.0 * s).min(inner.w * 0.6), 380.0 * s);
        let form = Rect::new(inner.x, y, form_w, h);
        let ip = 14.0 * s;
        let row = (26.0 * s).round();
        let mut fy = form.y + ip + 26.0 * s;
        let ops = [0, 1, 2, 3].map(|i| Rect::new(form.x + ip, fy + i as f32 * row, form.w - 2.0 * ip, row));
        fy += 4.0 * row + 30.0 * s;
        let cw = ((form.w - 2.0 * ip - 2.0 * gap) / 3.0).floor();
        let freqs = [0, 1, 2].map(|i| Rect::new(form.x + ip + i as f32 * (cw + gap), fy, cw, (28.0 * s).round()));
        fy += 36.0 * s;
        let dw = ((form.w - 2.0 * ip - 6.0 * 4.0 * s) / 7.0).floor();
        let weekdays = [0, 1, 2, 3, 4, 5, 6].map(|i| Rect::new(form.x + ip + i as f32 * (dw + 4.0 * s), fy, dw, (26.0 * s).round()));
        let sb = (28.0 * s).round();
        let day = [Rect::new(form.x + ip, fy, sb, sb), Rect::new(form.x + ip + 110.0 * s, fy, sb, sb)];
        fy += 64.0 * s;
        let time = [Rect::new(form.x + ip, fy, sb, sb), Rect::new(form.x + ip + 110.0 * s, fy, sb, sb)];
        let rx = form.right() + (16.0 * s).round();
        let rw = (inner.right() - rx).max(0.0);
        let list = Rect::new(rx, y + 22.0 * s, rw, ((h - 44.0 * s) * 0.5).max(0.0));
        let runs = Rect::new(rx, list.bottom() + 22.0 * s, rw, (y + h - list.bottom() - 22.0 * s).max(0.0));
        Layout {
            area,
            actions,
            form,
            ops,
            freqs,
            weekdays,
            day,
            time,
            list,
            runs,
            row_h: (46.0 * s).round(),
            status: Rect::new(area.x, area.bottom() - status_h, area.w, status_h),
        }
    }

    fn chip(g: &mut Gfx, r: Rect, label: &str, on: bool, hot: bool, th: &Theme, s: f32) {
        g.fill_rrect(
            r,
            5.0 * s,
            if on {
                th.accent
            } else if hot {
                th.hover
            } else {
                th.bg
            },
        );
        if !on {
            g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
        }
        let fg = if on {
            if th.dark {
                Color::rgb(0x000000)
            } else {
                Color::rgb(0xFFFFFF)
            }
        } else {
            th.text
        };
        // Narrow chips (the weekdays in a small window) in smaller letters before they are cut.
        let px = if g.measure(label, 12.0 * s, FontId::Regular) > r.w - 4.0 * s { 10.5 * s } else { 12.0 * s };
        let t = rt_gfx::dialog::ellipsize(g, label, r.w - 2.0 * s, px, FontId::Regular);
        g.text_in(&t, r, px, FontId::Regular, fg, Align::Center);
    }

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        use rt_core::i18n::strip_mnemonic;
        use rt_gfx::dialog::ellipsize;
        let l = Self::layout_in(area, s);
        g.fill_rect(area, th.bg);
        let pad = (24.0 * s).round();
        let (x, w) = (area.x + pad, (area.w - 2.0 * pad).max(0.0));
        g.text_in(&tr("Scheduler"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(
            g,
            &tr("The slow work at a set time, through Windows' Task Scheduler, even with rcare closed."),
            w,
            12.5 * s,
            FontId::Regular,
        );
        g.text_in(&about, Rect::new(x, area.y + pad + 32.0 * s, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
        for (i, a) in ACTIONS.iter().enumerate() {
            let r = l.actions[i];
            let on = self.enabled(*a);
            let primary = *a == Action::Create && on;
            let label = strip_mnemonic(&tr(a.label_key()));
            let label = label.trim_end_matches("...").to_string();
            if primary {
                Self::chip(g, r, &label, true, false, th, s);
            } else {
                g.fill_rrect(r, 5.0 * s, if on && self.hot == Some(Hit::Act(i)) { th.hover } else { th.bg });
                g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
                let t = ellipsize(g, &label, r.w - 10.0 * s, 12.5 * s, FontId::Regular);
                g.text_in(&t, r, 12.5 * s, FontId::Regular, if on { th.text } else { th.dim }, Align::Center);
            }
        }
        // The new schedule.
        let f = l.form;
        g.fill_rrect(f, 8.0 * s, th.panel);
        g.stroke_rrect(f, 8.0 * s, 1.0, th.border);
        let ip = 14.0 * s;
        let heading = |g: &mut Gfx, y: f32, text: &str| {
            g.text_in(text, Rect::new(f.x + ip, y, f.w - 2.0 * ip, 20.0 * s), 13.0 * s, FontId::Semibold, th.text, Align::Left);
        };
        heading(g, f.y + ip, &tr("What runs"));
        for (i, op) in Op::ALL.iter().enumerate() {
            let r = l.ops[i];
            let bx = Rect::new(r.x, r.y + (r.h - 16.0 * s) / 2.0, 16.0 * s, 16.0 * s);
            if self.form.ops[i] {
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
            let t = ellipsize(g, &tr(op.label()), r.w - 26.0 * s, 12.5 * s, FontId::Regular);
            g.text_in(&t, Rect::new(bx.right() + 10.0 * s, r.y, r.w - 26.0 * s, r.h), 12.5 * s, FontId::Regular, th.text, Align::Left);
        }
        heading(g, l.freqs[0].y - 26.0 * s, &tr("When"));
        for (i, k) in ["Daily", "Weekly", "Monthly"].iter().enumerate() {
            Self::chip(g, l.freqs[i], &tr(k), self.form.freq == i as u8, self.hot == Some(Hit::Freq(i)), th, s);
        }
        let stepper = |g: &mut Gfx, b: [Rect; 2], value: &str, hot_down: bool, hot_up: bool| {
            Self::chip(g, b[0], "−", false, hot_down, th, s);
            Self::chip(g, b[1], "+", false, hot_up, th, s);
            let mid = Rect::new(b[0].right(), b[0].y, b[1].x - b[0].right(), b[0].h);
            g.text_in(value, mid, 13.0 * s, FontId::Semibold, th.text, Align::Center);
        };
        match self.form.freq {
            1 => {
                for (i, d) in WEEKDAYS.iter().enumerate() {
                    Self::chip(g, l.weekdays[i], &tr(d), self.form.weekday == i as u8, self.hot == Some(Hit::Weekday(i)), th, s);
                }
            }
            2 => stepper(
                g,
                l.day,
                &tf("Day {}", &[&self.form.day.to_string()]),
                self.hot == Some(Hit::DayDown),
                self.hot == Some(Hit::DayUp),
            ),
            _ => {}
        }
        heading(g, l.time[0].y - 26.0 * s, &tr("Time"));
        let time = format!("{:02}:{:02}", self.form.time / 60, self.form.time % 60);
        stepper(g, l.time, &time, self.hot == Some(Hit::TimeDown), self.hot == Some(Hit::TimeUp));
        let note = tr("While your session is open and the computer is on.");
        let nr = Rect::new(f.x + ip, l.time[0].bottom() + 10.0 * s, f.w - 2.0 * ip, 18.0 * s);
        let nt = ellipsize(g, &note, nr.w, 11.0 * s, FontId::Regular);
        g.text_in(&nt, nr, 11.0 * s, FontId::Regular, th.dim, Align::Left);
        // The schedules.
        let small = 11.5 * s;
        g.text_in(
            &tr("Schedules"),
            Rect::new(l.list.x, l.list.y - 22.0 * s, l.list.w, 20.0 * s),
            13.0 * s,
            FontId::Semibold,
            th.text,
            Align::Left,
        );
        g.stroke_rrect(l.list, 4.0 * s, 1.0, th.border);
        g.set_clip(Some(l.list));
        if self.list.is_empty() {
            g.text_in(&tr("None yet: set one on the left and press Create."), l.list, 12.0 * s, FontId::Regular, th.dim, Align::Center);
        }
        for (i, sc) in self.list.iter().enumerate() {
            let y = l.list.y + i as f32 * l.row_h;
            let r = Rect::new(l.list.x + 1.0, y, l.list.w - 2.0, l.row_h);
            if self.sel == Some(i) {
                g.fill_rect(r, th.selection);
            } else if self.hot == Some(Hit::Row(i)) {
                g.fill_rect(r, th.hover);
            }
            let tw = r.w - 20.0 * s;
            let t = ellipsize(g, &format!("{} — {}", when_text(sc), ops_label(&sc.ops)), tw, 12.5 * s, FontId::Semibold);
            g.text_in(&t, Rect::new(r.x + 10.0 * s, y + 4.0 * s, tw, 20.0 * s), 12.5 * s, FontId::Semibold, th.text, Align::Left);
            let (line, color) = match self.status.get(&sc.name) {
                None if self.reading.busy() => (tr("Asking Task Scheduler…"), th.dim),
                None => (tr("Not in Task Scheduler: create it again."), th.warn),
                Some(st) => {
                    let next = if st.next.is_empty() { String::new() } else { tf("next {}", &[&short_time(&st.next)]) };
                    let last = match (st.last.is_empty(), st.result) {
                        (true, _) => tr("not run yet"),
                        (false, Some(0)) => tf("last {}: ok", &[&short_time(&st.last)]),
                        (false, Some(n)) => tf("last {}: code {}", &[&short_time(&st.last), &n.to_string()]),
                        (false, None) => tf("last {}", &[&short_time(&st.last)]),
                    };
                    (
                        format!("{} · {next} · {last}", st.state),
                        if st.result.is_some_and(|r| r != 0 && r != 267011) { th.warn } else { th.dim },
                    )
                }
            };
            let lt = ellipsize(g, &line, tw, small, FontId::Regular);
            g.text_in(&lt, Rect::new(r.x + 10.0 * s, y + 24.0 * s, tw, 18.0 * s), small, FontId::Regular, color, Align::Left);
        }
        g.set_clip(None);
        // The last runs.
        g.text_in(
            &tr("Last runs"),
            Rect::new(l.runs.x, l.runs.y - 22.0 * s, l.runs.w, 20.0 * s),
            13.0 * s,
            FontId::Semibold,
            th.text,
            Align::Left,
        );
        g.stroke_rrect(l.runs, 4.0 * s, 1.0, th.border);
        g.set_clip(Some(l.runs));
        if self.runs.is_empty() {
            g.text_in(&tr("Nothing has run yet."), l.runs, 12.0 * s, FontId::Regular, th.dim, Align::Center);
        }
        let lh = 20.0 * s;
        for (k, run) in self.runs.iter().enumerate().take(((l.runs.h - 8.0 * s) / lh).max(0.0) as usize) {
            let y = l.runs.y + 4.0 * s + k as f32 * lh;
            let r = Rect::new(l.runs.x + 10.0 * s, y, l.runs.w - 20.0 * s, lh);
            let color = if run.ok { th.good } else { th.warn };
            let t = ellipsize(g, &format!("{}  {} — {}", run.when, tr(run.op.label()), run.detail), r.w, small, FontId::Regular);
            g.text_in(&t, r, small, FontId::Regular, color, Align::Left);
        }
        g.set_clip(None);
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let left = self.note.clone().unwrap_or_else(|| {
            if !self.admin {
                tr("Scheduling needs administrator rights.")
            } else if self.busy() {
                tr("Asking Task Scheduler…")
            } else {
                String::new()
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
        let at = |rs: &[Rect]| rs.iter().position(|r| r.contains(x, y));
        if let Some(i) = at(&l.actions) {
            return Some(Hit::Act(i));
        }
        if let Some(i) = at(&l.ops) {
            return Some(Hit::Op(i));
        }
        if let Some(i) = at(&l.freqs) {
            return Some(Hit::Freq(i));
        }
        if self.form.freq == 1 {
            if let Some(i) = at(&l.weekdays) {
                return Some(Hit::Weekday(i));
            }
        }
        if self.form.freq == 2 {
            if l.day[0].contains(x, y) {
                return Some(Hit::DayDown);
            }
            if l.day[1].contains(x, y) {
                return Some(Hit::DayUp);
            }
        }
        if l.time[0].contains(x, y) {
            return Some(Hit::TimeDown);
        }
        if l.time[1].contains(x, y) {
            return Some(Hit::TimeUp);
        }
        if l.list.contains(x, y) && l.row_h > 0.0 {
            let i = ((y - l.list.y) / l.row_h) as usize;
            return (i < self.list.len()).then_some(Hit::Row(i));
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
            Some(Hit::Act(i)) if !self.enabled(ACTIONS[i]) => CursorIcon::Default,
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

    /// A click: the form changes, a row is selected; a button gives its action for the window to do.
    pub fn press(&mut self) -> Option<Action> {
        let f = &mut self.form;
        let out = match self.hot? {
            Hit::Act(i) => return Some(ACTIONS[i]),
            Hit::Op(i) => {
                f.ops[i] = !f.ops[i];
                None
            }
            Hit::Freq(i) => {
                f.freq = i as u8;
                None
            }
            Hit::Weekday(i) => {
                f.weekday = i as u8;
                None
            }
            Hit::DayDown => {
                f.day = if f.day <= 1 { 28 } else { f.day - 1 };
                None
            }
            Hit::DayUp => {
                f.day = if f.day >= 28 { 1 } else { f.day + 1 };
                None
            }
            Hit::TimeDown => {
                f.time = (f.time + 24 * 60 - 30) % (24 * 60);
                None
            }
            Hit::TimeUp => {
                f.time = (f.time + 30) % (24 * 60);
                None
            }
            Hit::Row(i) => {
                self.sel = Some(i);
                None
            }
        };
        self.note = None;
        self.fx.menus();
        self.fx.redraw();
        out
    }

    /// F5 reads again.
    pub fn key(&mut self, key: &winit::keyboard::Key) -> Option<Action> {
        use winit::keyboard::{Key, NamedKey};
        matches!(key, Key::Named(NamedKey::F5)).then_some(Action::Refresh)
    }

    /// The page's nodes for the screen readers: the schedules are 100 + their index.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role};
        let mut list = Node::new(Role::List);
        list.set_label(tr("Schedules"));
        let mut ids = Vec::new();
        for (i, sc) in self.list.iter().enumerate() {
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::ListItem);
            n.set_label(format!("{}: {}", when_text(sc), ops_label(&sc.ops)));
            n.set_selected(self.sel == Some(i));
            n.add_action(accesskit::Action::Click);
            ids.push(nid);
            nodes.push((nid, n));
        }
        list.set_children(ids);
        nodes.push((NodeId(1), list));
        children.push(NodeId(1));
        self.sel.map(|i| NodeId(100 + i as u64))
    }

    /// A screen reader clicked a schedule: it is selected.
    pub fn accessibility_click(&mut self, i: usize) {
        if i < self.list.len() {
            self.sel = Some(i);
            self.fx.menus();
            self.fx.redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form() {
        let f = Form::default();
        assert_eq!(f.ops(), vec![Op::Sfc, Op::Clean]);
        let s = f.schedule("Schedule 1".into());
        assert_eq!((s.freq, s.hour, s.minute), (Freq::Weekly(6), 3, 0));
        let mut p = SchedulePage::new(true, std::env::temp_dir());
        p.layout = SchedulePage::layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        let l = p.layout.clone();
        // Monthly, then the day and the time go round.
        p.pointer(l.freqs[2].x + 3.0, l.freqs[2].y + 3.0, false);
        p.press();
        assert_eq!(p.form.freq, 2);
        p.pointer(l.day[0].x + 3.0, l.day[0].y + 3.0, false);
        p.press();
        assert_eq!(p.form.day, 28, "before day 1, day 28");
        p.pointer(l.time[0].x + 3.0, l.time[0].y + 3.0, false);
        for _ in 0..7 {
            p.press();
        }
        assert_eq!(p.form.time, 23 * 60 + 30, "before midnight, back round");
        p.pointer(l.ops[0].x + 3.0, l.ops[0].y + 8.0, false);
        p.press();
        p.pointer(l.ops[3].x + 3.0, l.ops[3].y + 8.0, false);
        p.press();
        assert!(p.form.ops().is_empty() && !p.enabled(Action::Create), "nothing to run");
        assert!(!p.enabled(Action::RunNow), "nothing selected");
    }

    #[test]
    fn words() {
        crate::i18n::set_lang(crate::i18n::Lang::EnUS);
        let s = Schedule { name: "Schedule 1".into(), ops: vec![Op::Dism], freq: Freq::Weekly(6), hour: 2, minute: 30 };
        assert_eq!(when_text(&s), "Every Sunday at 02:30");
        assert_eq!(when_text(&Schedule { freq: Freq::Monthly(5), ..s.clone() }), "On day 5 of each month at 02:30");
        assert_eq!(ops_label(&[Op::Sfc, Op::Clean]), "System files (SFC), Clean the junk ticked");
        assert_eq!(short_time("2026-11-29T03:00:00"), "2026-11-29 03:00");
        // Without rights it says why, and nothing starts.
        let mut p = SchedulePage::new(false, std::env::temp_dir());
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
        }
        p.act(Action::Create, &mut No);
        assert!(p.fx.take().info.is_some() && !p.busy());
    }
}
