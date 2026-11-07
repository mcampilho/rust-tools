//! The treemap: every file a rectangle whose area is its size, folders the
//! rectangles that hold their contents.
//!
//! Layout: the squarified algorithm (Bruls, Huizing and van Wijk, 2000)
//! keeps rectangles close to squares, so small files stay visible.
//!
//! Shading: cushions (van Wijk and van de Wetering, 1999). Each rectangle
//! adds a parabolic ridge to a height field along x and along y, smaller at
//! each level down; a pixel's brightness comes from the slope of the summed
//! surface under a light from the top left. Folders then show as bulges
//! without drawing any borders.
//!
//! Files are coloured by extension: the twelve extensions taking the most
//! space get a colour each, the others grey.

use crate::model::{Id, Kind, Tree};
use rt_gfx::Color;

/// Colours of the largest extensions, in order.
pub const PALETTE: [u32; 12] = [
    0x2F7FEA, 0xE5533C, 0x3DB36B, 0xF0B429, 0x9B5DE5, 0x00B5C8, 0xF07A2D, 0xE04FA0, 0x7FBF3F, 0x5468D9, 0xC8A27A, 0x2E9E8E,
];
/// Files of the other extensions.
pub const OTHER: u32 = 0xA3A3A3;
/// Folders too small to show their contents, and links.
pub const FOLDER: u32 = 0x7C8794;

/// Height of the outermost ridge and the factor for each level down.
const RIDGE: f64 = 0.40;
const RIDGE_FALLOFF: f64 = 0.82;
/// Folders smaller than this (in square pixels) are drawn as one block.
const MIN_AREA: f64 = 6.0;
/// Ambient light, and the share of the directional light.
const AMBIENT: f64 = 0.18;

#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub node: Id,
    /// Drawn (a file, or a folder whose contents are not split further).
    pub leaf: bool,
    /// The cushion: coefficients of x², y², x, y.
    surface: [f32; 4],
}

impl Tile {
    pub fn w(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn h(&self) -> f32 {
        self.y1 - self.y0
    }

    /// The pixels it covers: [x0, x1) × [y0, y1).
    fn pixels(&self) -> (usize, usize, usize, usize) {
        let r = |v: f32| v.round().max(0.0) as usize;
        (r(self.x0), r(self.y0), r(self.x1), r(self.y1))
    }
}

/// Palette index of each extension id (`u8::MAX`: grey).
pub fn ext_colors(tree: &Tree) -> Vec<u8> {
    let mut table = vec![u8::MAX; tree.ext_ranking().len() + 1];
    for (rank, &ext) in tree.ext_ranking().iter().take(PALETTE.len()).enumerate() {
        if let Some(slot) = table.get_mut(ext as usize) {
            *slot = rank as u8;
        }
    }
    table
}

/// The base colour of a node in the treemap.
pub fn base_color(tree: &Tree, id: Id, table: &[u8]) -> u32 {
    let n = tree.node(id);
    match n.kind {
        Kind::File => match table.get(n.ext as usize) {
            Some(&i) if (i as usize) < PALETTE.len() => PALETTE[i as usize],
            _ => OTHER,
        },
        Kind::Dir | Kind::Link => FOLDER,
    }
}

/// A treemap laid out and drawn at a size.
pub struct Map {
    pub root: Id,
    pub w: u32,
    pub h: u32,
    /// Pre-order: a folder before its contents.
    pub tiles: Vec<Tile>,
    /// The image, opaque RGBA.
    pub image: tiny_skia::Pixmap,
    /// For each pixel, the index of the tile drawn there (`u32::MAX`: none).
    owner: Vec<u32>,
}

impl Map {
    /// Lays out and draws `root` of `tree` at `w`×`h` pixels.
    pub fn new(tree: &Tree, root: Id, w: u32, h: u32) -> Map {
        let (w, h) = (w.max(1), h.max(1));
        let mut tiles = Vec::new();
        if tree.size(root) > 0 {
            place(tree, root, [0.0, 0.0, w as f64, h as f64], [0.0; 4], RIDGE, &mut tiles);
        }
        let mut map = Map { root, w, h, tiles, image: tiny_skia::Pixmap::new(w, h).expect("treemap size"), owner: vec![u32::MAX; (w * h) as usize] };
        map.paint(tree);
        map
    }

    fn paint(&mut self, tree: &Tree) {
        let table = ext_colors(tree);
        let (w, h) = (self.w as usize, self.h as usize);
        // Light from the top left, mostly from the front.
        let (lx, ly, lz) = {
            let (x, y, z) = (-1.0f64, -1.0, 10.0);
            let n = (x * x + y * y + z * z).sqrt();
            (x / n, y / n, z / n)
        };
        let data = self.image.data_mut();
        for (ti, t) in self.tiles.iter().enumerate().filter(|(_, t)| t.leaf) {
            let base = Color::rgb(base_color(tree, t.node, &table));
            let (bx0, by0, bx1, by1) = t.pixels();
            let s = t.surface.map(f64::from);
            for y in by0..by1.min(h) {
                let ny = -(2.0 * s[1] * (y as f64 + 0.5) + s[3]);
                for x in bx0..bx1.min(w) {
                    let nx = -(2.0 * s[0] * (x as f64 + 0.5) + s[2]);
                    let cos = (nx * lx + ny * ly + lz) / (nx * nx + ny * ny + 1.0).sqrt();
                    let light = AMBIENT + (1.0 - AMBIENT) * cos.max(0.0);
                    // A little above 1 keeps the tops of the cushions bright.
                    let k = light * 1.18;
                    let ch = |c: u8| (c as f64 * k).min(255.0) as u8;
                    let i = (y * w + x) * 4;
                    data[i] = ch(base.r);
                    data[i + 1] = ch(base.g);
                    data[i + 2] = ch(base.b);
                    data[i + 3] = 255;
                    self.owner[y * w + x] = ti as u32;
                }
            }
        }
    }

    /// The node drawn at a pixel.
    pub fn hit(&self, x: f32, y: f32) -> Option<Id> {
        if x < 0.0 || y < 0.0 || x >= self.w as f32 || y >= self.h as f32 {
            return None;
        }
        let i = self.owner[y as usize * self.w as usize + x as usize];
        self.tiles.get(i as usize).map(|t| t.node)
    }

    /// The rectangle of `id`, or of its nearest folder that has one (too
    /// small to be drawn on its own).
    pub fn tile_of(&self, tree: &Tree, id: Id) -> Option<&Tile> {
        if !tree.is_within(id, self.root) {
            return None;
        }
        let mut cur = id;
        loop {
            if let Some(t) = self.tiles.iter().find(|t| t.node == cur) {
                return Some(t);
            }
            cur = tree.parent(cur)?;
        }
    }
}

/// A parabolic ridge over [a, b] of height `h`: what it adds to the
/// (quadratic, linear) coefficients.
fn ridge(a: f64, b: f64, h: f64) -> (f64, f64) {
    let w = b - a;
    if w > 0.0 {
        (-4.0 * h / w, 4.0 * h * (b + a) / w)
    } else {
        (0.0, 0.0)
    }
}

fn place(tree: &Tree, id: Id, r: [f64; 4], parent: [f64; 4], h: f64, out: &mut Vec<Tile>) {
    let [x0, y0, x1, y1] = r;
    let (qx, lx) = ridge(x0, x1, h);
    let (qy, ly) = ridge(y0, y1, h);
    let s = [parent[0] + qx, parent[1] + qy, parent[2] + lx, parent[3] + ly];
    let kids: Vec<(Id, f64)> = tree.children(id).iter().map(|&c| (c, tree.size(c) as f64)).take_while(|&(_, sz)| sz > 0.0).collect();
    let split = !kids.is_empty() && (x1 - x0) * (y1 - y0) >= MIN_AREA;
    out.push(Tile { x0: x0 as f32, y0: y0 as f32, x1: x1 as f32, y1: y1 as f32, node: id, leaf: !split, surface: s.map(|v| v as f32) });
    if !split {
        return;
    }
    for (c, cr) in squarify(&kids, r) {
        place(tree, c, cr, s, h * RIDGE_FALLOFF, out);
    }
}

/// The worst aspect ratio of a row of areas summing to `sum`, with the
/// largest `max` and smallest `min`, along a side of length `side`.
fn worst(sum: f64, min: f64, max: f64, side: f64) -> f64 {
    let (s2, w2) = (sum * sum, side * side);
    (w2 * max / s2).max(s2 / (w2 * min))
}

/// Splits `r` among `items` (largest first, all positive), in proportion to
/// their sizes.
pub fn squarify(items: &[(Id, f64)], r: [f64; 4]) -> Vec<(Id, [f64; 4])> {
    let total: f64 = items.iter().map(|i| i.1).sum();
    let [mut x0, mut y0, x1, y1] = r;
    let area = (x1 - x0) * (y1 - y0);
    if total <= 0.0 || area <= 0.0 {
        return Vec::new();
    }
    let scale = area / total;
    let mut out = Vec::with_capacity(items.len());
    let mut i = 0;
    while i < items.len() {
        let (w, h) = (x1 - x0, y1 - y0);
        let side = w.min(h);
        // Grow the row while its worst ratio improves.
        let first = items[i].1 * scale;
        let mut sum = first;
        let mut best = worst(sum, first, first, side);
        let mut j = i + 1;
        while j < items.len() {
            let a = items[j].1 * scale;
            let ratio = worst(sum + a, a, first, side);
            if ratio > best {
                break;
            }
            best = ratio;
            sum += a;
            j += 1;
        }
        let last_row = j == items.len();
        if w >= h {
            // A column at the left.
            let cw = if last_row { w } else { sum / h };
            let mut y = y0;
            for (k, it) in items[i..j].iter().enumerate() {
                let ih = if k + 1 == j - i { y1 - y } else { it.1 * scale / cw };
                out.push((it.0, [x0, y, x0 + cw, y + ih]));
                y += ih;
            }
            x0 += cw;
        } else {
            // A row at the top.
            let rh = if last_row { h } else { sum / w };
            let mut x = x0;
            for (k, it) in items[i..j].iter().enumerate() {
                let iw = if k + 1 == j - i { x1 - x } else { it.1 * scale / rh };
                out.push((it.0, [x, y0, x + iw, y0 + rh]));
                x += iw;
            }
            y0 += rh;
        }
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Entry, Tree};

    fn tree() -> Tree {
        let f = |n: &str, s: u64| Entry { name: n.into(), kind: Kind::File, size: s, alloc: s, mtime: 0, flags: 0 };
        let d = |n: &str| Entry { name: n.into(), kind: Kind::Dir, size: 0, alloc: 0, mtime: 0, flags: 0 };
        let mut t = Tree::new("/r", 0);
        let first = t.add_children(0, &[f("a.mp4", 6000), d("docs"), f("zero.txt", 0), f("b.zip", 1000)]);
        let docs = first + 1;
        let files: Vec<Entry> = (0..40).map(|i| f(&format!("n{i}.txt"), 50 + i)).collect();
        t.add_children(docs, &files);
        t.finish();
        t
    }

    #[test]
    fn squarify_fills_the_rectangle_in_proportion() {
        let items: Vec<(Id, f64)> = [6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0].iter().enumerate().map(|(i, &s)| (i as Id, s)).collect();
        let out = squarify(&items, [0.0, 0.0, 6.0, 4.0]);
        assert_eq!(out.len(), items.len());
        let mut total = 0.0;
        for ((_, size), (_, r)) in items.iter().zip(&out) {
            let a = (r[2] - r[0]) * (r[3] - r[1]);
            assert!((a - size).abs() < 1e-9, "{a} vs {size}");
            assert!(r[0] >= -1e-9 && r[1] >= -1e-9 && r[2] <= 6.0 + 1e-9 && r[3] <= 4.0 + 1e-9);
            total += a;
        }
        assert!((total - 24.0).abs() < 1e-9);
        // The classic example: the first two share a column.
        assert!((out[0].1[2] - 3.0).abs() < 1e-9 && (out[1].1[2] - 3.0).abs() < 1e-9);
    }

    #[test]
    fn every_pixel_belongs_to_a_file() {
        let t = tree();
        let m = Map::new(&t, 0, 300, 200);
        assert!(m.owner.iter().all(|&o| o != u32::MAX));
        assert!(m.image.data().chunks(4).all(|p| p[3] == 255));
        // Leaf tiles cover the area once; zero-size files have none.
        let leaf_area: f32 = m.tiles.iter().filter(|t| t.leaf).map(|t| t.w() * t.h()).sum();
        assert!((leaf_area - 60_000.0).abs() < 1.0, "{leaf_area}");
        let inside = |r: &Tile| r.x0 >= 0.0 && r.y0 >= 0.0 && r.x1 <= 300.001 && r.y1 <= 200.001 && r.x0 <= r.x1 && r.y0 <= r.y1;
        assert!(m.tiles.iter().all(inside));
        let zero = t_child(&t, "zero.txt");
        assert!(!m.tiles.iter().any(|r| r.node == zero));
    }

    fn t_child(t: &Tree, name: &str) -> Id {
        *t.children(0).iter().find(|&&c| t.name(c) == name).unwrap()
    }

    #[test]
    fn hits_and_tiles_find_nodes() {
        let t = tree();
        let m = Map::new(&t, 0, 300, 200);
        let a = t_child(&t, "a.mp4");
        let tile = *m.tile_of(&t, a).unwrap();
        assert_eq!(m.hit(tile.x0 + tile.w() / 2.0, tile.y0 + tile.h() / 2.0), Some(a));
        assert_eq!(m.hit(-1.0, 5.0), None);
        // a.mp4 has 6000 of 9780 bytes: 61% of the area.
        let share = tile.w() * tile.h() / 60_000.0;
        assert!((share - 6000.0 / 9780.0).abs() < 0.001, "{share}");
        // Zoomed into docs, files outside it have no tile.
        let docs = t_child(&t, "docs");
        let z = Map::new(&t, docs, 100, 100);
        assert!(z.tile_of(&t, a).is_none());
        assert!(z.tile_of(&t, t.children(docs)[5]).is_some());
    }

    #[test]
    fn largest_extensions_get_colours() {
        let t = tree();
        let table = ext_colors(&t);
        assert_eq!(base_color(&t, t_child(&t, "a.mp4"), &table), PALETTE[0]);
        assert_eq!(base_color(&t, t_child(&t, "docs"), &table), FOLDER);
        let empty = Map::new(&Tree::new("/e", 0), 0, 10, 10);
        assert!(empty.tiles.is_empty() && empty.hit(1.0, 1.0).is_none());
    }
}
