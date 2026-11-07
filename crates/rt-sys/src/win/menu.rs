//! Native Win32 menu bar and context menus, built from the app's menu tree.

use super::wide;
use rt_core::menu::{MenuCommand, Tree, M};
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Menu item ids start here (id = index + FIRST_ID).
const FIRST_ID: u32 = 1000;

pub struct MenuBar<C> {
    bar: HMENU,
    /// Command for each menu item id.
    ids: Vec<C>,
}

fn item_text<C: MenuCommand>(label: &str, cmd: C) -> String {
    match cmd.shortcut() {
        Some(k) => format!("{label}\t{k}"),
        None => label.to_string(),
    }
}

impl<C: MenuCommand> MenuBar<C> {
    /// Builds the menu bar and attaches it to `hwnd` (replacing any previous one).
    pub fn install(hwnd: HWND, tree: Tree<C>) -> MenuBar<C> {
        let mut ids = Vec::new();
        // SAFETY: creates menus owned by the window (destroyed with it or on
        // replacement below); all strings outlive the AppendMenuW calls.
        unsafe {
            let bar = CreateMenu();
            for (title, items) in tree {
                let sub = build(items, &mut ids);
                let t = wide(rt_core::i18n::tr(title));
                AppendMenuW(bar, MF_POPUP | MF_STRING, sub as usize, t.as_ptr());
            }
            let old = GetMenu(hwnd);
            SetMenu(hwnd, bar);
            if !old.is_null() {
                DestroyMenu(old);
            }
            DrawMenuBar(hwnd);
            MenuBar { bar, ids }
        }
    }

    /// The command of a `WM_COMMAND` id.
    pub fn command(&self, id: u32) -> Option<C> {
        id.checked_sub(FIRST_ID).and_then(|i| self.ids.get(i as usize)).copied()
    }

    /// Updates check marks and enabled states.
    pub fn sync(&self, checked: impl Fn(C) -> bool, enabled: impl Fn(C) -> bool) {
        for (i, cmd) in self.ids.iter().enumerate() {
            let id = FIRST_ID + i as u32;
            // SAFETY: the ids belong to items of our live menu bar.
            unsafe {
                CheckMenuItem(self.bar, id, MF_BYCOMMAND | if checked(*cmd) { MF_CHECKED } else { MF_UNCHECKED });
                EnableMenuItem(self.bar, id, MF_BYCOMMAND | if enabled(*cmd) { MF_ENABLED } else { MF_GRAYED });
            }
        }
    }
}

unsafe fn build<C: MenuCommand>(items: Vec<M<C>>, ids: &mut Vec<C>) -> HMENU {
    let menu = CreatePopupMenu();
    for it in items {
        match it {
            M::Sep => {
                AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            }
            M::Item(cmd) => {
                let id = FIRST_ID + ids.len() as u32;
                ids.push(cmd);
                let t = wide(&item_text(&cmd.label(), cmd));
                AppendMenuW(menu, MF_STRING, id as usize, t.as_ptr());
            }
            M::Dyn(cmd, label) => {
                let id = FIRST_ID + ids.len() as u32;
                ids.push(cmd);
                let t = wide(&label);
                AppendMenuW(menu, MF_STRING, id as usize, t.as_ptr());
            }
            M::Sub(title, children) => {
                let sub = build(children, ids);
                let t = wide(rt_core::i18n::tr(title));
                AppendMenuW(menu, MF_POPUP | MF_STRING, sub as usize, t.as_ptr());
            }
        }
    }
    menu
}

/// Shows a context menu at screen coordinates; returns the chosen command.
pub fn popup<C: MenuCommand>(
    hwnd: HWND,
    items: Vec<M<C>>,
    x: i32,
    y: i32,
    checked: impl Fn(C) -> bool,
    enabled: impl Fn(C) -> bool,
) -> Option<C> {
    let mut ids = Vec::new();
    // SAFETY: temporary popup menu, destroyed before returning.
    unsafe {
        let menu = build(items, &mut ids);
        for (i, cmd) in ids.iter().enumerate() {
            let id = FIRST_ID + i as u32;
            CheckMenuItem(menu, id, MF_BYCOMMAND | if checked(*cmd) { MF_CHECKED } else { MF_UNCHECKED });
            EnableMenuItem(menu, id, MF_BYCOMMAND | if enabled(*cmd) { MF_ENABLED } else { MF_GRAYED });
        }
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, 0, hwnd, std::ptr::null());
        DestroyMenu(menu);
        (cmd as u32).checked_sub(FIRST_ID).and_then(|i| ids.get(i as usize)).copied()
    }
}

/// Shows a flat context menu at the mouse pointer and returns the chosen
/// command. `None` entries are separators; `items` holds each command and
/// whether it is enabled.
pub fn popup_at_cursor<C: MenuCommand>(hwnd: HWND, items: &[Option<(C, bool)>]) -> Option<C> {
    // SAFETY: a temporary popup menu, destroyed before returning; strings
    // outlive the calls that use them.
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return None;
        }
        for (i, item) in items.iter().enumerate() {
            match item {
                Some((cmd, enabled)) => {
                    let t = wide(&item_text(&cmd.label(), *cmd));
                    AppendMenuW(
                        menu,
                        MF_STRING | if *enabled { MF_ENABLED } else { MF_GRAYED },
                        FIRST_ID as usize + i,
                        t.as_ptr(),
                    );
                }
                None => {
                    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                }
            }
        }
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let id = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, std::ptr::null());
        DestroyMenu(menu);
        (id as u32)
            .checked_sub(FIRST_ID)
            .and_then(|i| items.get(i as usize))
            .copied()
            .flatten()
            .map(|(c, _)| c)
    }
}
