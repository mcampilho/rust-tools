//! Native dialogs through zenity or kdialog, whichever the desktop has. They
//! block like the Win32 ones: the host calls are synchronous. Without either
//! tool the questions answer "no", so nothing is overwritten or lost.

use crate::app::Answer;
use crate::i18n::tr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Zenity,
    Kdialog,
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

/// The tool to use: the desktop's own first.
fn tool() -> Option<Tool> {
    static TOOL: OnceLock<Option<Tool>> = OnceLock::new();
    *TOOL.get_or_init(|| {
        let kde = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_uppercase().contains("KDE"));
        let order = if kde { [Tool::Kdialog, Tool::Zenity] } else { [Tool::Zenity, Tool::Kdialog] };
        order.into_iter().find(|t| on_path(if *t == Tool::Zenity { "zenity" } else { "kdialog" }))
    })
}

pub fn available() -> bool {
    tool().is_some()
}

fn run(tool: Tool, args: &[String]) -> Option<Output> {
    let name = if tool == Tool::Zenity { "zenity" } else { "kdialog" };
    Command::new(name).args(args).stderr(Stdio::null()).output().ok()
}

fn zenity_message(kind: &str, title: &str, text: &str, extra: &[String]) -> Vec<String> {
    let mut a = vec![kind.to_string(), "--no-markup".to_string(), "--width=380".to_string(), format!("--title={title}"), format!("--text={text}")];
    a.extend_from_slice(extra);
    a
}

pub fn ask_save(text: &str) -> Answer {
    let (save, dont, cancel) = (tr("Save"), tr("Don't Save"), tr("Cancel"));
    match tool() {
        Some(Tool::Zenity) => {
            let extra = [format!("--ok-label={save}"), format!("--cancel-label={cancel}"), format!("--extra-button={dont}")];
            let Some(out) = run(Tool::Zenity, &zenity_message("--question", "rpad", text, &extra)) else { return Answer::Cancel };
            if out.status.success() {
                Answer::Yes
            } else if String::from_utf8_lossy(&out.stdout).trim() == dont {
                Answer::No
            } else {
                Answer::Cancel
            }
        }
        Some(Tool::Kdialog) => {
            let args = ["--title", "rpad", "--yesnocancel", text, "--yes-label", &save, "--no-label", &dont, "--cancel-label", &cancel].map(String::from);
            match run(Tool::Kdialog, &args).and_then(|o| o.status.code()) {
                Some(0) => Answer::Yes,
                Some(1) => Answer::No,
                _ => Answer::Cancel,
            }
        }
        None => Answer::Cancel,
    }
}

pub fn confirm(title: &str, text: &str) -> bool {
    let out = match tool() {
        Some(Tool::Zenity) => run(Tool::Zenity, &zenity_message("--question", title, text, &[])),
        Some(Tool::Kdialog) => run(Tool::Kdialog, &["--title", title, "--yesno", text].map(String::from)),
        None => None,
    };
    out.is_some_and(|o| o.status.success())
}

fn message(zenity: &str, kdialog: &str, title: &str, text: &str) {
    match tool() {
        Some(Tool::Zenity) => {
            run(Tool::Zenity, &zenity_message(zenity, title, text, &[]));
        }
        Some(Tool::Kdialog) => {
            run(Tool::Kdialog, &["--title", title, kdialog, text].map(String::from));
        }
        None => eprintln!("{text}"),
    }
}

pub fn info(title: &str, text: &str) {
    message("--info", "--msgbox", title, text);
}

pub fn error(title: &str, text: &str) {
    message("--error", "--error", title, text);
}

fn lines(out: Option<Output>) -> Vec<PathBuf> {
    match out {
        Some(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect(),
        _ => Vec::new(),
    }
}

/// A folder as the tools expect it: with the final slash, zenity opens it.
fn folder(dir: Option<&Path>) -> String {
    dir.map_or_else(String::new, |d| format!("{}/", d.display()))
}

pub fn open_files(dir: Option<&Path>) -> Vec<PathBuf> {
    let start = folder(dir);
    lines(match tool() {
        Some(Tool::Zenity) => run(Tool::Zenity, &["--file-selection".to_string(), "--multiple".to_string(), "--separator=\n".to_string(), format!("--filename={start}")]),
        Some(Tool::Kdialog) => run(Tool::Kdialog, &["--getopenfilename", &start, "--multiple", "--separate-output"].map(String::from)),
        None => None,
    })
}

pub fn save_file(name: &str, dir: Option<&Path>) -> Option<PathBuf> {
    let start = format!("{}{name}", folder(dir));
    lines(match tool() {
        Some(Tool::Zenity) => run(Tool::Zenity, &["--file-selection".to_string(), "--save".to_string(), "--confirm-overwrite".to_string(), format!("--filename={start}")]),
        Some(Tool::Kdialog) => run(Tool::Kdialog, &["--getsavefilename", &start].map(String::from)),
        None => None,
    })
    .into_iter()
    .next()
}

pub fn pick_folder(title: &str) -> Option<PathBuf> {
    lines(match tool() {
        Some(Tool::Zenity) => run(Tool::Zenity, &["--file-selection".to_string(), "--directory".to_string(), format!("--title={title}")]),
        Some(Tool::Kdialog) => run(Tool::Kdialog, &["--title", title, "--getexistingdirectory", "."].map(String::from)),
        None => None,
    })
    .into_iter()
    .next()
}

fn stdout_line(out: Option<Output>) -> Option<String> {
    let o = out.filter(|o| o.status.success())?;
    let line = String::from_utf8_lossy(&o.stdout).lines().next()?.trim().to_string();
    (!line.is_empty()).then_some(line)
}

/// Picks a family from the monospaced fonts, then its size.
pub fn choose_font(face: &str, size: f32) -> Option<(String, f32)> {
    let title = tr("Font");
    let families = super::mono_families();
    if families.is_empty() {
        return None;
    }
    let size_label = tr("Font size");
    let current = format!("{size}");
    let (family, chosen) = match tool()? {
        Tool::Zenity => {
            let mut a = vec!["--list".to_string(), format!("--title={title}"), format!("--column={title}"), "--height=420".to_string()];
            a.extend(families);
            let family = stdout_line(run(Tool::Zenity, &a))?;
            let entry = ["--entry".to_string(), format!("--title={title}"), format!("--text={size_label}"), format!("--entry-text={current}")];
            (family, stdout_line(run(Tool::Zenity, &entry))?)
        }
        Tool::Kdialog => {
            let mut a = ["--title", &title, "--combobox", &title].map(String::from).to_vec();
            a.extend(families);
            a.extend(["--default".to_string(), face.to_string()]);
            let family = stdout_line(run(Tool::Kdialog, &a))?;
            let entry = ["--title", &title, "--inputbox", &size_label, &current].map(String::from);
            (family, stdout_line(run(Tool::Kdialog, &entry))?)
        }
    };
    let size = chosen.replace(',', ".").parse::<f32>().ok().filter(|s| (4.0..=96.0).contains(s))?;
    Some((family, size))
}
