//! Starting rwall with the session ("Start with Windows"), for the current
//! user only: a value under `HKCU\…\CurrentVersion\Run` on Windows, a file in
//! `~/.config/autostart` on Linux. Both start `rwall --tray`.

use std::path::{Path, PathBuf};

/// The command line that starts this rwall resident.
pub fn command(exe: &Path) -> String {
    format!("\"{}\" --tray", exe.display())
}

fn exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

/// Whether this rwall starts with the session.
pub fn enabled() -> bool {
    exe().is_ok_and(|e| imp::get().is_some_and(|v| same(&v, &e)))
}

/// Whether a stored command starts `exe` (paths compare without case on Windows).
fn same(stored: &str, exe: &Path) -> bool {
    let want = command(exe);
    if cfg!(windows) {
        stored.eq_ignore_ascii_case(&want)
    } else {
        stored == want
    }
}

/// Turns starting with the session on or off.
pub fn set(on: bool) -> Result<(), String> {
    if on {
        imp::put(&command(&exe()?))
    } else {
        imp::remove()
    }
}

#[cfg(windows)]
mod imp {
    use rt_sys::win::{from_wide, wide};
    use windows_sys::Win32::System::Registry::*;

    const KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const NAME: &str = "rwall";

    fn err(rc: u32) -> String {
        std::io::Error::from_raw_os_error(rc as i32).to_string()
    }

    pub fn get() -> Option<String> {
        let (k, n) = (wide(KEY), wide(NAME));
        let mut buf = vec![0u16; 2048];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: `buf` is as large as `size` says.
        let rc = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size) };
        (rc == 0).then(|| from_wide(&buf))
    }

    pub fn put(cmd: &str) -> Result<(), String> {
        let (k, n, v) = (wide(KEY), wide(NAME), wide(cmd));
        // SAFETY: NUL-terminated strings; the size covers `v` with its NUL.
        let rc = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), REG_SZ, v.as_ptr().cast(), (v.len() * 2) as u32) };
        if rc == 0 {
            Ok(())
        } else {
            Err(err(rc))
        }
    }

    pub fn remove() -> Result<(), String> {
        let (k, n) = (wide(KEY), wide(NAME));
        // SAFETY: NUL-terminated strings.
        let rc = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr()) };
        // Not there is as good as removed.
        if rc == 0 || rc == 2 {
            Ok(())
        } else {
            Err(err(rc))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::PathBuf;

    fn file() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("autostart").join("rwall.desktop"))
    }

    pub fn get() -> Option<String> {
        let text = std::fs::read_to_string(file()?).ok()?;
        text.lines().find_map(|l| l.strip_prefix("Exec=")).map(str::to_string)
    }

    pub fn put(cmd: &str) -> Result<(), String> {
        let f = file().ok_or("no configuration folder")?;
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let text = format!("[Desktop Entry]\nType=Application\nName=rwall\nExec={cmd}\nX-GNOME-Autostart-enabled=true\n");
        std::fs::write(&f, text).map_err(|e| e.to_string())
    }

    pub fn remove() -> Result<(), String> {
        match file().map(std::fs::remove_file) {
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turns starting with the session on and off again, for the test
    /// program (run on request: it writes the user's Run key or autostart).
    #[test]
    #[ignore]
    fn on_and_off() {
        set(true).unwrap();
        assert!(enabled());
        set(false).unwrap();
        assert!(!enabled());
        set(false).unwrap();
    }

    #[test]
    fn commands_compare() {
        let exe = Path::new(if cfg!(windows) { r"C:\Tools\rwall.exe" } else { "/opt/rwall" });
        let c = command(exe);
        assert!(c.ends_with("\" --tray"));
        assert!(same(&c, exe));
        assert!(!same(&c, Path::new("other")));
        if cfg!(windows) {
            assert!(same(&c.to_uppercase().replace("--TRAY", "--tray"), exe));
        }
    }
}
