//! Resident mode on Linux: a Unix socket for later launches, and the
//! notification-area icon through StatusNotifierItem (D-Bus).
//!
//! A client writes [`OPEN`] and NUL-terminated absolute paths. Only a resident
//! rview listens, as on Windows. Desktops without a StatusNotifierItem host
//! (plain GNOME, for example) have no icon, and so no resident mode.

use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const OPEN: u8 = b'A';

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    /// Show the window and open this file, if any.
    Open(Vec<PathBuf>),
    /// Quit (from the tray menu).
    Quit,
}

type Sink = Arc<dyn Fn(Msg) + Send + Sync>;

fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("rview.sock"),
        // SAFETY: getuid has no preconditions and cannot fail.
        None => std::env::temp_dir().join(format!("rview-{}.sock", unsafe { libc::getuid() })),
    }
}

fn message(paths: &[PathBuf]) -> Vec<u8> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut data = vec![OPEN];
    for p in paths {
        data.extend_from_slice(cwd.join(p).as_os_str().as_bytes());
        data.push(0);
    }
    data
}

fn parse(data: &[u8]) -> Option<Vec<PathBuf>> {
    let rest = data.strip_prefix(&[OPEN])?;
    Some(rest.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| PathBuf::from(std::ffi::OsString::from_vec(p.to_vec()))).collect())
}

/// Hands `paths` to the resident instance, if there is one.
pub fn hand_off_to_resident(paths: &[PathBuf]) -> bool {
    UnixStream::connect(socket_path()).is_ok_and(|mut s| s.write_all(&message(paths)).is_ok())
}

/// Binds the socket. A socket left behind by a crash is replaced.
fn bind(path: &Path) -> Option<UnixListener> {
    if UnixStream::connect(path).is_ok() {
        return None;
    }
    let _ = std::fs::remove_file(path);
    UnixListener::bind(path).ok()
}

fn listen(listener: UnixListener, sink: Sink) {
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut data = Vec::new();
            if stream.read_to_end(&mut data).is_ok() {
                if let Some(paths) = parse(&data) {
                    sink(Msg::Open(paths));
                }
            }
        }
    });
}

struct Tray {
    sink: Sink,
    open: String,
    exit: String,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "rview".into()
    }

    fn title(&self) -> String {
        "rview".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let size = 32;
        // RGBA to the ARGB byte order of the protocol.
        let mut data = crate::ui::appicon::render(size as u32);
        data.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p.rotate_right(1));
        vec![ksni::Icon { width: size, height: size, data }]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        (self.sink)(Msg::Open(Vec::new()));
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: self.open.clone(),
                activate: Box::new(|t: &mut Self| (t.sink)(Msg::Open(Vec::new()))),
                ..StandardItem::default()
            }
            .into(),
            StandardItem {
                label: self.exit.clone(),
                activate: Box::new(|t: &mut Self| (t.sink)(Msg::Quit)),
                ..StandardItem::default()
            }
            .into(),
        ]
    }
}

pub struct Shell {
    socket: Option<PathBuf>,
    _tray: ksni::blocking::Handle<Tray>,
}

impl Shell {
    /// Starts resident mode, or returns None when the desktop has no host
    /// for the icon.
    pub fn new(labels: (&str, &str), sink: Box<dyn Fn(Msg) + Send + Sync>) -> Option<Shell> {
        use ksni::blocking::TrayMethods;
        let sink: Sink = Arc::from(sink);
        let tray = Tray { sink: sink.clone(), open: labels.0.to_string(), exit: labels.1.to_string() }.spawn().ok()?;
        let path = socket_path();
        let socket = bind(&path).map(|listener| {
            listen(listener, sink);
            path
        });
        Some(Shell { socket, _tray: tray })
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        if let Some(path) = &self.socket {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn messages_round_trip() {
        let paths = vec![PathBuf::from("/a b/ç.pdf"), PathBuf::from(OsStr::from_bytes(b"/raw/\xFF.md"))];
        assert_eq!(parse(&message(&paths)), Some(paths));
        assert_eq!(parse(&message(&[])), Some(Vec::new()));
        assert_eq!(parse(b""), None);
    }
}
