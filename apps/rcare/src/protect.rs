//! Protect System: what protects the computer, as Windows itself reports it
//! — the antivirus and firewall products Windows Security knows (another
//! company's included), Microsoft Defender's state, Controlled folder access
//! (the "safe folder") and the Windows Firewall's profiles. No engine of our
//! own: the actions are Windows' tools (`MpCmdRun` for a quick scan and the
//! definitions, `netsh` and `Set-MpPreference` to turn on what is off). The
//! user's choice, 2026-11-22.

/// An antivirus or firewall product, as Windows Security lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Product {
    pub name: String,
    pub on: bool,
    /// Its definitions are up to date (antivirus).
    pub up_to_date: bool,
}

/// What `productState` says: (on, up to date). Its middle byte is the
/// scanner (0x10 on, 0x11 on and snoozed… 0x00/0x01 off), its last the
/// definitions (0x00 up to date, 0x10 out of date).
pub fn decode_state(state: u32) -> (bool, bool) {
    let scanner = (state >> 8) & 0xFF;
    let defs = state & 0xFF;
    (scanner & 0x10 != 0, defs == 0)
}

/// How Microsoft Defender runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// It is the antivirus.
    Active,
    /// Another antivirus is; Defender only looks on request (or not at all).
    Passive,
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Defender {
    pub mode: Mode,
    pub realtime: bool,
    /// Days since its definitions were updated.
    pub signature_age: Option<u32>,
    /// Days since its last quick scan.
    pub quick_scan_age: Option<u32>,
    pub tamper: bool,
}

/// Controlled folder access: what Windows Security calls ransomware protection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Folders {
    Off,
    On,
    /// It only notes what would have been blocked.
    Audit,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub antivirus: Vec<Product>,
    pub firewalls: Vec<Product>,
    pub defender: Option<Defender>,
    pub folders: Option<Folders>,
    /// The Windows Firewall's profiles (`Domain`, `Private`, `Public`) and whether each is on.
    pub profiles: Vec<(String, bool)>,
}

impl Status {
    /// An antivirus is on (Defender or another).
    pub fn antivirus_on(&self) -> bool {
        self.antivirus.iter().any(|p| p.on) || self.defender.as_ref().is_some_and(|d| d.mode == Mode::Active && d.realtime)
    }

    /// The Windows Firewall is on in every profile, or another firewall is on.
    pub fn firewall_on(&self) -> bool {
        (!self.profiles.is_empty() && self.profiles.iter().all(|(_, on)| *on)) || self.firewalls.iter().any(|p| p.on)
    }

    /// Defender is the antivirus: its scan and definitions can be used.
    pub fn defender_active(&self) -> bool {
        self.defender.as_ref().is_some_and(|d| d.mode == Mode::Active)
    }

    /// What is off and can be turned on from here: the Windows Firewall's profiles off,
    /// and Defender's real-time protection when Defender is the antivirus.
    pub fn off(&self) -> Vec<Fix> {
        let mut out: Vec<Fix> = self.profiles.iter().filter(|(_, on)| !on).map(|(n, _)| Fix::Profile(n.clone())).collect();
        if self.defender.as_ref().is_some_and(|d| d.mode == Mode::Active && !d.realtime) {
            out.push(Fix::Realtime);
        }
        out
    }
}

/// Something off to turn on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fix {
    /// A Windows Firewall profile.
    Profile(String),
    /// Defender's real-time protection.
    Realtime,
}

/// The PowerShell that asks, one fact a line (tab-separated).
pub const SCRIPT: &str = r#"$ErrorActionPreference='SilentlyContinue'
foreach($p in Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct){ "av`t$($p.displayName)`t$($p.productState)" }
foreach($p in Get-CimInstance -Namespace root/SecurityCenter2 -ClassName FirewallProduct){ "fw`t$($p.displayName)`t$($p.productState)" }
$m=Get-MpComputerStatus; if($m){ "mp`t$($m.AMRunningMode)`t$($m.RealTimeProtectionEnabled)`t$($m.AntivirusSignatureAge)`t$($m.QuickScanAge)`t$($m.IsTamperProtected)" }
$c=(Get-MpPreference).EnableControlledFolderAccess; if($c -ne $null){ "cfa`t$c" }
foreach($f in Get-NetFirewallProfile){ "profile`t$($f.Name)`t$($f.Enabled)" }"#;

fn yes(s: &str) -> bool {
    s.trim().eq_ignore_ascii_case("true") || s.trim() == "1"
}

/// A number of days, or None for Windows' "never" (65535, 4294967295…).
fn days(s: &str) -> Option<u32> {
    s.trim().parse::<u32>().ok().filter(|d| *d < 10_000)
}

/// What `SCRIPT` printed, read.
pub fn parse(text: &str) -> Status {
    let mut s = Status::default();
    for line in text.lines() {
        let f: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
        match f.as_slice() {
            ["av" | "fw", name, state, ..] if !name.trim().is_empty() => {
                let (on, up_to_date) = decode_state(state.trim().parse().unwrap_or(0));
                let p = Product { name: name.trim().to_string(), on, up_to_date };
                // Defender is described by its own line: not twice.
                if f[0] == "av" && p.name.eq_ignore_ascii_case("Windows Defender") {
                    continue;
                }
                if f[0] == "av" {
                    s.antivirus.push(p)
                } else {
                    s.firewalls.push(p)
                }
            }
            ["mp", mode, rt, sig, scan, tamper, ..] => {
                let m = mode.trim().to_lowercase();
                let mode = if m == "normal" {
                    Mode::Active
                } else if m.contains("passive") || m.contains("edr") {
                    Mode::Passive
                } else {
                    Mode::Off
                };
                s.defender =
                    Some(Defender { mode, realtime: yes(rt), signature_age: days(sig), quick_scan_age: days(scan), tamper: yes(tamper) });
            }
            ["cfa", v, ..] => {
                s.folders = match v.trim() {
                    "0" => Some(Folders::Off),
                    "1" => Some(Folders::On),
                    "2" | "3" | "4" | "5" | "6" => Some(Folders::Audit),
                    _ => None,
                }
            }
            ["profile", name, on, ..] if !name.trim().is_empty() => s.profiles.push((name.trim().to_string(), yes(on))),
            _ => {}
        }
    }
    s
}

/// Asks Windows. None: PowerShell could not be run.
pub fn read() -> Option<Status> {
    crate::ps::run(SCRIPT).map(|t| parse(&t))
}

/// What can be done, through Windows' own tools.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    QuickScan,
    UpdateDefinitions,
    TurnOn(Vec<Fix>),
}

/// Defender's command-line tool.
fn mpcmdrun() -> String {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    format!("{pf}\\Windows Defender\\MpCmdRun.exe")
}

/// The `netsh` profile word of a Windows Firewall profile.
pub fn netsh_profile(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "domain" => Some("domainprofile"),
        "private" => Some("privateprofile"),
        "public" => Some("publicprofile"),
        _ => None,
    }
}

/// Does `job` and waits (a quick scan takes minutes). Err: what the tool said.
pub fn run(job: &Job) -> Result<(), String> {
    let done = |r: Option<(bool, String)>| match r {
        Some((true, _)) => Ok(()),
        Some((false, text)) => Err(crate::ps::last_line(&text)),
        None => Err("could not be started".into()),
    };
    match job {
        Job::QuickScan => done(crate::ps::command(&mpcmdrun(), &["-Scan", "-ScanType", "1"])),
        Job::UpdateDefinitions => done(crate::ps::command(&mpcmdrun(), &["-SignatureUpdate"])),
        Job::TurnOn(fixes) => {
            for f in fixes {
                match f {
                    Fix::Profile(name) => {
                        let Some(p) = netsh_profile(name) else { continue };
                        done(crate::ps::command("netsh.exe", &["advfirewall", "set", p, "state", "on"]))?;
                    }
                    Fix::Realtime => {
                        crate::ps::run("Set-MpPreference -DisableRealtimeMonitoring $false; 'ok'")
                            .filter(|t| t.contains("ok"))
                            .ok_or_else(|| "Set-MpPreference".to_string())?;
                    }
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What this PC printed (Bitdefender and IObit on, Defender passive), plus a profile off.
    const HERE: &str = "av\tIObit Malware Fighter\t266240\r\nav\tWindows Defender\t393472\r\nav\tBitdefender Antivirus\t266240\r\n\
fw\tBitdefender Firewall\t266240\r\nmp\tNot running\tFalse\t65535\t4294967295\tFalse\r\n\
profile\tDomain\tTrue\r\nprofile\tPrivate\tTrue\r\nprofile\tPublic\tFalse\r\n";

    #[test]
    fn what_windows_says() {
        assert_eq!(decode_state(266240), (true, true));
        assert_eq!(decode_state(393472), (false, true));
        assert_eq!(decode_state(0x061110), (true, false));
        let s = parse(HERE);
        assert_eq!(s.antivirus.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["IObit Malware Fighter", "Bitdefender Antivirus"]);
        assert!(s.antivirus.iter().all(|p| p.on && p.up_to_date));
        assert_eq!(s.firewalls.len(), 1);
        let d = s.defender.clone().unwrap();
        assert_eq!((d.mode, d.realtime, d.signature_age, d.quick_scan_age), (Mode::Off, false, None, None));
        assert!(s.antivirus_on() && s.firewall_on() && !s.defender_active());
        assert_eq!(s.folders, None);
        // Defender is not the antivirus: only the firewall profile is offered.
        assert_eq!(s.off(), vec![Fix::Profile("Public".into())]);
    }

    #[test]
    fn defender_as_the_antivirus() {
        let s = parse("mp\tNormal\tFalse\t2\t9\tTrue\ncfa\t1\nprofile\tPublic\tTrue\nnonsense\nav\t\t1\n");
        let d = s.defender.clone().unwrap();
        assert_eq!((d.mode, d.realtime, d.signature_age, d.quick_scan_age, d.tamper), (Mode::Active, false, Some(2), Some(9), true));
        assert!(s.defender_active() && !s.antivirus_on());
        assert_eq!(s.folders, Some(Folders::On));
        assert_eq!(s.off(), vec![Fix::Realtime]);
        assert!(s.antivirus.is_empty());
        assert_eq!(parse("mp\tPassive Mode\tTrue\t1\t1\tFalse").defender.unwrap().mode, Mode::Passive);
        assert_eq!(parse("cfa\t2").folders, Some(Folders::Audit));
        assert_eq!(netsh_profile("Public"), Some("publicprofile"));
        assert_eq!(netsh_profile("Other"), None);
    }

    /// What this PC says.
    #[test]
    #[ignore]
    fn here() {
        println!("{:#?}", read());
    }
}
