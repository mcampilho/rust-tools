//! System Maintenance: the checks that exist so far, run in one go, one row
//! each, with what each found and a button to the page where it is fixed.
//! Like Custom Clean, it is rcare's own page and changes nothing: the
//! cleaner looks for junk, broken shortcuts and invalid registry entries,
//! the uninstaller for programs with a problem, the Privacy Sweep counts
//! Windows' traces, and Windows says how the drives are (`disks`). More checks join as the later phases bring them.
//!
//! This module only lays out, finds what the pointer is on and draws; the
//! window (`shell.rs`) reads the other pages and fills a [`View`].

use crate::custom::Status;
use crate::i18n::{tf, tr};
use rt_gfx::gfx::{Align, Color, FontId, Gfx, Rect};
use rt_ui::page::Effects;
use rt_ui::theme::Theme;
use winit::window::CursorIcon;

/// How many checks there are.
pub const N: usize = 6;

/// One check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    Junk,
    Privacy,
    Shortcuts,
    Registry,
    Software,
    Disks,
}

impl Check {
    pub const ALL: [Check; N] = [Check::Junk, Check::Privacy, Check::Shortcuts, Check::Registry, Check::Software, Check::Disks];

    /// The row's title (a translation key).
    pub fn title(self) -> &'static str {
        match self {
            Check::Junk => "Junk Files",
            Check::Privacy => "Privacy Sweep",
            Check::Shortcuts => "Broken Shortcuts",
            Check::Registry => "Invalid Registry Entries",
            Check::Software => "Software Health",
            Check::Disks => "Drive Health",
        }
    }

    /// What the row is about (a translation key).
    pub fn about(self) -> &'static str {
        match self {
            Check::Junk => "Temporary files and the caches of the system, the browsers and the package managers.",
            Check::Privacy => "The traces Windows keeps: recent documents, the Run box, Explorer's history.",
            Check::Shortcuts => "Shortcuts whose program or file is gone.",
            Check::Registry => "Uninstall entries, startup entries and paths that point to nothing.",
            Check::Software => "Programs whose uninstaller is gone or whose install failed.",
            Check::Disks => "What Windows says about each drive.",
        }
    }

    /// The page that fixes it (an id of `pages::PAGES`); None: its details are shown here.
    pub fn page_id(self) -> Option<&'static str> {
        match self {
            Check::Junk | Check::Shortcuts | Check::Registry => Some("clean"),
            Check::Software => Some("inst"),
            Check::Privacy => Some("privacy"),
            Check::Disks => None,
        }
    }
}

/// What a row says once it is done, and whether that is good news.
pub fn result_line(check: Check, status: &Status) -> (String, Option<bool>) {
    match status {
        Status::NotYet => (tr("Not checked yet"), None),
        Status::Working => (tr("Checking…"), None),
        Status::Failed(why) => (why.clone(), Some(false)),
        Status::Done { bytes, count } => {
            let n = rsize::format::count(*count);
            let none = *count == 0;
            let one = *count == 1;
            let text = match (check, none) {
                (Check::Shortcuts, false) if one => tr("One broken shortcut"),
                (Check::Registry, false) if one => tr("One invalid entry"),
                (Check::Software, false) if one => tr("One program with a problem"),
                (Check::Disks, false) if one => tr("One drive needs a repair"),
                (Check::Privacy, false) if one => tr("One trace"),
                (Check::Privacy, true) => tr("No traces"),
                (Check::Privacy, false) => tf("{} traces", &[&n]),
                (Check::Junk, true) => tr("Nothing to clean"),
                (Check::Junk, false) => tf("{} in {} files", &[&rsize::format::size(*bytes), &n]),
                (Check::Shortcuts, true) => tr("No broken shortcuts"),
                (Check::Shortcuts, false) => tf("{} broken shortcuts", &[&n]),
                (Check::Registry, true) => tr("No invalid entries"),
                (Check::Registry, false) => tf("{} invalid entries", &[&n]),
                (Check::Software, true) => tr("No problems found"),
                (Check::Software, false) => tf("{} programs with a problem", &[&n]),
                (Check::Disks, true) => tr("All drives are healthy"),
                (Check::Disks, false) => tf("{} drives need a repair", &[&n]),
            };
            (text, Some(none))
        }
    }
}

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Scan,
    /// The button of a row (index into `Check::ALL`).
    Row(usize),
}

/// What the page shows.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub status: [Status; N],
    pub hot: Option<Hit>,
    /// The checks can be started (nothing is running).
    pub can_scan: bool,
}

impl View {
    /// Once every check is done: how many found something to look at.
    pub fn findings(&self) -> Option<usize> {
        self.status
            .iter()
            .map(|s| match s {
                Status::Done { count, .. } => Some(usize::from(*count > 0)),
                Status::Failed(_) => Some(1),
                _ => None,
            })
            .sum()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub scan: Rect,
    pub rows: [Rect; N],
    pub buttons: [Rect; N],
}

/// Where everything goes in `area`: the rows as tall as the page lets them be.
pub fn layout(area: Rect, s: f32) -> Layout {
    let pad = (24.0 * s).round();
    let inner = Rect::new(area.x + pad, area.y + pad, (area.w - 2.0 * pad).max(0.0), (area.h - 2.0 * pad).max(0.0));
    let bh = (32.0 * s).round();
    let scan = Rect::new(inner.x, inner.y + (66.0 * s).round(), (180.0 * s).round(), bh);
    let top = scan.bottom() + (20.0 * s).round();
    let gap = (6.0 * s).round();
    let h = ((inner.bottom() - top - (N - 1) as f32 * gap) / N as f32).clamp(44.0 * s, 68.0 * s).floor();
    let mut rows = [Rect::default(); N];
    for (i, r) in rows.iter_mut().enumerate() {
        *r = Rect::new(inner.x, top + i as f32 * (h + gap), inner.w, h);
    }
    let buttons = rows.map(|r| {
        let w = (110.0 * s).round();
        Rect::new(r.right() - w - (12.0 * s).round(), r.y + ((r.h - bh) / 2.0).round(), w, bh)
    });
    Layout { area, scan, rows, buttons }
}

pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Hit> {
    if l.scan.contains(x, y) {
        Some(Hit::Scan)
    } else {
        l.buttons.iter().position(|r| r.contains(x, y)).map(Hit::Row)
    }
}

/// The label of a row's button.
pub fn button_label(check: Check) -> String {
    if check.page_id().is_some() {
        tr("Review")
    } else {
        tr("Details")
    }
}

fn button(g: &mut Gfx, r: Rect, label: &str, th: &Theme, s: f32, primary: bool, hot_on: (bool, bool)) {
    let (hot, on) = hot_on;
    let p = 12.5 * s;
    if primary {
        let fill = if !on {
            th.accent.mix(th.bg, 0.55)
        } else if hot {
            th.accent
        } else {
            th.accent.mix(th.bg, 0.12)
        };
        g.fill_rrect(r, 5.0 * s, fill);
        let fg = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
        g.text_in(label, r, p, FontId::Semibold, fg, Align::Center);
    } else {
        g.fill_rrect(r, 5.0 * s, th.bg);
        if hot && on {
            g.fill_rrect(r, 5.0 * s, th.hover);
        }
        g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
        g.text_in(label, r, p, FontId::Regular, if on { th.text } else { th.dim }, Align::Center);
    }
}

/// Draws the page.
pub fn draw(g: &mut Gfx, l: &Layout, th: &Theme, s: f32, v: &View) {
    use rt_core::i18n::strip_mnemonic;
    use rt_gfx::dialog::ellipsize;
    g.fill_rect(l.area, th.bg);
    let pad = (24.0 * s).round();
    let x = l.area.x + pad;
    let w = (l.area.w - 2.0 * pad).max(0.0);
    let y = l.area.y + pad;
    g.text_in(&tr("System Maintenance"), Rect::new(x, y, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
    let about = ellipsize(
        g,
        &tr("Checks the computer in one go. Nothing is changed here: each finding opens the page where it is fixed."),
        w,
        12.5 * s,
        FontId::Regular,
    );
    g.text_in(&about, Rect::new(x, y + 34.0 * s, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
    button(g, l.scan, &strip_mnemonic(&tr("&Scan")), th, s, true, (v.hot == Some(Hit::Scan), v.can_scan));
    if let Some(n) = v.findings() {
        let text = if n == 0 { tr("Everything looks fine.") } else { tf("{} checks found something to look at.", &[&n.to_string()]) };
        let r = Rect::new(l.scan.right() + 16.0 * s, l.scan.y, (x + w - l.scan.right() - 16.0 * s).max(0.0), l.scan.h);
        let text = ellipsize(g, &text, r.w, 13.0 * s, FontId::Semibold);
        g.text_in(&text, r, 13.0 * s, FontId::Semibold, th.text, Align::Left);
    }
    for (i, check) in Check::ALL.iter().enumerate() {
        let r = l.rows[i];
        g.fill_rrect(r, 6.0 * s, th.panel);
        g.stroke_rrect(r, 6.0 * s, 1.0, th.border);
        let b = l.buttons[i];
        // The result takes up to a third of the row, the title and what it is about the rest.
        let result_w = (r.w * 0.34).min(320.0 * s);
        let result = Rect::new(b.x - result_w - 12.0 * s, r.y, result_w, r.h);
        let left = Rect::new(r.x + 14.0 * s, r.y, (result.x - r.x - 24.0 * s).max(0.0), r.h);
        let mid = r.y + r.h / 2.0;
        let title = ellipsize(g, &tr(check.title()), left.w, 13.5 * s, FontId::Semibold);
        g.text_in(&title, Rect::new(left.x, mid - 20.0 * s, left.w, 20.0 * s), 13.5 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(g, &tr(check.about()), left.w, 11.5 * s, FontId::Regular);
        g.text_in(&about, Rect::new(left.x, mid + 1.0 * s, left.w, 18.0 * s), 11.5 * s, FontId::Regular, th.dim, Align::Left);
        let (text, good) = result_line(*check, &v.status[i]);
        let color = match good {
            Some(true) => th.good,
            Some(false) => th.warn,
            None => th.dim,
        };
        let text = ellipsize(g, &text, result.w, 12.5 * s, FontId::Semibold);
        g.text_in(&text, result, 12.5 * s, FontId::Semibold, color, Align::Right);
        let on = matches!(v.status[i], Status::Done { .. } | Status::Failed(_));
        button(g, b, &button_label(*check), th, s, false, (v.hot == Some(Hit::Row(i)), on));
    }
}

/// The page's state, as the window holds it.
#[derive(Default)]
pub struct MaintPage {
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    /// What it shows: the window fills the rows' status before drawing.
    pub view: View,
}

impl MaintPage {
    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        self.layout = layout(area, s);
        self.view.hot = self.hot;
        draw(g, &self.layout, th, s, &self.view);
    }

    /// Whether what the pointer is on does something now.
    fn works(&self, h: Hit) -> bool {
        match h {
            Hit::Scan => self.view.can_scan,
            Hit::Row(i) => matches!(self.view.status.get(i), Some(Status::Done { .. } | Status::Failed(_))),
        }
    }

    /// What the pointer is on; `blocked` while a message is open.
    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        let hot = if blocked { None } else { hit(&self.layout, x, y).filter(|&h| self.works(h)) };
        if hot != self.hot {
            self.hot = hot;
            self.fx.redraw();
        }
        self.cursor_icon = if hot.is_some() { CursorIcon::Pointer } else { CursorIcon::Default };
    }

    pub fn pointer_left(&mut self) {
        if self.hot.take().is_some() {
            self.fx.redraw();
        }
    }

    pub fn release(&mut self) {}

    /// A click: what was clicked, for the window to do.
    pub fn press(&mut self) -> Option<Hit> {
        self.hot.filter(|&h| self.works(h))
    }

    /// The page's nodes for the screen readers: the buttons are 30 (Scan)
    /// and 32–36 (the rows'); the rows' texts 40–44.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role};
        use rt_core::i18n::strip_mnemonic;
        let mut button = |id: u64, label: String, on: bool| {
            let mut n = Node::new(Role::Button);
            n.set_label(label);
            if on {
                n.add_action(accesskit::Action::Click);
            } else {
                n.set_disabled();
            }
            nodes.push((NodeId(id), n));
            children.push(NodeId(id));
        };
        button(30, strip_mnemonic(&tr("&Scan")), self.view.can_scan);
        for (i, check) in Check::ALL.iter().enumerate() {
            let on = self.works(Hit::Row(i));
            button(32 + i as u64, format!("{} — {}", button_label(*check), tr(check.title())), on);
        }
        for (i, check) in Check::ALL.iter().enumerate() {
            let mut n = Node::new(Role::Label);
            n.set_value(format!("{}: {}", tr(check.title()), result_line(*check, &self.view.status[i]).0));
            nodes.push((NodeId(40 + i as u64), n));
            children.push(NodeId(40 + i as u64));
        }
        None
    }

    /// The page has no rows for a screen reader to click.
    pub fn accessibility_click(&mut self, _i: usize) {}
}

/// The text of the Details of Drive Health: each drive and what Windows says, and how to repair one.
pub fn disks_text(volumes: &[crate::disks::Volume]) -> String {
    let mut lines: Vec<String> = volumes
        .iter()
        .map(|v| {
            let name = if v.label.is_empty() { format!("{}:", v.letter) } else { format!("{}: ({})", v.letter, v.label) };
            let fs = if v.file_system.is_empty() { String::new() } else { format!(", {}", v.file_system) };
            format!("{name}{fs} — {} / {}", v.health, v.status)
        })
        .collect();
    if let Some(bad) = volumes.iter().find(|v| v.needs_repair()) {
        lines.push(String::new());
        lines.push(tf(
            "To repair it, run “chkdsk {}: /f” as administrator, or use Windows' own check: the drive's Properties › Tools › Check.",
            &[&bad.letter.to_string()],
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_and_buttons_are_found_and_fit() {
        for (w, h, s) in [(956.0, 700.0, 1.0), (536.0, 480.0, 1.0), (1900.0, 1400.0, 2.0)] {
            let l = layout(Rect::new(224.0, 0.0, w, h), s);
            assert_eq!(hit(&l, l.scan.x + 3.0, l.scan.y + 3.0), Some(Hit::Scan));
            for (i, b) in l.buttons.iter().enumerate() {
                assert_eq!(hit(&l, b.x + 3.0, b.y + 3.0), Some(Hit::Row(i)));
                assert!(b.right() <= l.rows[i].right() && b.y >= l.rows[i].y && b.bottom() <= l.rows[i].bottom());
            }
            for pair in l.rows.windows(2) {
                assert!(pair[0].bottom() <= pair[1].y);
            }
            assert!(l.rows[N - 1].bottom() <= l.area.bottom(), "the last row is cut at {w}x{h}");
        }
    }

    #[test]
    fn what_rows_say() {
        crate::i18n::set_lang(crate::i18n::Lang::EnUS);
        assert_eq!(result_line(Check::Disks, &Status::Done { bytes: 0, count: 0 }), ("All drives are healthy".into(), Some(true)));
        let (t, good) = result_line(Check::Shortcuts, &Status::Done { bytes: 0, count: 3 });
        assert!(t.contains('3') && good == Some(false));
        assert_eq!(result_line(Check::Junk, &Status::NotYet).1, None);
        let mut v = View::default();
        assert_eq!(v.findings(), None);
        v.status = std::array::from_fn(|i| Status::Done { bytes: 0, count: u64::from(i == 1) });
        assert_eq!(v.findings(), Some(1));
    }

    #[test]
    fn every_row_opens_a_page_or_shows_details() {
        for c in Check::ALL {
            if let Some(id) = c.page_id() {
                assert!(crate::pages::PAGES.iter().any(|p| p.id == id));
            }
        }
        let d = crate::disks::parse("C|Healthy|OK|NTFS|SYSTEM\nS|Warning|Full Repair Needed|exFAT|SOFTWARE\n");
        let text = disks_text(&d);
        assert!(text.contains("S: (SOFTWARE)") && text.contains("chkdsk S: /f"));
    }
}
