//! The git Scoop uses (`Get-HelperPath -Helper Git`, `Invoke-Git`).

use super::Ctx;
use rt_scoop::resolve::helper;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone)]
pub struct Git {
    exe: PathBuf,
    proxy: Option<String>,
}

pub struct Answer {
    pub ok: bool,
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Git {
    /// Scoop's own git, else the one on `PATH`.
    pub fn find(ctx: &Ctx) -> Option<Git> {
        let own = [r"mingw64\bin\git.exe", r"mingw32\bin\git.exe"].iter().find_map(|f| helper(&ctx.scoop, "git", f));
        let names: &[&str] = if cfg!(windows) { &["git.exe"] } else { &["git"] };
        let exe = own.or_else(|| ctx.env.path().flat_map(|d| names.iter().map(move |n| d.join(n))).find(|p| p.is_file()))?;
        let proxy = Some(ctx.scoop.setting("proxy").str()).filter(|p| !p.is_empty() && *p != "none").map(|p| {
            match p.strip_prefix("currentuser@") {
                Some(rest) => format!(":@{rest}"),
                None => p.to_string(),
            }
        });
        Some(Git { exe, proxy })
    }

    /// `git [-C dir] args`, with its output caught. It never asks for a
    /// password on the console (several may run at once).
    pub fn run(&self, dir: Option<&Path>, args: &[&str]) -> Answer {
        let mut command = Command::new(&self.exe);
        if let Some(dir) = dir {
            command.arg("-C").arg(dir);
        }
        command.args(args).env("GIT_TERMINAL_PROMPT", "0");
        if let Some(proxy) = &self.proxy {
            command.env("HTTPS_PROXY", proxy).env("HTTP_PROXY", proxy);
        }
        match command.output() {
            Ok(o) => Answer {
                ok: o.status.success(),
                code: o.status.code().unwrap_or(-1),
                out: String::from_utf8_lossy(&o.stdout).trim_end().to_string(),
                err: String::from_utf8_lossy(&o.stderr).trim_end().to_string(),
            },
            Err(e) => Answer { ok: false, code: -1, out: String::new(), err: e.to_string() },
        }
    }

    /// The output of a command that worked.
    pub fn text(&self, dir: &Path, args: &[&str]) -> Option<String> {
        Some(self.run(Some(dir), args)).filter(|a| a.ok).map(|a| a.out)
    }
}
