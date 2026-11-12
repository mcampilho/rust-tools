//! Drawing a frame: the page, the treemap or chart it needs (made again only
//! when what it shows changed), the menus, the message; and the tree for
//! screen readers.

use super::*;

impl Shell {
    pub(super) fn render(&mut self) {
        let dark = self.dark();
        let theme = Theme::new(dark, self.accent);
        let s = self.scale();
        let Some(win) = &mut self.win else { return };
        let size = win.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if win.surface.resize(w, h).is_err() {
            return;
        }
        self.gfx.resize(w.get(), h.get());
        let full = Rect::new(0.0, 0.0, w.get() as f32, h.get() as f32);
        self.layout = view::layout(&self.app, self.settings.view, self.settings.panes(), full, s, cfg!(not(windows)));
        self.app.page = self.layout.page_rows();
        self.app.types_page = self.layout.ext_rows();
        self.ensure_map(theme.bg);
        self.ensure_lit();
        let frame = view::Frame {
            mode: self.settings.size_mode,
            kind: self.settings.chart_shown(),
            map: self.map.as_ref(),
            lit: self.lit.as_ref().map(|l| &l.1),
            chart: self.chart.as_ref(),
            hover_seg: self.hover_seg,
            hot: self.hot,
            hover: self.hover,
        };
        view::draw(&mut self.gfx, &self.app, &self.layout, &theme, s, &frame);
        #[cfg(not(windows))]
        {
            let g = &mut self.gfx;
            self.menu.draw_bar(g, self.layout.bar, &theme.menu_style(), s);
            self.menu.draw_popups(g, full, &theme.menu_style(), s);
        }
        if let Some(d) = &mut self.dialog {
            d.draw(&mut self.gfx, full, &theme.dialog_style(), s);
        }
        let Some(win) = &mut self.win else { return };
        rt_ui::window::present(&mut win.surface, &self.gfx);
        self.gfx.trim_cache();
        self.sync_menu();
        self.sync_menu_theme(dark);
        self.update_title();
        let tree = self.tree();
        if let Some(win) = &mut self.win {
            win.adapter.update_if_active(|| tree);
        }
    }

    /// Makes the treemap again when the folder, the size or the order
    /// changed. While the window is being resized, the old one is stretched
    /// and the new one waits until the size has stayed the same for a moment:
    /// a large tree takes a fraction of a second to lay out.
    pub(super) fn ensure_map(&mut self, bg: Color) {
        if view::page(&self.app) != Page::Result {
            return;
        }
        let r = self.layout.map;
        let (w, h) = (r.w.max(1.0) as u32, r.h.max(1.0) as u32);
        let key = (self.app.zoom, w, h, self.settings.size_mode, self.tree_gen);
        // The table of file types follows the folder the chart shows.
        let ekey = (self.app.zoom, self.settings.size_mode, self.tree_gen);
        if self.exts_key != Some(ekey) {
            self.exts_key = Some(ekey);
            self.app.refresh_exts();
        }
        let Some(scan) = &self.app.scan else { return };
        if self.settings.chart_shown() != ChartType::Treemap {
            // A few thousand segments at most: quick enough to redo at once.
            let s = self.scale();
            let bg = (bg.r as u32) << 16 | (bg.g as u32) << 8 | bg.b as u32;
            let ckey = (self.settings.chart_shown(), key, bg, s.to_bits());
            if self.chart_key != Some(ckey) || self.chart.is_none() {
                let t = &scan.tree;
                self.chart = Some(match self.settings.chart_shown() {
                    ChartType::Pie => Chart::pie(t, self.app.zoom, w, h, s, bg),
                    _ => Chart::rings(t, self.app.zoom, w, h, s, bg),
                });
                self.chart_key = Some(ckey);
                self.hover_seg = None;
            }
            return;
        }
        if self.map_key == Some(key) && self.map.is_some() {
            self.map_due = None;
            return;
        }
        let only_size = self.map_key.is_some_and(|k| (k.0, k.3, k.4) == (key.0, key.3, key.4)) && self.map.is_some();
        if only_size && self.resized.elapsed() < RESIZE_PAUSE {
            self.map_due = Some(self.resized + RESIZE_PAUSE);
            return;
        }
        let head = (16.0 * self.scale()).round();
        self.map = Some(Map::new(&scan.tree, self.app.zoom, w, h, head));
        self.map_key = Some(key);
        self.map_due = None;
    }

    /// Makes the treemap with the files of the picked type standing out,
    /// when a type is picked and the treemap or the type changed.
    pub(super) fn ensure_lit(&mut self) {
        let want = match (self.app.ext_sel, &self.map, self.map_key, &self.app.scan) {
            (Some(e), Some(m), Some(k), Some(scan)) if self.settings.chart_shown() == ChartType::Treemap => Some((k, e, m, &scan.tree)),
            _ => None,
        };
        match want {
            Some((k, e, m, t)) => {
                if self.lit.as_ref().map(|l| l.0) != Some((k, e)) {
                    self.lit = Some(((k, e), m.highlighted(t, e)));
                }
            }
            None => self.lit = None,
        }
    }

    /// A point of the window in the treemap's own pixels (it may be
    /// stretched while the window is resized).
    pub(super) fn map_point(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let (m, r) = (self.map.as_ref()?, self.layout.map);
        (r.contains(x, y) && r.w > 0.0 && r.h > 0.0).then(|| ((x - r.x) * m.w as f32 / r.w, (y - r.y) * m.h as f32 / r.h))
    }

    pub(super) fn tree(&self) -> accesskit::TreeUpdate {
        use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
        let root = NodeId(0);
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        let mut focus = root;
        if let (Page::Result, Some(scan)) = (view::page(&self.app), &self.app.scan) {
            let t = &scan.tree;
            let mut list = Node::new(Role::Tree);
            list.set_label(tr("Folders"));
            let mut rows = Vec::new();
            let end = (self.app.scroll + self.layout.page_rows()).min(self.app.rows.len());
            for (i, &(id, depth)) in self.app.rows.iter().enumerate().take(end).skip(self.app.scroll) {
                let nid = NodeId(100 + i as u64);
                let mut n = Node::new(Role::TreeItem);
                n.set_label(format!("{}, {}", t.name(id), rsize::format::size(t.size(id))));
                n.set_level(depth as usize + 1);
                if !t.children(id).is_empty() {
                    n.set_expanded(self.app.expanded.contains(&id));
                }
                n.set_selected(self.app.sel == Some(id));
                n.add_action(accesskit::Action::Click);
                if self.app.sel == Some(id) {
                    focus = nid;
                }
                rows.push(nid);
                nodes.push((nid, n));
            }
            list.set_children(rows);
            nodes.push((NodeId(1), list));
            let mut map = Node::new(Role::Image);
            map.set_label(view::chart_name(self.settings.chart_shown()));
            nodes.push((NodeId(2), map));
            children.extend([NodeId(1), NodeId(2)]);
        }
        let (left, right) = view::status_text(&self.app, None);
        let mut st = Node::new(Role::Label);
        st.set_value(format!("{left} {right}"));
        nodes.push((NodeId(3), st));
        children.push(NodeId(3));
        if let Some(dl) = &self.dialog {
            let mut dn = Node::new(Role::Dialog);
            dn.set_label(dl.title.clone());
            dn.set_children(vec![NodeId(5), NodeId(6)]);
            let mut tn = Node::new(Role::Label);
            tn.set_value(dl.spoken_text());
            let mut ok = Node::new(Role::Button);
            ok.set_label("OK");
            ok.add_action(accesskit::Action::Click);
            nodes.extend([(NodeId(4), dn), (NodeId(5), tn), (NodeId(6), ok)]);
            children.push(NodeId(4));
            focus = NodeId(6);
        }
        let mut r = Node::new(Role::Window);
        r.set_label("rsize");
        r.set_children(children);
        nodes.push((root, r));
        TreeUpdate { nodes, tree: Some(TreeInfo::new(root)), tree_id: TreeId::ROOT, focus }
    }
}
