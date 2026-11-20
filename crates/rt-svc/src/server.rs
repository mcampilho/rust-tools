//! The service's side of the protocol: accept clients, decide their level,
//! answer the requests that every service answers (`Hello`, `Ping`,
//! `Status`) and give the rest to a [`Handler`].

use crate::access::{client_of, Who};
use crate::frame::{kind, read_frame, write_frame, Frame, PROTOCOL, PROTOCOL_MIN};
use crate::pipe::{Conn, Event, Listener, SDDL};
use crate::proto::{code, encode_error, Hello, HelloOk, Level, Status};
use std::io::{self, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The most clients served at once.
pub const MAX_CLIENTS: u32 = 32;
/// A client that says nothing for this long is dropped.
const SILENCE_MS: u32 = 60_000;

/// What a request handler sees of the connection.
pub struct Session<'a> {
    pub who: &'a Who,
    pub level: Level,
    pub protocol: u8,
}

/// What a service does beyond the common requests.
pub trait Handler: Send + Sync + 'static {
    fn version(&self) -> String;
    /// The `caps` bits of what it can do.
    fn caps(&self) -> u32 {
        0
    }
    /// Scans served, for `Status`.
    fn scans(&self) -> u64 {
        0
    }
    /// The snapshots it keeps and their age in seconds, for `Status`.
    fn snapshots(&self) -> Vec<(String, u64)> {
        Vec::new()
    }
    /// A request that is not `Hello`, `Ping` or `Status`: write its answer
    /// frames to `out`. An error closes the connection.
    fn request(&self, _session: &Session, f: &Frame, out: &mut dyn Write) -> io::Result<()> {
        write_frame(out, &Frame::new(kind::ERROR, f.id, encode_error(code::UNKNOWN, "unknown request")))
    }
}

/// Where the log lines go.
pub type Log = Arc<dyn Fn(&str) + Send + Sync>;

struct Shared {
    handler: Arc<dyn Handler>,
    log: Log,
    clients: AtomicU32,
    started: Instant,
}

fn error(out: &mut dyn Write, id: u32, c: u16, msg: &str) -> io::Result<()> {
    write_frame(out, &Frame::new(kind::ERROR, id, encode_error(c, msg)))
}

/// One connection, from `Hello` to its end.
fn serve_client(mut conn: Conn, shared: Arc<Shared>) -> io::Result<()> {
    conn.set_timeout(SILENCE_MS);
    let who = client_of(&conn)?;
    let first = read_frame(&mut conn)?;
    if first.kind != kind::HELLO {
        return error(&mut conn, first.id, code::NO_HELLO, "the first message must be Hello");
    }
    let Ok(hello) = Hello::decode(&first.payload) else {
        return error(&mut conn, first.id, code::MALFORMED, "Hello is malformed");
    };
    let protocol = hello.protocol.min(PROTOCOL);
    if protocol < PROTOCOL_MIN || who.level == Level::Denied {
        let (c, m) = if who.level == Level::Denied { (code::DENIED, "not allowed") } else { (code::FAILED, "the protocol is too old") };
        (shared.log)(&format!("refused {} ({}): {m}", who.sid, hello.app));
        return error(&mut conn, first.id, c, m);
    }
    (shared.log)(&format!("{} {:?} from {} {}", who.sid, who.level, hello.app, hello.app_version));
    let ok = HelloOk { protocol, service_version: shared.handler.version(), level: who.level, caps: shared.handler.caps() };
    write_frame(&mut conn, &Frame::new(kind::HELLO_OK, first.id, ok.encode()))?;

    let session = Session { who: &who, level: who.level, protocol };
    loop {
        let f = match read_frame(&mut conn) {
            Ok(f) => f,
            // The client closed the pipe, or was silent too long, or the service stops.
            Err(e) if matches!(e.kind(), io::ErrorKind::UnexpectedEof | io::ErrorKind::TimedOut | io::ErrorKind::ConnectionAborted) => {
                return Ok(())
            }
            Err(e) => return Err(e),
        };
        match f.kind {
            kind::PING => write_frame(&mut conn, &Frame::new(kind::PONG, f.id, f.payload.iter().copied().take(64).collect()))?,
            kind::STATUS => {
                let s = Status {
                    uptime_secs: shared.started.elapsed().as_secs(),
                    clients: shared.clients.load(Ordering::Relaxed),
                    scans: shared.handler.scans(),
                    snapshots: shared.handler.snapshots(),
                };
                write_frame(&mut conn, &Frame::new(kind::STATUS_OK, f.id, s.encode()))?;
            }
            kind::HELLO => error(&mut conn, f.id, code::FAILED, "Hello was already said")?,
            k if k >= 100 => error(&mut conn, f.id, code::UNKNOWN, "an answer is not a request")?,
            _ => shared.handler.request(&session, &f, &mut conn)?,
        }
    }
}

/// Runs the service on the pipe `name` until `stop` is set. Returns when the
/// last client is gone (or after a few seconds).
pub fn run(name: &str, handler: Arc<dyn Handler>, stop: Arc<Event>, log: Log) -> io::Result<()> {
    let mut listener = Listener::new(name, SDDL, stop.0)?;
    let shared = Arc::new(Shared { handler, log, clients: AtomicU32::new(0), started: Instant::now() });
    (shared.log)(&format!("listening on {name}"));
    loop {
        let conn = match listener.accept(u32::MAX) {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::ConnectionAborted => break,
            Err(e) => {
                (shared.log)(&format!("accept failed: {e}"));
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
        };
        if shared.clients.fetch_add(1, Ordering::SeqCst) >= MAX_CLIENTS {
            shared.clients.fetch_sub(1, Ordering::SeqCst);
            let mut conn = conn;
            let _ = error(&mut conn, 0, code::BUSY, "too many clients");
            continue;
        }
        let (s, stop) = (shared.clone(), stop.clone());
        std::thread::spawn(move || {
            let _keep = stop;
            if let Err(e) = serve_client(conn, s.clone()) {
                (s.log)(&format!("client ended: {e}"));
            }
            s.clients.fetch_sub(1, Ordering::SeqCst);
        });
    }
    let end = Instant::now() + Duration::from_secs(5);
    while shared.clients.load(Ordering::SeqCst) > 0 && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    (shared.log)("stopped");
    Ok(())
}
