//! Uninstalling: what to run for each program (its own uninstaller,
//! Windows Installer, the package manager of Windows, Scoop, Chocolatey or
//! winget), running it, and waiting until it is done, with the processes it
//! starts: many uninstallers copy themselves to the temporary folder, start
//! that copy and end at once (Inno Setup, NSIS).

use crate::inventory::{command_file, Me, Problem, Program, Scope, Source};
use std::path::PathBuf;

/// How to uninstall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// With the uninstaller's own windows.
    Normal,
    /// Without questions, where the program says how (or its installer is
    /// known); otherwise as `Normal`.
    Quiet,
    /// Change or repair (Windows Installer's maintenance, `ModifyPath`).
    Modify,
}

/// What to do for a program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Start a program and wait for it (and what it starts).
    Run {
        file: String,
        params: String,
        /// Ask for administrator rights (Windows asks the user).
        elevate: bool,
        /// No window (package managers' command lines): its output is kept.
        hidden: bool,
        env: Vec<(String, String)>,
    },
    /// Remove a Store or MSIX package for the user running runinst.
    Package(String),
    /// It cannot be done from here: why.
    Cannot(String),
}

/// A command line split into the program and its parameters.
pub fn split(cmd: &str) -> (String, String) {
    let c = cmd.trim();
    if let Some(rest) = c.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return (rest[..end].to_string(), rest[end + 1..].trim().to_string());
        }
    }
    let file = command_file(c).map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let params = c.get(file.len()..).unwrap_or("").trim().to_string();
    (file, params)
}

/// Whether an uninstaller was made by Inno Setup (it takes `/VERYSILENT`).
fn inno(p: &Program) -> bool {
    p.values.iter().any(|(k, _)| k.starts_with("Inno Setup:"))
        || p.uninstall.as_deref().map(|u| split(u).0.to_lowercase()).is_some_and(|f| {
            let n = f.rsplit(['\\', '/']).next().unwrap_or("");
            n.starts_with("unins") && n.ends_with(".exe") && n.len() == 12
        })
}

/// A Windows path without its last part (the same on Linux, for the tests).
fn parent(path: &str) -> &str {
    path.rsplit_once(['\\', '/']).map_or("", |(a, _)| a)
}

/// What a message names `key`, for `Cannot` (translated by the window).
pub const OTHER_USER: &str = "other user";
pub const NO_UNINSTALLER: &str = "no uninstaller";
pub const NO_REMOVE: &str = "cannot be removed";
pub const NOT_ELEVATED_PACKAGE: &str = "package of the desktop user";
pub const NO_MODIFY: &str = "no modify";

fn run(file: &str, params: &str, elevate: bool) -> Plan {
    Plan::Run { file: file.to_string(), params: params.to_string(), elevate, hidden: false, env: Vec::new() }
}

fn hidden(file: &str, params: &str, elevate: bool, env: Vec<(String, String)>) -> Plan {
    Plan::Run { file: file.to_string(), params: params.to_string(), elevate, hidden: true, env }
}

/// What to do to uninstall (or change) `p`. `admin`: runinst has
/// administrator rights.
/// Whether "Modify or Repair" can do anything for `p`.
pub fn can_modify(p: &Program) -> bool {
    // Scoop's repair: `scoop install` again after a failed install, else
    // `scoop reset` (its shims, shortcuts and current version again).
    if p.sources.first() == Some(&Source::Scoop) {
        return true;
    }
    let manager = p.sources.first() == Some(&Source::Chocolatey);
    // `NoModify` and `NoRepair` both set: Windows offers no change either.
    let flag = |name: &str| p.values.iter().any(|(k, v)| k.eq_ignore_ascii_case(name) && (v.trim() == "1" || v.trim_end().ends_with("(1)")));
    let closed = flag("NoModify") && flag("NoRepair");
    !manager && !closed && p.package.is_none() && (p.msi.is_some() || p.modify.is_some())
}

pub fn plan(p: &Program, how: How, admin: bool, me: &Me) -> Plan {
    if p.no_remove && how != How::Modify {
        return Plan::Cannot(NO_REMOVE.into());
    }
    let mine = matches!(p.scope, Scope::User { current: true, .. });
    let other = matches!(p.scope, Scope::User { current: false, .. });
    let first = p.sources.first().copied();
    // Only a program with a change command of its own, or Windows
    // Installer's, can be changed; package managers have none (Scoop's and
    // Chocolatey's would uninstall).
    if how == How::Modify && !can_modify(p) {
        return Plan::Cannot(NO_MODIFY.into());
    }
    match first {
        Some(Source::Scoop) => {
            // The app's folder is `<root>\apps\<name>`.
            let root = PathBuf::from(parent(parent(&p.key)));
            let global = p.scope == Scope::Machine;
            let script = format!("{}\\apps\\scoop\\current\\bin\\scoop.ps1", root.display());
            // A failed install is installed again (Scoop purges the failed
            // one first); a good one is reset. `reset` finds a global app by
            // itself, and has no `--global`.
            let flag = if global { "--global " } else { "" };
            let params = if how == How::Modify && p.problems.contains(&Problem::InstallFailed) {
                format!("-NoProfile -ExecutionPolicy Bypass -File \"{script}\" install {flag}{}", p.name)
            } else if how == How::Modify {
                format!("-NoProfile -ExecutionPolicy Bypass -File \"{script}\" reset {}", p.name)
            } else {
                format!("-NoProfile -ExecutionPolicy Bypass -File \"{script}\" uninstall {flag}{}", p.name)
            };
            let var = if global { "SCOOP_GLOBAL" } else { "SCOOP" };
            return hidden("powershell.exe", &params, global && !admin, vec![(var.into(), root.display().to_string())]);
        }
        Some(Source::Chocolatey) => {
            let id = p.values.iter().find(|(k, _)| k == "id").map_or(p.name.clone(), |(_, v)| v.clone());
            let root = parent(parent(parent(&p.key)));
            let choco = if root.is_empty() { "choco.exe".to_string() } else { format!("{root}\\bin\\choco.exe") };
            return hidden(&choco, &format!("uninstall {id} -y"), !admin, Vec::new());
        }
        _ => {}
    }
    // A Store or MSIX package: removed by the package manager of Windows,
    // for the user runinst runs as.
    if let Some(full) = &p.package {
        if how == How::Modify {
            return Plan::Cannot(NO_UNINSTALLER.into());
        }
        return if mine && me.runs_as { Plan::Package(full.clone()) } else if mine { Plan::Cannot(NOT_ELEVATED_PACKAGE.into()) } else { Plan::Cannot(OTHER_USER.into()) };
    }
    // Another user's own program: their uninstaller would change their
    // settings, not this account's.
    if other && first == Some(Source::Registry) {
        return Plan::Cannot(OTHER_USER.into());
    }
    // Windows Installer.
    if let Some(code) = &p.msi {
        let machine = p.scope == Scope::Machine;
        return match how {
            How::Normal => run("msiexec.exe", &format!("/X{code}"), false),
            // Quiet needs the rights first: without them, Windows Installer cannot ask.
            How::Quiet => run("msiexec.exe", &format!("/X{code} /qn /norestart"), machine && !admin),
            How::Modify => run("msiexec.exe", &format!("/I{code}"), false),
        };
    }
    if how == How::Modify {
        return match &p.modify {
            Some(m) => {
                let (f, a) = split(m);
                run(&f, &a, false)
            }
            None => Plan::Cannot(NO_UNINSTALLER.into()),
        };
    }
    if how == How::Quiet {
        if let Some(q) = &p.quiet {
            let (f, a) = split(q);
            return run(&f, &a, false);
        }
        if inno(p) {
            if let Some(u) = &p.uninstall {
                let (f, a) = split(u);
                return run(&f, format!("{a} /VERYSILENT /SUPPRESSMSGBOXES /NORESTART").trim(), false);
            }
        }
    }
    if let Some(u) = &p.uninstall {
        let (f, a) = split(u);
        return run(&f, &a, false);
    }
    // Only winget knows it.
    if let Some(w) = &p.winget {
        let quiet = if how == How::Quiet { " --silent" } else { "" };
        return hidden("winget", &format!("uninstall --id \"{}\" --exact --disable-interactivity --accept-source-agreements{quiet}", w.id), false, Vec::new());
    }
    Plan::Cannot(NO_UNINSTALLER.into())
}

/// How it went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It ran and ended with this code (0, 3010 and 1641 are fine; 3010 and
    /// 1641 ask for a restart), and what it printed (hidden runs).
    Ended(i64, String),
    /// It could not be started, or the user said no to the rights: why.
    Failed(String),
    Cannot(String),
}

impl Outcome {
    pub fn ok(&self) -> bool {
        // 1605: Windows Installer does not know the product (already gone).
        matches!(self, Outcome::Ended(0 | 3010 | 1641 | 1605, _))
    }

    pub fn needs_restart(&self) -> bool {
        matches!(self, Outcome::Ended(3010 | 1641, _))
    }
}

/// Carries out a plan; returns when the uninstaller and what it started have
/// ended.
#[cfg(windows)]
pub fn carry_out(plan: &Plan) -> Outcome {
    match plan {
        Plan::Cannot(why) => Outcome::Cannot(why.clone()),
        Plan::Package(full) => remove_package(full),
        Plan::Run { file, params, elevate, hidden, env } if *hidden && !*elevate => {
            use std::os::windows::process::CommandExt;
            const NO_WINDOW: u32 = 0x0800_0000;
            let mut c = std::process::Command::new(file);
            c.raw_arg(params).creation_flags(NO_WINDOW).stdin(std::process::Stdio::null());
            for (k, v) in env {
                c.env(k, v);
            }
            match c.output() {
                Ok(o) => {
                    let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
                    text.push_str(&String::from_utf8_lossy(&o.stderr));
                    // The last lines say how it went.
                    let tail: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
                    let tail = tail[tail.len().saturating_sub(4)..].join("\n");
                    Outcome::Ended(o.status.code().unwrap_or(-1) as i64, tail)
                }
                Err(e) => Outcome::Failed(e.to_string()),
            }
        }
        Plan::Run { file, params, elevate, hidden, env } => {
            for (k, v) in env {
                // SAFETY of the call: only this worker thread runs a plan at a time.
                std::env::set_var(k, v);
            }
            shell_run(file, params, *elevate, *hidden)
        }
    }
}

#[cfg(not(windows))]
pub fn carry_out(plan: &Plan) -> Outcome {
    match plan {
        Plan::Cannot(why) => Outcome::Cannot(why.clone()),
        _ => Outcome::Failed("Windows only".into()),
    }
}

#[cfg(windows)]
fn remove_package(full: &str) -> Outcome {
    use windows::core::HSTRING;
    use windows::Management::Deployment::PackageManager;
    // SAFETY: initialises WinRT on this thread (a worker of its own).
    let _ = unsafe { windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED) };
    let result = PackageManager::new().and_then(|pm| pm.RemovePackageAsync(&HSTRING::from(full))).and_then(|op| op.join());
    match result {
        Ok(r) => match r.ExtendedErrorCode() {
            Ok(code) if code.is_err() => Outcome::Failed(r.ErrorText().map(|t| t.to_string()).unwrap_or_else(|_| code.message())),
            _ => Outcome::Ended(0, String::new()),
        },
        Err(e) => Outcome::Failed(e.message()),
    }
}

/// Starts `file` through the shell (so that an uninstaller that needs the
/// rights gets Windows' question) and waits for it and its descendants.
#[cfg(windows)]
fn shell_run(file: &str, params: &str, elevate: bool, hidden: bool) -> Outcome {
    use rt_sys::win::wide;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, GetProcessId, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};
    let (verb, f, p) = (wide("runas"), wide(file), wide(params));
    // SAFETY: SHELLEXECUTEINFOW is plain data; zeroed is a valid start.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    info.lpVerb = if elevate { verb.as_ptr() } else { std::ptr::null() };
    info.lpFile = f.as_ptr();
    info.lpParameters = p.as_ptr();
    info.nShow = if hidden { SW_HIDE } else { SW_SHOWNORMAL };
    // SAFETY: the strings outlive the call; `info` is filled as documented.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        return Outcome::Failed(std::io::Error::last_os_error().to_string());
    }
    if info.hProcess.is_null() {
        return Outcome::Ended(0, String::new());
    }
    // SAFETY: a process handle of our own, closed below.
    let pid = unsafe { GetProcessId(info.hProcess) };
    let mut family = std::collections::HashSet::from([pid]);
    // While it runs, and after: its children (the copy in the temporary
    // folder of many uninstallers) are part of it.
    loop {
        // SAFETY: waits on our own handle for at most 400 ms.
        let done = unsafe { WaitForSingleObject(info.hProcess, 400) } == 0;
        grow(&mut family);
        if done {
            break;
        }
    }
    let mut code = 0u32;
    // SAFETY: our own handle; closed once.
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
    }
    family.remove(&pid);
    // Its descendants, until none is left (at most a day).
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(86_400) {
        let alive = grow(&mut family);
        if alive == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Outcome::Ended(code as i64, String::new())
}

/// Adds to `family` the processes started by one of it; returns how many of
/// it are running.
#[cfg(windows)]
fn grow(family: &mut std::collections::HashSet<u32>) -> usize {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    // SAFETY: a snapshot of the processes, closed below.
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap == INVALID_HANDLE_VALUE {
        return 0;
    }
    let mut procs = Vec::new();
    // SAFETY: PROCESSENTRY32W is plain data, its size set as required.
    let mut e: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    // SAFETY: walks the snapshot with a valid entry.
    let mut ok = unsafe { Process32FirstW(snap, &mut e) } != 0;
    while ok {
        procs.push((e.th32ProcessID, e.th32ParentProcessID));
        // SAFETY: as above.
        ok = unsafe { Process32NextW(snap, &mut e) } != 0;
    }
    // SAFETY: closes the snapshot.
    unsafe { CloseHandle(snap) };
    // Children of children, as far as they go.
    loop {
        let before = family.len();
        for &(pid, parent) in &procs {
            if family.contains(&parent) {
                family.insert(pid);
            }
        }
        if family.len() == before {
            break;
        }
    }
    procs.iter().filter(|(pid, _)| family.contains(pid)).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::WingetInfo;

    fn prog(uninstall: Option<&str>) -> Program {
        Program { name: "X".into(), uninstall: uninstall.map(Into::into), sources: vec![Source::Registry], scope: Scope::Machine, ..Program::default() }
    }

    fn me() -> Me {
        Me { sid: "S-1-5-21-1".into(), name: "me".into(), home: None, runs_as: true }
    }

    #[test]
    fn commands_split() {
        assert_eq!(split("\"C:\\Program Files\\X\\unins000.exe\" /LOG"), ("C:\\Program Files\\X\\unins000.exe".into(), "/LOG".into()));
        assert_eq!(split("C:\\Program Files\\Y\\uninstall.exe /S"), ("C:\\Program Files\\Y\\uninstall.exe".into(), "/S".into()));
        assert_eq!(split("MsiExec.exe /X{AB}"), ("MsiExec.exe".into(), "/X{AB}".into()));
        assert_eq!(split("\"C:\\x.exe\""), ("C:\\x.exe".into(), String::new()));
    }

    #[test]
    fn plans() {
        let me = me();
        let p = prog(Some("\"C:\\P\\uninstall.exe\" /x"));
        assert_eq!(plan(&p, How::Normal, false, &me), run("C:\\P\\uninstall.exe", "/x", false));
        // No quiet way known: as normal.
        assert_eq!(plan(&p, How::Quiet, false, &me), run("C:\\P\\uninstall.exe", "/x", false));
        let mut q = p.clone();
        q.quiet = Some("\"C:\\P\\uninstall.exe\" /S".into());
        assert_eq!(plan(&q, How::Quiet, false, &me), run("C:\\P\\uninstall.exe", "/S", false));
        let inno = prog(Some("\"C:\\P\\unins000.exe\""));
        assert_eq!(plan(&inno, How::Quiet, false, &me), run("C:\\P\\unins000.exe", "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART", false));
        let mut msi = prog(Some("MsiExec.exe /I{AB}"));
        msi.msi = Some("{AB}".into());
        assert_eq!(plan(&msi, How::Normal, false, &me), run("msiexec.exe", "/X{AB}", false));
        assert_eq!(plan(&msi, How::Quiet, false, &me), run("msiexec.exe", "/X{AB} /qn /norestart", true));
        assert_eq!(plan(&msi, How::Quiet, true, &me), run("msiexec.exe", "/X{AB} /qn /norestart", false));
        assert_eq!(plan(&msi, How::Modify, false, &me), run("msiexec.exe", "/I{AB}", false));
        let mut nr = p.clone();
        nr.no_remove = true;
        assert_eq!(plan(&nr, How::Normal, false, &me), Plan::Cannot(NO_REMOVE.into()));
        let mut other = p.clone();
        other.scope = Scope::User { name: "Ana".into(), current: false };
        assert_eq!(plan(&other, How::Normal, true, &me), Plan::Cannot(OTHER_USER.into()));
        // A package of the desktop's user: from runinst as that user only.
        let pk = Program { package: Some("A_1_x64__z".into()), scope: Scope::User { name: "me".into(), current: true }, sources: vec![Source::Store], ..Program::default() };
        assert_eq!(plan(&pk, How::Normal, false, &me), Plan::Package("A_1_x64__z".into()));
        let as_admin = Me { runs_as: false, ..me.clone() };
        assert_eq!(plan(&pk, How::Normal, true, &as_admin), Plan::Cannot(NOT_ELEVATED_PACKAGE.into()));
        let w = Program { winget: Some(WingetInfo { id: "Some.App".into(), ..WingetInfo::default() }), sources: vec![Source::Winget], ..Program::default() };
        assert!(matches!(plan(&w, How::Quiet, false, &me), Plan::Run { ref file, ref params, hidden: true, .. } if file == "winget" && params.contains("--id \"Some.App\"") && params.ends_with("--silent")));
        assert_eq!(plan(&Program::default(), How::Normal, false, &me), Plan::Cannot(NO_UNINSTALLER.into()));
        let scoop = Program { name: "rg".into(), key: "D:\\scoop\\apps\\rg".into(), sources: vec![Source::Scoop], scope: Scope::User { name: "me".into(), current: true }, ..Program::default() };
        let Plan::Run { file, params, elevate, env, .. } = plan(&scoop, How::Normal, false, &me) else { panic!() };
        assert_eq!((file.as_str(), elevate), ("powershell.exe", false));
        assert!(params.contains("D:\\scoop\\apps\\scoop\\current\\bin\\scoop.ps1\" uninstall rg"), "{params}");
        assert_eq!(env, [("SCOOP".to_string(), "D:\\scoop".to_string())]);
        // Scoop's repair is `scoop reset`; never its uninstall.
        let Plan::Run { params, .. } = plan(&scoop, How::Modify, false, &me) else { panic!() };
        assert!(params.ends_with("scoop.ps1\" reset rg") && !params.contains("uninstall"), "{params}");
        // ...and `scoop install` again after a failed install.
        let failed = Program { problems: vec![Problem::InstallFailed], ..scoop.clone() };
        let Plan::Run { params, .. } = plan(&failed, How::Modify, false, &me) else { panic!() };
        assert!(params.ends_with("scoop.ps1\" install rg"), "{params}");
        let failed_global = Program { scope: Scope::Machine, ..failed.clone() };
        let Plan::Run { params, .. } = plan(&failed_global, How::Modify, true, &me) else { panic!() };
        assert!(params.ends_with("scoop.ps1\" install --global rg"), "{params}");
        let choco = Program { sources: vec![Source::Chocolatey], ..scoop.clone() };
        assert_eq!(plan(&choco, How::Modify, true, &me), Plan::Cannot(NO_MODIFY.into()));
        assert_eq!(plan(&w, How::Modify, false, &me), Plan::Cannot(NO_MODIFY.into()));
        assert_eq!(plan(&p, How::Modify, false, &me), Plan::Cannot(NO_MODIFY.into()));
        assert!(can_modify(&msi) && can_modify(&scoop) && !can_modify(&choco) && !can_modify(&pk));
        let mut closed = msi.clone();
        closed.values = vec![("NoModify".into(), "0x00000001 (1)".into()), ("NoRepair".into(), "1".into())];
        assert!(!can_modify(&closed));
        assert_eq!(plan(&closed, How::Modify, false, &me), Plan::Cannot(NO_MODIFY.into()));
        assert!(Outcome::Ended(3010, String::new()).ok() && Outcome::Ended(3010, String::new()).needs_restart());
        assert!(!Outcome::Ended(2, String::new()).ok());
    }
}
