//! Drawing a frame (the page, the menus, the message) and the tree for
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
        self.layout = view::layout(&self.app, full, s, cfg!(not(windows)));
        self.app.page = self.layout.page_rows();
        view::draw(&mut self.gfx, &self.app, &self.layout, &theme, s, self.hot);
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

    pub(super) fn tree(&self) -> accesskit::TreeUpdate {
        use accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
        let root = NodeId(0);
        let mut nodes = Vec::new();
        let mut children = Vec::new();
        let mut focus = root;
        if let (Page::Result, Some(found)) = (view::page(&self.app), &self.app.found) {
            let mut list = Node::new(Role::List);
            list.set_label(tr("Duplicates"));
            let mut rows = Vec::new();
            let end = (self.app.scroll + self.layout.page_rows()).min(self.app.rows.len());
            for (i, row) in self.app.rows.iter().enumerate().take(end).skip(self.app.scroll) {
                let nid = NodeId(100 + i as u64);
                let mut n = Node::new(Role::ListItem);
                n.set_label(match *row {
                    Row::Group(g) => view::group_text(found, g),
                    Row::File(g, f) => found.groups[g].files[f].path.display().to_string(),
                    Row::FolderGroup(g) => view::folder_group_text(&self.app, g),
                    Row::Folder(..) => self.app.path_of(i).map_or(String::new(), |p| p.display().to_string()),
                });
                n.set_selected(self.app.sel == Some(i));
                n.add_action(accesskit::Action::Click);
                if self.app.sel == Some(i) {
                    focus = nid;
                }
                rows.push(nid);
                nodes.push((nid, n));
            }
            list.set_children(rows);
            nodes.push((NodeId(1), list));
            children.push(NodeId(1));
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
        r.set_label("rdup");
        r.set_children(children);
        nodes.push((root, r));
        TreeUpdate { nodes, tree: Some(TreeInfo::new(root)), tree_id: TreeId::ROOT, focus }
    }
}
