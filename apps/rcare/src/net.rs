//! Internet Booster: Windows' TCP values and each connection's DNS, as
//! Windows reports them (PowerShell's own objects, the same words in every
//! language). The user's choices (2026-11-22): TCP is shown and can only be
//! put back to Windows' own values — no "boost" tweaks; the DNS can be set
//! to Automatic or a known service. Both changes need administrator rights
//! and are written first to a copy: a `.cmd` with the `netsh` lines that put
//! back what was there.

/// One TCP value: its name in Windows, and what it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tcp {
    pub auto_tuning: String,
    pub ecn: String,
    pub timestamps: String,
    pub initial_rto: String,
    pub congestion: String,
    pub rss: String,
    pub rsc: String,
}

impl Tcp {
    /// The values that differ from Windows' own, with what Windows' is: (name, now, Windows').
    /// ECN, timestamps and the initial timeout change with Windows' version: only shown.
    pub fn changed(&self) -> Vec<(&'static str, &str, &'static str)> {
        let mut out = Vec::new();
        for (name, now, want) in [
            ("Receive window auto-tuning", self.auto_tuning.as_str(), "Normal"),
            ("Receive-side scaling", self.rss.as_str(), "Enabled"),
            ("Receive segment coalescing", self.rsc.as_str(), "Enabled"),
        ] {
            if !now.is_empty() && !now.eq_ignore_ascii_case(want) {
                out.push((name, now, want));
            }
        }
        out
    }

    /// The `netsh` lines that set these values again (the copy before a change).
    pub fn netsh_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let word = |v: &str| v.trim().to_lowercase();
        let ok = |v: &str, allowed: &[&str]| allowed.contains(&word(v).as_str());
        if ok(&self.auto_tuning, &["normal", "disabled", "highlyrestricted", "restricted", "experimental"]) {
            out.push(format!("netsh int tcp set global autotuninglevel={}", word(&self.auto_tuning)));
        }
        if ok(&self.ecn, &["enabled", "disabled"]) {
            out.push(format!("netsh int tcp set global ecncapability={}", word(&self.ecn)));
        }
        if ok(&self.timestamps, &["enabled", "disabled"]) {
            out.push(format!("netsh int tcp set global timestamps={}", word(&self.timestamps)));
        }
        if ok(&self.rss, &["enabled", "disabled"]) {
            out.push(format!("netsh int tcp set global rss={}", word(&self.rss)));
        }
        if ok(&self.rsc, &["enabled", "disabled"]) {
            out.push(format!("netsh int tcp set global rsc={}", word(&self.rsc)));
        }
        if self.initial_rto.trim().parse::<u32>().is_ok() {
            out.push(format!("netsh int tcp set global initialRto={}", self.initial_rto.trim()));
        }
        out
    }
}

/// Windows' own TCP values, as `netsh` sets them.
pub const RESTORE: [&[&str]; 5] = [
    &["int", "tcp", "set", "global", "autotuninglevel=normal"],
    &["int", "tcp", "set", "global", "rss=enabled"],
    &["int", "tcp", "set", "global", "rsc=enabled"],
    &["int", "tcp", "set", "global", "ecncapability=default"],
    &["int", "tcp", "set", "global", "timestamps=default"],
];

/// A connection that is up, and its DNS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub index: u32,
    pub name: String,
    pub device: String,
    /// The servers were set by hand (not given by the router).
    pub manual: bool,
    pub v4: Vec<String>,
    pub v6: Vec<String>,
}

impl Connection {
    /// The known service these servers are, if any.
    pub fn provider(&self) -> Option<Provider> {
        Provider::ALL.into_iter().find(|p| self.v4.first().is_some_and(|a| p.v4().contains(&a.as_str())))
    }

    /// The `netsh` lines that put this connection's DNS back as it is now.
    pub fn netsh_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let name = self.name.replace('"', "");
        for (family, list) in [("ipv4", &self.v4), ("ipv6", &self.v6)] {
            if !self.manual || list.is_empty() {
                out.push(format!("netsh interface {family} set dnsservers name=\"{name}\" source=dhcp"));
                continue;
            }
            out.push(format!("netsh interface {family} set dnsservers name=\"{name}\" source=static address={} register=primary", list[0]));
            for (i, a) in list.iter().enumerate().skip(1) {
                out.push(format!("netsh interface {family} add dnsservers name=\"{name}\" address={a} index={}", i + 1));
            }
        }
        out
    }
}

/// What the DNS can be set to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    /// The router's (automatic).
    Automatic,
    Cloudflare,
    Quad9,
    Google,
}

impl Provider {
    pub const ALL: [Provider; 4] = [Provider::Automatic, Provider::Cloudflare, Provider::Quad9, Provider::Google];

    pub fn name(self) -> &'static str {
        match self {
            Provider::Automatic => "Automatic",
            Provider::Cloudflare => "Cloudflare",
            Provider::Quad9 => "Quad9",
            Provider::Google => "Google",
        }
    }

    pub fn v4(self) -> &'static [&'static str] {
        match self {
            Provider::Automatic => &[],
            Provider::Cloudflare => &["1.1.1.1", "1.0.0.1"],
            Provider::Quad9 => &["9.9.9.9", "149.112.112.112"],
            Provider::Google => &["8.8.8.8", "8.8.4.4"],
        }
    }

    pub fn v6(self) -> &'static [&'static str] {
        match self {
            Provider::Automatic => &[],
            Provider::Cloudflare => &["2606:4700:4700::1111", "2606:4700:4700::1001"],
            Provider::Quad9 => &["2620:fe::fe", "2620:fe::9"],
            Provider::Google => &["2001:4860:4860::8888", "2001:4860:4860::8844"],
        }
    }

    /// The PowerShell that sets it on the connection `index`.
    pub fn script(self, index: u32) -> String {
        if self == Provider::Automatic {
            return format!("Set-DnsClientServerAddress -InterfaceIndex {index} -ResetServerAddresses -ErrorAction Stop; 'ok'");
        }
        let all: Vec<String> = self.v4().iter().chain(self.v6()).map(|a| format!("'{a}'")).collect();
        format!("Set-DnsClientServerAddress -InterfaceIndex {index} -ServerAddresses ({}) -ErrorAction Stop; 'ok'", all.join(","))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Network {
    pub tcp: Option<Tcp>,
    pub connections: Vec<Connection>,
}

/// The PowerShell that asks, one fact a line (tab-separated).
pub const SCRIPT: &str = r#"$ErrorActionPreference='SilentlyContinue'
$t=Get-NetTCPSetting -SettingName Internet
$o=Get-NetOffloadGlobalSetting
if($t){ "tcp`t$($t.AutoTuningLevelLocal)`t$($t.EcnCapability)`t$($t.Timestamps)`t$($t.InitialRto)`t$($t.CongestionProvider)`t$($o.ReceiveSideScaling)`t$($o.ReceiveSegmentCoalescing)" }
foreach($a in Get-NetAdapter | Where-Object Status -eq 'Up'){
  $g=$a.InterfaceGuid
  $m4=(Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\$g").NameServer
  $m6=(Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters\Interfaces\$g").NameServer
  $v4=(Get-DnsClientServerAddress -InterfaceIndex $a.ifIndex -AddressFamily IPv4).ServerAddresses -join ','
  $v6=(Get-DnsClientServerAddress -InterfaceIndex $a.ifIndex -AddressFamily IPv6).ServerAddresses -join ','
  "dns`t$($a.ifIndex)`t$($a.Name)`t$($a.InterfaceDescription)`t$(-not [string]::IsNullOrWhiteSpace("$m4$m6"))`t$v4`t$v6"
}"#;

fn list(s: &str) -> Vec<String> {
    s.split([',', ' ']).map(str::trim).filter(|a| !a.is_empty()).map(str::to_string).collect()
}

/// What `SCRIPT` printed, read.
pub fn parse(text: &str) -> Network {
    let mut n = Network::default();
    for line in text.lines() {
        let f: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
        match f.as_slice() {
            ["tcp", at, ecn, ts, rto, cc, rss, rsc, ..] => {
                let s = |v: &str| v.trim().to_string();
                n.tcp = Some(Tcp {
                    auto_tuning: s(at),
                    ecn: s(ecn),
                    timestamps: s(ts),
                    initial_rto: s(rto),
                    congestion: s(cc),
                    rss: s(rss),
                    rsc: s(rsc),
                });
            }
            ["dns", index, name, device, manual, v4, rest @ ..] => {
                let Ok(index) = index.trim().parse() else { continue };
                // IPv6's link-local servers (fec0::…) are Windows' own placeholders.
                let v6 = list(rest.first().copied().unwrap_or("")).into_iter().filter(|a| !a.to_lowercase().starts_with("fec0:")).collect();
                n.connections.push(Connection {
                    index,
                    name: name.trim().to_string(),
                    device: device.trim().to_string(),
                    manual: manual.trim().eq_ignore_ascii_case("true"),
                    v4: list(v4),
                    v6,
                });
            }
            _ => {}
        }
    }
    n
}

/// Asks Windows. None: PowerShell could not be run.
pub fn read() -> Option<Network> {
    crate::ps::run(SCRIPT).map(|t| parse(&t))
}

/// The copy's text: a `.cmd` that, run as administrator, puts `lines` back.
pub fn copy_text(what: &str, lines: &[String]) -> String {
    let mut s = format!("@echo off\r\nrem rcare: {what}, as they were. Run as administrator to put them back.\r\n");
    for l in lines {
        s.push_str(l);
        s.push_str("\r\n");
    }
    s
}

/// What can be changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Windows' own TCP values again.
    RestoreTcp,
    /// The DNS of a connection.
    Dns { index: u32, provider: Provider },
}

/// Writes the copy to `copy` (`.cmd`), then changes. Err: what went wrong (nothing is changed when the copy fails).
pub fn apply(change: &Change, net: &Network, copy: &std::path::Path) -> Result<(), String> {
    let (what, lines) = match change {
        Change::RestoreTcp => ("TCP values", net.tcp.as_ref().map(Tcp::netsh_lines).unwrap_or_default()),
        Change::Dns { index, .. } => {
            ("DNS servers", net.connections.iter().find(|c| c.index == *index).map(Connection::netsh_lines).unwrap_or_default())
        }
    };
    if let Some(dir) = copy.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(copy, copy_text(what, &lines)).map_err(|e| e.to_string())?;
    match change {
        Change::RestoreTcp => {
            for args in RESTORE {
                match crate::ps::command("netsh.exe", args) {
                    Some((true, _)) => {}
                    Some((false, text)) => return Err(crate::ps::last_line(&text)),
                    None => return Err("netsh".into()),
                }
            }
            Ok(())
        }
        Change::Dns { index, provider } => crate::ps::run(&provider.script(*index))
            .filter(|t| t.contains("ok"))
            .map(|_| ())
            .ok_or_else(|| "Set-DnsClientServerAddress".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERE: &str = "tcp\tNormal\tDisabled\tDisabled\t1000\tCUBIC\tEnabled\tEnabled\r\n\
dns\t14\tEthernet\tIntel(R) Ethernet Connection (2) I219-V\tFalse\t192.168.1.1\tfec0:0:0:ffff::1,fec0:0:0:ffff::2\r\n";

    #[test]
    fn what_windows_says() {
        let n = parse(HERE);
        let t = n.tcp.clone().unwrap();
        assert_eq!((t.auto_tuning.as_str(), t.congestion.as_str(), t.rsc.as_str()), ("Normal", "CUBIC", "Enabled"));
        assert!(t.changed().is_empty());
        let c = &n.connections[0];
        assert_eq!((c.index, c.name.as_str(), c.manual), (14, "Ethernet", false));
        assert_eq!((c.v4.clone(), c.v6.is_empty()), (vec!["192.168.1.1".to_string()], true));
        assert_eq!(c.provider(), None);
        assert_eq!(
            c.netsh_lines(),
            vec![
                "netsh interface ipv4 set dnsservers name=\"Ethernet\" source=dhcp".to_string(),
                "netsh interface ipv6 set dnsservers name=\"Ethernet\" source=dhcp".to_string()
            ]
        );
    }

    #[test]
    fn changed_values_and_their_copy() {
        let mut t = parse(HERE).tcp.unwrap();
        t.auto_tuning = "Disabled".into();
        t.rss = "Disabled".into();
        assert_eq!(
            t.changed(),
            vec![("Receive window auto-tuning", "Disabled", "Normal"), ("Receive-side scaling", "Disabled", "Enabled")]
        );
        let lines = t.netsh_lines();
        assert!(lines.contains(&"netsh int tcp set global autotuninglevel=disabled".to_string()));
        assert!(lines.contains(&"netsh int tcp set global initialRto=1000".to_string()));
        t.timestamps = "Allowed".into();
        assert!(!t.netsh_lines().iter().any(|l| l.contains("timestamps")), "a word netsh does not take is left out");
        let text = copy_text("TCP values", &lines);
        assert!(text.starts_with("@echo off\r\n") && text.contains("autotuninglevel=disabled\r\n"));
    }

    #[test]
    fn dns_by_hand_and_providers() {
        let n = parse("dns\t7\tWi-Fi \"home\"\tIntel Wi-Fi\tTrue\t1.1.1.1,1.0.0.1\t2606:4700:4700::1111\n");
        let c = &n.connections[0];
        assert!(c.manual && c.provider() == Some(Provider::Cloudflare));
        let lines = c.netsh_lines();
        assert_eq!(lines[0], "netsh interface ipv4 set dnsservers name=\"Wi-Fi home\" source=static address=1.1.1.1 register=primary");
        assert_eq!(lines[1], "netsh interface ipv4 add dnsservers name=\"Wi-Fi home\" address=1.0.0.1 index=2");
        assert!(lines[2].contains("ipv6") && lines[2].contains("2606:4700:4700::1111"));
        assert!(Provider::Quad9
            .script(7)
            .contains("-InterfaceIndex 7 -ServerAddresses ('9.9.9.9','149.112.112.112','2620:fe::fe','2620:fe::9')"));
        assert!(Provider::Automatic.script(7).contains("-ResetServerAddresses"));
        assert!(parse("dns\tx\tA\tB\tTrue\t1.1.1.1\t\n").connections.is_empty());
    }

    /// What this PC says.
    #[test]
    #[ignore]
    fn here() {
        println!("{:#?}", read());
    }
}
