//! Custom Clean: one look at the junk, the duplicate files and the large
//! files, side by side. It is rcare's own page: the analysis is done by the
//! pages of the small apps (the cleaner analyses, rdup searches the folder,
//! rsize reads it), so their results are ready when one of them is opened
//! from here with "Review". Nothing is deleted on this page.
//!
//! This module only lays out, finds what the pointer is on and draws; the
//! window (`shell.rs`) reads the other pages and fills a [`View`].

use crate::i18n::{tf, tr};
use rclean::ui::theme::Theme;
use rt_gfx::gfx::{Align, Color, FontId, Gfx, Rect};
use rt_ui::page::Effects;
use winit::window::CursorIcon;

/// A file this large or larger is a large file.
pub const LARGE: u64 = 100 << 20;

/// The three things looked at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Junk,
    Dups,
    Large,
}

impl Part {
    pub const ALL: [Part; 3] = [Part::Junk, Part::Dups, Part::Large];

    /// The card's title (a translation key).
    pub fn title(self) -> &'static str {
        match self {
            Part::Junk => "Junk Files",
            Part::Dups => "Duplicate Files",
            Part::Large => "Large Files",
        }
    }

    /// What the card is about (a translation key).
    pub fn about(self) -> &'static str {
        match self {
            Part::Junk => "Temporary files and the caches of the system, the browsers and the package managers.",
            Part::Dups => "Files with the same contents in the folder: the extra copies take this space.",
            Part::Large => "Files of 100 MB or more in the folder.",
        }
    }

    /// The page that shows it in full (an id of `pages::PAGES`).
    pub fn page_id(self) -> &'static str {
        match self {
            Part::Junk => "clean",
            Part::Dups => "dups",
            Part::Large => "size",
        }
    }
}

/// How far one part got.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Status {
    #[default]
    NotYet,
    Working,
    /// The space and how many (files; for duplicates, groups of copies).
    Done {
        bytes: u64,
        count: u64,
    },
    /// The page said why it could not.
    Failed(String),
}

/// What the pointer can be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Analyze,
    Change,
    /// The "Review" button of a card (index into `Part::ALL`).
    Review(usize),
}

/// What the page shows.
#[derive(Clone, Debug, Default)]
pub struct View {
    /// Where duplicates and large files are looked for, as shown.
    pub folder: String,
    pub status: [Status; 3],
    pub hot: Option<Hit>,
    /// The analysis can be started (nothing is running).
    pub can_analyze: bool,
}

impl View {
    /// What the junk and the extra copies take, when both are known.
    pub fn freeable(&self) -> Option<u64> {
        match (&self.status[0], &self.status[1]) {
            (Status::Done { bytes: a, .. }, Status::Done { bytes: b, .. }) => Some(a + b),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub area: Rect,
    pub analyze: Rect,
    pub change: Rect,
    pub cards: [Rect; 3],
    pub reviews: [Rect; 3],
    /// The line under the cards (what can be freed).
    pub total: Rect,
}

/// Where everything goes in `area`: the cards side by side, or one under
/// the other when the page is narrow.
pub fn layout(area: Rect, s: f32) -> Layout {
    let pad = (24.0 * s).round();
    let inner = Rect::new(area.x + pad, area.y + pad, (area.w - 2.0 * pad).max(0.0), (area.h - 2.0 * pad).max(0.0));
    let bh = (32.0 * s).round();
    // Title (34), about (26), folder line with its button (bh), gap, Analyze (bh), gap.
    let folder_y = inner.y + (66.0 * s).round();
    let change = Rect::new(inner.right() - (150.0 * s).round(), folder_y, (150.0 * s).round(), bh);
    let analyze = Rect::new(inner.x, folder_y + bh + (14.0 * s).round(), (180.0 * s).round(), bh);
    let top = analyze.bottom() + (22.0 * s).round();
    let gap = (14.0 * s).round();
    // One under the other only when the page is narrow and tall enough for
    // three readable cards; otherwise side by side (narrow cards on a small window).
    let room = inner.bottom() - top - (28.0 * s).round() - gap;
    let columns = inner.w >= 760.0 * s || room < 3.0 * 120.0 * s + 2.0 * gap;
    let mut cards = [Rect::default(); 3];
    if columns {
        let w = ((inner.w - 2.0 * gap) / 3.0).floor();
        let h = (200.0 * s).round().min(room.max(150.0 * s));
        for (i, c) in cards.iter_mut().enumerate() {
            *c = Rect::new(inner.x + i as f32 * (w + gap), top, w, h);
        }
    } else {
        // One under the other: as tall as the page lets them be, up to 150 px.
        let h = ((room - 2.0 * gap) / 3.0).clamp(120.0 * s, 150.0 * s).floor();
        for (i, c) in cards.iter_mut().enumerate() {
            *c = Rect::new(inner.x, top + i as f32 * (h + gap), inner.w, h);
        }
    }
    let reviews = cards.map(|c| {
        let m = (14.0 * s).round();
        if narrow_card(c, s) {
            // A narrow card: the button along its bottom.
            Rect::new(c.x + m, c.bottom() - bh - m, c.w - 2.0 * m, bh)
        } else {
            let w = (110.0 * s).round();
            Rect::new(c.right() - w - m, c.bottom() - bh - m, w, bh)
        }
    });
    let total = Rect::new(inner.x, cards[2].bottom().max(cards[0].bottom()) + gap, inner.w, (28.0 * s).round());
    Layout { area, analyze, change, cards, reviews, total }
}

/// A card too narrow for its text beside its button (a small window).
fn narrow_card(c: Rect, s: f32) -> bool {
    c.w < 260.0 * s
}

pub fn hit(l: &Layout, x: f32, y: f32) -> Option<Hit> {
    if l.analyze.contains(x, y) {
        Some(Hit::Analyze)
    } else if l.change.contains(x, y) {
        Some(Hit::Change)
    } else {
        l.reviews.iter().position(|r| r.contains(x, y)).map(Hit::Review)
    }
}

/// The lines a card shows under its title: the size, and what it counts (or
/// how far it got).
pub fn card_lines(part: Part, status: &Status) -> (String, String) {
    match status {
        Status::NotYet => (String::new(), tr("Not analyzed yet")),
        Status::Working => (String::new(), tr("Analyzing…")),
        Status::Failed(why) => (String::new(), why.clone()),
        Status::Done { bytes, count } => {
            let n = rsize::format::count(*count);
            let what = match part {
                Part::Dups => tf("{} groups of copies", &[&n]),
                _ => tf("{} files", &[&n]),
            };
            (rsize::format::size(*bytes), what)
        }
    }
}

/// Whether "Review" works for a part: once it is done.
pub fn can_review(status: &Status) -> bool {
    matches!(status, Status::Done { .. } | Status::Failed(_))
}

/// How a button looks: filled with the accent colour, under the pointer, usable.
#[derive(Clone, Copy)]
struct Look {
    primary: bool,
    hot: bool,
    on: bool,
}

fn button(g: &mut Gfx, r: Rect, label: &str, th: &Theme, s: f32, look: Look) {
    let Look { primary, hot, on } = look;
    let p = 12.5 * s;
    if primary {
        let fill = if !on {
            th.accent.mix(th.bg, 0.55)
        } else if hot {
            th.accent
        } else {
            th.accent.mix(th.bg, 0.12)
        };
        g.fill_rrect(r, 5.0 * s, fill);
        let fg = if th.dark { Color::rgb(0x000000) } else { Color::rgb(0xFFFFFF) };
        g.text_in(label, r, p, FontId::Semibold, fg, Align::Center);
    } else {
        g.fill_rrect(r, 5.0 * s, th.bg);
        if hot && on {
            g.fill_rrect(r, 5.0 * s, th.hover);
        }
        g.stroke_rrect(r, 5.0 * s, 1.0, th.border);
        g.text_in(label, r, p, FontId::Regular, if on { th.text } else { th.dim }, Align::Center);
    }
}

/// Draws the page.
pub fn draw(g: &mut Gfx, l: &Layout, th: &Theme, s: f32, v: &View) {
    use rt_core::i18n::strip_mnemonic;
    use rt_gfx::dialog::{ellipsize, wrap_words};
    g.fill_rect(l.area, th.bg);
    let pad = (24.0 * s).round();
    let x = l.area.x + pad;
    let w = (l.area.w - 2.0 * pad).max(0.0);
    let mut y = l.area.y + pad;
    g.text_in(&tr("Custom Clean"), Rect::new(x, y, w, 30.0 * s), 20.0 * s, FontId::Semibold, th.text, Align::Left);
    y += 34.0 * s;
    let about = ellipsize(
        g,
        &tr("Junk, duplicate and large files in one look. Nothing is deleted here: review each in its own page, and clean there."),
        w,
        12.5 * s,
        FontId::Regular,
    );
    g.text_in(&about, Rect::new(x, y, w, 22.0 * s), 12.5 * s, FontId::Regular, th.dim, Align::Left);
    // Where.
    let f = l.change;
    let where_r = Rect::new(x, f.y, (f.x - x - 12.0 * s).max(0.0), f.h);
    let where_t = ellipsize(g, &tf("Duplicates and large files are looked for in {}", &[&v.folder]), where_r.w, 12.5 * s, FontId::Regular);
    g.text_in(&where_t, where_r, 12.5 * s, FontId::Regular, th.text, Align::Left);
    let change = strip_mnemonic(&tr("Change &Folder..."));
    button(g, l.change, &change, th, s, Look { primary: false, hot: v.hot == Some(Hit::Change), on: v.can_analyze });
    let analyze = strip_mnemonic(&tr("&Analyze"));
    button(g, l.analyze, &analyze, th, s, Look { primary: true, hot: v.hot == Some(Hit::Analyze), on: v.can_analyze });
    // The cards.
    for (i, part) in Part::ALL.iter().enumerate() {
        let c = l.cards[i];
        g.fill_rrect(c, 8.0 * s, th.panel);
        g.stroke_rrect(c, 8.0 * s, 1.0, th.border);
        let ix = c.x + 16.0 * s;
        let iw = (c.w - 32.0 * s).max(0.0);
        // A low card (a small window) has no room for what it is about, and
        // puts the size beside what it counts.
        let narrow = narrow_card(c, s);
        let compact = c.h < 130.0 * s || narrow;
        let mut cy = c.y + 14.0 * s;
        g.text_in(&tr(part.title()), Rect::new(ix, cy, iw, 22.0 * s), 14.0 * s, FontId::Semibold, th.text, Align::Left);
        cy += 26.0 * s;
        if !compact {
            for line in wrap_words(g, &tr(part.about()), iw, 11.5 * s, FontId::Regular).into_iter().take(2) {
                g.text_in(&line, Rect::new(ix, cy, iw, 17.0 * s), 11.5 * s, FontId::Regular, th.dim, Align::Left);
                cy += 17.0 * s;
            }
        }
        let (big, small) = card_lines(*part, &v.status[i]);
        let r = l.reviews[i];
        let mut left = Rect::new(ix, r.y, (r.x - ix - 10.0 * s).max(0.0), r.h);
        let color = if *part == Part::Large { th.text } else { th.good };
        if narrow {
            // The size, then what it counts, above the button.
            if !big.is_empty() {
                g.text_in(&big, Rect::new(ix, r.y - 56.0 * s, iw, 28.0 * s), 20.0 * s, FontId::Semibold, color, Align::Left);
            }
            left = Rect::new(ix, r.y - 28.0 * s, iw, 24.0 * s);
        } else if !big.is_empty() && compact {
            let bw = g.measure(&big, 18.0 * s, FontId::Semibold);
            g.text_in(&big, left, 18.0 * s, FontId::Semibold, color, Align::Left);
            left = Rect::new(left.x + bw + 10.0 * s, left.y, (left.w - bw - 10.0 * s).max(0.0), left.h);
        } else if !big.is_empty() {
            g.text_in(&big, Rect::new(ix, r.y - 30.0 * s, left.w, 28.0 * s), 22.0 * s, FontId::Semibold, color, Align::Left);
        }
        let small = ellipsize(g, &small, left.w, 12.0 * s, FontId::Regular);
        let failed = matches!(v.status[i], Status::Failed(_));
        g.text_in(&small, left, 12.0 * s, FontId::Regular, if failed { th.warn } else { th.dim }, Align::Left);
        button(g, r, &tr("Review"), th, s, Look { primary: false, hot: v.hot == Some(Hit::Review(i)), on: can_review(&v.status[i]) });
    }
    if let Some(bytes) = v.freeable() {
        let t = tf("Up to {} can be freed: the junk and the extra copies.", &[&rsize::format::size(bytes)]);
        g.text_in(&t, l.total, 13.0 * s, FontId::Semibold, th.text, Align::Left);
    }
}

/// The page's state, as the window holds it (like the pages of the small apps).
#[derive(Default)]
pub struct CustomPage {
    pub layout: Layout,
    pub hot: Option<Hit>,
    pub cursor_icon: CursorIcon,
    /// What the page asks of the window (see `rt_ui::page`).
    pub fx: Effects,
    /// What it shows: the window fills the folder and the parts' status before drawing.
    pub view: View,
}

impl CustomPage {
    /// Lays out and draws the page in `area`.
    pub fn render(&mut self, g: &mut Gfx, area: Rect, th: &Theme, s: f32) {
        self.layout = layout(area, s);
        self.view.hot = self.hot;
        draw(g, &self.layout, th, s, &self.view);
    }

    /// Whether what the pointer is on does something now.
    fn works(&self, h: Hit) -> bool {
        match h {
            Hit::Analyze | Hit::Change => self.view.can_analyze,
            Hit::Review(i) => self.view.status.get(i).is_some_and(can_review),
        }
    }

    /// What the pointer is on; `blocked` while a message is open.
    pub fn pointer(&mut self, x: f32, y: f32, blocked: bool) {
        let hot = if blocked { None } else { hit(&self.layout, x, y).filter(|&h| self.works(h)) };
        if hot != self.hot {
            self.hot = hot;
            self.fx.redraw();
        }
        self.cursor_icon = if hot.is_some() { CursorIcon::Pointer } else { CursorIcon::Default };
    }

    pub fn pointer_left(&mut self) {
        if self.hot.take().is_some() {
            self.fx.redraw();
        }
    }

    pub fn release(&mut self) {}

    /// A click: what was clicked, for the window to do.
    pub fn press(&mut self) -> Option<Hit> {
        self.hot.filter(|&h| self.works(h))
    }

    /// The page's nodes for the screen readers: the buttons are 30 (Analyze),
    /// 31 (Change folder) and 32–34 (Review); the cards' texts 40–42.
    pub fn accessibility(
        &self,
        nodes: &mut Vec<(accesskit::NodeId, accesskit::Node)>,
        children: &mut Vec<accesskit::NodeId>,
    ) -> Option<accesskit::NodeId> {
        use accesskit::{Node, NodeId, Role};
        use rt_core::i18n::strip_mnemonic;
        let mut button = |id: u64, label: String, on: bool| {
            let mut n = Node::new(Role::Button);
            n.set_label(label);
            if on {
                n.add_action(accesskit::Action::Click);
            } else {
                n.set_disabled();
            }
            nodes.push((NodeId(id), n));
            children.push(NodeId(id));
        };
        button(30, strip_mnemonic(&tr("&Analyze")), self.view.can_analyze);
        button(31, strip_mnemonic(&tr("Change &Folder...")), self.view.can_analyze);
        for (i, part) in Part::ALL.iter().enumerate() {
            button(32 + i as u64, format!("{} — {}", tr("Review"), tr(part.title())), can_review(&self.view.status[i]));
        }
        for (i, part) in Part::ALL.iter().enumerate() {
            let (big, small) = card_lines(*part, &self.view.status[i]);
            let mut n = Node::new(Role::Label);
            n.set_value(format!("{}: {big} {small}", tr(part.title())));
            nodes.push((NodeId(40 + i as u64), n));
            children.push(NodeId(40 + i as u64));
        }
        None
    }

    /// The page has no rows for a screen reader to click.
    pub fn accessibility_click(&mut self, _i: usize) {}
}
/// How many files of the tree are large, and what they take together.
pub fn large_files(t: &rsize::model::Tree) -> (u64, u64) {
    let (mut bytes, mut count) = (0, 0);
    for id in 0..t.len() as rsize::model::Id {
        if !t.node(id).is_dir() {
            let b = t.size(id);
            if b >= LARGE {
                bytes += b;
                count += 1;
            }
        }
    }
    (bytes, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_are_found_and_cards_do_not_overlap() {
        for (w, s) in [(956.0, 1.0), (560.0, 1.0), (1900.0, 2.0)] {
            let l = layout(Rect::new(224.0, 0.0, w, 700.0 * s), s);
            assert_eq!(hit(&l, l.analyze.x + 3.0, l.analyze.y + 3.0), Some(Hit::Analyze));
            assert_eq!(hit(&l, l.change.x + 3.0, l.change.y + 3.0), Some(Hit::Change));
            for (i, r) in l.reviews.iter().enumerate() {
                assert_eq!(hit(&l, r.x + 3.0, r.y + 3.0), Some(Hit::Review(i)));
                assert!(r.right() <= l.cards[i].right() && r.bottom() <= l.cards[i].bottom());
            }
            for a in 0..3 {
                for b in a + 1..3 {
                    let (p, q) = (l.cards[a], l.cards[b]);
                    let apart = p.right() <= q.x || q.right() <= p.x || p.bottom() <= q.y || q.bottom() <= p.y;
                    assert!(apart, "cards {a} and {b} overlap at width {w}");
                }
            }
            assert!(l.cards.iter().all(|c| c.x >= l.area.x && c.right() <= l.area.right()));
        }
    }

    #[test]
    fn what_a_card_says() {
        crate::i18n::set_lang(crate::i18n::Lang::EnUS);
        assert_eq!(card_lines(Part::Junk, &Status::NotYet).1, "Not analyzed yet");
        let (big, small) = card_lines(Part::Dups, &Status::Done { bytes: 3 << 20, count: 4 });
        assert!(!big.is_empty() && small.contains('4') && small.contains("groups"));
        assert!(can_review(&Status::Done { bytes: 0, count: 0 }) && !can_review(&Status::Working));
        let v =
            View { status: [Status::Done { bytes: 5, count: 1 }, Status::Done { bytes: 7, count: 1 }, Status::NotYet], ..View::default() };
        assert_eq!(v.freeable(), Some(12));
        assert_eq!(View::default().freeable(), None);
    }

    #[test]
    fn every_part_opens_a_page() {
        for p in Part::ALL {
            assert!(crate::pages::PAGES.iter().any(|q| q.id == p.page_id()));
        }
    }
}
