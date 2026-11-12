//! Keyboard and mouse in the window: the list, the treemap and charts, the
//! splitters and scroll bars.

use super::*;

impl Shell {
    pub(super) fn count_click(&mut self, x: f32, y: f32) -> u32 {
        let now = Instant::now();
        let n = match self.last_click {
            Some((t, cx, cy, n)) if now - t < DOUBLE_CLICK && (cx - x).abs() < DOUBLE_CLICK_SLOP && (cy - y).abs() < DOUBLE_CLICK_SLOP => n % 2 + 1,
            _ => 1,
        };
        self.last_click = Some((now, x, y, n));
        n
    }

    pub(super) fn key(&mut self, el: &ActiveEventLoop, event: &winit::event::KeyEvent) {
        if self.dialog.is_some() {
            if Message::closes_on(&event.logical_key) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        #[cfg(not(windows))]
        if self.menu_key(el, event) {
            return;
        }
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        let alt = self.mods.alt_key();
        let logical = if ctrl || alt {
            use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
            event.key_without_modifiers()
        } else {
            event.logical_key.clone()
        };
        let name = match &logical {
            WKey::Character(c) => c.to_lowercase(),
            WKey::Named(n) => match n {
                NamedKey::F1 => "f1".into(),
                NamedKey::F5 => "f5".into(),
                NamedKey::F6 => "f6".into(),
                NamedKey::F7 => "f7".into(),
                NamedKey::F8 => "f8".into(),
                NamedKey::F9 => "f9".into(),
                NamedKey::Delete => "delete".into(),
                NamedKey::Enter => "enter".into(),
                NamedKey::Backspace => "backspace".into(),
                NamedKey::Escape => "esc".into(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        if !name.is_empty() {
            if let Some(cmd) = commands::lookup(ctrl, shift, alt, &name) {
                return self.command(el, cmd);
            }
        }
        if view::page(&self.app) != Page::Result {
            return;
        }
        let page = self.layout.page_rows() as isize;
        match logical {
            WKey::Named(NamedKey::ArrowDown) => self.app.move_sel(1),
            WKey::Named(NamedKey::ArrowUp) => self.app.move_sel(-1),
            WKey::Named(NamedKey::PageDown) => self.app.move_sel(page),
            WKey::Named(NamedKey::PageUp) => self.app.move_sel(-page),
            WKey::Named(NamedKey::Home) => self.app.move_sel(isize::MIN / 2),
            WKey::Named(NamedKey::End) => self.app.move_sel(isize::MAX / 2),
            WKey::Named(NamedKey::ArrowLeft) => self.app.left(),
            WKey::Named(NamedKey::ArrowRight) => self.app.right(),
            WKey::Named(NamedKey::Space) => {
                if let Some(id) = self.app.sel {
                    self.app.toggle(id);
                }
            }
            _ => return,
        }
        self.menu_dirty = true;
        self.redraw();
    }

    /// Updates what the pointer is on.
    pub(super) fn pointer(&mut self, x: f32, y: f32) {
        let l = &self.layout;
        let mut seg = None;
        let (hot, hover) = match view::page(&self.app) {
            Page::Start if !l.body.contains(x, y) => (Hot::None, None),
            Page::Start => {
                let card = l.cards.iter().position(|r| r.contains(x, y));
                (card.map_or(if l.choose.contains(x, y) { Hot::Choose } else { Hot::None }, Hot::Card), None)
            }
            Page::Scanning => (if l.stop.contains(x, y) { Hot::Stop } else { Hot::None }, None),
            Page::Result => {
                if l.splitter.contains(x, y) {
                    (Hot::Splitter, None)
                } else if l.vsplit.contains(x, y) {
                    (Hot::VSplitter, None)
                } else if let Some(&(kind, _)) = l.tabs.iter().find(|(_, r)| r.contains(x, y)) {
                    (Hot::Tab(kind), None)
                } else if let Some(&(mode, _)) = l.modes.iter().find(|(_, r)| r.contains(x, y)) {
                    (Hot::Mode(mode), None)
                } else if let Some(c) = l.head_at(x, y) {
                    (Hot::Head(c), None)
                } else if let Some(c) = l.ext_head_at(x, y) {
                    (Hot::ExtHead(c), None)
                } else if let Some(i) = l.row_at(&self.app, x, y) {
                    (Hot::Row(i), Some(self.app.rows[i].0))
                } else if let Some(i) = l.ext_row_at(&self.app, x, y) {
                    (Hot::ExtRow(i), None)
                } else if let (Some(c), true) = (&self.chart, self.settings.chart_shown() != ChartType::Treemap && l.map.contains(x, y)) {
                    seg = c.hit(x - l.map.x, y - l.map.y);
                    let node = match seg {
                        Some(Hit::Segment(i)) => c.segments[i].node,
                        Some(Hit::Center) => Some(c.root),
                        None => None,
                    };
                    (Hot::None, node)
                } else if let Some((mx, my)) = self.map_point(x, y) {
                    (Hot::None, self.map.as_ref().and_then(|m| m.hit(mx, my)))
                } else {
                    (Hot::None, None)
                }
            }
        };
        let hot = if self.dialog.is_some() { Hot::None } else { hot };
        if hot != self.hot || hover != self.hover || seg != self.hover_seg {
            self.hot = hot;
            self.hover = hover;
            self.hover_seg = seg;
            self.redraw();
        }
        let icon = match (self.drag, hot) {
            (Some(Drag::Splitter), _) | (_, Hot::Splitter) => CursorIcon::NsResize,
            (Some(Drag::VSplitter), _) | (_, Hot::VSplitter) => CursorIcon::EwResize,
            (_, Hot::Card(_) | Hot::Choose | Hot::Stop | Hot::Tab(_) | Hot::Mode(_) | Hot::Head(_) | Hot::ExtHead(_)) => CursorIcon::Pointer,
            _ if seg == Some(Hit::Center) => CursorIcon::Pointer,
            _ => CursorIcon::Default,
        };
        if icon != self.cursor_icon {
            self.cursor_icon = icon;
            if let Some(w) = &self.win {
                w.window.set_cursor(icon);
            }
        }
    }

    pub(super) fn press(&mut self, el: &ActiveEventLoop, x: f32, y: f32) {
        if let Some(d) = &self.dialog {
            if d.closes_at(x, y) {
                self.dialog = None;
                self.redraw();
            }
            return;
        }
        #[cfg(not(windows))]
        if self.menu_press(el, x, y) {
            return;
        }
        let clicks = self.count_click(x, y);
        match self.hot {
            Hot::Card(i) => {
                if let Some(p) = self.app.volumes.as_ref().and_then(|v| v.get(i)).map(|v| v.path.clone()) {
                    self.start_scan(&p);
                }
            }
            Hot::Choose => self.command(el, Command::ChooseFolder),
            Hot::Stop => self.command(el, Command::Stop),
            Hot::Splitter => self.drag = Some(Drag::Splitter),
            Hot::VSplitter => self.drag = Some(Drag::VSplitter),
            Hot::Tab(kind) => self.command(el, Command::Chart(kind)),
            Hot::Mode(mode) => self.command(el, Command::View(mode)),
            Hot::Head(c) => self.app.sort_by(c),
            Hot::ExtHead(c) => self.app.sort_exts_by(c),
            Hot::ExtRow(i) => {
                let ext = self.app.exts.get(i).map(|e| e.0);
                self.app.ext_sel = if self.app.ext_sel == ext { None } else { ext };
            }
            Hot::Row(i) => {
                let (id, depth) = self.app.rows[i];
                if self.layout.on_arrow(x, depth) || clicks == 2 {
                    self.app.toggle(id);
                }
                self.app.select(id);
            }
            Hot::None => {
                let l = &self.layout;
                if l.track.contains(x, y) && self.app.rows.len() > l.page_rows() {
                    self.drag = Some(Drag::Thumb);
                    self.drag_to(x, y);
                } else if l.ext_track.contains(x, y) && self.app.exts.len() > l.ext_rows() {
                    self.drag = Some(Drag::ExtThumb);
                    self.drag_to(x, y);
                } else if self.settings.chart_shown() != ChartType::Treemap && l.map.contains(x, y) {
                    self.chart_press(clicks);
                } else if let Some((mx, my)) = self.map_point(x, y) {
                    if let Some(id) = self.map.as_ref().and_then(|m| m.hit(mx, my)) {
                        self.app.select(id);
                        if clicks == 2 {
                            let t = &self.app.scan.as_ref().expect("a treemap has a tree").tree;
                            // A folder's header shows that folder; a file, one folder down toward it.
                            if t.node(id).is_dir() && !t.children(id).is_empty() && id != self.app.zoom {
                                self.app.zoom = id;
                            } else {
                                self.zoom_toward(id);
                            }
                        }
                    }
                }
            }
        }
        self.menu_dirty = true;
        self.redraw();
    }

    /// A click on the pie or the rings selects; a double click on a folder
    /// shows it; a click in the middle of the rings goes up.
    pub(super) fn chart_press(&mut self, clicks: u32) {
        match self.hover_seg {
            Some(Hit::Center) => self.app.zoom_out(),
            Some(Hit::Segment(i)) => {
                let Some(id) = self.chart.as_ref().and_then(|c| c.segments.get(i)).and_then(|s| s.node) else { return };
                self.app.select(id);
                let t = &self.app.scan.as_ref().expect("a chart has a tree").tree;
                if clicks == 2 && t.node(id).is_dir() && !t.children(id).is_empty() {
                    self.app.zoom = id;
                }
            }
            None => {}
        }
    }

    /// A double click in the treemap shows the folder one level down
    /// toward `id`.
    pub(super) fn zoom_toward(&mut self, id: Id) {
        let Some(t) = self.app.scan.as_ref().map(|s| &s.tree) else { return };
        let mut cur = id;
        while let Some(p) = t.parent(cur) {
            if p == self.app.zoom {
                if t.node(cur).is_dir() && !t.children(cur).is_empty() {
                    self.app.zoom = cur;
                    self.app.select(cur);
                }
                return;
            }
            cur = p;
        }
    }

    pub(super) fn drag_to(&mut self, x: f32, y: f32) {
        match self.drag {
            Some(Drag::Splitter) => {
                // A share of the height below the block on top.
                let (top, h) = (self.layout.info.bottom(), self.layout.body.bottom() - self.layout.info.bottom());
                if h > 0.0 {
                    self.settings.panes_mut().split = ((y - top) / h).clamp(SPLIT_MIN, SPLIT_MAX);
                }
            }
            Some(Drag::VSplitter) => {
                let body = self.layout.body;
                if body.w > 0.0 {
                    self.settings.panes_mut().split_x = ((x - body.x) / body.w).clamp(SPLIT_X_MIN, SPLIT_X_MAX);
                }
            }
            Some(Drag::Thumb) => {
                let page = self.layout.page_rows();
                self.app.scroll = view::scroll_for(self.layout.track, self.app.rows.len(), page, y);
            }
            Some(Drag::ExtThumb) => {
                let page = self.layout.ext_rows();
                self.app.types_scroll = view::scroll_for(self.layout.ext_track, self.app.exts.len(), page, y);
            }
            None => return,
        }
        self.redraw();
    }
}
