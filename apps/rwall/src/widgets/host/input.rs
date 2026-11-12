//! Mouse and keyboard on the widgets: moving and resizing, writing in notes
//! and annotations, the calendars' arrows, the fences' files, the wheel, and
//! the right-click menu.

use super::*;

impl Host2<'_> {
    pub(super) fn events(&mut self) {
        loop {
            let Some((h, ev)) = QUEUE.with(|q| q.borrow_mut().pop_front()) else { return };
            let Some(i) = self.find(h) else { continue };
            match ev {
                Ev::Down(x, y, dbl) => self.down(i, x as f32, y as f32, dbl),
                Ev::Move(x, y) => self.moved(i, x as f32, y as f32),
                Ev::Up => self.up(i),
                Ev::Menu(x, y) => self.menu(i, x as f32, y as f32),
                Ev::Leave => {
                    let w = &mut self.ws[i];
                    w.hover = false;
                    w.hot = None;
                    w.dirty = true;
                }
                Ev::Wheel(d) => self.wheel(i, d),
                Ev::Char(c) => self.typed(i, c),
                Ev::Key(k) => self.key_down(i, k),
                Ev::Blur => self.stop_editing(i),
                Ev::Dpi => self.ws[i].dirty = true,
            }
        }
    }

    pub(super) fn start_drag(&mut self, i: usize, kind: DragKind) {
        let w = &mut self.ws[i];
        w.drag = Some(Drag { kind, start: cursor(), rect: (w.cfg.x, w.cfg.y, w.cfg.w, w.cfg.h), moved: false });
        // SAFETY: a window of this thread.
        unsafe { SetCapture(w.hwnd) };
    }

    /// The caret position under (`x`, `y`).
    pub(super) fn text_index(&mut self, i: usize, x: f32, y: f32) -> Option<usize> {
        let w = &self.ws[i];
        let (e, tb) = (w.editor.as_ref()?, w.painted.text.as_ref()?);
        let line = *tb.lines.get(tb.line_at(y))?;
        Some(e.index_at(&mut self.gfx, &line, x - tb.rect.x, tb.px, tb.font?))
    }

    pub(super) fn start_editing(&mut self, i: usize) {
        let w = &mut self.ws[i];
        if w.editor.is_none() {
            w.editor = Some(Editor::new(&w.cfg.text));
            w.caret_on = true;
            self.next_blink = Instant::now() + Duration::from_millis(530);
        }
        // SAFETY: a window of this thread; the click made rwall the
        // foreground program.
        unsafe {
            let _ = SetForegroundWindow(w.hwnd);
            let _ = SetFocus(Some(w.hwnd));
        }
        w.dirty = true;
        // Lay the text out for the caret.
        self.render(i);
    }

    /// Keeps what was written.
    pub(super) fn commit_text(&mut self, i: usize) {
        let w = &mut self.ws[i];
        if let Some(e) = &w.editor {
            if e.text != w.cfg.text || w.text_due.is_some() {
                w.cfg.text = e.text.clone();
                w.text_due = None;
                let cfg = w.cfg.clone();
                self.send(Event::Changed(cfg));
            }
        }
    }

    pub(super) fn stop_editing(&mut self, i: usize) {
        if self.ws[i].editor.is_none() {
            return;
        }
        self.commit_text(i);
        let w = &mut self.ws[i];
        w.editor = None;
        w.scroll = 0.0;
        w.dirty = true;
    }

    pub(super) fn down(&mut self, i: usize, x: f32, y: f32, dbl: bool) {
        let w = &self.ws[i];
        let zone = w.painted.zone(x, y).unwrap_or(Zone::Move);
        let (kind, locked) = (w.cfg.kind, w.cfg.locked);
        match zone {
            Zone::Resize if !locked => self.start_drag(i, DragKind::Resize),
            Zone::Prev | Zone::Next | Zone::Today => {
                let w = &mut self.ws[i];
                w.month = match zone {
                    Zone::Prev => crate::widgets::calendar::add_months(w.month.0, w.month.1, -1),
                    Zone::Next => crate::widgets::calendar::add_months(w.month.0, w.month.1, 1),
                    _ => {
                        let t = rt_sys::local_time();
                        (t.year, t.month)
                    }
                };
                w.dirty = true;
            }
            Zone::Text => {
                let editing = self.ws[i].editor.is_some();
                if !editing {
                    self.start_editing(i);
                }
                if let Some(at) = self.text_index(i, x, y) {
                    let shift = shift_down();
                    if let Some(e) = self.ws[i].editor.as_mut() {
                        if dbl {
                            e.select_word(at);
                        } else {
                            e.move_to(at, shift);
                            e.goal_x = None;
                        }
                    }
                }
                self.ws[i].caret_on = true;
                self.ws[i].dirty = true;
                if !dbl {
                    self.start_drag(i, DragKind::Select);
                }
            }
            Zone::Item(n) => {
                self.ws[i].selected = Some(n);
                self.ws[i].dirty = true;
                if dbl {
                    if let Some(e) = self.ws[i].entries.get(n) {
                        open(&e.path);
                    }
                } else {
                    self.start_drag(i, DragKind::Item(n));
                }
            }
            _ => {
                if dbl {
                    match kind {
                        WKind::Annotation => {
                            self.start_editing(i);
                            return;
                        }
                        WKind::Photos if !self.ws[i].photo_path.as_os_str().is_empty() => {
                            open(&self.ws[i].photo_path.clone());
                            return;
                        }
                        _ => {}
                    }
                }
                if zone == Zone::Blank && self.ws[i].selected.take().is_some() {
                    self.ws[i].dirty = true;
                }
                if !locked {
                    self.start_drag(i, DragKind::Move);
                }
            }
        }
    }

    pub(super) fn moved(&mut self, i: usize, x: f32, y: f32) {
        let w = &mut self.ws[i];
        if !w.hover {
            w.hover = true;
            w.dirty = true;
            let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: w.hwnd, dwHoverTime: 0 };
            // SAFETY: a window of this thread.
            let _ = unsafe { TrackMouseEvent(&mut t) };
        }
        let Some(d) = w.drag.as_mut() else {
            let zone = w.painted.zone(x, y).filter(|z| *z != Zone::Resize || !w.cfg.locked);
            if zone != w.hot {
                if matches!(zone, Some(Zone::Prev | Zone::Next | Zone::Today | Zone::Item(_) | Zone::Day(_))) || matches!(w.hot, Some(Zone::Prev | Zone::Next | Zone::Today | Zone::Item(_) | Zone::Day(_))) {
                    w.dirty = true;
                }
                w.hot = zone;
            }
            let p = match zone {
                Some(Zone::Resize) => Pointer::Resize,
                Some(Zone::Text) => Pointer::Text,
                Some(Zone::Prev | Zone::Next | Zone::Today) => Pointer::Hand,
                _ => Pointer::Arrow,
            };
            w.set_pointer(p);
            return;
        };
        let c = cursor();
        let (dx, dy) = (c.x - d.start.x, c.y - d.start.y);
        if !d.moved && dx.abs() + dy.abs() < 4 {
            return;
        }
        d.moved = true;
        let (rx, ry, rw, rh) = d.rect;
        let kind = d.kind;
        match kind {
            DragKind::Move => {
                let (mut nx, mut ny) = (rx + dx, ry + dy);
                // Snaps to the edges of the screen's work area.
                let wa = work_area(POINT { x: nx + rw / 2, y: ny + rh / 2 });
                let snap = 12;
                if (nx - wa.left).abs() < snap {
                    nx = wa.left;
                } else if (nx + rw - wa.right).abs() < snap {
                    nx = wa.right - rw;
                }
                if (ny - wa.top).abs() < snap {
                    ny = wa.top;
                } else if (ny + rh - wa.bottom).abs() < snap {
                    ny = wa.bottom - rh;
                }
                w.cfg.x = nx;
                w.cfg.y = ny;
                w.set_pointer(Pointer::Moving);
                // SAFETY: a window of this thread.
                unsafe {
                    let _ = SetWindowPos(w.hwnd, None, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                }
            }
            DragKind::Resize => {
                let s = dpi_scale(w.hwnd);
                let (mw, mh) = w.cfg.kind.min_size();
                let (mw, mh) = ((mw as f32 * s) as i32, (mh as f32 * s) as i32);
                let (mut nw, mut nh) = ((rw + dx).max(mw), (rh + dy).max(mh));
                if w.cfg.kind.square() {
                    nw = nw.max(nh);
                    nh = nw;
                }
                let limit = (2000.0 * s) as i32;
                w.cfg.w = nw.min(limit);
                w.cfg.h = nh.min(limit);
                w.dirty = true;
                self.render(i);
            }
            DragKind::Select => {
                if let Some(at) = self.text_index(i, x, y) {
                    if let Some(e) = self.ws[i].editor.as_mut() {
                        e.move_to(at, true);
                    }
                    self.ws[i].dirty = true;
                }
            }
            DragKind::Item(n) => {
                w.drag = None;
                if let Some(path) = w.entries.get(n).map(|e| e.path.clone()) {
                    drag_out(&path);
                }
            }
        }
    }

    pub(super) fn up(&mut self, i: usize) {
        let Some(d) = self.ws[i].drag.take() else { return };
        // SAFETY: releases this thread's capture.
        let _ = unsafe { ReleaseCapture() };
        self.ws[i].set_pointer(Pointer::Arrow);
        if d.moved && matches!(d.kind, DragKind::Move | DragKind::Resize) {
            self.send(Event::Changed(self.ws[i].cfg.clone()));
        }
    }

    pub(super) fn wheel(&mut self, i: usize, delta: i32) {
        let s = dpi_scale(self.ws[i].hwnd);
        let w = &mut self.ws[i];
        let step = match w.cfg.kind {
            WKind::Fence => CELL.1 * s * 0.5,
            WKind::Note | WKind::Annotation => 20.0 * s,
            _ => return,
        };
        let max = (w.painted.content_h - w.painted.room_h).max(0.0);
        let new = (w.scroll - delta as f32 / 120.0 * step * 1.5).clamp(0.0, max);
        if (new - w.scroll).abs() > 0.1 {
            w.scroll = new;
            w.dirty = true;
        }
    }

    pub(super) fn edited(&mut self, i: usize, changed: bool) {
        let w = &mut self.ws[i];
        w.caret_on = true;
        self.next_blink = Instant::now() + Duration::from_millis(530);
        w.dirty = true;
        if changed {
            w.text_due = Some(Instant::now() + Duration::from_secs(1));
        }
    }

    pub(super) fn typed(&mut self, i: usize, c: u16) {
        let w = &mut self.ws[i];
        let Some(e) = w.editor.as_mut() else { return };
        let text = if (0xD800..0xDC00).contains(&c) {
            w.surrogate = Some(c);
            return;
        } else if (0xDC00..0xE000).contains(&c) {
            match w.surrogate.take() {
                Some(hi) => String::from_utf16_lossy(&[hi, c]),
                None => return,
            }
        } else if c == 13 {
            "\n".to_string()
        } else if c < 32 || c == 127 {
            return;
        } else {
            String::from_utf16_lossy(&[c])
        };
        e.insert(&text);
        self.edited(i, true);
    }

    pub(super) fn key_down(&mut self, i: usize, k: u16) {
        let (ctrl, shift) = (ctrl_down(), shift_down());
        let vk = VIRTUAL_KEY(k);
        if self.ws[i].editor.is_none() {
            return;
        }
        if matches!(vk, VK_UP | VK_DOWN | VK_HOME | VK_END | VK_PRIOR | VK_NEXT) {
            self.line_move(i, vk, shift, ctrl);
            self.edited(i, false);
            return;
        }
        if vk == VK_ESCAPE {
            self.stop_editing(i);
            return;
        }
        let Some(e) = self.ws[i].editor.as_mut() else { return };
        let changed = match vk {
            VK_BACK => {
                e.backspace(ctrl);
                true
            }
            VK_DELETE => {
                e.delete(ctrl);
                true
            }
            VK_LEFT => {
                e.left(shift, ctrl);
                false
            }
            VK_RIGHT => {
                e.right(shift, ctrl);
                false
            }
            _ if ctrl && k == u16::from(b'A') => {
                e.select_all();
                false
            }
            _ if ctrl && (k == u16::from(b'C') || k == u16::from(b'X')) => {
                if let Some(t) = e.selected_text() {
                    rt_sys::set_clipboard_text(&t.replace('\n', "\r\n"));
                }
                if k == u16::from(b'X') {
                    e.backspace(false);
                    true
                } else {
                    false
                }
            }
            _ if ctrl && k == u16::from(b'V') => {
                if let Some(t) = rt_sys::clipboard_text() {
                    e.insert(&t.replace("\r\n", "\n"));
                }
                true
            }
            _ => return,
        };
        self.edited(i, changed);
    }

    /// Up, Down, Home, End, Page Up and Page Down, along the lines as drawn.
    pub(super) fn line_move(&mut self, i: usize, vk: VIRTUAL_KEY, shift: bool, ctrl: bool) {
        let w = &self.ws[i];
        let (Some(e), Some(tb)) = (w.editor.as_ref(), w.painted.text.as_ref()) else { return };
        let Some(font) = tb.font else { return };
        if tb.lines.is_empty() {
            return;
        }
        let n = line_of(&tb.lines, e.caret);
        let line = tb.lines[n];
        let goal = e.goal_x.unwrap_or_else(|| self.gfx.measure(&e.text[line.start..e.caret.clamp(line.start, line.end)], tb.px, font));
        let page = ((tb.rect.h / tb.line_h).floor() as usize).max(1);
        let target = match vk {
            VK_HOME if ctrl => Some(0),
            VK_END if ctrl => Some(e.text.len()),
            VK_HOME => Some(line.start),
            VK_END => Some(line.end),
            _ => None,
        };
        let (lines, px) = (tb.lines.clone(), tb.px);
        let at = match target {
            Some(t) => t,
            None => {
                let m = match vk {
                    VK_UP => n.checked_sub(1),
                    VK_DOWN => (n + 1 < lines.len()).then_some(n + 1),
                    VK_PRIOR => Some(n.saturating_sub(page)),
                    _ => Some((n + page).min(lines.len() - 1)),
                };
                match m {
                    Some(m) => {
                        let e = self.ws[i].editor.as_ref().expect("editing");
                        e.index_at(&mut self.gfx, &lines[m], goal, px, font)
                    }
                    None if vk == VK_UP => 0,
                    None => self.ws[i].editor.as_ref().map_or(0, |e| e.text.len()),
                }
            }
        };
        if let Some(e) = self.ws[i].editor.as_mut() {
            e.move_to(at, shift);
            e.goal_x = if matches!(vk, VK_UP | VK_DOWN | VK_PRIOR | VK_NEXT) { Some(goal) } else { None };
        }
    }

    pub(super) fn menu(&mut self, i: usize, x: f32, y: f32) {
        const NEW_NOTE: u32 = 1;
        const NOTE_COLOR: u32 = 10;
        const TEXT_COLOR: u32 = 20;
        const TEXT_SIZE: u32 = 30;
        const EDIT: u32 = 40;
        const SECONDS: u32 = 41;
        const H24: u32 = 42;
        const REFRESH: u32 = 43;
        const FAHRENHEIT: u32 = 44;
        const OPEN_FOLDER: u32 = 45;
        const NEXT_PHOTO: u32 = 46;
        const SHOW_PHOTO: u32 = 47;
        const OPEN_ITEM: u32 = 48;
        const SHOW_ITEM: u32 = 49;
        const ON_TOP: u32 = 50;
        const LOCK: u32 = 51;
        const SETTINGS: u32 = 52;
        const REMOVE: u32 = 53;

        let item = match self.ws[i].painted.zone(x, y) {
            Some(Zone::Item(n)) => {
                self.ws[i].selected = Some(n);
                self.ws[i].dirty = true;
                self.render(i);
                Some(n)
            }
            _ => None,
        };
        let cfg = self.ws[i].cfg.clone();
        // SAFETY: the menus are made, shown and destroyed here; the texts
        // outlive the calls that copy them.
        let chosen = unsafe {
            let Ok(m) = CreatePopupMenu() else { return };
            let add = |m: HMENU, id: u32, text: &str, checked: bool| {
                let flags = if checked { MF_STRING | MF_CHECKED } else { MF_STRING };
                let _ = AppendMenuW(m, flags, id as usize, &HSTRING::from(text));
            };
            let sep = |m: HMENU| {
                let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
            };
            let sub = |m: HMENU, text: &str, items: &[(u32, String, bool)]| {
                if let Ok(s) = CreatePopupMenu() {
                    for (id, t, c) in items {
                        add(s, *id, t, *c);
                    }
                    let _ = AppendMenuW(m, MF_POPUP, s.0 as usize, &HSTRING::from(text));
                }
            };
            if item.is_some() {
                add(m, OPEN_ITEM, &tr("Open"), false);
                add(m, SHOW_ITEM, &tr("Show the file"), false);
                sep(m);
            }
            match cfg.kind {
                WKind::Note => {
                    add(m, NEW_NOTE, &tr("New note"), false);
                    let colors: Vec<(u32, String, bool)> = NOTE_COLORS.iter().enumerate().map(|(k, (n, _))| (NOTE_COLOR + k as u32, tr(n), cfg.color as usize == k)).collect();
                    sub(m, &tr("Color"), &colors);
                    sep(m);
                }
                WKind::Annotation => {
                    add(m, EDIT, &tr("Edit the text"), false);
                    let sizes: Vec<(u32, String, bool)> = TEXT_SIZES.iter().enumerate().map(|(k, (n, _))| (TEXT_SIZE + k as u32, tr(n), cfg.size as usize == k)).collect();
                    sub(m, &tr("Size"), &sizes);
                    let colors: Vec<(u32, String, bool)> = TEXT_COLORS.iter().enumerate().map(|(k, (n, _))| (TEXT_COLOR + k as u32, tr(n), cfg.color as usize == k)).collect();
                    sub(m, &tr("Color"), &colors);
                    sep(m);
                }
                WKind::Analog | WKind::Digital | WKind::ClockCalendar => {
                    add(m, SECONDS, &tr("Seconds"), cfg.seconds);
                    if cfg.kind != WKind::Analog {
                        add(m, H24, &tr("24-hour clock"), cfg.h24);
                    }
                    sep(m);
                }
                WKind::Weather => {
                    add(m, REFRESH, &tr("Refresh"), false);
                    add(m, FAHRENHEIT, &tr("Fahrenheit (°F)"), cfg.fahrenheit);
                    sep(m);
                }
                WKind::Fence if !cfg.folder.as_os_str().is_empty() => {
                    add(m, OPEN_FOLDER, &tr("Open the folder"), false);
                    sep(m);
                }
                WKind::Photos if !cfg.folder.as_os_str().is_empty() => {
                    add(m, NEXT_PHOTO, &tr("Next photo"), false);
                    if !self.ws[i].photo_path.as_os_str().is_empty() {
                        add(m, SHOW_PHOTO, &tr("Show the file"), false);
                    }
                    add(m, OPEN_FOLDER, &tr("Open the folder"), false);
                    sep(m);
                }
                _ => {}
            }
            add(m, ON_TOP, &tr("Always on top"), cfg.on_top);
            add(m, LOCK, &tr("Lock position"), cfg.locked);
            sep(m);
            add(m, SETTINGS, &tr("Widget settings…"), false);
            add(m, REMOVE, &tr("Remove the widget"), false);
            let p = cursor();
            let _ = SetForegroundWindow(self.control);
            let r = TrackPopupMenuEx(m, (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY).0, p.x, p.y, self.control, None);
            let _ = PostMessageW(Some(self.control), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(m);
            r.0 as u32
        };
        let s = dpi_scale(self.ws[i].hwnd);
        let w = &mut self.ws[i];
        let mut changed = true;
        match chosen {
            0 => changed = false,
            NEW_NOTE => {
                changed = false;
                let (dw, dh) = WKind::Note.default_size();
                let off = (30.0 * s) as i32;
                (self.report)(Event::NewNote { x: w.cfg.x + off, y: w.cfg.y + off, w: (dw as f32 * s) as i32, h: (dh as f32 * s) as i32 });
            }
            c if (NOTE_COLOR..NOTE_COLOR + NOTE_COLORS.len() as u32).contains(&c) => w.cfg.color = (c - NOTE_COLOR) as u8,
            c if (TEXT_COLOR..TEXT_COLOR + TEXT_COLORS.len() as u32).contains(&c) => w.cfg.color = (c - TEXT_COLOR) as u8,
            c if (TEXT_SIZE..TEXT_SIZE + TEXT_SIZES.len() as u32).contains(&c) => w.cfg.size = (c - TEXT_SIZE) as u8,
            EDIT => {
                changed = false;
                self.start_editing(i);
            }
            SECONDS => w.cfg.seconds = !w.cfg.seconds,
            H24 => w.cfg.h24 = !w.cfg.h24,
            REFRESH => {
                changed = false;
                w.next_weather = Instant::now();
            }
            FAHRENHEIT => {
                w.cfg.fahrenheit = !w.cfg.fahrenheit;
                w.forecast = None;
                w.next_weather = Instant::now();
            }
            OPEN_FOLDER => {
                changed = false;
                open(&w.cfg.folder.clone());
            }
            NEXT_PHOTO => {
                changed = false;
                w.next_photo = Instant::now();
            }
            SHOW_PHOTO => {
                changed = false;
                rt_sys::reveal(&w.photo_path);
            }
            OPEN_ITEM | SHOW_ITEM => {
                changed = false;
                if let Some(e) = item.and_then(|n| w.entries.get(n)) {
                    if chosen == OPEN_ITEM {
                        open(&e.path);
                    } else {
                        rt_sys::reveal(&e.path);
                    }
                }
            }
            ON_TOP => {
                w.cfg.on_top = !w.cfg.on_top;
                self.commit_text(i);
                self.destroy(i);
                self.create(i);
            }
            LOCK => w.cfg.locked = !w.cfg.locked,
            SETTINGS => {
                changed = false;
                self.send(Event::Settings);
            }
            REMOVE => {
                changed = false;
                self.commit_text(i);
                let w = &self.ws[i];
                let ask = w.cfg.kind.editable() && !w.cfg.text.trim().is_empty();
                // SAFETY: a plain message box.
                let yes = !ask || unsafe { MessageBoxW(Some(w.hwnd), &HSTRING::from(tr("Remove this widget? What is written in it is lost.")), w!("rwall"), MB_YESNO | MB_ICONQUESTION) } == IDYES;
                if yes {
                    self.send(Event::Remove(w.cfg.id));
                }
            }
            _ => changed = false,
        }
        if changed {
            self.ws[i].dirty = true;
            self.send(Event::Changed(self.ws[i].cfg.clone()));
        }
    }
}
