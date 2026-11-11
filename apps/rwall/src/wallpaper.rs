//! Puts a picture on the desktop.
//!
//! Windows: the picture position goes to `HKCU\Control Panel\Desktop`
//! (`WallpaperStyle`, `TileWallpaper`) and `SystemParametersInfoW(
//! SPI_SETDESKWALLPAPER)` sets the file, saves it in the user's profile and
//! tells Explorer. The same picture goes on every screen.
//!
//! Linux: GNOME and its relatives through `gsettings`, KDE Plasma through
//! `plasma-apply-wallpaperimage`, XFCE through `xfconf-query`, and other
//! window managers through `feh`.

use crate::settings::Fit;
use std::path::Path;

/// Sets `file` as the wallpaper.
pub fn set(file: &Path, fit: Fit) -> Result<(), String> {
    let file = std::path::absolute(file).map_err(|e| e.to_string())?;
    imp::set(&file, fit)
}

#[cfg(windows)]
mod imp {
    use super::Fit;
    use rt_sys::win::wide;
    use std::path::Path;

    /// `WallpaperStyle` of each position.
    pub fn style(fit: Fit) -> &'static str {
        match fit {
            Fit::Fill => "10",
            Fit::Fit => "6",
            Fit::Stretch => "2",
            Fit::Center => "0",
            Fit::Span => "22",
        }
    }

    fn set_value(name: &str, value: &str) -> Result<(), String> {
        use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
        let (key, name, data) = (wide("Control Panel\\Desktop"), wide(name), wide(value));
        // SAFETY: NUL-terminated strings; the size covers `data` with its NUL.
        let rc = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32) };
        if rc == 0 {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(rc as i32).to_string())
        }
    }

    pub fn set(file: &Path, fit: Fit) -> Result<(), String> {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_SETDESKWALLPAPER};
        set_value("WallpaperStyle", style(fit))?;
        set_value("TileWallpaper", "0")?;
        let path = wide(&file.to_string_lossy());
        // SAFETY: a NUL-terminated path that outlives the call.
        let ok = unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, path.as_ptr() as *mut _, SPIF_UPDATEINIFILE | SPIF_SENDCHANGE) } != 0;
        if ok {
            Ok(())
        } else {
            Err(format!("{}: {}", file.display(), std::io::Error::last_os_error()))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Fit;
    use std::path::Path;
    use std::process::{Command, Stdio};

    fn run(cmd: &str, args: &[&str]) -> bool {
        Command::new(cmd).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    }

    fn desktop() -> String {
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase()
    }

    pub fn set(file: &Path, fit: Fit) -> Result<(), String> {
        let path = file.to_string_lossy().to_string();
        let uri = format!("file://{path}");
        let d = desktop();
        if d.contains("kde") && run("plasma-apply-wallpaperimage", &[&path]) {
            return Ok(());
        }
        if d.contains("xfce") {
            // Every monitor and workspace that has a picture set.
            let out = Command::new("xfconf-query").args(["-c", "xfce4-desktop", "-l"]).output().ok();
            let props: Vec<String> = out
                .map(|o| String::from_utf8_lossy(&o.stdout).lines().filter(|l| l.ends_with("/last-image")).map(str::to_string).collect())
                .unwrap_or_default();
            if !props.is_empty() && props.iter().all(|p| run("xfconf-query", &["-c", "xfce4-desktop", "-p", p, "-s", &path])) {
                return Ok(());
            }
        }
        let options = match fit {
            Fit::Fill => "zoom",
            Fit::Fit => "scaled",
            Fit::Stretch => "stretched",
            Fit::Center => "centered",
            Fit::Span => "spanned",
        };
        let schema = if d.contains("cinnamon") { "org.cinnamon.desktop.background" } else { "org.gnome.desktop.background" };
        if run("gsettings", &["set", schema, "picture-uri", &uri]) {
            // GNOME 42+ has a second picture for the dark style.
            let _ = run("gsettings", &["set", schema, "picture-uri-dark", &uri]);
            let _ = run("gsettings", &["set", schema, "picture-options", options]);
            return Ok(());
        }
        let feh = match fit {
            Fit::Fill => "--bg-fill",
            Fit::Fit => "--bg-max",
            Fit::Stretch => "--bg-scale",
            Fit::Center | Fit::Span => "--bg-center",
        };
        if run("feh", &[feh, &path]) {
            return Ok(());
        }
        Err("no way to set the wallpaper on this desktop (gsettings, plasma-apply-wallpaperimage, xfconf-query or feh)".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn styles_differ() {
        use crate::settings::Fit;
        let s: std::collections::HashSet<_> = Fit::ALL.iter().map(|f| super::imp::style(*f)).collect();
        assert_eq!(s.len(), Fit::ALL.len());
    }
}
