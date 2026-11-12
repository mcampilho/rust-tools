//! What happens with time: the clocks, the caret, the weather and photos
//! when due, the fences' folders; and painting the widgets that changed.

use super::*;

impl Host2<'_> {
    pub(super) fn tick(&mut self) {
        let now = Instant::now();
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        if secs != self.last_second {
            self.last_second = secs;
            let minute = secs / 60;
            let new_minute = minute != self.last_minute;
            self.last_minute = minute;
            for w in &mut self.ws {
                let k = w.cfg.kind;
                if (k.has_clock() && (w.cfg.seconds || new_minute)) || (new_minute && k == WKind::Calendar) {
                    w.dirty = true;
                }
            }
        }
        if now >= self.next_blink {
            self.next_blink = now + Duration::from_millis(530);
            for w in self.ws.iter_mut().filter(|w| w.editor.is_some()) {
                w.caret_on = !w.caret_on;
                w.dirty = true;
            }
        }
        for i in 0..self.ws.len() {
            if self.ws[i].text_due.is_some_and(|t| now >= t) {
                self.ws[i].text_due = None;
                if let Some(e) = &self.ws[i].editor {
                    self.ws[i].cfg.text = e.text.clone();
                }
                self.send(Event::Changed(self.ws[i].cfg.clone()));
            }
            match self.ws[i].cfg.kind {
                WKind::Weather => self.weather_due(i, now),
                WKind::Photos => self.photos_due(i, now),
                _ => {}
            }
        }
        if RESET.with(|c| c.replace(false)) {
            self.owner = find_owner(true);
            self.recreate_pinned();
        }
        if LAYOUT.with(|c| c.replace(false)) {
            for i in 0..self.ws.len() {
                if self.keep_on_screen(i) {
                    self.send(Event::Changed(self.ws[i].cfg.clone()));
                }
                self.ws[i].dirty = true;
            }
        }
        if now >= self.next_check {
            self.next_check = now + Duration::from_secs(2);
            // The icons may have moved to another window (Explorer, or the
            // dynamic wallpaper making its layer).
            let owner = find_owner(false);
            if owner.is_some() && owner != self.owner {
                self.owner = owner;
                self.recreate_pinned();
            }
            for i in 0..self.ws.len() {
                // SAFETY: plain query.
                if !unsafe { IsWindow(Some(self.ws[i].hwnd)) }.as_bool() {
                    self.destroy(i);
                    self.create(i);
                }
                if self.ws[i].cfg.kind == WKind::Fence {
                    self.scan_fence(i);
                }
            }
        }
    }

    pub(super) fn recreate_pinned(&mut self) {
        for i in 0..self.ws.len() {
            if !self.ws[i].cfg.on_top {
                self.commit_text(i);
                self.destroy(i);
                self.create(i);
            }
        }
    }

    pub(super) fn weather_due(&mut self, i: usize, now: Instant) {
        let w = &mut self.ws[i];
        let city = w.cfg.city.trim().to_string();
        if city.is_empty() || w.asking || now < w.next_weather {
            return;
        }
        w.asking = true;
        if w.forecast.is_none() {
            w.status = Some(tr("Looking for the weather…"));
            w.dirty = true;
        }
        let (id, known, lat, lon, f) = (w.cfg.id, w.cfg.has_place(), w.cfg.lat, w.cfg.lon, w.cfg.fahrenheit);
        let lang = self.style.lang.tag().split('-').next().unwrap_or("en").to_string();
        let (tx, waker) = (self.done_tx.clone(), self.waker);
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                let place = if known {
                    None
                } else {
                    match weather::find(&city, &lang) {
                        Ok(Some(p)) => Some(p),
                        Ok(None) => return Err(WeatherError::NotFound),
                        Err(e) => return Err(WeatherError::Net(e)),
                    }
                };
                let (lat, lon) = place.as_ref().map_or((lat, lon), |p| (p.lat, p.lon));
                weather::fetch(lat, lon, f).map(|fc| (place, fc)).map_err(WeatherError::Net)
            })
            .unwrap_or_else(|_| Err(WeatherError::Net("unexpected answer".into())));
            let _ = tx.send(Done::Weather(id, city, result));
            waker.wake();
        });
    }

    pub(super) fn photos_due(&mut self, i: usize, now: Instant) {
        let folder = self.ws[i].cfg.folder.clone();
        if folder.as_os_str().is_empty() || self.ws[i].loading {
            return;
        }
        let (tx, waker, id) = (self.done_tx.clone(), self.waker, self.ws[i].cfg.id);
        if now >= self.ws[i].next_list {
            let w = &mut self.ws[i];
            w.loading = true;
            w.next_list = now + Duration::from_secs(30 * 60);
            std::thread::spawn(move || {
                let list = crate::sources::folders::list_with(std::slice::from_ref(&folder), true, crate::picture::has_picture_ext);
                let _ = tx.send(Done::Photos(id, folder, list));
                waker.wake();
            });
            return;
        }
        if now < self.ws[i].next_photo || self.ws[i].photos.is_empty() {
            return;
        }
        let n = self.ws[i].photos.len();
        let mut k = (self.random() % n as u64) as usize;
        if n > 1 && self.ws[i].photos[k] == self.ws[i].photo_path {
            k = (k + 1) % n;
        }
        let w = &mut self.ws[i];
        let path = w.photos[k].clone();
        w.loading = true;
        w.next_photo = now + Duration::from_secs(u64::from(w.cfg.minutes.max(1)) * 60);
        let (pw, ph) = (((w.cfg.w * 2) as u32).clamp(64, 1600), ((w.cfg.h * 2) as u32).clamp(64, 1600));
        std::thread::spawn(move || {
            let pix = std::panic::catch_unwind(|| crate::picture::thumbnail(&path, pw, ph)).ok().flatten();
            let _ = tx.send(Done::Photo(id, path, pix));
            waker.wake();
        });
    }

    /// Reads a fence's folder again; asks for the icons it has not got.
    pub(super) fn scan_fence(&mut self, i: usize) {
        let folder = self.ws[i].cfg.folder.clone();
        if folder.as_os_str().is_empty() {
            return;
        }
        let Ok(rd) = std::fs::read_dir(&folder) else {
            let msg = tr("The folder cannot be read.");
            let w = &mut self.ws[i];
            if w.status.as_deref() != Some(msg.as_str()) {
                w.status = Some(msg);
                w.entries.clear();
                w.items.clear();
                w.dirty = true;
            }
            return;
        };
        // Folders first, then by name; (is a file, sort key, name, path, stamp).
        let mut found: Vec<(bool, String, String, PathBuf, u64)> = Vec::new();
        for e in rd.flatten().take(2000) {
            let Ok(m) = e.metadata() else { continue };
            // Hidden and system files stay hidden, as on the desktop.
            if m.file_attributes() & 0x6 != 0 {
                continue;
            }
            let path = e.path();
            let is_dir = m.is_dir();
            let ext = path.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            // Shortcuts show without their extension, as Explorer shows them.
            let name = if !is_dir && (ext == "lnk" || ext == "url") {
                path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            } else {
                e.file_name().to_string_lossy().to_string()
            };
            let stamp = m.last_write_time() ^ m.len().rotate_left(17);
            found.push((!is_dir, name.to_lowercase(), name, path, stamp));
        }
        found.sort();
        let sig = found.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, (d, k, _, _, st)| {
            let h = k.bytes().fold(h, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
            (h ^ st ^ u64::from(*d)).wrapping_mul(0x0100_0000_01b3)
        });
        let w = &mut self.ws[i];
        if w.status.is_some() {
            w.status = None;
            w.dirty = true;
        }
        if sig == w.signature && !w.entries.is_empty() {
            return;
        }
        w.signature = sig;
        w.entries = found.into_iter().map(|(_, _, name, path, _)| Entry { path, name }).collect();
        let wanted: Vec<PathBuf> = w.entries.iter().map(|e| e.path.clone()).filter(|p| !self.asked_icons.contains(p)).collect();
        if !wanted.is_empty() {
            self.asked_icons.extend(wanted.iter().cloned());
            let px = (40.0 * dpi_scale(self.ws[i].hwnd)).round() as u32;
            let (tx, waker) = (self.done_tx.clone(), self.waker);
            std::thread::spawn(move || {
                for chunk in wanted.chunks(16) {
                    let got: Vec<(PathBuf, Option<tiny_skia::Pixmap>)> = chunk.iter().map(|p| (p.clone(), crate::live::thumb::shell_image(p, px))).collect();
                    let _ = tx.send(Done::Icons(got));
                    waker.wake();
                }
            });
        }
        self.rebuild_items(i);
    }

    pub(super) fn rebuild_items(&mut self, i: usize) {
        let w = &mut self.ws[i];
        w.items = w.entries.iter().map(|e| Item { name: e.name.clone(), icon: self.icons.get(&e.path).cloned().flatten() }).collect();
        if w.selected.is_some_and(|s| s >= w.items.len()) {
            w.selected = None;
        }
        w.dirty = true;
    }

    pub(super) fn paint_dirty(&mut self) {
        for i in 0..self.ws.len() {
            if self.ws[i].dirty && !self.ws[i].hwnd.is_invalid() {
                self.render(i);
            }
        }
    }

    pub(super) fn render(&mut self, i: usize) {
        let look = self.look(i);
        let now = rt_sys::local_time();
        for _ in 0..3 {
            let w = &self.ws[i];
            let caption = w.photo_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let view = View {
                now,
                hover: w.hover,
                hot: w.hot,
                editor: w.editor.as_ref(),
                caret_on: w.caret_on,
                month: w.month,
                country: &crate::widgets::region::region().country,
                weather: w.forecast.as_ref(),
                status: w.status.as_deref(),
                photo: w.photo.as_ref(),
                caption: &caption,
                items: &w.items,
                selected: w.selected,
                scroll: w.scroll,
            };
            self.gfx.resize(w.cfg.w.max(1) as u32, w.cfg.h.max(1) as u32);
            let painted = paint::paint(&mut self.gfx, &w.cfg, &view, &look);
            let w = &mut self.ws[i];
            w.painted = painted;
            // Annotations grow to hold their text.
            if w.cfg.kind == WKind::Annotation {
                let need = (w.painted.content_h + 12.0 * look.s).ceil() as i32;
                if need > w.cfg.h && w.editor.is_some() {
                    w.cfg.h = need;
                    continue;
                }
            }
            // The caret stays in view.
            if let (Some(e), Some(tb)) = (&w.editor, &w.painted.text) {
                let n = line_of(&tb.lines, e.caret) as f32;
                let (top, bottom) = (n * tb.line_h, (n + 1.0) * tb.line_h);
                let room = tb.rect.h;
                let mut s = w.scroll;
                if top < s {
                    s = top;
                } else if bottom > s + room {
                    s = bottom - room;
                }
                if (s - w.scroll).abs() > 0.5 {
                    w.scroll = s;
                    continue;
                }
            }
            break;
        }
        let w = &mut self.ws[i];
        w.dirty = false;
        present(w.hwnd, &self.gfx.pix, w.cfg.x, w.cfg.y);
    }
}
