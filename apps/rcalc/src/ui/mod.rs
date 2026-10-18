//! Immediate-mode UI on top of the software renderer.
//!
//! Each frame, `view::build` lays out and paints the whole window and
//! registers every interactive element as a [`Widget`]. The event loop uses
//! that list for hit testing, keyboard focus and the accessibility tree, and
//! dispatches the widget's [`Action`] when it is activated.

pub mod gfx;
pub mod icons;
pub mod theme;
pub mod view;

#[cfg(test)]
mod screens;

use crate::app::{Action, ScrollId};
use gfx::{Align, Color, FontId, Gfx, Rect};
use icons::Icon;
use std::hash::{Hash, Hasher};
use theme::Theme;

pub type WId = u64;

pub fn wid(a: &Action) -> WId {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    a.hash(&mut h);
    h.finish() | 1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Button,
    Toggle,
    Radio,
    Tab,
    ListItem,
    MenuItem,
    ComboBox,
    /// A focusable text field (converter values).
    Field,
    Label,
    /// The main result display (live region).
    Display,
    /// Modal backdrop: catches clicks outside an overlay.
    Scrim,
}

#[derive(Clone, Debug)]
pub struct Widget {
    pub id: WId,
    pub rect: Rect,
    pub action: Action,
    pub role: Role,
    pub label: String,
    pub value: Option<String>,
    pub enabled: bool,
    pub checked: Option<bool>,
    pub layer: u8,
}

impl Widget {
    pub fn focusable(&self) -> bool {
        self.enabled && !matches!(self.role, Role::Label | Role::Display | Role::Scrim)
    }

    pub fn clickable(&self) -> bool {
        self.enabled && !matches!(self.role, Role::Label | Role::Display)
    }
}

/// Pointer and keyboard state the painter needs.
#[derive(Default, Clone, Debug)]
pub struct Input {
    pub hot: Option<WId>,
    pub active: Option<WId>,
    pub focus: Option<WId>,
    pub focus_visible: bool,
}

pub struct ScrollArea {
    pub id: ScrollId,
    pub rect: Rect,
    pub max: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// Number keys (brightest).
    Num,
    /// Operator and function keys.
    Op,
    /// The `=` key.
    Accent,
    /// Transparent until hovered (memory row, header icons, list items).
    Subtle,
}

pub enum Content<'s> {
    Text(&'s str, f32, FontId),
    Icon(Icon, f32),
    /// Text left-aligned with a trailing chevron (combo boxes, flyout openers).
    Dropdown(Option<Icon>, &'s str, f32),
    /// Base text with a smaller subscript (e.g. log base y).
    Sub(&'s str, &'s str, &'s str, f32),
    None,
}

pub struct Button<'s> {
    pub action: Action,
    pub rect: Rect,
    pub label: &'s str,
    pub content: Content<'s>,
    pub style: Style,
    pub enabled: bool,
    pub role: Role,
    pub checked: Option<bool>,
}

impl<'s> Button<'s> {
    pub fn new(action: Action, rect: Rect, label: &'s str, content: Content<'s>, style: Style) -> Button<'s> {
        Button {
            action,
            rect,
            label,
            content,
            style,
            enabled: true,
            role: Role::Button,
            checked: None,
        }
    }
    pub fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }
    pub fn role(mut self, r: Role) -> Self {
        self.role = r;
        self
    }
    pub fn checked(mut self, c: bool) -> Self {
        self.checked = Some(c);
        self
    }
}

pub struct Ui<'a> {
    pub g: &'a mut Gfx,
    pub t: Theme,
    /// Logical-to-physical scale factor.
    pub s: f32,
    pub inp: &'a Input,
    pub widgets: Vec<Widget>,
    pub scrolls: Vec<ScrollArea>,
    pub layer: u8,
    clip: Option<Rect>,
    label_seq: u64,
}

impl<'a> Ui<'a> {
    pub fn new(g: &'a mut Gfx, t: Theme, s: f32, inp: &'a Input) -> Ui<'a> {
        Ui {
            g,
            t,
            s,
            inp,
            widgets: Vec::with_capacity(96),
            scrolls: Vec::new(),
            layer: 0,
            clip: None,
            label_seq: 0,
        }
    }

    pub fn px(&self, v: f32) -> f32 {
        (v * self.s).round()
    }

    /// Unrounded scale (font sizes).
    pub fn fs(&self, v: f32) -> f32 {
        v * self.s
    }

    pub fn width(&self) -> f32 {
        self.g.width()
    }

    pub fn height(&self) -> f32 {
        self.g.height()
    }

    pub fn set_clip(&mut self, r: Option<Rect>) {
        self.clip = r;
        self.g.set_clip(r);
    }

    pub fn register(&mut self, mut w: Widget) {
        if let Some(c) = self.clip {
            w.rect = w.rect.intersect(&c);
            if w.rect.w <= 0.0 || w.rect.h <= 0.0 {
                return;
            }
        }
        w.layer = self.layer;
        self.widgets.push(w);
    }

    /// Registers static text for accessibility. IDs follow paint order so
    /// they stay stable while the text changes (e.g. the live display).
    pub fn label(&mut self, rect: Rect, text: &str, role: Role) {
        self.label_seq += 1;
        self.register(Widget {
            id: fxhash("label") ^ self.label_seq.wrapping_mul(0x9E37_79B9_7F4A_7C15),
            rect,
            action: Action::None,
            role,
            label: text.to_string(),
            value: None,
            enabled: true,
            checked: None,
            layer: 0,
        });
    }

    /// A full-window backdrop for overlays; clicking it runs `action`.
    pub fn scrim(&mut self, rect: Rect, action: Action, dim: bool) {
        if dim {
            let c = self.t.scrim;
            self.g.fill_rect(rect, c);
        }
        self.register(Widget {
            id: wid(&action) ^ 0x5C,
            rect,
            action,
            role: Role::Scrim,
            label: String::new(),
            value: None,
            enabled: true,
            checked: None,
            layer: 0,
        });
    }

    pub fn state(&self, id: WId) -> (bool, bool, bool) {
        let hover = self.inp.hot == Some(id);
        let pressed = hover && self.inp.active == Some(id);
        let focused = self.inp.focus == Some(id) && self.inp.focus_visible;
        (hover, pressed, focused)
    }

    pub fn button(&mut self, b: Button) {
        let id = wid(&b.action);
        let (hover, pressed, focused) = self.state(id);
        let (hover, pressed) = if b.enabled { (hover, pressed) } else { (false, false) };
        let t = self.t;
        let rad = self.px(4.0);
        let r = b.rect;
        let bg = match b.style {
            Style::Num => {
                if pressed {
                    t.num_pressed
                } else if hover {
                    t.num_hover
                } else {
                    t.num
                }
            }
            Style::Op => {
                if pressed {
                    t.op_pressed
                } else if hover {
                    t.op_hover
                } else {
                    t.op
                }
            }
            Style::Accent => {
                if !b.enabled {
                    t.op
                } else if pressed {
                    t.accent_pressed
                } else if hover {
                    t.accent_hover
                } else {
                    t.accent
                }
            }
            Style::Subtle => {
                if pressed {
                    t.subtle_pressed
                } else if hover {
                    t.subtle_hover
                } else {
                    Color::TRANSPARENT
                }
            }
        };
        self.g.fill_rrect(r, rad, bg);
        if matches!(b.style, Style::Num | Style::Op) {
            self.g.stroke_rrect(r, rad, 1.0, t.border);
        }
        let fg = if !b.enabled {
            t.text_disabled
        } else if b.style == Style::Accent {
            t.accent_text
        } else if pressed {
            t.text_secondary
        } else {
            t.text
        };
        self.paint_content(&b.content, r, fg);
        if focused {
            self.focus_ring(r);
        }
        self.register(Widget {
            id,
            rect: r,
            action: b.action,
            role: b.role,
            label: b.label.to_string(),
            value: None,
            enabled: b.enabled,
            checked: b.checked,
            layer: 0,
        });
    }

    pub fn focus_ring(&mut self, r: Rect) {
        let rad = self.px(4.0);
        let w = self.px(2.0).max(2.0);
        let (t, bg) = (self.t.focus, self.t.bg);
        self.g.stroke_rrect(r, rad, w, t);
        self.g.stroke_rrect(r.inset(w), (rad - w).max(0.0), 1.0, bg);
    }

    pub fn paint_content(&mut self, c: &Content, r: Rect, fg: Color) {
        match c {
            Content::Text(s, size, font) => {
                let px = self.fs(*size);
                let px = self.g.fit_size(s, r.w - self.px(8.0), px, px * 0.6, *font);
                self.g.text_in(s, r, px, *font, fg, Align::Center);
            }
            Content::Icon(icon, size) => {
                let px = self.fs(*size);
                icons::draw(self.g, *icon, r, px, fg);
            }
            Content::Dropdown(icon, s, size) => {
                let px = self.fs(*size);
                let pad = self.px(12.0);
                let mut inner = Rect::new(r.x + pad, r.y, r.w - 2.0 * pad, r.h);
                let chev = inner.take_right(self.px(16.0));
                if let Some(i) = icon {
                    let ir = inner.take_left(self.px(22.0));
                    icons::draw(self.g, *i, ir, px, fg);
                    inner.take_left(self.px(4.0));
                }
                let fit = self.g.fit_size(s, inner.w, px, px * 0.7, FontId::Regular);
                self.g.set_clip(Some(match self.clip {
                    Some(c) => inner.intersect(&c),
                    None => inner,
                }));
                self.g.text_in(s, inner, fit, FontId::Regular, fg, Align::Left);
                self.g.set_clip(self.clip);
                icons::draw(self.g, Icon::ChevronDown, chev, self.fs(10.0), fg);
            }
            Content::Sub(base, sub, tail, size) => {
                let px = self.fs(*size);
                let small = px * 0.62;
                let w = self.g.measure(base, px, FontId::Regular)
                    + self.g.measure(sub, small, FontId::Regular)
                    + self.g.measure(tail, px, FontId::Regular);
                let (asc, desc) = self.g.metrics(px, FontId::Regular);
                let baseline = r.center_y() + (asc + desc) / 2.0;
                let mut x = (r.x + (r.w - w) / 2.0).round();
                x += self.g.text(base, x, baseline, px, FontId::Regular, fg);
                x += self.g.text(sub, x, baseline + px * 0.18, small, FontId::Regular, fg);
                self.g.text(tail, x, baseline, px, FontId::Regular, fg);
            }
            Content::None => {}
        }
    }

    pub fn text(&mut self, s: &str, r: Rect, size: f32, font: FontId, c: Color, align: Align) -> f32 {
        let px = self.fs(size);
        self.g.text_in(s, r, px, font, c, align)
    }

    /// Text that shrinks to fit the rectangle width.
    #[allow(clippy::too_many_arguments)]
    pub fn fit_text(&mut self, s: &str, r: Rect, max: f32, min: f32, font: FontId, c: Color, align: Align) {
        let px = self.g.fit_size(s, r.w, self.fs(max), self.fs(min), font);
        if self.g.measure(s, px, font) > r.w {
            // Still too wide at the minimum size: clip, keeping the end visible.
            let prev = self.clip;
            let clip = match prev {
                Some(p) => r.intersect(&p),
                None => r,
            };
            self.g.set_clip(Some(clip));
            self.g.text_in(s, r, px, font, c, Align::Right);
            self.g.set_clip(prev);
        } else {
            self.g.text_in(s, r, px, font, c, align);
        }
    }

    /// Word-wraps `s` to `width` pixels.
    pub fn wrap(&mut self, s: &str, width: f32, size: f32, font: FontId) -> Vec<String> {
        let px = self.fs(size);
        let mut lines = Vec::new();
        for para in s.split('\n') {
            let mut line = String::new();
            for word in para.split(' ') {
                let candidate = if line.is_empty() {
                    word.to_string()
                } else {
                    format!("{line} {word}")
                };
                if self.g.measure(&candidate, px, font) > width && !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    line = word.to_string();
                } else {
                    line = candidate;
                }
            }
            lines.push(line);
        }
        lines
    }

    /// Paints wrapped text starting at `r.y`; returns the height used.
    pub fn paragraph(&mut self, s: &str, r: Rect, size: f32, font: FontId, c: Color) -> f32 {
        let lines = self.wrap(s, r.w, size, font);
        let lh = self.px(size * 1.45);
        let mut y = r.y;
        for l in &lines {
            self.text(l, Rect::new(r.x, y, r.w, lh), size, font, c, Align::Left);
            y += lh;
        }
        if !s.is_empty() {
            self.label(Rect::new(r.x, r.y, r.w, y - r.y), s, Role::Label);
        }
        y - r.y
    }

    /// Registers a wheel-scrollable region and draws a thin position
    /// indicator when the content overflows.
    pub fn scroll_area(&mut self, id: ScrollId, rect: Rect, content_h: f32, offset: f32) {
        let max = (content_h - rect.h).max(0.0);
        if max > 0.0 {
            let track = rect.h - self.px(8.0);
            let thumb = (track * rect.h / content_h).max(self.px(24.0));
            let y = rect.y + self.px(4.0) + (track - thumb) * (offset.min(max) / max);
            let bar = Rect::new(rect.right() - self.px(5.0), y, self.px(2.0), thumb);
            let c = self.t.text_secondary.mix(self.t.bg, 0.4);
            self.g.fill_rrect(bar, self.px(1.0), c);
        }
        self.scrolls.push(ScrollArea { id, rect, max });
    }

    /// Flyout surface with border and a soft shadow.
    pub fn flyout(&mut self, r: Rect) {
        let rad = self.px(8.0);
        let t = self.t;
        for i in 1..=4 {
            let d = i as f32 * self.s;
            let shadow = Color::rgba(0x000000, if t.dark { 0x18 } else { 0x0C });
            self.g
                .fill_rrect(Rect::new(r.x - d * 0.5, r.y + d * 0.5, r.w + d, r.h + d), rad + d, shadow);
        }
        self.g.fill_rrect(r, rad, t.flyout);
        self.g.stroke_rrect(r, rad, 1.0, t.flyout_border);
    }
}

fn fxhash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}
