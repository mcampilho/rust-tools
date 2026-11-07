//! Linux shell integration (see `rt-sys`): single instance over a Unix
//! socket, and the notification-area icon through StatusNotifierItem.
//! Desktops without an icon host (plain GNOME, for example) have no icon:
//! [`Shell::has_icon`] tells the caller.

use rt_sys::linux::shell::{self as sh, Config, Msg, TrayLabels};

const APP: &str = "rcalc";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellEvent {
    /// Show and focus the calculator window.
    Activate,
    /// Quit the resident instance.
    Exit,
}

/// If another instance is running, asks it to show its window and returns
/// true. Called before anything else at startup so the second launch exits
/// in milliseconds.
pub fn activate_existing() -> bool {
    sh::hand_off(APP, &[], true)
}

pub struct Shell {
    inner: sh::Shell,
}

fn labels(tip: &str, open: &str, exit: &str) -> TrayLabels {
    TrayLabels { tip: tip.to_string(), open: open.to_string(), exit: exit.to_string() }
}

impl Shell {
    /// Starts listening for later launches. With `tray`, adds the
    /// notification-area icon if the desktop has a host for it.
    pub fn new(tray: bool, tip: &str, menu: (&str, &str), sink: Box<dyn Fn(ShellEvent) + Send + Sync>) -> Option<Shell> {
        let cfg = Config { app: APP, icon: crate::ui::appicon::render, tray: tray.then(|| labels(tip, menu.0, menu.1)), listen_without_icon: true };
        let inner = sh::Shell::new(
            cfg,
            Box::new(move |m| {
                sink(match m {
                    Msg::Open(_) => ShellEvent::Activate,
                    Msg::Quit => ShellEvent::Exit,
                })
            }),
        );
        Some(Shell { inner })
    }

    /// Whether the notification-area icon is showing.
    pub fn has_icon(&self) -> bool {
        self.inner.has_icon()
    }

    /// Updates the tray menu labels (after a language change).
    pub fn set_labels(&self, tip: &str, open: &str, exit: &str) {
        self.inner.set_labels(labels(tip, open, exit));
    }
}
