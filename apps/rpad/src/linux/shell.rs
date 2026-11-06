//! Single instance over a Unix socket, and the notification-area icon through
//! StatusNotifierItem (D-Bus).
//!
//! The running instance greets each client with [`RESIDENT`] or [`NORMAL`]. A
//! client that hands off then writes [`OPEN`] and NUL-terminated absolute paths;
//! one that only asks closes without writing anything. Desktops without a
//! StatusNotifierItem host (plain GNOME, for example) have no icon.

use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const RESIDENT: u8 = b'R';
const NORMAL: u8 = b'N';
const OPEN: u8 = b'A';

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    /// Show the window and open these files, if any.
    Open(Vec<PathBuf>),
    /// Quit (from the tray menu).
    Quit,
}

type Sink = Arc<dyn Fn(Msg) + Send + Sync>;

fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("rpad.sock"),
        // SAFETY: getuid has no preconditions and cannot fail.
        None => std::env::temp_dir().join(format!("rpad-{}.sock", unsafe { libc::getuid() })),
    }
}

/// Connects to the running instance and reads its greeting.
fn connect() -> Option<(UnixStream, u8)> {
    let mut s = UnixStream::connect(socket_path()).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut greeting = [0u8; 1];
    s.read_exact(&mut greeting).ok()?;
    Some((s, greeting[0]))
}

pub fn resident_running() -> bool {
    connect().is_some_and(|(_, g)| g == RESIDENT)
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

/// Hands `paths` to the running instance, if it is resident or `any` is set.
pub fn hand_off(paths: &[PathBuf], any: bool) -> bool {
    match connect() {
        Some((mut s, greeting)) if any || greeting == RESIDENT => s.write_all(&message(paths)).is_ok(),
        _ => false,
    }
}

/// Binds the instance socket. Fails if a live instance owns it (only with
/// `--new-window`); a socket left behind by a crash is replaced.
fn bind(path: &Path) -> Option<UnixListener> {
    if UnixStream::connect(path).is_ok() {
        return None;
    }
    let _ = std::fs::remove_file(path);
    UnixListener::bind(path).ok()
}

fn serve(mut stream: UnixStream, greeting: u8, sink: &Sink) {
    if stream.write_all(&[greeting]).is_err() || stream.set_read_timeout(Some(Duration::from_secs(2))).is_err() {
        return;
    }
    let mut data = Vec::new();
    if stream.read_to_end(&mut data).is_ok() {
        if let Some(paths) = parse(&data) {
            sink(Msg::Open(paths));
        }
    }
}

fn listen(listener: UnixListener, greeting: u8, sink: Sink) {
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            serve(stream, greeting, &sink);
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
        "rpad".into()
    }

    fn title(&self) -> String {
        "rpad".into()
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
    tray: Option<ksni::blocking::Handle<Tray>>,
}

impl Shell {
    /// Starts listening for later launches. With `tray`, adds the
    /// notification-area icon if the desktop has a host for it.
    pub fn new(tray: bool, labels: (&str, &str), sink: Box<dyn Fn(Msg) + Send + Sync>) -> Shell {
        use ksni::blocking::TrayMethods;
        let sink: Sink = Arc::from(sink);
        let icon = tray.then(|| Tray { sink: sink.clone(), open: labels.0.to_string(), exit: labels.1.to_string() }.spawn().ok()).flatten();
        let path = socket_path();
        let socket = bind(&path).map(|listener| {
            listen(listener, if icon.is_some() { RESIDENT } else { NORMAL }, sink);
            path
        });
        Shell { socket, tray: icon }
    }

    /// Whether the notification-area icon is showing.
    pub fn has_icon(&self) -> bool {
        self.tray.is_some()
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
        let paths = vec![PathBuf::from("/a b/ç.txt"), PathBuf::from(OsStr::from_bytes(b"/raw/\xFF.txt"))];
        assert_eq!(parse(&message(&paths)), Some(paths));
        assert_eq!(parse(&message(&[])), Some(Vec::new()));
        assert_eq!(parse(b""), None);
    }

    #[test]
    fn relative_paths_become_absolute() {
        let got = parse(&message(&[PathBuf::from("x.txt")])).unwrap();
        assert!(got[0].is_absolute());
    }
}
