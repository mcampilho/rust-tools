//! Maths laid out as boxes: stacked fractions, roots with a bar over them, and operators with limits above
//! and below. Each box is measured once (`Ctx::math_atom`), then drawn as ordinary text items and rules, so
//! selection and copying keep working. A row is text runs and nested boxes side by side on one baseline.

use super::{font_of, Ctx, Item, Meta};
use crate::rich::{Inline, Style};
use crate::ui::gfx::{FontId, Rect};

/// A piece of text with its style and width.
type Run = (String, Style, f32);

/// What sits on one baseline: runs and boxes, with the room they need above and below it.
#[derive(Default)]
pub struct Row {
    pieces: Vec<Piece>,
    w: f32,
    asc: f32,
    desc: f32,
}

enum Piece {
    Run(Run),
    Frac(Frac),
    Root(Box<Root>),
}

pub struct Frac {
    num: Row,
    den: Row,
    w: f32,
    px: f32,
    style: Style,
}

pub struct Root {
    sign: Run,
    /// The radical sign's baseline, below the row's.
    dy: f32,
    sign_w: f32,
    row: Row,
    w: f32,
    px: f32,
    style: Style,
}

pub struct Limits {
    top: Row,
    base: Run,
    bottom: Row,
    w: f32,
    px: f32,
    base_asc: f32,
    base_desc: f32,
}

pub enum Boxed {
    Frac(Frac),
    Root(Box<Root>),
    Limits(Box<Limits>),
}

impl Frac {
    /// Distance from the baseline up to the numerator's baseline (the bar is on the maths axis).
    fn up(&self) -> f32 {
        self.px * 0.45 + self.num.desc
    }

    fn down(&self) -> f32 {
        self.den.asc - self.px * 0.08
    }

    pub fn asc(&self) -> f32 {
        self.up() + self.num.asc
    }

    pub fn desc(&self) -> f32 {
        self.down() + self.den.desc
    }
}

impl Root {
    fn asc(&self) -> f32 {
        self.row.asc + self.px * 0.2
    }
}

impl Row {
    /// Room taken above or below an operator by a row of limits.
    fn extra(&self, px: f32) -> f32 {
        if self.pieces.is_empty() {
            0.0
        } else {
            px * 0.1 + self.asc + self.desc
        }
    }
}

impl Boxed {
    pub fn width(&self) -> f32 {
        match self {
            Boxed::Frac(f) => f.w,
            Boxed::Root(r) => r.w,
            Boxed::Limits(l) => l.w,
        }
    }

    pub fn asc(&self) -> f32 {
        match self {
            Boxed::Frac(f) => f.asc(),
            Boxed::Root(r) => r.asc(),
            Boxed::Limits(l) => l.base_asc + l.top.extra(l.px),
        }
    }

    pub fn desc(&self) -> f32 {
        match self {
            Boxed::Frac(f) => f.desc(),
            Boxed::Root(r) => r.row.desc,
            Boxed::Limits(l) => l.base_desc + l.bottom.extra(l.px),
        }
    }
}

fn first_style(v: &[Inline]) -> Option<Style> {
    v.iter().find_map(|i| match i {
        Inline::Text(_, s) => Some(*s),
        Inline::Frac(a, b) => first_style(a).or_else(|| first_style(b)),
        Inline::Root(b) => first_style(b),
        Inline::Limits(t, b, u) => first_style(b).or_else(|| first_style(t)).or_else(|| first_style(u)),
        _ => None,
    })
}

impl Ctx<'_> {
    /// The box for a maths inline, or `None` for anything else.
    pub fn math_atom(&mut self, i: &Inline) -> Option<Boxed> {
        match i {
            Inline::Frac(n, d) => Some(Boxed::Frac(self.frac(n, d))),
            Inline::Root(b) => Some(Boxed::Root(Box::new(self.root(b)))),
            Inline::Limits(t, b, u) => self.limits(t, b, u).map(|l| Boxed::Limits(Box::new(l))),
            _ => None,
        }
    }

    fn row_of(&mut self, v: &[Inline]) -> Row {
        let mut row = Row::default();
        for i in v {
            let (piece, w, asc, desc) = match i {
                Inline::Text(t, s) => {
                    let px = self.px(s);
                    let w = self.g.measure(t, px, font_of(s));
                    let shift = s.valign as f32 * px * 0.35;
                    (Piece::Run((t.clone(), *s, w)), w, px * 0.78 + shift.max(0.0), px * 0.22 - shift.min(0.0))
                }
                Inline::Frac(n, d) => {
                    let f = self.frac(n, d);
                    let (w, asc, desc) = (f.w, f.asc(), f.desc());
                    (Piece::Frac(f), w, asc, desc)
                }
                Inline::Root(b) => {
                    let r = self.root(b);
                    let (w, asc, desc) = (r.w, r.asc(), r.row.desc);
                    (Piece::Root(Box::new(r)), w, asc, desc)
                }
                _ => continue,
            };
            row.w += w;
            row.asc = row.asc.max(asc);
            row.desc = row.desc.max(desc);
            row.pieces.push(piece);
        }
        row
    }

    fn frac(&mut self, n: &[Inline], d: &[Inline]) -> Frac {
        let style = Style { valign: 0, ..first_style(n).or_else(|| first_style(d)).unwrap_or_default() };
        let (num, den) = (self.row_of(n), self.row_of(d));
        let px = self.px(&style);
        Frac { w: num.w.max(den.w) + px * 0.3, num, den, px, style }
    }

    fn root(&mut self, body: &[Inline]) -> Root {
        let row = self.row_of(body);
        let style = Style { valign: 0, ..first_style(body).unwrap_or_default() };
        let px = self.px(&style);
        // The radical sign spans the body: top at the bar, bottom at the body's lowest point.
        let sign_px = ((row.asc + row.desc + px * 0.15) / 0.95).max(px);
        let sign_style = Style { size: style.size * sign_px / px, italic: false, bold: false, ..style };
        let sign_w = self.g.measure("√", self.px(&sign_style), FontId::Regular);
        Root {
            sign: ("√".to_string(), sign_style, sign_w),
            dy: row.desc - sign_px * 0.1,
            sign_w,
            w: sign_w + row.w + px * 0.15,
            row,
            px,
            style,
        }
    }

    fn limits(&mut self, top: &[Inline], base: &[Inline], bottom: &[Inline]) -> Option<Limits> {
        let (t, s) = base.iter().find_map(|i| if let Inline::Text(t, s) = i { Some((t.clone(), *s)) } else { None })?;
        let px = self.px(&s);
        let bw = self.g.measure(&t, px, font_of(&s));
        let (top, bottom) = (self.row_of(top), self.row_of(bottom));
        Some(Limits { w: top.w.max(bottom.w).max(bw), top, base: (t, s, bw), bottom, px, base_asc: px * 0.8, base_desc: px * 0.22 })
    }

    /// Draws one run; the first run of a box takes the line's wrap and break marks (`lead` is its gap flag),
    /// and `sep` marks a run that starts a new part of the box (copied after a space).
    fn emit_run(&mut self, run: &Run, x: f32, y: f32, lead: &mut Option<bool>, sep: bool) {
        let (t, s, tw) = run;
        let (px, font) = (self.px(s), font_of(s));
        let by = y - s.valign as f32 * px * 0.35;
        let color = self.color(s);
        let idx = self.out.push(Item::Text { x, y: by, s: t.clone(), px, font, color });
        let (wrapped, space, breaks) = match lead.take() {
            Some(gap) => {
                let wrapped = std::mem::take(&mut self.lead_space);
                (wrapped, wrapped || gap || sep, std::mem::take(&mut self.lead_breaks))
            }
            None => (false, sep, 0),
        };
        self.out.meta[idx] = Meta { node: self.node, style: *s, w: *tw, wrapped, space, breaks, ..Meta::default() };
    }

    fn emit_row(&mut self, row: &Row, x: f32, base: f32, lead: &mut Option<bool>, mut sep: bool) {
        let mut x = x;
        for p in &row.pieces {
            x += match p {
                Piece::Run(r) => {
                    self.emit_run(r, x, base, lead, sep);
                    r.2
                }
                Piece::Frac(f) => {
                    self.emit_frac(f, x, base, lead, sep);
                    f.w
                }
                Piece::Root(r) => {
                    self.emit_root(r, x, base, lead, sep);
                    r.w
                }
            };
            sep = false;
        }
    }

    fn rule(&mut self, x: f32, y: f32, w: f32, px: f32, style: &Style) {
        let thick = (px / 14.0).max(1.0).round();
        let color = self.color(style);
        self.out.push(Item::Fill { r: Rect::new(x, y.round(), w, thick), color });
    }

    fn emit_frac(&mut self, f: &Frac, left: f32, baseline: f32, lead: &mut Option<bool>, sep: bool) {
        self.rule(left + f.px * 0.08, baseline - f.px * 0.3, f.w - f.px * 0.16, f.px, &f.style);
        self.emit_row(&f.num, left + (f.w - f.num.w) / 2.0, baseline - f.up(), lead, sep);
        self.emit_row(&f.den, left + (f.w - f.den.w) / 2.0, baseline + f.down(), lead, true);
    }

    fn emit_root(&mut self, r: &Root, left: f32, baseline: f32, lead: &mut Option<bool>, sep: bool) {
        self.emit_run(&r.sign, left, baseline + r.dy, lead, sep);
        self.emit_row(&r.row, left + r.sign_w, baseline, lead, false);
        let x = left + r.sign_w - 2.5;
        self.rule(x, baseline - r.row.asc - r.px * 0.12, left + r.w - x, r.px, &r.style);
    }

    fn emit_limits(&mut self, l: &Limits, left: f32, baseline: f32, lead: &mut Option<bool>) {
        self.emit_run(&l.base, left + (l.w - l.base.2) / 2.0, baseline, lead, false);
        if !l.bottom.pieces.is_empty() {
            let y = baseline + l.base_desc + l.px * 0.1 + l.bottom.asc;
            self.emit_row(&l.bottom, left + (l.w - l.bottom.w) / 2.0, y, lead, true);
        }
        if !l.top.pieces.is_empty() {
            let y = baseline - l.base_asc - l.px * 0.1 - l.top.desc;
            self.emit_row(&l.top, left + (l.w - l.top.w) / 2.0, y, lead, true);
        }
    }

    /// Draws a box with its left edge at `left`; `gap` tells whether a space separates it from the previous item.
    pub fn emit_boxed(&mut self, b: &Boxed, left: f32, baseline: f32, gap: bool) {
        let mut lead = Some(gap);
        match b {
            Boxed::Frac(f) => self.emit_frac(f, left, baseline, &mut lead, false),
            Boxed::Root(r) => self.emit_root(r, left, baseline, &mut lead, false),
            Boxed::Limits(l) => self.emit_limits(l, left, baseline, &mut lead),
        }
    }
}
