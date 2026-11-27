//! The scripts of manifests (`pre_install`, `installer`, `uninstaller`,
//! `post_uninstall`…), run by Scoop's own code. One PowerShell per app loads
//! the `lib\*.ps1` of the installation, sets the variables the scripts see,
//! and then runs each command it is sent, in order, in the same session (as
//! in Scoop, what one script leaves in `$global:` the next one finds).
//!
//! The commands come and the answers go through two anonymous pipes. The
//! console stays the user's: the scripts write to it and `Read-Host` reads
//! from it. A script that ends PowerShell (`exit`, `abort`, an error that
//! stops it) ends the command as it ends Scoop's.

use crate::install::Scoop;
use std::path::Path;

/// A text for PowerShell, in single quotes.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// `$true` or `$false`.
pub fn boolean(value: bool) -> &'static str {
    if value {
        "$true"
    } else {
        "$false"
    }
}

/// The libraries `scoop.ps1` loads, then those of the command.
pub const UNINSTALL_LIBS: [&str; 12] =
    ["core", "buckets", "commands", "help", "getopt", "manifest", "system", "install", "download", "shortcuts", "psmodules", "versions"];

/// Those of `scoop-install.ps1` (and of `scoop-update.ps1`, which installs).
pub const INSTALL_LIBS: [&str; 16] = [
    "core",
    "buckets",
    "commands",
    "help",
    "getopt",
    "json",
    "autoupdate",
    "manifest",
    "system",
    "install",
    "download",
    "decompress",
    "shortcuts",
    "psmodules",
    "versions",
    "depends",
];

/// What PowerShell runs before the first command: the libraries of the
/// installation, then `setup` (the variables of the command).
#[cfg_attr(not(windows), allow(dead_code))]
fn script(scoop: &Scoop, libs: &[&str], setup: &str, handles: (usize, usize)) -> String {
    let lib = scoop.app("scoop", false).join("current").join("lib");
    let mut s = String::from("Set-StrictMode -Off\n");
    s += &format!("$env:SCOOP = {}\n", quote(&scoop.root.display().to_string()));
    for name in libs {
        let file = lib.join(format!("{name}.ps1"));
        let file = quote(&file.display().to_string());
        s += &format!("if (Test-Path -LiteralPath {file}) {{ . {file} }}\n");
    }
    s += setup;
    s += "\n";
    s += &format!(
        "$__rscoop_reader = New-Object System.IO.StreamReader((New-Object System.IO.Pipes.AnonymousPipeClientStream([System.IO.Pipes.PipeDirection]::In, '{}')))\n",
        handles.0
    );
    s += &format!(
        "$__rscoop_writer = New-Object System.IO.StreamWriter((New-Object System.IO.Pipes.AnonymousPipeClientStream([System.IO.Pipes.PipeDirection]::Out, '{}')))\n",
        handles.1
    );
    s += "$__rscoop_writer.AutoFlush = $true\n";
    s += "$__rscoop_writer.WriteLine('ready')\n";
    s += "while ($null -ne ($__rscoop_line = $__rscoop_reader.ReadLine())) {\n";
    s += "    if ($__rscoop_line -eq 'quit') { break }\n";
    s += "    . ([scriptblock]::Create($__rscoop_line))\n";
    s += "    $__rscoop_writer.WriteLine('done')\n";
    s += "}\n";
    s += "exit 0\n";
    s
}

/// A PowerShell waiting for commands.
pub struct Host {
    #[cfg(windows)]
    child: std::process::Child,
    #[cfg(windows)]
    to: std::fs::File,
    #[cfg(windows)]
    from: std::io::BufReader<std::fs::File>,
}

/// Why a command did not finish.
#[derive(Debug, PartialEq, Eq)]
pub enum Stop {
    /// PowerShell could not be started: why.
    Start(String),
    /// PowerShell ended, with this exit code (as Scoop would have).
    Ended(i32),
}

#[cfg(windows)]
mod pipes {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::Pipes::CreatePipe;

    /// A pipe whose end for the child (the read end with `child_reads`) is
    /// inherited and the other is not: (ours, the child's).
    pub fn pipe(child_reads: bool) -> std::io::Result<(OwnedHandle, OwnedHandle)> {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        let (mut read, mut write): (HANDLE, HANDLE) = (std::ptr::null_mut(), std::ptr::null_mut());
        // SAFETY: out-parameters; each handle is owned once below.
        unsafe {
            if CreatePipe(&mut read, &mut write, &attributes, 0) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let (read, write) = (OwnedHandle::from_raw_handle(read), OwnedHandle::from_raw_handle(write));
            let (ours, theirs) = if child_reads { (write, read) } else { (read, write) };
            use std::os::windows::io::AsRawHandle;
            SetHandleInformation(ours.as_raw_handle(), HANDLE_FLAG_INHERIT, 0);
            Ok((ours, theirs))
        }
    }
}

impl Host {
    /// Starts `powershell` on the installation, with `setup` run after the
    /// libraries are loaded.
    #[cfg(windows)]
    pub fn start(powershell: &Path, scoop: &Scoop, libs: &[&str], setup: &str) -> Result<Host, Stop> {
        use std::io::BufRead;
        use std::os::windows::io::AsRawHandle;
        let start = |e: std::io::Error| Stop::Start(e.to_string());
        let (to, child_in) = pipes::pipe(true).map_err(start)?;
        let (from, child_out) = pipes::pipe(false).map_err(start)?;
        let handles = (child_in.as_raw_handle() as usize, child_out.as_raw_handle() as usize);
        let text = script(scoop, libs, setup, handles);
        let encoded: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let child = std::process::Command::new(powershell)
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &base64(&encoded)])
            .spawn()
            .map_err(start)?;
        // Only the child holds its ends now: when it ends, the pipe does.
        drop((child_in, child_out));
        let mut host = Host { child, to: std::fs::File::from(to), from: std::io::BufReader::new(std::fs::File::from(from)) };
        let mut line = String::new();
        match host.from.read_line(&mut line) {
            Ok(n) if n > 0 && line.trim() == "ready" => Ok(host),
            _ => Err(Stop::Ended(host.end())),
        }
    }

    #[cfg(not(windows))]
    pub fn start(_powershell: &Path, _scoop: &Scoop, _libs: &[&str], _setup: &str) -> Result<Host, Stop> {
        Err(Stop::Start("Windows only".into()))
    }

    /// Runs one line of PowerShell and waits for it.
    #[cfg(windows)]
    pub fn run(&mut self, command: &str) -> Result<(), Stop> {
        use std::io::{BufRead, Write};
        let line = command.replace(['\r', '\n'], " ");
        if writeln!(self.to, "{line}").and_then(|()| self.to.flush()).is_err() {
            return Err(Stop::Ended(self.end()));
        }
        let mut answer = String::new();
        match self.from.read_line(&mut answer) {
            Ok(n) if n > 0 => Ok(()),
            _ => Err(Stop::Ended(self.end())),
        }
    }

    #[cfg(not(windows))]
    pub fn run(&mut self, _command: &str) -> Result<(), Stop> {
        Err(Stop::Start("Windows only".into()))
    }

    /// Waits for PowerShell to end: its exit code.
    #[cfg(windows)]
    fn end(&mut self) -> i32 {
        self.child.wait().ok().and_then(|s| s.code()).unwrap_or(1)
    }

    /// Lets PowerShell end.
    #[cfg(windows)]
    pub fn finish(mut self) -> i32 {
        use std::io::Write;
        let _ = writeln!(self.to, "quit");
        self.end()
    }

    #[cfg(not(windows))]
    pub fn finish(self) -> i32 {
        0
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_for_powershell() {
        assert_eq!(quote("it's"), "'it''s'");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    /// A made-up Scoop library: the hook writes what it sees to a file.
    #[cfg(windows)]
    #[test]
    fn scripts_run_in_one_session_and_can_end_it() {
        let t = crate::testing::Tree::new("hooks");
        t.file(
            "root/apps/scoop/current/lib/core.ps1",
            "function Note($text) { Add-Content -Path (Join-Path $env:SCOOP 'notes.txt') -Value $text }\n",
        );
        let scoop = t.scoop();
        let powershell = Path::new("powershell.exe");
        let mut host = Host::start(powershell, &scoop, &["core"], "$app = 'x'").unwrap();
        host.run("$global:kept = 'k'; Note \"$app $env:SCOOP\"").unwrap();
        host.run("Note $kept").unwrap();
        assert_eq!(host.finish(), 0);
        let notes = std::fs::read_to_string(t.dir("root").join("notes.txt")).unwrap();
        assert_eq!(notes, format!("x {}\r\nk\r\n", t.dir("root").display()));

        let mut host = Host::start(powershell, &scoop, &["core"], "").unwrap();
        assert_eq!(host.run("exit 7"), Err(Stop::Ended(7)));
        // An error that stops the script ends it too.
        let mut host = Host::start(powershell, &scoop, &["core"], "").unwrap();
        assert_eq!(host.run("throw 'no'"), Err(Stop::Ended(1)));
        assert!(matches!(Host::start(powershell, &scoop, &["core"], "throw 'x'"), Err(Stop::Ended(1))));
    }
}
