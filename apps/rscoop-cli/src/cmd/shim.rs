//! `shim add|rm|list|info` (`libexec\scoop-shim.ps1`). `alter`, which asks
//! the user to choose, is Scoop's.

use super::args::getopt;
use super::machinery::{remove_shim, say_made};
use super::query::find_command;
use super::Ctx;
use regex_lite::RegexBuilder;
use rt_scoop::json::{string, Json};
use rt_scoop::shims;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const SUBCOMMANDS: [&str; 5] = ["add", "rm", "list", "info", "alter"];

/// `Get-ShimPath`: the `.shim` or `.ps1` of a name.
fn shim_path(ctx: &Ctx, name: &str, global: bool) -> Option<PathBuf> {
    [".shim", ".ps1"].iter().map(|ext| ctx.scoop.shims(global).join(format!("{name}{ext}"))).find(|p| p.exists())
}

/// `Get-ShimInfo`.
struct Info {
    name: String,
    path: PathBuf,
    source: String,
    kind: &'static str,
    alternatives: Option<String>,
    global: bool,
    hidden: bool,
}

/// The aliases and functions a new PowerShell 7 has (`Get-Alias`, and the
/// functions of no module).
const PWSH: &str =
    "? % ac cat cd cd.. cd\\ cd~ chdir clc clear Clear-Host clhy cli clp cls clv cnsn compare copy cp cpi cpp cvpa dbp del diff \
dir dnsn ebp echo epal epcsv erase etsn exsn fc fhx fl foreach ft fw gal gbp gc gci gcm gcs gdr gerr ghy gi gjb gl gm gmo gp gps gpv group \
gsn gsv gu gv h help history icm iex ihy ii ipal ipcsv ipmo irm iwr kill ls man md measure mi mkdir mount move mp mv nal ndr ni nmo nsn nv \
ogv oh oss Pause popd prompt ps pushd pwd r rbp rcjb rcsn rd rdr ren ri rjb rm rmdir rmo rni rnp rp rsn rv rvpa sajb sal saps sasv sbp \
select set shcm si sl sleep sls sort sp spjb spps spsv start sv TabExpansion2 tee type where wjb write";

/// The same in Windows PowerShell 5.1.
const WINDOWS_POWERSHELL: &str = "% ? ac asnp cat cd cd.. cd\\ CFS chdir clc clear Clear-Host clhy cli clp cls clv cnsn compare copy cp \
cpi cpp curl cvpa dbp del diff dir dnsn ebp echo epal epcsv epsn erase etsn exsn fc fhx fl foreach ft fw gal gbp gc gci gcm gcs gdr \
Get-Verb ghy gi gjb gl gm gmo gp gps gpv group gsn gsnp gsv gu gv gwmi h help history icm iex ihy ii ImportSystemModules ipal ipcsv \
ipmo ipsn irm ise iwmi iwr kill lp ls man md measure mi mkdir more mount move mp mv nal ndr ni nmo npssc nsn nv ogv oh oss Pause popd \
prompt ps pushd pwd r rbp rcjb rcsn rd rdr ren ri rjb rm rmdir rmo rni rnp rp rsn rsnp rujb rv rvpa rwmi sajb sal saps sasv sbp sc \
select set shcm si sl sleep sls sort sp spjb spps spsv start sujb sv swmi TabExpansion2 tee trcm type wget where wjb write";

/// The lowercase names of the files in a folder.
fn file_names(dir: &Path) -> HashSet<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| !t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .collect()
}

/// What `Get-ShimInfo` asks of the system, read once for all the shims.
struct Lookup {
    /// The names `Get-Command` finds before any program while `scoop shim`
    /// runs: PowerShell's aliases and functions, and the functions of the
    /// Scoop libraries that command has loaded.
    first: HashSet<String>,
    /// The folders of `PATH` and their files.
    path: Vec<(PathBuf, HashSet<String>)>,
    extensions: Vec<String>,
    /// The files of the shims folders.
    folders: HashMap<PathBuf, Vec<String>>,
}

impl Lookup {
    fn new(ctx: &Ctx) -> Lookup {
        let pwsh = super::delegate::powershell(ctx).file_name().is_some_and(|n| n.eq_ignore_ascii_case("pwsh.exe"));
        let mut first: HashSet<String> = if pwsh { PWSH } else { WINDOWS_POWERSHELL }.split(' ').map(str::to_lowercase).collect();
        let lib = ctx.scoop.app("scoop", false).join("current").join("lib");
        for file in ["core", "buckets", "commands", "help", "getopt", "install", "system"] {
            let text = rt_scoop::json::read_text(&lib.join(format!("{file}.ps1"))).unwrap_or_default();
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("function ") {
                    let name: String = rest.chars().take_while(|c| !c.is_whitespace() && *c != '(' && *c != '{').collect();
                    first.insert(name.to_lowercase());
                }
            }
        }
        let extensions = std::iter::once(".ps1".to_string())
            .chain(
                ctx.env.get("PATHEXT").unwrap_or(".COM;.EXE;.BAT;.CMD").split(';').filter(|e| !e.is_empty()).map(str::to_ascii_lowercase),
            )
            .collect();
        let path = ctx.env.path().map(|d| (d.clone(), file_names(&d))).collect();
        let folders = [false, true]
            .iter()
            .map(|&g| {
                let dir = ctx.scoop.shims(g);
                let names = file_names(&dir).into_iter().collect();
                (dir, names)
            })
            .collect();
        Lookup { first, path, extensions, folders }
    }

    /// [`find_command`], in what was read.
    fn command(&self, name: &str) -> Option<PathBuf> {
        let lower = name.to_lowercase();
        let has_extension = Path::new(name).extension().is_some();
        self.path.iter().find_map(|(dir, files)| {
            if has_extension && files.contains(&lower) {
                return Some(dir.join(name));
            }
            self.extensions.iter().find(|e| files.contains(&format!("{lower}{e}"))).map(|e| dir.join(format!("{name}{e}")))
        })
    }
}

fn info_of(ctx: &Ctx, shim: &Path, lookup: &Lookup) -> Info {
    let file = shim.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let name = shims::strip_ext(&file).to_string();
    let text = shim.display().to_string();
    let path = PathBuf::from(match text.strip_suffix("shim") {
        Some(stem) => format!("{stem}exe"),
        None => text.clone(),
    });
    let source = Some(shims::app_of_shim(&ctx.scoop, shim)).filter(|s| !s.is_empty()).unwrap_or_else(|| "External".into());
    let mut others: Vec<String> = Vec::new();
    let prefix = format!("{}.", file.to_lowercase());
    let dir = shim.parent().unwrap_or(shim).to_path_buf();
    let listed = lookup.folders.get(&dir).cloned().unwrap_or_else(|| file_names(&dir).into_iter().collect());
    let mut listed: Vec<&String> = listed.iter().collect();
    listed.sort();
    for lower in listed {
        if lower.starts_with(&prefix) && ![".shim", ".cmd", ".ps1"].iter().any(|x| lower.ends_with(x)) {
            let ext = lower.rsplit('.').next().unwrap_or("").to_string();
            if !others.contains(&ext) {
                others.push(ext);
            }
        }
    }
    let alternatives = (!others.is_empty()).then(|| std::iter::once(source.clone()).chain(others).collect::<Vec<_>>().join(" "));
    let global = text.to_lowercase().starts_with(&ctx.scoop.shims(true).display().to_string().to_lowercase());
    let found = (!lookup.first.contains(&name.to_lowercase())).then(|| lookup.command(&name)).flatten();
    let hidden = !found.is_some_and(|f| f.display().to_string().eq_ignore_ascii_case(&path.display().to_string()));
    let kind = if text.to_lowercase().ends_with(".ps1") { "ExternalScript" } else { "Application" };
    Info { name, path, source, kind, alternatives, global, hidden }
}

fn yes(value: bool) -> String {
    if value { "True" } else { "False" }.to_string()
}

fn json_of(info: &Info) -> Json {
    let mut members = vec![
        ("Name".to_string(), string(info.name.clone())),
        ("Path".to_string(), string(info.path.display().to_string())),
        ("Source".to_string(), string(info.source.clone())),
        ("Type".to_string(), string(info.kind)),
    ];
    if let Some(a) = &info.alternatives {
        members.push(("Alternatives".into(), string(a.clone())));
    }
    members.push(("IsGlobal".into(), Json::Bool(info.global)));
    members.push(("IsHidden".into(), Json::Bool(info.hidden)));
    Json::Obj(members)
}

pub fn run(ctx: &mut Ctx, args: &[String]) -> i32 {
    let sub = args.first().map(String::as_str).unwrap_or("");
    if !SUBCOMMANDS.contains(&sub) {
        return if sub.is_empty() { ctx.fail("shim.no_subcommand", &[]) } else { ctx.fail("shim.bad_subcommand", &[("sub", sub)]) };
    }
    let opts = match getopt(&args[1..], &[('g', "global", false)]) {
        Ok(o) => o,
        Err(option) => return ctx.fail("cli.unknown_option", &[("opt", &option)]),
    };
    let global = opts.has('g');
    let other = &opts.rest;
    if sub != "list" && other.is_empty() {
        return ctx.fail("shim.no_name", &[("sub", sub)]);
    }
    match sub {
        "add" => add(ctx, other, global),
        "rm" => rm(ctx, other, global),
        "list" => list(ctx, other, global),
        _ => info(ctx, &other[0], global),
    }
}

fn add(ctx: &mut Ctx, other: &[String], global: bool) -> i32 {
    if other.len() < 2 || other[1].is_empty() {
        return ctx.fail("shim.no_path", &[]);
    }
    let (name, given) = (&other[0], &other[1]);
    let mut path = Some(PathBuf::from(given));
    if !given.contains(['\\', '/']) {
        path = shim_path(ctx, given, global).and_then(|s| shims::target(&s)).or_else(|| find_command(ctx, given));
    }
    let Some(path) = path.filter(|p| p.exists()) else {
        ctx.warn("shim.no_command", &[("path", given)]);
        return 3;
    };
    let line = ctx.t.fill(if global { "shim.adding_global" } else { "shim.adding" }, &[("name", name)]);
    ctx.say(line);
    match shims::create(&ctx.scoop, ctx.system, &path, global, Some(name), &other[2..]) {
        Ok(made) => {
            say_made(ctx, &made, global);
            0
        }
        Err(why) => ctx.fail("error.config", &[("why", &why)]),
    }
}

fn rm(ctx: &mut Ctx, other: &[String], global: bool) -> i32 {
    let dir = ctx.scoop.shims(global);
    let mut missing = Vec::new();
    for name in other {
        if shim_path(ctx, name, global).is_some() {
            remove_shim(ctx, &dir, name, None);
        } else {
            missing.push(name.clone());
        }
    }
    for name in &missing {
        ctx.warn(if global { "shim.not_found_global" } else { "shim.not_found" }, &[("name", name)]);
    }
    if missing.is_empty() {
        0
    } else {
        3
    }
}

/// The `.shim` and `.ps1` files under `dir`, in the order of their paths.
fn shim_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
    for entry in entries {
        let path = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            shim_files(&path, out);
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("shim") || e.eq_ignore_ascii_case("ps1")) {
            out.push(path);
        }
    }
}

fn list(ctx: &mut Ctx, other: &[String], global: bool) -> i32 {
    let patterns: Vec<&String> = other.iter().filter(|p| *p != "*").collect();
    for p in &patterns {
        if RegexBuilder::new(p).build().is_err() {
            return ctx.fail("shim.bad_pattern", &[("pattern", p)]);
        }
    }
    let joined: Vec<&str> = patterns.iter().map(|p| p.as_str()).collect();
    let pattern = RegexBuilder::new(&joined.join("|")).case_insensitive(true).build().ok().filter(|_| !patterns.is_empty());
    let mut files = Vec::new();
    if !global {
        shim_files(&ctx.scoop.shims(false), &mut files);
    }
    shim_files(&ctx.scoop.shims(true), &mut files);
    let lookup = Lookup::new(ctx);
    let infos: Vec<Info> = files
        .iter()
        .filter(|f| {
            let base = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            pattern.as_ref().is_none_or(|p| p.is_match(&base))
        })
        .map(|f| info_of(ctx, f, &lookup))
        .collect();
    if ctx.json {
        ctx.print_json(&Json::Arr(infos.iter().map(json_of).collect()));
        return 0;
    }
    let rows: Vec<Vec<String>> = infos
        .iter()
        .map(|i| vec![i.name.clone(), i.source.clone(), i.alternatives.clone().unwrap_or_default(), yes(i.global), yes(i.hidden)])
        .collect();
    if !rows.is_empty() {
        ctx.table(&["Name", "Source", "Alternatives", "IsGlobal", "IsHidden"], &rows);
    }
    0
}

fn info(ctx: &mut Ctx, name: &str, global: bool) -> i32 {
    let Some(shim) = shim_path(ctx, name, global) else {
        ctx.warn(if global { "shim.not_found_global" } else { "shim.not_found" }, &[("name", name)]);
        if shim_path(ctx, name, !global).is_some() {
            let line = ctx.t.fill(if global { "shim.local_exists" } else { "shim.global_exists" }, &[("name", name)]);
            ctx.say(line);
            return 2;
        }
        return 3;
    };
    let info = info_of(ctx, &shim, &Lookup::new(ctx));
    if ctx.json {
        ctx.print_json(&json_of(&info));
        return 0;
    }
    let mut pairs = vec![
        ("Name".to_string(), info.name.clone()),
        ("Path".to_string(), info.path.display().to_string()),
        ("Source".to_string(), info.source.clone()),
        ("Type".to_string(), info.kind.to_string()),
    ];
    if let Some(a) = &info.alternatives {
        pairs.push(("Alternatives".into(), a.clone()));
    }
    pairs.push(("IsGlobal".into(), yes(info.global)));
    pairs.push(("IsHidden".into(), yes(info.hidden)));
    ctx.pairs(&pairs);
    0
}
