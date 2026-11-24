//! Places and draws a Mermaid flowchart (`rich::mermaid`): nodes in layers, ordered to cut crossings, then
//! boxes, lines and arrowheads as polygons and text items (so the labels can be selected and copied).

use super::{Ctx, Item, Meta};
use crate::rich::mermaid::{Diagram, Dir, Line, Shape};
use crate::rich::Style;
use crate::ui::gfx::{Color, FontId, Rect};

/// Where everything goes, relative to the diagram's top-left corner.
struct Geo {
    w: f32,
    h: f32,
    /// Centre and size of each node.
    pos: Vec<(f32, f32)>,
    size: Vec<(f32, f32)>,
    /// The lines of text of each node and their widths.
    text: Vec<Vec<(String, f32)>>,
    px: f32,
}

impl Ctx<'_> {
    /// Draws `d` centred in the column `[x, x + w]` from `y`; returns the bottom.
    pub fn diagram(&mut self, d: &Diagram, x: f32, w: f32, y: f32) -> f32 {
        let mut geo = self.arrange(d, 1.0);
        if geo.w > w && w > 0.0 {
            // Too wide: everything scales with the text, so lay it out again smaller.
            geo = self.arrange(d, (w / geo.w).max(0.4));
        }
        let ox = x + ((w - geo.w) / 2.0).max(0.0);
        self.paint(d, &geo, ox, y);
        self.out.width = self.out.width.max(ox + geo.w);
        y + geo.h
    }

    fn arrange(&mut self, d: &Diagram, scale: f32) -> Geo {
        let px = self.em * 0.92 * scale;
        let line_h = px * 1.35;
        let n = d.nodes.len();
        // Size of each node from its text and shape.
        let mut text = Vec::with_capacity(n);
        let mut size = Vec::with_capacity(n);
        for node in &d.nodes {
            let lines: Vec<(String, f32)> = node.label.lines().map(|l| (l.to_string(), self.g.measure(l, px, FontId::Regular))).collect();
            let lines = if lines.is_empty() { vec![(String::new(), 0.0)] } else { lines };
            let tw = lines.iter().map(|l| l.1).fold(0.0, f32::max);
            let th = lines.len() as f32 * line_h;
            let (w, h) = (tw + px * 1.8, th + px * 1.0);
            let s = match node.shape {
                Shape::Diamond => ((tw * 1.5 + px * 1.6).max(px * 4.0), (th * 1.7 + px * 1.0).max(px * 2.6)),
                Shape::Circle => {
                    let r = w.max(h);
                    (r, r)
                }
                Shape::Hexagon => (w + px * 1.4, h),
                Shape::Stadium => (w + px * 0.8, h),
                _ => (w.max(px * 3.0), h),
            };
            text.push(lines);
            size.push(s);
        }
        let horizontal = matches!(d.dir, Dir::LeftRight | Dir::RightLeft);
        // (extent across the layers, extent along them)
        let ext: Vec<(f32, f32)> = size.iter().map(|&(w, h)| if horizontal { (h, w) } else { (w, h) }).collect();

        let layer = layers(d);
        let depth = layer.iter().copied().max().map_or(0, |m| m + 1);
        let mut rows: Vec<Vec<usize>> = vec![Vec::new(); depth];
        for (i, &l) in layer.iter().enumerate() {
            rows[l].push(i);
        }
        order(d, &mut rows, &layer);

        // Along the layers.
        let has_label = |r: usize| d.edges.iter().any(|e| !e.label.is_empty() && layer[e.from].min(layer[e.to]) == r);
        let mut along = vec![0.0f32; depth];
        let mut at = 0.0;
        let mut centre_along = vec![0.0f32; n];
        for (r, row) in rows.iter().enumerate() {
            let thick = row.iter().map(|&i| ext[i].1).fold(0.0, f32::max);
            for &i in row {
                centre_along[i] = at + thick / 2.0;
            }
            along[r] = thick;
            at += thick + if has_label(r) { px * 3.2 } else { px * 2.4 };
        }
        let total_along = (at - px * 2.4).max(0.0);

        // Across: packed, then pulled towards the neighbours in the other layers.
        let gap = px * 1.6;
        let mut cross = vec![0.0f32; n];
        for row in &rows {
            let mut p = 0.0;
            for &i in row {
                cross[i] = p + ext[i].0 / 2.0;
                p += ext[i].0 + gap;
            }
            let width = p - gap;
            for &i in row {
                cross[i] -= width / 2.0;
            }
        }
        for pass in 0..8 {
            let order_rows: Vec<usize> = if pass % 2 == 0 { (0..depth).collect() } else { (0..depth).rev().collect() };
            for r in order_rows {
                let row = &rows[r];
                let want: Vec<f32> = row
                    .iter()
                    .map(|&i| {
                        let nb: Vec<f32> = d
                            .edges
                            .iter()
                            .filter_map(|e| {
                                if e.from == i {
                                    Some(e.to)
                                } else if e.to == i {
                                    Some(e.from)
                                } else {
                                    None
                                }
                            })
                            .filter(|&o| o != i && if pass % 2 == 0 { layer[o] < r } else { layer[o] > r })
                            .map(|o| cross[o])
                            .collect();
                        if nb.is_empty() {
                            cross[i]
                        } else {
                            nb.iter().sum::<f32>() / nb.len() as f32
                        }
                    })
                    .collect();
                // Keep the order and the gaps; shift the row so the average pull is zero.
                let mut p: Vec<f32> = Vec::with_capacity(row.len());
                for (k, &i) in row.iter().enumerate() {
                    let min = if k == 0 { f32::MIN } else { p[k - 1] + (ext[row[k - 1]].0 + ext[i].0) / 2.0 + gap };
                    p.push(want[k].max(min));
                }
                let shift = p.iter().zip(&want).map(|(a, b)| a - b).sum::<f32>() / row.len().max(1) as f32;
                for (k, &i) in row.iter().enumerate() {
                    cross[i] = p[k] - shift;
                }
                // The shift can break the gaps again: one more left-to-right pass.
                for k in 1..row.len() {
                    let min = cross[row[k - 1]] + (ext[row[k - 1]].0 + ext[row[k]].0) / 2.0 + gap;
                    if cross[row[k]] < min {
                        cross[row[k]] = min;
                    }
                }
            }
        }
        let lo = (0..n).map(|i| cross[i] - ext[i].0 / 2.0).fold(f32::MAX, f32::min);
        let hi = (0..n).map(|i| cross[i] + ext[i].0 / 2.0).fold(f32::MIN, f32::max);
        let margin = px * 0.5;
        let total_cross = (hi - lo).max(0.0);

        let mut pos = vec![(0.0, 0.0); n];
        for i in 0..n {
            let c = cross[i] - lo + margin;
            let mut a = centre_along[i] + margin;
            if matches!(d.dir, Dir::BottomUp | Dir::RightLeft) {
                a = total_along - centre_along[i] + margin;
            }
            pos[i] = if horizontal { (a, c) } else { (c, a) };
        }
        let (w, h) = if horizontal { (total_along, total_cross) } else { (total_cross, total_along) };
        Geo { w: w + 2.0 * margin, h: h + 2.0 * margin, pos, size, text, px }
    }

    fn paint(&mut self, d: &Diagram, g: &Geo, ox: f32, oy: f32) {
        let px = g.px;
        let stroke = (px * 0.09).max(1.0);
        let (line_color, node_line, fill, ink) = (self.pal.muted, self.pal.link, self.pal.code_bg, self.text);
        let centre = |i: usize| (ox + g.pos[i].0, oy + g.pos[i].1);
        self.enter(crate::clip::Tag::Para { align: crate::rich::Align::Left, pre: false, heading: 0, bg: None });
        // Edges first: nodes are drawn over their ends.
        let mut labels: Vec<(usize, (f32, f32))> = Vec::new();
        for (k, e) in d.edges.iter().enumerate() {
            if e.from == e.to {
                continue;
            }
            let (a, b) = (centre(e.from), centre(e.to));
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = (dx * dx + dy * dy).sqrt().max(0.001);
            // Edges that go both ways run side by side.
            let both = d.edges.iter().any(|o| o.from == e.to && o.to == e.from);
            let off = if both { px * 0.28 * if e.from < e.to { 1.0 } else { -1.0 } } else { 0.0 };
            let (nx, ny) = (-dy / len * off, dx / len * off);
            let p = border(a, g.size[e.from], d.nodes[e.from].shape, (dx, dy));
            let q = border(b, g.size[e.to], d.nodes[e.to].shape, (-dx, -dy));
            let (p, q) = ((p.0 + nx, p.1 + ny), (q.0 + nx, q.1 + ny));
            let width = if e.line == Line::Thick { stroke * 2.2 } else { stroke * 1.3 };
            let head = px * 0.55;
            // Stop the line short of the arrowheads.
            let (ux, uy) = (dx / len, dy / len);
            let from = if e.back_arrow { (p.0 + ux * head * 0.8, p.1 + uy * head * 0.8) } else { p };
            let to = if e.arrow { (q.0 - ux * head * 0.8, q.1 - uy * head * 0.8) } else { q };
            if e.line == Line::Dotted {
                let seg = px * 0.5;
                let total = ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt();
                let mut t = 0.0;
                while t < total {
                    let t2 = (t + seg).min(total);
                    let pt = |t: f32| (from.0 + ux * t, from.1 + uy * t);
                    self.out.push(Item::Shape { pts: vec![pt(t), pt(t2)], color: line_color, width, close: false });
                    t += seg * 2.0;
                }
            } else {
                self.out.push(Item::Shape { pts: vec![from, to], color: line_color, width, close: false });
            }
            let arrow_at = |this: &mut Self, tip: (f32, f32), dir: (f32, f32)| {
                let base = (tip.0 - dir.0 * head, tip.1 - dir.1 * head);
                let side = (-dir.1 * head * 0.42, dir.0 * head * 0.42);
                let pts = vec![tip, (base.0 + side.0, base.1 + side.1), (base.0 - side.0, base.1 - side.1)];
                this.out.push(Item::Shape { pts, color: line_color, width: 0.0, close: true });
            };
            if e.arrow {
                arrow_at(self, q, (ux, uy));
            }
            if e.back_arrow {
                arrow_at(self, p, (-ux, -uy));
            }
            if !e.label.is_empty() {
                labels.push((k, ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0)));
            }
        }
        for (k, (mx, my)) in labels {
            let label = &d.edges[k].label;
            let lines: Vec<&str> = label.lines().collect();
            let line_h = px * 1.2;
            let widths: Vec<f32> = lines.iter().map(|l| self.g.measure(l, px * 0.9, FontId::Regular)).collect();
            let w = widths.iter().copied().fold(0.0, f32::max) + px * 0.6;
            let h = lines.len() as f32 * line_h + px * 0.2;
            self.out.push(Item::Fill { r: Rect::new(mx - w / 2.0, my - h / 2.0, w, h), color: self.pal.bg });
            for (n, l) in lines.iter().enumerate() {
                let base = my - h / 2.0 + px * 0.1 + n as f32 * line_h + px * 0.95;
                self.diagram_text(l, mx - widths[n] / 2.0, base, px * 0.9, widths[n], ink);
            }
        }
        for (i, node) in d.nodes.iter().enumerate() {
            let (cx, cy) = centre(i);
            let (w, h) = g.size[i];
            let (l, t) = (cx - w / 2.0, cy - h / 2.0);
            let pts = outline(node.shape, l, t, w, h);
            self.out.push(Item::Shape { pts: pts.clone(), color: fill, width: 0.0, close: true });
            self.out.push(Item::Shape { pts, color: node_line, width: stroke * 1.4, close: true });
            if node.shape == Shape::Subroutine {
                let inset = px * 0.45;
                for x in [l + inset, l + w - inset] {
                    self.out.push(Item::Shape { pts: vec![(x, t), (x, t + h)], color: node_line, width: stroke, close: false });
                }
            }
            let line_h = px * 1.35;
            let top = cy - g.text[i].len() as f32 * line_h / 2.0;
            for (n, (s, tw)) in g.text[i].iter().enumerate() {
                self.diagram_text(s, cx - tw / 2.0, top + n as f32 * line_h + px * 1.0, px, *tw, ink);
            }
        }
    }

    fn diagram_text(&mut self, s: &str, x: f32, y: f32, px: f32, w: f32, color: Color) {
        if s.is_empty() {
            return;
        }
        let idx = self.out.push(Item::Text { x, y, s: s.to_string(), px, font: FontId::Regular, color });
        self.out.meta[idx] = Meta { node: self.node, style: Style::default(), w, breaks: 1, ..Meta::default() };
    }
}

/// The layer of each node: the longest path from a source, with cycles broken at their back edges.
fn layers(d: &Diagram) -> Vec<usize> {
    let n = d.nodes.len();
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in &d.edges {
        if e.from != e.to && !out[e.from].contains(&e.to) {
            out[e.from].push(e.to);
        }
    }
    // Depth-first from each node in turn: an edge to a node still being visited closes a cycle.
    let mut state = vec![0u8; n];
    let mut finish: Vec<usize> = Vec::with_capacity(n);
    let mut dag: Vec<(usize, usize)> = Vec::new();
    for s in 0..n {
        if state[s] != 0 {
            continue;
        }
        let mut stack = vec![(s, 0usize)];
        state[s] = 1;
        while let Some(&mut (u, ref mut k)) = stack.last_mut() {
            if *k < out[u].len() {
                let v = out[u][*k];
                *k += 1;
                match state[v] {
                    0 => {
                        dag.push((u, v));
                        state[v] = 1;
                        stack.push((v, 0));
                    }
                    1 => dag.push((v, u)), // back edge, turned round
                    _ => dag.push((u, v)),
                }
            } else {
                state[u] = 2;
                finish.push(u);
                stack.pop();
            }
        }
    }
    let mut layer = vec![0usize; n];
    for &u in finish.iter().rev() {
        for &(a, b) in &dag {
            if a == u {
                layer[b] = layer[b].max(layer[u] + 1);
            }
        }
    }
    layer
}

/// Orders the nodes of each layer to cut crossings: a few sweeps by the average place of the neighbours.
fn order(d: &Diagram, rows: &mut [Vec<usize>], layer: &[usize]) {
    let n = d.nodes.len();
    let mut place = vec![0.0f32; n];
    for row in rows.iter() {
        for (k, &i) in row.iter().enumerate() {
            place[i] = k as f32;
        }
    }
    for pass in 0..6 {
        let order_rows: Vec<usize> =
            if pass % 2 == 0 { (1..rows.len()).collect() } else { (0..rows.len().saturating_sub(1)).rev().collect() };
        for r in order_rows {
            let down = pass % 2 == 0;
            let mut keyed: Vec<(f32, usize)> = rows[r]
                .iter()
                .map(|&i| {
                    let nb: Vec<f32> = d
                        .edges
                        .iter()
                        .filter_map(|e| {
                            if e.from == i {
                                Some(e.to)
                            } else if e.to == i {
                                Some(e.from)
                            } else {
                                None
                            }
                        })
                        .filter(|&o| o != i && if down { layer[o] < r } else { layer[o] > r })
                        .map(|o| place[o])
                        .collect();
                    (if nb.is_empty() { place[i] } else { nb.iter().sum::<f32>() / nb.len() as f32 }, i)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            rows[r] = keyed.iter().map(|k| k.1).collect();
            for (k, &i) in rows[r].iter().enumerate() {
                place[i] = k as f32;
            }
        }
    }
}

/// Where the line from a node's centre in direction `dir` leaves its outline.
fn border(c: (f32, f32), size: (f32, f32), shape: Shape, dir: (f32, f32)) -> (f32, f32) {
    let (hw, hh) = (size.0 / 2.0, size.1 / 2.0);
    if shape == Shape::Circle {
        let len = (dir.0 * dir.0 + dir.1 * dir.1).sqrt().max(1e-4);
        return (c.0 + dir.0 / len * hw, c.1 + dir.1 / len * hw);
    }
    let (ax, ay) = (dir.0.abs().max(1e-4), dir.1.abs().max(1e-4));
    let t = if shape == Shape::Diamond { 1.0 / (ax / hw + ay / hh) } else { (hw / ax).min(hh / ay) };
    (c.0 + dir.0 * t, c.1 + dir.1 * t)
}

/// The outline of a node as a polygon in the box `(l, t, w, h)`.
fn outline(shape: Shape, l: f32, t: f32, w: f32, h: f32) -> Vec<(f32, f32)> {
    let (r, b) = (l + w, t + h);
    match shape {
        Shape::Diamond => vec![(l + w / 2.0, t), (r, t + h / 2.0), (l + w / 2.0, b), (l, t + h / 2.0)],
        Shape::Hexagon => {
            let k = (h / 2.0).min(w / 4.0);
            vec![(l + k, t), (r - k, t), (r, t + h / 2.0), (r - k, b), (l + k, b), (l, t + h / 2.0)]
        }
        Shape::Circle => ellipse(l + w / 2.0, t + h / 2.0, w / 2.0, h / 2.0),
        Shape::Round => rounded(l, t, w, h, (h * 0.22).min(w / 4.0)),
        Shape::Stadium => rounded(l, t, w, h, h / 2.0),
        Shape::Rect | Shape::Subroutine => vec![(l, t), (r, t), (r, b), (l, b)],
    }
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<(f32, f32)> {
    (0..36)
        .map(|k| {
            let a = k as f32 / 36.0 * std::f32::consts::TAU;
            (cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

fn rounded(l: f32, t: f32, w: f32, h: f32, r: f32) -> Vec<(f32, f32)> {
    let corners = [(l + w - r, t + r, -90.0f32), (l + w - r, t + h - r, 0.0), (l + r, t + h - r, 90.0), (l + r, t + r, 180.0)];
    let mut pts = Vec::new();
    for (cx, cy, start) in corners {
        for k in 0..=6 {
            let a = (start + k as f32 * 15.0).to_radians();
            pts.push((cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    pts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rich::mermaid::parse;

    #[test]
    fn layers_follow_the_edges_and_survive_cycles() {
        let d = parse("graph TD\nA --> B\nB --> C\nA --> C\nC --> A").unwrap();
        let l = layers(&d);
        assert_eq!(l, [0, 1, 2]);
        let d = parse("graph TD\nA --> B\nA --> C\nB --> D\nC --> D").unwrap();
        assert_eq!(layers(&d), [0, 1, 1, 2]);
    }

    #[test]
    fn borders_sit_on_the_outline() {
        let p = border((0.0, 0.0), (20.0, 10.0), Shape::Rect, (1.0, 0.0));
        assert!((p.0 - 10.0).abs() < 1e-3 && p.1.abs() < 1e-3);
        let p = border((0.0, 0.0), (20.0, 10.0), Shape::Diamond, (0.0, 1.0));
        assert!(p.0.abs() < 1e-2 && (p.1 - 5.0).abs() < 1e-2);
        let p = border((0.0, 0.0), (10.0, 10.0), Shape::Circle, (3.0, 4.0));
        assert!((p.0 - 3.0).abs() < 1e-3 && (p.1 - 4.0).abs() < 1e-3);
    }
}
