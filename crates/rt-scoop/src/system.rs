//! What Scoop changes outside its own folders: the environment variables in
//! the registry, the shortcuts of the Start menu, and the processes it looks
//! at. [`WinSystem`] does it on Windows; [`FakeSystem`] keeps it in memory for
//! the tests.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A running program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    pub id: u32,
    pub name: String,
    pub path: PathBuf,
}

pub trait System {
    /// `Get-EnvVar`: the value as written (not expanded), and whether it is
    /// `REG_EXPAND_SZ`. User variables, or the machine's with `global`.
    fn env_get(&self, name: &str, global: bool) -> Option<(String, bool)>;
    /// Writes a variable (`expand`: as `REG_EXPAND_SZ`); None removes it.
    fn env_put(&self, name: &str, global: bool, value: Option<(&str, bool)>) -> Result<(), String>;
    /// Tells the running programs that the environment changed
    /// (`WM_SETTINGCHANGE`).
    fn env_changed(&self);
    /// The `Start Menu` folder of the user, or of all users.
    fn start_menu(&self, global: bool) -> Option<PathBuf>;
    /// Writes a `.lnk`, as `WScript.Shell` does.
    fn shortcut(&self, link: &Path, target: &Path, working_dir: &Path, arguments: &str, icon: Option<&Path>) -> Result<(), String>;
    fn processes(&self) -> Vec<Process>;
}

/// The variables, shortcuts and processes of a made-up machine.
#[derive(Default)]
pub struct FakeSystem {
    /// (global, name in capitals) -> (value, expand)
    pub vars: RefCell<BTreeMap<(bool, String), (String, bool)>>,
    pub menu: PathBuf,
    pub running: Vec<Process>,
    /// How many times the change was announced.
    pub announced: RefCell<usize>,
}

impl FakeSystem {
    pub fn new(menu: PathBuf) -> FakeSystem {
        FakeSystem { menu, ..FakeSystem::default() }
    }

    pub fn var(&self, name: &str, global: bool) -> Option<String> {
        self.env_get(name, global).map(|(v, _)| v)
    }
}

impl System for FakeSystem {
    fn env_get(&self, name: &str, global: bool) -> Option<(String, bool)> {
        self.vars.borrow().get(&(global, name.to_ascii_uppercase())).cloned()
    }

    fn env_put(&self, name: &str, global: bool, value: Option<(&str, bool)>) -> Result<(), String> {
        let key = (global, name.to_ascii_uppercase());
        match value {
            Some((v, expand)) => self.vars.borrow_mut().insert(key, (v.to_string(), expand)),
            None => self.vars.borrow_mut().remove(&key),
        };
        Ok(())
    }

    fn env_changed(&self) {
        *self.announced.borrow_mut() += 1;
    }

    fn start_menu(&self, global: bool) -> Option<PathBuf> {
        Some(self.menu.join(if global { "common" } else { "user" }))
    }

    /// The shortcut is a text file with its parts on separate lines.
    fn shortcut(&self, link: &Path, target: &Path, working_dir: &Path, arguments: &str, icon: Option<&Path>) -> Result<(), String> {
        let icon = icon.map(|i| i.display().to_string()).unwrap_or_default();
        let text = format!("{}\n{}\n{arguments}\n{icon}\n", target.display(), working_dir.display());
        std::fs::write(link, text).map_err(|e| e.to_string())
    }

    fn processes(&self) -> Vec<Process> {
        self.running.clone()
    }
}

/// The real machine.
pub struct WinSystem;

#[cfg(windows)]
mod win {
    use super::Process;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use windows_sys::core::GUID;
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Registry::*;

    pub fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(Some(0)).collect()
    }

    fn text(s: &str) -> Vec<u16> {
        wide(std::ffi::OsStr::new(s))
    }

    fn key(global: bool) -> (HKEY, Vec<u16>) {
        if global {
            (HKEY_LOCAL_MACHINE, text(r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"))
        } else {
            (HKEY_CURRENT_USER, text("Environment"))
        }
    }

    pub fn env_get(name: &str, global: bool) -> Option<(String, bool)> {
        let (hive, path) = key(global);
        let name = text(name);
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
        let mut kind = 0u32;
        let mut size = 0u32;
        // SAFETY: NUL-terminated strings; the first call only asks the size.
        unsafe {
            if RegGetValueW(hive, path.as_ptr(), name.as_ptr(), flags, &mut kind, std::ptr::null_mut(), &mut size) != ERROR_SUCCESS {
                return None;
            }
            let mut buf = vec![0u16; size as usize / 2 + 1];
            size = (buf.len() * 2) as u32;
            if RegGetValueW(hive, path.as_ptr(), name.as_ptr(), flags, &mut kind, buf.as_mut_ptr().cast(), &mut size) != ERROR_SUCCESS {
                return None;
            }
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some((String::from_utf16_lossy(&buf[..len]), kind == REG_EXPAND_SZ))
        }
    }

    pub fn env_put(name: &str, global: bool, value: Option<(&str, bool)>) -> Result<(), String> {
        let (hive, path) = key(global);
        let name = text(name);
        let mut handle: HKEY = std::ptr::null_mut();
        // SAFETY: NUL-terminated strings; the key is closed below.
        unsafe {
            let status = RegOpenKeyExW(hive, path.as_ptr(), 0, KEY_SET_VALUE, &mut handle);
            if status != ERROR_SUCCESS {
                return Err(std::io::Error::from_raw_os_error(status as i32).to_string());
            }
            let status = match value {
                Some((v, expand)) => {
                    let data = text(v);
                    let kind = if expand { REG_EXPAND_SZ } else { REG_SZ };
                    RegSetValueExW(handle, name.as_ptr(), 0, kind, data.as_ptr().cast(), (data.len() * 2) as u32)
                }
                None => match RegDeleteValueW(handle, name.as_ptr()) {
                    ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                    other => other,
                },
            };
            RegCloseKey(handle);
            if status != ERROR_SUCCESS {
                return Err(std::io::Error::from_raw_os_error(status as i32).to_string());
            }
        }
        Ok(())
    }

    pub fn env_changed() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
        let what = text("Environment");
        let mut result = 0usize;
        // SAFETY: `what` outlives the call.
        unsafe {
            SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0, what.as_ptr() as isize, SMTO_ABORTIFHUNG, 5000, &mut result);
        }
    }

    pub fn start_menu(global: bool) -> Option<PathBuf> {
        use windows_sys::Win32::System::Com::CoTaskMemFree;
        use windows_sys::Win32::UI::Shell::{FOLDERID_CommonStartMenu, FOLDERID_StartMenu, SHGetKnownFolderPath};
        let id = if global { &FOLDERID_CommonStartMenu } else { &FOLDERID_StartMenu };
        let mut out: windows_sys::core::PWSTR = std::ptr::null_mut();
        // SAFETY: the string Windows gives is freed with CoTaskMemFree.
        unsafe {
            let ok = SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut out) == 0;
            let path = ok.then(|| {
                let len = (0..).take_while(|&i| *out.add(i) != 0).count();
                PathBuf::from(String::from_utf16_lossy(std::slice::from_raw_parts(out, len)))
            });
            CoTaskMemFree(out.cast());
            path
        }
    }

    const CLSID_SHELL_LINK: GUID = GUID::from_u128(0x00021401_0000_0000_c000_000000000046);
    const IID_ISHELL_LINK_W: GUID = GUID::from_u128(0x000214f9_0000_0000_c000_000000000046);
    const IID_IPERSIST_FILE: GUID = GUID::from_u128(0x0000010b_0000_0000_c000_000000000046);

    type Method = unsafe extern "system" fn(*mut c_void, *const u16) -> i32;
    type QueryInterface = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32;
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;
    type SetIcon = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32;
    type Save = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32;

    /// The method at `index` of the COM object `this`.
    unsafe fn method(this: *mut c_void, index: usize) -> *const c_void {
        let table = *(this as *const *const *const c_void);
        *table.add(index)
    }

    /// `IShellLinkW` and `IPersistFile`, as `WScript.Shell`'s `CreateShortcut`
    /// and `Save` use them.
    pub fn shortcut(link: &Path, target: &Path, working_dir: &Path, arguments: &str, icon: Option<&Path>) -> Result<(), String> {
        use windows_sys::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
        let fail = |what: &str, hr: i32| Err(format!("{what}: 0x{:08X}", hr as u32));
        // SAFETY: the vtable slots are those of IShellLinkW (SetWorkingDirectory
        // 9, SetArguments 11, SetIconLocation 17, SetPath 20) and IPersistFile
        // (Save 6); every string is NUL-terminated and outlives its call, and
        // both objects are released.
        unsafe {
            CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
            let mut shell: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(&CLSID_SHELL_LINK, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_ISHELL_LINK_W, &mut shell);
            if hr < 0 {
                return fail("ShellLink", hr);
            }
            let release = |this: *mut c_void| std::mem::transmute::<*const c_void, Release>(method(this, 2))(this);
            let call = |index: usize, value: &std::ffi::OsStr| {
                std::mem::transmute::<*const c_void, Method>(method(shell, index))(shell, wide(value).as_ptr())
            };
            let mut hr = call(20, target.as_os_str());
            if hr >= 0 {
                hr = call(9, working_dir.as_os_str());
            }
            if hr >= 0 && !arguments.is_empty() {
                hr = call(11, std::ffi::OsStr::new(arguments));
            }
            if let (true, Some(icon)) = (hr >= 0, icon) {
                let set: SetIcon = std::mem::transmute(method(shell, 17));
                hr = set(shell, wide(icon.as_os_str()).as_ptr(), 0);
            }
            if hr >= 0 {
                let query: QueryInterface = std::mem::transmute(method(shell, 0));
                let mut file: *mut c_void = std::ptr::null_mut();
                hr = query(shell, &IID_IPERSIST_FILE, &mut file);
                if hr >= 0 {
                    let save: Save = std::mem::transmute(method(file, 6));
                    hr = save(file, wide(link.as_os_str()).as_ptr(), 1);
                    release(file);
                }
            }
            release(shell);
            if hr < 0 {
                return fail(&link.display().to_string(), hr);
            }
        }
        Ok(())
    }

    pub fn processes() -> Vec<Process> {
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
        };
        use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
        let mut found = Vec::new();
        // SAFETY: the snapshot and every process handle are closed; the
        // buffers are passed with their sizes.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return found;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                if !process.is_null() {
                    let mut buf = vec![0u16; 32768];
                    let mut len = buf.len() as u32;
                    if QueryFullProcessImageNameW(process, 0, buf.as_mut_ptr(), &mut len) != 0 {
                        let name_len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                        let name = String::from_utf16_lossy(&entry.szExeFile[..name_len]);
                        let path = PathBuf::from(String::from_utf16_lossy(&buf[..len as usize]));
                        found.push(Process { id: entry.th32ProcessID, name, path });
                    }
                    CloseHandle(process);
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
        found
    }
}

#[cfg(windows)]
impl System for WinSystem {
    fn env_get(&self, name: &str, global: bool) -> Option<(String, bool)> {
        win::env_get(name, global)
    }

    fn env_put(&self, name: &str, global: bool, value: Option<(&str, bool)>) -> Result<(), String> {
        win::env_put(name, global, value)
    }

    fn env_changed(&self) {
        win::env_changed()
    }

    fn start_menu(&self, global: bool) -> Option<PathBuf> {
        win::start_menu(global)
    }

    fn shortcut(&self, link: &Path, target: &Path, working_dir: &Path, arguments: &str, icon: Option<&Path>) -> Result<(), String> {
        win::shortcut(link, target, working_dir, arguments, icon)
    }

    fn processes(&self) -> Vec<Process> {
        win::processes()
    }
}

/// Scoop is Windows only: elsewhere nothing is there and nothing is written.
#[cfg(not(windows))]
impl System for WinSystem {
    fn env_get(&self, _name: &str, _global: bool) -> Option<(String, bool)> {
        None
    }

    fn env_put(&self, _name: &str, _global: bool, _value: Option<(&str, bool)>) -> Result<(), String> {
        Err("Windows only".into())
    }

    fn env_changed(&self) {}

    fn start_menu(&self, _global: bool) -> Option<PathBuf> {
        None
    }

    fn shortcut(&self, _link: &Path, _target: &Path, _working_dir: &Path, _arguments: &str, _icon: Option<&Path>) -> Result<(), String> {
        Err("Windows only".into())
    }

    fn processes(&self) -> Vec<Process> {
        Vec::new()
    }
}

/// `test_running_process`: the programs running from the folder of an app.
pub fn running_from(system: &dyn System, app_dir: &Path) -> Vec<Process> {
    let dir = crate::disk::plain(&std::path::absolute(app_dir).unwrap_or_else(|_| app_dir.to_path_buf()));
    let prefix = format!("{}\\", dir.display().to_string().trim_end_matches('\\')).to_lowercase();
    system.processes().into_iter().filter(|p| p.path.display().to_string().to_lowercase().starts_with(&prefix)).collect()
}
