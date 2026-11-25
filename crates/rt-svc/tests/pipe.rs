//! The service and a client on a real named pipe, in one process.
#![cfg(windows)]

use rt_svc::access::{current_process_token, read_token};
use rt_svc::client::{connect_to, Trust};
use rt_svc::frame::{read_frame, write_frame, Frame};
use rt_svc::pipe::{connect, Event};
use rt_svc::proto::{code, decode_error, Hello};
use rt_svc::server::{run, Handler};
use rt_svc::{kind, Level};
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

struct Echo;

impl Handler for Echo {
    fn version(&self) -> String {
        "test".into()
    }

    fn scans(&self) -> u64 {
        42
    }

    fn snapshots(&self) -> Vec<(String, u64)> {
        vec![("programs".into(), 3)]
    }
}

/// A server on a pipe of its own, stopped on drop.
struct Running {
    name: String,
    stop: Arc<Event>,
    thread: Option<JoinHandle<()>>,
}

fn start() -> Running {
    static N: AtomicU32 = AtomicU32::new(0);
    let name = format!(r"\\.\pipe\rt-svc-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
    let stop = Arc::new(Event::new().unwrap());
    let (n, s) = (name.clone(), stop.clone());
    let thread = std::thread::spawn(move || run(&n, Arc::new(Echo), s, Arc::new(|m| eprintln!("[svc] {m}"))).unwrap());
    // The pipe exists as soon as `run` has created it.
    for _ in 0..100 {
        if connect(&name, 0).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Running { name, stop, thread: Some(thread) }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.set();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn me() -> Level {
    read_token(current_process_token().unwrap().0).unwrap().level
}

#[test]
fn a_client_says_hello_and_is_told_its_level() {
    let s = start();
    let mut c = connect_to(&s.name, Trust::Anyone, "test", "1.0", 1000).unwrap();
    assert_eq!(c.level, me());
    assert_eq!(c.protocol, rt_svc::PROTOCOL);
    assert_eq!(c.service_version, "test");
    c.ping().unwrap();
    // The connection that `start` used to see the pipe is closed by now, or soon.
    let mut st = c.status().unwrap();
    for _ in 0..50 {
        if st.clients == 1 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        st = c.status().unwrap();
    }
    assert_eq!((st.scans, st.clients), (42, 1));
    assert_eq!(st.snapshots, vec![("programs".to_string(), 3)]);
}

#[test]
fn several_clients_at_once() {
    let s = start();
    let mut a = connect_to(&s.name, Trust::Anyone, "a", "1", 1000).unwrap();
    let mut b = connect_to(&s.name, Trust::Anyone, "b", "1", 1000).unwrap();
    assert!(a.ping().is_ok() && b.ping().is_ok());
    let mut clients = b.status().unwrap().clients;
    for _ in 0..50 {
        if clients == 2 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        clients = b.status().unwrap().clients;
    }
    assert_eq!(clients, 2);
    drop(a);
    assert!(b.ping().is_ok());
}

#[test]
fn unknown_requests_are_answered_with_an_error() {
    let s = start();
    let mut c = connect_to(&s.name, Trust::Anyone, "test", "1", 1000).unwrap();
    let e = c.call(77, vec![], 0).unwrap_err();
    assert!(e.to_string().contains("unknown request"), "{e}");
    // The connection is still good.
    c.ping().unwrap();
}

#[test]
fn the_first_message_must_be_hello() {
    let s = start();
    let mut raw = connect(&s.name, 1000).unwrap();
    write_frame(&mut raw, &Frame::new(kind::PING, 5, vec![])).unwrap();
    let f = read_frame(&mut raw).unwrap();
    assert_eq!(f.kind, kind::ERROR);
    assert_eq!(decode_error(&f.payload).unwrap().0, code::NO_HELLO);
    // And the service has closed the connection.
    assert!(read_frame(&mut raw).is_err());
}

#[test]
fn a_hello_that_does_not_decode_is_refused() {
    let s = start();
    let mut raw = connect(&s.name, 1000).unwrap();
    write_frame(&mut raw, &Frame::new(kind::HELLO, 1, vec![1, 2])).unwrap();
    let f = read_frame(&mut raw).unwrap();
    assert_eq!(decode_error(&f.payload).unwrap().0, code::MALFORMED);
}

#[test]
fn garbage_closes_the_connection_and_not_the_service() {
    let s = start();
    let mut raw = connect(&s.name, 1000).unwrap();
    let hello = Hello { protocol: 1, app: "x".into(), app_version: "1".into() };
    write_frame(&mut raw, &Frame::new(kind::HELLO, 1, hello.encode())).unwrap();
    assert_eq!(read_frame(&mut raw).unwrap().kind, kind::HELLO_OK);
    // A length that no frame may have.
    raw.write_all(&[0xff, 0xff, 0xff, 0xff, 1, 2, 3]).unwrap();
    assert!(read_frame(&mut raw).is_err());
    // Another client is served as before.
    let mut c = connect_to(&s.name, Trust::Anyone, "test", "1", 1000).unwrap();
    c.ping().unwrap();
}

#[test]
fn a_client_that_checks_does_not_trust_just_any_server() {
    let s = start();
    // This server is not the registered service: whoever started it, it is refused.
    let e = connect_to(&s.name, Trust::Installed, "test", "1", 1000).err().expect("must be refused");
    assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied, "{e}");
}

#[test]
fn clients_that_come_and_go_never_take_the_pipe_away() {
    let s = start();
    // A client that leaves before the service looks at it must not leave a moment without the pipe: others would be
    // told there is no service, and anyone could create a pipe of the same name in that moment.
    for i in 0..200 {
        for _ in 0..3 {
            drop(connect(&s.name, 1000).unwrap_or_else(|e| panic!("round {i}: {e}")));
        }
        let mut c = connect_to(&s.name, Trust::Anyone, "test", "1", 1000).unwrap_or_else(|e| panic!("round {i}: {e}"));
        c.ping().unwrap();
    }
}

#[test]
fn no_service_is_a_quick_error() {
    let start = std::time::Instant::now();
    let e = connect_to(r"\\.\pipe\rt-svc-nobody-here", Trust::Anyone, "t", "1", 200).err().unwrap();
    assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn stopping_ends_the_service_even_with_a_client_connected() {
    let s = start();
    let mut c = connect_to(&s.name, Trust::Anyone, "test", "1", 1000).unwrap();
    c.ping().unwrap();
    drop(s); // sets stop and joins `run`
    let r = c.ping();
    assert!(r.is_err(), "{r:?}");
}
