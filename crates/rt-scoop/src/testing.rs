//! Fake installations for the tests, under `target\` (never in TEMP, because
//! of the antivirus).

use crate::install::{Env, Scoop};
use std::path::PathBuf;

pub struct Tree(PathBuf);

impl Tree {
    pub fn new(name: &str) -> Tree {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace")
            .join("target")
            .join("rt-scoop-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Tree(dir)
    }

    fn path(&self, relative: &str) -> PathBuf {
        relative.split('/').fold(self.0.clone(), |p, part| p.join(part))
    }

    pub fn dir(&self, relative: &str) -> PathBuf {
        let p = self.path(relative);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    pub fn file(&self, relative: &str, text: &str) -> PathBuf {
        let p = self.path(relative);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    pub fn manifest(&self, bucket: &str, app: &str, text: &str) {
        self.file(&format!("root/buckets/{bucket}/bucket/{app}.json"), text);
    }

    /// An installed app. `current` is a plain folder with the same files: the
    /// code only reads through it.
    pub fn app(&self, name: &str, version: &str, manifest: &str, info: &str) {
        for folder in [version, "current"] {
            self.file(&format!("root/apps/{name}/{folder}/scoop-manifest.json"), manifest);
            self.file(&format!("root/apps/{name}/{folder}/scoop-install.json"), info);
        }
    }

    /// The installation in `root`, with no configuration.
    pub fn scoop(&self) -> Scoop {
        let env = Env::new([("USERPROFILE".to_string(), self.dir("home").display().to_string())]);
        Scoop::locate(&env, Some(&self.dir("root").display().to_string()), None)
    }
}

/// A junction, as Scoop makes for `current` and persisted folders.
#[cfg(windows)]
pub fn junction(link: &std::path::Path, target: &std::path::Path) {
    let made = std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(link).arg(target).output().unwrap();
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
}
