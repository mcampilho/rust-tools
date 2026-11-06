//! Software-rendered UI: renderer (`gfx`), layout helpers, widgets and the
//! window view. Each frame the view paints everything and registers the
//! interactive elements as widgets used for hit-testing, tooltips and the
//! accessibility tree.

pub mod appicon;
pub mod gfx;
pub mod layout;
pub mod lineedit;
pub mod markdown;
#[cfg(not(windows))]
pub mod menu;
pub mod theme;
pub mod view;

use crate::app::commands::Command;
use crate::app::Focus;
use gfx::{Align, Color, FontId, Gfx, Rect};
use lineedit::LineEdit;
use std::hash::{Hash, Hasher};
use theme::Theme;

pub type WId = u64;

/// What a widget does when activated.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum UiAction {
    Cmd(Command),
    Tab(usize),
    CloseTab(usize),
    NewTab,
    TabScroll(i8),
    FindOpt(u8),
    FindNext,
    FindPrev,
    ReplaceOne,
    ReplaceAll,
    CloseFind,
    ToggleReplace,
    Field(Focus),
    NoticeReload,
    NoticeKeep,
    NoticeDismiss,
    NoticeSaveAs,
    PaletteItem(usize),
    Result(usize),
    CloseResults,
    FifSearch,
    FifStop,
    FifBrowse,
    /// Status bar segment (opens a menu): 0 language, 1 encoding, 2 EOL, 3 indentation, 4 position.
    Status(u8),
    Outline(usize),
    /// Top-level menu drawn in the window.
    #[cfg(not(windows))]
    MenuTitle(usize),
    /// Row of an open popup: (level, item).
    #[cfg(not(windows))]
    MenuRow(usize, usize),
    Editor,
    Label(u32),
    None,
}

pub fn wid(a: &UiAction) -> WId {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    a.hash(&mut h);
    h.finish() | 1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Button,
    Toggle,
    Tab,
    TextField,
    ListItem,
    Label,
    Editor,
}

#[derive(Clone, Debug)]
pub struct Widget {
    pub id: WId,
    pub rect: Rect,
    pub action: UiAction,
    pub role: Role,
    pub label: String,
    pub tooltip: Option<String>,
    pub value: Option<String>,
    pub enabled: bool,
    pub checked: Option<bool>,
    pub layer: u8,
}

/// Pointer state the painter needs.
#[derive(Default, Clone, Debug)]
pub struct Input {
    pub hot: Option<WId>,
    pub active: Option<WId>,
    /// Widget whose tooltip should be shown.
    pub tooltip: Option<WId>,
    pub caret_on: bool,
    pub focused_window: bool,
}

pub struct Ui<'a> {
    pub g: &'a mut Gfx,
    pub t: Theme,
    pub s: f32,
    pub inp: &'a Input,
    pub widgets: Vec<Widget>,
    pub layer: u8,
    label_seq: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Transparent until hovered.
    Flat,
    /// Bordered push button.
    Push,
    /// Accent-filled primary button.
    Primary,
}

impl<'a> Ui<'a> {
    pub fn new(g: &'a mut Gfx, t: Theme, s: f32, inp: &'a Input) -> Ui<'a> {
        Ui { g, t, s, inp, widgets: Vec::with_capacity(128), layer: 0, label_seq: 0 }
    }

    pub fn px(&self, v: f32) -> f32 {
        (v * self.s).round()
    }

    pub fn fs(&self, v: f32) -> f32 {
        v * self.s
    }

    pub fn register(&mut self, w: Widget) {
        let mut w = w;
        w.layer = self.layer;
        self.widgets.push(w);
    }

    /// Accessible static text.
    pub fn label(&mut self, rect: Rect, text: &str) {
        self.label_seq += 1;
        let action = UiAction::Label(self.label_seq);
        self.register(Widget {
            id: wid(&action),
            rect,
            action,
            role: Role::Label,
            label: text.to_string(),
            tooltip: None,
            value: None,
            enabled: true,
            checked: None,
            layer: 0,
        });
    }

    fn state(&self, id: WId) -> (bool, bool) {
        let hover = self.inp.hot == Some(id);
        (hover, hover && self.inp.active == Some(id))
    }

    /// A button with text or an icon glyph. `icon` is a Segoe MDL2 code point
    /// used when the icon font exists, otherwise `text` is drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn button(&mut self, action: UiAction, r: Rect, text: &str, icon: Option<char>, label: &str, tooltip: Option<&str>, style: Style, checked: Option<bool>, enabled: bool) {
        let id = wid(&action);
        let (hover, pressed) = self.state(id);
        let t = self.t;
        let rad = self.px(4.0);
        let on = checked == Some(true);
        let bg = match style {
            Style::Primary => {
                if pressed {
                    t.accent.mix(t.window, 0.25)
                } else if hover {
                    t.accent.mix(t.window, 0.12)
                } else {
                    t.accent
                }
            }
            _ => {
                if pressed {
                    t.pressed
                } else if hover && enabled {
                    t.hover
                } else if on {
                    t.accent.with_alpha(0x38)
                } else {
                    Color::TRANSPARENT
                }
            }
        };
        if style == Style::Push {
            self.g.fill_rrect(r, rad, t.field);
            self.g.fill_rrect(r, rad, bg);
            self.g.stroke_rrect(r, rad, 1.0, t.field_border);
        } else {
            self.g.fill_rrect(r, rad, bg);
        }
        if on && style == Style::Flat {
            self.g.stroke_rrect(r, rad, 1.0, t.accent.with_alpha(0x90));
        }
        let fg = if !enabled {
            t.text_dim.with_alpha(0x80)
        } else if style == Style::Primary {
            t.accent_text
        } else {
            t.text
        };
        match icon.filter(|_| self.g.has_icons()) {
            Some(ch) => {
                let px = self.fs(13.0);
                self.g.text_in(&ch.to_string(), r, px, FontId::Icons, fg, Align::Center);
            }
            None => {
                let px = self.g.fit_size(text, r.w - self.px(8.0), self.fs(12.5), self.fs(9.0), FontId::Regular);
                self.g.text_in(text, r, px, FontId::Regular, fg, Align::Center);
            }
        }
        self.register(Widget {
            id,
            rect: r,
            action,
            role: if checked.is_some() { Role::Toggle } else { Role::Button },
            label: label.to_string(),
            tooltip: tooltip.map(str::to_string),
            value: None,
            enabled,
            checked,
            layer: 0,
        });
    }

    /// A single-line text field. Keeps the caret visible by scrolling.
    #[allow(clippy::too_many_arguments)]
    pub fn field(&mut self, action: UiAction, r: Rect, edit: &mut LineEdit, focused: bool, placeholder: &str, label: &str, error: bool) {
        let t = self.t;
        let rad = self.px(4.0);
        self.g.fill_rrect(r, rad, t.field);
        let border = if error {
            t.error
        } else if focused {
            t.accent
        } else {
            t.field_border
        };
        self.g.stroke_rrect(r, rad, if focused { 2.0 } else { 1.0 }, border);
        let pad = self.px(6.0);
        let inner = Rect::new(r.x + pad, r.y, r.w - 2.0 * pad, r.h);
        let px = self.fs(13.0);
        let font = FontId::Regular;
        let before: String = edit.text.chars().take(edit.caret).collect();
        let caret_x = self.g.measure(&before, px, font);
        if caret_x - edit.scroll > inner.w - 2.0 {
            edit.scroll = caret_x - inner.w + 2.0;
        } else if caret_x < edit.scroll {
            edit.scroll = caret_x;
        }
        let total = self.g.measure(&edit.text, px, font);
        if total - edit.scroll < inner.w {
            edit.scroll = (total - inner.w).max(0.0);
        }
        self.g.set_clip(Some(inner));
        let (asc, desc) = self.g.metrics(px, font);
        let base = inner.center_y() + (asc + desc) / 2.0;
        let x0 = inner.x - edit.scroll;
        if edit.text.is_empty() && !placeholder.is_empty() {
            self.g.text(placeholder, inner.x, base, px, font, t.text_dim);
        } else {
            let (a, b) = edit.selection();
            if a != b && focused {
                let sa: String = edit.text.chars().take(a).collect();
                let sb: String = edit.text.chars().take(b).collect();
                let (xa, xb) = (self.g.measure(&sa, px, font), self.g.measure(&sb, px, font));
                self.g.fill_rect(Rect::new(x0 + xa, inner.y + self.px(4.0), xb - xa, inner.h - self.px(8.0)), t.selection);
            }
            self.g.text(&edit.text, x0, base, px, font, t.text);
        }
        if focused && self.inp.caret_on {
            self.g.fill_rect(Rect::new((x0 + caret_x).round(), inner.y + self.px(6.0), self.px(1.0).max(1.0), inner.h - self.px(12.0)), t.caret);
        }
        self.g.set_clip(None);
        self.register(Widget {
            id: wid(&action),
            rect: r,
            action,
            role: Role::TextField,
            label: label.to_string(),
            tooltip: None,
            value: Some(edit.text.clone()),
            enabled: true,
            checked: None,
            layer: 0,
        });
    }

    pub fn text(&mut self, s: &str, r: Rect, size: f32, font: FontId, c: Color, align: Align) -> f32 {
        let px = self.fs(size);
        self.g.text_in(s, r, px, font, c, align)
    }

    /// Text clipped to `r` (with an ellipsis when too long).
    pub fn text_clip(&mut self, s: &str, r: Rect, size: f32, font: FontId, c: Color, align: Align) {
        let px = self.fs(size);
        let w = self.g.measure(s, px, font);
        if w <= r.w {
            self.g.text_in(s, r, px, font, c, align);
            return;
        }
        let ell = "…";
        let ew = self.g.measure(ell, px, font);
        let mut out = String::new();
        let mut acc = 0.0;
        for ch in s.chars() {
            let cw = self.g.advance(ch, px, font);
            if acc + cw + ew > r.w {
                break;
            }
            acc += cw;
            out.push(ch);
        }
        out.push_str(ell);
        self.g.text_in(&out, r, px, font, c, Align::Left);
    }

    /// Popup surface with border and shadow.
    pub fn popup(&mut self, r: Rect) {
        let rad = self.px(8.0);
        let t = self.t;
        for i in 1..=4 {
            let d = i as f32 * self.s;
            self.g.fill_rrect(Rect::new(r.x - d * 0.5, r.y + d * 0.5, r.w + d, r.h + d), rad + d, Color::rgba(0x000000, if t.dark { 0x20 } else { 0x10 }));
        }
        self.g.fill_rrect(r, rad, t.popup);
        self.g.stroke_rrect(r, rad, 1.0, t.border);
    }

    /// Draws the tooltip of the widget under `inp.tooltip`, if any.
    pub fn draw_tooltip(&mut self, mouse: (f32, f32)) {
        let Some(id) = self.inp.tooltip else { return };
        let Some(w) = self.widgets.iter().find(|w| w.id == id) else { return };
        let Some(tip) = w.tooltip.clone() else { return };
        let rect = w.rect;
        let px = self.fs(12.0);
        let tw = self.g.measure(&tip, px, FontId::Regular) + self.px(16.0);
        let h = self.px(26.0);
        let (gw, gh) = (self.g.width(), self.g.height());
        let x = mouse.0.min(gw - tw - self.px(4.0)).max(self.px(4.0)).round();
        let mut y = rect.bottom() + self.px(6.0);
        if y + h > gh {
            y = rect.y - h - self.px(6.0);
        }
        let r = Rect::new(x, y.round(), tw, h);
        self.popup(r);
        let t = self.t;
        self.g.text_in(&tip, r, px, FontId::Regular, t.text, Align::Center);
    }
}
