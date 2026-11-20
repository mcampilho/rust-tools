//! Running: as the Windows service (`run`), or in a console (`console`).

use crate::config::{console_log, data_dir, file_log, Config};
use crate::handler::Svc;
use crate::scm::wide;
use rt_svc::pipe::Event;
use rt_svc::server::{run, Log};
use rt_svc::{DEV_PIPE_NAME, PIPE_NAME, SERVICE_NAME};
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use windows_sys::Win32::Foundation::{BOOL, ERROR_ACCESS_DENIED};
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
use windows_sys::Win32::System::Services::{
    RegisterServiceCtrlHandlerExW, SetServiceStatus, StartServiceCtrlDispatcherW, SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP,
    SERVICE_CONTROL_INTERROGATE, SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STATUS,
    SERVICE_STOPPED, SERVICE_STOP_PENDING, SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS,
};

static STOP: OnceLock<Arc<Event>> = OnceLock::new();
static STATUS_HANDLE: AtomicIsize = AtomicIsize::new(0);
static STATE: AtomicU32 = AtomicU32::new(SERVICE_START_PENDING);

fn report(state: u32, exit: u32, hint_ms: u32) {
    STATE.store(state, Ordering::SeqCst);
    let accepted = if state == SERVICE_RUNNING { SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN } else { 0 };
    let st = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: accepted,
        dwWin32ExitCode: exit,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: hint_ms,
    };
    // SAFETY: the handle was given by RegisterServiceCtrlHandlerExW; the structure is valid.
    unsafe { SetServiceStatus(STATUS_HANDLE.load(Ordering::SeqCst) as _, &st) };
}

unsafe extern "system" fn control(code: u32, _event: u32, _data: *mut c_void, _ctx: *mut c_void) -> u32 {
    match code {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            report(SERVICE_STOP_PENDING, 0, 10_000);
            if let Some(stop) = STOP.get() {
                stop.set();
            }
        }
        SERVICE_CONTROL_INTERROGATE => report(STATE.load(Ordering::SeqCst), 0, 0),
        _ => {}
    }
    0
}

unsafe extern "system" fn service_main(_argc: u32, _argv: *mut *mut u16) {
    let name = wide(SERVICE_NAME);
    let h = RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(control), std::ptr::null_mut());
    if h.is_null() {
        return;
    }
    STATUS_HANDLE.store(h as isize, Ordering::SeqCst);
    report(SERVICE_START_PENDING, 0, 10_000);
    let log = file_log(&data_dir().join("logs"));

    // Only from a folder that only administrators may change.
    let dir = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf()));
    let problems = dir.as_deref().map(rt_svc::secure::check_dir).unwrap_or_else(|| vec!["the folder of the program is unknown".into()]);
    if !problems.is_empty() {
        for p in &problems {
            log(&format!("not starting, unsafe folder: {p}"));
        }
        report(SERVICE_STOPPED, ERROR_ACCESS_DENIED, 0);
        return;
    }

    let stop = Arc::new(Event::new().expect("an event"));
    let _ = STOP.set(stop.clone());
    let cfg = Config::load();
    log(&format!("starting {} (visibility {:?})", env!("CARGO_PKG_VERSION"), cfg.visibility));
    report(SERVICE_RUNNING, 0, 0);
    let result = run(PIPE_NAME, Arc::new(Svc::new(cfg.visibility, crate::handler::ntfs_source())), stop, log.clone());
    if let Err(e) = result {
        log(&format!("the pipe could not be served: {e}"));
    }
    report(SERVICE_STOPPED, 0, 0);
}

/// What Windows starts (`rtsvc run`). Returns the exit code of the process.
pub fn run_service() -> i32 {
    let name = wide(SERVICE_NAME);
    let table = [
        SERVICE_TABLE_ENTRYW { lpServiceName: name.as_ptr() as *mut u16, lpServiceProc: Some(service_main) },
        SERVICE_TABLE_ENTRYW { lpServiceName: std::ptr::null_mut(), lpServiceProc: None },
    ];
    // SAFETY: the table ends with a null entry and outlives the call (it returns when the service stops).
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        return 1;
    }
    0
}

unsafe extern "system" fn console_ctrl(_kind: u32) -> BOOL {
    if let Some(stop) = STOP.get() {
        stop.set();
    }
    1
}

/// The service in this window, as this user, on the development pipe.
pub fn console(visibility: rt_svc::Visibility, log: Log) -> std::io::Result<()> {
    let stop = Arc::new(Event::new()?);
    let _ = STOP.set(stop.clone());
    // SAFETY: the handler is a plain function.
    unsafe { SetConsoleCtrlHandler(Some(console_ctrl), 1) };
    run(DEV_PIPE_NAME, Arc::new(Svc::new(visibility, crate::handler::ntfs_source())), stop, log)
}

pub fn console_logger() -> Log {
    console_log()
}
