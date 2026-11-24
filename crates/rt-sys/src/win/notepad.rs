//! Replacing Windows Notepad with an editor, the way Notepad3 and Notepad++ do: the Image File Execution Options
//! key of `notepad.exe` gets a `Debugger` value, and from then on Windows starts that program instead, with the
//! original command line after it (`"rpad.exe" --from-notepad notepad.exe file.txt`). Whoever starts Notepad
//! (Win+R, a shortcut, another program) gets the editor. It is in HKLM, so writing it needs administrator rights
//! (reading does not); both the 64-bit and the 32-bit view are written. Nothing else of Notepad is touched.

use super::wide;
use std::path::Path;
use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::System::Registry::*;

const SUBKEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\notepad.exe";
const VALUE: &str = "Debugger";

/// The flag that tells the editor it was started in place of Notepad: what follows is Notepad's own command line.
pub const FLAG: &str = "--from-notepad";

/// What Windows is told to run for `notepad.exe`.
pub fn debugger_for(exe: &Path) -> String {
    format!("\"{}\" {FLAG}", exe.display())
}

#[derive(Debug, PartialEq, Eq)]
pub enum State {
    /// `exe` is what runs in place of Notepad.
    Ours,
    /// Something else does (another editor, or `exe` before it was moved): its command.
    Other(String),
    /// Notepad is Notepad.
    Off,
}

/// Who runs in place of Notepad now (readable without administrator rights).
pub fn state(exe: &Path) -> State {
    state_in(HKEY_LOCAL_MACHINE, SUBKEY, exe)
}

/// Makes `exe` run in place of Notepad. Needs administrator rights.
pub fn replace(exe: &Path) -> Result<(), String> {
    write_in(HKEY_LOCAL_MACHINE, SUBKEY, &debugger_for(exe)).map_err(describe)
}

/// Gives Notepad back, if `exe` is what replaces it (another editor's setting is left alone). Needs administrator
/// rights.
pub fn restore(exe: &Path) -> Result<(), String> {
    remove_in(HKEY_LOCAL_MACHINE, SUBKEY, &debugger_for(exe)).map_err(describe)
}

fn describe(code: u32) -> String {
    std::io::Error::from_raw_os_error(code as i32).to_string()
}

fn state_in(root: HKEY, sub: &str, exe: &Path) -> State {
    match read_in(root, sub, RRF_SUBKEY_WOW6464KEY) {
        None => State::Off,
        Some(v) if v.trim().is_empty() => State::Off,
        Some(v) if v.trim().eq_ignore_ascii_case(&debugger_for(exe)) => State::Ours,
        Some(v) => State::Other(v),
    }
}

fn read_in(root: HKEY, sub: &str, view_flag: u32) -> Option<String> {
    let (k, n) = (wide(sub), wide(VALUE));
    let mut size = 0u32;
    // SAFETY: the first call asks for the size, the second fills a buffer of that size.
    unsafe {
        if RegGetValueW(root, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ | view_flag, std::ptr::null_mut(), std::ptr::null_mut(), &mut size) != 0
        {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
        let mut size = (buf.len() * 2) as u32;
        if RegGetValueW(root, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ | view_flag, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size)
            != 0
        {
            return None;
        }
        Some(super::from_wide(&buf))
    }
}

fn write_in(root: HKEY, sub: &str, value: &str) -> Result<(), u32> {
    let (k, n, v) = (wide(sub), wide(VALUE), wide(value));
    for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: NUL-terminated strings; the key is closed below.
        let rc = unsafe {
            RegCreateKeyExW(
                root,
                k.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE | view,
                std::ptr::null(),
                &mut h,
                std::ptr::null_mut(),
            )
        };
        if rc != 0 {
            return Err(rc);
        }
        // SAFETY: `h` is open; the data length includes the NUL.
        let rc = unsafe { RegSetValueExW(h, n.as_ptr(), 0, REG_SZ, v.as_ptr().cast(), (v.len() * 2) as u32) };
        // SAFETY: closes the key opened above.
        unsafe { RegCloseKey(h) };
        if rc != 0 {
            return Err(rc);
        }
    }
    Ok(())
}

/// Deletes the value in both views if it is `only_if`, and the key if that leaves it empty.
fn remove_in(root: HKEY, sub: &str, only_if: &str) -> Result<(), u32> {
    let (k, n) = (wide(sub), wide(VALUE));
    for (view, flag) in [(KEY_WOW64_64KEY, RRF_SUBKEY_WOW6464KEY), (KEY_WOW64_32KEY, RRF_SUBKEY_WOW6432KEY)] {
        match read_in(root, sub, flag) {
            Some(v) if v.trim().eq_ignore_ascii_case(only_if) => {}
            _ => continue,
        }
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: NUL-terminated strings; the key is closed below.
        let rc = unsafe { RegOpenKeyExW(root, k.as_ptr(), 0, KEY_SET_VALUE | KEY_QUERY_VALUE | view, &mut h) };
        if rc == ERROR_FILE_NOT_FOUND {
            continue;
        }
        if rc != 0 {
            return Err(rc);
        }
        // SAFETY: `h` is open.
        let rc = unsafe { RegDeleteValueW(h, n.as_ptr()) };
        let (mut subkeys, mut values) = (0u32, 0u32);
        // SAFETY: `h` is open; the counts are plain outputs.
        let q = unsafe {
            RegQueryInfoKeyW(
                h,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                &mut subkeys,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut values,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        // SAFETY: closes the key opened above.
        unsafe { RegCloseKey(h) };
        if rc != 0 && rc != ERROR_FILE_NOT_FOUND {
            return Err(rc);
        }
        if q == 0 && subkeys == 0 && values == 0 {
            // SAFETY: deletes the key, now empty, in the same view.
            unsafe { RegDeleteKeyExW(root, k.as_ptr(), view, 0) };
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key of our own under HKCU, gone when the test ends.
    struct Scratch(String);

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            Scratch(format!(r"Software\rust-tools-test\notepad-{tag}-{}\notepad.exe", std::process::id()))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let top = self.0.split('\\').take(3).collect::<Vec<_>>().join("\\");
            let k = wide(&top);
            // SAFETY: deletes the scratch key tree under HKCU.
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, k.as_ptr()) };
            let parent = wide(r"Software\rust-tools-test");
            // SAFETY: removes the parent when nothing else is in it.
            unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, parent.as_ptr()) };
        }
    }

    #[test]
    fn the_command_is_quoted_and_flagged() {
        assert_eq!(debugger_for(Path::new(r"C:\Tools\a b\rpad.exe")), r#""C:\Tools\a b\rpad.exe" --from-notepad"#);
    }

    #[test]
    fn replacing_and_giving_notepad_back() {
        let s = Scratch::new("a");
        let exe = Path::new(r"C:\Tools\rpad.exe");
        assert_eq!(state_in(HKEY_CURRENT_USER, &s.0, exe), State::Off);
        write_in(HKEY_CURRENT_USER, &s.0, &debugger_for(exe)).expect("written");
        assert_eq!(state_in(HKEY_CURRENT_USER, &s.0, exe), State::Ours);
        // Another program's setting is not ours to remove.
        let other = Path::new(r"D:\Elsewhere\rpad.exe");
        assert_eq!(state_in(HKEY_CURRENT_USER, &s.0, other), State::Other(debugger_for(exe)));
        remove_in(HKEY_CURRENT_USER, &s.0, &debugger_for(other)).expect("nothing to do");
        assert_eq!(state_in(HKEY_CURRENT_USER, &s.0, exe), State::Ours);
        remove_in(HKEY_CURRENT_USER, &s.0, &debugger_for(exe)).expect("removed");
        assert_eq!(state_in(HKEY_CURRENT_USER, &s.0, exe), State::Off);
        // The key went with the value (and a second removal is fine).
        let k = wide(&s.0);
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: opening a key to see whether it exists.
        let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, k.as_ptr(), 0, KEY_READ, &mut h) };
        assert_ne!(rc, 0, "the empty key is deleted");
        remove_in(HKEY_CURRENT_USER, &s.0, &debugger_for(exe)).expect("again");
    }

    #[test]
    fn other_values_of_the_key_are_kept() {
        let s = Scratch::new("b");
        let exe = Path::new(r"C:\Tools\rpad.exe");
        write_in(HKEY_CURRENT_USER, &s.0, &debugger_for(exe)).expect("written");
        let (k, n, v) = (wide(&s.0), wide("GlobalFlag"), wide("x"));
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: adds a second value to the scratch key, then closes it.
        unsafe {
            assert_eq!(
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    k.as_ptr(),
                    0,
                    std::ptr::null(),
                    0,
                    KEY_SET_VALUE | KEY_WOW64_64KEY,
                    std::ptr::null(),
                    &mut h,
                    std::ptr::null_mut()
                ),
                0
            );
            RegSetValueExW(h, n.as_ptr(), 0, REG_SZ, v.as_ptr().cast(), (v.len() * 2) as u32);
            RegCloseKey(h);
        }
        remove_in(HKEY_CURRENT_USER, &s.0, &debugger_for(exe)).expect("removed");
        assert_eq!(state_in(HKEY_CURRENT_USER, &s.0, exe), State::Off);
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: opening to see that the key is still there.
        let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, k.as_ptr(), 0, KEY_READ | KEY_WOW64_64KEY, &mut h) };
        assert_eq!(rc, 0, "a key with another value stays");
        // SAFETY: closes it.
        unsafe { RegCloseKey(h) };
    }
}
