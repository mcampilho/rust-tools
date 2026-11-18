//! --fix: repair what the checks found, keeping a backup of everything changed.
//!
//! - `current` that is an empty folder or a broken junction -> junction to the newest version folder;
//! - persisted items missing or linked to the wrong place -> junction (folder) or hard link (file);
//! - shims, .reg files and config files with paths inside another root -> old root replaced in the text;
//! - Start menu shortcuts -> re-created through WScript.Shell with the old root replaced;
//! - HKCU values and environment variables -> old root replaced, type kept.

use crate::checks::{is_empty_dir, version_dirs, CheckResult, Context, Problem};
use crate::paths::{clean, cp1252_decode, cp1252_encode, dirname, exists, is_dir, join, looks_utf16le, norm, u16s};
use crate::registry::RegData;
use crate::report::js;
use crate::sys::{self, link_kind, LinkKind};
use std::collections::{BTreeMap, HashMap};

pub const FIX_ORDER: [&str; 8] = ["current", "persist", "shims", "regfiles", "configs", "shortcuts", "registry", "env"];
const OTHER_CODES: [&str; 2] = ["other_root", "other_root_n"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Done,
    Planned,
    Failed,
    Skipped,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Done => "done",
            Status::Planned => "planned",
            Status::Failed => "failed",
            Status::Skipped => "skipped",
        }
    }
}

pub struct Entry {
    pub status: Status,
    pub check: &'static str,
    pub item: String,
    pub key: &'static str,
    pub args: Vec<(String, String)>,
}

#[derive(Default)]
pub struct FixLog {
    pub entries: Vec<Entry>,
}

impl FixLog {
    fn add(&mut self, status: Status, check: &'static str, item: &str, key: &'static str, args: &[(&str, String)]) {
        let args = args.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        self.entries.push(Entry { status, check, item: item.to_string(), key, args });
    }

    pub fn count(&self, s: Status) -> usize {
        self.entries.iter().filter(|e| e.status == s).count()
    }
}

/// The spellings of a root found in text: `\`, `/` and the escaped `\\` of JSON and .reg files.
fn root_variants(root: &str) -> [String; 3] {
    let r = clean(root);
    [r.clone(), r.replace('\\', "/"), r.replace('\\', "\\\\")]
}

fn eq_ci(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// Replace each old root by its new one in all spellings: (text, count).
pub fn replace_roots(text: &str, pairs: &[(String, String)]) -> (String, usize) {
    let mut text = text.to_string();
    let mut total = 0;
    for (old, new) in pairs {
        for (o, n) in root_variants(old).iter().zip(root_variants(new).iter()) {
            let pat: Vec<char> = o.chars().collect();
            let t: Vec<char> = text.chars().collect();
            let mut out = String::with_capacity(text.len());
            let mut i = 0;
            while i < t.len() {
                let hit = i + pat.len() <= t.len()
                    && t[i..i + pat.len()].iter().zip(&pat).all(|(a, b)| eq_ci(*a, *b))
                    && t.get(i + pat.len()).is_none_or(|c| matches!(c, '\\' | '/' | '"' | '\'' | ';' | ',' | '=' | ')' | ']') || c.is_whitespace());
                if hit {
                    out.push_str(n);
                    i += pat.len();
                    total += 1;
                } else {
                    out.push(t[i]);
                    i += 1;
                }
            }
            text = out;
        }
    }
    (text, total)
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Enc {
    Utf8,
    Utf16Le,
    Utf16Be,
    Cp1252,
}

/// (text, encoding, BOM) of a text file's bytes.
fn decode(data: &[u8]) -> Result<(String, Enc, Vec<u8>), String> {
    let utf16 = |d: &[u8], big: bool| -> Result<String, String> {
        if !d.len().is_multiple_of(2) {
            return Err("odd UTF-16 length".into());
        }
        String::from_utf16(&u16s(d, big)).map_err(|e| e.to_string())
    };
    if let Some(d) = data.strip_prefix(b"\xef\xbb\xbf") {
        return Ok((String::from_utf8(d.to_vec()).map_err(|e| e.to_string())?, Enc::Utf8, b"\xef\xbb\xbf".to_vec()));
    }
    if let Some(d) = data.strip_prefix(b"\xff\xfe") {
        return Ok((utf16(d, false)?, Enc::Utf16Le, b"\xff\xfe".to_vec()));
    }
    if let Some(d) = data.strip_prefix(b"\xfe\xff") {
        return Ok((utf16(d, true)?, Enc::Utf16Be, b"\xfe\xff".to_vec()));
    }
    if looks_utf16le(data) {
        return Ok((utf16(data, false)?, Enc::Utf16Le, Vec::new()));
    }
    match std::str::from_utf8(data) {
        Ok(s) => Ok((s.to_string(), Enc::Utf8, Vec::new())),
        Err(_) => Ok((cp1252_decode(data), Enc::Cp1252, Vec::new())),
    }
}

fn encode(text: &str, enc: Enc) -> Result<Vec<u8>, String> {
    Ok(match enc {
        Enc::Utf8 => text.as_bytes().to_vec(),
        Enc::Utf16Le => text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect(),
        Enc::Utf16Be => text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect(),
        Enc::Cp1252 => cp1252_encode(text).ok_or("character not in Windows-1252")?,
    })
}

/// A shortcut to rewrite: path and (old, new) root pairs.
pub struct LnkJob {
    pub path: String,
    pub pairs: Vec<(String, String)>,
}

/// Rewrites shortcuts; {path: None when done, else the error}.
pub type PowerShellRunner = Box<dyn Fn(&[LnkJob]) -> Result<HashMap<String, Option<String>>, String>>;

pub struct Fixer<'c, 'a> {
    ctx: &'c Context<'a>,
    pub dry: bool,
    pub log: FixLog,
    pub backup_dir: String,
    registry_backup: Vec<String>,
    run_powershell: PowerShellRunner,
    env_changed: bool,
}

fn error(e: impl std::fmt::Display) -> Vec<(&'static str, String)> {
    vec![("error", e.to_string())]
}

impl<'c, 'a> Fixer<'c, 'a> {
    pub fn new(ctx: &'c Context<'a>, dry: bool, backup_root: Option<&str>, run_powershell: Option<PowerShellRunner>) -> Self {
        let base = match backup_root {
            Some(b) => b.to_string(),
            None => {
                let local = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().to_string());
                join(&join(&local, "rscoop"), "backup")
            }
        };
        Fixer {
            ctx,
            dry,
            log: FixLog::default(),
            backup_dir: join(&base, &sys::stamp()),
            registry_backup: Vec::new(),
            run_powershell: run_powershell.unwrap_or_else(|| Box::new(run_powershell_jobs)),
            env_changed: false,
        }
    }

    /// (old root, new root) pairs from the problems of one item.
    fn pairs_for(&self, problems: &[&Problem]) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for p in problems {
            if let Some(old) = p.arg("other") {
                if out.iter().all(|(o, _)| norm(o) != norm(old)) {
                    out.push((old.to_string(), self.ctx.root.clone()));
                }
            }
        }
        out
    }

    fn backup_file(&self, path: &str) -> std::io::Result<()> {
        let p = clean(path);
        let (drive, rest) = if p.as_bytes().get(1) == Some(&b':') { (p[..1].to_string(), &p[2..]) } else { ("_".to_string(), &p[..]) };
        let dest = join(&join(&self.backup_dir, &drive), rest.trim_start_matches(['\\', '/']));
        std::fs::create_dir_all(dirname(&dest))?;
        std::fs::copy(path, &dest).map(|_| ())
    }

    pub fn run(mut self, results: &[CheckResult], only: Option<&[String]>) -> (FixLog, String, bool) {
        for id in FIX_ORDER {
            if only.is_some_and(|o| !o.iter().any(|x| x == id)) {
                continue;
            }
            let Some(res) = results.iter().find(|r| r.id == id) else { continue };
            if res.problems.is_empty() {
                continue;
            }
            match id {
                "current" => self.fix_current(res),
                "persist" => self.fix_persist(res),
                "shims" | "regfiles" | "configs" => self.fix_text_files(res),
                "shortcuts" => self.fix_shortcuts(res),
                _ => self.fix_values(res),
            }
        }
        if !self.registry_backup.is_empty() && !self.dry {
            let body = format!("[\n{}\n]\n", self.registry_backup.join(",\n"));
            let file = join(&self.backup_dir, "registry.json");
            if let Err(e) = std::fs::create_dir_all(&self.backup_dir).and_then(|_| std::fs::write(&file, body)) {
                self.log.add(Status::Failed, "registry", &file, "fix.error", &error(e));
            }
        }
        if self.env_changed && !self.dry {
            sys::broadcast_environment_change();
        }
        (self.log, self.backup_dir, self.dry)
    }

    fn fix_current(&mut self, res: &CheckResult) {
        for p in res.problems.iter().filter(|p| p.code == "current_empty" || p.code == "link_broken") {
            let Some(target) = version_dirs(&dirname(&p.item)).into_iter().next() else {
                self.log.add(Status::Skipped, "current", &p.item, "fix.no_version", &[]);
                continue;
            };
            let args = [("target", target.clone())];
            if self.dry {
                self.log.add(Status::Planned, "current", &p.item, "act.junction", &args);
                continue;
            }
            let r = (|| -> std::io::Result<bool> {
                if p.code == "current_empty" {
                    if !is_empty_dir(&p.item) {
                        return Ok(false);
                    }
                    sys::make_writable(&p.item); // Scoop leaves some 'current' folders read-only
                    std::fs::remove_dir(&p.item)?;
                } else {
                    sys::remove_link(&p.item)?;
                }
                sys::make_junction(&p.item, &target)?;
                Ok(true)
            })();
            match r {
                Ok(true) => self.log.add(Status::Done, "current", &p.item, "act.junction", &args),
                Ok(false) => self.log.add(Status::Skipped, "current", &p.item, "fix.not_empty", &[]),
                Err(e) => self.log.add(Status::Failed, "current", &p.item, "fix.error", &error(e)),
            }
        }
    }

    fn fix_persist(&mut self, res: &CheckResult) {
        let codes = ["persist_missing", "link_wrong", "link_broken", "other_root"];
        for p in res.problems.iter().filter(|p| codes.contains(&p.code.as_str())) {
            let Some(store) = p.expected.clone() else { continue };
            let here = &p.item;
            if !exists(&store) {
                self.log.add(Status::Skipped, "persist", here, "fix.store_missing", &[("store", store)]);
                continue;
            }
            let folder = is_dir(&store);
            let key = if folder { "act.junction" } else { "act.hardlink" };
            let args = [("target", store.clone())];
            if self.dry {
                self.log.add(Status::Planned, "persist", here, key, &args);
                continue;
            }
            let r = (|| -> std::io::Result<bool> {
                match link_kind(here) {
                    Some(LinkKind::Junction | LinkKind::Symlink) => sys::remove_link(here)?,
                    Some(_) => return Ok(false),
                    None => {}
                }
                if folder {
                    sys::make_junction(here, &store)?;
                } else {
                    std::fs::hard_link(&store, here)?;
                }
                Ok(true)
            })();
            match r {
                Ok(true) => self.log.add(Status::Done, "persist", here, key, &args),
                Ok(false) => self.log.add(Status::Skipped, "persist", here, "fix.exists", &[]),
                Err(e) => self.log.add(Status::Failed, "persist", here, "fix.error", &error(e)),
            }
        }
    }

    fn by_item(res: &CheckResult) -> BTreeMap<&str, Vec<&Problem>> {
        let mut items: BTreeMap<&str, Vec<&Problem>> = BTreeMap::new();
        for p in res.problems.iter().filter(|p| OTHER_CODES.contains(&p.code.as_str())) {
            items.entry(p.item.as_str()).or_default().push(p);
        }
        items
    }

    fn fix_text_files(&mut self, res: &CheckResult) {
        for (path, problems) in Self::by_item(res) {
            let pairs = self.pairs_for(&problems);
            let r = (|| -> Result<Option<Vec<(&'static str, String)>>, String> {
                let data = std::fs::read(path).map_err(|e| e.to_string())?;
                let (text, enc, bom) = decode(&data)?;
                let (new, n) = replace_roots(&text, &pairs);
                if n == 0 {
                    return Ok(None);
                }
                let args = vec![("old", olds(&pairs)), ("new", self.ctx.root.clone()), ("n", n.to_string())];
                if self.dry {
                    return Ok(Some(args));
                }
                let mut bytes = bom;
                bytes.extend(encode(&new, enc)?);
                self.backup_file(path).map_err(|e| e.to_string())?;
                let was_read_only = sys::make_writable(path);
                let w = std::fs::write(path, bytes);
                if was_read_only {
                    sys::set_read_only(path);
                }
                w.map_err(|e| e.to_string())?;
                Ok(Some(args))
            })();
            match r {
                Ok(None) => self.log.add(Status::Skipped, res.id, path, "fix.no_change", &[]),
                Ok(Some(args)) => {
                    let s = if self.dry { Status::Planned } else { Status::Done };
                    self.log.add(s, res.id, path, "act.rewrite", &args)
                }
                Err(e) => self.log.add(Status::Failed, res.id, path, "fix.error", &error(e)),
            }
        }
    }

    fn fix_shortcuts(&mut self, res: &CheckResult) {
        let mut jobs = Vec::new();
        for (path, problems) in Self::by_item(res) {
            let pairs = self.pairs_for(&problems);
            let args = [("old", olds(&pairs)), ("new", self.ctx.root.clone())];
            if self.dry {
                self.log.add(Status::Planned, "shortcuts", path, "act.shortcut", &args);
                continue;
            }
            if let Err(e) = self.backup_file(path) {
                self.log.add(Status::Failed, "shortcuts", path, "fix.error", &error(e));
                continue;
            }
            jobs.push(LnkJob { path: path.to_string(), pairs });
        }
        if jobs.is_empty() {
            return;
        }
        let outcome = (self.run_powershell)(&jobs);
        for j in &jobs {
            let args = [("old", olds(&j.pairs)), ("new", self.ctx.root.clone())];
            match &outcome {
                Err(e) => self.log.add(Status::Failed, "shortcuts", &j.path, "fix.error", &error(e)),
                Ok(map) => match map.get(&j.path) {
                    Some(None) => self.log.add(Status::Done, "shortcuts", &j.path, "act.shortcut", &args),
                    Some(Some(e)) => self.log.add(Status::Failed, "shortcuts", &j.path, "fix.error", &error(e)),
                    None => self.log.add(Status::Failed, "shortcuts", &j.path, "fix.error", &error("no result")),
                },
            }
        }
    }

    fn fix_values(&mut self, res: &CheckResult) {
        let mut values: BTreeMap<(String, String, String), Vec<&Problem>> = BTreeMap::new();
        for p in res.problems.iter().filter(|p| OTHER_CODES.contains(&p.code.as_str())) {
            if let Some(fd) = &p.fixdata {
                values.entry(fd.clone()).or_default().push(p);
            }
        }
        for ((hive, key, name), problems) in values {
            let pairs = self.pairs_for(&problems);
            let shown = if name.is_empty() { "(default)" } else { &name };
            let item = format!("{hive}\\{key} [{shown}]");
            let f = |s: &str| replace_roots(s, &pairs).0;
            let (old, new, vtype) = match self.ctx.registry.replace_value(&hive, &key, &name, &f, !self.dry) {
                Ok(v) => v,
                Err(e) => {
                    self.log.add(Status::Failed, res.id, &item, "fix.error", &error(e));
                    continue;
                }
            };
            if new == old {
                self.log.add(Status::Skipped, res.id, &item, "fix.no_change", &[]);
                continue;
            }
            let args = [("old", olds(&pairs)), ("new", self.ctx.root.clone())];
            if self.dry {
                self.log.add(Status::Planned, res.id, &item, "act.registry", &args);
                continue;
            }
            self.registry_backup.push(format!(
                "  {{\"hive\": {}, \"key\": {}, \"name\": {}, \"type\": {vtype}, \"old\": {}, \"new\": {}}}",
                js(&hive),
                js(&key),
                js(&name),
                reg_json(&old),
                reg_json(&new)
            ));
            if res.id == "env" {
                self.env_changed = true;
            }
            self.log.add(Status::Done, res.id, &item, "act.registry", &args);
        }
    }
}

fn olds(pairs: &[(String, String)]) -> String {
    pairs.iter().map(|(o, _)| o.as_str()).collect::<Vec<_>>().join(", ")
}

fn reg_json(d: &RegData) -> String {
    match d {
        RegData::Str(s) => js(s),
        RegData::Multi(v) => format!("[{}]", v.iter().map(|s| js(s)).collect::<Vec<_>>().join(", ")),
    }
}

const PS_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$jobs = Get-Content -Raw -Encoding UTF8 -LiteralPath $args[0] | ConvertFrom-Json
$sh = New-Object -ComObject WScript.Shell
foreach ($j in $jobs) {
  try {
    $l = $sh.CreateShortcut($j.path)
    # Saving a shortcut WScript could not read would wipe its target.
    if (-not $l.TargetPath) { throw 'WScript.Shell cannot read the shortcut target' }
    $props = @{}
    foreach ($f in 'TargetPath', 'Arguments', 'WorkingDirectory', 'IconLocation', 'Description', 'Hotkey', 'WindowStyle') {
      $v = $l.$f
      if ($v -is [string]) {
        foreach ($p in $j.pairs) {
          $v = [regex]::Replace($v, [regex]::Escape($p.old), $p.new.Replace('$', '$$'), 'IgnoreCase')
        }
      }
      $props[$f] = $v
    }
    # Re-create the file: only setting TargetPath leaves the old path in LinkInfo.
    Remove-Item -LiteralPath $j.path -Force
    $n = $sh.CreateShortcut($j.path)
    foreach ($f in $props.Keys) { if ($props[$f]) { $n.$f = $props[$f] } }
    $n.Save()
    "OK`t" + $j.path
  } catch {
    "ERR`t" + $j.path + "`t" + $_.Exception.Message
  }
}
"#;

/// Rewrite shortcuts in one PowerShell process.
fn run_powershell_jobs(jobs: &[LnkJob]) -> Result<HashMap<String, Option<String>>, String> {
    let folder = std::env::temp_dir().join(format!("rscoop-{}-{}", std::process::id(), sys::stamp()));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let result = (|| {
        let script = folder.join("fix-lnk.ps1");
        let data = folder.join("jobs.json");
        std::fs::write(&script, format!("\u{feff}{PS_SCRIPT}")).map_err(|e| e.to_string())?;
        let body: Vec<String> = jobs
            .iter()
            .map(|j| {
                let pairs: Vec<String> = j.pairs.iter().map(|(o, n)| format!("{{\"old\": {}, \"new\": {}}}", js(o), js(n))).collect();
                format!("{{\"path\": {}, \"pairs\": [{}]}}", js(&j.path), pairs.join(", "))
            })
            .collect();
        std::fs::write(&data, format!("[{}]", body.join(",\n"))).map_err(|e| e.to_string())?;
        let out = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .arg(&data)
            .output()
            .map_err(|e| e.to_string())?;
        let mut map = HashMap::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let parts: Vec<&str> = line.trim_start_matches('\u{feff}').split('\t').collect();
            if parts.len() >= 2 && (parts[0] == "OK" || parts[0] == "ERR") {
                let err = (parts[0] == "ERR").then(|| parts.get(2).unwrap_or(&"error").to_string());
                map.insert(parts[1].to_string(), err);
            }
        }
        if map.is_empty() && !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().chars().take(300).collect());
        }
        Ok(map)
    })();
    let _ = std::fs::remove_dir_all(&folder);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings() {
        let text = r#"a="D:\Portable\Scoop\apps\x" b=D:/Portable/scoop/apps/y c="D:\\Portable\\scoop\\apps" d=D:\Portable\scoopy\x"#;
        let (new, n) = replace_roots(text, &[(r"D:\Portable\scoop".into(), r"E:\Portable\scoop".into())]);
        assert_eq!(n, 3);
        assert!(new.contains(r#"a="E:\Portable\scoop\apps\x""#));
        assert!(new.contains("b=E:/Portable/scoop/apps/y"));
        assert!(new.contains(r#"c="E:\\Portable\\scoop\\apps""#));
        assert!(new.contains(r"d=D:\Portable\scoopy\x"));
    }

    #[test]
    fn decode_keeps_encoding() {
        let (t, e, bom) = decode(b"\xff\xfea\x00b\x00").unwrap();
        assert_eq!((t.as_str(), e, bom.len()), ("ab", Enc::Utf16Le, 2));
        assert_eq!(encode("ab", e).unwrap(), b"a\x00b\x00");
        assert_eq!(decode(b"caf\xe9").unwrap().1, Enc::Cp1252);
    }
}
