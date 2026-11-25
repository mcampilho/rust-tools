//! `install`, `uninstall` and `status`.

use crate::config::{data_dir, Config};
use crate::scm::{self, State};
use rt_cli::Texts;
use rt_svc::client::{connect_to, Trust};
use rt_svc::{Level, Visibility};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const EXE: &str = "rtsvc.exe";

pub struct InstallOpts {
    pub dir: Option<PathBuf>,
    pub visibility: Visibility,
    pub yes: bool,
}

fn step<T>(t: &Texts, what: &str, r: std::io::Result<T>) -> Result<T, String> {
    r.map_err(|e| t.fill("err.step", &[("what", what), ("reason", &e.to_string())]))
}

/// The default place: `%ProgramFiles%\rust-tools`.
pub fn default_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".into())).join("rust-tools")
}

fn confirm(t: &Texts, input: &mut dyn BufRead, out: &mut dyn Write) -> bool {
    let _ = write!(out, "{} ", t.get("msg.confirm"));
    let _ = out.flush();
    let mut line = String::new();
    let _ = input.read_line(&mut line);
    let a = line.trim().to_lowercase();
    a == t.get("msg.confirm_yes") || a == "y" || a == "yes"
}

/// Copies this program into `dir` (not over a running copy: the service is stopped first).
fn copy_self(dir: &Path) -> std::io::Result<PathBuf> {
    let target = dir.join(EXE);
    let this = std::env::current_exe()?;
    // Installing again with the installed program itself: nothing to copy, and it must not be removed.
    if matches!((this.canonicalize(), target.canonicalize()), (Ok(a), Ok(b)) if a == b) {
        return Ok(target);
    }
    let tmp = dir.join("rtsvc.exe.new");
    std::fs::copy(this, &tmp)?;
    let _ = std::fs::remove_file(&target);
    std::fs::rename(&tmp, &target)?;
    Ok(target)
}

pub fn install(o: &InstallOpts, t: &Texts, input: &mut dyn BufRead, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match install_inner(o, t, input, out) {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(m) => {
            let _ = writeln!(err, "rtsvc: {m}");
            1
        }
    }
}

fn install_inner(o: &InstallOpts, t: &Texts, input: &mut dyn BufRead, out: &mut dyn Write) -> Result<bool, String> {
    if !rt_svc::access::is_elevated() {
        return Err(t.get("err.not_elevated").to_string());
    }
    let dir = o.dir.clone().unwrap_or_else(default_dir);
    let who = if o.visibility == Visibility::Elevated { "msg.who_elevated" } else { "msg.who_user" };
    let _ = writeln!(out, "{}", t.fill("msg.notice", &[("who", t.get(who))]));
    if !o.yes && !confirm(t, input, out) {
        let _ = writeln!(out, "{}", t.get("msg.cancelled"));
        return Ok(false);
    }
    // Stop a copy that runs from this folder, so that it can be replaced.
    if matches!(scm::query(), Ok((s, _)) if s != State::Stopped) && !step(t, "stop", scm::stop())? {
        return Err(t.get("err.refused_stop").to_string());
    }
    let existed = dir.exists();
    step(t, "create the folder", std::fs::create_dir_all(&dir))?;
    // The folder first (before anything of ours is in it).
    let problems = rt_svc::secure::check_dir(&dir);
    if !problems.is_empty() {
        if !existed {
            let _ = std::fs::remove_dir(&dir);
        }
        let mut m = t.fill("err.unsafe_folder", &[("dir", &dir.display().to_string())]);
        for p in &problems {
            m.push_str(&format!("\n  - {p}"));
        }
        m.push('\n');
        m.push_str(t.get("err.unsafe_hint"));
        return Err(m);
    }
    let exe = step(t, "copy the program", copy_self(&dir))?;
    // And once more with the program in it.
    let problems = rt_svc::secure::check_dir(&dir);
    if !problems.is_empty() {
        // Not the program that is running this (installed again by itself): that one stays.
        let _ = std::fs::remove_file(&exe);
        return Err(format!("{}\n  - {}", t.fill("err.unsafe_folder", &[("dir", &dir.display().to_string())]), problems.join("\n  - ")));
    }
    // A data folder that someone else made is not taken over: it is looked at before and after.
    let data = data_dir();
    let usable = || match rt_svc::secure::check_data_dir(&data) {
        p if p.is_empty() => Ok(()),
        p => Err(format!("{}\n  - {}", t.fill("err.unsafe_data", &[("dir", &data.display().to_string())]), p.join("\n  - "))),
    };
    usable()?;
    step(t, "protect the data folder", crate::config::protect(&data, crate::config::DATA_SDDL))?;
    usable()?;
    step(t, "save the settings", Config { visibility: o.visibility }.save())?;
    step(t, "register the service", scm::install(&exe))?;
    if !step(t, "start the service", scm::start())? {
        return Err(t.get("err.start_unsafe").to_string());
    }
    let _ = writeln!(out, "{}", t.fill("msg.installed", &[("dir", &dir.display().to_string())]));
    Ok(true)
}

pub fn uninstall(purge: bool, t: &Texts, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let r = (|| -> Result<bool, String> {
        if !rt_svc::access::is_elevated() {
            return Err(t.get("err.not_elevated").to_string());
        }
        let Ok((_, image)) = scm::query() else {
            let _ = writeln!(out, "{}", t.get("msg.not_installed"));
            return Ok(false);
        };
        if !step(t, "stop", scm::stop())? {
            return Err(t.get("err.refused_stop").to_string());
        }
        step(t, "remove the service", scm::remove())?;
        // The program, and its folder if nothing else is in it.
        let _ = std::fs::remove_file(&image);
        if let Some(dir) = image.parent() {
            let _ = std::fs::remove_dir(dir);
        }
        if purge {
            let _ = std::fs::remove_dir_all(data_dir());
        }
        let _ = writeln!(out, "{}", t.get("msg.uninstalled"));
        Ok(true)
    })();
    match r {
        Ok(_) => 0,
        Err(m) => {
            let _ = writeln!(err, "rtsvc: {m}");
            1
        }
    }
}

fn state_text(t: &Texts, s: State) -> String {
    match s {
        State::Stopped => t.get("state.stopped").to_string(),
        State::Starting => t.get("state.starting").to_string(),
        State::Running => t.get("state.running").to_string(),
        State::Stopping => t.get("state.stopping").to_string(),
        State::Other(n) => t.fill("state.other", &[("n", &n.to_string())]),
    }
}

pub fn status(t: &Texts, out: &mut dyn Write) -> i32 {
    let (state, image) = match scm::query() {
        Ok(x) => x,
        Err(_) => {
            let _ = writeln!(out, "{}", t.get("msg.not_installed"));
            return 1;
        }
    };
    let _ = writeln!(out, "{}", t.fill("msg.state", &[("name", rt_svc::SERVICE_NAME), ("state", &state_text(t, state))]));
    let _ = writeln!(out, "{}", t.fill("msg.image", &[("image", &image.display().to_string())]));
    match connect_to(rt_svc::PIPE_NAME, Trust::Installed, "rtsvc", env!("CARGO_PKG_VERSION"), 500).and_then(|mut c| {
        let s = c.status()?;
        Ok((c.level, c.service_version.clone(), s))
    }) {
        Ok((level, version, s)) => {
            let level = match level {
                Level::Denied => "level.denied",
                Level::User => "level.user",
                Level::Elevated => "level.elevated",
            };
            let _ = writeln!(
                out,
                "{}",
                t.fill(
                    "msg.answers",
                    &[
                        ("version", &version),
                        ("level", t.get(level)),
                        ("uptime", &s.uptime_secs.to_string()),
                        ("clients", &s.clients.to_string()),
                        ("scans", &s.scans.to_string()),
                    ]
                )
            );
            for (name, age) in s.snapshots {
                let _ = writeln!(out, "{}", t.fill("msg.snapshot", &[("name", &name), ("age", &age.to_string())]));
            }
            0
        }
        Err(e) => {
            let _ = writeln!(out, "{}", t.fill("msg.no_answer", &[("reason", &e.to_string())]));
            1
        }
    }
}
