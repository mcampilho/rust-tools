//! `checkup`: the tests of Scoop's `lib\diagnostic.ps1` and
//! `libexec\scoop-checkup.ps1`, then the checks of rscoop-cli.

use super::{paint, Ctx, Outside};
use crate::registry::Registry;
use crate::sys::{filesystem_name, is_admin, registry_dword};
use rt_scoop::buckets;
use rt_scoop::install::Source;
use rt_scoop::json::{pretty, string, Json};
use rt_scoop::resolve::helper;
use std::path::Path;

const DEFENDER: &str = r"SOFTWARE\Microsoft\Windows Defender";

/// What Scoop's checkup would say: (problem, what to do).
fn scoop_problems(ctx: &Ctx, registry: &dyn Registry) -> Vec<(String, String)> {
    let t = &ctx.t;
    let scoop = &ctx.scoop;
    let mut out = Vec::new();
    let mut add = |problem: String, advice: &str| out.push((problem, advice.to_string()));

    if is_admin() && ctx.env.get("USERNAME") != Some("WDAGUtilityAccount") {
        let realtime_off = registry_dword(&format!(r"{DEFENDER}\Real-Time Protection"), "DisableRealtimeMonitoring") == Some(1);
        if !realtime_off {
            let excluded = registry.values("HKLM", &format!(r"{DEFENDER}\Exclusions\Paths"));
            for path in [&scoop.root, &scoop.global] {
                let path = path.display().to_string();
                // The exclusions are the names of the values.
                let found = excluded.iter().any(|(name, _, _)| name.eq_ignore_ascii_case(&path))
                    || registry_dword(&format!(r"{DEFENDER}\Exclusions\Paths"), &path).is_some();
                if !found {
                    add(t.fill("checkup.defender", &[("path", &path)]), &format!("sudo Add-MpPreference -ExclusionPath '{path}'"));
                }
            }
        }
    }
    if !buckets::local(scoop).iter().any(|b| b.eq_ignore_ascii_case("main")) {
        add(t.get("checkup.main").into(), "scoop bucket add main");
    }
    if registry_dword(r"SYSTEM\CurrentControlSet\Control\FileSystem", "LongPathsEnabled").unwrap_or(0) == 0 {
        add(
            t.get("checkup.long_paths").into(),
            r"sudo Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name 'LongPathsEnabled' -Value 1",
        );
    }
    if registry_dword(r"SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock", "AllowDevelopmentWithoutDevLicense") != Some(1) {
        add(t.get("checkup.developer").into(), "https://blogs.windows.com/windowsdeveloper/2016/12/02/symlinks-windows-10/");
    }
    let missing = |names: &[(&str, &str)]| names.iter().all(|(app, file)| helper(scoop, app, file).is_none());
    if missing(&[("7zip", "7z.exe")]) && !scoop.enabled("use_external_7zip") {
        add(t.fill("checkup.helper", &[("name", "7-Zip")]), "scoop install 7zip");
    }
    if missing(&[("innounp-unicode", "innounp.exe"), ("innounp", "innounp.exe")]) {
        add(t.fill("checkup.helper", &[("name", "Inno Setup Unpacker")]), "scoop install innounp");
    }
    if missing(&[("dark", "dark.exe"), ("wixtoolset", "wix.exe")]) {
        add(t.fill("checkup.helper", &[("name", "dark")]), "scoop install dark");
    }
    for (path, setting) in [(&scoop.global, "global_path"), (&scoop.root, "root_path")] {
        if let Some(fs) = filesystem_name(Path::new(path)).filter(|fs| !fs.eq_ignore_ascii_case("NTFS")) {
            let path = path.display().to_string();
            add(t.fill("checkup.ntfs", &[("path", &path), ("fs", &fs)]), &format!("scoop config {setting} <NTFS>"));
        }
    }
    out
}

pub fn run(ctx: &mut Ctx, args: &[String], outside: &Outside) -> i32 {
    let problems = scoop_problems(ctx, outside.registry);

    // The checks of rscoop-cli, on the same installation.
    let mut line = Vec::new();
    // Without `--root` the checker finds the same one, and says how.
    if ctx.scoop.root_source == Source::Option {
        line.extend(["--root".to_string(), ctx.scoop.root.display().to_string()]);
    }
    if let Some(lang) = &ctx.lang {
        line.extend(["--lang".to_string(), lang.clone()]);
    }
    if ctx.json {
        line.push("--json".into());
    }
    line.extend(args.iter().cloned());

    if !ctx.json {
        let title = ctx.t.get("checkup.title").to_string();
        ctx.say_as(title, paint::HEADER);
        for (problem, advice) in &problems {
            ctx.say_as(format!("  ! {problem}"), paint::WARN);
            ctx.say_as(format!("      {advice}"), paint::DIM);
        }
        if problems.is_empty() {
            let ok = ctx.t.get("checkup.none").to_string();
            ctx.say_as(format!("  {ok}"), paint::GOOD);
        }
        ctx.say("");
    }

    let mut report = Vec::new();
    let code =
        crate::cli::run(&line, outside.registry, outside.vars.clone(), &ctx.env.home.display().to_string(), &mut report, ctx.err, None);
    if ctx.json && code != 2 {
        let list: Vec<Json> = problems.iter().map(|(p, a)| Json::Arr(vec![string(p.clone()), string(a.clone())])).collect();
        let text = format!("{{\n\"scoop\": {},\n\"rscoop\": {}\n}}", pretty(&Json::Arr(list)), String::from_utf8_lossy(&report).trim_end());
        ctx.say(text);
    } else {
        let _ = ctx.out.write_all(&report);
    }
    if code == 0 && !problems.is_empty() {
        1
    } else {
        code
    }
}
