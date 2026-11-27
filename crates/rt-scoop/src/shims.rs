//! Shims as Scoop makes them (`shim`, `warn_on_overwrite`, `get_shim_path`,
//! `Get-ShimTarget`, `get_app_name` in `lib\core.ps1`; `rm_shim` in
//! `lib\install.ps1`): one format for each kind of program, and the shim of
//! another app kept aside as `name.ext.<app>` until this one goes.

use crate::env::{self, Change};
use crate::install::Scoop;
use crate::system::System;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// `get_shim_path`: the `shim.exe` that is copied, by the `shim` setting.
pub fn exe_source(scoop: &Scoop) -> Result<PathBuf, String> {
    let kind = match scoop.setting("shim").str() {
        "" | "kiennq" => "kiennq",
        "scoopcs" | "default" => "scoopcs",
        "71" => "71",
        other => return Err(format!("Unknown shim version: '{other}'")),
    };
    Ok(scoop.app("scoop", false).join("current").join("supporting").join("shims").join(kind).join("shim.exe"))
}

/// `get_app_name`: the app whose folder `text` names first (lower case), or
/// "" when it names none.
pub fn app_name(scoop: &Scoop, text: &str) -> String {
    let lower = text.to_lowercase();
    for global in [false, true] {
        let apps = scoop.apps(global);
        if !apps.is_dir() {
            continue;
        }
        let dir = env::absolute(&apps).display().to_string().to_lowercase();
        let mut from = 0;
        while let Some(at) = lower[from..].find(&dir).map(|i| i + from) {
            let rest = &lower[at + dir.len()..];
            if let Some(after) = rest.strip_prefix(['/', '\\']) {
                let name: String = after.chars().take_while(|c| *c != '/' && *c != '\\').collect();
                if !name.is_empty() {
                    return name;
                }
            }
            from = at + 1;
        }
    }
    String::new()
}

/// `get_app_name_from_shim`.
pub fn app_of_shim(scoop: &Scoop, shim: &Path) -> String {
    match crate::json::read_text(shim) {
        Some(text) => app_name(scoop, &text.lines().collect::<Vec<_>>().join(" ")),
        None => String::new(),
    }
}

/// `strip_ext`: without the last extension.
pub fn strip_ext(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) => &name[..i],
        None => name,
    }
}

fn leaf(path: &str) -> &str {
    path.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or(path)
}

/// `Get-PESubsystem` (2: a window program); -1 when it is no PE file.
pub fn pe_subsystem(path: &Path) -> i32 {
    let read = || -> std::io::Result<i16> {
        let mut f = std::fs::File::open(path)?;
        let mut four = [0u8; 4];
        f.seek(SeekFrom::Start(0x3C))?;
        f.read_exact(&mut four)?;
        let pe = u32::from_le_bytes(four);
        let mut two = [0u8; 2];
        f.seek(SeekFrom::Start(u64::from(pe) + 0x5C))?;
        f.read_exact(&mut two)?;
        Ok(i16::from_le_bytes(two))
    };
    read().map_or(-1, i32::from)
}

/// `Get-PEMachine` (0x014c: x86).
pub fn pe_machine(path: &Path) -> u16 {
    let read = || -> std::io::Result<u16> {
        let mut f = std::fs::File::open(path)?;
        let mut four = [0u8; 4];
        f.seek(SeekFrom::Start(0x3C))?;
        f.read_exact(&mut four)?;
        let mut two = [0u8; 2];
        f.seek(SeekFrom::Start(u64::from(u32::from_le_bytes(four)) + 4))?;
        f.read_exact(&mut two)?;
        Ok(u16::from_le_bytes(two))
    };
    read().unwrap_or(0)
}

/// `Set-PESubsystem`.
fn set_pe_subsystem(path: &Path, subsystem: i16) -> std::io::Result<()> {
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let mut four = [0u8; 4];
    f.seek(SeekFrom::Start(0x3C))?;
    f.read_exact(&mut four)?;
    f.seek(SeekFrom::Start(u64::from(u32::from_le_bytes(four)) + 0x5C))?;
    f.write_all(&subsystem.to_le_bytes())
}

/// A shim of another app that this one replaced.
#[derive(Debug, PartialEq, Eq)]
pub struct Overwrite {
    /// The names Scoop shows: the shim and the new target, `.shim` as `.exe`.
    pub shim: String,
    pub target: String,
    /// The app the old shim came from ("" when none).
    pub from: String,
}

/// `warn_on_overwrite`: an existing shim of another app is kept aside as
/// `<shim>.<its app>`.
fn overwrite(scoop: &Scoop, shim: &Path, path: &Path) -> Option<Overwrite> {
    if !shim.exists() {
        return None;
    }
    let shim_app = app_of_shim(scoop, shim);
    let path_app = app_name(scoop, &path.display().to_string());
    if shim_app == path_app {
        return None;
    }
    let beside = |app: &str| {
        let mut name = shim.as_os_str().to_os_string();
        name.push(format!(".{app}"));
        PathBuf::from(name)
    };
    // An empty name would name the shim itself.
    if !path_app.is_empty() && beside(&path_app).is_file() {
        let _ = std::fs::remove_file(beside(&path_app));
    }
    if !shim_app.is_empty() && !beside(&shim_app).exists() {
        let _ = std::fs::rename(shim, beside(&shim_app));
    }
    let as_exe = |name: &str| match name.strip_suffix(".shim") {
        Some(stem) => format!("{stem}.exe"),
        None => name.to_string(),
    };
    Some(Overwrite { shim: as_exe(leaf(&shim.display().to_string())), target: as_exe(leaf(&path.display().to_string())), from: shim_app })
}

/// What making a shim did.
#[derive(Debug, Default)]
pub struct Made {
    /// The folder of the shims, added to `PATH`.
    pub path: Vec<Change>,
    pub overwritten: Vec<Overwrite>,
    /// The `.exe` that was made a window program.
    pub gui: Option<PathBuf>,
}

/// `[System.IO.File]::WriteAllLines`: the text and a line end, UTF-8.
fn write_lines(path: &Path, lines: &[String]) -> std::io::Result<()> {
    std::fs::write(path, format!("{}\r\n", lines.join("\r\n")))
}

fn with_ext(shim: &Path, ext: &str) -> PathBuf {
    let mut name = shim.as_os_str().to_os_string();
    name.push(ext);
    PathBuf::from(name)
}

/// `Resolve-Path -Relative` from `from`: `.\x` or `..\..\x`, or the path
/// itself on another drive.
fn relative(from: &Path, to: &Path) -> String {
    let parts = |p: &Path| p.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>();
    let (a, b) = (parts(from), parts(to));
    let same = a.iter().zip(&b).take_while(|(x, y)| x.eq_ignore_ascii_case(y)).count();
    if same == 0 {
        return to.display().to_string();
    }
    let ups = a.len() - same;
    let rest = b[same..].join("\\");
    if ups == 0 {
        format!(".\\{rest}")
    } else {
        format!("{}{rest}", "..\\".repeat(ups))
    }
}

/// `$arg` as PowerShell puts it in a string: the items joined by spaces.
fn joined(args: &[String]) -> String {
    args.join(" ")
}

/// `if ($arg)`.
fn truthy(args: &[String]) -> bool {
    match args {
        [] => false,
        [one] => !one.is_empty(),
        _ => true,
    }
}

/// `shim`: a shim named `name` (by default the program's name) in the shims
/// folder of the scope, for the file `path`, with `args` before the user's.
pub fn create(scoop: &Scoop, system: &dyn System, path: &Path, global: bool, name: Option<&str>, args: &[String]) -> Result<Made, String> {
    if !path.exists() {
        let file = leaf(&path.display().to_string()).to_string();
        return Err(format!("Can't shim '{file}': couldn't find '{}'.", path.display()));
    }
    let dir = scoop.shims(global);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let dir = env::absolute(&dir);
    let mut made = Made { path: env::add_path(system, &[dir.display().to_string()], "PATH", global, false)?, ..Made::default() };
    let given = path.display().to_string();
    let name = match name.filter(|n| !n.is_empty()) {
        Some(n) => n.to_string(),
        None => strip_ext(leaf(&given)).to_string(),
    };
    let shim = dir.join(name.to_lowercase());
    let resolved = crate::disk::plain(&std::path::absolute(path).map_err(|e| e.to_string())?);
    let resolved_text = resolved.display().to_string();
    let arg = joined(args);
    let lower = given.to_lowercase();
    let fail = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let mut overwritten = Vec::new();
    let mut note = |o: Option<Overwrite>| overwritten.extend(o);

    if lower.ends_with(".exe") || lower.ends_with(".com") {
        note(overwrite(scoop, &with_ext(&shim, ".shim"), path));
        let exe = with_ext(&shim, ".exe");
        let source = exe_source(scoop)?;
        if exe.exists() {
            crate::disk::set_link_readonly(&exe, false);
        }
        std::fs::copy(&source, &exe).map_err(|e| fail(&source, e))?;
        // An x86 shim on a 64-bit Windows sees the system folders swapped.
        let mut written = resolved_text.clone();
        if pe_machine(&exe) == 0x014c && cfg!(target_pointer_width = "64") {
            let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
            let swap = |from: &str, to: &str, text: &str| -> Option<String> {
                let prefix = format!("{root}\\{from}\\");
                text.to_lowercase().starts_with(&prefix.to_lowercase()).then(|| format!("{root}\\{to}\\{}", &text[prefix.len()..]))
            };
            if let Some(w) = swap("System32", "Sysnative", &written).or_else(|| swap("SysWOW64", "System32", &written)) {
                written = w;
            }
        }
        let file = with_ext(&shim, ".shim");
        write_lines(&file, &[format!("path = \"{written}\"")]).map_err(|e| fail(&file, e))?;
        if truthy(args) {
            let mut f = std::fs::OpenOptions::new().append(true).open(&file).map_err(|e| fail(&file, e))?;
            f.write_all(format!("args = {arg}").as_bytes()).map_err(|e| fail(&file, e))?;
        }
        if pe_subsystem(&resolved) == 2 {
            set_pe_subsystem(&exe, 2).map_err(|e| fail(&exe, e))?;
            made.gui = Some(exe);
        }
        made.overwritten = overwritten;
        return Ok(made);
    }

    // The other kinds: a `.cmd` (or `.ps1`) and a script for `sh` beside it.
    let parent = resolved.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let (windows, sh): (Vec<(String, Vec<String>)>, Vec<String>) = if lower.ends_with(".bat") || lower.ends_with(".cmd") {
        (
            vec![(".cmd".into(), vec![format!("@rem {resolved_text}"), format!("@\"{resolved_text}\" {arg} %*")])],
            vec![
                "#!/bin/sh".into(),
                format!("# {resolved_text}"),
                format!("MSYS2_ARG_CONV_EXCL=/C cmd.exe /C \"{resolved_text}\" {arg} \"$@\""),
            ],
        )
    } else if lower.ends_with(".ps1") {
        let relative = relative(&dir, &resolved);
        let on_other_drive = {
            let r = relative.strip_prefix(".\\").unwrap_or(&relative);
            r.len() >= 2 && r.as_bytes()[1] == b':' && r.as_bytes()[0].is_ascii_alphanumeric()
        };
        let path_line =
            if on_other_drive { format!("$path = \"{given}\"") } else { format!("$path = Join-Path $PSScriptRoot \"{relative}\"") };
        let call = format!("if ($MyInvocation.ExpectingInput) {{ $input | & $path {arg} @args }} else {{ & $path {arg} @args }}");
        (
            vec![
                (".ps1".into(), vec![format!("# {resolved_text}"), path_line, call, "exit $LASTEXITCODE".into()]),
                (
                    ".cmd".into(),
                    vec![
                        format!("@rem {resolved_text}"),
                        "@echo off".into(),
                        "where /q pwsh.exe".into(),
                        "if %errorlevel% equ 0 (".into(),
                        format!("    pwsh -noprofile -ex unrestricted -file \"{resolved_text}\" {arg} %*"),
                        ") else (".into(),
                        format!("    powershell -noprofile -ex unrestricted -file \"{resolved_text}\" {arg} %*"),
                        ")".into(),
                    ],
                ),
            ],
            vec![
                "#!/bin/sh".into(),
                format!("# {resolved_text}"),
                "if command -v pwsh.exe > /dev/null 2>&1; then".into(),
                format!("    pwsh.exe -noprofile -ex unrestricted -file \"{resolved_text}\" {arg} \"$@\""),
                "else".into(),
                format!("    powershell.exe -noprofile -ex unrestricted -file \"{resolved_text}\" {arg} \"$@\""),
                "fi".into(),
            ],
        )
    } else if lower.ends_with(".jar") {
        (
            vec![(
                ".cmd".into(),
                vec![
                    format!("@rem {resolved_text}"),
                    format!("@pushd {parent}"),
                    format!("@java -jar \"{resolved_text}\" {arg} %*"),
                    "@popd".into(),
                ],
            )],
            vec![
                "#!/bin/sh".into(),
                format!("# {resolved_text}"),
                "if [ $WSL_INTEROP ]".into(),
                "then".into(),
                format!("  cd $(wslpath -u '{parent}')"),
                "else".into(),
                format!("  cd $(cygpath -u '{parent}')"),
                "fi".into(),
                format!("java.exe -jar \"{resolved_text}\" {arg} \"$@\""),
            ],
        )
    } else if lower.ends_with(".py") {
        (
            vec![(".cmd".into(), vec![format!("@rem {resolved_text}"), format!("@python \"{resolved_text}\" {arg} %*")])],
            vec!["#!/bin/sh".into(), format!("# {resolved_text}"), format!("python.exe \"{resolved_text}\" {arg} \"$@\"")],
        )
    } else {
        let quoted: Vec<String> = args.iter().map(|a| format!("\"{a}\"")).collect();
        let quoted = quoted.join(" ");
        (
            vec![(
                ".cmd".into(),
                vec![
                    format!("@rem {resolved_text}"),
                    "@echo off".into(),
                    "bash -c \"command -v wslpath >/dev/null\"".into(),
                    "if %errorlevel% equ 0 (".into(),
                    format!("  bash \"$(wslpath -u '{resolved_text}')\" {quoted} %*"),
                    ") else (".into(),
                    format!("  set args={quoted} %*"),
                    "  setlocal enabledelayedexpansion".into(),
                    "  if not \"!args!\"==\"\" set args=!args:\"=\"\"!".into(),
                    format!("  bash -c \"$(cygpath -u '{resolved_text}') !args!\""),
                    ")".into(),
                ],
            )],
            vec![
                "#!/bin/sh".into(),
                format!("# {resolved_text}"),
                "if [ $WSL_INTEROP ]".into(),
                "then".into(),
                format!("  \"$(wslpath -u '{resolved_text}')\" {arg} \"$@\""),
                "else".into(),
                format!("  \"$(cygpath -u '{resolved_text}')\" {arg} \"$@\""),
                "fi".into(),
            ],
        )
    };
    for (ext, lines) in &windows {
        let file = with_ext(&shim, ext);
        note(overwrite(scoop, &file, path));
        write_lines(&file, lines).map_err(|e| fail(&file, e))?;
    }
    note(overwrite(scoop, &shim, path));
    std::fs::write(&shim, sh.join("\n")).map_err(|e| fail(&shim, e))?;
    made.overwritten = overwritten;
    Ok(made)
}

/// The files `<leaf>.*` beside `path`, without those ending in `.shim`,
/// `.cmd` or `.ps1` (`Get-Item "$path.*" -Exclude …`).
fn kept_aside(path: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return Vec::new() };
    let prefix = format!("{}.", name.to_string_lossy().to_lowercase());
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            n.starts_with(&prefix) && ![".shim", ".cmd", ".ps1"].iter().any(|x| n.ends_with(x))
        })
        .map(|e| e.path())
        .collect()
}

/// `rm_shim`: removes the shims named `name`. With `app`, a shim of that app
/// kept aside goes instead; when the shim itself goes, the newest one kept
/// aside takes its place. Returns the names of the files removed.
pub fn remove(dir: &Path, name: &str, app: Option<&str>) -> Vec<String> {
    let mut removed = Vec::new();
    for ext in ["", ".shim", ".cmd", ".ps1"] {
        let shim = dir.join(format!("{name}{ext}"));
        let aside = app.map(|a| with_ext(&shim, &format!(".{a}")));
        if let Some(aside) = aside.filter(|p| p.is_file()) {
            if std::fs::remove_file(&aside).is_ok() {
                removed.push(format!("{name}{ext}.{}", app.unwrap_or_default()));
            }
        } else if shim.is_file() && !name.is_empty() {
            if std::fs::remove_file(&shim).is_err() {
                continue;
            }
            removed.push(format!("{name}{ext}"));
            let mut older = kept_aside(&shim);
            if older.is_empty() {
                if ext == ".shim" && std::fs::remove_file(dir.join(format!("{name}.exe"))).is_ok() {
                    removed.push(format!("{name}.exe"));
                }
            } else {
                older.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
                let newest = older.pop().unwrap_or_default();
                let back = PathBuf::from(strip_ext(&newest.display().to_string()));
                let _ = std::fs::rename(&newest, back);
            }
        }
    }
    removed
}

/// `Get-ShimTarget`: the program a shim runs, if it is there.
pub fn target(shim: &Path) -> Option<PathBuf> {
    let text = crate::json::read_text(shim)?;
    let mut found = if shim.extension().is_some_and(|e| e.eq_ignore_ascii_case("shim")) {
        text.lines().next().unwrap_or("").replace("path = ", "").replace('"', "")
    } else {
        // `^(?:@rem|#)\s*(.*)$`, the first line that matches.
        text.lines().find_map(|l| l.strip_prefix("@rem").or_else(|| l.strip_prefix('#'))).map(|r| r.trim().to_string()).unwrap_or_default()
    };
    if found.is_empty() {
        // The last quoted text without `@` or `&`.
        found = text
            .split(['\'', '"'])
            .enumerate()
            .filter(|(i, s)| i % 2 == 1 && !s.contains(['@', '&']))
            .map(|(_, s)| s.to_string())
            .last()
            .unwrap_or_default();
    }
    let path = PathBuf::from(found.trim());
    if !found.trim().is_empty() && path.exists() {
        return Some(env::absolute(&path));
    }
    let swapped = found.replace(r"\Sysnative\", r"\System32\");
    (swapped != found && Path::new(&swapped).exists()).then(|| env::absolute(Path::new(&swapped)))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::system::FakeSystem;
    use crate::testing::Tree;

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    /// An installation whose Scoop has a `shim.exe`: a copy of a real window
    /// program's header is enough to see the subsystem change.
    fn installation(name: &str) -> (Tree, Scoop) {
        let t = Tree::new(name);
        let mut exe = vec![0u8; 0x200];
        exe[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        exe[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        exe[0x80 + 0x5C..0x80 + 0x5E].copy_from_slice(&3u16.to_le_bytes());
        let source = t.dir("root/apps/scoop/current/supporting/shims/kiennq").join("shim.exe");
        std::fs::write(&source, &exe).unwrap();
        let mut gui = exe.clone();
        gui[0x80 + 0x5C..0x80 + 0x5E].copy_from_slice(&2u16.to_le_bytes());
        std::fs::write(t.dir("root/apps/win/1.0").join("win.exe"), gui).unwrap();
        std::fs::write(t.dir("root/apps/cli/1.0").join("cli.exe"), &exe).unwrap();
        let scoop = t.scoop();
        (t, scoop)
    }

    #[test]
    fn programs_get_an_exe_and_a_shim_file() {
        let (t, scoop) = installation("shim-exe");
        let s = FakeSystem::default();
        let shims = t.dir("root/shims");
        let win = t.dir("root/apps/win/1.0").join("win.exe");
        let made = create(&scoop, &s, &win, false, None, &["-a".into(), "$x".into()]).unwrap();
        assert_eq!(made.path, [Change::Added(shims.display().to_string())]);
        assert_eq!(read(&shims.join("win.shim")), format!("path = \"{}\"\r\nargs = -a $x", win.display()));
        assert_eq!(made.gui.as_deref(), Some(shims.join("win.exe").as_path()));
        assert_eq!(pe_subsystem(&shims.join("win.exe")), 2);
        // The folder is on PATH now: not added twice.
        let cli = t.dir("root/apps/cli/1.0").join("cli.exe");
        let made = create(&scoop, &s, &cli, false, Some("Cli"), &[]).unwrap();
        assert!(made.path.is_empty() && made.gui.is_none());
        assert_eq!(read(&shims.join("cli.shim")), format!("path = \"{}\"\r\n", cli.display()));
        assert_eq!(target(&shims.join("cli.shim")), Some(cli.clone()));
        assert_eq!(app_of_shim(&scoop, &shims.join("cli.shim")), "cli");
    }

    #[test]
    fn another_apps_shim_is_kept_and_comes_back() {
        let (t, scoop) = installation("shim-over");
        let s = FakeSystem::default();
        let shims = t.dir("root/shims");
        let cli = t.dir("root/apps/cli/1.0").join("cli.exe");
        let win = t.dir("root/apps/win/1.0").join("win.exe");
        create(&scoop, &s, &cli, false, Some("tool"), &[]).unwrap();
        let made = create(&scoop, &s, &win, false, Some("tool"), &[]).unwrap();
        assert_eq!(made.overwritten, [Overwrite { shim: "tool.exe".into(), target: "win.exe".into(), from: "cli".into() }]);
        assert!(shims.join("tool.shim.cli").is_file());
        assert_eq!(app_of_shim(&scoop, &shims.join("tool.shim")), "win");

        // The app that replaced it goes: the old one comes back.
        assert_eq!(remove(&shims, "tool", Some("win")), ["tool.shim"]);
        assert_eq!(app_of_shim(&scoop, &shims.join("tool.shim")), "cli");
        assert!(shims.join("tool.exe").is_file());
        // The one kept aside goes on its own, leaving the shim in place.
        create(&scoop, &s, &win, false, Some("tool"), &[]).unwrap();
        assert_eq!(remove(&shims, "tool", Some("cli")), ["tool.shim.cli"]);
        assert_eq!(remove(&shims, "tool", Some("win")), ["tool.shim", "tool.exe"]);
        assert!(!shims.join("tool.exe").exists() && !shims.join("tool.shim").exists());
    }

    #[test]
    fn scripts_get_their_own_formats() {
        let (t, scoop) = installation("shim-scripts");
        let s = FakeSystem::default();
        let shims = t.dir("root/shims");
        let dir = t.dir("root/apps/tools/1.0");
        let cmd = t.file("root/apps/tools/1.0/run.cmd", "");
        let ps1 = t.file("root/apps/tools/1.0/sub/go.ps1", "");
        let sh = t.file("root/apps/tools/1.0/tool", "");
        create(&scoop, &s, &cmd, false, None, &[]).unwrap();
        let c = cmd.display();
        assert_eq!(read(&shims.join("run.cmd")), format!("@rem {c}\r\n@\"{c}\"  %*\r\n"));
        assert_eq!(read(&shims.join("run")), format!("#!/bin/sh\n# {c}\nMSYS2_ARG_CONV_EXCL=/C cmd.exe /C \"{c}\"  \"$@\""));
        assert_eq!(target(&shims.join("run.cmd")), Some(cmd.clone()));

        create(&scoop, &s, &ps1, false, None, &["-x".into()]).unwrap();
        let text = read(&shims.join("go.ps1"));
        assert!(
            text.starts_with(&format!("# {}\r\n$path = Join-Path $PSScriptRoot \"..\\apps\\tools\\1.0\\sub\\go.ps1\"\r\n", ps1.display())),
            "{text}"
        );
        assert!(text.contains("& $path -x @args"), "{text}");
        assert!(read(&shims.join("go.cmd")).contains(&format!("pwsh -noprofile -ex unrestricted -file \"{}\" -x %*", ps1.display())));
        assert!(shims.join("go").is_file());

        create(&scoop, &s, &sh, false, Some("t"), &["a b".into()]).unwrap();
        assert!(read(&shims.join("t.cmd")).contains(&format!("  bash \"$(wslpath -u '{}')\" \"a b\" %*", sh.display())));
        assert_eq!(remove(&shims, "go", None), ["go", "go.cmd", "go.ps1"]);
        assert!(create(&scoop, &s, &dir.join("none.exe"), false, None, &[]).is_err());
    }
}
