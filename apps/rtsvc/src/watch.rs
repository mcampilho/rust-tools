//! Watching registry keys: a change in the list of installed programs makes the
//! snapshot of programs dirty, so that it is made again when the computer is idle.

use crate::scm::wide;
use rt_svc::pipe::Event;
use std::sync::Arc;
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, KEY_NOTIFY, REG_NOTIFY_CHANGE_LAST_SET, REG_NOTIFY_CHANGE_NAME,
};
use windows_sys::Win32::System::Threading::WaitForMultipleObjects;

/// A key being watched, closed on drop.
struct Watched {
    key: HKEY,
    event: Event,
}

// SAFETY: a registry key handle can be used from any thread.
unsafe impl Send for Watched {}

impl Watched {
    fn open(root: HKEY, path: &str) -> Option<Watched> {
        let mut key: HKEY = std::ptr::null_mut();
        let p = wide(path);
        // SAFETY: valid string and out pointer.
        if unsafe { RegOpenKeyExW(root, p.as_ptr(), 0, KEY_NOTIFY, &mut key) } != ERROR_SUCCESS {
            return None;
        }
        let w = Watched { key, event: Event::new().ok()? };
        w.arm().then_some(w)
    }

    /// Asks Windows to signal the event at the next change of the key or what is below it.
    fn arm(&self) -> bool {
        // SAFETY: the key and the event are ours and valid.
        unsafe {
            RegNotifyChangeKeyValue(self.key, 1, REG_NOTIFY_CHANGE_NAME | REG_NOTIFY_CHANGE_LAST_SET, self.event.0, 1) == ERROR_SUCCESS
        }
    }
}

impl Drop for Watched {
    fn drop(&mut self) {
        // SAFETY: the key is ours.
        unsafe { RegCloseKey(self.key) };
    }
}

/// Calls `changed` whenever one of `paths` (under `root`) changes, until `stop` is set.
/// Keys that cannot be opened are left out. Returns how many are watched.
pub fn watch(root: HKEY, paths: &[&str], stop: Arc<Event>, changed: impl Fn() + Send + 'static) -> usize {
    let watched: Vec<Watched> = paths.iter().filter_map(|p| Watched::open(root, p)).collect();
    let n = watched.len();
    if n == 0 {
        return 0;
    }
    std::thread::spawn(move || {
        // The events, then the stop event last.
        let handles: Vec<HANDLE> = watched.iter().map(|w| w.event.0).chain(std::iter::once(stop.0)).collect();
        loop {
            // SAFETY: the handles are valid events and the count matches.
            let r = unsafe { WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, u32::MAX) };
            let i = r.wrapping_sub(WAIT_OBJECT_0) as usize;
            if i >= watched.len() {
                return;
            }
            changed();
            // The event stays set until it is reset; then the key is watched again.
            // SAFETY: valid event.
            unsafe { windows_sys::Win32::System::Threading::ResetEvent(watched[i].event.0) };
            if !watched[i].arm() {
                return;
            }
        }
    });
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows_sys::Win32::System::Registry::{RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD};

    #[test]
    fn a_change_under_a_key_is_noticed() {
        let base = format!("Software\\rtsvc-test-{}", std::process::id());
        let p = wide(&base);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: valid strings and out pointers; the key is closed below.
        unsafe {
            assert_eq!(
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    p.as_ptr(),
                    0,
                    std::ptr::null(),
                    0,
                    KEY_WRITE,
                    std::ptr::null(),
                    &mut key,
                    std::ptr::null_mut()
                ),
                ERROR_SUCCESS
            );
        }
        let stop = Arc::new(Event::new().unwrap());
        let count = Arc::new(AtomicU32::new(0));
        let c = count.clone();
        assert_eq!(
            watch(HKEY_CURRENT_USER, &[base.as_str(), "Software\\rtsvc-no-such-key"], stop.clone(), move || {
                c.fetch_add(1, Ordering::SeqCst);
            }),
            1
        );
        let name = wide("Value");
        for round in 1..=2u32 {
            // SAFETY: valid key, name and a 4-byte value.
            unsafe { RegSetValueExW(key, name.as_ptr(), 0, REG_DWORD, round.to_le_bytes().as_ptr(), 4) };
            for _ in 0..100 {
                if count.load(Ordering::SeqCst) >= round {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            assert!(count.load(Ordering::SeqCst) >= round, "change {round} not noticed");
        }
        stop.set();
        // SAFETY: the key and the tree are ours.
        unsafe {
            RegCloseKey(key);
            RegDeleteTreeW(HKEY_CURRENT_USER, p.as_ptr());
        }
    }
}
