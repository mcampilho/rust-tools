//! Running: as the Windows service (`run`), or in a console (`console`).

use crate::config::{console_log, data_dir, file_log, Config};
use crate::handler::Svc;
use crate::scm::wide;
use crate::snapshots::{now, Store};
use rt_svc::pipe::Event;
use rt_svc::server::{run, Log};
use rt_svc::{DEV_PIPE_NAME, PIPE_NAME, SERVICE_NAME};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
use windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE;
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
    // Before anything is read from or written to it (the log included): the data folder is the service's own.
    let data = data_dir();
    let usable = || rt_svc::secure::check_data_dir(&data).is_empty();
    if !usable() || crate::config::protect(&data, crate::config::DATA_SDDL).is_err() || !usable() {
        report(SERVICE_STOPPED, ERROR_ACCESS_DENIED, 0);
        return;
    }
    let log = file_log(&data.join("logs"));

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
    let store = start_background(&stop, &log, Some(data_dir().join("cache")));
    let result = run(PIPE_NAME, Arc::new(Svc::new(cfg.visibility, crate::handler::ntfs_source(), store, jobs())), stop, log.clone());
    // A failure is told to the Service Control Manager too (it shows in its state and in the event log).
    let mut exit = 0;
    if let Err(e) = result {
        log(&format!("the pipe could not be served: {e}"));
        exit = e.raw_os_error().map_or(ERROR_ACCESS_DENIED, |c| c as u32);
    }
    report(SERVICE_STOPPED, exit, 0);
}

/// The snapshots and what makes them: a thread that makes them when the computer is idle (or a
/// client asks), and the watch on the keys of the installed programs. `cache` is where they are
/// kept: a folder only administrators may read, made here (without it nothing is kept on disk).
fn start_background(stop: &Arc<Event>, log: &Log, cache: Option<std::path::PathBuf>) -> Arc<Store> {
    let dir = cache.filter(|d| match crate::config::protect(d, crate::config::PRIVATE_SDDL) {
        Ok(()) => true,
        Err(e) => {
            log(&format!("the snapshots are not kept on disk: {e}"));
            false
        }
    });
    let store = Arc::new(Store::new(crate::kinds::all(), dir));
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let (store, stop, cancel, log) = (store.clone(), stop.clone(), cancel.clone(), log.clone());
        std::thread::spawn(move || {
            // The wait on `stop` is what ends the thread: set the flag, wake the scheduler.
            let watcher = {
                let (stop, cancel, store) = (stop.clone(), cancel.clone(), store.clone());
                std::thread::spawn(move || {
                    stop.wait(u32::MAX);
                    cancel.store(true, Ordering::SeqCst);
                    store.poke();
                })
            };
            crate::idle::work_in_background();
            let idle = crate::idle::Idle::default();
            while !cancel.load(Ordering::SeqCst) {
                store.wait(std::time::Duration::from_secs(30));
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let n = store.tick(now(), idle.check(), &cancel);
                if n > 0 {
                    log(&format!("made {n} snapshot(s)"));
                }
            }
            let _ = watcher.join();
        });
    }
    // A change in the list of installed programs (both views of the registry).
    let s = store.clone();
    crate::watch::watch(
        HKEY_LOCAL_MACHINE,
        &[
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ],
        stop.clone(),
        move || s.mark_dirty(rt_svc::proto::snapshots::PROGRAMS),
    );
    store
}

/// The jobs, done by what the service really does.
fn jobs() -> Arc<crate::jobs::Jobs> {
    Arc::new(crate::jobs::Jobs::new(Arc::new(crate::work::Real::new(data_dir().join("backups")))))
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
    let store = start_background(&stop, &log, None);
    run(DEV_PIPE_NAME, Arc::new(Svc::new(visibility, crate::handler::ntfs_source(), store, jobs())), stop, log)
}

pub fn console_logger() -> Log {
    console_log()
}
