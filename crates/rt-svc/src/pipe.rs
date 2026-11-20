//! The named pipe: the ends of a connection (reads and writes that give up
//! after a time, or when the service is asked to stop), the listener of the
//! service and the connection of a client.

use std::io::{self, Read, Write};
use std::mem::zeroed;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
    FILE_WRITE_DATA, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, SYNCHRONIZE,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, WaitNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

/// Who may reach the pipe: SYSTEM and Administrators fully; the network not at
/// all; authenticated users may read and write data, read the attributes (what
/// `CreateFile` always asks for), the security descriptor (to see the owner)
/// and synchronize: 0x120083. Not `FILE_APPEND_DATA` (0x4): it is the right to
/// create an instance of the pipe, which would let a user answer in the
/// service's place.
pub const SDDL: &str = "D:(D;;GA;;;NU)(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;OW)(A;;0x00120083;;;AU)";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_error() -> io::Error {
    // SAFETY: no arguments.
    io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}

/// An event handle closed on drop.
pub struct Event(pub HANDLE);

// SAFETY: an event handle can be used from any thread.
unsafe impl Send for Event {}
// SAFETY: the Win32 event functions are thread-safe.
unsafe impl Sync for Event {}

impl Event {
    /// A manual-reset event, not set.
    pub fn new() -> io::Result<Event> {
        // SAFETY: plain call, no pointers.
        let h = unsafe { CreateEventW(null(), 1, 0, null()) };
        if h.is_null() {
            return Err(last_error());
        }
        Ok(Event(h))
    }

    /// Whether the event is set.
    pub fn is_set(&self) -> bool {
        // SAFETY: the handle is a valid event; a zero wait only looks.
        unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
    }

    /// Waits until the event is set, or ms milliseconds have passed. Returns whether it is set.
    pub fn wait(&self, ms: u32) -> bool {
        // SAFETY: the handle is a valid event.
        unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(self.0, ms) == WAIT_OBJECT_0 }
    }

    pub fn set(&self) {
        // SAFETY: the handle is a valid event.
        unsafe { windows_sys::Win32::System::Threading::SetEvent(self.0) };
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: the handle is ours.
        unsafe { CloseHandle(self.0) };
    }
}

/// Waits for an overlapped operation to finish, for `timeout_ms`, or until `stop` is set.
/// On a timeout or a stop the operation is cancelled.
fn wait(h: HANDLE, ov: &mut OVERLAPPED, stop: HANDLE, timeout_ms: u32) -> io::Result<usize> {
    let handles = [ov.hEvent, stop];
    let n = if stop.is_null() { 1 } else { 2 };
    // SAFETY: the handles are valid events and the count matches the array.
    let r = unsafe { WaitForMultipleObjects(n, handles.as_ptr(), 0, timeout_ms) };
    let mut got = 0u32;
    if r != WAIT_OBJECT_0 {
        // SAFETY: cancel the operation of this overlapped structure, then wait for it to end.
        unsafe {
            CancelIoEx(h, ov);
            if GetOverlappedResult(h, ov, &mut got, 1) != 0 && got > 0 {
                return Ok(got as usize);
            }
        }
        return Err(if r == WAIT_TIMEOUT {
            io::Error::new(io::ErrorKind::TimedOut, "the other end of the pipe is silent")
        } else {
            io::Error::new(io::ErrorKind::ConnectionAborted, "stopped")
        });
    }
    // SAFETY: the operation finished.
    if unsafe { GetOverlappedResult(h, ov, &mut got, 1) } == 0 {
        return Err(last_error());
    }
    Ok(got as usize)
}

/// One end of a connection.
pub struct Conn {
    h: HANDLE,
    /// An event that, when set, interrupts every wait (null: none).
    stop: HANDLE,
    timeout_ms: u32,
}

// SAFETY: a pipe handle can be used from one thread at a time, and a Conn is moved, not shared.
unsafe impl Send for Conn {}

impl Conn {
    /// The raw handle, for the checks of `access`.
    pub fn handle(&self) -> HANDLE {
        self.h
    }

    pub fn set_timeout(&mut self, ms: u32) {
        self.timeout_ms = ms;
    }

    fn io(&mut self, read: bool, buf: *mut u8, len: u32) -> io::Result<usize> {
        let ev = Event::new()?;
        // SAFETY: an all-zero OVERLAPPED is its initial state.
        let mut ov: OVERLAPPED = unsafe { zeroed() };
        ov.hEvent = ev.0;
        // SAFETY: the buffer is valid for `len` bytes and outlives the operation (we wait for it).
        let ok = unsafe {
            if read {
                ReadFile(self.h, buf, len, null_mut(), &mut ov)
            } else {
                WriteFile(self.h, buf, len, null_mut(), &mut ov)
            }
        };
        if ok == 0 {
            // SAFETY: no arguments.
            let e = unsafe { GetLastError() };
            if e == ERROR_BROKEN_PIPE && read {
                return Ok(0);
            }
            if e != ERROR_IO_PENDING {
                return Err(io::Error::from_raw_os_error(e as i32));
            }
            return match wait(self.h, &mut ov, self.stop, self.timeout_ms) {
                Err(e) if read && e.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) => Ok(0),
                r => r,
            };
        }
        let mut got = 0u32;
        // SAFETY: the operation completed at once.
        if unsafe { GetOverlappedResult(self.h, &ov, &mut got, 1) } == 0 {
            return Err(last_error());
        }
        Ok(got as usize)
    }
}

impl Read for Conn {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let len = buf.len().min(1 << 20) as u32;
        self.io(true, buf.as_mut_ptr(), len)
    }
}

impl Write for Conn {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let len = buf.len().min(1 << 20) as u32;
        self.io(false, buf.as_ptr() as *mut u8, len)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        // SAFETY: the handle is ours.
        // Not DisconnectNamedPipe: it throws away what the client has not read yet.
        unsafe {
            CloseHandle(self.h);
        }
    }
}

/// The service's side: instances of the pipe, one always waiting.
pub struct Listener {
    name: Vec<u16>,
    sd: *mut core::ffi::c_void,
    pending: HANDLE,
    stop: HANDLE,
}

// SAFETY: used from the accept thread only; the handles are plain values.
unsafe impl Send for Listener {}

impl Listener {
    /// Creates the pipe `name` with the descriptor `sddl`. It fails when the
    /// name is already taken: nobody else may be first. `stop` interrupts `accept`.
    pub fn new(name: &str, sddl: &str, stop: HANDLE) -> io::Result<Listener> {
        let mut sd = null_mut();
        let text = wide(sddl);
        // SAFETY: a valid string and out pointer.
        if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), SDDL_REVISION_1, &mut sd, null_mut()) } == 0 {
            return Err(last_error());
        }
        let mut l = Listener { name: wide(name), sd, pending: INVALID_HANDLE_VALUE, stop };
        l.pending = l.instance(true)?;
        Ok(l)
    }

    fn instance(&self, first: bool) -> io::Result<HANDLE> {
        let sa = SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: self.sd, bInheritHandle: 0 };
        let open = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { 0 };
        // SAFETY: valid name, attributes and sizes.
        let h = unsafe {
            CreateNamedPipeW(
                self.name.as_ptr(),
                open,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                1 << 16,
                1 << 16,
                0,
                &sa,
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(last_error());
        }
        Ok(h)
    }

    /// Waits for a client. The next instance is created before this one is
    /// handed over, so there is never a moment without one waiting.
    pub fn accept(&mut self, timeout_ms: u32) -> io::Result<Conn> {
        if self.pending == INVALID_HANDLE_VALUE {
            self.pending = self.instance(false)?;
        }
        let ev = Event::new()?;
        // SAFETY: an all-zero OVERLAPPED is its initial state.
        let mut ov: OVERLAPPED = unsafe { zeroed() };
        ov.hEvent = ev.0;
        // SAFETY: the pending instance is a valid pipe handle.
        let ok = unsafe { ConnectNamedPipe(self.pending, &mut ov) };
        if ok == 0 {
            // SAFETY: no arguments.
            let e = unsafe { GetLastError() };
            let waited = if e == ERROR_IO_PENDING {
                wait(self.pending, &mut ov, self.stop, timeout_ms)
            } else if e != ERROR_PIPE_CONNECTED {
                Err(io::Error::from_raw_os_error(e as i32))
            } else {
                Ok(0)
            };
            if let Err(e) = waited {
                // A client that connected and left before we looked ("the pipe is being
                // closed"), or a stop: this instance is no good any more. The next call makes another.
                // SAFETY: the handle is ours and not used again.
                unsafe { CloseHandle(self.pending) };
                self.pending = INVALID_HANDLE_VALUE;
                return Err(e);
            }
        }
        let connected = self.pending;
        let conn = Conn { h: connected, stop: self.stop, timeout_ms: 60_000 };
        // If the next instance cannot be made, `conn` is dropped (closed) with the error,
        // and the next call makes the instance again.
        self.pending = INVALID_HANDLE_VALUE;
        self.pending = self.instance(false)?;
        Ok(conn)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        // SAFETY: both are ours.
        unsafe {
            if self.pending != INVALID_HANDLE_VALUE {
                CloseHandle(self.pending);
            }
            LocalFree(self.sd as *mut _);
        }
    }
}

/// Connects to the pipe `name`, waiting at most `wait_ms` for it to be free.
/// The server may only learn who we are, not act as us.
pub fn connect(name: &str, wait_ms: u32) -> io::Result<Conn> {
    let w = wide(name);
    let start = std::time::Instant::now();
    loop {
        // SAFETY: valid name; the rights asked are exactly those the pipe grants to users.
        let h = unsafe {
            CreateFileW(
                w.as_ptr(),
                FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE | 0x20000, /* READ_CONTROL: the owner is read */
                0,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                null_mut(),
            )
        };
        if h != INVALID_HANDLE_VALUE {
            return Ok(Conn { h, stop: null_mut(), timeout_ms: 60_000 });
        }
        // SAFETY: no arguments.
        let e = unsafe { GetLastError() };
        let left = (wait_ms as u128).saturating_sub(start.elapsed().as_millis()) as u32;
        if e != ERROR_PIPE_BUSY || left == 0 {
            return Err(io::Error::from_raw_os_error(e as i32));
        }
        // SAFETY: valid name.
        unsafe { WaitNamedPipeW(w.as_ptr(), left) };
    }
}
