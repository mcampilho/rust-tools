//! Keyboard and mouse for the menus drawn in the window (`rt_gfx::menu`,
//! used where there are no Win32 menus). Each function says what the app is
//! to do next ([`Step`]); the app builds the items of a menu when told to
//! open it, and runs the commands.

use rt_gfx::menu::{Act, Item, Menu};
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// What the app does after the menus had their say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step<C> {
    /// Not for the menus: the app handles it.
    NotMine,
    /// The menus used it, and nothing changed on screen.
    Used,
    /// The menus used it: draw again.
    Redraw,
    /// Open top-level menu `index` (build its items, then [`open`]).
    Open { index: usize, keyboard: bool },
    /// Run a command (the menu is closed already).
    Run(C),
}

/// Opens top-level menu `index` with its items, under its title.
pub fn open<C: Copy>(m: &mut Menu<C>, index: usize, items: Vec<Item<C>>, keyboard: bool) {
    let Some(rect) = m.titles.get(index).map(|t| t.rect) else { return };
    m.open(Some(index), items, (rect.x, rect.bottom() + 2.0), keyboard);
}

fn act<C: Copy>(m: &mut Menu<C>, a: Act<C>) -> Step<C> {
    match a {
        Act::Run(c) => {
            m.close();
            Step::Run(c)
        }
        Act::Switch(dir) => {
            let n = m.titles.len() as isize;
            match m.top {
                Some(top) if n > 0 => Step::Open { index: (top as isize + dir).rem_euclid(n) as usize, keyboard: true },
                _ => Step::Redraw,
            }
        }
        Act::None => Step::Redraw,
    }
}

/// A key: F10 or Alt+letter opens a menu; with a menu open, the arrows,
/// Enter, Space, Esc and the letters work in it. `key` is the key without
/// modifiers.
pub fn key<C: Copy>(m: &mut Menu<C>, mods: ModifiersState, key: &Key) -> Step<C> {
    if !m.is_open() {
        let found = match key {
            Key::Named(NamedKey::F10) => Some(0),
            Key::Character(c) if mods.alt_key() && !mods.control_key() => {
                let k = c.chars().next().and_then(|c| c.to_lowercase().next());
                m.titles.iter().position(|t| t.mnemonic == k)
            }
            _ => None,
        };
        return match found {
            Some(index) if index < m.titles.len() => Step::Open { index, keyboard: true },
            _ => Step::NotMine,
        };
    }
    let a = match key {
        Key::Named(NamedKey::ArrowUp) => {
            m.up_down(-1);
            Act::None
        }
        Key::Named(NamedKey::ArrowDown) => {
            m.up_down(1);
            Act::None
        }
        Key::Named(NamedKey::ArrowLeft) => m.left(),
        Key::Named(NamedKey::ArrowRight) => m.right(),
        Key::Named(NamedKey::Enter | NamedKey::Space) => m.enter(),
        Key::Named(NamedKey::Escape) => {
            m.back();
            Act::None
        }
        Key::Character(c) => c.chars().next().map_or(Act::None, |c| m.mnemonic(c)),
        _ => Act::None,
    };
    act(m, a)
}

/// The pointer moved: the hot title follows it; with a menu open, the rows
/// light up and another title opens its menu. The flag says whether to
/// draw again even when the step is `NotMine`.
pub fn pointer<C: Copy>(m: &mut Menu<C>, x: f32, y: f32) -> (bool, Step<C>) {
    let title = m.title_at(x, y);
    let redraw = title != m.hot;
    m.hot = title;
    if !m.is_open() {
        return (redraw, Step::NotMine);
    }
    if let Some((l, i)) = m.row_at(x, y) {
        m.hover(l, i);
        return (true, Step::Redraw);
    }
    match title.filter(|i| m.top.is_some_and(|t| t != *i)) {
        Some(index) => (true, Step::Open { index, keyboard: false }),
        None => (redraw, if redraw { Step::Redraw } else { Step::Used }),
    }
}

/// A left click: on a title it opens or closes its menu; in a menu it
/// picks the row; elsewhere it closes the menu.
pub fn press<C: Copy>(m: &mut Menu<C>, x: f32, y: f32) -> Step<C> {
    if let Some(index) = m.title_at(x, y) {
        if m.top == Some(index) {
            m.close();
            return Step::Redraw;
        }
        return Step::Open { index, keyboard: false };
    }
    if !m.is_open() {
        return Step::NotMine;
    }
    match m.row_at(x, y) {
        Some((l, i)) => {
            let a = m.click(l, i);
            act(m, a)
        }
        None if !m.contains(x, y) => {
            m.close();
            Step::Redraw
        }
        None => Step::Used,
    }
}

/// The wheel over an open menu scrolls it (`down`: towards the end).
pub fn wheel<C: Copy>(m: &mut Menu<C>, at: Option<(f32, f32)>, down: bool) -> Step<C> {
    if !m.is_open() {
        return Step::NotMine;
    }
    match at.and_then(|(x, y)| m.row_at(x, y)) {
        Some((l, _)) => {
            m.wheel(l, if down { 2 } else { -2 });
            Step::Redraw
        }
        None => Step::Used,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_gfx::menu::Title;

    fn menu() -> Menu<u8> {
        Menu { titles: vec![Title::new("&File"), Title::new("&View")], ..Menu::default() }
    }

    #[test]
    fn opening_from_the_keyboard() {
        let mut m = menu();
        assert_eq!(key(&mut m, ModifiersState::empty(), &Key::Named(NamedKey::F10)), Step::Open { index: 0, keyboard: true });
        assert_eq!(key(&mut m, ModifiersState::ALT, &Key::Character("v".into())), Step::Open { index: 1, keyboard: true });
        assert_eq!(key(&mut m, ModifiersState::empty(), &Key::Character("v".into())), Step::NotMine);
        assert_eq!(key(&mut m, ModifiersState::ALT | ModifiersState::CONTROL, &Key::Character("v".into())), Step::NotMine);
        // Nothing open: the pointer and clicks elsewhere are the app's.
        assert_eq!(press(&mut m, -5.0, -5.0), Step::NotMine);
        assert_eq!(wheel(&mut m, None, true), Step::NotMine);
        assert_eq!(pointer(&mut m, -5.0, -5.0).1, Step::NotMine);
    }

    #[test]
    fn running_and_switching() {
        let mut m = menu();
        open(&mut m, 0, vec![Item::new("&Open", Some(7u8), true)], true);
        assert!(m.is_open());
        assert_eq!(act(&mut m, Act::Switch(-1)), Step::Open { index: 1, keyboard: true });
        assert_eq!(act(&mut m, Act::Run(7)), Step::Run(7));
        assert!(!m.is_open());
    }
}
