//! Resident mode on Linux: the notification-area icon and the socket for
//! later launches. Without an icon host, a hidden window could not be
//! reached or closed, so the app then runs as a normal window.

use rt_sys::linux::shell::{Config, Msg, Shell, TrayLabels};

/// Starts the icon of `app` ("Open rsize", "Exit" already translated);
/// None when the desktop has no host for it. Only a resident app listens
/// for later launches, as on Windows.
pub fn start(app: &'static str, icon: fn(u32) -> Vec<u8>, open: String, exit: String, send: Box<dyn Fn(Msg) + Send + Sync>) -> Option<Shell> {
    let labels = TrayLabels { tip: app.into(), open, exit };
    let cfg = Config { app, icon, tray: Some(labels), listen_without_icon: false };
    let shell = Shell::new(cfg, send);
    shell.has_icon().then_some(shell)
}
