//! The client: what a tool does to ask the service. Without a service (it is
//! not installed, not running, or a system that has none) `connect` fails and
//! the tool does what it did before.

#![cfg_attr(not(windows), allow(unused_imports, dead_code))]

use crate::frame::{kind, read_frame, write_frame, Frame};
use crate::proto::{
    decode_batch, decode_done, decode_error, encode_job_id, Hello, HelloOk, JobInfo, JobKind, JobRequest, Level, ScanProgress, ScanRequest,
    ScanStart, SnapshotData, SnapshotRequest, Status, WireEntry,
};
use std::io;
use std::time::{Duration, Instant};

/// How long a tool waits to find the service before giving up.
pub const CONNECT_WAIT_MS: u32 = 200;

/// Which server a client accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    /// The installed service: the pipe's owner is SYSTEM or Administrators and
    /// the process on it is the image the Service Control Manager registered.
    Installed,
    /// Anything: for `rtsvc console` and the tests.
    Anyone,
}

/// A connection to the service, after `Hello`.
pub struct Client {
    #[cfg(windows)]
    conn: crate::pipe::Conn,
    next: u32,
    /// What the service made of this client.
    pub level: Level,
    /// What the service can do (`proto::caps`).
    pub caps: u32,
    pub protocol: u8,
    pub service_version: String,
}

/// The installed service, checked. Fails (quickly) when there is none.
pub fn connect(app: &str, app_version: &str) -> io::Result<Client> {
    connect_to(crate::PIPE_NAME, Trust::Installed, app, app_version, CONNECT_WAIT_MS)
}

#[cfg(not(windows))]
pub fn connect_to(_name: &str, _trust: Trust, _app: &str, _app_version: &str, _wait_ms: u32) -> io::Result<Client> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "there is no service on this system"))
}

#[cfg(windows)]
pub fn connect_to(name: &str, trust: Trust, app: &str, app_version: &str, wait_ms: u32) -> io::Result<Client> {
    let mut conn = crate::pipe::connect(name, wait_ms)?;
    if trust == Trust::Installed {
        verify::server(&conn)?;
    }
    let hello = Hello { protocol: crate::PROTOCOL, app: app.to_string(), app_version: app_version.to_string() };
    write_frame(&mut conn, &Frame::new(kind::HELLO, 1, hello.encode()))?;
    let answer = read_frame(&mut conn)?;
    match answer.kind {
        kind::HELLO_OK => {
            let ok = HelloOk::decode(&answer.payload).map_err(io::Error::other)?;
            Ok(Client { conn, next: 2, level: ok.level, caps: ok.caps, protocol: ok.protocol, service_version: ok.service_version })
        }
        kind::ERROR => Err(answer_error(&answer)),
        _ => Err(io::Error::new(io::ErrorKind::InvalidData, "the service answered something else than Hello")),
    }
}

fn answer_error(f: &Frame) -> io::Error {
    match decode_error(&f.payload) {
        Ok((c, m)) => io::Error::other(format!("the service refused ({c}): {m}")),
        Err(e) => io::Error::new(io::ErrorKind::InvalidData, e),
    }
}

#[cfg(windows)]
impl Client {
    /// Sends a request and returns its number.
    pub fn send(&mut self, kind: u16, payload: Vec<u8>) -> io::Result<u32> {
        let id = self.next;
        self.next = self.next.wrapping_add(1).max(2);
        write_frame(&mut self.conn, &Frame::new(kind, id, payload))?;
        Ok(id)
    }

    /// The next frame of the service. An error answer is an `Err`.
    pub fn recv(&mut self) -> io::Result<Frame> {
        let f = read_frame(&mut self.conn)?;
        if f.kind == kind::ERROR {
            return Err(answer_error(&f));
        }
        Ok(f)
    }

    /// How long the service takes to answer, to wait for each answer at most `ms`.
    pub fn set_timeout(&mut self, ms: u32) {
        self.conn.set_timeout(ms);
    }

    /// A request with one answer of the kind `expect`.
    pub fn call(&mut self, kind: u16, payload: Vec<u8>, expect: u16) -> io::Result<Frame> {
        let id = self.send(kind, payload)?;
        let f = self.recv()?;
        if f.id != id || f.kind != expect {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "the answer is not the one asked for"));
        }
        Ok(f)
    }

    pub fn ping(&mut self) -> io::Result<Duration> {
        let start = Instant::now();
        self.call(kind::PING, b"rt".to_vec(), kind::PONG)?;
        Ok(start.elapsed())
    }

    pub fn status(&mut self) -> io::Result<Status> {
        let f = self.call(kind::STATUS, Vec::new(), kind::STATUS_OK)?;
        Status::decode(&f.payload).map_err(io::Error::other)
    }

    /// Starts a job. Only an elevated client may. Returns its number.
    pub fn submit_job(&mut self, kind: JobKind, items: Vec<String>) -> io::Result<u64> {
        let f = self.call(kind::SUBMIT_JOB, JobRequest { kind, items }.encode(), kind::JOB_INFO)?;
        Ok(JobInfo::decode(&f.payload).map_err(io::Error::other)?.id)
    }

    /// How a job is going.
    pub fn job(&mut self, id: u64) -> io::Result<JobInfo> {
        let f = self.call(kind::JOB, encode_job_id(id), kind::JOB_INFO)?;
        JobInfo::decode(&f.payload).map_err(io::Error::other)
    }

    /// Stops a job (it is told, and stops at the next item).
    pub fn cancel_job(&mut self, id: u64) -> io::Result<JobInfo> {
        let f = self.call(kind::CANCEL_JOB, encode_job_id(id), kind::JOB_INFO)?;
        JobInfo::decode(&f.payload).map_err(io::Error::other)
    }

    /// The snapshot ``name``, if the service has one that is at most `max_age_secs` old.
    pub fn snapshot(&mut self, name: &str, max_age_secs: u64) -> io::Result<SnapshotData> {
        let q = SnapshotRequest { name: name.to_string(), max_age_secs };
        let f = self.call(kind::GET_SNAPSHOT, q.encode(), kind::SNAPSHOT)?;
        SnapshotData::decode(&f.payload).map_err(io::Error::other)
    }

    /// Asks the service to make the snapshot `name` again, without waiting for it.
    pub fn refresh(&mut self, name: &str) -> io::Result<()> {
        let q = SnapshotRequest { name: name.to_string(), max_age_secs: 0 };
        self.call(kind::REFRESH, q.encode(), kind::REFRESH_OK).map(|_| ())
    }

    /// Asks the service to read the NTFS table of the volume `root` is on and
    /// sends what it finds to `sink`, folder by folder. `cancel` stops it: the
    /// connection is then useless and must be dropped. Returns the number of nodes.
    pub fn scan_volume(&mut self, root: &str, cancel: &std::sync::atomic::AtomicBool, sink: &mut dyn ScanSink) -> io::Result<u32> {
        use std::sync::atomic::Ordering;
        let stopped = || io::Error::new(io::ErrorKind::Interrupted, "the scan was stopped");
        let id = self.send(kind::SCAN_VOLUME, ScanRequest { root: root.to_string() }.encode())?;
        let mut started = false;
        let mut nodes = 1u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(stopped());
            }
            let f = self.recv()?;
            if f.id != id {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "the answer is not the one asked for"));
            }
            let bad = |e: crate::codec::DecodeError| io::Error::new(io::ErrorKind::InvalidData, e);
            match f.kind {
                kind::SCAN_START if !started => {
                    let s = ScanStart::decode(&f.payload).map_err(bad)?;
                    sink.start(&s.root, s.mtime);
                    started = true;
                }
                kind::SCAN_PROGRESS => sink.progress(&ScanProgress::decode(&f.payload).map_err(bad)?),
                kind::SCAN_BATCH if started => {
                    for c in decode_batch(&f.payload).map_err(bad)? {
                        nodes += c.entries.len() as u64;
                        sink.chunk(c.dir, c.last, c.entries);
                    }
                }
                kind::SCAN_DONE if started => {
                    let n = decode_done(&f.payload).map_err(bad)?;
                    if u64::from(n) != nodes {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "the service sent a different number of entries than it said",
                        ));
                    }
                    return Ok(n);
                }
                _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "an unexpected answer to a scan")),
            }
        }
    }
}

/// Where the answers of a scan go.
pub trait ScanSink {
    /// The root and its time: first.
    fn start(&mut self, root: &str, mtime: i64);
    fn progress(&mut self, p: &ScanProgress);
    /// A run of the entries of folder number `dir` (the number is the order in which
    /// the entries were sent: the root is 0, its entries follow, and so on);
    /// `last` when the folder is complete.
    fn chunk(&mut self, dir: u32, last: bool, entries: Vec<WireEntry>);
}

#[cfg(not(windows))]
impl Client {
    pub fn send(&mut self, _kind: u16, _payload: Vec<u8>) -> io::Result<u32> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn recv(&mut self) -> io::Result<Frame> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn set_timeout(&mut self, _ms: u32) {}

    pub fn call(&mut self, _kind: u16, _payload: Vec<u8>, _expect: u16) -> io::Result<Frame> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn ping(&mut self) -> io::Result<Duration> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn status(&mut self) -> io::Result<Status> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn scan_volume(&mut self, _root: &str, _cancel: &std::sync::atomic::AtomicBool, _sink: &mut dyn ScanSink) -> io::Result<u32> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn submit_job(&mut self, _kind: JobKind, _items: Vec<String>) -> io::Result<u64> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn job(&mut self, _id: u64) -> io::Result<JobInfo> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn cancel_job(&mut self, _id: u64) -> io::Result<JobInfo> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn snapshot(&mut self, _name: &str, _max_age_secs: u64) -> io::Result<SnapshotData> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub fn refresh(&mut self, _name: &str) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

/// Checks that the server of a pipe is the installed service.
#[cfg(windows)]
mod verify {
    use super::*;
    use crate::access::{owner_of, SID_ADMINISTRATORS, SID_SYSTEM};
    use crate::pipe::Conn;
    use std::path::PathBuf;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId;
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceConfigW, QUERY_SERVICE_CONFIGW, SC_MANAGER_CONNECT,
        SERVICE_QUERY_CONFIG,
    };
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};

    fn denied(why: &str) -> io::Error {
        io::Error::new(io::ErrorKind::PermissionDenied, format!("the server of the pipe is not the service: {why}"))
    }

    /// The image the Service Control Manager has for the service.
    fn registered_image() -> io::Result<PathBuf> {
        let name: Vec<u16> = crate::SERVICE_NAME.encode_utf16().chain(Some(0)).collect();
        // SAFETY: plain calls with valid arguments; every handle opened is closed below.
        unsafe {
            let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
            if scm.is_null() {
                return Err(io::Error::from_raw_os_error(GetLastError() as i32));
            }
            let svc = OpenServiceW(scm, name.as_ptr(), SERVICE_QUERY_CONFIG);
            if svc.is_null() {
                let e = io::Error::from_raw_os_error(GetLastError() as i32);
                CloseServiceHandle(scm);
                return Err(e);
            }
            let mut need = 0u32;
            QueryServiceConfigW(svc, null_mut(), 0, &mut need);
            let mut buf = vec![0u64; (need as usize).div_ceil(8).max(1)];
            let ok = QueryServiceConfigW(svc, buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW, (buf.len() * 8) as u32, &mut need);
            let result = if ok == 0 {
                Err(io::Error::from_raw_os_error(GetLastError() as i32))
            } else {
                let p = (*(buf.as_ptr() as *const QUERY_SERVICE_CONFIGW)).lpBinaryPathName;
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                Ok(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)))
            };
            CloseServiceHandle(svc);
            CloseServiceHandle(scm);
            let line = result?;
            // `"C:\...\rtsvc.exe" run`: the quoted part is the image.
            let image = match line.strip_prefix('"') {
                Some(rest) => rest.split('"').next().unwrap_or(""),
                None => line.split(' ').next().unwrap_or(""),
            };
            Ok(PathBuf::from(image))
        }
    }

    /// The image of the process with id `pid`.
    fn image_of(pid: u32) -> io::Result<PathBuf> {
        // SAFETY: plain calls; the handle is closed.
        unsafe {
            let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if p.is_null() {
                return Err(io::Error::from_raw_os_error(GetLastError() as i32));
            }
            let mut buf = vec![0u16; 1024];
            let mut n = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(p, 0, buf.as_mut_ptr(), &mut n);
            let e = io::Error::from_raw_os_error(GetLastError() as i32);
            CloseHandle(p);
            if ok == 0 {
                return Err(e);
            }
            Ok(PathBuf::from(String::from_utf16_lossy(&buf[..n as usize])))
        }
    }

    fn same_path(a: &std::path::Path, b: &std::path::Path) -> bool {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }

    pub fn server(conn: &Conn) -> io::Result<()> {
        let owner = owner_of(conn).map_err(|e| denied(&format!("its owner is unknown ({e})")))?;
        if owner != SID_SYSTEM && owner != SID_ADMINISTRATORS {
            return Err(denied("it is not owned by SYSTEM or Administrators"));
        }
        let mut pid = 0u32;
        // SAFETY: the handle is a connected pipe client end.
        if unsafe { GetNamedPipeServerProcessId(conn.handle(), &mut pid) } == 0 {
            return Err(denied("its process is unknown"));
        }
        let running = image_of(pid).map_err(|e| denied(&format!("its image is unknown ({e})")))?;
        let registered = registered_image().map_err(|e| denied(&format!("the service is not registered ({e})")))?;
        if !same_path(&running, &registered) {
            return Err(denied("its image is not the registered one"));
        }
        Ok(())
    }
}
