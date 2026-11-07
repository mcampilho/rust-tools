//! The two round charts, as alternatives to the treemap:
//!
//! - **Pie**: one level, the contents of the folder shown as slices, the
//!   small ones gathered in one "other" slice, with a legend beside it.
//! - **Rings**: the folder in the middle and its contents in rings around it,
//!   one ring per level, each item inside the angle of its folder. The colour
//!   follows the angle around the circle and gets lighter with depth.
//!
//! The chart is drawn once into an image; the slice under the pointer is then
//! drawn on top of it, moved out from the centre (`view.rs`).
//!
//! Angles are in radians, from the top, clockwise.

use crate::model::{Id, Kind as NodeKind, Tree};
use crate::treemap::{OTHER, PALETTE};
use rt_gfx::Rect;
use std::f32::consts::{PI, TAU};
use tiny_skia::{Color as SkColor, FillRule, GradientStop, Paint, PathBuilder, Pixmap, Point, RadialGradient, SpreadMode, Stroke, Transform};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pie,
    Rings,
}

/// Slices shown at most in the pie; the rest go into "other".
pub const PIE_SLICES: usize = 12;
/// Smaller slices go into "other" too.
const PIE_MIN_SHARE: f64 = 0.01;
/// Rings around the middle.
pub const RING_LEVELS: u16 = 6;
/// Segments narrower than this, in pixels along their middle, are left out
/// (with what is inside them).
const MIN_ARC: f32 = 3.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    /// The item, or None for the slice of small items.
    pub node: Option<Id>,
    /// The segment of its folder (rings).
    pub parent: Option<usize>,
    /// 1 for the first ring (the pie has only that).
    pub depth: u16,
    pub a0: f32,
    pub a1: f32,
    pub r0: f32,
    pub r1: f32,
    pub color: u32,
    pub size: u64,
    /// How many items the "other" slice holds.
    pub others: usize,
}

impl Segment {
    pub fn mid(&self) -> f32 {
        (self.a0 + self.a1) / 2.0
    }
}

/// What a point of the chart is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The middle of the rings: the folder shown.
    Center,
    Segment(usize),
}

pub struct Chart {
    pub kind: Kind,
    pub root: Id,
    pub w: u32,
    pub h: u32,
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
    /// Radius of the middle (rings; 0 for the pie).
    pub inner: f32,
    pub segments: Vec<Segment>,
    /// Pie legend: a row per slice, in the chart's pixels.
    pub legend: Vec<(usize, Rect)>,
    pub image: Pixmap,
}

/// A point at angle `a` and radius `r` around (cx, cy).
pub fn polar(cx: f32, cy: f32, r: f32, a: f32) -> (f32, f32) {
    (cx + r * a.sin(), cy - r * a.cos())
}

/// The outline of a ring segment (a slice when `r0` is 0).
pub fn sector(cx: f32, cy: f32, r0: f32, r1: f32, a0: f32, a1: f32) -> Vec<(f32, f32)> {
    let steps = (((a1 - a0) / TAU * 240.0).ceil() as usize).clamp(1, 240);
    let mut pts = Vec::with_capacity(2 * steps + 3);
    for i in 0..=steps {
        pts.push(polar(cx, cy, r1, a0 + (a1 - a0) * i as f32 / steps as f32));
    }
    if r0 <= 0.0 {
        pts.push((cx, cy));
    } else {
        for i in (0..=steps).rev() {
            pts.push(polar(cx, cy, r0, a0 + (a1 - a0) * i as f32 / steps as f32));
        }
    }
    pts
}

/// HSL (h in turns, s and l 0..1) to 0xRRGGBB.
pub fn hsl(h: f32, s: f32, l: f32) -> u32 {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(1.0) * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let ch = |v: f32| (((v + m) * 255.0).round().clamp(0.0, 255.0)) as u32;
    ch(r) << 16 | ch(g) << 8 | ch(b)
}

impl Chart {
    /// The pie of the contents of `root`, with its legend, at `w`×`h`.
    pub fn pie(tree: &Tree, root: Id, w: u32, h: u32, s: f32, bg: u32) -> Chart {
        let (wf, hf) = (w as f32, h as f32);
        let pad = 16.0 * s;
        let legend_w = if wf > 520.0 * s { (wf * 0.42).min(400.0 * s) } else { 0.0 };
        let radius = ((hf - 2.0 * pad).min(wf - legend_w - 3.0 * pad) / 2.0).max(4.0);
        let cx = ((wf - legend_w) / 2.0).max(pad + radius).round();
        let cy = (hf / 2.0).round();
        let mut c = Chart::empty(Kind::Pie, root, w, h, cx, cy, radius, 0.0);
        let total = tree.size(root);
        if total == 0 {
            c.paint(bg, s);
            return c;
        }
        let kids: Vec<Id> = tree.children(root).iter().copied().filter(|&k| tree.size(k) > 0).collect();
        let shown = kids.iter().take(PIE_SLICES).take_while(|&&k| tree.size(k) as f64 / total as f64 >= PIE_MIN_SHARE).count();
        // One "other" slice only when it holds more than one item.
        let shown = if kids.len() == shown + 1 { kids.len() } else { shown };
        let mut a = 0.0f32;
        let angle = |size: u64| (size as f64 / total as f64 * TAU as f64) as f32;
        for (i, &k) in kids[..shown].iter().enumerate() {
            let size = tree.size(k);
            let a1 = a + angle(size);
            c.segments.push(Segment { node: Some(k), parent: None, depth: 1, a0: a, a1, r0: 0.0, r1: radius, color: PALETTE[i % PALETTE.len()], size, others: 0 });
            a = a1;
        }
        if shown < kids.len() {
            let size: u64 = kids[shown..].iter().map(|&k| tree.size(k)).sum();
            c.segments.push(Segment { node: None, parent: None, depth: 1, a0: a, a1: TAU, r0: 0.0, r1: radius, color: OTHER, size, others: kids.len() - shown });
        }
        if let Some(last) = c.segments.last_mut() {
            last.a1 = TAU;
        }
        // The legend, centred beside the pie.
        if legend_w > 0.0 {
            let row = (24.0 * s).round();
            let x = (cx + radius + 2.0 * pad).round();
            let lw = wf - x - pad;
            let fit = ((hf - 2.0 * pad) / row).floor().max(0.0) as usize;
            let n = c.segments.len().min(fit);
            let y0 = ((hf - n as f32 * row) / 2.0).round();
            c.legend = (0..n).map(|i| (i, Rect::new(x, y0 + i as f32 * row, lw, row))).collect();
        }
        c.paint(bg, s);
        c
    }

    /// The rings around `root`, at `w`×`h`.
    pub fn rings(tree: &Tree, root: Id, w: u32, h: u32, s: f32, bg: u32) -> Chart {
        let pad = 12.0 * s;
        let radius = ((w.min(h) as f32 - 2.0 * pad) / 2.0).max(8.0);
        let inner = (radius * 0.2).round();
        let (cx, cy) = ((w as f32 / 2.0).round(), (h as f32 / 2.0).round());
        let mut c = Chart::empty(Kind::Rings, root, w, h, cx, cy, radius, inner);
        if tree.size(root) > 0 {
            c.ring(tree, root, None, 1, 0.0, TAU);
        }
        c.paint(bg, s);
        c
    }

    #[allow(clippy::too_many_arguments)]
    fn empty(kind: Kind, root: Id, w: u32, h: u32, cx: f32, cy: f32, radius: f32, inner: f32) -> Chart {
        let image = Pixmap::new(w.max(1), h.max(1)).expect("chart size");
        Chart { kind, root, w, h, cx, cy, radius, inner, segments: Vec::new(), legend: Vec::new(), image }
    }

    /// Thickness of a ring.
    pub fn ring_width(&self) -> f32 {
        (self.radius - self.inner) / RING_LEVELS as f32
    }

    /// Adds the contents of `id` (spanning a0..a1) as ring `depth`.
    fn ring(&mut self, tree: &Tree, id: Id, parent: Option<usize>, depth: u16, a0: f32, a1: f32) {
        if depth > RING_LEVELS {
            return;
        }
        let total = tree.size(id);
        if total == 0 {
            return;
        }
        let rw = self.ring_width();
        let (r0, r1) = (self.inner + (depth - 1) as f32 * rw, self.inner + depth as f32 * rw);
        let mid_r = (r0 + r1) / 2.0;
        let mut a = a0;
        for &k in tree.children(id) {
            let size = tree.size(k);
            if size == 0 {
                break;
            }
            let span = (size as f64 / total as f64 * (a1 - a0) as f64) as f32;
            let (s0, s1) = (a, a + span);
            a = s1;
            if span * mid_r < MIN_ARC {
                // Children are sorted: the rest are as small.
                break;
            }
            let file = tree.node(k).kind != NodeKind::Dir;
            let d = depth as f32 - 1.0;
            let (sat, light) = (0.68 - d * 0.04, 0.47 + d * 0.04);
            let color = hsl((s0 + s1) / 2.0 / TAU, if file { sat * 0.6 } else { sat }, light);
            let idx = self.segments.len();
            self.segments.push(Segment { node: Some(k), parent, depth, a0: s0, a1: s1, r0, r1, color, size, others: 0 });
            if !file {
                self.ring(tree, k, Some(idx), depth + 1, s0, s1);
            }
        }
    }

    fn paint(&mut self, bg: u32, s: f32) {
        let sk = |c: u32, a: u8| SkColor::from_rgba8((c >> 16) as u8, (c >> 8) as u8, c as u8, a);
        self.image.fill(sk(bg, 255));
        let id = Transform::identity();
        let path_of = |pts: &[(f32, f32)]| {
            let mut pb = PathBuilder::new();
            pb.move_to(pts[0].0, pts[0].1);
            for p in &pts[1..] {
                pb.line_to(p.0, p.1);
            }
            pb.close();
            pb.finish()
        };
        let mut paint = Paint { anti_alias: true, ..Paint::default() };
        let edge = Stroke { width: s.max(1.0), ..Stroke::default() };
        let mut line = Paint { anti_alias: true, ..Paint::default() };
        line.set_color(sk(bg, 255));
        for seg in &self.segments {
            let pts = sector(self.cx, self.cy, seg.r0, seg.r1, seg.a0, seg.a1);
            if let Some(path) = path_of(&pts) {
                paint.set_color(sk(seg.color, 255));
                self.image.fill_path(&path, &paint, FillRule::Winding, id, None);
                if self.segments.len() > 1 {
                    self.image.stroke_path(&path, &line, &edge, id, None);
                }
            }
        }
        // A soft light from the middle gives the pie some volume.
        if self.kind == Kind::Pie && !self.segments.is_empty() {
            let stops = vec![GradientStop::new(0.0, SkColor::from_rgba8(255, 255, 255, 70)), GradientStop::new(1.0, SkColor::from_rgba8(0, 0, 0, 40))];
            let c = Point::from_xy(self.cx - self.radius * 0.25, self.cy - self.radius * 0.3);
            if let (Some(shader), Some(circle)) = (RadialGradient::new(c, c, self.radius * 1.3, stops, SpreadMode::Pad, id), PathBuilder::from_circle(self.cx, self.cy, self.radius)) {
                let p = Paint { shader, anti_alias: true, ..Paint::default() };
                self.image.fill_path(&circle, &p, FillRule::Winding, id, None);
            }
        }
        if self.kind == Kind::Rings {
            if let Some(circle) = PathBuilder::from_circle(self.cx, self.cy, self.inner - 1.0) {
                let mid = |v: u32, sh: u32| (((v >> sh) & 0xFF) as f32 * 0.85 + 128.0 * 0.15) as u32;
                let c = mid(bg, 16) << 16 | mid(bg, 8) << 8 | mid(bg, 0);
                paint.set_color(sk(c, 255));
                self.image.fill_path(&circle, &paint, FillRule::Winding, id, None);
            }
        }
    }

    /// What is at a point of the chart's image.
    pub fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        if let Some((i, _)) = self.legend.iter().find(|(_, r)| r.contains(x, y)) {
            return Some(Hit::Segment(*i));
        }
        let (dx, dy) = (x - self.cx, y - self.cy);
        let r = (dx * dx + dy * dy).sqrt();
        if r > self.radius {
            return None;
        }
        if self.kind == Kind::Rings && r < self.inner {
            return Some(Hit::Center);
        }
        let a = dx.atan2(-dy).rem_euclid(TAU);
        self.segments.iter().position(|s| r >= s.r0 && r < s.r1 && a >= s.a0 && a < s.a1).map(Hit::Segment)
    }

    /// The segment of `id`, or of the nearest folder above it that has one.
    pub fn segment_of(&self, tree: &Tree, id: Id) -> Option<usize> {
        let mut cur = id;
        loop {
            if cur == self.root {
                return None;
            }
            if let Some(i) = self.segments.iter().position(|s| s.node == Some(cur)) {
                return Some(i);
            }
            cur = tree.parent(cur)?;
        }
    }

    /// `i` and every segment inside it (the rings further out).
    pub fn subtree(&self, i: usize) -> Vec<usize> {
        let mut inside = vec![false; self.segments.len()];
        inside[i] = true;
        // Parents come before their contents.
        for (k, s) in self.segments.iter().enumerate().skip(i + 1) {
            if s.parent.is_some_and(|p| inside[p]) {
                inside[k] = true;
            }
        }
        inside.iter().enumerate().filter(|(_, &b)| b).map(|(k, _)| k).collect()
    }
}

/// How far a pointed segment moves out, as a share of the radius.
pub const POP: f32 = 0.05;

/// The offset of a segment moved out along its middle angle.
pub fn pop_offset(seg: &Segment, radius: f32) -> (f32, f32) {
    let d = (radius * POP).clamp(4.0, 14.0);
    let a = if seg.a1 - seg.a0 >= TAU - 0.001 { PI } else { seg.mid() };
    (d * a.sin(), -d * a.cos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entry, Kind as K};

    fn tree() -> Tree {
        let f = |n: &str, s: u64| Entry { name: n.into(), kind: K::File, size: s, alloc: s, mtime: 0, flags: 0 };
        let d = |n: &str| Entry { name: n.into(), kind: K::Dir, size: 0, alloc: 0, mtime: 0, flags: 0 };
        let mut t = Tree::new("/r", 0);
        // 1 big (dir), 2 half.bin, then 30 tiny files.
        let mut top = vec![d("big"), f("half.bin", 4000)];
        top.extend((0..30).map(|i| f(&format!("t{i}"), 3)));
        t.add_children(0, &top);
        let big_kids: Vec<Entry> = (0..4).map(|i| f(&format!("b{i}"), 1000 + i)).collect();
        let first = t.add_children(1, &big_kids);
        let _ = first;
        t.finish();
        t
    }

    fn named(t: &Tree, c: &Chart, name: &str) -> usize {
        c.segments.iter().position(|s| s.node.is_some_and(|n| t.name(n) == name)).unwrap()
    }

    #[test]
    fn pie_slices_cover_the_circle() {
        let t = tree();
        let c = Chart::pie(&t, 0, 800, 400, 1.0, 0xFFFFFF);
        // big, half.bin, and the 30 small files in "other".
        assert_eq!(c.segments.len(), 3);
        let other = &c.segments[2];
        assert_eq!((other.node, other.others, other.size), (None, 30, 90));
        assert!((c.segments[0].a0).abs() < 1e-6 && (c.segments[2].a1 - TAU).abs() < 1e-6);
        for w in c.segments.windows(2) {
            assert!((w[0].a1 - w[1].a0).abs() < 1e-5);
        }
        let big = &c.segments[0];
        let share = (big.a1 - big.a0) / TAU;
        assert!((share - 4006.0 / 8096.0).abs() < 1e-4, "{share}");
        assert_eq!(c.legend.len(), 3);
        // The middle of the big slice is on it; far away is nothing.
        let (x, y) = polar(c.cx, c.cy, c.radius / 2.0, big.mid());
        assert_eq!(c.hit(x, y), Some(Hit::Segment(0)));
        assert_eq!(c.hit(1.0, 1.0), None);
        let (_, r) = c.legend[1];
        assert_eq!(c.hit(r.x + 2.0, r.y + 2.0), Some(Hit::Segment(1)));
    }

    #[test]
    fn rings_nest_inside_their_folders() {
        let t = tree();
        let c = Chart::rings(&t, 0, 600, 600, 1.0, 0x000000);
        let big = named(&t, &c, "big");
        let b0 = named(&t, &c, "b0");
        assert_eq!(c.segments[b0].parent, Some(big));
        assert_eq!(c.segments[b0].depth, 2);
        let (pb, sb) = (&c.segments[big], &c.segments[b0]);
        assert!(sb.a0 >= pb.a0 - 1e-5 && sb.a1 <= pb.a1 + 1e-5 && sb.r0 >= pb.r1 - 1e-3);
        // The four files of big fill its angle.
        let kids: Vec<&Segment> = c.segments.iter().filter(|s| s.parent == Some(big)).collect();
        assert_eq!(kids.len(), 4);
        let span: f32 = kids.iter().map(|s| s.a1 - s.a0).sum();
        assert!((span - (pb.a1 - pb.a0)).abs() < 1e-4);
        assert_eq!(c.subtree(big).len(), 5);
        assert_eq!(c.hit(c.cx, c.cy), Some(Hit::Center));
        let (x, y) = polar(c.cx, c.cy, (sb.r0 + sb.r1) / 2.0, sb.mid());
        assert_eq!(c.hit(x, y), Some(Hit::Segment(b0)));
        // Tiny files are too thin for a segment of their own.
        assert!(c.segments.len() < 2 + 4 + 30);
        // The segment of a file left out is its folder's (here: none, the root).
        let tiny = *t.children(0).last().unwrap();
        assert_eq!(c.segment_of(&t, tiny), None);
        assert_eq!(c.segment_of(&t, t.children(1)[0]), Some(named(&t, &c, t.name(t.children(1)[0]))));
    }

    #[test]
    fn colours_and_shapes() {
        assert_eq!(hsl(0.0, 1.0, 0.5), 0xFF0000);
        assert_eq!(hsl(1.0 / 3.0, 1.0, 0.5), 0x00FF00);
        assert_eq!(hsl(0.5, 0.0, 1.0), 0xFFFFFF);
        let pts = sector(0.0, 0.0, 0.0, 10.0, 0.0, PI / 2.0);
        assert_eq!(*pts.last().unwrap(), (0.0, 0.0));
        let (x, y) = pts[0];
        assert!(x.abs() < 1e-5 && (y + 10.0).abs() < 1e-5, "starts at the top");
        let empty = Chart::rings(&Tree::new("/e", 0), 0, 100, 100, 1.0, 0);
        assert!(empty.segments.is_empty());
    }
}
