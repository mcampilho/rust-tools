//! System Information: what the computer is and how its parts are, as
//! Windows reports it (one hidden PowerShell: CIM, `Get-PhysicalDisk`,
//! `Get-PnpDevice`), and Fast Startup (`HiberbootEnabled`). Drivers are
//! not looked for online: devices with a problem are listed and Windows
//! Update's optional updates are opened (the user's choice, 2026-11-23).

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Windows {
    pub edition: String,
    /// `22H2`, as Windows names its releases.
    pub release: String,
    /// `19045.6466`.
    pub build: String,
    pub arch: String,
    pub installed: String,
    pub uptime_hours: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Computer {
    pub name: String,
    pub maker: String,
    pub model: String,
    pub board: String,
    pub bios: String,
    pub memory: u64,
    pub cpus: Vec<Cpu>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cpu {
    pub name: String,
    pub cores: u32,
    pub threads: u32,
    pub mhz: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gpu {
    pub name: String,
    pub driver: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disk {
    pub name: String,
    /// `SSD`, `HDD`, `Unspecified`.
    pub media: String,
    /// `NVMe`, `SATA`, `USB`…
    pub bus: String,
    pub size: u64,
    /// `Healthy`, `Warning`, `Unhealthy` (Windows' words).
    pub health: String,
}

impl Disk {
    pub fn healthy(&self) -> bool {
        self.health.eq_ignore_ascii_case("Healthy")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub class: String,
    /// `CM_PROB_…`, Windows' own name of the problem.
    pub problem: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Battery {
    pub name: String,
    pub charge: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Info {
    pub windows: Windows,
    pub computer: Computer,
    pub gpus: Vec<Gpu>,
    pub disks: Vec<Disk>,
    pub devices: Vec<Device>,
    pub batteries: Vec<Battery>,
    /// Fast Startup is on (None: unknown).
    pub fast_startup: Option<bool>,
}

impl Info {
    /// What the computer is called by its maker, or its board when the maker left the placeholders.
    pub fn model(&self) -> String {
        let c = &self.computer;
        let placeholder = |s: &str| {
            let l = s.to_lowercase();
            l.is_empty()
                || l.contains("system product name")
                || l.contains("system manufacturer")
                || l.contains("to be filled")
                || l == "default string"
        };
        if !placeholder(&c.model) {
            if placeholder(&c.maker) {
                c.model.clone()
            } else {
                format!("{} {}", c.maker, c.model)
            }
        } else {
            c.board.clone()
        }
    }

    /// Everything as text (to copy).
    pub fn text(&self, tr: impl Fn(&str) -> String) -> String {
        let w = &self.windows;
        let c = &self.computer;
        let mut s = format!("{} {} ({}), {}\n", w.edition, w.release, w.build, w.arch);
        s += &format!("{}: {}\n{}: {}\n", tr("Computer"), c.name, tr("Model"), self.model());
        for p in &c.cpus {
            s += &format!("{}: {} ({} {}, {} {})\n", tr("Processor"), p.name, p.cores, tr("cores"), p.threads, tr("threads"));
        }
        s += &format!("{}: {}\n", tr("Memory"), rsize::format::size(c.memory));
        s += &format!("{}: {}\n", tr("Motherboard"), c.board);
        s += &format!("BIOS: {}\n", c.bios);
        for g in &self.gpus {
            s += &format!("{}: {} ({})\n", tr("Graphics"), g.name, g.driver);
        }
        for d in &self.disks {
            s += &format!("{}: {} — {} {}, {}, {}\n", tr("Drive"), d.name, d.media, d.bus, rsize::format::size(d.size), d.health);
        }
        for d in &self.devices {
            s += &format!("{}: {} ({}) — {}\n", tr("Device with a problem"), d.name, d.class, d.problem);
        }
        s
    }
}

/// What Windows' problem name means (a translation key).
pub fn problem_text(code: &str) -> &'static str {
    match code {
        "CM_PROB_FAILED_START" => "It could not start",
        "CM_PROB_DISABLED" => "Turned off",
        "CM_PROB_FAILED_INSTALL" | "CM_PROB_NOT_CONFIGURED" | "CM_PROB_REINSTALL" => "Its driver is not installed",
        "CM_PROB_DRIVER_FAILED_LOAD" | "CM_PROB_DRIVER_FAILED_PRIOR_UNLOAD" => "Its driver did not load",
        "CM_PROB_PHANTOM" => "Not connected",
        _ => "It has a problem",
    }
}

/// The PowerShell that asks, one fact a line (tab-separated).
pub const SCRIPT: &str = r#"$ErrorActionPreference='SilentlyContinue'
$o=Get-CimInstance Win32_OperatingSystem
$cv=Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
"os`t$($o.Caption)`t$($cv.DisplayVersion)`t$($o.BuildNumber).$($cv.UBR)`t$($o.OSArchitecture)`t$(if($o.InstallDate){$o.InstallDate.ToString('yyyy-MM-dd')})`t$(if($o.LastBootUpTime){[int]((Get-Date)-$o.LastBootUpTime).TotalHours})"
$c=Get-CimInstance Win32_ComputerSystem
$b=Get-CimInstance Win32_BaseBoard; $bi=Get-CimInstance Win32_BIOS
"pc`t$($c.Name)`t$($c.Manufacturer)`t$($c.Model)`t$($b.Manufacturer) $($b.Product)`t$($bi.SMBIOSBIOSVersion) $(if($bi.ReleaseDate){$bi.ReleaseDate.ToString('yyyy-MM-dd')})`t$($c.TotalPhysicalMemory)"
foreach($p in Get-CimInstance Win32_Processor){ "cpu`t$($p.Name.Trim())`t$($p.NumberOfCores)`t$($p.NumberOfLogicalProcessors)`t$($p.MaxClockSpeed)" }
foreach($g in Get-CimInstance Win32_VideoController){ "gpu`t$($g.Name)`t$($g.DriverVersion)" }
foreach($k in Get-PhysicalDisk){ "disk`t$($k.FriendlyName)`t$($k.MediaType)`t$($k.BusType)`t$($k.Size)`t$($k.HealthStatus)" }
foreach($x in Get-CimInstance Win32_Battery){ "battery`t$($x.Name)`t$($x.EstimatedChargeRemaining)" }
foreach($v in Get-PnpDevice -PresentOnly | Where-Object Status -eq 'Error'){ "device`t$($v.FriendlyName)`t$($v.Class)`t$($v.Problem)" }
"fast`t$((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager\Power').HiberbootEnabled)""#;

/// What `SCRIPT` printed, read.
pub fn parse(text: &str) -> Info {
    let mut info = Info::default();
    let s = |v: &str| v.trim().to_string();
    for line in text.lines() {
        let f: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
        match f.as_slice() {
            ["os", edition, release, build, arch, installed, up, ..] => {
                info.windows = Windows {
                    edition: s(edition),
                    release: s(release),
                    build: s(build).trim_end_matches('.').to_string(),
                    arch: s(arch),
                    installed: s(installed),
                    uptime_hours: up.trim().parse().ok(),
                }
            }
            ["pc", name, maker, model, board, bios, memory, ..] => {
                let cpus = std::mem::take(&mut info.computer.cpus);
                info.computer = Computer {
                    name: s(name),
                    maker: s(maker),
                    model: s(model),
                    board: s(board),
                    bios: s(bios),
                    memory: memory.trim().parse().unwrap_or(0),
                    cpus,
                };
            }
            ["cpu", name, cores, threads, mhz, ..] => info.computer.cpus.push(Cpu {
                name: s(name),
                cores: cores.trim().parse().unwrap_or(0),
                threads: threads.trim().parse().unwrap_or(0),
                mhz: mhz.trim().parse().unwrap_or(0),
            }),
            ["gpu", name, driver, ..] if !name.trim().is_empty() => info.gpus.push(Gpu { name: s(name), driver: s(driver) }),
            ["disk", name, media, bus, size, health, ..] => info.disks.push(Disk {
                name: s(name),
                media: s(media),
                bus: s(bus),
                size: size.trim().parse().unwrap_or(0),
                health: s(health),
            }),
            ["battery", name, charge, ..] => info.batteries.push(Battery { name: s(name), charge: charge.trim().parse().ok() }),
            ["device", name, class, problem, ..] if !name.trim().is_empty() => {
                info.devices.push(Device { name: s(name), class: s(class), problem: s(problem) })
            }
            // Absent means on (Windows' default).
            ["fast", v, ..] => info.fast_startup = Some(v.trim() != "0"),
            _ => {}
        }
    }
    info
}

/// Asks Windows. None: PowerShell could not be run.
pub fn read() -> Option<Info> {
    crate::ps::run(SCRIPT).map(|t| parse(&t))
}

/// Turns Fast Startup on or off (administrator rights).
#[cfg(windows)]
pub fn set_fast_startup(on: bool) -> Result<(), String> {
    use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_LOCAL_MACHINE, REG_DWORD};
    let key: Vec<u16> = "SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Power".encode_utf16().chain([0]).collect();
    let name: Vec<u16> = "HiberbootEnabled".encode_utf16().chain([0]).collect();
    let v: u32 = u32::from(on);
    // SAFETY: NUL-terminated names; the data is a DWORD with its size.
    let rc = unsafe { RegSetKeyValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), name.as_ptr(), REG_DWORD, (&v as *const u32).cast(), 4) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(rc as i32).to_string())
    }
}

#[cfg(not(windows))]
pub fn set_fast_startup(_on: bool) -> Result<(), String> {
    Err("Windows only".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What this PC printed.
    const HERE: &str = "os\tMicrosoft Windows 10 Pro N\t22H2\t19045.6466\t64-bit\t2026-10-14\t38\r\n\
pc\tMCDESK1\tSystem manufacturer\tSystem Product Name\tASUSTeK COMPUTER INC. MAXIMUS VIII HERO\t1402 2016-01-07\t34267713536\r\n\
cpu\tIntel(R) Core(TM) i7-6700K CPU @ 4.00GHz\t4\t8\t4001\r\n\
gpu\tNVIDIA GeForce GTX 960\t32.0.15.8278\r\n\
disk\tSamsung SSD 970 EVO 1TB\tSSD\tNVMe\t1000204886016\tHealthy\r\n\
disk\tST3500418AS\tHDD\tSATA\t500107862016\tWarning\r\n\
device\tGenesys Logic USB2.0 Card Reader\tUSB\tCM_PROB_FAILED_START\r\n\
fast\t1\r\n";

    #[test]
    fn what_windows_says() {
        let i = parse(HERE);
        assert_eq!((i.windows.release.as_str(), i.windows.build.as_str(), i.windows.uptime_hours), ("22H2", "19045.6466", Some(38)));
        assert_eq!(i.computer.cpus.len(), 1);
        assert_eq!((i.computer.cpus[0].cores, i.computer.cpus[0].threads), (4, 8));
        assert_eq!(i.computer.memory, 34_267_713_536);
        // The maker left the placeholders: the board says what it is.
        assert_eq!(i.model(), "ASUSTeK COMPUTER INC. MAXIMUS VIII HERO");
        assert_eq!(i.disks.iter().filter(|d| d.healthy()).count(), 1);
        assert_eq!(i.devices[0].problem, "CM_PROB_FAILED_START");
        assert_eq!(problem_text(&i.devices[0].problem), "It could not start");
        assert_eq!(i.fast_startup, Some(true));
        assert!(i.batteries.is_empty());
        let t = i.text(|s| s.to_string());
        assert!(t.contains("Processor: Intel(R) Core(TM) i7-6700K") && t.contains("Device with a problem: Genesys"));
    }

    #[test]
    fn makers_and_odd_lines() {
        let mut i = parse("pc\tLAPTOP\tLENOVO\t20L5\tLENOVO 20L5\tN24ET\t8000\nfast\t0\nfast\nbattery\tBAT0\t87\ngpu\t\t1\n");
        assert_eq!(i.model(), "LENOVO 20L5");
        assert_eq!(i.fast_startup, Some(false));
        assert_eq!(i.batteries[0].charge, Some(87));
        assert!(i.gpus.is_empty());
        i.computer.maker = "To be filled by O.E.M.".into();
        assert_eq!(i.model(), "20L5");
        assert_eq!(parse("fast\t\n").fast_startup, Some(true), "absent: Windows' default, on");
    }

    /// What this PC says.
    #[test]
    #[ignore]
    fn here() {
        println!("{:#?}", read());
    }
}
