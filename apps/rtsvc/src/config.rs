//! Where the service keeps its data, its settings and its log.

use rt_svc::server::Log;
use rt_svc::Visibility;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, ACL, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION,
};

/// `%ProgramData%\rust-tools`: SYSTEM and Administrators write, users read.
pub fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("ProgramData").unwrap_or_else(|| r"C:\ProgramData".into())).join("rust-tools")
}

fn config_file() -> PathBuf {
    data_dir().join("service.cfg")
}

/// The settings of the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub visibility: Visibility,
}

impl Default for Config {
    fn default() -> Config {
        Config { visibility: Visibility::User }
    }
}

impl Config {
    /// `key = value` lines; what is unknown or wrong is left at its default.
    pub fn parse(text: &str) -> Config {
        let mut c = Config::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            if k.trim() == "visibility" {
                c.visibility = match v.trim() {
                    "elevated" => Visibility::Elevated,
                    _ => Visibility::User,
                };
            }
        }
        c
    }

    pub fn text(&self) -> String {
        format!("visibility = {}\n", if self.visibility == Visibility::Elevated { "elevated" } else { "user" })
    }

    pub fn load() -> Config {
        std::fs::read_to_string(config_file()).map(|t| Config::parse(&t)).unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir())?;
        let tmp = data_dir().join("service.cfg.tmp");
        std::fs::write(&tmp, self.text())?;
        std::fs::rename(tmp, config_file())
    }
}

/// Who may do what in `%ProgramData%\rust-tools`: SYSTEM and Administrators everything,
/// authenticated users read and execute. Inheritance from the parent is cut.
pub const DATA_SDDL: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;AU)";
/// For what only administrators may even read (the snapshots hold other users' data).
pub const PRIVATE_SDDL: &str = "O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

/// Creates `dir` if need be, gives it to Administrators and gives it exactly the permissions of `sddl` (no inheritance).
pub fn protect(dir: &Path, sddl: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = crate::scm::wide(sddl);
    let mut sd = std::ptr::null_mut();
    // SAFETY: valid string and out pointer.
    if unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), SDDL_REVISION_1, &mut sd, std::ptr::null_mut()) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let (mut present, mut defaulted) = (0, 0);
    let mut dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `sd` is the descriptor just made; the out pointers are valid.
    let ok = unsafe { GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) };
    let mut result = if ok == 0 || present == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) };
    let mut owner = std::ptr::null_mut();
    // SAFETY: `sd` is the descriptor just made; the out pointers are valid.
    if result.is_ok() && unsafe { GetSecurityDescriptorOwner(sd, &mut owner, &mut defaulted) } == 0 {
        result = Err(std::io::Error::last_os_error());
    }
    if result.is_ok() {
        let path = crate::scm::wide(&dir.to_string_lossy());
        // SAFETY: valid path, owner and DACL (inside `sd`, alive until the LocalFree).
        let r = unsafe {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                owner,
                std::ptr::null_mut(),
                dacl,
                std::ptr::null_mut(),
            )
        };
        if r != 0 {
            result = Err(std::io::Error::from_raw_os_error(r as i32));
        }
    }
    // SAFETY: allocated by the conversion.
    unsafe { LocalFree(sd as *mut _) };
    result
}

/// `YYYY-MM-DD HH:MM:SS` (UTC) of a time in seconds since 1970.
pub fn stamp(secs: u64) -> String {
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The log file, written a line at a time and kept to about a megabyte: when
/// it is full it becomes `.1` (the one before is lost).
pub fn file_log(dir: &Path) -> Log {
    const FULL: u64 = 1 << 20;
    let dir = dir.to_path_buf();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("rtsvc.log");
    let lock = Mutex::new(());
    Arc::new(move |line: &str| {
        let _g = lock.lock();
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > FULL) {
            let _ = std::fs::rename(&path, dir.join("rtsvc.log.1"));
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(f, "{} {line}", stamp(now()));
        }
    })
}

/// The log on the error output, for `console`.
pub fn console_log() -> Log {
    Arc::new(|line: &str| eprintln!("{} {line}", stamp(now())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_read_leniently() {
        assert_eq!(Config::parse("").visibility, Visibility::User);
        assert_eq!(Config::parse("visibility = elevated\n").visibility, Visibility::Elevated);
        assert_eq!(Config::parse("visibility=nonsense").visibility, Visibility::User);
        assert_eq!(Config::parse("# x\nother = 1\nvisibility = elevated").visibility, Visibility::Elevated);
        let c = Config { visibility: Visibility::Elevated };
        assert_eq!(Config::parse(&c.text()), c);
    }

    #[test]
    fn the_time_is_written_in_utc() {
        assert_eq!(stamp(0), "1970-01-01 00:00:00");
        assert_eq!(stamp(951_782_400 + 3661), "2000-02-29 01:01:01");
        assert_eq!(stamp(1_784_000_000), "2026-07-14 03:33:20");
    }
}
