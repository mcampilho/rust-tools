//! What the jobs really do: clean the targets of the cleaner's catalogue that
//! need administrator rights, and uninstall programs of the machine by their
//! Windows Installer product code. Nothing else, and never from a path or a
//! command of the client.

use crate::jobs::Executor;
use rt_clean::targets::{all, Env, Kind, Target};
use rt_inst::inventory::{self, Scope};
use rt_svc::proto::JobKind;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// How long the list of programs that may be uninstalled is trusted.
const FRESH: Duration = Duration::from_secs(60);

/// `{8-4-4-4-12}` hex digits: a Windows Installer product code.
pub fn is_product_code(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 38
        && b[0] == b'{'
        && b[37] == b'}'
        && b[1..37].iter().enumerate().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { *c == b'-' } else { c.is_ascii_hexdigit() })
}

/// The targets a job may clean: of the system (need rights), plain files or the Recycle Bin.
pub fn cleanable(t: &Target) -> bool {
    t.admin && matches!(t.kind, Kind::Files | Kind::RecycleBin)
}

pub struct Real {
    backups: PathBuf,
    /// The product codes of the programs of the machine, and when they were read.
    codes: Mutex<Option<(Instant, Vec<String>)>>,
}

impl Real {
    pub fn new(backups: PathBuf) -> Real {
        Real { backups, codes: Mutex::new(None) }
    }

    fn machine_codes(&self) -> Vec<String> {
        let mut g = self.codes.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, list)) = g.as_ref() {
            if at.elapsed() < FRESH {
                return list.clone();
            }
        }
        let list: Vec<String> = inventory::read()
            .programs
            .into_iter()
            .filter(|p| p.scope == Scope::Machine && !p.no_remove)
            .filter_map(|p| p.msi.map(|c| c.to_uppercase()))
            .collect();
        *g = Some((Instant::now(), list.clone()));
        list
    }
}

impl Executor for Real {
    fn check(&self, kind: JobKind, item: &str) -> Result<(), String> {
        match kind {
            JobKind::Clean => all(&Env::here())
                .iter()
                .any(|t| t.id == item && cleanable(t))
                .then_some(())
                .ok_or_else(|| "it is not a target the service cleans".to_string()),
            JobKind::UninstallMsi => {
                if !is_product_code(item) {
                    return Err("it is not a product code".into());
                }
                self.machine_codes()
                    .contains(&item.to_uppercase())
                    .then_some(())
                    .ok_or_else(|| "it is not a program of this machine that may be removed".to_string())
            }
        }
    }

    fn run(&self, kind: JobKind, item: &str, cancel: &AtomicBool) -> Result<String, String> {
        match kind {
            JobKind::Clean => {
                let t =
                    all(&Env::here()).into_iter().find(|t| t.id == item && cleanable(t)).ok_or("it is not a target the service cleans")?;
                let found = rt_clean::clean::analyse(&t, cancel);
                // As SYSTEM, in folders others may write to: plain files only where they were listed.
                let done = match t.kind {
                    Kind::Files => rt_clean::clean::clean_in_place(&found, cancel),
                    _ => rt_clean::clean::clean(&t, &found, cancel, &self.backups),
                };
                match done.error {
                    Some(e) => Err(e),
                    None => Ok(format!("freed {} bytes in {} files, {} stayed", done.bytes, done.files, done.failed)),
                }
            }
            JobKind::UninstallMsi => {
                let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
                let msiexec = PathBuf::from(root).join("System32").join("msiexec.exe");
                let status = std::process::Command::new(msiexec)
                    .args(["/x", item, "/qn", "/norestart"])
                    .creation_flags(CREATE_NO_WINDOW)
                    .status()
                    .map_err(|e| e.to_string())?;
                // 0 done, 1605 not installed any more, 1641 and 3010 done and a restart is needed.
                match status.code() {
                    Some(0 | 1605) => Ok("removed".into()),
                    Some(1641 | 3010) => Ok("removed; a restart is needed".into()),
                    Some(c) => Err(format!("the uninstaller ended with code {c}")),
                    None => Err("the uninstaller was stopped".into()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_codes_have_a_fixed_shape() {
        assert!(is_product_code("{AC76BA86-7AD7-1033-7B44-A90000000001}"));
        assert!(is_product_code("{ac76ba86-7ad7-1033-7b44-a90000000001}"));
        for bad in [
            "",
            "AC76BA86-7AD7-1033-7B44-A90000000001",
            "{AC76BA86-7AD7-1033-7B44-A9000000000}",
            "{AC76BA86-7AD7-1033-7B44-A9000000000G}",
            "{AC76BA86_7AD7-1033-7B44-A90000000001}",
            "{AC76BA86-7AD7-1033-7B44-A90000000001} /x",
        ] {
            assert!(!is_product_code(bad), "{bad}");
        }
    }

    #[test]
    fn only_system_targets_with_plain_files_may_be_cleaned() {
        let ts = all(&Env::here());
        let ok: Vec<&Target> = ts.iter().filter(|t| cleanable(t)).collect();
        assert!(!ok.is_empty());
        assert!(ok.iter().all(|t| t.admin && matches!(t.kind, Kind::Files | Kind::RecycleBin)));
        // Commands, shortcuts and registry entries are never done by the service.
        assert!(ts.iter().filter(|t| matches!(t.kind, Kind::Command | Kind::Shortcuts | Kind::Registry)).all(|t| !cleanable(t)));
    }

    #[test]
    fn what_is_not_a_known_item_is_refused_before_anything_runs() {
        let r = Real::new(PathBuf::from("backups"));
        assert!(r.check(JobKind::Clean, "no.such.target").is_err());
        assert!(r.check(JobKind::Clean, "C:\\Windows").is_err(), "a path is not an id");
        assert!(r.check(JobKind::UninstallMsi, "msiexec /x something").is_err());
        assert!(r.check(JobKind::UninstallMsi, "{00000000-0000-0000-0000-000000000000}").is_err(), "not a program of this machine");
    }
}
