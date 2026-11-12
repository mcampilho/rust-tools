//! Drawing a frame and the tree for screen readers.

use super::*;

impl Shell {
    pub(super) fn render(&mut self) {
        let dark = self.dark();
        let theme = ui::theme::Theme::new(dark);
        let s = self.scale();
        let Some(win) = &mut self.win else { return };
        let size = win.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if win.surface.resize(w, h).is_err() {
            return;
        }
        self.gfx.resize(w.get(), h.get());
        let full = Rect::new(0.0, 0.0, w.get() as f32, h.get() as f32);
        let mut area = full;
        #[cfg(not(windows))]
        let bar = area.take_top((28.0 * s).round());
        let status = area.take_bottom((24.0 * s).round());
        self.viewer.dark_pages = self.settings.dark_pages;
        self.viewer.draw(&mut self.gfx, area, &theme, s, &self.media);
        // Status bar.
        let g = &mut self.gfx;
        g.fill_rect(status, theme.status);
        g.fill_rect(Rect::new(status.x, status.y, status.w, 1.0), theme.border);
        let px = 12.0 * s;
        let pad = 10.0 * s;
        let right = self.viewer.status().join("    ");
        let rw = g.measure(&right, px, FontId::Regular);
        g.text_in(&right, Rect::new(status.right() - rw - pad, status.y, rw, status.h), px, FontId::Regular, theme.dim, Align::Left);
        let left = self.viewer.hover_link.clone().or_else(|| self.status_note.clone()).or_else(|| self.viewer.path.as_ref().map(|p| p.display().to_string())).unwrap_or_default();
        let lw = (status.w - rw - 3.0 * pad).max(0.0);
        let left = rt_gfx::dialog::ellipsize(g, &left, lw, px, FontId::Regular);
        g.text_in(&left, Rect::new(status.x + pad, status.y, lw, status.h), px, FontId::Regular, theme.text, Align::Left);
        #[cfg(not(windows))]
        {
            self.menu.draw_bar(g, bar, &theme.menu_style(), s);
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

    pub(super) fn tree(&self) -> accesskit::TreeUpdate {
        use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
        let root = NodeId(0);
        let docid = NodeId(1);
        let statusid = NodeId(2);
        let mut d = Node::new(Role::Document);
        d.set_label(self.viewer.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "rview".into()));
        d.set_value(self.viewer.accessible_text());
        d.add_action(accesskit::Action::Focus);
        let mut st = Node::new(Role::Label);
        st.set_value(self.viewer.status().join(", "));
        let mut r = Node::new(Role::Window);
        r.set_label("rview");
        let mut nodes = vec![(docid, d), (statusid, st)];
        let mut children = vec![docid, statusid];
        let mut focus = docid;
        if let Some(dl) = &self.dialog {
            let dlgid = NodeId(3);
            let textid = NodeId(4);
            let okid = NodeId(5);
            let mut dn = Node::new(Role::Dialog);
            dn.set_label(dl.title.clone());
            dn.set_children(vec![textid, okid]);
            let mut tn = Node::new(Role::Label);
            tn.set_value(dl.text.replace('\t', "  "));
            let mut ok = Node::new(Role::Button);
            ok.set_label("OK");
            ok.add_action(accesskit::Action::Click);
            nodes.extend([(dlgid, dn), (textid, tn), (okid, ok)]);
            children.push(dlgid);
            focus = okid;
        }
        r.set_children(children);
        nodes.push((root, r));
        TreeUpdate { nodes, tree: Some(TreeInfo::new(root)), tree_id: TreeId::ROOT, focus }
    }
}
