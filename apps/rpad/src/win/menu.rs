//! Native Win32 menu bar built from the command table.

use super::wide;
use crate::app::commands::{shortcut, Command, M};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub struct MenuBar {
    bar: HMENU,
    /// Command for each menu item id (id = index + FIRST_ID).
    ids: Vec<Command>,
}

const FIRST_ID: u32 = 1000;

fn item_text(label: &str, cmd: Command) -> String {
    match shortcut(cmd) {
        Some(k) => format!("{label}\t{k}"),
        None => label.to_string(),
    }
}

impl MenuBar {
    /// Builds the menu bar and attaches it to `hwnd` (replacing any previous one).
    pub fn install(hwnd: HWND, tree: Vec<(&'static str, Vec<M>)>) -> MenuBar {
        let mut ids = Vec::new();
        // SAFETY: creates menus owned by the window (destroyed with it or on
        // replacement below); all strings outlive the AppendMenuW calls.
        unsafe {
            let bar = CreateMenu();
            for (title, items) in tree {
                let sub = build(items, &mut ids);
                let t = wide(title);
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

    pub fn command(&self, id: u32) -> Option<Command> {
        id.checked_sub(FIRST_ID).and_then(|i| self.ids.get(i as usize)).copied()
    }

    /// Updates check marks and enabled states.
    pub fn sync(&self, checked: impl Fn(Command) -> bool, enabled: impl Fn(Command) -> bool) {
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

unsafe fn build(items: Vec<M>, ids: &mut Vec<Command>) -> HMENU {
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
                let t = wide(title);
                AppendMenuW(menu, MF_POPUP | MF_STRING, sub as usize, t.as_ptr());
            }
        }
    }
    menu
}

/// Shows a context menu at screen coordinates; returns the chosen command.
pub fn popup(hwnd: HWND, items: Vec<M>, x: i32, y: i32, checked: impl Fn(Command) -> bool, enabled: impl Fn(Command) -> bool) -> Option<Command> {
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
