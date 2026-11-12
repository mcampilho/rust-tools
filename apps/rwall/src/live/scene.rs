//! Scenes drawn by rwall, frame by frame, on the processor: northern lights
//! over mountains, a slowly turning starry sky, a sea at sunset and soft
//! flowing colours. Each is drawn small (see [`Scene::size`]) and stretched
//! to the screen; what does not move is drawn once, in `base`.
//!
//! Pixels are `0x00RRGGBB`, rows top to bottom.

use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Aurora,
    Stars,
    Sea,
    Flow,
}

impl Scene {
    pub const ALL: [Scene; 4] = [Scene::Aurora, Scene::Stars, Scene::Sea, Scene::Flow];

    pub fn key(self) -> &'static str {
        match self {
            Scene::Aurora => "aurora",
            Scene::Stars => "stars",
            Scene::Sea => "sea",
            Scene::Flow => "flow",
        }
    }

    pub fn from_key(k: &str) -> Option<Scene> {
        Scene::ALL.into_iter().find(|s| s.key() == k)
    }

    /// Text in the settings window (translated by the caller).
    pub fn label(self) -> &'static str {
        match self {
            Scene::Aurora => "Northern lights",
            Scene::Stars => "Starry sky",
            Scene::Sea => "Sunset sea",
            Scene::Flow => "Color flow",
        }
    }

    /// The size each frame is drawn at, before it is stretched.
    pub fn size(self) -> (u32, u32) {
        match self {
            Scene::Flow => (320, 180),
            _ => (960, 540),
        }
    }

    /// Frames a second: the scenes move slowly.
    pub fn fps(self) -> u32 {
        match self {
            Scene::Flow => 20,
            _ => 24,
        }
    }
}

/// A small, repeatable random number source.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// From 0 to 1.
    fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

type Rgb = [f32; 3];

fn pack(c: Rgb) -> u32 {
    let ch = |v: f32| v.clamp(0.0, 255.0) as u32;
    (ch(c[0]) << 16) | (ch(c[1]) << 8) | ch(c[2])
}

fn unpack(p: u32) -> Rgb {
    [((p >> 16) & 255) as f32, ((p >> 8) & 255) as f32, (p & 255) as f32]
}

fn mix(a: Rgb, b: Rgb, k: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]
}

fn scale(a: Rgb, k: f32) -> Rgb {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// Adds light to a pixel.
fn add(p: &mut u32, c: Rgb, k: f32) {
    let o = unpack(*p);
    *p = pack([o[0] + c[0] * k, o[1] + c[1] * k, o[2] + c[2] * k]);
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Colour along stops `(position, colour)`, positions from 0 to 1 in order.
fn ramp(stops: &[(f32, Rgb)], x: f32) -> Rgb {
    let mut prev = stops[0];
    for s in stops {
        if x <= s.0 {
            let span = (s.0 - prev.0).max(1e-6);
            return mix(prev.1, s.1, ((x - prev.0) / span).clamp(0.0, 1.0));
        }
        prev = *s;
    }
    prev.1
}

struct Star {
    /// Position, or angle and distance from the pole (`Scene::Stars`).
    a: f32,
    b: f32,
    bright: f32,
    phase: f32,
    speed: f32,
    tint: Rgb,
}

/// A curtain of the northern lights: its vertical profile is worked out once.
struct Curtain {
    centre: f32,
    height: f32,
    seed: f32,
    strength: f32,
    /// Light for each row, from the lower edge (index 0 is `EDGE` rows below it).
    light: Vec<f32>,
    colour: Vec<Rgb>,
}

/// Rows of glow under the lower edge of a curtain.
const EDGE: usize = 6;

/// Draws one scene, frame after frame.
pub struct Painter {
    scene: Scene,
    w: usize,
    h: usize,
    base: Vec<u32>,
    stars: Vec<Star>,
    /// The first row of the ground in each column (stars and lights stay above).
    ground: Vec<usize>,
    curtains: Vec<Curtain>,
    /// The sea's colour in each row, darker and lighter (`Scene::Sea`).
    sea_rows: Vec<[u32; 2]>,
}

impl Painter {
    pub fn new(scene: Scene) -> Painter {
        let (w, h) = scene.size();
        let (w, h) = (w as usize, h as usize);
        let mut p = Painter { scene, w, h, base: vec![0; w * h], stars: Vec::new(), ground: vec![h; w], curtains: Vec::new(), sea_rows: Vec::new() };
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ scene.key().len() as u64);
        match scene {
            Scene::Aurora => p.aurora_base(&mut rng),
            Scene::Stars => p.stars_base(&mut rng),
            Scene::Sea => p.sea_base(),
            Scene::Flow => {}
        }
        p
    }

    pub fn size(&self) -> (u32, u32) {
        (self.w as u32, self.h as u32)
    }

    pub fn fps(&self) -> u32 {
        self.scene.fps()
    }

    /// Draws the scene at `t` seconds into `out` (resized to fit).
    pub fn frame(&self, t: f32, out: &mut Vec<u32>) {
        out.resize(self.w * self.h, 0);
        match self.scene {
            Scene::Aurora => self.aurora(t, out),
            Scene::Stars => self.starry(t, out),
            Scene::Sea => self.sea(t, out),
            Scene::Flow => self.flow(t, out),
        }
    }

    // ------------------------------------------------------------ aurora

    fn aurora_base(&mut self, rng: &mut Rng) {
        let (w, h) = (self.w, self.h);
        let hf = h as f32;
        let sky = [(0.0, [3.0, 5.0, 16.0]), (0.55, [8.0, 16.0, 34.0]), (0.8, [16.0, 30.0, 48.0]), (1.0, [18.0, 32.0, 46.0])];
        let far = |x: f32| hf * (0.70 + 0.045 * (x * 0.0045 + 3.0).sin() + 0.02 * (x * 0.013 + 1.0).sin() + 0.008 * (x * 0.041).sin());
        let near = |x: f32| hf * (0.80 + 0.055 * (x * 0.0068 + 1.0).sin() + 0.03 * (x * 0.019 + 2.0).sin() + 0.012 * (x * 0.052).sin());
        for x in 0..w {
            let xf = x as f32;
            let (f, n) = (far(xf), near(xf));
            self.ground[x] = f.max(0.0) as usize;
            for y in 0..h {
                let yf = y as f32;
                let c = if yf >= n {
                    mix([6.0, 9.0, 13.0], [3.0, 4.0, 6.0], (yf - n) / (hf - n).max(1.0))
                } else if yf >= f {
                    // The far ridge catches a little of the lights.
                    mix([14.0, 26.0, 32.0], [9.0, 15.0, 20.0], (yf - f) / (n - f).max(1.0))
                } else {
                    ramp(&sky, yf / hf)
                };
                self.base[y * w + x] = pack(c);
            }
        }
        for _ in 0..900 {
            let x = rng.f() * w as f32;
            let y = rng.f().powf(1.4) * hf * 0.72;
            if (y as usize) < self.ground[x as usize] {
                self.stars.push(star(rng, x, y));
            }
        }
        let curtain = |centre: f32, height: f32, seed: f32, strength: f32, low: Rgb, high: Rgb| {
            let rows = (height * hf) as usize + EDGE;
            let mut light = Vec::with_capacity(rows);
            let mut colour = Vec::with_capacity(rows);
            for i in 0..rows {
                let up = i as f32 - EDGE as f32;
                let s = up / (height * hf);
                let l = if up < 0.0 { 1.0 + up / EDGE as f32 } else { (1.0 - s).powf(1.6) * (0.35 + 0.65 * (-s * 5.0).exp()) };
                light.push(l.max(0.0));
                colour.push(mix(low, high, smooth(0.15, 0.85, s)));
            }
            Curtain { centre, height, seed, strength, light, colour }
        };
        self.curtains.push(curtain(0.30, 0.22, 2.0, 0.55, [70.0, 230.0, 210.0], [170.0, 70.0, 210.0]));
        self.curtains.push(curtain(0.44, 0.30, 0.0, 0.85, [50.0, 255.0, 140.0], [130.0, 60.0, 200.0]));
    }

    fn aurora(&self, t: f32, out: &mut [u32]) {
        out.copy_from_slice(&self.base);
        self.draw_fixed_stars(t, out);
        let (w, hf) = (self.w, self.h as f32);
        for c in &self.curtains {
            for x in 0..w {
                let fx = x as f32 / w as f32;
                let l = c.seed;
                let lower = hf * (c.centre + 0.06 * (fx * 6.0 + t * 0.07 + l).sin() + 0.03 * (fx * 15.0 - t * 0.11 + 2.0 * l).sin());
                let glow = 0.5 + 0.5 * (fx * 4.0 + t * 0.05 + 3.0 * l).sin();
                let rays = 0.55 + 0.45 * (fx * 90.0 + 2.5 * (fx * 7.0 + t * 0.2 + l).sin() + t * 0.6).sin();
                let k = glow * glow * (0.35 + 0.65 * rays) * c.strength;
                if k < 0.01 {
                    continue;
                }
                let bottom = lower as isize + EDGE as isize;
                for (i, (light, colour)) in c.light.iter().zip(&c.colour).enumerate() {
                    let y = bottom - i as isize;
                    if y < 0 {
                        break;
                    }
                    let y = y as usize;
                    if y >= self.ground[x] {
                        continue;
                    }
                    add(&mut out[y * w + x], *colour, k * light);
                }
                let _ = c.height;
            }
        }
    }

    /// Stars that stay put and twinkle.
    fn draw_fixed_stars(&self, t: f32, out: &mut [u32]) {
        for s in &self.stars {
            let (x, y) = (s.a as usize, s.b as usize);
            let v = s.bright * (0.7 + 0.3 * (t * s.speed + s.phase).sin());
            add(&mut out[y * self.w + x], s.tint, v);
        }
    }

    // ------------------------------------------------------------ stars

    /// Where the sky turns around, in pixels (above the top left).
    fn pole(&self) -> (f32, f32) {
        (self.w as f32 * 0.18, -(self.h as f32) * 0.12)
    }

    fn stars_base(&mut self, rng: &mut Rng) {
        let (w, h) = (self.w, self.h);
        let (wf, hf) = (w as f32, h as f32);
        let sky = [(0.0, [2.0, 3.0, 12.0]), (0.6, [10.0, 12.0, 34.0]), (0.9, [30.0, 22.0, 52.0]), (1.0, [34.0, 24.0, 50.0])];
        // The Milky Way: a soft band across the sky, uneven.
        let (ax, ay, bx, by) = (0.05 * wf, hf, 0.95 * wf, -0.1 * hf);
        let (dx, dy) = (bx - ax, by - ay);
        let len = (dx * dx + dy * dy).sqrt();
        let width = 0.13 * hf;
        let ground = |x: f32| hf * (0.88 + 0.025 * (x * 0.011).sin() + 0.02 * (x * 0.027 + 1.0).sin());
        for x in 0..w {
            self.ground[x] = ground(x as f32) as usize;
        }
        // Pine trees along the hills.
        for _ in 0..34 {
            let cx = rng.f() * wf;
            let th = 14.0 + rng.f() * 30.0;
            let tw = th * 0.32;
            let x0 = (cx - tw).max(0.0) as usize;
            let x1 = ((cx + tw) as usize).min(w - 1);
            for x in x0..=x1 {
                let top = ground(cx) - th * (1.0 - (x as f32 - cx).abs() / tw);
                self.ground[x] = self.ground[x].min(top.max(0.0) as usize);
            }
        }
        for y in 0..h {
            for x in 0..w {
                let (xf, yf) = (x as f32, y as f32);
                let mut c = ramp(&sky, yf / hf);
                if y >= self.ground[x] {
                    c = [3.0, 4.0, 8.0];
                } else {
                    let d = ((xf - ax) * dy - (yf - ay) * dx).abs() / len;
                    let band = (-(d / width) * (d / width)).exp();
                    let clumps = 0.55 + 0.25 * (xf * 0.031 + yf * 0.017).sin() * (xf * 0.012 - yf * 0.043).cos() + 0.2 * (xf * 0.09 + yf * 0.07).sin();
                    c = mix(c, [58.0, 54.0, 84.0], (band * clumps).clamp(0.0, 1.0) * 0.55);
                }
                self.base[y * w + x] = pack(c);
            }
        }
        // Enough stars around the pole to fill the sky while it turns.
        let (px, py) = self.pole();
        let far = ((wf - px).powi(2) + (hf - py).powi(2)).sqrt();
        for _ in 0..9000 {
            let mut s = star(rng, 0.0, 0.0);
            s.a = rng.f() * TAU;
            s.b = far * rng.f().sqrt();
            self.stars.push(s);
        }
    }

    fn starry(&self, t: f32, out: &mut [u32]) {
        out.copy_from_slice(&self.base);
        let (w, h) = (self.w, self.h);
        let (px, py) = self.pole();
        let turn = t * 0.004;
        for s in &self.stars {
            let a = s.a + turn;
            let (x, y) = (px + s.b * a.cos(), py + s.b * a.sin());
            if x < 0.0 || y < 0.0 {
                continue;
            }
            let (xi, yi) = (x as usize, y as usize);
            if xi >= w || yi >= h || yi >= self.ground[xi] {
                continue;
            }
            let v = s.bright * (0.65 + 0.35 * (t * s.speed + s.phase).sin());
            add(&mut out[yi * w + xi], s.tint, v);
            if s.bright > 0.8 {
                for (nx, ny) in [(xi.wrapping_sub(1), yi), (xi + 1, yi), (xi, yi.wrapping_sub(1)), (xi, yi + 1)] {
                    if nx < w && ny < self.ground[nx.min(w - 1)] {
                        add(&mut out[ny * w + nx], s.tint, v * 0.3);
                    }
                }
            }
        }
        // Now and then, a shooting star.
        let every = 11.0;
        let n = (t / every).floor();
        let phase = t - n * every;
        if phase < 0.9 {
            let mut rng = Rng(0xA5A5_5A5A ^ (n as i64 as u64).wrapping_mul(0x9E37_79B9));
            rng.next();
            let (x0, y0) = ((0.25 + 0.6 * rng.f()) * w as f32, (0.05 + 0.25 * rng.f()) * h as f32);
            let angle = PI * (0.75 + 0.1 * rng.f());
            let (dx, dy) = (angle.cos(), angle.sin());
            let head = phase / 0.9 * 220.0;
            let fade = 1.0 - smooth(0.6, 0.9, phase);
            for i in 0..70 {
                let d = head - i as f32 * 1.2;
                if d < 0.0 {
                    break;
                }
                let (x, y) = (x0 + dx * d, y0 + dy * d);
                if x < 0.0 || y < 0.0 || x as usize >= w || y as usize >= h {
                    continue;
                }
                let (xi, yi) = (x as usize, y as usize);
                if yi < self.ground[xi] {
                    add(&mut out[yi * w + xi], [255.0, 245.0, 230.0], fade * (1.0 - i as f32 / 70.0));
                }
            }
        }
    }

    // ------------------------------------------------------------ sea

    fn horizon(&self) -> f32 {
        self.h as f32 * 0.56
    }

    /// Sky, sun and the colours of the sea, without the waves.
    fn sea_base(&mut self) {
        let (w, h) = (self.w, self.h);
        let (wf, hf) = (w as f32, h as f32);
        let hz = self.horizon();
        let sky = [(0.0, [34.0, 26.0, 80.0]), (0.45, [150.0, 70.0, 120.0]), (0.8, [240.0, 120.0, 90.0]), (1.0, [255.0, 175.0, 100.0])];
        let sea = [(0.0, [210.0, 120.0, 100.0]), (0.25, [120.0, 60.0, 90.0]), (1.0, [22.0, 18.0, 48.0])];
        let (sx, sy, r) = (wf * 0.5, hz - hf * 0.045, hf * 0.075);
        for y in 0..h {
            for x in 0..w {
                let (xf, yf) = (x as f32, y as f32);
                let c = if yf < hz {
                    let mut c = ramp(&sky, yf / hz);
                    let d = ((xf - sx).powi(2) + (yf - sy).powi(2)).sqrt();
                    let glow = (-(d / (hf * 0.28)).powi(2)).exp();
                    c = mix(c, [255.0, 190.0, 120.0], glow * 0.6);
                    if d < r {
                        c = mix(c, [255.0, 232.0, 170.0], smooth(r, r - 1.5, d));
                    }
                    // Long thin clouds.
                    let band = (-((yf - hf * 0.3) / (hf * 0.025)).powi(2)).exp() * (0.5 + 0.5 * (xf * 0.006 + 1.0).sin()).powi(2);
                    let band2 = (-((yf - hf * 0.42) / (hf * 0.015)).powi(2)).exp() * (0.5 + 0.5 * (xf * 0.009 + 4.0).sin()).powi(2);
                    mix(c, [110.0, 50.0, 90.0], (band * 0.7 + band2 * 0.6).min(0.8))
                } else {
                    ramp(&sea, (yf - hz) / (hf - hz))
                };
                self.base[y * w + x] = pack(c);
            }
        }
        self.sea_rows = (0..h)
            .map(|y| {
                let c = if (y as f32) < hz { [0.0; 3] } else { ramp(&sea, (y as f32 - hz) / (hf - hz)) };
                [pack(scale(c, 0.9)), pack(scale(c, 0.97))]
            })
            .collect();
    }

    fn sea(&self, t: f32, out: &mut [u32]) {
        out.copy_from_slice(&self.base);
        let (w, h) = (self.w, self.h);
        let (wf, hf) = (w as f32, h as f32);
        let hz = self.horizon();
        let layers = 9;
        for i in 0..layers {
            let k = (i + 1) as f32 / layers as f32;
            let base_y = hz + (hf - hz) * k.powf(1.7) * 0.92;
            let amp = 0.8 + i as f32 * 1.9;
            let freq = 0.07 / (1.0 + i as f32 * 0.55);
            let speed = (0.35 + i as f32 * 0.12) * if i % 2 == 0 { 1.0 } else { -1.0 };
            let shade = i % 2;
            let crest_light = 0.4 * (1.0 - k * 0.7);
            for x in 0..w {
                let xf = x as f32;
                let fi = i as f32;
                let crest = base_y + amp * ((xf * freq + t * speed + fi).sin() + 0.5 * (xf * freq * 2.3 - t * speed * 1.3 + 2.0 * fi).sin());
                let top = crest.max(hz).ceil() as usize;
                for y in top..h {
                    let c = self.sea_rows[y][shade];
                    let edge = y - top;
                    out[y * w + x] = if edge < 2 { pack(mix(unpack(c), [255.0, 180.0, 120.0], crest_light * (1.0 - edge as f32 * 0.5))) } else { c };
                }
            }
        }
        // The path of the sun on the water.
        let cx = wf * 0.5;
        let r = hf * 0.075;
        for y in (hz as usize + 1)..h {
            let d = (y as f32 - hz) / (hf - hz);
            let half = r * (0.5 + 1.8 * d);
            let x0 = (cx - half).max(0.0) as usize;
            let x1 = ((cx + half) as usize).min(w - 1);
            for x in x0..=x1 {
                let fall = 1.0 - ((x as f32 - cx).abs() / half);
                let shimmer = 0.5 + 0.5 * (y as f32 * 0.9 + t * 2.5 + 2.0 * (x as f32 * 0.15 + t * 1.7).sin()).sin();
                add(&mut out[y * w + x], [255.0, 200.0, 130.0], shimmer.powi(3) * fall * 0.6 * (1.0 - d * 0.6));
            }
        }
    }

    // ------------------------------------------------------------ flow

    fn flow(&self, t: f32, out: &mut [u32]) {
        const BLOBS: [(Rgb, f32, f32, f32, f32); 6] = [
            ([20.0, 160.0, 170.0], 0.051, 0.037, 0.0, 1.3),
            ([120.0, 60.0, 190.0], 0.043, 0.061, 2.1, 0.4),
            ([250.0, 140.0, 70.0], 0.067, 0.029, 4.0, 2.6),
            ([230.0, 80.0, 130.0], 0.031, 0.053, 1.2, 5.1),
            ([40.0, 90.0, 210.0], 0.058, 0.047, 3.3, 3.7),
            ([240.0, 200.0, 90.0], 0.039, 0.071, 5.5, 0.9),
        ];
        let (w, h) = (self.w, self.h);
        let aspect = w as f32 / h as f32;
        let pos: Vec<(f32, f32)> = BLOBS.iter().map(|b| (0.5 * aspect + 0.42 * aspect * (t * b.1 + b.3).sin(), 0.5 + 0.42 * (t * b.2 + b.4).cos())).collect();
        for y in 0..h {
            let yf = (y as f32 + 0.5) / h as f32;
            for x in 0..w {
                let xf = (x as f32 + 0.5) / h as f32;
                let (mut sum, mut c) = (0.0, [0.0; 3]);
                for (b, (bx, by)) in BLOBS.iter().zip(&pos) {
                    let d2 = (xf - bx).powi(2) + (yf - by).powi(2);
                    let k = 1.0 / (1.0 + d2 * 22.0);
                    let k = k * k * k;
                    sum += k;
                    c = [c[0] + b.0[0] * k, c[1] + b.0[1] * k, c[2] + b.0[2] * k];
                }
                let v = 1.0 - 0.25 * ((xf / aspect - 0.5).powi(2) + (yf - 0.5).powi(2));
                out[y * w + x] = pack(scale(c, v / sum));
            }
        }
    }
}

fn star(rng: &mut Rng, x: f32, y: f32) -> Star {
    let bright = rng.f().powi(3) * 0.9 + 0.1;
    let warm = rng.f();
    Star {
        a: x,
        b: y,
        bright: bright * 1.6,
        phase: rng.f() * TAU,
        speed: 0.5 + rng.f() * 2.5,
        tint: mix([190.0, 205.0, 255.0], [255.0, 225.0, 190.0], warm),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenes_draw_and_move() {
        for s in Scene::ALL {
            assert_eq!(Scene::from_key(s.key()), Some(s));
            let p = Painter::new(s);
            let (w, h) = p.size();
            let (mut a, mut b) = (Vec::new(), Vec::new());
            p.frame(3.0, &mut a);
            p.frame(4.5, &mut b);
            assert_eq!(a.len(), (w * h) as usize);
            assert_ne!(a, b, "{s:?} does not move");
            // Not one flat colour, and not black.
            let lit = a.iter().filter(|p| **p & 0xF0F0F0 != 0).count();
            assert!(lit > a.len() / 3, "{s:?} is too dark");
            let mut again = Vec::new();
            p.frame(3.0, &mut again);
            assert_eq!(a, again, "{s:?} is not repeatable");
        }
    }

    /// A frame of each scene as a PNG in `target/screens` (run on request:
    /// `cargo test -p rwall --lib scene_pictures -- --ignored`).
    #[test]
    #[ignore]
    fn scene_pictures() {
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
        std::fs::create_dir_all(&out).unwrap();
        for s in Scene::ALL {
            let p = Painter::new(s);
            let (w, h) = p.size();
            let mut buf = Vec::new();
            p.frame(10.3, &mut buf);
            let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
            for (d, px) in pm.pixels_mut().iter_mut().zip(&buf) {
                *d = tiny_skia::ColorU8::from_rgba((px >> 16) as u8, (px >> 8) as u8, *px as u8, 255).premultiply();
            }
            pm.save_png(out.join(format!("scene-{}.png", s.key()))).unwrap();
        }
        // An animated GIF to try the player with.
        let p = Painter::new(Scene::Flow);
        let (w, h) = p.size();
        let mut enc = image::codecs::gif::GifEncoder::new_with_speed(std::fs::File::create(out.join("flow.gif")).unwrap(), 10);
        enc.set_repeat(image::codecs::gif::Repeat::Infinite).unwrap();
        let mut buf = Vec::new();
        for i in 0..60 {
            p.frame(i as f32 * 0.5, &mut buf);
            let img = image::RgbaImage::from_fn(w, h, |x, y| {
                let px = buf[(y * w + x) as usize];
                image::Rgba([(px >> 16) as u8, (px >> 8) as u8, px as u8, 255])
            });
            enc.encode_frame(image::Frame::from_parts(img, 0, 0, image::Delay::from_numer_denom_ms(80, 1))).unwrap();
        }
    }

    /// Time to draw a frame of each scene (run on request, in release:
    /// `cargo test -p rwall --release --lib scene_speed -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn scene_speed() {
        for s in Scene::ALL {
            let p = Painter::new(s);
            let mut buf = Vec::new();
            let start = std::time::Instant::now();
            for i in 0..50 {
                p.frame(i as f32 * 0.04, &mut buf);
            }
            println!("{s:?}: {:.2} ms a frame", start.elapsed().as_secs_f64() * 1000.0 / 50.0);
        }
    }

    #[test]
    fn colours() {
        assert_eq!(pack([300.0, -5.0, 127.9]), 0xFF007F);
        assert_eq!(unpack(0x102030), [16.0, 32.0, 48.0]);
        assert_eq!(ramp(&[(0.0, [0.0; 3]), (1.0, [100.0; 3])], 0.25), [25.0; 3]);
    }
}
