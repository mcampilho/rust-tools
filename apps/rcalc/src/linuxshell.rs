//! Linux shell integration: single instance over a Unix socket, and the
//! notification-area icon through StatusNotifierItem (D-Bus).
//!
//! A socket client that writes [`ACTIVATE`] asks the running instance to show
//! its window. Desktops without a StatusNotifierItem host (plain GNOME, for
//! example) have no icon: [`Shell::has_icon`] tells the caller.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;

const ACTIVATE: &[u8] = b"A";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellEvent {
    /// Show and focus the calculator window.
    Activate,
    /// Quit the resident instance.
    Exit,
}

type Sink = Arc<dyn Fn(ShellEvent) + Send + Sync>;

fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("rcalc.sock"),
        // SAFETY: getuid has no preconditions and cannot fail.
        None => std::env::temp_dir().join(format!("rcalc-{}.sock", unsafe { libc::getuid() })),
    }
}

/// If another instance is running, asks it to show its window and returns
/// true. Called before anything else at startup so the second launch exits
/// in milliseconds.
pub fn activate_existing() -> bool {
    UnixStream::connect(socket_path()).is_ok_and(|mut s| s.write_all(ACTIVATE).is_ok())
}

/// Binds the instance socket. Fails if a live instance owns it (only with
/// `--new-window`); a socket left behind by a crash is replaced.
fn bind(path: &PathBuf) -> Option<UnixListener> {
    if UnixStream::connect(path).is_ok() {
        return None;
    }
    let _ = std::fs::remove_file(path);
    UnixListener::bind(path).ok()
}

fn listen(listener: UnixListener, sink: Sink) {
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut byte = [0u8; 1];
            if stream.read_exact(&mut byte).is_ok() && byte == ACTIVATE {
                sink(ShellEvent::Activate);
            }
        }
    });
}

struct Tray {
    sink: Sink,
    tip: String,
    open: String,
    exit: String,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "rcalc".into()
    }

    fn title(&self) -> String {
        self.tip.clone()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let size = 32;
        // RGBA to the ARGB byte order of the protocol.
        let mut data = crate::ui::appicon::render(size as u32);
        data.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p.rotate_right(1));
        vec![ksni::Icon { width: size, height: size, data }]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        (self.sink)(ShellEvent::Activate);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: self.open.clone(),
                activate: Box::new(|t: &mut Self| (t.sink)(ShellEvent::Activate)),
                ..StandardItem::default()
            }
            .into(),
            StandardItem {
                label: self.exit.clone(),
                activate: Box::new(|t: &mut Self| (t.sink)(ShellEvent::Exit)),
                ..StandardItem::default()
            }
            .into(),
        ]
    }
}

pub struct Shell {
    socket: Option<PathBuf>,
    tray: Option<ksni::blocking::Handle<Tray>>,
}

impl Shell {
    /// Starts listening for later launches. With `tray`, adds the
    /// notification-area icon if the desktop has a host for it.
    pub fn new(tray: bool, tip: &str, menu: (&str, &str), sink: Box<dyn Fn(ShellEvent) + Send + Sync>) -> Option<Shell> {
        use ksni::blocking::TrayMethods;
        let sink: Sink = Arc::from(sink);
        let path = socket_path();
        let socket = bind(&path).map(|listener| {
            listen(listener, sink.clone());
            path
        });
        let tray = tray.then(|| Tray { sink, tip: tip.to_string(), open: menu.0.to_string(), exit: menu.1.to_string() });
        Some(Shell { socket, tray: tray.and_then(|t| t.spawn().ok()) })
    }

    /// Whether the notification-area icon is showing.
    pub fn has_icon(&self) -> bool {
        self.tray.is_some()
    }

    /// Updates the tray menu labels (after a language change).
    pub fn set_labels(&self, tip: &str, open: &str, exit: &str) {
        if let Some(h) = &self.tray {
            h.update(|t| {
                t.tip = tip.to_string();
                t.open = open.to_string();
                t.exit = exit.to_string();
            });
        }
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        if let Some(path) = &self.socket {
            let _ = std::fs::remove_file(path);
        }
    }
}
