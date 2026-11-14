//! The "Default app" panel of an app's file types (Windows; rview and rpad):
//! a list of the types to tick, who opens each one now (and whether that is
//! the user's own choice, which Windows protects), "Open with…" per type for
//! those, Apply, and Windows Settings › Default apps. It writes through
//! `rt_sys::win::assoc`, for the current user only.

use crate::controls::{Painter, Palette};
use rt_core::i18n::{tf, tr};
use rt_gfx::{Color, FontId, Gfx, Rect};
use rt_sys::win::assoc::{self, Current, Keys, Spec};
use winit::keyboard::{Key, NamedKey};

/// What a click lands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Tick(usize),
    OpenWith(usize),
    /// The box over the list: all or none.
    All,
    Apply,
    DefaultApps,
    Close,
    /// Anywhere else on the panel.
    Inside,
}

/// What the app does after an event.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Redraw,
    Close,
}

struct Row {
    ext: &'static str,
    ticked: bool,
    now: Current,
}

pub struct AssocPanel {
    spec: &'static Spec,
    /// The context menu entry ("Open with rview"), translated.
    verb: String,
    rows: Vec<Row>,
    scroll: f32,
    /// Height of the list's contents and of the part shown, at the last drawing.
    content_h: f32,
    list: Rect,
    hits: Vec<(Rect, Hit)>,
    hot: Option<Hit>,
    /// What the last action did (and whether it needs the user).
    note: Option<(String, bool)>,
}

impl AssocPanel {
    /// The panel for `spec`, with each type ticked when the app opens it now.
    pub fn new(spec: &'static Spec, verb: &str) -> AssocPanel {
        let mut p = AssocPanel {
            spec,
            verb: verb.to_string(),
            rows: spec.exts.iter().map(|(e, _)| Row { ext: e, ticked: false, now: Current::Nothing }).collect(),
            scroll: 0.0,
            content_h: 0.0,
            list: Rect::default(),
            hits: Vec::new(),
            hot: None,
            note: None,
        };
        p.reread(true);
        p
    }

    fn keys(&self) -> Keys {
        Keys::user(self.spec.name)
    }

    /// Reads again who opens each type; `ticks` also sets the boxes from it.
    fn reread(&mut self, ticks: bool) {
        let keys = self.keys();
        for r in &mut self.rows {
            r.now = assoc::current(self.spec, &keys, r.ext);
            if ticks {
                r.ticked = matches!(r.now, Current::Ours { .. });
            }
        }
    }

    fn apply(&mut self) {
        let want: Vec<&str> = self.rows.iter().filter(|r| r.ticked).map(|r| r.ext).collect();
        let result = assoc::current_exe().and_then(|exe| {
            assoc::set_defaults(self.spec, &self.keys(), &exe, &self.verb, &want)
                .map_err(|e| tf("Cannot change the Windows registry (error {}).", &[&e.to_string()]))
        });
        self.note = Some(match result {
            Ok(d) => {
                let app = self.spec.name;
                let mut parts = Vec::new();
                if !d.set.is_empty() {
                    parts.push(tf("{} now opens: {}", &[app, &assoc::list(&d.set)]));
                }
                if !d.released.is_empty() {
                    parts.push(tf("Given back to the program before: {}", &[&assoc::list(&d.released)]));
                }
                if !d.user_choice.is_empty() {
                    parts.push(tf("Windows keeps your own choice for: {}. Use Open with… on those.", &[&assoc::list(&d.user_choice)]));
                } else if parts.is_empty() {
                    parts.push(tr("Nothing to change.").to_string());
                }
                (parts.join(" "), !d.user_choice.is_empty())
            }
            Err(e) => (e, true),
        });
        self.reread(false);
    }

    /// Windows' "Open with" for one type; the app is registered first, so it
    /// is in the list.
    fn open_with(&mut self, hwnd: rt_sys::win::HWND, i: usize) {
        let Some(ext) = self.rows.get(i).map(|r| r.ext) else { return };
        if let Ok(exe) = assoc::current_exe() {
            let _ = assoc::register(self.spec, &self.keys(), &exe, &self.verb);
        }
        assoc::open_with(hwnd, ext);
        let keys = self.keys();
        if let Some(r) = self.rows.get_mut(i) {
            r.now = assoc::current(self.spec, &keys, ext);
            r.ticked = matches!(r.now, Current::Ours { .. });
        }
        self.note = None;
    }

    fn all_ticked(&self) -> bool {
        self.rows.iter().all(|r| r.ticked)
    }

    /// A click at (x, y).
    pub fn click(&mut self, hwnd: rt_sys::win::HWND, x: f32, y: f32) -> Outcome {
        match Painter::hit_at(&self.hits, x, y) {
            Some(Hit::Tick(i)) => {
                if let Some(r) = self.rows.get_mut(i) {
                    r.ticked = !r.ticked;
                }
            }
            Some(Hit::All) => {
                let on = !self.all_ticked();
                self.rows.iter_mut().for_each(|r| r.ticked = on);
            }
            Some(Hit::OpenWith(i)) => self.open_with(hwnd, i),
            Some(Hit::Apply) => self.apply(),
            Some(Hit::DefaultApps) => assoc::open_default_apps(),
            Some(Hit::Close) => return Outcome::Close,
            Some(Hit::Inside) | None => return Outcome::Nothing,
        }
        Outcome::Redraw
    }

    /// The pointer moved; true when what it is on changed.
    pub fn pointer(&mut self, x: f32, y: f32) -> bool {
        let hot = Painter::hit_at(&self.hits, x, y).filter(|h| *h != Hit::Inside);
        let changed = hot != self.hot;
        self.hot = hot;
        changed
    }

    /// The wheel over the list (`lines` down when positive); true when it moved.
    pub fn wheel(&mut self, lines: f32, s: f32) -> bool {
        let max = (self.content_h - self.list.h).max(0.0);
        let to = (self.scroll + lines * 30.0 * s).clamp(0.0, max);
        let moved = to != self.scroll;
        self.scroll = to;
        moved
    }

    /// Esc closes it.
    pub fn key(&mut self, key: &Key) -> Outcome {
        match key {
            Key::Named(NamedKey::Escape) => Outcome::Close,
            _ => Outcome::Nothing,
        }
    }

    /// Text for screen readers.
    pub fn spoken_text(&self) -> String {
        let mut t = self.intro();
        for r in &self.rows {
            t.push_str(&format!("\n{} {}: {}", if r.ticked { "☑" } else { "☐" }, r.ext, self.now_text(&r.now)));
        }
        t
    }

    fn intro(&self) -> String {
        tf(
            "Tick the file types {} should open, then Apply. For a type whose default you chose yourself, Windows keeps your choice: use Open with… and pick “Always”.",
            &[self.spec.name],
        )
    }

    fn now_text(&self, now: &Current) -> String {
        match now {
            Current::Ours { protected: false } => self.spec.name.to_string(),
            Current::Ours { protected: true } => tf("{} (your choice)", &[self.spec.name]),
            Current::Other { name, protected: false } => name.clone(),
            Current::Other { name, protected: true } => tf("{} (your choice)", &[name]),
            Current::Nothing => tr("No program").to_string(),
        }
    }

    /// Draws it over what is in `g`, centred in `full`.
    pub fn draw(&mut self, g: &mut Gfx, full: Rect, dark: bool, accent: Option<Color>, s: f32) {
        let pal = Palette::new(dark, accent);
        g.fill_rect(full, Color::rgba(0x000000, if dark { 0x90 } else { 0x60 }));
        let px = |v: f32| (v * s).round();
        let w = (full.w - px(32.0)).min(px(640.0));
        let h = (full.h - px(32.0)).min(px(600.0));
        let panel = Rect::new(((full.w - w) / 2.0).round(), ((full.h - h) / 2.0).round(), w, h);
        let r = px(8.0);
        for i in 1..=4 {
            let d = i as f32 * s;
            g.fill_rrect(Rect::new(panel.x - d, panel.y - d + 2.0 * s, panel.w + 2.0 * d, panel.h + 2.0 * d), r + d, Color::rgba(0x000000, 0x18));
        }
        g.fill_rrect(panel, r, pal.panel);
        g.stroke_rrect(panel, r, 1.0, pal.border);
        let pad = px(20.0);
        let (x, inner_w) = (panel.x + pad, panel.w - 2.0 * pad);
        let mut hits = vec![(panel, Hit::Inside)];

        // Title, the explanation, the box for all.
        let mut p = Painter::new(g, &pal, s, self.hot, panel);
        let title_px = 17.0 * s;
        let (asc, _) = p.g.metrics(title_px, FontId::Bold);
        p.g.text(tr("Default app for file types"), x, panel.y + pad + asc, title_px, FontId::Bold, pal.text);
        let mut y = panel.y + pad + title_px * 1.6;
        y = p.note(&self.intro(), x, y, inner_w, false) + px(8.0);
        let all = self.all_ticked();
        p.choice(x, y, px(160.0), tr("All types"), all, false, Hit::All);
        let ext_w = px(130.0);
        let open_label = tr("Open with…");
        let open_w = p.text_width(open_label, 96.0);
        p.label(tr("Opens now"), Rect::new(x + ext_w, y, inner_w - ext_w - open_w, px(26.0)), 11.5, FontId::Semibold, pal.dim);
        y += px(30.0);
        p.g.fill_rect(Rect::new(x, y - px(2.0), inner_w, 1.0), pal.border);

        // The buttons and the note at the bottom.
        let btn_h = px(30.0);
        let by = panel.bottom() - pad - btn_h;
        let note_lines = self.note.as_ref().map_or(0.0, |_| 3.0);
        let list_bottom = by - px(10.0) - note_lines * px(18.0);
        if let Some((text, warn)) = &self.note {
            p.note(text, x, list_bottom + px(6.0), inner_w, *warn);
        }
        let mut bx = panel.right() - pad;
        for (label, hit, primary) in [(tr("Close"), Hit::Close, false), (tr("Apply"), Hit::Apply, true), (tr("Default apps…"), Hit::DefaultApps, false)] {
            let bw = p.text_width(label, 88.0);
            bx -= bw;
            p.button(Rect::new(bx, by, bw, btn_h), label, hit, true, primary);
            bx -= px(8.0);
        }
        hits.append(&mut p.hits);

        // The list, scrolled and clipped.
        self.list = Rect::new(x, y, inner_w, (list_bottom - y).max(0.0));
        let row_h = px(30.0);
        self.content_h = self.rows.len() as f32 * row_h;
        self.scroll = self.scroll.clamp(0.0, (self.content_h - self.list.h).max(0.0));
        g.set_clip(Some(self.list));
        let mut p = Painter::new(g, &pal, s, self.hot, self.list);
        for (i, row) in self.rows.iter().enumerate() {
            let ry = self.list.y + i as f32 * row_h - self.scroll;
            if ry + row_h < self.list.y || ry > self.list.bottom() {
                continue;
            }
            p.choice(x, ry + px(2.0), ext_w - px(8.0), row.ext, row.ticked, false, Hit::Tick(i));
            let c = match row.now {
                Current::Ours { .. } => pal.accent,
                _ => pal.text,
            };
            let text = self.now_text(&row.now);
            p.label(&text, Rect::new(x + ext_w, ry + px(2.0), inner_w - ext_w - open_w - px(8.0), px(26.0)), 12.5, FontId::Regular, c);
            // "Open with…" is the way past a protected choice: it stands out
            // where the app is wanted and Windows keeps another one.
            let blocked = row.ticked && matches!(row.now, Current::Other { protected: true, .. });
            let br = Rect::new(x + inner_w - open_w, ry + px(2.0), open_w, px(26.0));
            p.button(br, open_label, Hit::OpenWith(i), true, blocked);
        }
        hits.append(&mut p.hits);
        g.set_clip(None);
        if self.content_h > self.list.h {
            // A thin bar: where the list is.
            let frac = self.list.h / self.content_h;
            let th = (self.list.h * frac).max(px(20.0));
            let ty = self.list.y + (self.list.h - th) * (self.scroll / (self.content_h - self.list.h));
            g.fill_rrect(Rect::new(panel.right() - pad / 2.0 - px(2.0), ty, px(4.0), th), px(2.0), pal.border);
        }
        self.hits = hits;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static SPEC: Spec = Spec {
        name: "rt-ui-assoc-test",
        description: "test",
        exts: &[(".rtuitesta", "rt-ui-assoc-test.a"), (".rtuitestb", "rt-ui-assoc-test.b")],
        progids: &[("rt-ui-assoc-test.a", "A"), ("rt-ui-assoc-test.b", "B")],
    };

    /// Drawn, the boxes and buttons answer clicks (nothing is written: no
    /// Apply, no Open with).
    #[test]
    fn ticks_and_all() {
        let Ok(mut g) = Gfx::new() else { return };
        g.resize(800, 600);
        let mut p = AssocPanel::new(&SPEC, "Open");
        assert!(p.rows.iter().all(|r| !r.ticked && r.now == Current::Nothing));
        p.draw(&mut g, Rect::new(0.0, 0.0, 800.0, 600.0), false, None, 1.0);
        let at = |p: &AssocPanel, h: Hit| p.hits.iter().find(|(_, x)| *x == h).map(|(r, _)| (r.x + 4.0, r.center_y())).unwrap();
        let (x, y) = at(&p, Hit::Tick(1));
        assert_eq!(p.click(std::ptr::null_mut(), x, y), Outcome::Redraw);
        assert!(p.rows[1].ticked && !p.rows[0].ticked);
        let (x, y) = at(&p, Hit::All);
        p.click(std::ptr::null_mut(), x, y);
        assert!(p.all_ticked());
        p.click(std::ptr::null_mut(), x, y);
        assert!(p.rows.iter().all(|r| !r.ticked));
        let (x, y) = at(&p, Hit::Close);
        assert_eq!(p.click(std::ptr::null_mut(), x, y), Outcome::Close);
        assert_eq!(p.click(std::ptr::null_mut(), 5.0, 5.0), Outcome::Nothing, "outside");
        assert!(p.spoken_text().contains(".rtuitesta"));
    }

    /// The panel with rview's types and who opens them on this PC (read
    /// only), to `target/screens/assoc-*.png`:
    /// `cargo test -p rt-ui --lib assoc_pictures -- --ignored`.
    #[test]
    #[ignore]
    fn assoc_pictures() {
        use rt_core::i18n::{set_lang, Lang};
        static RVIEW: Spec = Spec {
            name: "rview",
            description: "test",
            exts: &[
                (".pdf", "rview.pdf"),
                (".md", "rview.md"),
                (".markdown", "rview.md"),
                (".html", "rview.html"),
                (".htm", "rview.html"),
                (".txt", "rview.txt"),
                (".log", "rview.txt"),
                (".csv", "rview.csv"),
                (".tsv", "rview.csv"),
            ],
            progids: &[],
        };
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&out).unwrap();
        let mut g = Gfx::new().unwrap();
        for (name, lang, dark, w, h, note) in [
            ("assoc-light-pt", Lang::PtPT, false, 900u32, 640u32, false),
            ("assoc-dark-en", Lang::EnUS, true, 900, 640, true),
            ("assoc-small-de", Lang::DeDE, false, 520, 420, true),
        ] {
            set_lang(lang);
            let mut p = AssocPanel::new(&RVIEW, "Open with rview");
            p.rows[1].ticked = true;
            if note {
                p.note = Some((tf("Windows keeps your own choice for: {}. Use Open with… on those.", &[".pdf, .html"]), true));
            }
            g.resize(w, h);
            g.fill_rect(Rect::new(0.0, 0.0, w as f32, h as f32), if dark { Color::rgb(0x202020) } else { Color::rgb(0xF0F0F0) });
            p.draw(&mut g, Rect::new(0.0, 0.0, w as f32, h as f32), dark, None, 1.0);
            g.pix.save_png(out.join(format!("{name}.png"))).unwrap();
        }
        set_lang(Lang::EnUS);
    }
}
