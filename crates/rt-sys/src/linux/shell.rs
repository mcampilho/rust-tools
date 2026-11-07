//! Single instance over a Unix socket, and the notification-area icon through
//! StatusNotifierItem (D-Bus).
//!
//! The running instance greets each client with [`RESIDENT`] or [`NORMAL`]. A
//! client that hands off then writes [`OPEN`] and NUL-terminated absolute
//! paths (none: just show the window); one that only asks closes without
//! writing anything. Desktops without a StatusNotifierItem host (plain GNOME,
//! for example) have no icon: [`Shell::has_icon`] tells the app.

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

/// The tray icon's texts.
#[derive(Clone, Debug, Default)]
pub struct TrayLabels {
    /// Name shown by the icon host.
    pub tip: String,
    /// Menu item that shows the window.
    pub open: String,
    /// Menu item that quits.
    pub exit: String,
}

fn socket_path(app: &str) -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(dir) => PathBuf::from(dir).join(format!("{app}.sock")),
        // SAFETY: getuid has no preconditions and cannot fail.
        None => std::env::temp_dir().join(format!("{app}-{}.sock", unsafe { libc::getuid() })),
    }
}

/// Connects to the running instance and reads its greeting.
fn connect(app: &str) -> Option<(UnixStream, u8)> {
    let mut s = UnixStream::connect(socket_path(app)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut greeting = [0u8; 1];
    s.read_exact(&mut greeting).ok()?;
    Some((s, greeting[0]))
}

/// Whether a resident (tray) instance of `app` is running.
pub fn resident_running(app: &str) -> bool {
    connect(app).is_some_and(|(_, g)| g == RESIDENT)
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
    Some(
        rest.split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| PathBuf::from(std::ffi::OsString::from_vec(p.to_vec())))
            .collect(),
    )
}

/// Hands `paths` to the running instance of `app`, if it is resident or
/// `any` is set. True if handed over (this process should exit).
pub fn hand_off(app: &str, paths: &[PathBuf], any: bool) -> bool {
    match connect(app) {
        Some((mut s, greeting)) if any || greeting == RESIDENT => s.write_all(&message(paths)).is_ok(),
        _ => false,
    }
}

/// Binds the instance socket. Fails if a live instance owns it; a socket
/// left behind by a crash is replaced.
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
    app: &'static str,
    icon: fn(u32) -> Vec<u8>,
    sink: Sink,
    labels: TrayLabels,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        self.app.into()
    }

    fn title(&self) -> String {
        self.labels.tip.clone()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        let size = 32;
        // RGBA to the ARGB byte order of the protocol.
        let mut data = (self.icon)(size as u32);
        data.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p.rotate_right(1));
        vec![ksni::Icon {
            width: size,
            height: size,
            data,
        }]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        (self.sink)(Msg::Open(Vec::new()));
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![
            StandardItem {
                label: self.labels.open.clone(),
                activate: Box::new(|t: &mut Self| (t.sink)(Msg::Open(Vec::new()))),
                ..StandardItem::default()
            }
            .into(),
            StandardItem {
                label: self.labels.exit.clone(),
                activate: Box::new(|t: &mut Self| (t.sink)(Msg::Quit)),
                ..StandardItem::default()
            }
            .into(),
        ]
    }
}

/// What [`Shell::new`] sets up.
pub struct Config {
    /// App name: the socket and the icon's id.
    pub app: &'static str,
    /// Renders the icon at `n`×`n` pixels as straight RGBA.
    pub icon: fn(u32) -> Vec<u8>,
    /// Shows the notification-area icon, if the desktop has a host for it.
    pub tray: Option<TrayLabels>,
    /// Listens for later launches even without an icon. When false, only a
    /// resident instance (one with an icon) listens.
    pub listen_without_icon: bool,
}

pub struct Shell {
    socket: Option<PathBuf>,
    tray: Option<ksni::blocking::Handle<Tray>>,
}

impl Shell {
    /// Starts listening for later launches and, if asked, adds the icon.
    pub fn new(cfg: Config, sink: Box<dyn Fn(Msg) + Send + Sync>) -> Shell {
        use ksni::blocking::TrayMethods;
        let sink: Sink = Arc::from(sink);
        let tray = cfg.tray.and_then(|labels| {
            Tray {
                app: cfg.app,
                icon: cfg.icon,
                sink: sink.clone(),
                labels,
            }
            .spawn()
            .ok()
        });
        let mut socket = None;
        if tray.is_some() || cfg.listen_without_icon {
            let path = socket_path(cfg.app);
            if let Some(listener) = bind(&path) {
                listen(listener, if tray.is_some() { RESIDENT } else { NORMAL }, sink);
                socket = Some(path);
            }
        }
        Shell { socket, tray }
    }

    /// Whether the notification-area icon is showing.
    pub fn has_icon(&self) -> bool {
        self.tray.is_some()
    }

    /// Updates the icon's texts (after a language change).
    pub fn set_labels(&self, labels: TrayLabels) {
        if let Some(h) = &self.tray {
            h.update(|t| t.labels = labels);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::sync::mpsc;

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

    /// A later launch hands its files to the running instance.
    #[test]
    fn hand_off_reaches_the_running_instance() {
        let app: &'static str = Box::leak(format!("rt-sys-test-{}", std::process::id()).into_boxed_str());
        let (tx, rx) = mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let cfg = Config {
            app,
            icon: |n| vec![0; (n * n * 4) as usize],
            tray: None,
            listen_without_icon: true,
        };
        let shell = Shell::new(cfg, Box::new(move |m| tx.lock().unwrap().send(m).unwrap()));
        assert!(!shell.has_icon());
        assert!(!resident_running(app), "no icon, so not resident");
        assert!(!hand_off(app, &[PathBuf::from("/a.txt")], false), "only to a resident instance");
        assert!(hand_off(app, &[PathBuf::from("/a.txt")], true));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Msg::Open(vec![PathBuf::from("/a.txt")])
        );
        drop(shell);
        assert!(!hand_off(app, &[], true), "the socket is gone");
    }
}
