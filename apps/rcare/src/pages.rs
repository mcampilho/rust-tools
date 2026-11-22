//! The pages of the sidebar, in order. A page is what a small app of
//! rust-tools shows in its window; here it sits to the right of the sidebar.

/// A page in the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageInfo {
    /// Stable name, saved in the settings.
    pub id: &'static str,
    /// What the sidebar shows (a translation key).
    pub label: &'static str,
    pub glyph: Glyph,
}

/// The small picture before a page's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// A sparkle: cleaning.
    Sparkle,
}

pub const PAGES: &[PageInfo] = &[PageInfo { id: "clean", label: "Disk Cleaner", glyph: Glyph::Sparkle }];

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
        }
        assert_eq!(index_of("nothing"), 0);
    }
}
