//! Linux's packages: dpkg (Debian, Ubuntu, Mint), rpm (Fedora, openSUSE,
//! RHEL), Flatpak and Snap. dpkg's records are read from its own file
//! (`/var/lib/dpkg/status`, faster than `dpkg-query`); the others from what
//! their commands print.
//!
//! Packages installed only because another needs them (apt's
//! `Auto-Installed`), those the system cannot do without (`Priority:
//! required` or `important`), libraries, Flatpak's runtimes and Snap's
//! bases count as parts of the system: hidden unless asked for, as Windows'
//! system components are.
//!
//! The text readers below work on any system, for the tests.

use crate::inventory::{Problem, Program, Scope, Source};
use std::collections::HashSet;
use std::path::PathBuf;

/// A value of a program, by name.
pub fn value<'a>(p: &'a Program, name: &str) -> Option<&'a str> {
    p.values.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// The name a package manager knows the program by: dpkg's `name:arch`,
/// rpm's and Snap's name, Flatpak's ref.
pub fn package_id(p: &Program) -> &str {
    value(p, "Package").or_else(|| value(p, "Ref")).unwrap_or(&p.name)
}

/// The packages apt installed as another's need (`extended_states`).
pub fn parse_apt_auto(text: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    for block in text.split("\n\n") {
        let field = |name: &str| block.lines().find_map(|l| l.strip_prefix(name)).map(str::trim);
        if field("Auto-Installed:") == Some("1") {
            if let Some(p) = field("Package:") {
                out.insert(p.to_string());
                if let Some(a) = field("Architecture:") {
                    out.insert(format!("{p}:{a}"));
                }
            }
        }
    }
    out
}

/// A name and e-mail (`Ubuntu Developers <x@y>`) without the e-mail.
fn without_mail(s: &str) -> String {
    s.split('<').next().unwrap_or(s).trim().to_string()
}

/// Days since 1970 to a date.
fn date_of(secs: i64) -> Option<(i32, u32, u32)> {
    (secs > 0).then(|| rt_scan::format::civil((secs / 86_400) as i32))
}

/// A name that says it is a library or a part of another package.
pub fn part_name(name: &str) -> bool {
    let n = name.split(':').next().unwrap_or(name);
    (n.starts_with("lib") && !n.starts_with("libreoffice"))
        || ["-libs", "-common", "-data", "-filesystem", "-devel", "-dev", "-doc", "-l10n"].iter().any(|s| n.ends_with(s))
        || n.contains("-langpack-")
        || n.starts_with("fonts-")
        || n.starts_with("python3-")
        || n.starts_with("perl-")
        || n.starts_with("golang-")
}

/// dpkg's records: every package installed, or whose settings are all that
/// is left. `auto`: apt's `Auto-Installed` ones; `date`: when a package was
/// installed (its file list's time).
pub fn parse_dpkg(text: &str, auto: &HashSet<String>, date: &dyn Fn(&str) -> Option<i64>) -> Vec<Program> {
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let mut values: Vec<(String, String)> = Vec::new();
        for line in block.lines() {
            // Continuation lines (the long description, the files kept).
            if line.starts_with([' ', '\t']) {
                continue;
            }
            if let Some((k, v)) = line.split_once(':') {
                values.push((k.trim().to_string(), v.trim().to_string()));
            }
        }
        let get = |name: &str| values.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        let (Some(name), Some(status)) = (get("Package"), get("Status")) else { continue };
        let state = status.split_whitespace().last().unwrap_or("");
        let problems = match state {
            "installed" => Vec::new(),
            "config-files" => vec![Problem::ConfigOnly],
            "not-installed" => continue,
            // half-installed, unpacked, half-configured, triggers-awaited...
            _ => vec![Problem::InstallFailed],
        };
        let arch = get("Architecture").unwrap_or("");
        let spec = if arch.is_empty() || arch == "all" { name.to_string() } else { format!("{name}:{arch}") };
        let priority = get("Priority").unwrap_or("");
        let is_auto = auto.contains(name) || auto.contains(&spec);
        let section = get("Section").unwrap_or("");
        let system = problems.is_empty()
            && (is_auto || matches!(priority, "required" | "important") || matches!(section, "libs" | "oldlibs" | "libdevel") || get("Essential") == Some("yes"));
        let file = |n: &str| date(n);
        let installed = file(&spec).or_else(|| file(name));
        let mut values = vec![("Package".to_string(), spec.clone())];
        for k in ["Version", "Architecture", "Section", "Priority", "Maintainer", "Homepage", "Description", "Status"] {
            if let Some(v) = get(k) {
                values.push((k.to_string(), v.to_string()));
            }
        }
        if is_auto {
            values.push(("Auto-Installed".into(), "1".into()));
        }
        out.push(Program {
            name: name.to_string(),
            publisher: get("Maintainer").map(without_mail).unwrap_or_default(),
            version: get("Version").unwrap_or("").to_string(),
            size: get("Installed-Size").and_then(|s| s.parse::<u64>().ok()).map(|k| k * 1024),
            date: installed.and_then(date_of),
            system,
            no_remove: get("Essential") == Some("yes"),
            key: format!("dpkg: {spec}"),
            values,
            sources: vec![Source::Dpkg],
            problems,
            ..Program::default()
        });
    }
    out
}

/// The format `rpm -qa` is asked for: one package a line, fields by tabs.
pub const RPM_FORMAT: &str = "%{NAME}\\t%{VERSION}-%{RELEASE}\\t%{ARCH}\\t%{VENDOR}\\t%{SIZE}\\t%{INSTALLTIME}\\t%{SUMMARY}\\t%{URL}\\n";

/// What `rpm -qa --queryformat RPM_FORMAT` prints.
pub fn parse_rpm(text: &str) -> Vec<Program> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            let [name, version, arch, vendor, size, time, summary, url] = f[..] else { return None };
            fn none(s: &str) -> &str {
                if s == "(none)" {
                    ""
                } else {
                    s
                }
            }
            // The keys of rpm itself are packages too.
            if name == "gpg-pubkey" {
                return None;
            }
            let spec = if matches!(arch, "noarch" | "(none)" | "") { name.to_string() } else { format!("{name}.{arch}") };
            let mut values = vec![("Package".to_string(), spec.clone()), ("Version".into(), version.into()), ("Architecture".into(), arch.into())];
            for (k, v) in [("Vendor", none(vendor)), ("Summary", none(summary)), ("URL", none(url))] {
                if !v.is_empty() {
                    values.push((k.into(), v.into()));
                }
            }
            Some(Program {
                name: name.to_string(),
                publisher: none(vendor).to_string(),
                version: version.to_string(),
                size: size.parse().ok(),
                date: time.parse().ok().and_then(date_of),
                system: part_name(name) || name.starts_with("kernel") || name.starts_with("glibc"),
                key: format!("rpm: {spec}"),
                values,
                sources: vec![Source::Rpm],
                ..Program::default()
            })
        })
        .collect()
}

/// The columns `flatpak list` is asked for.
pub const FLATPAK_COLUMNS: &str = "ref,application,name,version,origin,installation,size,description";

/// A size as Flatpak writes it: `1.2 GB`, `350.5 MB` (powers of 1000).
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.replace('\u{a0}', " ");
    let mut parts = s.split_whitespace();
    let n: f64 = parts.next()?.replace(',', ".").parse().ok()?;
    let mult = match parts.next().unwrap_or("bytes").to_lowercase().as_str() {
        "bytes" | "byte" | "b" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        _ => return None,
    };
    Some((n * mult) as u64)
}

/// Where a Flatpak app's files are, and when it was installed (seconds).
pub type FlatpakFiles = (Option<PathBuf>, Option<i64>);

/// A snap's file: its size, and when it was installed (seconds).
pub type SnapFile = (Option<u64>, Option<i64>);

/// What `flatpak list --columns=FLATPAK_COLUMNS` prints (`runtime`: the
/// list of runtimes). `me`: the user running runinst; `installed`: where an
/// app's files are, and when it was installed.
pub fn parse_flatpak(text: &str, runtime: bool, me: &str, installed: &dyn Fn(&str, &str) -> FlatpakFiles) -> Vec<Program> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\t').map(str::trim).collect();
            let [r#ref, app, name, version, origin, installation, size, description] = f[..] else { return None };
            if r#ref.is_empty() || r#ref == "Ref" {
                return None;
            }
            let user = installation == "user";
            let (location, time) = installed(r#ref, installation);
            let mut values = vec![("Ref".to_string(), r#ref.to_string()), ("Application".into(), app.into()), ("Origin".into(), origin.into()), ("Installation".into(), installation.into())];
            if !description.is_empty() {
                values.push(("Description".into(), description.into()));
            }
            Some(Program {
                name: if name.is_empty() { app.to_string() } else { name.to_string() },
                publisher: origin.to_string(),
                version: version.to_string(),
                size: parse_size(size),
                date: time.and_then(date_of),
                location,
                system: runtime,
                scope: if user { Scope::User { name: me.to_string(), current: true } } else { Scope::Machine },
                key: format!("flatpak: {installation} {}", r#ref),
                values,
                sources: vec![Source::Flatpak],
                ..Program::default()
            })
        })
        .collect()
}

/// What `snap list --unicode=never` prints. `file`: a snap's file (size and
/// date), by name and revision.
pub fn parse_snap(text: &str, file: &dyn Fn(&str, &str) -> SnapFile) -> Vec<Program> {
    let mut lines = text.lines();
    let Some(head) = lines.next() else { return Vec::new() };
    // Columns by the positions of the header's words.
    let starts: Vec<usize> = head.match_indices(|c: char| !c.is_whitespace()).filter(|(i, _)| *i == 0 || head.as_bytes()[i - 1] == b' ').map(|(i, _)| i).collect();
    let titles: Vec<&str> = head.split_whitespace().collect();
    lines
        .filter_map(|line| {
            let col = |title: &str| {
                let i = titles.iter().position(|t| *t == title)?;
                let a = *starts.get(i)?;
                let b = starts.get(i + 1).copied().unwrap_or(line.len()).min(line.len());
                line.get(a.min(line.len())..b).map(str::trim)
            };
            let name = col("Name").filter(|n| !n.is_empty())?;
            let rev = col("Rev").unwrap_or("");
            let notes = col("Notes").unwrap_or("");
            let publisher = col("Publisher").unwrap_or("").trim_end_matches(['*', '✓', '✪']);
            let (size, time) = file(name, rev);
            let system = notes.split(',').any(|n| matches!(n.trim(), "base" | "core" | "snapd" | "gadget" | "kernel"));
            let mut values = vec![("Package".to_string(), name.to_string()), ("Revision".into(), rev.into())];
            for (k, v) in [("Tracking", col("Tracking").unwrap_or("")), ("Notes", notes)] {
                if !v.is_empty() && v != "-" {
                    values.push((k.into(), v.into()));
                }
            }
            Some(Program {
                name: name.to_string(),
                publisher: if publisher == "-" { String::new() } else { publisher.to_string() },
                version: col("Version").unwrap_or("").to_string(),
                size,
                date: time.and_then(date_of),
                location: Some(PathBuf::from(format!("/snap/{name}/current"))),
                system,
                key: format!("snap: {name}"),
                values,
                sources: vec![Source::Snap],
                ..Program::default()
            })
        })
        .collect()
}

/// Whether runinst runs as root (its effective user is 0).
pub fn is_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("Uid:")).and_then(|l| l.split_whitespace().nth(1)).map(|u| u == "0"))
        .unwrap_or(false)
}

/// The first of `names` found on the `PATH`.
pub fn on_path(names: &[&'static str]) -> Option<&'static str> {
    let path = std::env::var_os("PATH")?;
    names.iter().copied().find(|n| std::env::split_paths(&path).any(|d| d.join(n).is_file()))
}

#[cfg(not(windows))]
pub use imp::{dpkg_and_rpm, flatpak, snap};

#[cfg(not(windows))]
mod imp {
    use super::*;
    use std::path::Path;
    use std::process::{Command, Stdio};

    fn mtime(p: &Path) -> Option<i64> {
        let t = std::fs::metadata(p).ok()?.modified().ok()?;
        Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
    }

    /// What a command prints, when it ran well.
    fn output(cmd: &str, args: &[&str]) -> Option<String> {
        let o = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    }

    /// dpkg's packages and rpm's (a system has one, rarely both).
    pub fn dpkg_and_rpm() -> Vec<Program> {
        let mut out = Vec::new();
        if let Ok(status) = std::fs::read_to_string("/var/lib/dpkg/status") {
            let auto = std::fs::read_to_string("/var/lib/apt/extended_states").map(|t| parse_apt_auto(&t)).unwrap_or_default();
            let info = Path::new("/var/lib/dpkg/info");
            out.extend(parse_dpkg(&status, &auto, &|n| mtime(&info.join(format!("{n}.list")))));
        }
        if on_path(&["rpm"]).is_some() {
            // rpm reads the `\t` and `\n` itself.
            if let Some(t) = output("rpm", &["-qa", "--queryformat", RPM_FORMAT]) {
                out.extend(parse_rpm(&t));
            }
        }
        out
    }

    /// Flatpak's apps and runtimes, of the system and of the user; None when
    /// Flatpak is not there.
    pub fn flatpak(me: &str) -> Option<Vec<Program>> {
        on_path(&["flatpak"])?;
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let installed = |r: &str, inst: &str| {
            let base = if inst == "user" { home.join(".local/share/flatpak") } else { PathBuf::from("/var/lib/flatpak") };
            // app/<id>/<arch>/<branch>/active
            let dir = base.join(r).join("active");
            let files = dir.join("files");
            (files.is_dir().then_some(files), mtime(&dir))
        };
        let mut out = Vec::new();
        for (kind, runtime) in [("--app", false), ("--runtime", true)] {
            let cols = format!("--columns={FLATPAK_COLUMNS}");
            if let Some(t) = output("flatpak", &["list", kind, &cols]) {
                out.extend(parse_flatpak(&t, runtime, me, &installed));
            }
        }
        Some(out)
    }

    /// Snap's packages; None when Snap is not there.
    pub fn snap() -> Option<Vec<Program>> {
        on_path(&["snap"])?;
        let t = output("snap", &["list", "--unicode=never"])?;
        let file = |name: &str, rev: &str| {
            let f = PathBuf::from(format!("/var/lib/snapd/snaps/{name}_{rev}.snap"));
            (std::fs::metadata(&f).ok().map(|m| m.len()), mtime(&f))
        };
        Some(parse_snap(&t, &file))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = "Package: bash
Essential: yes
Status: install ok installed
Priority: required
Section: shells
Installed-Size: 2020
Maintainer: Ubuntu Developers <ubuntu-devel-discuss@lists.ubuntu.com>
Architecture: amd64
Version: 5.3-2ubuntu1
Conffiles:
 /etc/bash.bashrc a2b7735b79750a503c4265166cab2f88
Description: GNU Bourne Again SHell
 Bash is an sh-compatible command language interpreter.

Package: htop
Status: install ok installed
Priority: optional
Section: utils
Installed-Size: 440
Maintainer: Ubuntu Developers <ubuntu-devel-discuss@lists.ubuntu.com>
Architecture: amd64
Version: 3.4.1-5build2
Description: interactive processes viewer

Package: libvulkan1
Status: install ok installed
Priority: optional
Section: libs
Architecture: amd64
Version: 1.4

Package: fontconfig
Status: install ok installed
Priority: optional
Section: utils
Architecture: amd64
Version: 2.15

Package: vlc
Status: deinstall ok config-files
Priority: optional
Section: video
Architecture: amd64
Version: 3.0.21

Package: broken
Status: install reinstreq half-installed
Architecture: all
Version: 1

Package: gone
Status: purge ok not-installed
Architecture: amd64
";

    #[test]
    fn dpkg_records() {
        let auto = parse_apt_auto("Package: fontconfig\nArchitecture: amd64\nAuto-Installed: 1\n\nPackage: htop\nArchitecture: amd64\nAuto-Installed: 0\n");
        assert!(auto.contains("fontconfig") && auto.contains("fontconfig:amd64") && !auto.contains("htop"));
        let ps = parse_dpkg(STATUS, &auto, &|n| (n == "htop:amd64").then_some(1_731_456_000));
        let names: Vec<&str> = ps.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["bash", "htop", "libvulkan1", "fontconfig", "vlc", "broken"]);
        let by = |n: &str| ps.iter().find(|p| p.name == n).unwrap();
        let htop = by("htop");
        assert_eq!((htop.publisher.as_str(), htop.size, htop.date, htop.system), ("Ubuntu Developers", Some(440 * 1024), Some((2024, 11, 13)), false));
        assert_eq!(package_id(htop), "htop:amd64");
        assert!(by("bash").system && by("bash").no_remove, "required and essential");
        assert!(by("libvulkan1").system && by("fontconfig").system, "a library; installed for another");
        assert_eq!(by("vlc").problems, [Problem::ConfigOnly]);
        assert!(!by("vlc").system, "settings left are shown");
        assert_eq!((by("broken").problems.as_slice(), package_id(by("broken"))), ([Problem::InstallFailed].as_slice(), "broken"));
        assert_eq!(value(by("bash"), "Description"), Some("GNU Bourne Again SHell"));
    }

    #[test]
    fn rpm_lines() {
        let t = "firefox\t132.0-1.fc41\tx86_64\tFedora Project\t250000000\t1731456000\tMozilla Firefox Web browser\thttps://www.mozilla.org/\n\
                 glibc-common\t2.40-9.fc41\tx86_64\tFedora Project\t1000\t1731456000\tCommon binaries\t(none)\n\
                 gpg-pubkey\te99d6ad1-64d2612c\t(none)\t(none)\t0\t1731456000\tkey\t(none)\n";
        let ps = parse_rpm(t);
        assert_eq!(ps.len(), 2);
        assert_eq!((ps[0].name.as_str(), package_id(&ps[0]), ps[0].size, ps[0].date, ps[0].system), ("firefox", "firefox.x86_64", Some(250_000_000), Some((2024, 11, 13)), false));
        assert!(ps[1].system && value(&ps[1], "URL").is_none());
    }

    /// What this system has: `cargo test -p runinst --lib packages_here -- --ignored --nocapture`.
    #[test]
    #[ignore]
    #[cfg(not(windows))]
    fn packages_here() {
        let t = std::time::Instant::now();
        let ps = dpkg_and_rpm();
        let shown: Vec<&Program> = ps.iter().filter(|p| p.visible()).collect();
        println!("dpkg/rpm: {} ({} shown, {} with problems) in {:?}", ps.len(), shown.len(), ps.iter().filter(|p| !p.problems.is_empty()).count(), t.elapsed());
        for p in shown.iter().take(40) {
            println!("  {} {} [{}] {:?} {:?}", p.name, p.version, p.publisher, p.size, p.date);
        }
        let me = std::env::var("USER").unwrap_or_default();
        for (what, list) in [("flatpak", flatpak(&me)), ("snap", snap())] {
            match list {
                None => println!("{what}: not here"),
                Some(l) => {
                    println!("{what}: {}", l.len());
                    for p in l {
                        println!("  {} {} [{}] {:?} {:?} system={}", p.name, p.version, p.publisher, p.size, p.date, p.system);
                    }
                }
            }
        }
    }

    #[test]
    fn flatpak_and_snap_lists() {
        assert_eq!(parse_size("1.2\u{a0}GB"), Some(1_200_000_000));
        assert_eq!(parse_size("350,5 kB"), Some(350_500));
        let t = "app/org.mozilla.firefox/x86_64/stable\torg.mozilla.firefox\tFirefox\t132.0\tflathub\tsystem\t280.1 MB\tWeb browser\n\
                 app/org.gimp.GIMP/x86_64/stable\torg.gimp.GIMP\tGNU Image Manipulation Program\t2.10\tflathub\tuser\t1.2 GB\t\n";
        let ps = parse_flatpak(t, false, "ana", &|_, _| (None, Some(1_731_456_000)));
        assert_eq!(ps.len(), 2);
        assert_eq!((package_id(&ps[0]), ps[0].scope.clone(), ps[0].size), ("app/org.mozilla.firefox/x86_64/stable", Scope::Machine, Some(280_100_000)));
        assert_eq!(ps[1].scope, Scope::User { name: "ana".into(), current: true });
        let t = "Name             Version             Rev    Tracking       Publisher    Notes\n\
                 asciiquarium     1.1-snap9378        59     latest/stable  brlin        -\n\
                 core22           20260909            2987   latest/stable  canonical**  base\n\
                 no-more-secrets  1.0.1               12     latest/stable  alexmurray*  -\n";
        let ps = parse_snap(t, &|n, r| (Some(if n == "core22" && r == "2987" { 7 } else { 1 }), None));
        assert_eq!(ps.len(), 3);
        assert_eq!((ps[0].name.as_str(), ps[0].version.as_str(), ps[0].publisher.as_str(), ps[0].system), ("asciiquarium", "1.1-snap9378", "brlin", false));
        assert_eq!((ps[1].publisher.as_str(), ps[1].system, ps[1].size), ("canonical", true, Some(7)));
        assert_eq!(ps[2].publisher, "alexmurray");
        assert!(part_name("libgtk-3-0") && part_name("python3-yaml") && !part_name("libreoffice-writer") && !part_name("htop"));
    }
}
