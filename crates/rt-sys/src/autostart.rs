//! Starting an app with the session ("Start with Windows"), for the current
//! user only: a value named after the app under `HKCU\…\CurrentVersion\Run`
//! on Windows, a file `~/.config/autostart/<app>.desktop` on Linux. Both
//! start `<exe> --tray`, resident in the notification area.

use std::path::{Path, PathBuf};

/// The command line that starts this program resident.
pub fn command(exe: &Path) -> String {
    format!("\"{}\" --tray", exe.display())
}

fn exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

/// Whether this program (this very file) starts with the session as `app`.
pub fn enabled(app: &str) -> bool {
    exe().is_ok_and(|e| imp::get(app).is_some_and(|v| same(&v, &e)))
}

/// Whether a stored command starts `exe` (paths compare without case on Windows).
pub fn same(stored: &str, exe: &Path) -> bool {
    let want = command(exe);
    if cfg!(windows) {
        stored.eq_ignore_ascii_case(&want)
    } else {
        stored == want
    }
}

/// Turns starting with the session on or off.
pub fn set(app: &str, on: bool) -> Result<(), String> {
    if on {
        imp::put(app, &command(&exe()?))
    } else {
        imp::remove(app)
    }
}

/// For an app that was called `old` before: if `old` started with the
/// session, the new name does instead (pointing at this program).
pub fn rename(old: &str, new: &str) -> Result<(), String> {
    if imp::get(old).is_none() {
        return Ok(());
    }
    imp::remove(old)?;
    set(new, true)
}

#[cfg(windows)]
mod imp {
    use crate::win::{from_wide, wide};
    use windows_sys::Win32::System::Registry::*;

    const KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

    fn err(rc: u32) -> String {
        std::io::Error::from_raw_os_error(rc as i32).to_string()
    }

    pub fn get(app: &str) -> Option<String> {
        let (k, n) = (wide(KEY), wide(app));
        let mut buf = vec![0u16; 2048];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: `buf` is as large as `size` says.
        let rc = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size) };
        (rc == 0).then(|| from_wide(&buf))
    }

    pub fn put(app: &str, cmd: &str) -> Result<(), String> {
        let (k, n, v) = (wide(KEY), wide(app), wide(cmd));
        // SAFETY: NUL-terminated strings; the size covers `v` with its NUL.
        let rc = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), REG_SZ, v.as_ptr().cast(), (v.len() * 2) as u32) };
        if rc == 0 {
            Ok(())
        } else {
            Err(err(rc))
        }
    }

    pub fn remove(app: &str) -> Result<(), String> {
        let (k, n) = (wide(KEY), wide(app));
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

    fn file(app: &str) -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("autostart").join(format!("{app}.desktop")))
    }

    pub fn get(app: &str) -> Option<String> {
        let text = std::fs::read_to_string(file(app)?).ok()?;
        text.lines().find_map(|l| l.strip_prefix("Exec=")).map(str::to_string)
    }

    pub fn put(app: &str, cmd: &str) -> Result<(), String> {
        let f = file(app).ok_or("no configuration folder")?;
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let text = format!("[Desktop Entry]\nType=Application\nName={app}\nExec={cmd}\nX-GNOME-Autostart-enabled=true\n");
        std::fs::write(&f, text).map_err(|e| e.to_string())
    }

    pub fn remove(app: &str) -> Result<(), String> {
        match file(app).map(std::fs::remove_file) {
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turns starting with the session on and off again, under a name of
    /// its own (run on request: it writes the user's Run key or autostart).
    #[test]
    #[ignore]
    fn on_and_off() {
        let app = "rt-autostart-test";
        set(app, true).unwrap();
        assert!(enabled(app));
        set(app, false).unwrap();
        assert!(!enabled(app));
        set(app, false).unwrap();
        // A new name takes over from the old one.
        let new = "rt-autostart-test-new";
        set(app, true).unwrap();
        rename(app, new).unwrap();
        assert!(!enabled(app) && enabled(new));
        rename(app, new).unwrap();
        assert!(enabled(new));
        set(new, false).unwrap();
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
