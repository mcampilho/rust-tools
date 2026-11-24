//! The System Information page: six cards — Windows, the computer, the
//! graphics, the drives, the devices with a problem and the battery — as
//! Windows reports them (`sysinfo`), and Windows' own options: Fast Startup
//! (administrator), Default Apps and Windows Update's driver updates
//! (Windows' own pages). "Copy" puts it all on the clipboard as text.

use crate::i18n::{tf, tr};
use crate::sysinfo::{self, Info};
use rt_gfx::gfx::{Align, FontId, Gfx, Rect};
use rt_ui::page::Effects;
use rt_ui::theme::Theme;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use winit::window::CursorIcon;

/// What the page does (its buttons and menu entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Refresh,
    Copy,
    FastStartup,
    DefaultApps,
    Drivers,
}

pub const ACTIONS: [Action; 5] = [Action::Refresh, Action::Copy, Action::FastStartup, Action::DefaultApps, Action::Drivers];

impl Action {
    /// The menu label (a translation key with its access key).
    pub fn label_key(self) -> &'static str {
        match self {
            Action::Refresh => "&Refresh",
            Action::Copy => "&Copy",
            Action::FastStartup => "Fast &Startup",
            Action::DefaultApps => "&Default Apps",
            Action::Drivers => "Driver &Updates",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub actions: [Rect; 5],
    pub cards: [Rect; 6],
    pub status: Rect,
}

/// A line of a card: label, value, and whether it is good (None: neither).
type Line = (String, String, Option<bool>);
/// A card: its title and lines.
type Card = (String, Vec<Line>);

#[derive(Default)]
pub struct SysPage {
    pub info: Option<Info>,
    pub failed: bool,
    pub admin: bool,
    pub layout: Layout,
    pub hot: Option<usize>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    pub note: Option<String>,
    /// Where the last reading is kept (`sysinfo.cache`); None: it is not kept.
    cache: Option<PathBuf>,
    /// The text of the reading shown (what the script printed), and when it was read (seconds since 1970).
    raw: Option<String>,
    read_at: Option<u64>,
    reading: Option<Receiver<Option<String>>>,
}

impl SysPage {
    /// The page; the last reading is kept in `sysinfo.cache` in `data_dir`.
    pub fn new(admin: bool, data_dir: PathBuf) -> SysPage {
        SysPage { admin, cache: Some(data_dir.join("sysinfo.cache")), ..SysPage::default() }
    }

    /// The first time the page is shown: the kept reading at once; Windows is asked only
    /// when there is none, or it is older than a day (then in the background, the old one shown meanwhile).
    pub fn refresh(&mut self) {
        let kept = self.cache.as_ref().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| sysinfo::parse_cache(&t));
        match kept {
            Some((text, at)) => {
                self.info = Some(sysinfo::info_of(&text));
                self.raw = Some(text);
                self.read_at = Some(at);
                self.failed = false;
                self.fx.menus();
                self.fx.redraw();
                if sysinfo::unix_age(at) > sysinfo::MAX_AGE {
                    self.reload();
                }
            }
            None => self.reload(),
        }
    }

    /// Asks Windows again, in a thread (Refresh, F5).
    pub fn reload(&mut self) {
        if self.reading.is_some() {
            return;
        }
        let (send, recv) = channel();
        std::thread::spawn(move || {
            let _ = send.send(sysinfo::read_text());
        });
        self.reading = Some(recv);
        self.fx.menus();
        self.fx.redraw();
    }

    /// Keeps the reading shown, for the next time.
    fn keep(&self) {
        if let (Some(file), Some(text), Some(at)) = (&self.cache, &self.raw, self.read_at) {
            let _ = std::fs::write(file, sysinfo::cache_text(text, at));
        }
    }

    /// While reading: takes what arrived, and when to look again.
    pub fn animate(&mut self) -> Option<Instant> {
        let r = self.reading.as_ref()?;
        let got = match r.try_recv() {
            Ok(i) => Some(i),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
            Err(_) => None,
        };
        if let Some(text) = got {
            match text {
                Some(text) => {
                    self.info = Some(sysinfo::info_of(&text));
                    self.failed = false;
                    self.raw = Some(text);
                    self.read_at = Some(sysinfo::unix_now());
                    self.keep();
                }
                // Asked and not answered: a reading kept stays; without one, it failed.
                None => self.failed = self.info.is_none(),
            }
            self.reading = None;
            self.fx.menus();
            self.fx.redraw();
        }
        self.reading.as_ref().map(|_| Instant::now() + Duration::from_millis(150))
    }

    pub fn enabled(&self, a: Action) -> bool {
        match a {
            Action::Refresh => self.reading.is_none(),
            Action::Copy => self.info.is_some(),
            Action::FastStartup => self.admin && self.info.as_ref().is_some_and(|i| i.fast_startup.is_some()),
            Action::DefaultApps | Action::Drivers => cfg!(windows),
        }
    }

    pub fn checked(&self, a: Action) -> bool {
        a == Action::FastStartup && self.info.as_ref().is_some_and(|i| i.fast_startup == Some(true))
    }

    /// Does what a button or its menu entry asks.
    pub fn act(&mut self, a: Action) {
        if !self.enabled(a) {
            if a == Action::FastStartup && !self.admin {
                self.fx.info(
                    &tr("System Information"),
                    &tr("Changing Fast Startup needs administrator rights: restart rcare as administrator (File menu)."),
                );
            }
            return;
        }
        self.note = None;
        match a {
            Action::Refresh => self.reload(),
            Action::Copy => {
                if let Some(i) = &self.info {
                    let text = i.text(tr);
                    #[cfg(windows)]
                    let ok = rt_sys::win::clipboard::set_text(&text);
                    #[cfg(not(windows))]
                    let ok = {
                        let _ = &text;
                        false
                    };
                    self.note = Some(if ok { tr("Copied to the clipboard.") } else { tr("The clipboard could not be used.") });
                }
            }
            Action::FastStartup => {
                let on = !self.checked(Action::FastStartup);
                match sysinfo::set_fast_startup(on) {
                    Ok(()) => {
                        if let Some(i) = &mut self.info {
                            i.fast_startup = Some(on);
                        }
                        // The kept reading follows what was changed.
                        if let Some(t) = &self.raw {
                            self.raw = Some(sysinfo::with_fast_startup(t, on));
                            self.keep();
                        }
                        self.note = Some(if on {
                            tr("Fast Startup is on.")
                        } else {
                            tr("Fast Startup is off: Windows starts from scratch each time.")
                        });
                    }
                    Err(e) => self.fx.info(&tr("System Information"), &tf("Could not change it: {}", &[&e])),
                }
            }
            Action::DefaultApps => open("ms-settings:defaultapps"),
            Action::Drivers => open("ms-settings:windowsupdate-optionalupdates"),
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
            Action::Refresh | Action::Copy => 100.0,
            Action::FastStartup => 170.0,
            _ => 150.0,
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
        let h = (area.bottom() - status_h - (16.0 * s).round() - y).max(0.0);
        let cg = (12.0 * s).round();
        let cw = ((inner.w - 2.0 * cg) / 3.0).max(0.0).floor();
        let ch = ((h - cg) / 2.0).max(0.0).floor();
        let cards = [0, 1, 2, 3, 4, 5].map(|i| Rect::new(inner.x + (i % 3) as f32 * (cw + cg), y + (i / 3) as f32 * (ch + cg), cw, ch));
        Layout { area, actions, cards, status: Rect::new(area.x, area.bottom() - status_h, area.w, status_h) }
    }

    /// The cards' contents: (title, lines of (label, value, good?)).
    fn cards(&self) -> Vec<Card> {
        let Some(i) = &self.info else { return Vec::new() };
        let w = &i.windows;
        let c = &i.computer;
        let mut win = vec![
            (tr("Edition"), w.edition.clone(), None),
            (tr("Version"), format!("{} ({})", w.release, w.build), None),
            (tr("Installed"), w.installed.clone(), None),
        ];
        if let Some(h) = w.uptime_hours {
            let up = if h < 48 { tf("{} hours", &[&h.to_string()]) } else { tf("{} days", &[&(h / 24).to_string()]) };
            win.push((tr("Running for"), up, None));
        }
        if let Some(f) = i.fast_startup {
            win.push((tr("Fast Startup"), if f { tr("On") } else { tr("Off") }, None));
        }
        let mut pc = vec![(tr("Name"), c.name.clone(), None), (tr("Model"), i.model(), None)];
        for p in &c.cpus {
            pc.push((tr("Processor"), format!("{} · {}/{}", p.name, p.cores, p.threads), None));
        }
        pc.push((tr("Memory"), rsize::format::size(c.memory), None));
        pc.push(("BIOS".into(), c.bios.clone(), None));
        let gpus = i.gpus.iter().map(|g| (g.name.clone(), tf("Driver {}", &[&g.driver]), None)).collect();
        let disks = i
            .disks
            .iter()
            .map(|d| {
                let health = if d.healthy() { tr("Healthy") } else { d.health.clone() };
                (format!("{} · {} {} · {}", d.name, d.media, d.bus, rsize::format::size(d.size)), health, Some(d.healthy()))
            })
            .collect();
        let devices = if i.devices.is_empty() {
            vec![(tr("Every device works."), String::new(), Some(true))]
        } else {
            i.devices.iter().map(|d| (format!("{} ({})", d.name, d.class), tr(sysinfo::problem_text(&d.problem)), Some(false))).collect()
        };
        let battery = if i.batteries.is_empty() {
            vec![(tr("No battery: this computer runs on mains power."), String::new(), None)]
        } else {
            i.batteries.iter().map(|b| (b.name.clone(), b.charge.map_or_else(String::new, |c| format!("{c} %")), None)).collect()
        };
        vec![
            ("Windows".into(), win),
            (tr("Computer"), pc),
            (tr("Graphics"), gpus),
            (tr("Drives"), disks),
            (tr("Devices with a problem"), devices),
            (tr("Battery"), battery),
        ]
    }

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        use rt_core::i18n::strip_mnemonic;
        use rt_gfx::dialog::ellipsize;
        let l = Self::layout_in(area, s);
        g.fill_rect(area, th.bg);
        let pad = (24.0 * s).round();
        let (x, w) = (area.x + pad, (area.w - 2.0 * pad).max(0.0));
        g.text_in(&tr("System Information"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(g, &tr("What this computer is and how its parts are, as Windows reports it."), w, 12.5 * s, FontId::Regular);
        g.text_in(&about, Rect::new(x, area.y + pad + 32.0 * s, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
        for (k, a) in ACTIONS.iter().enumerate() {
            let r = l.actions[k];
            let on = self.enabled(*a) || (*a == Action::FastStartup && self.info.is_some());
            g.fill_rrect(r, 5.0 * s, th.bg);
            if on && self.hot == Some(k) {
                g.fill_rrect(r, 5.0 * s, th.hover);
            }
            g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
            let mut label = strip_mnemonic(&tr(a.label_key()));
            if *a == Action::FastStartup && self.info.as_ref().is_some_and(|i| i.fast_startup.is_some()) {
                label = format!("{label}: {}", if self.checked(*a) { tr("On") } else { tr("Off") });
            }
            let label = ellipsize(g, &label, r.w - 10.0 * s, 12.5 * s, FontId::Regular);
            g.text_in(&label, r, 12.5 * s, FontId::Regular, if self.enabled(*a) { th.text } else { th.dim }, Align::Center);
        }
        let cards = self.cards();
        for (k, r) in l.cards.iter().enumerate() {
            g.fill_rrect(*r, 8.0 * s, th.panel);
            g.stroke_rrect(*r, 8.0 * s, 1.0, th.border);
            let ip = 14.0 * s;
            let Some((title, lines)) = cards.get(k) else {
                if k == 1 {
                    let m = if self.failed { tr("Windows could not be asked (PowerShell did not run).") } else { tr("Asking Windows…") };
                    g.text_in(&m, *r, 12.5 * s, FontId::Regular, th.dim, Align::Center);
                }
                continue;
            };
            let tr_ = Rect::new(r.x + ip, r.y + ip, r.w - 2.0 * ip, 22.0 * s);
            let t = ellipsize(g, title, tr_.w, 14.5 * s, FontId::Semibold);
            g.text_in(&t, tr_, 14.5 * s, FontId::Semibold, th.text, Align::Left);
            g.set_clip(Some(*r));
            let mut y = r.y + ip + 30.0 * s;
            // Not room for two rows a line: one row each, the label left and the value right.
            let lh = 19.0 * s;
            let two_rows: f32 = lines.iter().map(|(_, v, _)| if v.is_empty() { lh + 5.0 * s } else { 2.0 * lh + 5.0 * s }).sum();
            if two_rows > r.bottom() - y - 6.0 * s {
                for (label, value, ok) in lines {
                    let row = Rect::new(r.x + ip, y, (r.w - 2.0 * ip).max(0.0), lh);
                    let color = match ok {
                        Some(true) => th.good,
                        Some(false) => th.warn,
                        None => th.text,
                    };
                    if value.is_empty() {
                        let lt = ellipsize(g, label, row.w, 11.5 * s, FontId::Regular);
                        g.text_in(&lt, row, 11.5 * s, FontId::Regular, th.text, Align::Left);
                    } else {
                        let lw = (row.w * 0.42).floor();
                        let lt = ellipsize(g, label, lw - 6.0 * s, 11.5 * s, FontId::Regular);
                        g.text_in(&lt, Rect::new(row.x, y, lw, lh), 11.5 * s, FontId::Regular, th.dim, Align::Left);
                        let vt = ellipsize(g, value, row.w - lw, 11.5 * s, FontId::Regular);
                        g.text_in(&vt, Rect::new(row.x + lw, y, row.w - lw, lh), 11.5 * s, FontId::Regular, color, Align::Right);
                    }
                    y += lh;
                }
                g.set_clip(None);
                continue;
            }
            for (label, value, ok) in lines {
                let lr = Rect::new(r.x + ip, y, (r.w - 2.0 * ip).max(0.0), lh);
                let lt = ellipsize(g, label, lr.w, 11.5 * s, FontId::Regular);
                g.text_in(&lt, lr, 11.5 * s, FontId::Regular, if value.is_empty() { th.text } else { th.dim }, Align::Left);
                if !value.is_empty() {
                    let vr = Rect::new(r.x + ip, y + lh, (r.w - 2.0 * ip).max(0.0), lh);
                    let color = match ok {
                        Some(true) => th.good,
                        Some(false) => th.warn,
                        None => th.text,
                    };
                    let vt = ellipsize(g, value, vr.w, 12.5 * s, FontId::Regular);
                    g.text_in(&vt, vr, 12.5 * s, FontId::Regular, color, Align::Left);
                    y += lh;
                }
                y += lh + 5.0 * s;
            }
            g.set_clip(None);
        }
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let age = self.read_at.map_or(0, sysinfo::unix_age);
        let left = self.note.clone().unwrap_or_else(|| match &self.info {
            Some(_) if self.reading.is_some() => tr("Asking Windows…"),
            // A reading kept from earlier: it says how old it is.
            Some(_) if age >= 3600 => tf("Read {} ago. Refresh asks Windows again.", &[&age_text(age)]),
            Some(i) if !i.devices.is_empty() => tf("Devices with a problem: {}.", &[&i.devices.len().to_string()]),
            Some(_) => tr("Every device works."),
            None if self.reading.is_some() => tr("Asking Windows…"),
            None => String::new(),
        });
        let sr = Rect::new(l.status.x + 10.0 * s, l.status.y, (l.status.w - 20.0 * s).max(0.0), l.status.h);
        let left = ellipsize(g, &left, sr.w, 12.0 * s, FontId::Regular);
        g.text_in(&left, sr, 12.0 * s, FontId::Regular, th.dim, Align::Left);
        self.layout = l;
    }

    // ------------------------------------------------------------ input

    /// What the pointer is on; `blocked` while a message is open.
    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        let hot = if blocked { None } else { self.layout.actions.iter().position(|r| r.contains(x, y)) };
        if hot != self.hot {
            self.hot = hot;
            self.fx.redraw();
        }
        self.cursor_icon = match hot {
            Some(i) if self.enabled(ACTIONS[i]) => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        };
    }

    pub fn pointer_left(&mut self) {
        if self.hot.take().is_some() {
            self.fx.redraw();
        }
    }

    pub fn release(&mut self) {}

    /// A click on a button does what it says (without rights, Fast Startup says why).
    pub fn press(&mut self) {
        if let Some(i) = self.hot {
            self.act(ACTIONS[i]);
        }
    }

    /// F5 reads again.
    pub fn key(&mut self, key: &winit::keyboard::Key) {
        use winit::keyboard::{Key, NamedKey};
        if matches!(key, Key::Named(NamedKey::F5)) {
            self.reload();
        }
    }

    /// The page's nodes for the screen readers: one text per card.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role};
        for (k, (title, lines)) in self.cards().iter().enumerate() {
            let mut n = Node::new(Role::Label);
            let body: Vec<String> = lines.iter().map(|(l, v, _)| if v.is_empty() { l.clone() } else { format!("{l}: {v}") }).collect();
            n.set_label(format!("{title}. {}", body.join(". ")));
            let id = NodeId(100 + k as u64);
            nodes.push((id, n));
            children.push(id);
        }
        None
    }

    pub fn accessibility_click(&mut self, _i: usize) {}
}

/// How old a reading is, as people say it: "5 hours", "3 days".
fn age_text(secs: u64) -> String {
    let h = secs / 3600;
    if h < 48 {
        tf("{} hours", &[&h.max(1).to_string()])
    } else {
        tf("{} days", &[&(h / 24).to_string()])
    }
}

/// Opens one of Windows' Settings pages.
fn open(uri: &str) {
    #[cfg(windows)]
    {
        rt_sys::win::shell_open(uri);
    }
    #[cfg(not(windows))]
    let _ = uri;
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERE: &str = "os\tMicrosoft Windows 10 Pro N\t22H2\t19045.6466\t64-bit\t2026-10-14\t50\n\
pc\tMCDESK1\tSystem manufacturer\tSystem Product Name\tASUSTeK MAXIMUS VIII HERO\t1402\t34267713536\n\
cpu\tIntel i7-6700K\t4\t8\t4001\ndevice\tCard Reader\tUSB\tCM_PROB_FAILED_START\nfast\t1\n";

    #[test]
    fn cards_and_options() {
        let mut p = SysPage::new(false, std::env::temp_dir().join("rcare-sys-cards"));
        assert!(p.cards().is_empty() && !p.enabled(Action::Copy));
        p.info = Some(sysinfo::parse(HERE));
        let cards = p.cards();
        assert_eq!(cards.len(), 6);
        assert!(cards[0].1.iter().any(|(l, v, _)| l == "Running for" && v == "2 days"));
        assert_eq!(cards[4].1[0].2, Some(false), "a device with a problem");
        assert!(p.checked(Action::FastStartup) && !p.enabled(Action::FastStartup));
        p.act(Action::FastStartup);
        assert!(p.fx.take().info.is_some(), "without rights it says why");
        p.admin = true;
        assert!(p.enabled(Action::FastStartup));
    }

    #[test]
    fn a_kept_reading_is_shown_at_once() {
        let dir = std::env::temp_dir().join(format!("rcare-sys-kept-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Read an hour ago: shown without asking Windows, and says how old it is.
        let hour_ago = sysinfo::unix_now() - 2 * 3600;
        std::fs::write(dir.join("sysinfo.cache"), sysinfo::cache_text(HERE, hour_ago)).unwrap();
        let mut p = SysPage::new(true, dir.clone());
        p.refresh();
        assert!(p.reading.is_none(), "nothing asked: the reading is from today");
        assert_eq!(p.info.as_ref().map(|i| i.computer.name.as_str()), Some("MCDESK1"));
        assert_eq!(p.read_at, Some(hour_ago));
        // The uptime is the clock's.
        if let Some(h) = sysinfo::uptime_hours() {
            assert_eq!(p.info.as_ref().unwrap().windows.uptime_hours, Some(h));
        }
        // Fast Startup, changed, is kept: another start shows it.
        p.raw = Some(sysinfo::with_fast_startup(HERE, false));
        p.keep();
        let mut q = SysPage::new(true, dir.clone());
        q.refresh();
        assert_eq!(q.info.as_ref().unwrap().fast_startup, Some(false));
        assert_eq!(q.read_at, Some(hour_ago), "keeping it does not make it newer");
        // A reading older than a day is shown and asked again meanwhile.
        std::fs::write(dir.join("sysinfo.cache"), sysinfo::cache_text(HERE, sysinfo::unix_now() - 2 * 86_400)).unwrap();
        let mut old = SysPage::new(true, dir.clone());
        old.refresh();
        assert!(old.info.is_some() && old.reading.is_some());
        // Nothing kept, or a file that is not ours: Windows is asked.
        std::fs::write(dir.join("sysinfo.cache"), "garbage").unwrap();
        let mut none = SysPage::new(true, dir.clone());
        none.refresh();
        assert!(none.info.is_none() && none.reading.is_some());
        assert_eq!(age_text(5 * 3600), "5 hours");
        assert_eq!(age_text(3 * 86_400), "3 days");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn layout() {
        let l = SysPage::layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        assert!(l.cards[2].right() <= 224.0 + 956.0 && l.cards[3].y > l.cards[0].bottom());
        assert!(l.actions[4].right() <= 224.0 + 956.0);
    }
}
