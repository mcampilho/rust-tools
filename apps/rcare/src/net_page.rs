//! The Internet Booster page: Windows' TCP values (those that differ from
//! Windows' own are marked, and can be put back) and each connection's DNS
//! (set to Automatic or a known service). Every change asks first, needs
//! administrator rights and writes a `.cmd` copy that puts back what was
//! there (`backups\network <date>` beside the settings).

use crate::i18n::{tf, tr};
use crate::net::{self, Change, Network, Provider};
use rt_gfx::gfx::{Align, FontId, Gfx, Rect};
use rt_ui::page::{Dialogs, Effects};
use rt_ui::theme::Theme;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use winit::window::CursorIcon;

/// What the page does (its buttons and menu entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Refresh,
    RestoreTcp,
    Copies,
    Dns(Provider),
}

/// The buttons above the cards.
pub const TOP: [Action; 3] = [Action::Refresh, Action::RestoreTcp, Action::Copies];

impl Action {
    /// The menu label (a translation key with its access key).
    pub fn label_key(self) -> &'static str {
        match self {
            Action::Refresh => "&Refresh",
            Action::RestoreTcp => "&Restore Windows' TCP Values...",
            Action::Copies => "Open the C&opies Folder",
            Action::Dns(Provider::Automatic) => "DNS: &Automatic...",
            Action::Dns(Provider::Cloudflare) => "DNS: &Cloudflare...",
            Action::Dns(Provider::Quad9) => "DNS: &Quad9...",
            Action::Dns(Provider::Google) => "DNS: &Google...",
        }
    }
}

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Top(usize),
    Connection(usize),
    Dns(usize),
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub top: [Rect; 3],
    pub tcp: Rect,
    pub dns: Rect,
    /// The connections' rows, and the four DNS choices.
    pub rows: Vec<Rect>,
    pub chips: [Rect; 4],
    pub status: Rect,
}

enum Work {
    Reading(Receiver<Option<Network>>),
    Applying(Receiver<Result<(), String>>),
}

#[derive(Default)]
pub struct NetPage {
    pub net: Option<Network>,
    pub failed: bool,
    /// The connection selected.
    pub sel: usize,
    pub admin: bool,
    pub data_dir: PathBuf,
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    pub note: Option<String>,
    work: Option<Work>,
}

impl NetPage {
    pub fn new(admin: bool, data_dir: PathBuf) -> NetPage {
        NetPage { admin, data_dir, ..NetPage::default() }
    }

    pub fn busy(&self) -> bool {
        self.work.is_some()
    }

    pub fn copies_dir(&self) -> PathBuf {
        self.data_dir.join("backups")
    }

    /// Asks Windows again, in a thread.
    pub fn refresh(&mut self) {
        if self.busy() {
            return;
        }
        let (send, recv) = channel();
        std::thread::spawn(move || {
            let _ = send.send(net::read());
        });
        self.work = Some(Work::Reading(recv));
        self.fx.menus();
        self.fx.redraw();
    }

    /// While a thread works: takes what it says, and when to look again.
    pub fn animate(&mut self) -> Option<Instant> {
        match self.work.as_ref()? {
            Work::Reading(r) => match r.try_recv() {
                Ok(n) => {
                    self.failed = n.is_none();
                    self.net = n;
                    self.sel = self.sel.min(self.connections().len().saturating_sub(1));
                    self.work = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.failed = true;
                    self.work = None;
                }
                Err(_) => {}
            },
            Work::Applying(r) => {
                let got = match r.try_recv() {
                    Ok(res) => Some(res),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err(String::new())),
                    Err(_) => None,
                };
                if let Some(res) = got {
                    self.work = None;
                    if let Err(why) = res {
                        self.note = None;
                        self.fx.info(&tr("Internet Booster"), &tf("It did not work: {}", &[&why]));
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

    pub fn connections(&self) -> &[net::Connection] {
        self.net.as_ref().map_or(&[], |n| &n.connections)
    }

    /// Whether the action could be done now, rights apart.
    fn possible(&self, a: Action) -> bool {
        match a {
            Action::Refresh => !self.busy(),
            Action::Copies => self.copies_dir().is_dir(),
            Action::RestoreTcp => !self.busy() && self.net.as_ref().and_then(|n| n.tcp.as_ref()).is_some_and(|t| !t.changed().is_empty()),
            Action::Dns(p) => {
                !self.busy()
                    && self.connections().get(self.sel).is_some_and(|c| match p {
                        Provider::Automatic => c.manual,
                        p => c.provider() != Some(p),
                    })
            }
        }
    }

    pub fn enabled(&self, a: Action) -> bool {
        self.possible(a) && (self.admin || matches!(a, Action::Refresh | Action::Copies))
    }

    /// Does what a button or its menu entry asks.
    pub fn act(&mut self, a: Action, dialogs: &mut dyn Dialogs) {
        if !self.enabled(a) {
            if self.possible(a) && !self.admin {
                self.fx.info(
                    &tr("Internet Booster"),
                    &tr("Changing these needs administrator rights: restart rcare as administrator (File menu)."),
                );
            }
            return;
        }
        let saved = tr("What is there now is saved first to a .cmd file in the copies folder: run it as administrator to put it back.");
        let change = match a {
            Action::Refresh => return self.refresh(),
            Action::Copies => return rt_sys::reveal(&self.copies_dir()),
            Action::RestoreTcp => {
                let Some(t) = self.net.as_ref().and_then(|n| n.tcp.clone()) else { return };
                let list: Vec<String> = t.changed().iter().map(|(name, now, want)| format!("  {}: {now} → {want}", tr(name))).collect();
                let text = format!("{}\n\n{}\n\n{saved}", tr("Put Windows' own TCP values back?"), list.join("\n"));
                if !dialogs.confirm(&tr("Internet Booster"), &text) {
                    return;
                }
                Change::RestoreTcp
            }
            Action::Dns(p) => {
                let Some(c) = self.connections().get(self.sel).cloned() else { return };
                let first = if p == Provider::Automatic {
                    tf("Use the router's DNS (automatic) for “{}”?", &[&c.name])
                } else {
                    format!("{}\n\n  {}\n  {}", tf("Use {} for “{}”?", &[p.name(), &c.name]), p.v4().join(", "), p.v6().join(", "))
                };
                if !dialogs.confirm(&tr("Internet Booster"), &format!("{first}\n\n{saved}")) {
                    return;
                }
                Change::Dns { index: c.index, provider: p }
            }
        };
        let stamp = {
            let t = rt_sys::local_time();
            format!("{:04}-{:02}-{:02} {:02}{:02}{:02}", t.year, t.month, t.day, t.hour, t.minute, t.second)
        };
        let file = match &change {
            Change::RestoreTcp => "tcp.cmd".to_string(),
            Change::Dns { index, .. } => format!("dns-{index}.cmd"),
        };
        let copy = self.copies_dir().join(format!("network {stamp}")).join(file);
        let snapshot = self.net.clone().unwrap_or_default();
        let (send, recv) = channel();
        std::thread::spawn(move || {
            let _ = send.send(net::apply(&change, &snapshot, &copy));
        });
        self.note = Some(tr("Changing…"));
        self.work = Some(Work::Applying(recv));
        self.fx.menus();
        self.fx.redraw();
    }

    // ------------------------------------------------------------ drawing

    pub fn layout_in(&self, area: Rect, s: f32) -> Layout {
        let pad = (24.0 * s).round();
        let inner = Rect::new(area.x + pad, area.y + pad, (area.w - 2.0 * pad).max(0.0), (area.h - pad).max(0.0));
        let bh = (30.0 * s).round();
        let top_y = inner.y + (62.0 * s).round();
        let wants = TOP.map(|a| match a {
            Action::Refresh => 100.0,
            Action::RestoreTcp => 230.0,
            _ => 180.0,
        });
        let gap = 8.0 * s;
        let fit = ((inner.w - 2.0 * gap) / (wants.iter().sum::<f32>() * s)).min(1.0);
        let mut top = [Rect::default(); 3];
        let mut x = inner.x;
        for (i, want) in wants.iter().enumerate() {
            let w = (want * s * fit).floor();
            top[i] = Rect::new(x, top_y, w, bh);
            x += w + gap;
        }
        let status_h = (24.0 * s).round();
        let y = top_y + bh + (16.0 * s).round();
        let h = (area.bottom() - status_h - (16.0 * s).round() - y).max(0.0);
        let cg = (12.0 * s).round();
        let tcp_w = ((inner.w - cg) * 0.42).floor();
        let tcp = Rect::new(inner.x, y, tcp_w, h);
        let dns = Rect::new(tcp.right() + cg, y, (inner.right() - tcp.right() - cg).max(0.0), h);
        let ip = 14.0 * s;
        let row_h = (52.0 * s).round();
        let rows = (0..self.connections().len())
            .map(|i| Rect::new(dns.x + 1.0, dns.y + 64.0 * s + i as f32 * row_h, dns.w - 2.0, row_h))
            .collect();
        let chip_w = ((dns.w - 2.0 * ip - 3.0 * gap) / 4.0).max(0.0).floor();
        let chips = [0, 1, 2, 3].map(|i| Rect::new(dns.x + ip + i as f32 * (chip_w + gap), dns.bottom() - ip - bh, chip_w, bh));
        Layout { area, top, tcp, dns, rows, chips, status: Rect::new(area.x, area.bottom() - status_h, area.w, status_h) }
    }

    /// A button: `(on, hot)` say whether it can be used and whether the pointer is on it.
    fn button(g: &mut Gfx, r: Rect, label: &str, (on, hot): (bool, bool), th: &Theme, s: f32) {
        use rt_gfx::dialog::ellipsize;
        g.fill_rrect(r, 5.0 * s, th.bg);
        if on && hot {
            g.fill_rrect(r, 5.0 * s, th.hover);
        }
        g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
        let t = ellipsize(g, label, r.w - 10.0 * s, 12.5 * s, FontId::Regular);
        g.text_in(&t, r, 12.5 * s, FontId::Regular, if on { th.text } else { th.dim }, Align::Center);
    }

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        use rt_core::i18n::strip_mnemonic;
        use rt_gfx::dialog::{ellipsize, ellipsize_middle};
        let l = self.layout_in(area, s);
        g.fill_rect(area, th.bg);
        let pad = (24.0 * s).round();
        let (x, w) = (area.x + pad, (area.w - 2.0 * pad).max(0.0));
        g.text_in(&tr("Internet Booster"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(
            g,
            &tr("Windows tunes TCP itself: values changed by other tools can be put back. The DNS can be the router's or a known service."),
            w,
            12.5 * s,
            FontId::Regular,
        );
        g.text_in(&about, Rect::new(x, area.y + pad + 32.0 * s, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
        for (i, a) in TOP.iter().enumerate() {
            let label = strip_mnemonic(&tr(a.label_key()));
            Self::button(g, l.top[i], label.trim_end_matches("..."), (self.possible(*a), self.hot == Some(Hit::Top(i))), th, s);
        }
        let reading = matches!(self.work, Some(Work::Reading(_)));
        let ip = 14.0 * s;
        for (r, title) in [(l.tcp, "TCP"), (l.dns, "DNS")] {
            g.fill_rrect(r, 8.0 * s, th.panel);
            g.stroke_rrect(r, 8.0 * s, 1.0, th.border);
            g.text_in(title, Rect::new(r.x + ip, r.y + ip, r.w - 2.0 * ip, 22.0 * s), 14.5 * s, FontId::Semibold, th.text, Align::Left);
        }
        // TCP.
        if let Some(t) = self.net.as_ref().and_then(|n| n.tcp.clone()) {
            let changed = t.changed();
            let note = match changed.len() {
                0 => tr("Windows' own values."),
                1 => tr("1 value differs from Windows' own."),
                n => tf("{} values differ from Windows' own.", &[&n.to_string()]),
            };
            let nr = Rect::new(l.tcp.x + ip, l.tcp.y + ip + 24.0 * s, l.tcp.w - 2.0 * ip, 20.0 * s);
            let nt = ellipsize(g, &note, nr.w, 12.0 * s, FontId::Regular);
            g.text_in(&nt, nr, 12.0 * s, FontId::Regular, if changed.is_empty() { th.good } else { th.warn }, Align::Left);
            let mut y = l.tcp.y + ip + 54.0 * s;
            g.set_clip(Some(l.tcp));
            for (name, value) in [
                ("Receive window auto-tuning", &t.auto_tuning),
                ("Receive-side scaling", &t.rss),
                ("Receive segment coalescing", &t.rsc),
                ("Congestion control", &t.congestion),
                ("ECN", &t.ecn),
                ("RFC 1323 timestamps", &t.timestamps),
                ("Initial retransmission timeout (ms)", &t.initial_rto),
            ] {
                let lh = 20.0 * s;
                let lr = Rect::new(l.tcp.x + ip, y, l.tcp.w - 2.0 * ip, lh);
                let lt = ellipsize(g, &tr(name), lr.w, 12.0 * s, FontId::Regular);
                g.text_in(&lt, lr, 12.0 * s, FontId::Regular, th.dim, Align::Left);
                let off = changed.iter().find(|(n, _, _)| n == &name);
                let v = match off {
                    Some((_, _, want)) => format!("{value} ({})", tf("Windows: {}", &[want])),
                    None => value.clone(),
                };
                let vr = Rect::new(l.tcp.x + ip, y + lh, l.tcp.w - 2.0 * ip, lh);
                let vt = ellipsize(g, &v, vr.w, 12.5 * s, FontId::Regular);
                g.text_in(&vt, vr, 12.5 * s, FontId::Regular, if off.is_some() { th.warn } else { th.text }, Align::Left);
                y += lh * 2.0 + 6.0 * s;
            }
            g.set_clip(None);
        } else if reading {
            g.text_in(&tr("Asking Windows…"), l.tcp, 12.5 * s, FontId::Regular, th.dim, Align::Center);
        } else if self.failed {
            g.text_in(
                &tr("Windows could not be asked (PowerShell did not run)."),
                l.tcp,
                12.0 * s,
                FontId::Regular,
                th.warn,
                Align::Center,
            );
        }
        // DNS.
        let hint = Rect::new(l.dns.x + ip, l.dns.y + ip + 24.0 * s, l.dns.w - 2.0 * ip, 20.0 * s);
        let ht = ellipsize(g, &tr("Choose a connection, then its DNS:"), hint.w, 12.0 * s, FontId::Regular);
        g.text_in(&ht, hint, 12.0 * s, FontId::Regular, th.dim, Align::Left);
        let list_clip = Rect::new(l.dns.x, l.dns.y, l.dns.w, (l.chips[0].y - 8.0 * s - l.dns.y).max(0.0));
        g.set_clip(Some(list_clip));
        let conns = self.connections().to_vec();
        for (i, (r, c)) in l.rows.iter().zip(&conns).enumerate() {
            if self.sel == i {
                g.fill_rect(*r, th.selection);
            } else if self.hot == Some(Hit::Connection(i)) {
                g.fill_rect(*r, th.hover);
            }
            let inner = Rect::new(r.x + ip, r.y + 4.0 * s, r.w - 2.0 * ip, 22.0 * s);
            let kind = match (c.manual, c.provider()) {
                (_, Some(p)) => p.name().to_string(),
                (true, None) => tr("Set by hand"),
                (false, None) => tr("Automatic (the router's)"),
            };
            let kind_w = (170.0 * s).min(inner.w * 0.4);
            let name = ellipsize(g, &format!("{} — {}", c.name, c.device), (inner.w - kind_w).max(0.0), 12.5 * s, FontId::Semibold);
            g.text_in(&name, inner, 12.5 * s, FontId::Semibold, th.text, Align::Left);
            let kt = ellipsize(g, &kind, kind_w, 12.0 * s, FontId::Regular);
            g.text_in(&kt, Rect::new(inner.right() - kind_w, inner.y, kind_w, inner.h), 12.0 * s, FontId::Regular, th.accent, Align::Right);
            let servers: Vec<&str> = c.v4.iter().chain(&c.v6).map(String::as_str).collect();
            let sr = Rect::new(inner.x, inner.bottom(), inner.w, 20.0 * s);
            let st = ellipsize_middle(g, &servers.join("  ·  "), sr.w, 11.5 * s, FontId::Regular);
            g.text_in(&st, sr, 11.5 * s, FontId::Regular, th.dim, Align::Left);
        }
        g.set_clip(None);
        if !reading && self.net.is_some() && conns.is_empty() {
            g.text_in(&tr("No connection is up."), list_clip, 12.5 * s, FontId::Regular, th.dim, Align::Center);
        }
        for (i, p) in Provider::ALL.iter().enumerate() {
            let label = if *p == Provider::Automatic { tr("Automatic") } else { p.name().to_string() };
            Self::button(g, l.chips[i], &label, (self.possible(Action::Dns(*p)), self.hot == Some(Hit::Dns(i))), th, s);
        }
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let left = self.note.clone().unwrap_or_else(|| {
            if reading {
                tr("Asking Windows…")
            } else if self.admin {
                String::new()
            } else {
                tr("Changing these needs administrator rights.")
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
        if let Some(i) = l.top.iter().position(|r| r.contains(x, y)) {
            return Some(Hit::Top(i));
        }
        if let Some(i) = l.chips.iter().position(|r| r.contains(x, y)) {
            return Some(Hit::Dns(i));
        }
        let limit = l.chips[0].y - 8.0;
        l.rows.iter().position(|r| r.contains(x, y) && y < limit).map(Hit::Connection)
    }

    /// What the pointer is on; `blocked` while a message is open.
    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        let hot = if blocked { None } else { self.hit(x, y) };
        if hot != self.hot {
            self.hot = hot;
            self.fx.redraw();
        }
        self.cursor_icon = match hot {
            Some(Hit::Top(i)) if self.possible(TOP[i]) => CursorIcon::Pointer,
            Some(Hit::Dns(i)) if self.possible(Action::Dns(Provider::ALL[i])) => CursorIcon::Pointer,
            Some(Hit::Connection(_)) => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        };
    }

    pub fn pointer_left(&mut self) {
        if self.hot.take().is_some() {
            self.fx.redraw();
        }
    }

    pub fn release(&mut self) {}

    /// A click: a connection is selected; a button gives its action for the window to do.
    pub fn press(&mut self) -> Option<Action> {
        self.note = None;
        let out = match self.hot {
            Some(Hit::Top(i)) => Some(TOP[i]),
            Some(Hit::Dns(i)) => Some(Action::Dns(Provider::ALL[i])),
            Some(Hit::Connection(i)) => {
                self.sel = i;
                None
            }
            None => None,
        };
        self.fx.menus();
        self.fx.redraw();
        out
    }

    /// The arrows choose the connection; F5 reads again.
    pub fn key(&mut self, key: &winit::keyboard::Key) -> Option<Action> {
        use winit::keyboard::{Key, NamedKey};
        let n = self.connections().len();
        match key {
            Key::Named(NamedKey::F5) => return Some(Action::Refresh),
            Key::Named(NamedKey::ArrowDown) if n > 0 => self.sel = (self.sel + 1).min(n - 1),
            Key::Named(NamedKey::ArrowUp) => self.sel = self.sel.saturating_sub(1),
            _ => return None,
        }
        self.fx.menus();
        self.fx.redraw();
        None
    }

    /// The page's nodes for the screen readers: the connections are 100 + their index.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role};
        let mut list = Node::new(Role::List);
        list.set_label(tr("Internet Booster"));
        let mut rows = Vec::new();
        for (i, c) in self.connections().iter().enumerate() {
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::ListItem);
            n.set_label(format!("{}: {}", c.name, c.v4.iter().chain(&c.v6).cloned().collect::<Vec<_>>().join(", ")));
            n.set_selected(self.sel == i);
            n.add_action(accesskit::Action::Click);
            rows.push(nid);
            nodes.push((nid, n));
        }
        list.set_children(rows);
        nodes.push((NodeId(1), list));
        children.push(NodeId(1));
        (!self.connections().is_empty()).then(|| NodeId(100 + self.sel as u64))
    }

    /// A screen reader clicked a connection: it is selected.
    pub fn accessibility_click(&mut self, i: usize) {
        if i < self.connections().len() {
            self.sel = i;
            self.fx.menus();
            self.fx.redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERE: &str = "tcp\tDisabled\tDisabled\tDisabled\t1000\tCUBIC\tEnabled\tEnabled\n\
dns\t14\tEthernet\tIntel Ethernet\tFalse\t192.168.1.1\t\ndns\t7\tWi-Fi\tIntel Wi-Fi\tTrue\t1.1.1.1,1.0.0.1\t\n";

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

    #[test]
    fn what_can_be_done() {
        let dir = std::env::temp_dir().join(format!("rcare-net-{}", std::process::id()));
        let mut p = NetPage::new(false, dir.clone());
        p.net = Some(net::parse(HERE));
        // Auto-tuning was changed: it can be put back, as administrator.
        assert!(p.possible(Action::RestoreTcp) && !p.enabled(Action::RestoreTcp));
        p.act(Action::RestoreTcp, &mut No);
        assert!(p.fx.take().info.is_some(), "says why");
        p.admin = true;
        assert!(p.enabled(Action::RestoreTcp));
        // The router's DNS: Automatic is what it is already; the others can be chosen.
        assert!(!p.enabled(Action::Dns(Provider::Automatic)) && p.enabled(Action::Dns(Provider::Quad9)));
        p.sel = 1;
        assert!(p.enabled(Action::Dns(Provider::Automatic)) && !p.enabled(Action::Dns(Provider::Cloudflare)));
        // Said no: nothing is written, nothing runs.
        p.act(Action::Dns(Provider::Google), &mut No);
        assert!(!p.busy() && !dir.exists());
    }

    #[test]
    fn layout_and_pointer() {
        let mut p = NetPage::new(true, std::env::temp_dir());
        p.net = Some(net::parse(HERE));
        p.layout = p.layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        let l = p.layout.clone();
        assert!(l.tcp.right() < l.dns.x && l.chips[3].right() <= l.dns.right());
        p.pointer(l.rows[1].x + 30.0, l.rows[1].y + 10.0, false);
        assert_eq!(p.hot, Some(Hit::Connection(1)));
        assert_eq!(p.press(), None);
        assert_eq!(p.sel, 1);
        p.pointer(l.chips[2].x + 3.0, l.chips[2].y + 3.0, false);
        assert_eq!(p.press(), Some(Action::Dns(Provider::Quad9)));
        assert_eq!(p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::F5)), Some(Action::Refresh));
        p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowUp));
        assert_eq!(p.sel, 0);
    }
}
