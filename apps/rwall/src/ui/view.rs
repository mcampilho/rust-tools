//! The settings window, in two tabs.
//!
//! - Static wallpaper: the picture on the desktop now, how often it changes,
//!   where pictures come from, how they fill the screen.
//! - Dynamic wallpaper: a video, an animated picture or a scene playing
//!   behind the desktop icons, how it fills the screens and when it pauses.
//! - Widgets: clocks, calendars, notes, the weather, fences and photos on
//!   the desktop; each one's options.
//!
//! [`draw`] paints the window and returns where each control is, so the
//! clicks are tested against exactly what was drawn.

use super::gfx::{Align, Color, FontId, Gfx, Rect};
use super::theme::Theme;
use crate::i18n::{tf, tr};
use crate::live::{Kind, LiveFit, PauseWhen, Scene};
use crate::schedule::Frequency;
use crate::settings::{Fit, Settings};
use crate::sources::Source;
use crate::widgets::{WKind, Widget};
use rt_gfx::dialog::{ellipsize, ellipsize_middle, wrap_words};

/// A text box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    /// Bing market ("pt-PT").
    Market,
    Wallhaven,
    /// The city of a weather widget.
    City(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Static,
    Dynamic,
    Widgets,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Static, Tab::Dynamic, Tab::Widgets];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Static => "Static wallpaper",
            Tab::Dynamic => "Dynamic wallpaper",
            Tab::Widgets => "Widgets",
        }
    }
}

/// What a click can land on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Tab(Tab),
    Next,
    About,
    OpenFolder,
    Frequency(Frequency),
    CustomLess,
    CustomMore,
    Source(Source),
    Field(Field),
    AddFolder,
    RemoveFolder(usize),
    Subfolders,
    Fit(Fit),
    KeepLess,
    KeepMore,
    Autostart,
    /// Turns the dynamic wallpaper on or off.
    LiveOn,
    LiveKind(Kind),
    /// Choose the file (or folder) of a kind.
    LiveChoose(Kind),
    /// Another video from the folder.
    LiveNext,
    LiveShowFile,
    LiveScene(Scene),
    LiveFit(LiveFit),
    LivePause(PauseWhen),
    LiveSubfolders,
    /// Shows or hides all the widgets.
    WidgetsOn,
    AddWidget(WKind),
    WidgetTop(u32),
    WidgetRemove(u32),
    WidgetSeconds(u32),
    WidgetH24(u32),
    WidgetHolidays(u32),
    /// Choose the folder of a fence or of photos.
    WidgetFolder(u32),
    /// Photos change less or more often.
    WidgetLess(u32),
    WidgetMore(u32),
    /// °C (false) or °F (true).
    WidgetUnits(u32, bool),
}

/// The widgets, as the window shows them.
#[derive(Default)]
pub struct WidgetsModel<'a> {
    /// Windows only, for now.
    pub available: bool,
    /// Resident (`--tray`): they stay when the window closes.
    pub tray: bool,
    /// A word about some of them (the weather).
    pub status: &'a [(u32, String)],
    /// The country of the regional settings (ISO code), for the holidays.
    pub country: &'a str,
}

/// The dynamic wallpaper, as the window shows it.
#[derive(Default)]
pub struct LiveModel<'a> {
    /// Windows only, for now.
    pub available: bool,
    pub thumb: Option<&'a tiny_skia::Pixmap>,
    /// What plays: a file name or a scene.
    pub title: String,
    /// Playing, paused (and why), off.
    pub state: String,
    /// Under the card: what went wrong, or a word about the mode.
    pub note: String,
    pub error: Option<&'a str>,
    pub can_show_file: bool,
}

/// What the window shows, besides the settings.
pub struct Model<'a> {
    pub settings: &'a Settings,
    pub tab: Tab,
    pub thumb: Option<&'a tiny_skia::Pixmap>,
    /// A change is under way.
    pub busy: bool,
    /// "Changed …. Next change: …".
    pub status: String,
    pub error: Option<&'a str>,
    pub autostart: bool,
    pub focus: Option<Field>,
    pub hot: Option<Hit>,
    pub scroll: f32,
    pub live: LiveModel<'a>,
    pub widgets: WidgetsModel<'a>,
}

/// Where the controls are, and how tall the content is (for scrolling).
#[derive(Default)]
pub struct Drawn {
    pub hits: Vec<(Rect, Hit)>,
    pub content_h: f32,
}

impl Drawn {
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, h)| *h)
    }
}

/// Text of each frequency (translated by the caller).
pub fn frequency_label(f: Frequency) -> &'static str {
    match f {
        Frequency::Daily => "Every day",
        Frequency::HalfDay => "Twice a day (00:00, 12:00)",
        Frequency::Quarter => "Four times a day (00:00, 06:00, 12:00, 18:00)",
        Frequency::Weekly => "Every week (on Monday)",
        Frequency::Monthly => "Every month (on the 1st)",
        Frequency::Custom => "Every",
    }
}

struct Painter<'a> {
    g: &'a mut Gfx,
    t: &'a Theme,
    s: f32,
    hot: Option<Hit>,
    hits: Vec<(Rect, Hit)>,
    /// Clicks count only inside this.
    full: Rect,
}

/// A button: text, what it does, enabled, primary.
type Button = (String, Hit, bool, bool);

impl Painter<'_> {
    fn px(&self, v: f32) -> f32 {
        v * self.s
    }

    fn hit(&mut self, r: Rect, h: Hit) {
        let vis = r.intersect(&self.full);
        if vis.w > 0.0 && vis.h > 0.0 {
            self.hits.push((vis, h));
        }
    }

    fn label(&mut self, text: &str, r: Rect, px: f32, font: FontId, c: Color) -> f32 {
        let s = ellipsize(self.g, text, r.w, self.px(px), font);
        self.g.text_in(&s, r, self.px(px), font, c, Align::Left)
    }

    fn heading(&mut self, text: &str, x: f32, y: f32, w: f32) {
        let r = Rect::new(x, y, w, self.px(24.0));
        let c = self.t.text;
        self.label(text, r, 13.5, FontId::Semibold, c);
    }

    /// Lines of dim (or warning) text, wrapped; returns the new `y`.
    fn note(&mut self, text: &str, x: f32, mut y: f32, w: f32, warn: bool) -> f32 {
        let c = if warn { self.t.warn } else { self.t.dim };
        let px = self.px(11.5);
        for l in wrap_words(self.g, text, w, px, FontId::Regular).iter().take(3) {
            self.label(l, Rect::new(x, y, w, self.px(18.0)), 11.5, FontId::Regular, c);
            y += self.px(18.0);
        }
        y
    }

    fn button(&mut self, r: Rect, text: &str, h: Hit, enabled: bool, primary: bool) {
        let t = *self.t;
        let rad = self.px(4.0);
        let hot = enabled && self.hot == Some(h);
        if primary && enabled {
            let bg = if hot { t.accent.mix(t.bg, 0.15) } else { t.accent };
            self.g.fill_rrect(r, rad, bg);
        } else {
            self.g.fill_rrect(r, rad, t.panel);
            if hot {
                self.g.fill_rrect(r, rad, t.hover);
            }
            self.g.stroke_rrect(r.inset(0.5), rad, 1.0, t.border);
        }
        let c = if !enabled {
            t.dim
        } else if primary {
            t.on_accent
        } else {
            t.text
        };
        let px = self.px(12.5);
        let label = ellipsize(self.g, text, r.w - self.px(12.0), px, FontId::Regular);
        self.g.text_in(&label, r, px, FontId::Regular, c, Align::Center);
        if enabled {
            self.hit(r, h);
        }
    }

    /// A button as wide as its text (at least `min`).
    fn text_width(&mut self, text: &str, min: f32) -> f32 {
        (self.g.measure(text, self.px(12.5), FontId::Regular) + self.px(24.0)).max(self.px(min))
    }

    /// A check box or a radio button with its text; returns the row's width.
    #[allow(clippy::too_many_arguments)]
    fn choice(&mut self, x: f32, y: f32, w: f32, text: &str, on: bool, radio: bool, h: Hit) -> f32 {
        let t = *self.t;
        let row = Rect::new(x, y, w, self.px(26.0));
        let b = self.px(16.0);
        let bx = Rect::new(x + self.px(2.0), row.center_y() - b / 2.0, b, b);
        if self.hot == Some(h) {
            self.g.fill_rrect(Rect::new(x - self.px(4.0), y, w + self.px(8.0), row.h), self.px(4.0), t.hover);
        }
        if radio {
            let (cx, cy) = (bx.x + b / 2.0, bx.y + b / 2.0);
            self.g.fill_circle(cx, cy, b / 2.0, if on { t.accent } else { t.border });
            self.g.fill_circle(cx, cy, b / 2.0 - self.px(1.2), t.field);
            if on {
                self.g.fill_circle(cx, cy, b / 2.0 - self.px(4.5), t.accent);
            }
        } else if on {
            self.g.fill_rrect(bx, self.px(3.0), t.accent);
            let (x0, y0) = (bx.x, bx.y);
            let pts = [(x0 + b * 0.22, y0 + b * 0.52), (x0 + b * 0.42, y0 + b * 0.72), (x0 + b * 0.78, y0 + b * 0.3)];
            self.g.polyline(&pts, self.px(1.8), t.on_accent, false);
        } else {
            self.g.fill_rrect(bx, self.px(3.0), t.field);
            self.g.stroke_rrect(bx.inset(0.5), self.px(3.0), 1.0, t.dim);
        }
        let tx = bx.right() + self.px(8.0);
        let tw = self.label(text, Rect::new(tx, y, w - (tx - x), row.h), 12.5, FontId::Regular, t.text);
        let used = tx - x + tw + self.px(6.0);
        self.hit(Rect::new(x, y, used.min(w), row.h), h);
        used
    }

    fn field(&mut self, r: Rect, value: &str, placeholder: &str, f: Field, focus: bool) {
        let t = *self.t;
        let rad = self.px(4.0);
        self.g.fill_rrect(r, rad, t.field);
        self.g.stroke_rrect(r.inset(0.5), rad, if focus { 1.5 } else { 1.0 }, if focus { t.accent } else { t.border });
        let inner = Rect::new(r.x + self.px(7.0), r.y, r.w - self.px(14.0), r.h);
        let px = self.px(12.5);
        if value.is_empty() && !focus {
            self.label(placeholder, inner, 12.5, FontId::Italic, t.dim);
        } else {
            // The end of the text stays in view while typing.
            let mut shown = value.to_string();
            while !shown.is_empty() && self.g.measure(&shown, px, FontId::Regular) > inner.w - self.px(4.0) {
                shown.remove(0);
            }
            let w = self.g.text_in(&shown, inner, px, FontId::Regular, t.text, Align::Left);
            if focus {
                let cx = (inner.x + w + self.px(1.0)).round();
                self.g.fill_rect(Rect::new(cx, r.y + self.px(5.0), self.px(1.0).max(1.0), r.h - self.px(10.0)), t.text);
            }
        }
        self.hit(r, Hit::Field(f));
    }

    /// "[−] value [+]".
    fn stepper(&mut self, x: f32, y: f32, value: &str, less: Hit, more: Hit) -> f32 {
        let b = self.px(26.0);
        self.button(Rect::new(x, y, b, b), "−", less, true, false);
        let vw = self.g.measure(value, self.px(12.5), FontId::Semibold).max(self.px(28.0)) + self.px(12.0);
        let t = self.t.text;
        self.g.text_in(value, Rect::new(x + b, y, vw, b), self.px(12.5), FontId::Semibold, t, Align::Center);
        self.button(Rect::new(x + b + vw, y, b, b), "+", more, true, false);
        2.0 * b + vw
    }

    /// Buttons side by side, as wide as their texts when they fit, else in
    /// equal shares.
    fn segments(&mut self, x: f32, y: f32, w: f32, buttons: &[Button], max_each: f32) {
        let n = buttons.len() as f32;
        let gap = self.px(6.0);
        let seg_w = ((w - (n - 1.0) * gap) / n).min(self.px(max_each));
        for (i, (text, h, enabled, primary)) in buttons.iter().enumerate() {
            let r = Rect::new(x + i as f32 * (seg_w + gap), y, seg_w, self.px(28.0));
            self.button(r, text, *h, *enabled, *primary);
        }
    }

    /// The card at the top of each tab: a preview, a title, two lines of
    /// detail and one more line, with buttons along the bottom. Returns its
    /// height.
    #[allow(clippy::too_many_arguments)]
    fn card(&mut self, x: f32, y: f32, w: f32, thumb: Option<&tiny_skia::Pixmap>, title: Option<&str>, empty: &str, detail: &str, last: &str, buttons: &[Button]) -> f32 {
        let t = *self.t;
        let tw = self.px(192.0).min(w * 0.4);
        let th = tw * 9.0 / 16.0;
        let card_h = self.px(12.0) + th + self.px(12.0) + self.px(28.0) + self.px(12.0);
        let card = Rect::new(x, y, w, card_h);
        self.g.fill_rrect(card, self.px(6.0), t.panel);
        self.g.stroke_rrect(card.inset(0.5), self.px(6.0), 1.0, t.border);
        let thumb_box = Rect::new(x + self.px(12.0), y + self.px(12.0), tw, th);
        self.g.fill_rect(thumb_box, t.field);
        if let Some(img) = thumb {
            // Fitted into the box, centred.
            let k = (thumb_box.w / img.width() as f32).min(thumb_box.h / img.height() as f32);
            let (iw, ih) = (img.width() as f32 * k, img.height() as f32 * k);
            let dest = Rect::new(thumb_box.x + (thumb_box.w - iw) / 2.0, thumb_box.y + (thumb_box.h - ih) / 2.0, iw, ih);
            self.g.draw_image(img, dest);
        } else {
            self.g.text_in(&tr("No picture yet"), thumb_box, self.px(11.5), FontId::Italic, t.dim, Align::Center);
        }
        let ix = thumb_box.right() + self.px(14.0);
        let iw = card.right() - ix - self.px(12.0);
        let mut ty = y + self.px(10.0);
        match title {
            Some(title) => {
                self.label(title, Rect::new(ix, ty, iw, self.px(22.0)), 14.0, FontId::Semibold, t.text);
                ty += self.px(22.0);
                let px = self.px(11.5);
                let mut lines = wrap_words(self.g, detail, iw, px, FontId::Regular);
                if lines.len() > 2 {
                    lines.truncate(2);
                    let more = format!("{}…", lines[1]);
                    lines[1] = ellipsize(self.g, &more, iw, px, FontId::Regular);
                }
                for l in &lines {
                    self.label(l, Rect::new(ix, ty, iw, self.px(17.0)), 11.5, FontId::Regular, t.dim);
                    ty += self.px(17.0);
                }
                self.label(last, Rect::new(ix, ty, iw, self.px(17.0)), 11.5, FontId::Regular, t.dim);
            }
            None => {
                self.label(empty, Rect::new(ix, ty, iw, self.px(22.0)), 12.5, FontId::Regular, t.dim);
            }
        }
        // Buttons along the bottom of the card.
        let by = card.bottom() - self.px(12.0) - self.px(28.0);
        let (bx0, bw) = (x + self.px(12.0), w - self.px(24.0));
        let n = buttons.len().max(1) as f32;
        let mut widths: Vec<f32> = buttons.iter().map(|b| self.text_width(&b.0, 0.0)).collect();
        let total: f32 = widths.iter().sum::<f32>() + self.px(8.0) * (n - 1.0);
        if total <= bw {
            let extra = (bw - total) / n;
            widths.iter_mut().for_each(|w| *w += extra.min(self.px(24.0)));
        } else {
            widths.fill(((bw - self.px(8.0) * (n - 1.0)) / n).max(self.px(60.0)));
        }
        let mut bx = bx0;
        for (i, (text, h, enabled, primary)) in buttons.iter().enumerate() {
            let r = Rect::new(bx, by, widths[i], self.px(28.0));
            bx += widths[i] + self.px(8.0);
            self.button(r, text, *h, *enabled, *primary);
        }
        card_h
    }
}

/// Height of the row of tabs.
const TABS_H: f32 = 42.0;

/// Paints the window into `full` and returns where its controls are.
pub fn draw(g: &mut Gfx, m: &Model, t: &Theme, s: f32, full: Rect) -> Drawn {
    g.clear(t.bg);
    let bar = Rect::new(full.x, full.y, full.w, TABS_H * s);
    let content = Rect::new(full.x, bar.bottom(), full.w, (full.h - bar.h).max(0.0));
    g.set_clip(Some(content));
    let mut p = Painter { g, t, s, hot: m.hot, hits: Vec::new(), full: content };
    let pad = p.px(20.0);
    let x = full.x + pad;
    let w = (full.w - 2.0 * pad).max(p.px(200.0));
    let y0 = content.y + p.px(14.0) - m.scroll;
    let y = match m.tab {
        Tab::Static => static_tab(&mut p, m, x, y0, w),
        Tab::Dynamic => dynamic_tab(&mut p, m, x, y0, w),
        Tab::Widgets => widgets_tab(&mut p, m, x, y0, w),
    };
    let content_h = y + m.scroll - full.y;

    // The tabs stay on top.
    p.g.set_clip(Some(full));
    p.full = full;
    p.g.fill_rect(bar, t.bg);
    p.g.fill_rect(Rect::new(bar.x, bar.bottom() - p.px(1.0), bar.w, p.px(1.0)), t.border);
    let mut tx = x;
    for tab in Tab::ALL {
        let label = tr(tab.label());
        let tw = p.g.measure(&label, p.px(13.0), FontId::Semibold) + p.px(24.0);
        let r = Rect::new(tx, bar.y + p.px(6.0), tw, bar.h - p.px(6.0));
        let on = m.tab == tab;
        if p.hot == Some(Hit::Tab(tab)) && !on {
            p.g.fill_rrect(r.inset(2.0), p.px(4.0), t.hover);
        }
        let c = if on { t.text } else { t.dim };
        p.g.text_in(&label, r, p.px(13.0), if on { FontId::Semibold } else { FontId::Regular }, c, Align::Center);
        if on {
            p.g.fill_rect(Rect::new(r.x + p.px(8.0), r.bottom() - p.px(3.0), r.w - p.px(16.0), p.px(3.0)), t.accent);
        }
        p.hit(r, Hit::Tab(tab));
        tx += tw + p.px(4.0);
    }
    // For all of rwall, not one tab: at the right of the tabs.
    let text = autostart_label();
    let room = x + w - tx - p.px(8.0);
    let want = p.g.measure(&text, p.px(12.5), FontId::Regular) + p.px(34.0);
    let cw = want.min(room);
    if cw > p.px(60.0) {
        p.choice(x + w - cw, bar.y + (bar.h - p.px(26.0)) / 2.0 + p.px(2.0), cw, &text, m.autostart, false, Hit::Autostart);
    }
    p.g.set_clip(None);
    Drawn { hits: p.hits, content_h }
}

/// The still picture: returns the `y` under the last control.
fn static_tab(p: &mut Painter, m: &Model, x: f32, mut y: f32, w: f32) -> f32 {
    let t = *p.t;
    let st = m.settings;
    let row = p.px(28.0);
    let gap = p.px(18.0);

    // ---- Now on the desktop
    p.heading(&tr("On the desktop now"), x, y, w);
    y += p.px(30.0);
    let (title, detail, last) = match &st.current {
        Some(c) => {
            let title = if c.title.is_empty() { c.file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() } else { c.title.clone() };
            let src = Source::from_key(&c.source).map(|s| tr(s.label())).unwrap_or_default();
            let line = if c.when.is_empty() { src } else { format!("{src} · {}", c.when) };
            (Some(title), c.credit.clone(), line)
        }
        None => (None, String::new(), String::new()),
    };
    let link = st.current.as_ref().is_some_and(|c| !c.link.is_empty());
    let next = if m.busy { tr("Changing…") } else { tr("Next picture") };
    let buttons = [
        (next, Hit::Next, !m.busy && !st.sources.is_empty(), true),
        (tr("About this picture"), Hit::About, link, false),
        (tr("Show the file"), Hit::OpenFolder, true, false),
    ];
    let empty = tr("rwall has not changed the wallpaper yet.");
    y += p.card(x, y, w, m.thumb, title.as_deref(), &empty, &detail, &last, &buttons) + p.px(6.0);
    let (status, warn) = match m.error {
        Some(e) => (e.to_string(), true),
        None => (m.status.clone(), false),
    };
    y = p.note(&status, x, y, w, warn) + gap;

    // ---- How often
    p.heading(&tr("Change the picture"), x, y, w);
    y += p.px(30.0);
    let col = (w - p.px(12.0)) / 2.0;
    for (i, f) in Frequency::ALL.into_iter().enumerate() {
        let cx = x + (i % 2) as f32 * (col + p.px(12.0));
        let cy = y + (i / 2) as f32 * row;
        if f == Frequency::Custom {
            let used = p.choice(cx, cy, p.px(80.0).min(col), &tr(frequency_label(f)), st.frequency == f, true, Hit::Frequency(f));
            let sx = cx + used + p.px(2.0);
            let value = tf("{} min", &[&st.custom_minutes.to_string()]);
            p.stepper(sx, cy, &value, Hit::CustomLess, Hit::CustomMore);
        } else {
            p.choice(cx, cy, col, &tr(frequency_label(f)), st.frequency == f, true, Hit::Frequency(f));
        }
    }
    y += 3.0 * row + gap;

    // ---- Sources
    p.heading(&tr("Take pictures from"), x, y, w);
    y += p.px(30.0);
    let field_h = p.px(26.0);
    for src in Source::ALL {
        let on = st.source_on(src);
        let (field, value, placeholder, fw) = match src {
            Source::Bing => (Some(Field::Market), st.bing_market.as_str(), crate::i18n::lang().tag().to_string(), 90.0),
            Source::Wallhaven => (Some(Field::Wallhaven), st.wallhaven_query.as_str(), tr("any subject"), 170.0),
            _ => (None, "", String::new(), 0.0),
        };
        let fw = p.px(fw);
        let label_w = if field.is_some() { w - fw - p.px(110.0) } else { w };
        p.choice(x, y, label_w, &tr(src.label()), on, false, Hit::Source(src));
        if let Some(f) = field {
            let caption = match f {
                Field::Market => tr("Region:"),
                Field::Wallhaven | Field::City(_) => tr("Search:"),
            };
            let fx = x + w - fw;
            let cw = p.px(100.0);
            let cap = ellipsize(p.g, &caption, cw, p.px(12.0), FontId::Regular);
            p.g.text_in(&cap, Rect::new(fx - cw - p.px(8.0), y, cw, field_h), p.px(12.0), FontId::Regular, t.dim, Align::Right);
            p.field(Rect::new(fx, y, fw, field_h), value, &placeholder, f, m.focus == Some(f));
        }
        y += row + p.px(2.0);
    }
    // The folders, under "My folders".
    let indent = x + p.px(26.0);
    let fw = w - p.px(26.0);
    for (i, f) in st.folders.iter().enumerate() {
        let r = Rect::new(indent, y, fw, p.px(26.0));
        if m.hot == Some(Hit::RemoveFolder(i)) {
            p.g.fill_rrect(r, p.px(4.0), t.hover);
        }
        let px = p.px(12.0);
        let text = ellipsize_middle(p.g, &f.display().to_string(), fw - p.px(44.0), px, FontId::Regular);
        let c = if st.source_on(Source::Folders) { t.text } else { t.dim };
        p.g.text_in(&text, Rect::new(indent + p.px(6.0), y, fw, r.h), px, FontId::Regular, c, Align::Left);
        p.button(Rect::new(r.right() - p.px(28.0), y + p.px(1.0), p.px(24.0), p.px(24.0)), "×", Hit::RemoveFolder(i), true, false);
        y += p.px(28.0);
    }
    if st.folders.is_empty() {
        p.label(&tr("No folder chosen."), Rect::new(indent + p.px(6.0), y, fw, p.px(24.0)), 12.0, FontId::Italic, t.dim);
        y += p.px(26.0);
    }
    let add_w = p.px(150.0);
    p.button(Rect::new(indent, y + p.px(2.0), add_w, p.px(28.0)), &tr("Add a folder…"), Hit::AddFolder, true, false);
    p.choice(indent + add_w + p.px(16.0), y + p.px(3.0), fw - add_w - p.px(16.0), &tr("Include subfolders"), st.subfolders, false, Hit::Subfolders);
    y += p.px(36.0) + gap;

    // ---- Position
    p.heading(&tr("Picture position"), x, y, w);
    y += p.px(30.0);
    let fits: Vec<Button> = Fit::ALL.into_iter().map(|f| (tr(f.label()), Hit::Fit(f), true, st.fit == f)).collect();
    p.segments(x, y, w, &fits, 110.0);
    y += p.px(28.0) + gap;

    // ---- Other
    p.heading(&tr("Other"), x, y, w);
    y += p.px(30.0);
    let keep_label = tr("Downloaded pictures to keep:");
    let lw = p.g.measure(&keep_label, p.px(12.5), FontId::Regular).min(w * 0.6);
    p.g.text_in(&keep_label, Rect::new(x, y, lw, p.px(26.0)), p.px(12.5), FontId::Regular, t.text, Align::Left);
    p.stepper(x + lw + p.px(10.0), y, &st.keep.to_string(), Hit::KeepLess, Hit::KeepMore);
    y + row + p.px(14.0)
}

fn autostart_label() -> String {
    if cfg!(windows) {
        tr("Start with Windows")
    } else {
        tr("Start when I log in")
    }
}

/// The dynamic wallpaper: returns the `y` under the last control.
fn dynamic_tab(p: &mut Painter, m: &Model, x: f32, mut y: f32, w: f32) -> f32 {
    let t = *p.t;
    let lv = &m.live;
    let l = &m.settings.live;
    let row = p.px(28.0);
    let gap = p.px(18.0);

    p.heading(&tr("On the desktop now"), x, y, w);
    y += p.px(30.0);
    if !lv.available {
        return p.note(&tr("The dynamic wallpaper is available only on Windows for now."), x, y, w, false) + gap;
    }
    let can_play = l.show().is_some() || (l.kind == Kind::Folder && !l.folder.as_os_str().is_empty());
    let on_off = if l.on { (tr("Turn off"), Hit::LiveOn, true, false) } else { (tr("Turn on"), Hit::LiveOn, can_play, true) };
    let mut buttons = vec![on_off];
    if l.kind == Kind::Folder {
        buttons.push((tr("Next video"), Hit::LiveNext, l.on, false));
    }
    if l.kind != Kind::Scene {
        buttons.push((tr("Show the file"), Hit::LiveShowFile, lv.can_show_file, false));
    }
    let title = (!lv.title.is_empty()).then_some(lv.title.as_str());
    let empty = tr("Nothing chosen to play.");
    y += p.card(x, y, w, lv.thumb, title, &empty, &tr(l.kind.label()), &lv.state, &buttons) + p.px(6.0);
    y = match lv.error {
        Some(e) => p.note(e, x, y, w, true),
        None => p.note(&lv.note, x, y, w, false),
    } + gap;

    // ---- What plays
    p.heading(&tr("Play"), x, y, w);
    y += p.px(30.0);
    let choose = tr("Choose…");
    let bw = p.text_width(&choose, 84.0);
    for kind in Kind::ALL {
        let on = l.kind == kind;
        if kind == Kind::Scene {
            p.choice(x, y, w, &tr(kind.label()), on, true, Hit::LiveKind(kind));
            y += row + p.px(2.0);
            let scenes: Vec<Button> = Scene::ALL.into_iter().map(|s| (tr(s.label()), Hit::LiveScene(s), true, on && l.scene == s)).collect();
            p.segments(x + p.px(26.0), y, w - p.px(26.0), &scenes, 140.0);
            y += row + p.px(6.0);
            continue;
        }
        let label = tr(kind.label());
        let lw = (p.g.measure(&label, p.px(12.5), FontId::Regular) + p.px(40.0)).min(w * 0.5);
        p.choice(x, y, lw, &label, on, true, Hit::LiveKind(kind));
        let file = l.file(kind);
        let px = p.px(12.0);
        let (text, c, font) = if file.as_os_str().is_empty() {
            (tr("Nothing chosen."), t.dim, FontId::Italic)
        } else {
            (file.display().to_string(), if on { t.text } else { t.dim }, FontId::Regular)
        };
        let path_x = x + lw + p.px(8.0);
        let path_w = (w - lw - p.px(16.0) - bw).max(0.0);
        let text = ellipsize_middle(p.g, &text, path_w, px, font);
        p.g.text_in(&text, Rect::new(path_x, y, path_w, p.px(26.0)), px, font, c, Align::Left);
        p.button(Rect::new(x + w - bw, y, bw, p.px(26.0)), &choose, Hit::LiveChoose(kind), true, false);
        y += row + p.px(2.0);
        if kind == Kind::Folder {
            let indent = x + p.px(26.0);
            p.choice(indent, y, w - p.px(26.0), &tr("Include subfolders"), l.subfolders, false, Hit::LiveSubfolders);
            y += row;
            y = p.note(&tr("Another video comes at each change chosen on the Static wallpaper tab (with rwall --tray)."), indent, y, w - p.px(26.0), false) + p.px(6.0);
        }
    }
    y += gap - p.px(6.0);

    // ---- Position
    p.heading(&tr("Position on each screen"), x, y, w);
    y += p.px(30.0);
    let fits: Vec<Button> = LiveFit::ALL.into_iter().map(|f| (tr(f.label()), Hit::LiveFit(f), true, l.fit == f)).collect();
    p.segments(x, y, w, &fits, 110.0);
    y += p.px(28.0) + gap;

    // ---- Pauses
    p.heading(&tr("Pause when"), x, y, w);
    y += p.px(30.0);
    for pw in PauseWhen::ALL {
        p.choice(x, y, w, &tr(pw.label()), l.pause_on(pw), false, Hit::LivePause(pw));
        y += row;
    }
    y + p.px(14.0)
}

/// The widgets: returns the `y` under the last control.
fn widgets_tab(p: &mut Painter, m: &Model, x: f32, mut y: f32, w: f32) -> f32 {
    let t = *p.t;
    let ws = &m.settings.widgets;
    let gap = p.px(18.0);

    p.heading(&tr("Widgets on the desktop"), x, y, w);
    y += p.px(30.0);
    if !m.widgets.available {
        return p.note(&tr("The widgets are available only on Windows for now."), x, y, w, false) + gap;
    }
    p.choice(x, y, w, &tr("Show the widgets on the desktop"), ws.on, false, Hit::WidgetsOn);
    y += p.px(30.0);
    y = p.note(&tr("Drag a widget to move it, or its bottom right corner to resize it; right-click it for more options."), x, y, w, false);
    if !m.widgets.tray {
        y = p.note(&tr("The widgets close with this window; with rwall --tray they stay on the desktop."), x, y, w, false);
    }
    y += gap;

    // ---- Add
    p.heading(&tr("Add a widget"), x, y, w);
    y += p.px(30.0);
    for row in WKind::ALL.chunks(3) {
        let buttons: Vec<Button> = row.iter().map(|k| (tr(k.label()), Hit::AddWidget(*k), ws.can_add(*k), false)).collect();
        p.segments(x, y, w, &buttons, 400.0);
        y += p.px(34.0);
    }
    let most = crate::widgets::MAX_WIDGETS.to_string();
    let (text, warn) = if ws.list.len() >= crate::widgets::MAX_WIDGETS {
        (tf("That is the most: {} widgets. Remove one to add another.", &[&most]), true)
    } else {
        (tf("One clock, calendar and weather of each kind; up to {} widgets in all.", &[&most]), false)
    };
    y = p.note(&text, x, y, w, warn);
    y += gap - p.px(6.0);

    // ---- The list
    p.heading(&tr("Your widgets"), x, y, w);
    y += p.px(30.0);
    if ws.list.is_empty() {
        p.label(&tr("No widgets yet."), Rect::new(x, y, w, p.px(24.0)), 12.0, FontId::Italic, t.dim);
        y += p.px(28.0);
    }
    for wd in &ws.list {
        y = widget_row(p, m, wd, x, y, w) + p.px(8.0);
    }
    if ws.list.iter().any(|wd| wd.kind == WKind::Weather) {
        y = p.note(&tr("Weather data by Open-Meteo.com"), x, y, w, false);
    }
    y + p.px(14.0)
}

/// One widget of the list, in a box; returns the `y` under it.
fn widget_row(p: &mut Painter, m: &Model, wd: &Widget, x: f32, y: f32, w: f32) -> f32 {
    let t = *p.t;
    let pad = p.px(10.0);
    let (ix, iw) = (x + pad, w - 2.0 * pad);
    let row = p.px(28.0);
    // The box is drawn first and sized after: count the rows.
    let extra = match wd.kind {
        WKind::Analog | WKind::Digital | WKind::Calendar | WKind::ClockCalendar | WKind::Fence => 1,
        WKind::Weather => 2,
        WKind::Photos => 2,
        _ => 0,
    };
    let h = pad * 2.0 + row * (1 + extra) as f32;
    let r = Rect::new(x, y, w, h);
    p.g.fill_rrect(r, p.px(6.0), t.panel);
    p.g.stroke_rrect(r.inset(0.5), p.px(6.0), 1.0, t.border);
    let mut ry = y + pad;

    // Its name, a word about it, "Always on top" and ×.
    let rm = p.px(26.0);
    p.button(Rect::new(ix + iw - rm, ry + p.px(1.0), rm, rm), "×", Hit::WidgetRemove(wd.id), true, false);
    let top_label = tr("Always on top");
    let top_w = (p.g.measure(&top_label, p.px(12.5), FontId::Regular) + p.px(34.0)).min(iw * 0.4);
    let top_x = ix + iw - rm - p.px(10.0) - top_w;
    p.choice(top_x, ry, top_w, &top_label, wd.on_top, false, Hit::WidgetTop(wd.id));
    let name = tr(wd.kind.label());
    let nw = p.g.measure(&name, p.px(13.0), FontId::Semibold).min(top_x - ix - p.px(8.0));
    p.label(&name, Rect::new(ix, ry, nw, row), 13.0, FontId::Semibold, t.text);
    let summary = wd.summary();
    // The weather, fences and photos say more on the rows below.
    if !summary.is_empty() && wd.kind.editable() {
        let sx = ix + nw + p.px(10.0);
        let sw = (top_x - sx - p.px(8.0)).max(0.0);
        let px = p.px(12.0);
        let s = ellipsize(p.g, &summary, sw, px, FontId::Regular);
        p.g.text_in(&s, Rect::new(sx, ry, sw, row), px, FontId::Regular, t.dim, Align::Left);
    }
    ry += row;

    match wd.kind {
        WKind::Analog | WKind::Digital | WKind::Calendar | WKind::ClockCalendar => {
            let mut cx = ix;
            if wd.kind.has_clock() {
                cx += p.choice(cx, ry, iw / 3.0, &tr("Seconds"), wd.seconds, false, Hit::WidgetSeconds(wd.id)) + p.px(16.0);
                if wd.kind != WKind::Analog {
                    cx += p.choice(cx, ry, iw / 3.0, &tr("24-hour clock"), wd.h24, false, Hit::WidgetH24(wd.id)) + p.px(16.0);
                }
            }
            if wd.kind.has_month() {
                let rw = ix + iw - cx;
                match crate::widgets::holidays::country_name(m.widgets.country) {
                    Some(name) => {
                        p.choice(cx, ry, rw, &tf("Holidays ({})", &[&tr(name)]), wd.holidays, false, Hit::WidgetHolidays(wd.id));
                    }
                    None => {
                        let px = p.px(11.5);
                        let s = ellipsize(p.g, &tr("No holidays known for this region."), rw, px, FontId::Italic);
                        p.g.text_in(&s, Rect::new(cx, ry, rw, p.px(26.0)), px, FontId::Italic, t.dim, Align::Left);
                    }
                }
            }
        }
        WKind::Weather => {
            let cap = tr("City:");
            let cw = p.g.measure(&cap, p.px(12.5), FontId::Regular) + p.px(8.0);
            p.g.text_in(&cap, Rect::new(ix, ry, cw, p.px(26.0)), p.px(12.5), FontId::Regular, t.text, Align::Left);
            let units = [("°C".to_string(), Hit::WidgetUnits(wd.id, false), true, !wd.fahrenheit), ("°F".to_string(), Hit::WidgetUnits(wd.id, true), true, wd.fahrenheit)];
            let uw = p.px(104.0);
            let fw = (iw - cw - uw - p.px(12.0)).max(p.px(80.0));
            let f = Field::City(wd.id);
            p.field(Rect::new(ix + cw, ry, fw, p.px(26.0)), &wd.city, &tr("e.g. Lisbon"), f, m.focus == Some(f));
            p.segments(ix + iw - uw, ry - p.px(1.0), uw, &units, 50.0);
            ry += row + p.px(2.0);
            let status = m.widgets.status.iter().find(|(id, _)| *id == wd.id).map(|(_, s)| s.clone());
            let (text, warn) = match status {
                Some(s) => (s, true),
                None if wd.has_place() => (wd.place.clone(), false),
                None => (tr("Type a city in the Widgets tab of rwall."), false),
            };
            p.label(&text, Rect::new(ix, ry, iw, p.px(22.0)), 11.5, FontId::Regular, if warn { t.warn } else { t.dim });
        }
        WKind::Fence | WKind::Photos => {
            let choose = tr("Choose…");
            let bw = p.text_width(&choose, 84.0);
            let px = p.px(12.0);
            let (text, c, font) = if wd.folder.as_os_str().is_empty() { (tr("Nothing chosen."), t.dim, FontId::Italic) } else { (wd.folder.display().to_string(), t.text, FontId::Regular) };
            let tw = (iw - bw - p.px(10.0)).max(0.0);
            let text = ellipsize_middle(p.g, &text, tw, px, font);
            p.g.text_in(&text, Rect::new(ix, ry, tw, p.px(26.0)), px, font, c, Align::Left);
            p.button(Rect::new(ix + iw - bw, ry, bw, p.px(26.0)), &choose, Hit::WidgetFolder(wd.id), true, false);
            if wd.kind == WKind::Photos {
                ry += row + p.px(2.0);
                let label = tr("Change every");
                let lw = p.g.measure(&label, p.px(12.5), FontId::Regular).min(iw * 0.6);
                p.g.text_in(&label, Rect::new(ix, ry, lw, p.px(26.0)), p.px(12.5), FontId::Regular, t.text, Align::Left);
                let value = tf("{} min", &[&wd.minutes.to_string()]);
                p.stepper(ix + lw + p.px(10.0), ry, &value, Hit::WidgetLess(wd.id), Hit::WidgetMore(wd.id));
            }
        }
        _ => {}
    }
    y + h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(st: &Settings, tab: Tab) -> Model<'_> {
        Model {
            settings: st,
            tab,
            thumb: None,
            busy: false,
            status: String::new(),
            error: None,
            autostart: false,
            focus: None,
            hot: None,
            scroll: 0.0,
            live: LiveModel { available: true, ..LiveModel::default() },
            widgets: WidgetsModel { available: true, ..WidgetsModel::default() },
        }
    }

    #[test]
    fn the_widgets_tab() {
        let Ok(mut g) = Gfx::new() else { return };
        g.resize(620, 1400);
        let mut st = Settings::default();
        for k in [WKind::Digital, WKind::Weather, WKind::Photos, WKind::Note, WKind::ClockCalendar] {
            st.widgets.add(k);
        }
        let status = [(2, "City not found: Xyz".to_string())];
        let m = Model { widgets: WidgetsModel { available: true, tray: true, status: &status, country: "PT" }, ..model(&st, Tab::Widgets) };
        let d = draw(&mut g, &m, &Theme::new(false, None), 1.0, Rect::new(0.0, 0.0, 620.0, 1400.0));
        for h in [
            Hit::WidgetsOn,
            Hit::AddWidget(WKind::Fence),
            Hit::AddWidget(WKind::Photos),
            Hit::WidgetSeconds(1),
            Hit::WidgetH24(1),
            Hit::Field(Field::City(2)),
            Hit::WidgetUnits(2, true),
            Hit::WidgetFolder(3),
            Hit::WidgetMore(3),
            Hit::WidgetTop(4),
            Hit::WidgetRemove(4),
            Hit::WidgetH24(5),
            Hit::WidgetHolidays(5),
            Hit::Autostart,
        ] {
            let (r, _) = d.hits.iter().find(|(_, x)| *x == h).unwrap_or_else(|| panic!("{h:?} not drawn"));
            assert_eq!(d.hit(r.x + r.w / 2.0, r.y + r.h / 2.0), Some(h));
        }
        // One digital clock is enough; notes can be many.
        assert!(!d.hits.iter().any(|(_, h)| *h == Hit::AddWidget(WKind::Digital)));
        assert!(d.hits.iter().any(|(_, h)| *h == Hit::AddWidget(WKind::Note)));
        // Elsewhere than Windows, only a word.
        let m = Model { widgets: WidgetsModel::default(), ..model(&st, Tab::Widgets) };
        let d = draw(&mut g, &m, &Theme::new(false, None), 1.0, Rect::new(0.0, 0.0, 620.0, 1000.0));
        assert_eq!(d.hits.iter().filter(|(_, h)| !matches!(h, Hit::Tab(_) | Hit::Autostart)).count(), 0);
    }

    #[test]
    fn controls_are_where_they_were_drawn() {
        let Ok(mut g) = Gfx::new() else { return };
        g.resize(620, 900);
        let st = Settings { folders: vec!["C:\\Fotos".into(), "D:\\Wallpapers".into()], ..Settings::default() };
        let m = model(&st, Tab::Static);
        let d = draw(&mut g, &m, &Theme::new(false, None), 1.0, Rect::new(0.0, 0.0, 620.0, 900.0));
        for h in [
            Hit::Tab(Tab::Dynamic),
            Hit::Next,
            Hit::OpenFolder,
            Hit::Frequency(Frequency::Weekly),
            Hit::CustomMore,
            Hit::Source(Source::Folders),
            Hit::RemoveFolder(1),
            Hit::Fit(Fit::Span),
            Hit::Autostart,
        ] {
            let (r, _) = d.hits.iter().find(|(_, x)| *x == h).unwrap_or_else(|| panic!("{h:?} not drawn"));
            assert_eq!(d.hit(r.x + r.w / 2.0, r.y + r.h / 2.0), Some(h));
        }
        // Nothing to say about a picture yet.
        assert!(!d.hits.iter().any(|(_, h)| *h == Hit::About));
        assert!(d.content_h > 600.0 && d.content_h < 950.0, "{}", d.content_h);
        // Scrolled to the end, the last control is still inside, and the
        // tabs still on top.
        let m2 = Model { scroll: d.content_h - 500.0, ..model(&st, Tab::Static) };
        let d2 = draw(&mut g, &m2, &Theme::new(true, None), 1.0, Rect::new(0.0, 0.0, 620.0, 500.0));
        assert!(d2.hits.iter().any(|(_, h)| *h == Hit::Autostart));
        assert!(!d2.hits.iter().any(|(_, h)| *h == Hit::Next));
        let (r, _) = d2.hits.iter().find(|(_, h)| *h == Hit::Tab(Tab::Static)).unwrap();
        assert_eq!(d2.hit(r.x + r.w / 2.0, r.y + r.h / 2.0), Some(Hit::Tab(Tab::Static)));
    }

    #[test]
    fn the_dynamic_tab() {
        let Ok(mut g) = Gfx::new() else { return };
        g.resize(620, 1000);
        let mut st = Settings::default();
        st.live.kind = Kind::Folder;
        let m = model(&st, Tab::Dynamic);
        let d = draw(&mut g, &m, &Theme::new(false, None), 1.0, Rect::new(0.0, 0.0, 620.0, 1000.0));
        for h in [
            Hit::LiveKind(Kind::Video),
            Hit::LiveChoose(Kind::Anim),
            Hit::LiveScene(Scene::Sea),
            Hit::LiveSubfolders,
            Hit::LiveFit(LiveFit::Fit),
            Hit::LivePause(PauseWhen::Battery),
            Hit::Autostart,
        ] {
            let (r, _) = d.hits.iter().find(|(_, x)| *x == h).unwrap_or_else(|| panic!("{h:?} not drawn"));
            assert_eq!(d.hit(r.x + r.w / 2.0, r.y + r.h / 2.0), Some(h));
        }
        // No folder yet: nothing to turn on, no other video.
        assert!(!d.hits.iter().any(|(_, h)| matches!(h, Hit::LiveOn | Hit::LiveNext)));
        assert!(!d.hits.iter().any(|(_, h)| *h == Hit::Next));
        // A scene can always play.
        st.live.kind = Kind::Scene;
        let d = draw(&mut g, &model(&st, Tab::Dynamic), &Theme::new(true, None), 1.0, Rect::new(0.0, 0.0, 620.0, 1000.0));
        assert!(d.hits.iter().any(|(_, h)| *h == Hit::LiveOn));
        // Elsewhere than Windows, only a word.
        let m = Model { live: LiveModel::default(), ..model(&st, Tab::Dynamic) };
        let d = draw(&mut g, &m, &Theme::new(false, None), 1.0, Rect::new(0.0, 0.0, 620.0, 1000.0));
        assert_eq!(d.hits.iter().filter(|(_, h)| !matches!(h, Hit::Tab(_) | Hit::Autostart)).count(), 0);
    }
}
