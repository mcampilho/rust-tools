//! Commands not done here run in the Scoop of the installation, with the
//! console as it is.

use super::Ctx;
use std::path::PathBuf;
use std::process::Command;

/// PowerShell 7 when it is there, as Scoop's own shims prefer.
pub(super) fn powershell(ctx: &Ctx) -> PathBuf {
    ctx.env.path().map(|d| d.join("pwsh.exe")).find(|p| p.is_file()).unwrap_or_else(|| PathBuf::from("powershell.exe"))
}

/// Runs `scoop <args>`; with `--root`, on that installation.
pub fn run(ctx: &mut Ctx, args: &[String], root_given: bool) -> i32 {
    let script = ctx.scoop.app("scoop", false).join("current").join("bin").join("scoop.ps1");
    if !script.is_file() {
        let root = ctx.scoop.root.display().to_string();
        return ctx.fail("error.no_scoop", &[("root", &root)]);
    }
    let _ = ctx.out.flush();
    let mut command = Command::new(powershell(ctx));
    command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(&script).args(args);
    if root_given {
        command.env("SCOOP", &ctx.scoop.root);
    }
    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            let e = e.to_string();
            ctx.fail("error.start", &[("what", "PowerShell"), ("why", &e)])
        }
    }
}
