//! Drawing a frame and the tree for screen readers.

use super::*;

impl Shell {
    pub(super) fn render(&mut self) {
        let theme = self.theme();
        if self.app.font_dirty {
            self.app.font_dirty = false;
            #[cfg(windows)]
            let file = win::dialogs::font_file(&self.app.settings.font_face);
            #[cfg(not(windows))]
            let file = linux::font_file(&self.app.settings.font_face);
            if !self.gfx.set_mono(file.as_deref()) {
                self.app.notify(i18n::tf("Font \"{}\" could not be loaded; using the default.", &[&self.app.settings.font_face]));
            }
        }
        let Some(win) = &mut self.win else { return };
        let size = win.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if win.surface.resize(w, h).is_err() {
            return;
        }
        let scale = win.window.scale_factor() as f32;
        self.gfx.resize(w.get(), h.get());
        let mouse = self.cursor.unwrap_or((-1.0, -1.0));
        let mut u = ui::Ui::new(&mut self.gfx, theme, scale, &self.input);
        let ime = ui::view::build(&mut self.app, &mut u, mouse);
        self.widgets = std::mem::take(&mut u.widgets);
        drop(u);
        if let Some(m) = self.host.message.borrow_mut().take() {
            self.dialog = Some((m.0, m.1, Rect::default(), Rect::default()));
        }
        if let Some((title, text, panel, ok)) = &mut self.dialog {
            let style = rt_gfx::dialog::DialogStyle { dark: theme.dark, text: theme.text, dim: theme.text_dim, border: theme.border };
            let full = Rect::new(0.0, 0.0, w.get() as f32, h.get() as f32);
            (*panel, *ok) = rt_gfx::dialog::draw_message(&mut self.gfx, full, &style, scale, title, text);
        }
        if let Ok(mut buf) = win.surface.buffer_mut() {
            self.gfx.copy_to(&mut buf);
            let _ = buf.present();
        }
        self.gfx.trim_cache();
        // Title.
        let d = self.app.doc();
        let title = format!("{}{} - rpad", if d.is_modified() { "● " } else { "" }, d.display_name());
        if win.window.title() != title {
            win.window.set_title(&title);
        }
        // IME position.
        if ime != self.ime_rect {
            self.ime_rect = ime;
            if let Some(r) = ime {
                win.window.set_ime_cursor_area(PhysicalPosition::new(r.x as f64, r.y as f64), PhysicalSize::new(r.w as f64, r.h as f64));
            }
        }
        self.sync_menu();
        self.sync_menu_theme(theme.dark);
        if self.update_hot() {
            self.redraw();
        }
        self.update_cursor_icon();
        let tree = self.tree();
        if let Some(win) = &mut self.win {
            win.adapter.update_if_active(|| tree);
        }
    }

    pub(super) fn tree(&self) -> accesskit::TreeUpdate {
        use accesskit::{Node, NodeId, Rect as ARect, Role as ARole, Toggled, TreeId, TreeInfo, TreeUpdate};
        let root = NodeId(0);
        let layer = self.top_layer();
        let mut nodes = Vec::with_capacity(self.widgets.len() + 1);
        let mut children = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for w in &self.widgets {
            if w.layer != layer || w.label.is_empty() || !seen.insert(w.id) {
                continue;
            }
            let role = match w.role {
                Role::Button | Role::Toggle => ARole::Button,
                Role::Tab => ARole::Tab,
                Role::TextField => ARole::TextInput,
                Role::ListItem => ARole::ListItem,
                Role::Label => ARole::Label,
                Role::Editor => ARole::MultilineTextInput,
            };
            let mut n = Node::new(role);
            n.set_label(w.label.clone());
            n.set_bounds(ARect { x0: w.rect.x as f64, y0: w.rect.y as f64, x1: w.rect.right() as f64, y1: w.rect.bottom() as f64 });
            if let Some(v) = &w.value {
                n.set_value(v.clone());
            } else if w.role == Role::Label {
                n.set_value(w.label.clone());
            }
            if w.role != Role::Label {
                n.add_action(accesskit::Action::Focus);
                n.add_action(accesskit::Action::Click);
            }
            if !w.enabled {
                n.set_disabled();
            }
            match (w.role, w.checked) {
                (Role::Toggle, Some(c)) => n.set_toggled(if c { Toggled::True } else { Toggled::False }),
                (Role::Tab | Role::ListItem, Some(c)) => n.set_selected(c),
                _ => {}
            }
            children.push(NodeId(w.id));
            nodes.push((NodeId(w.id), n));
        }
        let mut r = Node::new(ARole::Window);
        r.set_label("rpad");
        r.set_children(children);
        nodes.push((root, r));
        let focus_action = match self.app.focus {
            Focus::Editor => UiAction::Editor,
            f => UiAction::Field(f),
        };
        let fid = ui::wid(&focus_action);
        let focus = if seen.contains(&fid) { NodeId(fid) } else { root };
        TreeUpdate { nodes, tree: Some(TreeInfo::new(root)), tree_id: TreeId::ROOT, focus }
    }
}
