//! The dynamic wallpaper: a video, an animated picture or a scene drawn by
//! rwall, playing behind the desktop icons.
//!
//! This module holds what does not depend on the system: the settings, where
//! a frame goes on each screen, and the choice of a video in a folder. The
//! player itself is Windows only (`player`), as the window behind the icons
//! is a trick of Explorer's (see `desktop`).

pub mod anim;
pub mod scene;

#[cfg(windows)]
mod desktop;
#[cfg(windows)]
pub mod player;
#[cfg(windows)]
pub mod thumb;

pub use scene::Scene;

use crate::storage::Store;
use std::path::{Path, PathBuf};

/// What plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// One video file, in a loop.
    Video,
    /// A video from a folder, another one with each period of the schedule.
    Folder,
    /// An animated GIF or WebP.
    Anim,
    /// A scene drawn by rwall.
    Scene,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Video, Kind::Folder, Kind::Anim, Kind::Scene];

    pub fn key(self) -> &'static str {
        match self {
            Kind::Video => "video",
            Kind::Folder => "folder",
            Kind::Anim => "anim",
            Kind::Scene => "scene",
        }
    }

    pub fn from_key(k: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|x| x.key() == k)
    }

    /// Text in the settings window (translated by the caller).
    pub fn label(self) -> &'static str {
        match self {
            Kind::Video => "A video",
            Kind::Folder => "Videos from a folder",
            Kind::Anim => "An animated picture (GIF, WebP)",
            Kind::Scene => "A scene drawn by rwall",
        }
    }
}

/// How a frame fills each screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveFit {
    /// Covers the screen, cutting what sticks out.
    Fill,
    /// All of it, with black bars.
    Fit,
}

impl LiveFit {
    pub const ALL: [LiveFit; 2] = [LiveFit::Fill, LiveFit::Fit];

    pub fn key(self) -> &'static str {
        match self {
            LiveFit::Fill => "fill",
            LiveFit::Fit => "fit",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LiveFit::Fill => "Fill",
            LiveFit::Fit => "Fit",
        }
    }
}

/// When playing stops for a while, to leave the computer alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseWhen {
    /// A program or a game fills a screen.
    FullScreen,
    /// Every screen is covered by a maximized window.
    Maximized,
    /// The computer is locked or the screen is off.
    Locked,
    /// The computer runs on its battery.
    Battery,
}

impl PauseWhen {
    pub const ALL: [PauseWhen; 4] = [PauseWhen::FullScreen, PauseWhen::Maximized, PauseWhen::Locked, PauseWhen::Battery];

    pub fn key(self) -> &'static str {
        match self {
            PauseWhen::FullScreen => "fullscreen",
            PauseWhen::Maximized => "maximized",
            PauseWhen::Locked => "locked",
            PauseWhen::Battery => "battery",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PauseWhen::FullScreen => "A program or a game is in full screen",
            PauseWhen::Maximized => "Maximized windows cover every screen",
            PauseWhen::Locked => "The computer is locked or the screen is off",
            PauseWhen::Battery => "The computer runs on battery",
        }
    }
}

/// The settings of the dynamic wallpaper (`live.*` in `settings.ini`).
#[derive(Clone, Debug, PartialEq)]
pub struct LiveSettings {
    /// Playing behind the icons.
    pub on: bool,
    pub kind: Kind,
    pub video: PathBuf,
    pub folder: PathBuf,
    pub subfolders: bool,
    pub anim: PathBuf,
    pub scene: Scene,
    pub fit: LiveFit,
    pub pauses: Vec<PauseWhen>,
    /// The video of the folder playing now.
    pub playing: PathBuf,
}

impl Default for LiveSettings {
    fn default() -> Self {
        LiveSettings {
            on: false,
            kind: Kind::Scene,
            video: PathBuf::new(),
            folder: PathBuf::new(),
            subfolders: true,
            anim: PathBuf::new(),
            scene: Scene::Aurora,
            fit: LiveFit::Fill,
            pauses: PauseWhen::ALL.to_vec(),
            playing: PathBuf::new(),
        }
    }
}

impl LiveSettings {
    pub fn load(s: &Store) -> LiveSettings {
        let d = LiveSettings::default();
        let path = |k: &str| PathBuf::from(s.get(k).unwrap_or(""));
        LiveSettings {
            on: s.get("live").is_some_and(|v| v == "1"),
            kind: s.get("live.kind").and_then(Kind::from_key).unwrap_or(d.kind),
            video: path("live.video"),
            folder: path("live.folder"),
            subfolders: s.get("live.subfolders").map_or(d.subfolders, |v| v != "0"),
            anim: path("live.anim"),
            scene: s.get("live.scene").and_then(Scene::from_key).unwrap_or(d.scene),
            fit: if s.get("live.fit") == Some("fit") { LiveFit::Fit } else { LiveFit::Fill },
            pauses: match s.get("live.pause") {
                Some(list) => PauseWhen::ALL.into_iter().filter(|p| list.split(',').any(|k| k.trim() == p.key())).collect(),
                None => d.pauses,
            },
            playing: path("live.playing"),
        }
    }

    pub fn save(&self, s: &mut Store) {
        s.set("live", if self.on { "1" } else { "0" });
        s.set("live.kind", self.kind.key());
        s.set("live.video", self.video.to_string_lossy());
        s.set("live.folder", self.folder.to_string_lossy());
        s.set("live.subfolders", if self.subfolders { "1" } else { "0" });
        s.set("live.anim", self.anim.to_string_lossy());
        s.set("live.scene", self.scene.key());
        s.set("live.fit", self.fit.key());
        s.set("live.pause", self.pauses.iter().map(|p| p.key()).collect::<Vec<_>>().join(","));
        s.set("live.playing", self.playing.to_string_lossy());
    }

    pub fn pause_on(&self, p: PauseWhen) -> bool {
        self.pauses.contains(&p)
    }

    pub fn toggle_pause(&mut self, p: PauseWhen) {
        if self.pause_on(p) {
            self.pauses.retain(|x| *x != p);
        } else {
            self.pauses.push(p);
            self.pauses.sort_by_key(|x| PauseWhen::ALL.iter().position(|y| y == x));
        }
    }

    /// The file chosen for `kind` (empty for a scene).
    pub fn file(&self, kind: Kind) -> &Path {
        match kind {
            Kind::Video => &self.video,
            Kind::Folder => &self.folder,
            Kind::Anim => &self.anim,
            Kind::Scene => Path::new(""),
        }
    }

    /// What the player shows now, or None when nothing is chosen (or the
    /// folder has no video picked yet: see [`pick_video`]).
    pub fn show(&self) -> Option<Show> {
        let file = |p: &Path| (!p.as_os_str().is_empty()).then(|| p.to_path_buf());
        match self.kind {
            Kind::Video => file(&self.video).map(Show::Video),
            Kind::Folder => file(&self.playing).filter(|p| p.starts_with(&self.folder) && !self.folder.as_os_str().is_empty()).map(Show::Video),
            Kind::Anim => file(&self.anim).map(Show::Anim),
            Kind::Scene => Some(Show::Scene(self.scene)),
        }
    }

    pub fn spec(&self) -> Option<Spec> {
        Some(Spec { show: self.show()?, fit: self.fit, pauses: self.pauses.clone() })
    }
}

/// What the player shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Show {
    Video(PathBuf),
    Anim(PathBuf),
    Scene(Scene),
}

impl Show {
    /// The file shown, if any.
    pub fn file(&self) -> Option<&Path> {
        match self {
            Show::Video(p) | Show::Anim(p) => Some(p),
            Show::Scene(_) => None,
        }
    }
}

/// What to play and how.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub show: Show,
    pub fit: LiveFit,
    pub pauses: Vec<PauseWhen>,
}

/// Why playing stopped for a while.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Asked from the menu of the icon.
    User,
    When(PauseWhen),
}

/// What the player reports.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Off,
    Playing,
    Paused(Reason),
    /// It cannot play; the text says why (English, from the system).
    Failed(String),
}

/// Video files the player is offered (Media Foundation decides what plays).
pub const VIDEO_EXTENSIONS: [&str; 10] = ["mp4", "m4v", "mov", "wmv", "avi", "mkv", "webm", "mpg", "mpeg", "3gp"];
/// Animated pictures.
pub const ANIM_EXTENSIONS: [&str; 2] = ["gif", "webp"];

fn has_ext(p: &Path, list: &[&str]) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| list.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

pub fn is_video(p: &Path) -> bool {
    has_ext(p, &VIDEO_EXTENSIONS)
}

pub fn is_anim(p: &Path) -> bool {
    has_ext(p, &ANIM_EXTENSIONS)
}

/// A video of `folder` at random, other than `not` when there are others.
pub fn pick_video(folder: &Path, subfolders: bool, not: &Path) -> Option<PathBuf> {
    if folder.as_os_str().is_empty() {
        return None;
    }
    let all = crate::sources::folders::list_with(&[folder.to_path_buf()], subfolders, is_video);
    let others: Vec<&PathBuf> = all.iter().filter(|p| p.as_path() != not).collect();
    let pool: Vec<&PathBuf> = if others.is_empty() { all.iter().collect() } else { others };
    if pool.is_empty() {
        return None;
    }
    Some(pool[(crate::sources::seed() % pool.len() as u64) as usize].clone())
}

/// A still of what `show` plays, at most `w`×`h`, for the settings window.
/// Slow for a video or a big animation: call it on a thread of its own.
pub fn preview(show: &Show, w: u32, h: u32) -> Option<tiny_skia::Pixmap> {
    match show {
        Show::Anim(p) => crate::picture::thumbnail(p, w, h),
        #[cfg(windows)]
        Show::Video(p) => {
            let (vw, vh, rgba) = thumb::video(p, w, h)?;
            tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(vw, vh)?)
        }
        #[cfg(not(windows))]
        Show::Video(_) => None,
        Show::Scene(s) => {
            let painter = scene::Painter::new(*s);
            let (sw, sh) = painter.size();
            let mut buf = Vec::new();
            painter.frame(12.0, &mut buf);
            let mut full = tiny_skia::Pixmap::new(sw, sh)?;
            for (d, px) in full.pixels_mut().iter_mut().zip(&buf) {
                *d = tiny_skia::ColorU8::from_rgba((px >> 16) as u8, (px >> 8) as u8, *px as u8, 255).premultiply();
            }
            let k = (w as f32 / sw as f32).min(h as f32 / sh as f32).min(1.0);
            let (tw, th) = (((sw as f32 * k) as u32).max(1), ((sh as f32 * k) as u32).max(1));
            let mut small = tiny_skia::Pixmap::new(tw, th)?;
            let paint = tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
            small.draw_pixmap(0, 0, full.as_ref(), &paint, tiny_skia::Transform::from_scale(k, k), None);
            Some(small)
        }
    }
}

/// A rectangle in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Px {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Where a frame of `src` pixels goes on the screen `dst`: the part of the
/// frame that shows (left, top, right, bottom, from 0 to 1) and where it
/// lands. Fill cuts the frame evenly on both sides; Fit leaves bars.
pub fn place(src: (u32, u32), dst: Px, fit: LiveFit) -> ([f32; 4], Px) {
    let (sw, sh) = (src.0.max(1) as f64, src.1.max(1) as f64);
    let (dw, dh) = (dst.w.max(1) as f64, dst.h.max(1) as f64);
    let (sa, da) = (sw / sh, dw / dh);
    match fit {
        LiveFit::Fill => {
            if sa > da {
                // Wider than the screen: cut the sides.
                let keep = da / sa;
                let m = ((1.0 - keep) / 2.0) as f32;
                ([m, 0.0, 1.0 - m, 1.0], dst)
            } else {
                let keep = sa / da;
                let m = ((1.0 - keep) / 2.0) as f32;
                ([0.0, m, 1.0, 1.0 - m], dst)
            }
        }
        LiveFit::Fit => {
            let k = (dw / sw).min(dh / sh);
            let (w, h) = ((sw * k).round() as i32, (sh * k).round() as i32);
            let r = Px { x: dst.x + (dst.w - w) / 2, y: dst.y + (dst.h - h) / 2, w, h };
            ([0.0, 0.0, 1.0, 1.0], r)
        }
    }
}

/// The size of the picture made for a screen of `screen` pixels from a frame
/// of `fw`×`fh` (see [`compose`]): the part of the frame that shows (Fill),
/// or the frame with bars (Fit), at the frame's own scale. The screen
/// stretches it.
pub fn composed_size(fw: u32, fh: u32, screen: (i32, i32), fit: LiveFit) -> (u32, u32) {
    let (fw, fh) = (fw.max(1), fh.max(1));
    let full = Px { x: 0, y: 0, w: screen.0.max(1), h: screen.1.max(1) };
    match fit {
        LiveFit::Fill => {
            let (src, _) = place((fw, fh), full, fit);
            let w = ((src[2] - src[0]) * fw as f32).round().max(1.0) as u32;
            let h = ((src[3] - src[1]) * fh as f32).round().max(1.0) as u32;
            (w.min(fw), h.min(fh))
        }
        LiveFit::Fit => {
            let a = full.w as f64 / full.h as f64;
            if fw as f64 / fh as f64 > a {
                (fw, ((fw as f64 / a).round() as u32).max(fh))
            } else {
                (((fh as f64 * a).round() as u32).max(fw), fh)
            }
        }
    }
}

/// Makes the picture for one screen from a frame (`0x00RRGGBB` rows): cut
/// evenly (Fill) or with black bars (Fit), at the size [`composed_size`]
/// gives. `out` is reused from frame to frame.
pub fn compose(px: &[u32], fw: u32, fh: u32, screen: (i32, i32), fit: LiveFit, out: &mut Vec<u32>) -> (u32, u32) {
    let (w, h) = composed_size(fw, fh, screen, fit);
    let (fw, fh, w, h) = (fw as usize, fh as usize, w as usize, h as usize);
    out.clear();
    match fit {
        LiveFit::Fill => {
            let (x0, y0) = ((fw - w) / 2, (fh - h) / 2);
            for y in 0..h {
                let row = (y0 + y) * fw + x0;
                out.extend_from_slice(&px[row..row + w]);
            }
        }
        LiveFit::Fit => {
            out.resize(w * h, 0);
            let (x0, y0) = ((w - fw) / 2, (h - fh) / 2);
            for y in 0..fh {
                out[(y0 + y) * w + x0..(y0 + y) * w + x0 + fw].copy_from_slice(&px[y * fw..(y + 1) * fw]);
            }
        }
    }
    (w as u32, h as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_for_each_screen() {
        // A 4×2 frame numbered by pixel.
        let px: Vec<u32> = (0..8).collect();
        let mut out = Vec::new();
        // Same shape as the screen: all of it.
        assert_eq!(compose(&px, 4, 2, (1920, 960), LiveFit::Fill, &mut out), (4, 2));
        assert_eq!(out, px);
        // A square screen: the middle columns.
        assert_eq!(compose(&px, 4, 2, (1000, 1000), LiveFit::Fill, &mut out), (2, 2));
        assert_eq!(out, vec![1, 2, 5, 6]);
        // Fit on a square screen: bars above and below.
        assert_eq!(compose(&px, 4, 2, (1000, 1000), LiveFit::Fit, &mut out), (4, 4));
        assert_eq!(out, vec![0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 0, 0, 0, 0]);
        // Fit on a very wide screen: bars at the sides.
        assert_eq!(compose(&px, 4, 2, (4000, 1000), LiveFit::Fit, &mut out), (8, 2));
        assert_eq!(&out[..8], &[0, 0, 0, 1, 2, 3, 0, 0]);
        assert_eq!(composed_size(960, 540, (1920, 1080), LiveFit::Fit), (960, 540));
        assert_eq!(composed_size(960, 540, (1080, 1920), LiveFit::Fill), (304, 540));
    }

    #[test]
    fn settings_round_trip() {
        let mut l = LiveSettings {
            on: true,
            kind: Kind::Folder,
            video: r"C:\V\a b.mp4".into(),
            folder: PathBuf::from("Vídeos"),
            subfolders: false,
            anim: "x.gif".into(),
            scene: Scene::Sea,
            fit: LiveFit::Fit,
            pauses: vec![PauseWhen::Locked],
            playing: Path::new("Vídeos").join("c.mkv"),
        };
        l.toggle_pause(PauseWhen::FullScreen);
        assert_eq!(l.pauses, vec![PauseWhen::FullScreen, PauseWhen::Locked]);
        let mut s = Store::default();
        l.save(&mut s);
        let back = LiveSettings::load(&Store::parse(&s.serialize("rwall")));
        assert_eq!(back, l);
        assert_eq!(back.show(), Some(Show::Video(Path::new("Vídeos").join("c.mkv"))));
        // A video left from another folder does not play.
        let other = LiveSettings { folder: PathBuf::from("Outros"), ..back };
        assert_eq!(other.show(), None);
        let d = LiveSettings::load(&Store::parse("live.kind=zzz\nlive.pause=battery, nope\n"));
        assert!(!d.on);
        assert_eq!(d.kind, Kind::Scene);
        assert_eq!(d.pauses, vec![PauseWhen::Battery]);
        assert_eq!(LiveSettings::load(&Store::default()).pauses, PauseWhen::ALL.to_vec());
        assert_eq!(d.show(), Some(Show::Scene(Scene::Aurora)));
        assert_eq!(LiveSettings { kind: Kind::Video, ..d }.show(), None);
    }

    #[test]
    fn fill_cuts_evenly_and_fit_leaves_bars() {
        let screen = Px { x: 1920, y: 0, w: 1920, h: 1080 };
        // Same shape: all of it, everywhere.
        assert_eq!(place((1280, 720), screen, LiveFit::Fill), ([0.0, 0.0, 1.0, 1.0], screen));
        assert_eq!(place((1280, 720), screen, LiveFit::Fit).1, screen);
        // A square video on a wide screen.
        let (src, dst) = place((1000, 1000), screen, LiveFit::Fill);
        assert_eq!(dst, screen);
        assert!((src[1] - 0.21875).abs() < 1e-4 && (src[3] - 0.78125).abs() < 1e-4 && src[0] == 0.0);
        let (src, dst) = place((1000, 1000), screen, LiveFit::Fit);
        assert_eq!(src, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(dst, Px { x: 1920 + 420, y: 0, w: 1080, h: 1080 });
        // A very wide one on a portrait screen.
        let tall = Px { x: 0, y: -200, w: 1080, h: 1920 };
        let (src, _) = place((3840, 1080), tall, LiveFit::Fill);
        assert!(src[0] > 0.4 && src[2] < 0.6 && src[1] == 0.0 && src[3] == 1.0);
        let (_, dst) = place((3840, 1080), tall, LiveFit::Fit);
        assert_eq!((dst.w, dst.y), (1080, -200 + (1920 - 304) / 2));
    }

    #[test]
    fn videos_from_a_folder() {
        let d = std::env::temp_dir().join(format!("rwall-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        for f in ["a.mp4", "b.MKV", "c.txt", "sub/d.webm"] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        assert!(is_video(Path::new("x.WMV")) && !is_video(Path::new("x.gif")) && is_anim(Path::new("x.GIF")));
        for _ in 0..10 {
            let v = pick_video(&d, false, &d.join("a.mp4")).unwrap();
            assert_eq!(v, d.join("b.MKV"));
        }
        assert!(pick_video(&d.join("sub"), false, &d.join("sub/d.webm")).is_some());
        assert!(pick_video(&d.join("nothing"), true, Path::new("")).is_none());
        assert!(pick_video(Path::new(""), true, Path::new("")).is_none());
        let _ = std::fs::remove_dir_all(d);
    }
}
