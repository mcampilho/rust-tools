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
    /// rcare's own: Game Mode, memory, processor, programs open.
    Speed,
    /// rcare's own: services of other companies that start with Windows.
    Services,
    /// rcare's own: what is added to the browsers.
    Extensions,
    /// rcare's own: invalid registry entries, and the registry's size.
    Registry,
    /// rcare's own: the antivirus, the firewall and Defender.
    Protect,
    /// rcare's own: TCP values and the DNS.
    Net,
    /// rcare's own: the traces Windows keeps.
    Privacy,
    /// rcare's own: the computer and its parts.
    Sys,
    /// rcare's own: Windows' repair tools.
    Repair,
    /// rcare's own: the slow work at a set time.
    Schedule,
    /// rcare's own: File Tools.
    Files,
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
    /// A gauge: speed.
    Gauge,
    /// Two cogs: services.
    Cogs,
    /// A puzzle piece: extensions.
    Puzzle,
    /// A stack of blocks: the registry.
    Blocks,
    /// A shield: protection.
    Shield,
    /// A globe: the network.
    Globe,
    /// An eye: privacy.
    Eye,
    /// A screen: the computer.
    Monitor,
    /// A wrench: repairs.
    Wrench,
    /// A clock: the scheduler.
    Clock,
    /// A folder: the file tools.
    Folder,
}

pub const PAGES: &[PageInfo] = &[
    PageInfo { id: "maint", kind: Kind::Maint, label: "System Maintenance", glyph: Glyph::Pulse },
    PageInfo { id: "custom", kind: Kind::Custom, label: "Custom Clean", glyph: Glyph::Grid },
    PageInfo { id: "clean", kind: Kind::Clean, label: "Disk Cleaner", glyph: Glyph::Sparkle },
    PageInfo { id: "dups", kind: Kind::Dups, label: "Duplicate Files", glyph: Glyph::Copies },
    PageInfo { id: "size", kind: Kind::Size, label: "Large Files", glyph: Glyph::Bars },
    PageInfo { id: "inst", kind: Kind::Inst, label: "Uninstall Apps", glyph: Glyph::Remove },
    PageInfo { id: "speed", kind: Kind::Speed, label: "Speed Up", glyph: Glyph::Gauge },
    PageInfo { id: "startup", kind: Kind::Startup, label: "Startup Optimizer", glyph: Glyph::Power },
    PageInfo { id: "services", kind: Kind::Services, label: "Program Deactivator", glyph: Glyph::Cogs },
    PageInfo { id: "extensions", kind: Kind::Extensions, label: "Extensions", glyph: Glyph::Puzzle },
    PageInfo { id: "registry", kind: Kind::Registry, label: "Registry Cleaner", glyph: Glyph::Blocks },
    PageInfo { id: "protect", kind: Kind::Protect, label: "Protect System", glyph: Glyph::Shield },
    PageInfo { id: "net", kind: Kind::Net, label: "Internet Booster", glyph: Glyph::Globe },
    PageInfo { id: "privacy", kind: Kind::Privacy, label: "Privacy Sweep", glyph: Glyph::Eye },
    PageInfo { id: "sysinfo", kind: Kind::Sys, label: "System Information", glyph: Glyph::Monitor },
    PageInfo { id: "repair", kind: Kind::Repair, label: "Repair", glyph: Glyph::Wrench },
    PageInfo { id: "schedule", kind: Kind::Schedule, label: "Scheduler", glyph: Glyph::Clock },
    PageInfo { id: "files", kind: Kind::Files, label: "File Tools", glyph: Glyph::Folder },
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
