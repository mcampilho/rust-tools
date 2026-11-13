//! Which programs are running, by the names of their processes: a browser
//! that is open holds some of its cache, and says so before cleaning.

use std::collections::HashSet;

/// The names of the processes running, in lower case (`chrome.exe` on
/// Windows, `firefox` on Linux).
pub fn running() -> HashSet<String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
        let mut out = HashSet::new();
        // SAFETY: a snapshot of the processes, closed below.
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        // SAFETY: PROCESSENTRY32W is plain data, its size set as required.
        let mut e: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        // SAFETY: walks the snapshot with a valid entry.
        let mut ok = unsafe { Process32FirstW(snap, &mut e) } != 0;
        while ok {
            let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            out.insert(String::from_utf16_lossy(&e.szExeFile[..n]).to_lowercase());
            // SAFETY: as above.
            ok = unsafe { Process32NextW(snap, &mut e) } != 0;
        }
        // SAFETY: closes the snapshot.
        unsafe { CloseHandle(snap) };
        out
    }
    #[cfg(not(windows))]
    {
        std::fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()))
            .filter_map(|e| std::fs::read_to_string(e.path().join("comm")).ok())
            .map(|c| c.trim().to_lowercase())
            .collect()
    }
}

/// Whether one of `names` is running (Linux cuts names at 15 characters).
pub fn any_running(running: &HashSet<String>, names: &[&str]) -> bool {
    names.iter().any(|n| {
        let n = n.to_lowercase();
        running.contains(&n) || n.len() > 15 && running.contains(&n[..15])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_running() {
        let r = running();
        assert!(!r.is_empty());
        let me = std::env::current_exe().unwrap();
        let name = me.file_name().unwrap().to_string_lossy().to_lowercase();
        assert!(any_running(&r, &[&name]), "{name} not in {} processes", r.len());
        assert!(!any_running(&r, &["no-such-program.exe"]));
    }
}
