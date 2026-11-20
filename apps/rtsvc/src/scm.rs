//! The Service Control Manager: installing, starting, stopping and removing
//! the service, and asking what state it is in.

use rt_svc::SERVICE_NAME;
use std::io;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Services::{
    ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, ControlService, CreateServiceW, DeleteService, OpenSCManagerW,
    OpenServiceW, QueryServiceConfigW, QueryServiceStatus, StartServiceW, QUERY_SERVICE_CONFIGW, SC_ACTION, SC_ACTION_RESTART, SC_HANDLE,
    SC_MANAGER_CONNECT, SC_MANAGER_CREATE_SERVICE, SERVICE_AUTO_START, SERVICE_CONFIG_DELAYED_AUTO_START_INFO, SERVICE_CONFIG_DESCRIPTION,
    SERVICE_CONFIG_FAILURE_ACTIONS, SERVICE_CONTROL_STOP, SERVICE_DELAYED_AUTO_START_INFO, SERVICE_DESCRIPTIONW, SERVICE_ERROR_NORMAL,
    SERVICE_FAILURE_ACTIONSW, SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START, SERVICE_START_PENDING,
    SERVICE_STATUS, SERVICE_STOP, SERVICE_STOPPED, SERVICE_STOP_PENDING, SERVICE_WIN32_OWN_PROCESS,
};

const DELETE: u32 = 0x1_0000;
const SERVICE_NO_CHANGE: u32 = 0xffff_ffff;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn last_error() -> io::Error {
    // SAFETY: no arguments.
    io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}

/// A handle of the manager or of a service, closed on drop.
struct Handle(SC_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle is ours.
        unsafe { CloseServiceHandle(self.0) };
    }
}

fn manager(access: u32) -> io::Result<Handle> {
    // SAFETY: null names mean this machine and the active database.
    let h = unsafe { OpenSCManagerW(null(), null(), access) };
    if h.is_null() {
        return Err(last_error());
    }
    Ok(Handle(h))
}

fn open(scm: &Handle, access: u32) -> io::Result<Handle> {
    let name = wide(SERVICE_NAME);
    // SAFETY: valid handle and name.
    let h = unsafe { OpenServiceW(scm.0, name.as_ptr(), access) };
    if h.is_null() {
        return Err(last_error());
    }
    Ok(Handle(h))
}

/// What state the service is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Stopped,
    Starting,
    Running,
    Stopping,
    Other(u32),
}

fn state_of(h: &Handle) -> io::Result<State> {
    // SAFETY: an all-zero SERVICE_STATUS is valid to be filled in.
    let mut st: SERVICE_STATUS = unsafe { std::mem::zeroed() };
    // SAFETY: valid handle and out pointer.
    if unsafe { QueryServiceStatus(h.0, &mut st) } == 0 {
        return Err(last_error());
    }
    Ok(match st.dwCurrentState {
        SERVICE_STOPPED => State::Stopped,
        SERVICE_START_PENDING => State::Starting,
        SERVICE_RUNNING => State::Running,
        SERVICE_STOP_PENDING => State::Stopping,
        n => State::Other(n),
    })
}

/// The state of the installed service, and the program it runs; `NotFound`
/// when it is not installed.
pub fn query() -> io::Result<(State, PathBuf)> {
    let scm = manager(SC_MANAGER_CONNECT)?;
    let svc = open(&scm, SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG)?;
    let state = state_of(&svc)?;
    let mut need = 0u32;
    // SAFETY: asking for the size.
    unsafe { QueryServiceConfigW(svc.0, null_mut(), 0, &mut need) };
    let mut buf = vec![0u64; (need as usize).div_ceil(8).max(1)];
    // SAFETY: the buffer has `need` bytes at least.
    if unsafe { QueryServiceConfigW(svc.0, buf.as_mut_ptr() as *mut QUERY_SERVICE_CONFIGW, (buf.len() * 8) as u32, &mut need) } == 0 {
        return Err(last_error());
    }
    // SAFETY: the buffer holds the structure and the string it points to.
    let line = unsafe {
        let p = (*(buf.as_ptr() as *const QUERY_SERVICE_CONFIGW)).lpBinaryPathName;
        let mut n = 0;
        while *p.add(n) != 0 {
            n += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
    };
    let image = match line.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or("").to_string(),
        None => line.split(' ').next().unwrap_or("").to_string(),
    };
    Ok((state, PathBuf::from(image)))
}

fn wait_for(svc: &Handle, want: State, secs: u64) -> io::Result<bool> {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        if state_of(svc)? == want {
            return Ok(true);
        }
        if Instant::now() >= end {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Stops the service if it runs. Returns whether it is stopped.
pub fn stop() -> io::Result<bool> {
    let scm = manager(SC_MANAGER_CONNECT)?;
    let svc = open(&scm, SERVICE_STOP | SERVICE_QUERY_STATUS)?;
    if state_of(&svc)? == State::Stopped {
        return Ok(true);
    }
    // SAFETY: an all-zero SERVICE_STATUS is valid to be filled in.
    let mut st: SERVICE_STATUS = unsafe { std::mem::zeroed() };
    // SAFETY: valid handle and out pointer.
    unsafe { ControlService(svc.0, SERVICE_CONTROL_STOP, &mut st) };
    wait_for(&svc, State::Stopped, 20)
}

/// Starts the service and waits until it runs. Returns whether it does.
pub fn start() -> io::Result<bool> {
    let scm = manager(SC_MANAGER_CONNECT)?;
    let svc = open(&scm, SERVICE_START | SERVICE_QUERY_STATUS)?;
    // SAFETY: no arguments for the service.
    if unsafe { StartServiceW(svc.0, 0, null()) } == 0 {
        let e = last_error();
        // 1056: already running.
        if e.raw_os_error() != Some(1056) {
            return Err(e);
        }
    }
    wait_for(&svc, State::Running, 20)
}

/// Registers the service (or changes the program of the one registered) to run `exe run`, automatic with a delayed start,
/// restarted after a failure.
pub fn install(exe: &Path) -> io::Result<()> {
    let scm = manager(SC_MANAGER_CONNECT | SC_MANAGER_CREATE_SERVICE)?;
    let line = wide(&format!("\"{}\" run", exe.display()));
    let name = wide(SERVICE_NAME);
    let display = wide("rust-tools service");
    let svc = match open(&scm, 0xF01FF) {
        Ok(existing) => {
            // SAFETY: valid handle and strings; the others stay as they are.
            let ok = unsafe {
                ChangeServiceConfigW(
                    existing.0,
                    SERVICE_NO_CHANGE,
                    SERVICE_AUTO_START,
                    SERVICE_NO_CHANGE,
                    line.as_ptr(),
                    null(),
                    null_mut(),
                    null(),
                    null(),
                    null(),
                    display.as_ptr(),
                )
            };
            if ok == 0 {
                return Err(last_error());
            }
            existing
        }
        Err(e) if e.raw_os_error() == Some(1060) => {
            // SAFETY: valid strings; LocalSystem is the default account.
            let h = unsafe {
                CreateServiceW(
                    scm.0,
                    name.as_ptr(),
                    display.as_ptr(),
                    0xF01FF,
                    SERVICE_WIN32_OWN_PROCESS,
                    SERVICE_AUTO_START,
                    SERVICE_ERROR_NORMAL,
                    line.as_ptr(),
                    null(),
                    null_mut(),
                    null(),
                    null(),
                    null(),
                )
            };
            if h.is_null() {
                return Err(last_error());
            }
            Handle(h)
        }
        Err(e) => return Err(e),
    };
    let mut text = wide("Lets the rust-tools read system-wide information and does in the background what an elevated tool asks.");
    let desc = SERVICE_DESCRIPTIONW { lpDescription: text.as_mut_ptr() };
    let delayed = SERVICE_DELAYED_AUTO_START_INFO { fDelayedAutostart: 1 };
    let mut actions = [
        SC_ACTION { Type: SC_ACTION_RESTART, Delay: 5000 },
        SC_ACTION { Type: SC_ACTION_RESTART, Delay: 30_000 },
        SC_ACTION { Type: SC_ACTION_RESTART, Delay: 60_000 },
    ];
    let failure = SERVICE_FAILURE_ACTIONSW {
        dwResetPeriod: 86_400,
        lpRebootMsg: null_mut(),
        lpCommand: null_mut(),
        cActions: actions.len() as u32,
        lpsaActions: actions.as_mut_ptr(),
    };
    // SAFETY: valid handle; each structure is the one its level asks for and outlives the call.
    unsafe {
        ChangeServiceConfig2W(svc.0, SERVICE_CONFIG_DESCRIPTION, &desc as *const _ as *const _);
        ChangeServiceConfig2W(svc.0, SERVICE_CONFIG_DELAYED_AUTO_START_INFO, &delayed as *const _ as *const _);
        ChangeServiceConfig2W(svc.0, SERVICE_CONFIG_FAILURE_ACTIONS, &failure as *const _ as *const _);
    }
    Ok(())
}

/// Removes the registration of the service (it must be stopped).
pub fn remove() -> io::Result<()> {
    let scm = manager(SC_MANAGER_CONNECT)?;
    let svc = open(&scm, DELETE | SERVICE_QUERY_STATUS)?;
    // SAFETY: valid handle.
    if unsafe { DeleteService(svc.0) } == 0 {
        return Err(last_error());
    }
    Ok(())
}
