//! Repair: Windows' own repair tools, run one at a time with what they write
//! shown as it comes — `sfc /scannow`, `DISM /Online /Cleanup-Image
//! /RestoreHealth`, `chkdsk <drive> /scan` (online, no restart) and, when it
//! finds errors, `/spotfix` (on the system drive Windows does it at the next
//! start), and repairing the Start menu (its processes restarted and Windows'
//! own package registered again). The user's choices, 2026-11-23. All need
//! administrator rights.

/// A repair.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Task {
    Sfc,
    Dism,
    /// `chkdsk X: /scan`.
    Check(char),
    /// `chkdsk X: /spotfix`.
    Fix(char),
    StartMenu,
}

/// Restarts Start's and Search's processes (Windows starts them again) and
/// registers Windows' own Start menu and shell packages again.
const START_MENU: &str = "$ErrorActionPreference='Continue'
'Restarting Start and Search…'
Get-Process StartMenuExperienceHost,ShellExperienceHost,SearchHost,SearchApp -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
foreach($p in Get-AppxPackage -AllUsers Microsoft.Windows.StartMenuExperienceHost, Microsoft.Windows.ShellExperienceHost){
  \"Registering $($p.Name)…\"
  Add-AppxPackage -DisableDevelopmentMode -Register \"$($p.InstallLocation)\\AppXManifest.xml\"
}
'Done.'";

impl Task {
    /// The program, its arguments, and what is typed to it (chkdsk asks before scheduling).
    pub fn command(&self) -> (String, Vec<String>, Option<&'static str>) {
        let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        match self {
            Task::Sfc => ("sfc.exe".into(), s(&["/scannow"]), None),
            Task::Dism => ("dism.exe".into(), s(&["/Online", "/Cleanup-Image", "/RestoreHealth"]), None),
            Task::Check(d) => ("chkdsk.exe".into(), vec![format!("{d}:"), "/scan".into()], None),
            Task::Fix(d) => ("chkdsk.exe".into(), vec![format!("{d}:"), "/spotfix".into()], Some("Y\r\n")),
            Task::StartMenu => {
                ("powershell.exe".into(), s(&["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", START_MENU]), None)
            }
        }
    }

    /// The line as it would be typed (shown above the output).
    pub fn command_line(&self) -> String {
        if *self == Task::StartMenu {
            return "Stop-Process StartMenuExperienceHost…; Add-AppxPackage -Register …".into();
        }
        let (exe, args, _) = self.command();
        format!("{} {}", exe.trim_end_matches(".exe"), args.join(" "))
    }
}

/// What a finished `chkdsk /scan` says, by its exit code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckResult {
    /// 0: no errors.
    Clean,
    /// 1, 2: errors were fixed, or a cleanup was done.
    Fixed,
    /// 3: errors that need `/spotfix`.
    NeedsFix,
    Other(i32),
}

pub fn check_result(code: i32) -> CheckResult {
    match code {
        0 => CheckResult::Clean,
        1 | 2 => CheckResult::Fixed,
        3 => CheckResult::NeedsFix,
        n => CheckResult::Other(n),
    }
}

/// The last percentage in a line (`Verification 45% complete.`, `[==== 45.0% ]`).
pub fn percent(line: &str) -> Option<f32> {
    let i = line.rfind('%')?;
    let head = line[..i].trim_end();
    let start = head.rfind(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).map_or(0, |k| k + 1);
    head[start..].replace(',', ".").parse::<f32>().ok().filter(|p| (0.0..=100.0).contains(p))
}

/// The lines a tool wrote: `\r` rewrites the line (progress), `\n` ends it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub lines: Vec<String>,
    pub current: String,
    pending_cr: bool,
}

impl Output {
    /// The most lines kept.
    pub const KEEP: usize = 2000;

    pub fn feed(&mut self, text: &str) {
        for ch in text.chars() {
            match ch {
                '\n' => {
                    self.lines.push(std::mem::take(&mut self.current).trim_end().to_string());
                    self.pending_cr = false;
                }
                '\r' => self.pending_cr = true,
                '\0' => {}
                c => {
                    if self.pending_cr {
                        self.current.clear();
                        self.pending_cr = false;
                    }
                    self.current.push(c);
                }
            }
        }
        if self.lines.len() > Self::KEEP {
            let extra = self.lines.len() - Self::KEEP;
            self.lines.drain(..extra);
        }
    }

    /// Every line, the one being written last; empty lines in a row are one.
    pub fn all(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        let cur = self.current.trim_end();
        for l in self.lines.iter().map(String::as_str).chain((!cur.is_empty()).then_some(cur)) {
            if l.trim().is_empty() && out.last().is_some_and(|p| p.trim().is_empty()) {
                continue;
            }
            out.push(l);
        }
        out
    }

    /// The progress, as the last line that says a percentage.
    pub fn progress(&self) -> Option<f32> {
        percent(&self.current).or_else(|| self.lines.iter().rev().take(3).find_map(|l| percent(l)))
    }
}

/// Turns a tool's bytes into text: UTF-16 (what `sfc` writes) or the console's
/// code page (`chkdsk`, `DISM`), decided by the first bytes.
#[derive(Default)]
pub struct Decoder {
    utf16: Option<bool>,
    carry: Vec<u8>,
}

impl Decoder {
    pub fn decode(&mut self, bytes: &[u8]) -> String {
        let mut b = std::mem::take(&mut self.carry);
        b.extend_from_slice(bytes);
        if self.utf16.is_none() && b.len() >= 2 {
            // ASCII in UTF-16LE: a letter, then 0.
            let zeros = b.iter().skip(1).step_by(2).take(16).filter(|x| **x == 0).count();
            let pairs = b.len().min(32) / 2;
            self.utf16 = Some(pairs > 0 && zeros * 2 >= pairs);
        }
        match self.utf16 {
            Some(true) => {
                if b.len() % 2 == 1 {
                    self.carry.push(b.pop().unwrap_or(0));
                }
                let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                String::from_utf16_lossy(&units)
            }
            Some(false) => oem(&b),
            None => {
                self.carry = b;
                String::new()
            }
        }
    }
}

/// The console's code page to text.
#[cfg(windows)]
fn oem(b: &[u8]) -> String {
    use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP};
    if b.is_empty() {
        return String::new();
    }
    if let Ok(s) = std::str::from_utf8(b) {
        return s.to_string();
    }
    let mut out = vec![0u16; b.len() * 2];
    // SAFETY: the buffers and their lengths are given.
    let n = unsafe { MultiByteToWideChar(CP_OEMCP, 0, b.as_ptr(), b.len() as i32, out.as_mut_ptr(), out.len() as i32) };
    String::from_utf16_lossy(&out[..n.max(0) as usize])
}

#[cfg(not(windows))]
fn oem(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// What a running repair says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Text(String),
    /// It ended: its exit code (None: it could not start, with why).
    Done(Result<i32, String>),
}

/// Starts `task` and sends what it writes, then how it ended.
pub fn run(task: &Task, send: impl Fn(Event) + Send + Sync + 'static) {
    let (exe, args, input) = task.command();
    run_command(exe, args, input, send);
}

/// Starts a program in a thread and sends what it writes, then how it ended.
pub fn run_command(exe: String, args: Vec<String>, input: Option<&'static str>, send: impl Fn(Event) + Send + Sync + 'static) {
    std::thread::spawn(move || {
        let mut cmd = std::process::Command::new(&exe);
        cmd.args(&args)
            .stdin(if input.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() })
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return send(Event::Done(Err(e.to_string()))),
        };
        if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
            use std::io::Write;
            let _ = stdin.write_all(text.as_bytes());
        }
        let send = std::sync::Arc::new(send);
        let pipe = |r: Option<Box<dyn std::io::Read + Send>>, send: std::sync::Arc<dyn Fn(Event) + Send + Sync>| {
            std::thread::spawn(move || {
                let Some(mut r) = r else { return };
                let mut d = Decoder::default();
                let mut buf = [0u8; 4096];
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let t = d.decode(&buf[..n]);
                    if !t.is_empty() {
                        send(Event::Text(t));
                    }
                }
            })
        };
        let s1: std::sync::Arc<dyn Fn(Event) + Send + Sync> = send.clone();
        let out = pipe(child.stdout.take().map(|o| Box::new(o) as Box<dyn std::io::Read + Send>), s1);
        let s2: std::sync::Arc<dyn Fn(Event) + Send + Sync> = send.clone();
        let err = pipe(child.stderr.take().map(|o| Box::new(o) as Box<dyn std::io::Read + Send>), s2);
        let _ = out.join();
        let _ = err.join();
        let code = child.wait().map(|s| s.code().unwrap_or(-1)).map_err(|e| e.to_string());
        send(Event::Done(code));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands() {
        assert_eq!(Task::Sfc.command_line(), "sfc /scannow");
        assert_eq!(Task::Dism.command_line(), "dism /Online /Cleanup-Image /RestoreHealth");
        assert_eq!(Task::Check('E').command_line(), "chkdsk E: /scan");
        let (_, args, input) = Task::Fix('C').command();
        assert_eq!((args, input), (vec!["C:".to_string(), "/spotfix".to_string()], Some("Y\r\n")));
        assert!(Task::StartMenu.command().1.last().unwrap().contains("Add-AppxPackage -DisableDevelopmentMode -Register"));
        assert_eq!(check_result(0), CheckResult::Clean);
        assert_eq!(check_result(3), CheckResult::NeedsFix);
        assert_eq!(check_result(7), CheckResult::Other(7));
    }

    #[test]
    fn progress_and_lines() {
        assert_eq!(percent("Verification 45% complete."), Some(45.0));
        assert_eq!(percent("[==========                 18.4%                          ]"), Some(18.4));
        assert_eq!(percent("Fase 1: 12,5% concluído"), Some(12.5));
        assert_eq!(percent("no number %"), None);
        assert_eq!(percent("250%"), None);
        let mut o = Output::default();
        o.feed("Beginning system scan.\r\n\r\n\r\nVerification 1% complete.\rVerification 2");
        o.feed("% complete.\r");
        assert_eq!(o.all(), vec!["Beginning system scan.", "", "Verification 2% complete."]);
        assert_eq!(o.progress(), Some(2.0));
        o.feed("Verification 100% complete.\r\nWindows Resource Protection did not find any integrity violations.\r\n");
        assert_eq!(o.all().last(), Some(&"Windows Resource Protection did not find any integrity violations."));
        assert!(o.all().contains(&"Verification 100% complete."));
    }

    #[test]
    fn decoding() {
        // What sfc writes: UTF-16LE, cut in odd places.
        let wide: Vec<u8> = "Verificação 5%\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let mut d = Decoder::default();
        let a = d.decode(&wide[..7]);
        let b = d.decode(&wide[7..]);
        assert_eq!(format!("{a}{b}"), "Verificação 5%\r\n");
        // What chkdsk and DISM write: the console's code page (ASCII here).
        let mut d = Decoder::default();
        assert_eq!(
            d.decode(b"Stage 1: Examining basic file system structure ...\r\n"),
            "Stage 1: Examining basic file system structure ...\r\n"
        );
        // One byte: wait for the next to decide.
        let mut d = Decoder::default();
        assert_eq!(d.decode(b"S"), "");
        assert_eq!(d.decode(b"tage"), "Stage");
    }

    /// Runs a harmless command through the same path as the repairs (which need administrator rights).
    #[cfg(windows)]
    #[test]
    fn runs_a_tool_and_reads_it() {
        let (send, recv) = std::sync::mpsc::channel();
        let send = std::sync::Mutex::new(send);
        run_command("cmd.exe".into(), vec!["/c".into(), "echo 42% done& exit 3".into()], None, move |e| {
            let _ = send.lock().unwrap().send(e);
        });
        let mut o = Output::default();
        let code = loop {
            match recv.recv_timeout(std::time::Duration::from_secs(20)).expect("an event") {
                Event::Text(t) => o.feed(&t),
                Event::Done(c) => break c,
            }
        };
        assert_eq!(code, Ok(3));
        assert_eq!(o.progress(), Some(42.0), "{:?}", o.all());
    }
}
