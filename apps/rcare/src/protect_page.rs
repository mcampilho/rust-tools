//! The Protect System page: three cards — the antivirus, the firewall and
//! Microsoft Defender — as Windows reports them (`protect`), and Windows'
//! own actions: a quick scan and new definitions (when Defender is the
//! antivirus), turning on what is off (administrator), and Windows
//! Security's own pages. Reading and the actions run in a thread.

use crate::i18n::{tf, tr};
use crate::protect::{self, Folders, Job, Mode, Status};
use rt_gfx::gfx::{Align, FontId, Gfx, Rect};
use rt_ui::page::{Dialogs, Effects};
use rt_ui::theme::Theme;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use winit::window::CursorIcon;

/// What the page does (its buttons and menu entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Refresh,
    QuickScan,
    UpdateDefinitions,
    TurnOn,
    Security,
    Folders,
}

pub const ACTIONS: [Action; 6] =
    [Action::Refresh, Action::QuickScan, Action::UpdateDefinitions, Action::TurnOn, Action::Security, Action::Folders];

impl Action {
    /// The menu label (a translation key with its access key).
    pub fn label_key(self) -> &'static str {
        match self {
            Action::Refresh => "&Refresh",
            Action::QuickScan => "&Quick Scan",
            Action::UpdateDefinitions => "&Update Definitions",
            Action::TurnOn => "&Turn On What Is Off...",
            Action::Security => "Open Windows &Security",
            Action::Folders => "Controlled &Folder Access",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub actions: [Rect; 6],
    pub cards: [Rect; 3],
    pub status: Rect,
}

/// What a thread is doing.
enum Work {
    Reading(Receiver<Option<Status>>),
    Job(Job, Receiver<Result<(), String>>),
}

#[derive(Default)]
pub struct ProtectPage {
    pub status: Option<Status>,
    /// Windows could not be asked.
    pub failed: bool,
    pub admin: bool,
    pub layout: Layout,
    pub hot: Option<usize>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    pub note: Option<String>,
    work: Option<Work>,
}

impl ProtectPage {
    pub fn new(admin: bool) -> ProtectPage {
        ProtectPage { admin, ..ProtectPage::default() }
    }

    pub fn busy(&self) -> bool {
        self.work.is_some()
    }

    /// The job under way, if any.
    pub fn job(&self) -> Option<&Job> {
        match &self.work {
            Some(Work::Job(j, _)) => Some(j),
            _ => None,
        }
    }

    /// Asks Windows again, in a thread.
    pub fn refresh(&mut self) {
        if self.busy() {
            return;
        }
        let (send, recv) = channel();
        std::thread::spawn(move || {
            let _ = send.send(protect::read());
        });
        self.work = Some(Work::Reading(recv));
        self.fx.menus();
        self.fx.redraw();
    }

    fn start(&mut self, job: Job) {
        let (send, recv) = channel();
        let j = job.clone();
        std::thread::spawn(move || {
            let _ = send.send(protect::run(&j));
        });
        self.note = Some(match job {
            Job::QuickScan => tr("Quick scan under way: it takes a few minutes."),
            Job::UpdateDefinitions => tr("Updating the definitions…"),
            Job::TurnOn(_) => tr("Turning on…"),
        });
        self.work = Some(Work::Job(job, recv));
        self.fx.menus();
        self.fx.redraw();
    }

    /// While a thread works: takes what it says, and when to look again.
    pub fn animate(&mut self) -> Option<Instant> {
        let work = self.work.as_ref()?;
        match work {
            Work::Reading(r) => match r.try_recv() {
                Ok(s) => {
                    self.failed = s.is_none();
                    self.status = s;
                    self.work = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.failed = true;
                    self.work = None;
                }
                Err(_) => {}
            },
            Work::Job(job, r) => {
                let job = job.clone();
                let got = match r.try_recv() {
                    Ok(res) => Some(res),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err(String::new())),
                    Err(_) => None,
                };
                if let Some(res) = got {
                    self.work = None;
                    let what = tr(match job {
                        Job::QuickScan => "Quick Scan",
                        Job::UpdateDefinitions => "Update Definitions",
                        Job::TurnOn(_) => "Turn On What Is Off",
                    });
                    match res {
                        Ok(()) => self.note = Some(tf("{}: done.", &[&what])),
                        Err(why) => {
                            self.note = None;
                            self.fx.info(&tr("Protect System"), &tf("{} did not work: {}", &[&what, &why]));
                        }
                    }
                    self.refresh();
                }
            }
        }
        if self.work.is_none() {
            self.fx.menus();
            self.fx.redraw();
        }
        self.work.as_ref().map(|_| Instant::now() + Duration::from_millis(150))
    }

    pub fn enabled(&self, a: Action) -> bool {
        let s = self.status.as_ref();
        match a {
            Action::Refresh => !self.busy(),
            Action::QuickScan | Action::UpdateDefinitions => !self.busy() && s.is_some_and(Status::defender_active),
            Action::TurnOn => !self.busy() && self.admin && s.is_some_and(|s| !s.off().is_empty()),
            Action::Security | Action::Folders => cfg!(windows),
        }
    }

    /// Does what a button or its menu entry asks.
    pub fn act(&mut self, a: Action, dialogs: &mut dyn Dialogs) {
        if !self.enabled(a) {
            if a == Action::TurnOn && !self.admin && self.status.as_ref().is_some_and(|s| !s.off().is_empty()) {
                self.fx.info(
                    &tr("Protect System"),
                    &tr("Turning these on needs administrator rights: restart rcare as administrator (File menu)."),
                );
            }
            return;
        }
        match a {
            Action::Refresh => self.refresh(),
            Action::QuickScan => self.start(Job::QuickScan),
            Action::UpdateDefinitions => self.start(Job::UpdateDefinitions),
            Action::TurnOn => {
                let fixes = self.status.as_ref().map(Status::off).unwrap_or_default();
                let list: Vec<String> = fixes
                    .iter()
                    .map(|f| match f {
                        protect::Fix::Profile(p) => format!("  {}", tf("Windows Firewall, {} profile", &[&profile_name(p)])),
                        protect::Fix::Realtime => format!("  {}", tr("Microsoft Defender, real-time protection")),
                    })
                    .collect();
                let text = format!("{}\n\n{}", tr("Turn these on?"), list.join("\n"));
                if dialogs.confirm(&tr("Protect System"), &text) {
                    self.start(Job::TurnOn(fixes));
                }
            }
            Action::Security => open("windowsdefender:"),
            Action::Folders => open("windowsdefender://ransomwareprotection"),
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
            Action::Refresh => 100.0,
            Action::QuickScan => 120.0,
            Action::UpdateDefinitions => 160.0,
            Action::TurnOn => 170.0,
            Action::Security => 190.0,
            Action::Folders => 190.0,
        });
        let gap = 8.0 * s;
        let fit = ((inner.w - 5.0 * gap) / (wants.iter().sum::<f32>() * s)).min(1.0);
        let mut actions = [Rect::default(); 6];
        // On a narrow page: two rows of three, each row filling the width.
        let rows = if fit < 0.8 { 2 } else { 1 };
        let per_row = ACTIONS.len() / rows;
        for (row, chunk) in wants.chunks(per_row).enumerate() {
            let row_fit = if rows == 1 { fit } else { (inner.w - (per_row - 1) as f32 * gap) / (chunk.iter().sum::<f32>() * s) };
            let mut x = inner.x;
            for (k, want) in chunk.iter().enumerate() {
                let w = (want * s * row_fit).floor();
                actions[row * per_row + k] = Rect::new(x, top + row as f32 * (bh + gap), w, bh);
                x += w + gap;
            }
        }
        let status_h = (24.0 * s).round();
        let y = top + rows as f32 * (bh + gap) - gap + (16.0 * s).round();
        let h = (area.bottom() - status_h - (16.0 * s).round() - y).max(0.0);
        let cg = (12.0 * s).round();
        let cw = ((inner.w - 2.0 * cg) / 3.0).max(0.0).floor();
        let cards = [0, 1, 2].map(|i| Rect::new(inner.x + i as f32 * (cw + cg), y, cw, h));
        Layout { area, actions, cards, status: Rect::new(area.x, area.bottom() - status_h, area.w, status_h) }
    }

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        use rt_core::i18n::strip_mnemonic;
        use rt_gfx::dialog::ellipsize;
        let l = Self::layout_in(area, s);
        g.fill_rect(area, th.bg);
        let pad = (24.0 * s).round();
        let (x, w) = (area.x + pad, (area.w - 2.0 * pad).max(0.0));
        g.text_in(&tr("Protect System"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(
            g,
            &tr("What protects this computer, as Windows reports it. The actions are Windows' own: rcare has no antivirus of its own."),
            w,
            12.5 * s,
            FontId::Regular,
        );
        g.text_in(&about, Rect::new(x, area.y + pad + 32.0 * s, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
        for (i, a) in ACTIONS.iter().enumerate() {
            let r = l.actions[i];
            let on = self.enabled(*a);
            g.fill_rrect(r, 5.0 * s, th.bg);
            if on && self.hot == Some(i) {
                g.fill_rrect(r, 5.0 * s, th.hover);
            }
            g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
            let label = strip_mnemonic(&tr(a.label_key()));
            let label = ellipsize(g, label.trim_end_matches("..."), r.w - 10.0 * s, 12.5 * s, FontId::Regular);
            g.text_in(&label, r, 12.5 * s, FontId::Regular, if on { th.text } else { th.dim }, Align::Center);
        }
        let reading = matches!(self.work, Some(Work::Reading(_)));
        let st = self.status.clone();
        let on_off = |b: bool| if b { tr("On") } else { tr("Off") };
        // (title, whether all is well (None: unknown), lines of (label, value, good?))
        type Lines = Vec<(String, String, Option<bool>)>;
        let mut cards: [(String, Option<bool>, Lines); 3] =
            [(tr("Antivirus"), None, Vec::new()), (tr("Firewall"), None, Vec::new()), ("Microsoft Defender".to_string(), None, Vec::new())];
        if let Some(st) = &st {
            cards[0].1 = Some(st.antivirus_on());
            for p in &st.antivirus {
                let v = if !p.on {
                    tr("Off")
                } else if p.up_to_date {
                    tr("On, up to date")
                } else {
                    tr("On, out of date")
                };
                cards[0].2.push((p.name.clone(), v, Some(p.on && p.up_to_date)));
            }
            if st.defender_active() {
                let realtime = st.defender.as_ref().is_some_and(|d| d.realtime);
                cards[0].2.push(("Microsoft Defender".into(), on_off(realtime), Some(realtime)));
            }
            cards[1].1 = Some(st.firewall_on());
            for (name, on) in &st.profiles {
                cards[1].2.push((tf("Windows Firewall, {} profile", &[&profile_name(name)]), on_off(*on), Some(*on)));
            }
            for p in &st.firewalls {
                cards[1].2.push((p.name.clone(), on_off(p.on), Some(p.on)));
            }
            if let Some(d) = &st.defender {
                let (mode, good) = match d.mode {
                    Mode::Active => (tr("The antivirus"), true),
                    Mode::Passive => (tr("Passive: another antivirus protects"), st.antivirus_on()),
                    Mode::Off => (if st.antivirus_on() { tr("Off: another antivirus protects") } else { tr("Off") }, st.antivirus_on()),
                };
                cards[2].1 = Some(good);
                cards[2].2.push((tr("State"), mode, Some(good)));
                if d.mode != Mode::Off {
                    cards[2].2.push((tr("Real-time protection"), on_off(d.realtime), Some(d.realtime || d.mode != Mode::Active)));
                    let age = |a: Option<u32>| match a {
                        None => tr("never"),
                        Some(0) => tr("today"),
                        Some(1) => tr("1 day ago"),
                        Some(d) => tf("{} days ago", &[&d.to_string()]),
                    };
                    cards[2].2.push((tr("Definitions updated"), age(d.signature_age), Some(d.signature_age.is_some_and(|a| a <= 7))));
                    cards[2].2.push((tr("Last quick scan"), age(d.quick_scan_age), None));
                    cards[2].2.push((tr("Tamper protection"), on_off(d.tamper), None));
                }
            }
            let folders = match st.folders {
                Some(Folders::On) => tr("On"),
                Some(Folders::Off) => tr("Off"),
                Some(Folders::Audit) => tr("Audit only"),
                None => tr("Unknown"),
            };
            cards[2].2.push((tr("Controlled folder access"), folders, None));
        }
        for (r, (title, good, lines)) in l.cards.iter().zip(cards) {
            g.fill_rrect(*r, 8.0 * s, th.panel);
            g.stroke_rrect(*r, 8.0 * s, 1.0, th.border);
            let ip = 14.0 * s;
            let dot = good.map(|ok| if ok { th.good } else { th.warn }).unwrap_or(th.border);
            g.fill_circle(r.x + ip + 5.0 * s, r.y + ip + 11.0 * s, 5.0 * s, dot);
            let tr_ = Rect::new(r.x + ip + 16.0 * s, r.y + ip, (r.w - 2.0 * ip - 16.0 * s).max(0.0), 22.0 * s);
            let t = ellipsize(g, &title, tr_.w, 14.5 * s, FontId::Semibold);
            g.text_in(&t, tr_, 14.5 * s, FontId::Semibold, th.text, Align::Left);
            let state = match good {
                Some(true) => tr("Protected"),
                Some(false) => tr("Needs attention"),
                None if reading => tr("Asking Windows…"),
                None => String::new(),
            };
            let sr = Rect::new(r.x + ip, r.y + ip + 24.0 * s, (r.w - 2.0 * ip).max(0.0), 20.0 * s);
            let st_t = ellipsize(g, &state, sr.w, 12.0 * s, FontId::Regular);
            g.text_in(&st_t, sr, 12.0 * s, FontId::Regular, dot.mix(th.text, 0.25), Align::Left);
            let mut y = r.y + ip + 54.0 * s;
            g.set_clip(Some(*r));
            for (label, value, ok) in lines {
                let lh = 20.0 * s;
                let lr = Rect::new(r.x + ip, y, (r.w - 2.0 * ip).max(0.0), lh);
                let lt = ellipsize(g, &label, lr.w, 12.0 * s, FontId::Regular);
                g.text_in(&lt, lr, 12.0 * s, FontId::Regular, th.dim, Align::Left);
                let vr = Rect::new(r.x + ip, y + lh, (r.w - 2.0 * ip).max(0.0), lh);
                let color = match ok {
                    Some(true) => th.good,
                    Some(false) => th.warn,
                    None => th.text,
                };
                let vt = ellipsize(g, &value, vr.w, 12.5 * s, FontId::Regular);
                g.text_in(&vt, vr, 12.5 * s, FontId::Regular, color, Align::Left);
                y += lh * 2.0 + 6.0 * s;
            }
            g.set_clip(None);
        }
        if self.failed && st.is_none() {
            g.text_in(
                &tr("Windows could not be asked (PowerShell did not run)."),
                l.cards[1],
                12.5 * s,
                FontId::Regular,
                th.warn,
                Align::Center,
            );
        }
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let left = self.note.clone().unwrap_or_else(|| match &st {
            Some(st) => {
                let a = if st.antivirus_on() { tr("on") } else { tr("off") };
                let f = if st.firewall_on() { tr("on") } else { tr("off") };
                tf("Antivirus: {}. Firewall: {}.", &[&a, &f])
            }
            None if reading => tr("Asking Windows…"),
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

    /// A click on a button gives its action for the window to do.
    pub fn press(&mut self) -> Option<Action> {
        self.hot.map(|i| ACTIONS[i])
    }

    /// F5 reads again.
    pub fn key(&mut self, key: &winit::keyboard::Key) -> Option<Action> {
        use winit::keyboard::{Key, NamedKey};
        matches!(key, Key::Named(NamedKey::F5)).then_some(Action::Refresh)
    }

    /// The page's nodes for the screen readers: one text per card.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role};
        let mut n = Node::new(Role::Label);
        let text = match &self.status {
            Some(st) => {
                let a = if st.antivirus_on() { tr("on") } else { tr("off") };
                let f = if st.firewall_on() { tr("on") } else { tr("off") };
                tf("Antivirus: {}. Firewall: {}.", &[&a, &f])
            }
            None => tr("Asking Windows…"),
        };
        n.set_label(text);
        nodes.push((NodeId(100), n));
        children.push(NodeId(100));
        None
    }

    pub fn accessibility_click(&mut self, _i: usize) {}
}

/// A Windows Firewall profile's name, as Windows Security says it.
fn profile_name(p: &str) -> String {
    match p.to_lowercase().as_str() {
        "domain" => tr("domain"),
        "private" => tr("private"),
        "public" => tr("public"),
        _ => p.to_string(),
    }
}

/// Opens one of Windows Security's pages.
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

    const HERE: &str = "av\tBitdefender Antivirus\t266240\nmp\tNot running\tFalse\t65535\t4294967295\tFalse\nprofile\tDomain\tTrue\nprofile\tPublic\tFalse\n";

    #[test]
    fn what_can_be_done() {
        let mut p = ProtectPage::new(false);
        assert!(!p.enabled(Action::QuickScan) && p.enabled(Action::Refresh));
        p.status = Some(protect::parse(HERE));
        // Another antivirus: Defender's actions are not offered; turning on needs administrator rights.
        assert!(!p.enabled(Action::QuickScan) && !p.enabled(Action::TurnOn));
        struct Yes;
        impl Dialogs for Yes {
            fn confirm(&mut self, _: &str, _: &str) -> bool {
                true
            }
            fn pick_folder(&mut self, _: &str) -> Option<std::path::PathBuf> {
                None
            }
            fn save_file(&mut self, _: &str, _: (&str, &str), _: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
                None
            }
            fn recycle(&mut self, _: &std::path::Path) -> Result<(), String> {
                Ok(())
            }
        }
        p.act(Action::TurnOn, &mut Yes);
        assert!(p.fx.take().info.is_some() && !p.busy(), "says why, does nothing");
        p.admin = true;
        assert!(p.enabled(Action::TurnOn));
        p.status = Some(protect::parse("mp\tNormal\tTrue\t1\t1\tTrue\nprofile\tPublic\tTrue\n"));
        assert!(p.enabled(Action::QuickScan) && p.enabled(Action::UpdateDefinitions) && !p.enabled(Action::TurnOn));
    }

    #[test]
    fn buttons_and_cards() {
        let mut p = ProtectPage::new(true);
        p.layout = ProtectPage::layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        let l = p.layout.clone();
        assert!(l.cards[0].right() < l.cards[1].x && l.cards[2].right() <= 224.0 + 956.0);
        assert!(l.actions[5].right() <= 224.0 + 956.0);
        p.pointer(l.actions[0].x + 3.0, l.actions[0].y + 3.0, false);
        assert_eq!(p.press(), Some(Action::Refresh));
        assert_eq!(p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::F5)), Some(Action::Refresh));
        let small = ProtectPage::layout_in(Rect::new(224.0, 0.0, 536.0, 480.0), 1.0);
        assert!(small.actions[5].right() <= 224.0 + 536.0 && small.cards[0].h > 100.0);
    }
}
