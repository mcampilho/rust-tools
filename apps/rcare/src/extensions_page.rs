//! The Extensions page: what is added to the browsers (`extensions`), one row
//! each with its browser, profile, whether it is on and who put it there.
//! Nothing is changed from here: "Open in Browser" opens the browser's own
//! extensions page (on that extension, where the browser can), where it is
//! turned off or removed.

use crate::extensions::{self, Extension};
use crate::i18n::{tf, tr};
use rt_gfx::gfx::{Align, FontId, Gfx, Rect};
use rt_ui::page::Effects;
use rt_ui::theme::Theme;
use winit::window::CursorIcon;

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// A button of the bar (index into `ACTIONS`).
    Action(usize),
    Row(usize),
}

/// What the buttons above the list do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    OpenBrowser,
    OpenFolder,
    Refresh,
}

pub const ACTIONS: [Action; 3] = [Action::OpenBrowser, Action::OpenFolder, Action::Refresh];

impl Action {
    /// The menu label (a translation key with its access key).
    pub fn label_key(self) -> &'static str {
        match self {
            Action::OpenBrowser => "Open in &Browser",
            Action::OpenFolder => "Open &Folder",
            Action::Refresh => "&Refresh",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub actions: [Rect; 3],
    pub header: Rect,
    pub list: Rect,
    pub row_h: f32,
    /// Columns (x, width): name, browser, profile, state, source.
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

#[derive(Default)]
pub struct ExtensionsPage {
    pub list: Vec<Extension>,
    pub sel: Option<usize>,
    pub scroll: usize,
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    /// The list was read at least once.
    pub read: bool,
    /// A line for the status bar (what was just done).
    pub note: Option<String>,
}

impl ExtensionsPage {
    /// Reads the list again, keeping the selection on the same extension.
    pub fn refresh(&mut self) {
        let keep = self.sel.and_then(|i| self.list.get(i)).map(|e| (e.browser, e.profile_dir.clone(), e.id.clone()));
        self.list = extensions::read();
        self.sel = keep.and_then(|(b, p, id)| self.list.iter().position(|e| e.browser == b && e.profile_dir == p && e.id == id));
        self.scroll = self.scroll.min(self.list.len().saturating_sub(1));
        self.read = true;
        self.fx.menus();
        self.fx.redraw();
    }

    pub fn enabled(&self, a: Action) -> bool {
        let sel = self.sel.and_then(|i| self.list.get(i));
        match a {
            Action::OpenBrowser => sel.is_some(),
            Action::OpenFolder => sel.is_some_and(|e| e.folder.as_ref().is_some_and(|f| f.exists())),
            Action::Refresh => true,
        }
    }

    /// Does what a button or its menu entry asks.
    pub fn act(&mut self, a: Action) {
        if !self.enabled(a) {
            return;
        }
        let sel = self.sel.and_then(|i| self.list.get(i)).cloned();
        match (a, sel) {
            (Action::OpenBrowser, Some(e)) => match extensions::open_page(&e) {
                Ok(()) => self.note = Some(tf("The extensions page of {} was opened.", &[e.browser.name()])),
                Err(why) => self.fx.info(&tr("Extensions"), &tf("Could not open the browser: {}", &[&why])),
            },
            (Action::OpenFolder, Some(e)) => {
                if let Some(f) = e.folder {
                    rt_sys::reveal(&f);
                }
            }
            (Action::Refresh, _) => self.refresh(),
            _ => {}
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
            Action::Refresh => 110.0,
            _ => 160.0,
        });
        // On a narrow page the buttons shrink together to fit.
        let gap = 8.0 * s;
        let fit = ((inner.w - 2.0 * gap) / (wants.iter().sum::<f32>() * s)).min(1.0);
        let mut actions = [Rect::default(); 3];
        let mut x = inner.x;
        for (i, want) in wants.iter().enumerate() {
            let w = (want * s * fit).floor();
            actions[i] = Rect::new(x, top, w, bh);
            x += w + gap;
        }
        let header = Rect::new(inner.x, top + bh + (14.0 * s).round(), inner.w, (26.0 * s).round());
        let status_h = (24.0 * s).round();
        let list = Rect::new(inner.x, header.bottom(), inner.w, (area.bottom() - status_h - header.bottom()).max(0.0));
        let status = Rect::new(area.x, area.bottom() - status_h, area.w, status_h);
        // Name gets what the others leave; they keep a share each.
        let shares = [0.0, 0.17, 0.14, 0.1, 0.14];
        let mut cols = [(0.0, 0.0); 5];
        let rest_w: f32 = shares.iter().map(|f| f * inner.w).sum();
        let name_w = (inner.w - rest_w).max(0.0);
        let mut x = inner.x;
        for (i, f) in shares.iter().enumerate() {
            let w = if i == 0 { name_w } else { f * inner.w };
            cols[i] = (x, w);
            x += w;
        }
        Layout { area, actions, header, list, row_h: (30.0 * s).round(), cols, status }
    }

    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        use rt_core::i18n::strip_mnemonic;
        use rt_gfx::dialog::ellipsize;
        let l = Self::layout_in(area, s);
        g.fill_rect(area, th.bg);
        let pad = (24.0 * s).round();
        let (x, w) = (area.x + pad, (area.w - 2.0 * pad).max(0.0));
        g.text_in(&tr("Extensions"), Rect::new(x, area.y + pad, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
        let about = ellipsize(
            g,
            &tr("What is added to the browsers. An extension is turned off or removed in the browser's own page: select it and open it there."),
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
        for (title, (cx, cw)) in ["Name", "Browser", "Profile", "State", "Installed by"].iter().zip(l.cols) {
            let r = Rect::new(cx + 10.0 * s, hd.y, (cw - 16.0 * s).max(0.0), hd.h);
            let t = ellipsize(g, &tr(title), r.w, p, FontId::Semibold);
            g.text_in(&t, r, p, FontId::Semibold, th.dim, Align::Left);
        }
        // Rows.
        let end = (self.scroll + l.page_rows()).min(self.list.len());
        g.set_clip(Some(l.list));
        for i in self.scroll..end {
            let e = &self.list[i];
            let y = l.list.y + (i - self.scroll) as f32 * l.row_h;
            let row = Rect::new(l.list.x, y, l.list.w, l.row_h);
            if self.sel == Some(i) {
                g.fill_rect(row, th.selection);
            } else if self.hot == Some(Hit::Row(i)) {
                g.fill_rect(row, th.hover);
            }
            let cell = |c: (f32, f32)| Rect::new(c.0 + 10.0 * s, y, (c.1 - 16.0 * s).max(0.0), l.row_h);
            let name_r = cell(l.cols[0]);
            let name = ellipsize(g, &e.name, name_r.w, p, FontId::Regular);
            g.text_in(&name, name_r, p, FontId::Regular, if e.enabled { th.text } else { th.dim }, Align::Left);
            let state = if e.enabled { tr("On") } else { tr("Off") };
            let source = tr(e.source.label());
            for (k, text) in [(1, e.browser.name()), (2, e.profile.as_str()), (3, state.as_str()), (4, source.as_str())] {
                let r = cell(l.cols[k]);
                let t = ellipsize(g, text, r.w, 11.5 * s, FontId::Regular);
                let color = if k == 3 && e.enabled { th.good } else { th.dim };
                g.text_in(&t, r, 11.5 * s, FontId::Regular, color, Align::Left);
            }
        }
        g.set_clip(None);
        if self.read && self.list.is_empty() {
            g.text_in(&tr("No browser extensions were found."), l.list, p, FontId::Regular, th.dim, Align::Center);
        }
        // Status bar.
        g.fill_rect(l.status, th.status);
        g.fill_rect(Rect::new(l.status.x, l.status.y, l.status.w, 1.0), th.border);
        let on = self.list.iter().filter(|e| e.enabled).count();
        let mut browsers: Vec<_> = self.list.iter().map(|e| e.browser).collect();
        browsers.dedup();
        let left = self.note.clone().unwrap_or_else(|| {
            tf("Extensions: {} in {} browsers. On: {}.", &[&self.list.len().to_string(), &browsers.len().to_string(), &on.to_string()])
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
        (i < self.list.len()).then_some(Hit::Row(i))
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
            _ => CursorIcon::Default,
        };
    }

    pub fn pointer_left(&mut self) {
        if self.hot.take().is_some() {
            self.fx.redraw();
        }
    }

    pub fn release(&mut self) {}

    /// A click: a button acts, a row is selected.
    pub fn press(&mut self) {
        self.note = None;
        match self.hot {
            Some(Hit::Action(i)) => self.act(ACTIONS[i]),
            Some(Hit::Row(i)) => self.sel = Some(i),
            None => {}
        }
        self.fx.menus();
        self.fx.redraw();
    }

    /// The arrows move; Enter opens the browser's page; F5 reads again.
    pub fn key(&mut self, key: &winit::keyboard::Key) {
        use winit::keyboard::{Key, NamedKey};
        let n = self.list.len();
        match key {
            Key::Named(NamedKey::F5) => return self.refresh(),
            Key::Named(NamedKey::Enter) => return self.act(Action::OpenBrowser),
            _ if n == 0 => return,
            _ => {}
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
        let max = self.list.len().saturating_sub(self.layout.page_rows());
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
        list.set_label(tr("Extensions"));
        let mut rows = Vec::new();
        let end = (self.scroll + self.layout.page_rows()).min(self.list.len());
        for i in self.scroll..end {
            let e = &self.list[i];
            let nid = NodeId(100 + i as u64);
            let mut n = Node::new(Role::ListItem);
            let state = if e.enabled { tr("On") } else { tr("Off") };
            n.set_label(format!("{}, {}, {}, {state}", e.name, e.browser.name(), e.profile));
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

    /// A screen reader clicked the row `i`: it is selected.
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
    use crate::extensions::{Browser, Source};

    fn ext(i: usize) -> Extension {
        Extension {
            id: format!("id{i}"),
            name: format!("Ext {i}"),
            version: "1.0".into(),
            browser: if i.is_multiple_of(2) { Browser::Chrome } else { Browser::Brave },
            profile_dir: "Default".into(),
            profile: "Me".into(),
            enabled: !i.is_multiple_of(3),
            source: Source::Store,
            folder: None,
        }
    }

    #[test]
    fn rows_buttons_and_keys() {
        let mut p = ExtensionsPage { list: (0..40).map(ext).collect(), ..ExtensionsPage::default() };
        assert!(!p.enabled(Action::OpenBrowser) && p.enabled(Action::Refresh));
        p.layout = ExtensionsPage::layout_in(Rect::new(224.0, 0.0, 956.0, 700.0), 1.0);
        let l = p.layout.clone();
        // The columns fill the list and do not overlap.
        assert!(l.cols.windows(2).all(|w| w[0].0 + w[0].1 <= w[1].0 + 0.01));
        assert!((l.cols[4].0 + l.cols[4].1 - l.list.right()).abs() < 0.5);
        p.pointer(l.cols[0].0 + 20.0, l.list.y + l.row_h * 4.5, false);
        assert_eq!(p.hot, Some(Hit::Row(4)));
        p.press();
        assert_eq!(p.sel, Some(4));
        assert!(p.enabled(Action::OpenBrowser) && !p.enabled(Action::OpenFolder));
        p.pointer(l.actions[2].x + 3.0, l.actions[2].y + 3.0, false);
        assert_eq!(p.hot, Some(Hit::Action(2)));
        p.pointer(l.actions[2].x + 3.0, l.actions[2].y + 3.0, true);
        assert_eq!(p.hot, None, "nothing is hot under a message");
        p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::End));
        assert_eq!(p.sel, Some(39));
        assert!(p.scroll + l.page_rows() > 39);
        p.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::Home));
        assert_eq!((p.sel, p.scroll), (Some(0), 0));
    }
}
