//! The checks. Each takes the [`Context`] and returns a [`CheckResult`];
//! nothing here changes the system.

use crate::cache::{stamp_of, Cache};
use crate::lnk;
use crate::paths::{
    clean, dirname, exists, extract_paths, is_dir, is_file, is_under, join, norm, normpath, read_text, relative_to,
    scoop_root_of, PathRef,
};
use crate::registry::{Registry, MACHINE_ENV, REG_EXPAND_SZ, USER_ENV};
use crate::sys::{expand, link_kind, read_link, same_file, LinkKind};
use rt_core::json::{self, Json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

pub const CHECK_IDS: [&str; 10] = ["roots", "shims", "current", "persist", "install", "shortcuts", "regfiles", "registry", "env", "configs"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sev {
    Error,
    Warning,
}

impl Sev {
    pub fn as_str(self) -> &'static str {
        match self {
            Sev::Error => "error",
            Sev::Warning => "warning",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Problem {
    pub severity: Sev,
    pub item: String,
    pub code: String,
    pub actual: Option<String>,
    pub expected: Option<String>,
    pub args: Vec<(String, String)>,
    /// What --fix needs beyond the item: (hive, key, value name).
    pub fixdata: Option<(String, String, String)>,
}

impl Problem {
    pub fn arg(&self, k: &str) -> Option<&str> {
        self.args.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug)]
pub struct CheckResult {
    pub id: &'static str,
    pub checked: usize,
    pub seconds: Option<f64>,
    pub problems: Vec<Problem>,
    /// (label key, value, status key)
    pub info: Vec<(String, String, String)>,
}

impl CheckResult {
    pub fn new(id: &'static str) -> CheckResult {
        CheckResult { id, checked: 0, seconds: None, problems: Vec::new(), info: Vec::new() }
    }

    pub fn add(&mut self, severity: Sev, item: &str, code: &str) -> &mut Problem {
        self.problems.push(Problem {
            severity,
            item: item.to_string(),
            code: code.to_string(),
            actual: None,
            expected: None,
            args: Vec::new(),
            fixdata: None,
        });
        self.problems.last_mut().unwrap()
    }

    pub fn count(&self, sev: Sev) -> usize {
        self.problems.iter().filter(|p| p.severity == sev).count()
    }
}

impl Problem {
    fn actual(&mut self, v: impl Into<String>) -> &mut Problem {
        self.actual = Some(v.into());
        self
    }

    fn expected(&mut self, v: Option<String>) -> &mut Problem {
        self.expected = v;
        self
    }

    fn with(&mut self, k: &str, v: impl Into<String>) -> &mut Problem {
        self.args.push((k.to_string(), v.into()));
        self
    }
}

/// What is wrong with one path.
#[derive(Clone, Debug)]
pub struct Eval {
    pub code: &'static str,
    pub actual: String,
    pub expected: Option<String>,
    pub other: Option<String>,
}

pub fn add_eval<'r>(res: &'r mut CheckResult, sev: Sev, item: &str, e: &Eval, missing_sev: Option<Sev>) -> &'r mut Problem {
    let sev = if e.code == "missing" { missing_sev.unwrap_or(sev) } else { sev };
    let p = res.add(sev, item, e.code);
    p.actual = Some(e.actual.clone());
    p.expected = e.expected.clone();
    if let Some(o) = &e.other {
        p.args.push(("other".into(), o.clone()));
    }
    p
}

/// One problem per kind of fault in `item`, with how many paths have it.
pub fn add_grouped(res: &mut CheckResult, item: &str, evals: &[Eval], other_sev: Sev, missing_sev: Sev) {
    let mut groups: Vec<(&Eval, usize)> = Vec::new();
    for e in evals {
        match groups.iter_mut().find(|(g, _)| g.code == e.code && g.other == e.other) {
            Some(g) => g.1 += 1,
            None => groups.push((e, 1)),
        }
    }
    for (e, n) in groups {
        let sev = if e.code == "other_root" { other_sev } else { missing_sev };
        let code = if n > 1 { format!("{}_n", e.code) } else { e.code.to_string() };
        let p = res.add(sev, item, &code);
        p.actual = Some(e.actual.clone());
        p.expected = e.expected.clone();
        if let Some(o) = &e.other {
            p.args.push(("other".into(), o.clone()));
        }
        p.args.push(("count".into(), n.to_string()));
    }
}

// ---------------------------------------------------------------------------
// Roots
// ---------------------------------------------------------------------------

pub struct Roots {
    pub root: String,
    pub root_source: String,
    pub global_root: String,
    pub global_source: String,
    pub config_path: String,
    pub config: Json,
    /// (name, scope, value)
    pub vars: Vec<(String, String, String)>,
}

pub fn load_json(path: &str) -> Option<Json> {
    let text = std::fs::read_to_string(path).ok()?;
    parse_object(&text)
}

fn parse_object(text: &str) -> Option<Json> {
    match json::parse(text.trim_start_matches('\u{feff}')) {
        Ok(j @ Json::Obj(_)) => Some(j),
        _ => None,
    }
}

pub fn config_file(environ: &HashMap<String, String>, home: &str) -> String {
    let base = match environ.get("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        Some(x) => x.clone(),
        None => join(home, ".config"),
    };
    join(&join(&base, "scoop"), "config.json")
}

/// Work out the Scoop root and global root the way Scoop does.
pub fn find_roots(reg: &dyn Registry, environ: &HashMap<String, String>, home: &str, root_option: Option<&str>) -> Roots {
    let config_path = config_file(environ, home);
    let config = load_json(&config_path).unwrap_or(Json::Obj(Vec::new()));
    let mut vars = Vec::new();
    for name in ["SCOOP", "SCOOP_GLOBAL"] {
        if let Some(v) = environ.get(name).filter(|v| !v.is_empty()) {
            vars.push((name.to_string(), "process".to_string(), v.clone()));
        }
        for (scope, (hive, path)) in [("user", USER_ENV), ("machine", MACHINE_ENV)] {
            if let Some((v, _)) = reg.get(hive, path, name) {
                vars.push((name.to_string(), scope.to_string(), expand(&v)));
            }
        }
    }
    let first = |name: &str| {
        vars.iter().find(|(n, _, v)| n == name && !v.is_empty()).map(|(_, s, v)| (v.clone(), format!("env_{s}")))
    };
    let cfg = |k: &str| Some(config.get(k).str().to_string()).filter(|s| !s.is_empty());
    let (root, root_source) = match root_option {
        Some(r) => (r.to_string(), "option".to_string()),
        None => first("SCOOP")
            .or_else(|| cfg("root_path").map(|r| (r, "config".into())))
            .unwrap_or_else(|| (join(home, "scoop"), "default".into())),
    };
    let (global_root, global_source) = first("SCOOP_GLOBAL").or_else(|| cfg("global_path").map(|r| (r, "config".into()))).unwrap_or_else(|| {
        let pd = environ.get("ProgramData").or_else(|| environ.get("PROGRAMDATA")).cloned().unwrap_or_else(|| r"C:\ProgramData".into());
        (join(&pd, "scoop"), "default".into())
    });
    Roots { root: clean(&root), root_source, global_root: clean(&global_root), global_source, config_path, config, vars }
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

pub struct App {
    pub name: String,
    pub dir: String,
    pub scope: &'static str,
    pub current: String,
    pub usable: bool,
    pub install_exists: bool,
    pub manifest_exists: bool,
    pub install: Option<Json>,
    pub manifest: Option<Json>,
}

pub const INSTALL_NAMES: [&str; 2] = ["scoop-install.json", "install.json"];
pub const MANIFEST_NAMES: [&str; 2] = ["scoop-manifest.json", "manifest.json"];

fn first_file(folder: &str, names: &[&str]) -> Option<String> {
    names.iter().map(|n| join(folder, n)).find(|p| is_file(p))
}

pub fn is_empty_dir(path: &str) -> bool {
    std::fs::read_dir(path).map(|mut it| it.next().is_none()).unwrap_or(false)
}

/// Version folders of an app (real folders other than `current`), newest first.
pub fn version_dirs(app_dir: &str) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(app_dir) else { return Vec::new() };
    let mut out: Vec<(bool, std::time::SystemTime, String)> = rd
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().eq_ignore_ascii_case("current"))
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| {
            let p = e.path().to_string_lossy().to_string();
            let t = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            (first_file(&p, &INSTALL_NAMES).is_some(), t, p)
        })
        .collect();
    out.sort_by_key(|x| std::cmp::Reverse((x.0, x.1)));
    out.into_iter().map(|x| x.2).collect()
}

pub enum Class {
    Current,
    Other(String),
    Outside,
}

pub struct Context<'a> {
    pub roots: Roots,
    pub root: String,
    pub global_root: String,
    pub registry: &'a dyn Registry,
    pub environ: HashMap<String, String>,
    pub config_depth: usize,
    pub persist_depth: usize,
    pub max_config_size: usize,
    pub cache: Cache,
    /// (bucket, app) pairs from Scoop's scoop.db, lower case.
    pub scoop_db: Option<HashSet<(String, String)>>,
    root_n: String,
    global_n: String,
    old_roots: Vec<(String, String)>,
    /// normalised other root -> shown path
    pub discovered: Mutex<BTreeMap<String, String>>,
    apps: OnceLock<Vec<App>>,
    memo: Mutex<HashMap<String, bool>>,
}

impl<'a> Context<'a> {
    pub fn new(roots: Roots, registry: &'a dyn Registry) -> Context<'a> {
        let (root, global_root) = (roots.root.clone(), roots.global_root.clone());
        Context {
            root_n: norm(&root),
            global_n: norm(&global_root),
            roots,
            root,
            global_root,
            registry,
            environ: HashMap::new(),
            config_depth: 3,
            persist_depth: 5,
            max_config_size: 512 * 1024,
            cache: Cache::disabled(),
            scoop_db: None,
            old_roots: Vec::new(),
            discovered: Mutex::new(BTreeMap::new()),
            apps: OnceLock::new(),
            memo: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_old_roots(mut self, old: &[String]) -> Self {
        self.old_roots = old.iter().map(|r| (norm(r), clean(r))).collect();
        self
    }

    /// Does `p` exist? Remembered for the run: config files repeat paths a lot.
    pub fn exists(&self, p: &str) -> bool {
        if let Some(&v) = self.memo.lock().unwrap().get(p) {
            return v;
        }
        let v = exists(p);
        self.memo.lock().unwrap().insert(p.to_string(), v);
        v
    }

    /// (root, scope) pairs that exist: the user root and the global root.
    pub fn scope_roots(&self) -> Vec<(String, &'static str)> {
        let mut out = vec![(self.root.clone(), "user")];
        if is_dir(&self.global_root) && self.global_n != self.root_n {
            out.push((self.global_root.clone(), "global"));
        }
        out
    }

    pub fn classify(&self, path: &str) -> Class {
        let n = norm(path);
        if is_under(&n, &self.root_n) || is_under(&n, &self.global_n) {
            return Class::Current;
        }
        for (old_n, old) in &self.old_roots {
            if is_under(&n, old_n) {
                self.discovered.lock().unwrap().entry(old_n.clone()).or_insert_with(|| old.clone());
                return Class::Other(old.clone());
            }
        }
        if let Some(other) = scoop_root_of(path) {
            self.discovered.lock().unwrap().entry(norm(&other)).or_insert_with(|| other.clone());
            return Class::Other(other);
        }
        Class::Outside
    }

    pub fn translate(&self, path: &str, other: &str) -> Option<String> {
        relative_to(path, other).map(|rel| join(&self.root, &rel))
    }

    /// Judge one path: None when fine.
    pub fn evaluate(&self, pref: &PathRef, missing_anywhere: bool) -> Option<Eval> {
        let ex = |p: &str| self.exists(p);
        let shown = pref.shown_with(&ex);
        match self.classify(&shown) {
            Class::Other(base) => {
                Some(Eval { code: "other_root", expected: self.translate(&shown, &base), actual: shown, other: Some(base) })
            }
            c => {
                if (matches!(c, Class::Current) || missing_anywhere) && !pref.exists_with(&ex) {
                    Some(Eval { code: "missing", actual: shown, expected: None, other: None })
                } else {
                    None
                }
            }
        }
    }

    pub fn evaluate_refs(&self, refs: &[PathRef], missing_anywhere: bool) -> Vec<Eval> {
        refs.iter().filter_map(|r| self.evaluate(r, missing_anywhere)).collect()
    }

    /// Judge every absolute path in `text`.
    pub fn evaluate_text(&self, text: &str, missing_anywhere: bool) -> Vec<Eval> {
        if !text.contains(":\\") && !text.contains(":/") {
            return Vec::new();
        }
        self.evaluate_refs(&extract_paths(text), missing_anywhere)
    }

    fn cached_json(&self, path: &str) -> Option<Json> {
        self.cache.get_file("json", path, || std::fs::read_to_string(path).ok()).and_then(|t| parse_object(&t))
    }

    pub fn apps(&self) -> &[App] {
        self.apps.get_or_init(|| {
            let mut out = Vec::new();
            for (root, scope) in self.scope_roots() {
                let Ok(rd) = std::fs::read_dir(join(&root, "apps")) else { continue };
                let mut entries: Vec<_> = rd.flatten().filter(|e| e.path().is_dir()).collect();
                entries.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
                for e in entries {
                    let dir = e.path().to_string_lossy().to_string();
                    let current = join(&dir, "current");
                    let ip = first_file(&current, &INSTALL_NAMES);
                    let mp = first_file(&current, &MANIFEST_NAMES);
                    out.push(App {
                        name: e.file_name().to_string_lossy().to_string(),
                        usable: is_dir(&current) && !is_empty_dir(&current),
                        install_exists: ip.is_some(),
                        manifest_exists: mp.is_some(),
                        install: ip.and_then(|p| self.cached_json(&p)),
                        manifest: mp.and_then(|p| self.cached_json(&p)),
                        dir,
                        current,
                        scope,
                    });
                }
            }
            out
        })
    }

    pub fn persist_dir(&self, app: &App) -> String {
        let base = if app.scope == "global" { &self.global_root } else { &self.root };
        join(&join(base, "persist"), &app.name)
    }
}

fn pref(path: &str) -> PathRef {
    PathRef::new(path)
}

// ---------------------------------------------------------------------------
// roots
// ---------------------------------------------------------------------------

pub fn check_roots(ctx: &Context) -> CheckResult {
    let mut res = CheckResult::new("roots");
    let roots = &ctx.roots;
    res.info.push(("info.root".into(), ctx.root.clone(), format!("src.{}", roots.root_source)));
    res.info.push(("info.global_root".into(), ctx.global_root.clone(), format!("src.{}", roots.global_source)));
    let found = if is_file(&roots.config_path) { "status.found" } else { "status.not_found" };
    res.info.push(("info.config".into(), roots.config_path.clone(), found.into()));

    res.checked += 1;
    if !is_dir(&ctx.root) {
        res.add(Sev::Error, &ctx.root, "root_missing");
    } else if !is_dir(&join(&ctx.root, "apps")) {
        res.add(Sev::Error, &ctx.root, "root_no_apps");
    }
    for (name, scope, value) in &roots.vars {
        res.checked += 1;
        let ok = is_dir(value);
        res.info.push((format!("var:{name} ({scope})"), value.clone(), if ok { "status.exists" } else { "status.missing" }.into()));
        let item = format!("{name} ({scope})");
        if !ok {
            res.add(Sev::Error, &item, "var_missing_dir").actual(value.clone());
        } else if let Some(e) = ctx.evaluate(&pref(value), false).filter(|e| e.code == "other_root") {
            add_eval(&mut res, Sev::Error, &item, &e, None);
        }
    }
    if !roots.vars.iter().any(|(n, _, _)| n == "SCOOP") {
        res.info.push(("var:SCOOP".into(), "-".into(), "status.not_set".into()));
    }
    for key in ["root_path", "global_path"] {
        let value = roots.config.get(key).str();
        if !value.is_empty() {
            res.checked += 1;
            if !is_dir(value) {
                res.add(Sev::Error, &format!("config.json {key}"), "var_missing_dir").actual(value);
            }
        }
    }
    let env_root = roots.vars.iter().find(|(n, _, _)| n == "SCOOP").map(|v| v.2.clone());
    let cfg_root = roots.config.get("root_path").str();
    if let Some(env_root) = env_root {
        if !cfg_root.is_empty() && norm(&env_root) != norm(cfg_root) {
            res.add(Sev::Warning, "SCOOP / root_path", "root_differs").actual(cfg_root).expected(Some(env_root));
        }
    }
    res
}

// ---------------------------------------------------------------------------
// shims
// ---------------------------------------------------------------------------

/// (base name, kind, is alternative)
pub fn shim_kind(name: &str) -> Option<(String, &'static str, bool)> {
    let low = name.to_lowercase();
    if let Some(b) = low.strip_suffix(".exe") {
        return Some((b.to_string(), "exe", false));
    }
    for (ext, kind) in [(".shim", "shim"), (".cmd", "cmd"), (".ps1", "ps1")] {
        if let Some(b) = low.strip_suffix(ext) {
            return Some((b.to_string(), kind, false));
        }
        if let Some(i) = low.find(&format!("{ext}.")) {
            return Some((low[..i].to_string(), kind, true));
        }
    }
    None
}

fn value_after(line: &str, key: &str) -> Option<String> {
    let l = line.trim_start();
    if l.len() < key.len() || !l.is_char_boundary(key.len()) || !l[..key.len()].eq_ignore_ascii_case(key) {
        return None;
    }
    let rest = l[key.len()..].trim_start().strip_prefix('=')?.trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

fn lines(text: &str) -> Vec<&str> {
    text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect()
}

/// (target, other paths) of one shim file.
pub fn parse_shim(kind: &str, text: &str, shim_dir: &str) -> (Option<PathRef>, Vec<PathRef>) {
    let mut ls = lines(text);
    if ls.last() == Some(&"") {
        ls.pop();
    }
    if kind == "shim" {
        let mut target = None;
        let mut others = Vec::new();
        for line in &ls {
            if target.is_none() {
                if let Some(v) = value_after(line, "path") {
                    target = Some(pref(&v));
                    continue;
                }
            }
            if let Some(v) = value_after(line, "args") {
                others.extend(extract_paths(&v));
            }
        }
        return (target, others);
    }
    let first = ls.first().map(|l| l.trim()).unwrap_or("");
    let mut target = None;
    match kind {
        "cmd" => {
            let low = first.to_ascii_lowercase();
            if low.starts_with("@rem") && first[4..].starts_with(char::is_whitespace) && !first[4..].trim().is_empty() {
                target = Some(pref(first[4..].trim()));
            }
        }
        "ps1" => {
            if first.starts_with('#') && first.contains(':') {
                target = Some(pref(first.trim_start_matches('#').trim()));
            } else if let Some(rel) = ps1_relative(text) {
                target = Some(pref(&normpath(&join(shim_dir, &rel))));
            }
        }
        "bash" => {
            let second = ls.get(1).map(|l| l.trim()).unwrap_or("");
            if second.starts_with('#') && second.contains(':') {
                target = Some(pref(second.trim_start_matches('#').trim()));
            }
        }
        _ => {}
    }
    let mut found = extract_paths(text);
    if target.is_none() && !found.is_empty() {
        target = Some(found.remove(0));
    }
    let tn = target.as_ref().map(|t| norm(&t.shown()));
    let others = found.into_iter().filter(|p| Some(norm(&p.shown())) != tn).collect();
    (target, others)
}

/// `Join-Path "$PSScriptRoot" "<rel>"` in a PowerShell shim.
fn ps1_relative(text: &str) -> Option<String> {
    let low = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = low[from..].find("join-path") {
        let start = from + i + "join-path".len();
        from = start;
        let rest = &low[start..];
        let t = rest.trim_start();
        if t.len() == rest.len() {
            continue;
        }
        let t = t.strip_prefix('"').unwrap_or(t);
        let Some(t) = t.strip_prefix("$psscriptroot") else { continue };
        let t = t.strip_prefix('"').unwrap_or(t);
        let t2 = t.trim_start();
        if t2.len() == t.len() {
            continue;
        }
        let Some(t3) = t2.strip_prefix('"') else { continue };
        let Some(end) = t3.find('"') else { continue };
        if end == 0 {
            continue;
        }
        let off = text.len() - t3.len();
        return Some(text[off..off + end].to_string());
    }
    None
}

pub fn check_shims(ctx: &Context) -> CheckResult {
    let mut res = CheckResult::new("shims");
    for (root, scope) in ctx.scope_roots() {
        let folder = join(&root, "shims");
        let Ok(rd) = std::fs::read_dir(&folder) else {
            if scope == "user" {
                res.add(Sev::Error, &folder, "dir_missing");
            }
            continue;
        };
        let entries: Vec<_> = rd.flatten().collect();
        let names: HashSet<String> = entries.iter().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
        for e in entries {
            let path = e.path().to_string_lossy().to_string();
            let Ok(md) = e.metadata() else { continue };
            let md = if md.file_type().is_symlink() { std::fs::metadata(&path).ok() } else { Some(md) };
            let Some(md) = md.filter(|m| m.is_file()) else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            let read = || ctx.cache.get("shim", &path, stamp_of(&md), || read_text(&path, 65536));
            let (base, kind, alt, text) = match shim_kind(&name) {
                Some((b, k, a)) => (b, k, a, None),
                None => match read() {
                    Some(t) if t.starts_with("#!") => (String::new(), "bash", false, Some(t)),
                    _ => continue,
                },
            };
            res.checked += 1;
            if kind == "exe" {
                if !names.contains(&format!("{base}.shim")) {
                    res.add(Sev::Warning, &path, "exe_no_shim");
                }
                continue;
            }
            if kind == "shim" && !alt && !names.contains(&format!("{base}.exe")) {
                res.add(Sev::Error, &path, "shim_no_exe");
            }
            let Some(text) = text.or_else(read) else {
                res.add(Sev::Warning, &path, "unreadable");
                continue;
            };
            let (target, others) = parse_shim(kind, &text, &folder);
            let Some(target) = target else {
                // Some manifests copy plain scripts into shims (lf's lfcd.ps1);
                // only a .shim file without a path is a fault.
                if kind == "shim" {
                    res.add(Sev::Warning, &path, "shim_no_target");
                }
                continue;
            };
            let sev = if alt { Sev::Warning } else { Sev::Error };
            if let Some(ev) = ctx.evaluate(&target, true) {
                add_eval(&mut res, sev, &path, &ev, None);
            }
            for o in &others {
                if let Some(ev) = ctx.evaluate(o, false) {
                    add_eval(&mut res, sev, &path, &ev, None);
                }
            }
        }
    }
    res
}

// ---------------------------------------------------------------------------
// current
// ---------------------------------------------------------------------------

pub fn check_current(ctx: &Context) -> CheckResult {
    let mut res = CheckResult::new("current");
    let no_junction = matches!(ctx.roots.config.get("no_junction"), Json::Bool(true));
    for app in ctx.apps() {
        res.checked += 1;
        match link_kind(&app.current) {
            None => {
                res.add(Sev::Error, &app.dir, "no_current");
            }
            Some(LinkKind::Junction | LinkKind::Symlink) => {
                let Some(target) = read_link(&app.current) else {
                    res.add(Sev::Error, &app.current, "link_unreadable");
                    continue;
                };
                if let Some(e) = ctx.evaluate(&pref(&target), false).filter(|e| e.code == "other_root") {
                    add_eval(&mut res, Sev::Error, &app.current, &e, None);
                } else if !is_dir(&target) {
                    res.add(Sev::Error, &app.current, "link_broken").actual(target);
                } else if norm(&dirname(&target)) != norm(&app.dir) {
                    res.add(Sev::Warning, &app.current, "current_elsewhere").actual(target).expected(Some(join(&app.dir, "<version>")));
                }
            }
            Some(LinkKind::Dir) => {
                if is_empty_dir(&app.current) {
                    let v = version_dirs(&app.dir).into_iter().next();
                    res.add(Sev::Error, &app.current, "current_empty").expected(v);
                } else if !no_junction && !app.name.eq_ignore_ascii_case("scoop") {
                    // Scoop's own 'current' is a git clone, not a junction.
                    res.add(Sev::Warning, &app.current, "current_not_link");
                }
            }
            Some(LinkKind::File) => {
                res.add(Sev::Error, &app.current, "current_not_link");
            }
        }
    }
    res
}

// ---------------------------------------------------------------------------
// persist
// ---------------------------------------------------------------------------

/// (source, target) pairs of a manifest's `persist` field.
pub fn persist_entries(manifest: Option<&Json>) -> Vec<(String, String)> {
    let Some(m) = manifest else { return Vec::new() };
    let value = m.get("persist");
    let items: Vec<&Json> = match value {
        Json::Str(_) => vec![value],
        Json::Arr(a) => a.iter().collect(),
        _ => return Vec::new(),
    };
    let strip = |s: &str| s.trim_matches(|c| c == '\\' || c == '/' || c == ' ').to_string();
    let mut out = Vec::new();
    for item in items {
        match item {
            Json::Str(s) => out.push((strip(s), strip(s))),
            Json::Arr(a) if !a.is_empty() => {
                let Json::Str(src) = &a[0] else { continue };
                let dst = match a.get(1) {
                    Some(Json::Str(d)) if !d.is_empty() => d.clone(),
                    Some(Json::Str(_)) | None | Some(Json::Null) => src.clone(),
                    _ => continue,
                };
                out.push((strip(src), strip(&dst)));
            }
            _ => {}
        }
    }
    out
}

pub fn check_persist(ctx: &Context) -> CheckResult {
    let mut res = CheckResult::new("persist");
    for app in ctx.apps().iter().filter(|a| a.usable) {
        for (src, dst) in persist_entries(app.manifest.as_ref()) {
            res.checked += 1;
            let here = join(&app.current, &src);
            let store = join(&ctx.persist_dir(app), &dst);
            match link_kind(&here) {
                None => {
                    res.add(Sev::Error, &here, "persist_missing").expected(Some(store));
                }
                Some(LinkKind::Junction | LinkKind::Symlink) => {
                    let target = read_link(&here).unwrap_or_default();
                    let other = if target.is_empty() { None } else { ctx.evaluate(&pref(&target), false) };
                    if let Some(mut e) = other.filter(|e| e.code == "other_root") {
                        e.expected = Some(store);
                        add_eval(&mut res, Sev::Error, &here, &e, None);
                    } else if norm(&target) != norm(&store) {
                        res.add(Sev::Error, &here, "link_wrong").actual(target).expected(Some(store));
                    } else if !exists(&target) {
                        res.add(Sev::Error, &here, "link_broken").actual(target);
                    }
                }
                Some(LinkKind::File) => {
                    if !exists(&store) {
                        res.add(Sev::Warning, &here, "persist_store_missing").expected(Some(store));
                    } else if !same_file(&here, &store) {
                        res.add(Sev::Warning, &here, "persist_not_linked").expected(Some(store));
                    }
                }
                Some(LinkKind::Dir) => {
                    res.add(Sev::Warning, &here, "persist_not_linked").expected(Some(store));
                }
            }
        }
    }
    res
}

// ---------------------------------------------------------------------------
// install state and buckets
// ---------------------------------------------------------------------------

fn bucket_manifest(bucket_dir: &str, app: &str) -> bool {
    is_file(&join(&join(bucket_dir, "bucket"), &format!("{app}.json"))) || is_file(&join(bucket_dir, &format!("{app}.json")))
}

pub fn check_install(ctx: &Context) -> CheckResult {
    let mut res = CheckResult::new("install");
    let buckets_dir = join(&ctx.root, "buckets");
    let mut buckets: BTreeMap<String, String> = BTreeMap::new();
    match std::fs::read_dir(&buckets_dir) {
        Ok(rd) => {
            for e in rd.flatten().filter(|e| e.path().is_dir()) {
                buckets.insert(e.file_name().to_string_lossy().to_lowercase(), e.path().to_string_lossy().to_string());
            }
        }
        Err(_) => {
            res.add(Sev::Error, &buckets_dir, "dir_missing");
        }
    }
    for path in buckets.values() {
        res.checked += 1;
        if !exists(&join(path, ".git")) {
            res.add(Sev::Warning, path, "bucket_not_git");
        }
    }
    let mut missing: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for app in ctx.apps() {
        if app.name.eq_ignore_ascii_case("scoop") {
            continue;
        }
        res.checked += 1;
        if !app.usable {
            continue; // reported by the "current" check
        }
        if !app.install_exists || !app.manifest_exists {
            let files: Vec<&str> = [("scoop-install.json", app.install_exists), ("scoop-manifest.json", app.manifest_exists)]
                .iter()
                .filter(|(_, ok)| !ok)
                .map(|(n, _)| *n)
                .collect();
            res.add(Sev::Error, &app.dir, "install_failed").with("files", files.join(", "));
            continue;
        }
        let (Some(install), Some(_)) = (&app.install, &app.manifest) else {
            res.add(Sev::Error, &app.dir, "json_invalid");
            continue;
        };
        let bucket = install.get("bucket").str();
        if bucket.is_empty() {
            continue; // installed from a URL or a local file
        }
        match buckets.get(&bucket.to_lowercase()) {
            None => missing.entry(bucket.to_string()).or_default().push(app.name.clone()),
            Some(bdir) => {
                let in_db =
                    ctx.scoop_db.as_ref().is_some_and(|db| db.contains(&(bucket.to_lowercase(), app.name.to_lowercase())));
                if !in_db && !bucket_manifest(bdir, &app.name) {
                    res.add(Sev::Warning, &app.dir, "manifest_gone").with("bucket", bucket);
                }
            }
        }
    }
    for (bucket, apps) in missing {
        let mut list = apps.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
        if apps.len() > 8 {
            list.push_str(" ...");
        }
        res.add(Sev::Error, &join(&buckets_dir, &bucket), "bucket_missing").with("count", apps.len().to_string()).with("apps", list);
    }
    res
}

// ---------------------------------------------------------------------------
// Start menu shortcuts
// ---------------------------------------------------------------------------

pub fn shortcut_dirs(environ: &HashMap<String, String>) -> Vec<String> {
    let sub = r"Microsoft\Windows\Start Menu\Programs\Scoop Apps";
    let mut out = Vec::new();
    if let Some(a) = environ.get("APPDATA").filter(|s| !s.is_empty()) {
        out.push(join(a, sub));
    }
    if let Some(p) = environ.get("ProgramData").or_else(|| environ.get("PROGRAMDATA")).filter(|s| !s.is_empty()) {
        out.push(join(p, sub));
    }
    out
}

fn files_below(folder: &str, out: &mut Vec<(String, std::fs::Metadata)>) {
    let Ok(rd) = std::fs::read_dir(folder) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path().to_string_lossy().to_string();
        if ft.is_dir() {
            files_below(&p, out);
        } else if let Ok(md) = e.metadata() {
            out.push((p, md));
        }
    }
}

fn opt(s: &Option<String>) -> String {
    match s {
        Some(v) => format!("+{v}"),
        None => "-".into(),
    }
}

fn unopt(s: &str) -> Option<String> {
    s.strip_prefix('+').map(str::to_string)
}

/// Strip the `,0` icon index from an icon location.
fn icon_path(s: &str) -> &str {
    if let Some(i) = s.rfind(',') {
        let idx = s[i + 1..].trim_start();
        let digits = idx.strip_prefix('-').unwrap_or(idx);
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            return &s[..i];
        }
    }
    s
}

pub fn check_shortcuts(ctx: &Context, folders: Option<Vec<String>>) -> CheckResult {
    let mut res = CheckResult::new("shortcuts");
    for folder in folders.unwrap_or_else(|| shortcut_dirs(&ctx.environ)) {
        let mut files = Vec::new();
        files_below(&folder, &mut files);
        files.sort_by(|a, b| a.0.cmp(&b.0));
        for (path, md) in files {
            if !path.to_lowercase().ends_with(".lnk") {
                continue;
            }
            res.checked += 1;
            let enc = ctx.cache.get("lnk", &path, stamp_of(&md), || {
                lnk::read(&path).map(|s| [opt(&s.target), opt(&s.working_dir), opt(&s.arguments), opt(&s.icon_location)].join("\x1f"))
            });
            let fields: Vec<Option<String>> = enc.as_deref().map(|e| e.split('\x1f').map(unopt).collect()).unwrap_or_default();
            let get = |i: usize| fields.get(i).cloned().flatten().filter(|s| !s.is_empty());
            let Some(target) = get(0) else {
                res.add(Sev::Warning, &path, "lnk_unreadable");
                continue;
            };
            if let Some(e) = ctx.evaluate(&pref(&target), true) {
                add_eval(&mut res, Sev::Error, &path, &e, None);
                continue; // working folder and icon usually share the fault
            }
            let mut found = Vec::new();
            for extra in [get(1), get(3)].into_iter().flatten() {
                if let Some(e) = ctx.evaluate(&pref(icon_path(&extra)), false) {
                    found.push(e);
                }
            }
            if let Some(a) = get(2) {
                found.extend(ctx.evaluate_text(&a, false));
            }
            add_grouped(&mut res, &path, &found, Sev::Warning, Sev::Warning);
        }
    }
    res
}

// ---------------------------------------------------------------------------
// .reg files and the registry
// ---------------------------------------------------------------------------

/// Paths found in a text file, encoded for the cache.
fn encode_refs(refs: &[PathRef]) -> String {
    refs.iter().map(|r| r.candidates.join("\x1f")).collect::<Vec<_>>().join("\x1e")
}

fn decode_refs(s: &str) -> Vec<PathRef> {
    if s.is_empty() {
        return Vec::new();
    }
    s.split('\x1e').map(|r| PathRef { candidates: r.split('\x1f').map(str::to_string).collect() }).collect()
}

/// The paths in a text file (from the cache when it did not change); None when unreadable.
fn file_refs(ctx: &Context, kind: &str, path: &str, stamp: (i64, i64), limit: usize) -> Option<Vec<PathRef>> {
    ctx.cache
        .get(kind, path, stamp, || {
            let text = read_text(path, limit)?;
            if !text.contains(":\\") && !text.contains(":/") {
                return Some(String::new());
            }
            Some(encode_refs(&extract_paths(&text)))
        })
        .map(|s| decode_refs(&s))
}

pub fn check_regfiles(ctx: &Context) -> CheckResult {
    let mut res = CheckResult::new("regfiles");
    for app in ctx.apps() {
        let Ok(rd) = std::fs::read_dir(&app.current) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path().to_string_lossy().to_string();
            if !path.to_lowercase().ends_with(".reg") {
                continue;
            }
            let Ok(md) = std::fs::metadata(&path) else { continue };
            if !md.is_file() {
                continue;
            }
            res.checked += 1;
            match file_refs(ctx, "reg", &path, stamp_of(&md), 1 << 20) {
                None => {
                    res.add(Sev::Warning, &path, "unreadable");
                }
                Some(refs) => add_grouped(&mut res, &path, &ctx.evaluate_refs(&refs, false), Sev::Error, Sev::Warning),
            }
        }
    }
    res
}

/// hive, key, missing files anywhere count (false: only under a Scoop root)
pub const REGISTRY_KEYS: [(&str, &str, bool); 5] = [
    ("HKCU", r"Software\Classes", false),
    ("HKCU", r"Software\Microsoft\Windows\CurrentVersion\Run", true),
    ("HKCU", r"Software\Microsoft\Windows\CurrentVersion\RunOnce", true),
    ("HKCU", r"Software\Microsoft\Windows\CurrentVersion\App Paths", true),
    ("HKCU", r"Software\Microsoft\Windows\CurrentVersion\Uninstall", false),
];

pub fn check_registry(ctx: &Context) -> CheckResult {
    let mut res = CheckResult::new("registry");
    for (hive, key, anywhere) in REGISTRY_KEYS {
        for (path, name, value, vtype) in ctx.registry.walk(hive, key, 8) {
            res.checked += 1;
            let text = if vtype == REG_EXPAND_SZ { expand(&value) } else { value };
            let shown = if name.is_empty() { "(default)" } else { &name };
            let item = format!("{hive}\\{path} [{shown}]");
            for e in ctx.evaluate_text(&text, anywhere) {
                add_eval(&mut res, Sev::Error, &item, &e, Some(Sev::Warning)).fixdata =
                    Some((hive.to_string(), path.clone(), name.clone()));
            }
        }
    }
    res
}

// ---------------------------------------------------------------------------
// environment variables
// ---------------------------------------------------------------------------

const LIST_VARS: [&str; 3] = ["path", "psmodulepath", "pathext"];

fn looks_like_path(value: &str) -> bool {
    let v = value.trim_start();
    let v = v.strip_prefix('"').unwrap_or(v);
    let b = v.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || v.starts_with(r"\\")
}

pub fn check_env(ctx: &Context) -> CheckResult {
    let mut res = CheckResult::new("env");
    let shims_n: HashSet<String> = ctx.scope_roots().iter().map(|(r, _)| norm(&join(r, "shims"))).collect();
    let mut shims_seen = false;
    for (scope, (hive, key)) in [("user", USER_ENV), ("machine", MACHINE_ENV)] {
        for (name, value, _) in ctx.registry.values(hive, key) {
            let low = name.to_lowercase();
            if low == "pathext" {
                continue;
            }
            let parts: Vec<String> = if LIST_VARS.contains(&low.as_str()) || (value.contains(';') && looks_like_path(&value)) {
                value.split(';').filter(|p| !p.trim().is_empty()).map(str::to_string).collect()
            } else if looks_like_path(&expand(&value)) {
                vec![value.clone()]
            } else {
                continue;
            };
            let fixdata = Some((hive.to_string(), key.to_string(), name.clone()));
            for part in parts {
                res.checked += 1;
                let text = expand(&part);
                let text = text.trim().trim_matches('"').to_string();
                let item = format!("{name} ({scope})");
                if low == "path" && shims_n.contains(&norm(&text)) {
                    shims_seen = true;
                }
                if exists(&text) {
                    if let Some(e) = ctx.evaluate(&pref(&text), false) {
                        add_eval(&mut res, Sev::Error, &item, &e, None).fixdata = fixdata.clone();
                    }
                    continue;
                }
                let p = if LIST_VARS.contains(&low.as_str()) {
                    PathRef { candidates: vec![clean(&text)] }
                } else {
                    match extract_paths(&text).into_iter().next() {
                        Some(p) => p,
                        None => continue,
                    }
                };
                // Default PowerShell module folders often do not exist; only
                // Scoop entries of PSModulePath are checked for existence.
                if let Some(e) = ctx.evaluate(&p, low != "psmodulepath") {
                    add_eval(&mut res, Sev::Error, &item, &e, Some(Sev::Warning)).fixdata = fixdata.clone();
                }
            }
        }
    }
    if !shims_seen {
        res.add(Sev::Warning, "PATH", "shims_not_in_path").expected(Some(join(&ctx.root, "shims")));
    }
    res
}

// ---------------------------------------------------------------------------
// configuration files
// ---------------------------------------------------------------------------

const CONFIG_EXTS: [&str; 15] =
    ["ini", "json", "cfg", "conf", "config", "xml", "toml", "yaml", "yml", "properties", "prefs", "settings", "rc", "env", "jsonc"];
const CONFIG_NAMES: [&str; 7] = ["gitconfig", ".gitconfig", "config", "settings", ".npmrc", ".yarnrc", "npmrc"];
const SKIP_DIRS: [&str; 6] = ["node_modules", ".git", "__pycache__", "site-packages", ".hg", ".svn"];

pub fn is_config_name(name: &str) -> bool {
    let low = name.to_lowercase();
    if INSTALL_NAMES.contains(&low.as_str()) || MANIFEST_NAMES.contains(&low.as_str()) {
        return false;
    }
    if CONFIG_NAMES.contains(&low.as_str()) {
        return true;
    }
    // Python's splitext: a leading dot is not an extension.
    match low.rfind('.') {
        Some(i) if low[..i].chars().any(|c| c != '.') => CONFIG_EXTS.contains(&&low[i + 1..]),
        _ => false,
    }
}

/// Config-like files below `top`, without following junctions or links.
fn config_files(top: &str, max_depth: usize, max_size: u64, out: &mut Vec<(String, (i64, i64))>) {
    let mut stack = vec![(top.to_string(), 0usize)];
    while let Some((folder, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&folder) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            // On Windows a junction or link is neither is_dir nor is_file here.
            if ft.is_dir() {
                if depth < max_depth && !SKIP_DIRS.contains(&name.to_lowercase().as_str()) {
                    stack.push((e.path().to_string_lossy().to_string(), depth + 1));
                }
            } else if ft.is_file() && is_config_name(&name) {
                if let Ok(md) = e.metadata() {
                    if md.len() <= max_size {
                        out.push((e.path().to_string_lossy().to_string(), stamp_of(&md)));
                    }
                }
            }
        }
    }
}

/// Run `job` over `items` on a few threads; results in the order of `items`.
pub fn parallel<T: Sync, R: Send>(items: &[T], job: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 16).min(items.len().max(1));
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= items.len() {
                    break;
                }
                *slots[i].lock().unwrap() = Some(job(&items[i]));
            });
        }
    });
    slots.into_iter().map(|s| s.into_inner().unwrap().unwrap()).collect()
}

pub fn check_configs(ctx: &Context) -> CheckResult {
    let mut res = CheckResult::new("configs");
    // (top folder, depth): each app's current, then each folder of persist.
    let mut jobs: Vec<(String, usize)> = ctx.apps().iter().filter(|a| a.usable).map(|a| (a.current.clone(), ctx.config_depth)).collect();
    for (root, _) in ctx.scope_roots() {
        let persist = join(&root, "persist");
        if !is_dir(&persist) {
            continue;
        }
        jobs.push((persist.clone(), 0));
        if ctx.persist_depth == 0 {
            continue;
        }
        if let Ok(rd) = std::fs::read_dir(&persist) {
            let mut subs: Vec<_> = rd
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .filter(|e| !SKIP_DIRS.contains(&e.file_name().to_string_lossy().to_lowercase().as_str()))
                .map(|e| e.path().to_string_lossy().to_string())
                .collect();
            subs.sort();
            jobs.extend(subs.into_iter().map(|s| (s, ctx.persist_depth - 1)));
        }
    }
    let max = ctx.max_config_size as u64;
    let parts = parallel(&jobs, |(top, depth)| {
        let mut files = Vec::new();
        config_files(top, *depth, max, &mut files);
        files.sort();
        let mut part = CheckResult::new("configs");
        for (path, stamp) in files {
            part.checked += 1;
            if let Some(refs) = file_refs(ctx, "cfg", &path, stamp, ctx.max_config_size) {
                add_grouped(&mut part, &path, &ctx.evaluate_refs(&refs, false), Sev::Error, Sev::Warning);
            }
        }
        part
    });
    for p in parts {
        res.checked += p.checked;
        res.problems.extend(p.problems);
    }
    res
}

pub fn run_check(ctx: &Context, id: &str) -> CheckResult {
    match id {
        "roots" => check_roots(ctx),
        "shims" => check_shims(ctx),
        "current" => check_current(ctx),
        "persist" => check_persist(ctx),
        "install" => check_install(ctx),
        "shortcuts" => check_shortcuts(ctx, None),
        "regfiles" => check_regfiles(ctx),
        "registry" => check_registry(ctx),
        "env" => check_env(ctx),
        _ => check_configs(ctx),
    }
}
