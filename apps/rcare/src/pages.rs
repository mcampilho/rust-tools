//! The pages of the sidebar, in order. A page is what a small app of
//! rust-tools shows in its window; here it sits to the right of the sidebar.

/// Which page it is (what the window holds).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// rcare's own overview: junk, duplicates and large files in one look.
    Custom,
    /// rcare's own checks in one go: System Maintenance.
    Maint,
    /// The cleaner of rclean.
    Clean,
    /// The duplicate finder of rdup.
    Dups,
    /// The disk space analyzer of rsize.
    Size,
    /// The uninstaller of rinst.
    Inst,
    /// rcare's own: what starts with Windows.
    Startup,
}

/// A page in the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageInfo {
    /// Stable name, saved in the settings.
    pub id: &'static str,
    pub kind: Kind,
    /// What the sidebar shows (a translation key).
    pub label: &'static str,
    pub glyph: Glyph,
}

/// The small picture before a page's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// Four squares: an overview.
    Grid,
    /// A heartbeat: the computer's health.
    Pulse,
    /// A sparkle: cleaning.
    Sparkle,
    /// Two sheets, one over the other: copies.
    Copies,
    /// Bars of different lengths: sizes.
    Bars,
    /// A box with an arrow out of it: taking a program away.
    Remove,
    /// A power sign: what starts with the computer.
    Power,
}

pub const PAGES: &[PageInfo] = &[
    PageInfo { id: "maint", kind: Kind::Maint, label: "System Maintenance", glyph: Glyph::Pulse },
    PageInfo { id: "custom", kind: Kind::Custom, label: "Custom Clean", glyph: Glyph::Grid },
    PageInfo { id: "clean", kind: Kind::Clean, label: "Disk Cleaner", glyph: Glyph::Sparkle },
    PageInfo { id: "dups", kind: Kind::Dups, label: "Duplicate Files", glyph: Glyph::Copies },
    PageInfo { id: "size", kind: Kind::Size, label: "Large Files", glyph: Glyph::Bars },
    PageInfo { id: "inst", kind: Kind::Inst, label: "Uninstall Apps", glyph: Glyph::Remove },
    PageInfo { id: "startup", kind: Kind::Startup, label: "Startup Optimizer", glyph: Glyph::Power },
];

/// The index of the page saved as `id`; the first one when it is unknown.
pub fn index_of(id: &str) -> usize {
    PAGES.iter().position(|p| p.id == id).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_found() {
        for (i, p) in PAGES.iter().enumerate() {
            assert_eq!(index_of(p.id), i);
            assert_eq!(PAGES.iter().filter(|q| q.id == p.id).count(), 1);
            assert_eq!(PAGES.iter().filter(|q| q.kind == p.kind).count(), 1);
        }
        assert_eq!(index_of("nothing"), 0);
    }
}
