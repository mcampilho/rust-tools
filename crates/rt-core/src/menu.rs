//! The menu tree. An app describes its menus once, as a list of titles with
//! their items; native Win32 menus (`rt-sys`) and the menus drawn in the
//! window (`rt-gfx`) are both built from it.

/// A command that can appear in a menu.
pub trait MenuCommand: Copy + PartialEq {
    /// The label in the current language, with its `&` access key.
    fn label(self) -> String;
    /// The keyboard shortcut shown next to the label, e.g. `Ctrl+O`.
    fn shortcut(self) -> Option<&'static str>;
}

/// One entry of a menu.
#[derive(Debug, Clone, PartialEq)]
pub enum M<C> {
    Item(C),
    /// An item with a label chosen at run time (recent files).
    Dyn(C, String),
    Sep,
    /// A submenu: its title (an untranslated key) and items.
    Sub(&'static str, Vec<M<C>>),
}

/// The menu bar: each top-level title (an untranslated key) with its items.
pub type Tree<C> = Vec<(&'static str, Vec<M<C>>)>;

impl<C: Copy> M<C> {
    /// Calls `f` for every command in `items`, submenus included.
    pub fn each_command(items: &[M<C>], f: &mut impl FnMut(C)) {
        for it in items {
            match it {
                M::Item(c) | M::Dyn(c, _) => f(*c),
                M::Sub(_, children) => M::each_command(children, f),
                M::Sep => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_found_in_submenus() {
        let items = vec![M::Item(1), M::Sep, M::Sub("More", vec![M::Item(2), M::Dyn(3, "x".into())])];
        let mut seen = Vec::new();
        M::each_command(&items, &mut |c| seen.push(c));
        assert_eq!(seen, vec![1, 2, 3]);
    }
}
