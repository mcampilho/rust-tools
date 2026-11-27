//! `lib\system.ps1` and the `env_*` of `lib\install.ps1`: environment
//! variables and `PATH`, in the registry.

use crate::install::Scoop;
use crate::json::Json;
use crate::manifest::{strings, Arch, Manifest};
use crate::system::System;
use std::path::{Path, PathBuf};

/// What a change did, for the messages Scoop shows.
#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    /// A folder put at the front of a path variable.
    Added(String),
    /// A folder taken out.
    Removed(String),
    /// `env_set`: a variable written (name, value).
    Set(String, String),
    /// `env_rm`: a variable removed.
    Unset(String),
}

/// `$scoopPathEnvVar`: where the folders of the apps go.
pub fn path_variable(scoop: &Scoop) -> String {
    match scoop.setting("use_isolated_path") {
        Json::Str(s) if !s.is_empty() => s.to_uppercase(),
        Json::Bool(true) => "SCOOP_PATH".into(),
        _ => "PATH".into(),
    }
}

/// PowerShell's `-like`, ignoring case: `*` and `?` are wildcards.
fn like(text: &str, pattern: &str) -> bool {
    let (t, p): (Vec<char>, Vec<char>) = (text.to_lowercase().chars().collect(), pattern.to_lowercase().chars().collect());
    let (mut ti, mut pi, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            ti += 1;
            pi += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// `Split-PathLikeEnvVar`: the entries of `value` that match one of
/// `patterns` (in the order of the patterns), and the others.
pub fn split_like(patterns: &[String], value: &str) -> (Vec<String>, Vec<String>) {
    let mut rest: Vec<String> = value.split(';').filter(|s| !s.is_empty()).map(String::from).collect();
    let mut found = Vec::new();
    for pattern in patterns.iter().flat_map(|p| p.split(';')).filter(|s| !s.is_empty()) {
        let (matching, other): (Vec<String>, Vec<String>) = rest.into_iter().partition(|e| like(e, pattern));
        found.extend(matching);
        rest = other;
    }
    (found, rest)
}

/// `Set-EnvVar`: `REG_EXPAND_SZ` when the value has a `%`, else the kind it
/// had, else `REG_SZ`; an empty value removes the variable, if it has one.
pub fn set_var(system: &dyn System, name: &str, value: Option<&str>, global: bool) -> Result<(), String> {
    let old = system.env_get(name, global);
    let result = match value.filter(|v| !v.is_empty()) {
        None if old.as_ref().is_some_and(|(v, _)| !v.is_empty()) => system.env_put(name, global, None),
        None => Ok(()),
        Some(v) => {
            let expand = v.contains('%') || old.as_ref().is_some_and(|(o, expand)| !o.is_empty() && *expand);
            system.env_put(name, global, Some((v, expand)))
        }
    };
    system.env_changed();
    result
}

/// `Add-Path`: puts `paths` at the front of `variable`, unless one of them is
/// there already (`force`: in any case, moving them to the front).
pub fn add_path(system: &dyn System, paths: &[String], variable: &str, global: bool, force: bool) -> Result<Vec<Change>, String> {
    let current = system.env_get(variable, global).map(|(v, _)| v).unwrap_or_default();
    let (found, rest) = split_like(paths, &current);
    if !found.is_empty() && !force {
        return Ok(Vec::new());
    }
    let value: Vec<String> = paths.iter().cloned().chain(rest).collect();
    set_var(system, variable, Some(&value.join(";")), global)?;
    Ok(paths.iter().map(|p| Change::Added(p.clone())).collect())
}

/// `Remove-Path`.
pub fn remove_path(system: &dyn System, paths: &[String], variable: &str, global: bool) -> Result<Vec<Change>, String> {
    let current = system.env_get(variable, global).map(|(v, _)| v).unwrap_or_default();
    let (found, rest) = split_like(paths, &current);
    if found.is_empty() {
        return Ok(Vec::new());
    }
    set_var(system, variable, Some(&rest.join(";")), global)?;
    Ok(paths.iter().map(|p| Change::Removed(p.clone())).collect())
}

/// `Get-AbsolutePath`: absolute, `..` resolved, no separator at the end.
pub fn absolute(path: &Path) -> PathBuf {
    let full = crate::disk::plain(&std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()));
    let text = full.display().to_string();
    let trimmed = text.trim_end_matches(['\\', '/']);
    if trimmed.len() < text.len() && !trimmed.ends_with(':') && !trimmed.is_empty() {
        PathBuf::from(trimmed)
    } else {
        full
    }
}

/// `is_in_dir`: `check` is `dir` or under it.
pub fn is_in_dir(dir: &Path, check: &Path) -> bool {
    let (dir, check) = (absolute(dir).display().to_string().to_lowercase(), absolute(check).display().to_string().to_lowercase());
    if check == dir {
        return true;
    }
    let dir = if dir.ends_with(['\\', '/']) { dir } else { format!("{dir}\\") };
    check.starts_with(&dir)
}

/// The folders of `env_add_path`, under `dir`.
fn app_paths(manifest: &Manifest, dir: &Path, arch: Arch) -> Vec<String> {
    let dir = PathBuf::from(dir.display().to_string().trim_end_matches('\\'));
    strings(manifest.arch("env_add_path", arch))
        .iter()
        .map(|p| absolute(&dir.join(p)))
        .filter(|p| is_in_dir(&dir, p))
        .map(|p| p.display().to_string())
        .collect()
}

/// `env_add_path`.
pub fn env_add_path(
    system: &dyn System,
    scoop: &Scoop,
    manifest: &Manifest,
    dir: &Path,
    global: bool,
    arch: Arch,
) -> Result<Vec<Change>, String> {
    if strings(manifest.arch("env_add_path", arch)).is_empty() {
        return Ok(Vec::new());
    }
    let variable = path_variable(scoop);
    let mut changes = Vec::new();
    if scoop.enabled("use_isolated_path") || matches!(scoop.setting("use_isolated_path"), Json::Str(s) if !s.is_empty()) {
        changes.extend(add_path(system, &[format!("%{variable}%")], "PATH", global, false)?);
    }
    changes.extend(add_path(system, &app_paths(manifest, dir, arch), &variable, global, true)?);
    Ok(changes)
}

/// `env_rm_path`: from `PATH` and from the variable of the apps.
pub fn env_rm_path(
    system: &dyn System,
    scoop: &Scoop,
    manifest: &Manifest,
    dir: &Path,
    global: bool,
    arch: Arch,
) -> Result<Vec<Change>, String> {
    let paths = app_paths(manifest, dir, arch);
    if strings(manifest.arch("env_add_path", arch)).is_empty() {
        return Ok(Vec::new());
    }
    let mut changes = remove_path(system, &paths, "PATH", global)?;
    changes.extend(remove_path(system, &paths, &path_variable(scoop), global)?);
    Ok(changes)
}

/// The names and values of `env_set`, in the order of the manifest.
pub fn env_set_values(manifest: &Manifest, arch: Arch) -> Vec<(String, String)> {
    match manifest.arch("env_set", arch) {
        Json::Obj(members) => members.iter().map(|(k, v)| (k.clone(), v.str().to_string())).collect(),
        _ => Vec::new(),
    }
}

/// The variables a value of `env_set` can name, for `ExpandString`.
pub struct Vars<'a> {
    pub known: Vec<(&'a str, String)>,
    /// `$env:NAME`.
    pub env: &'a dyn Fn(&str) -> Option<String>,
}

/// `$ExecutionContext.InvokeCommand.ExpandString`, for the plain forms
/// `$name`, `${name}` and `$env:NAME`. None when the text has anything else
/// (an expression, an escape, a variable this program does not know): such
/// a value is left to Scoop.
pub fn expand(text: &str, vars: &Vars) -> Option<String> {
    if text.contains('`') || text.contains("$(") {
        return None;
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let (name, len) = if let Some(braced) = after.strip_prefix('{') {
            let end = braced.find('}')?;
            (&braced[..end], end + 2)
        } else {
            let end = after.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':')).unwrap_or(after.len());
            (&after[..end], end)
        };
        if name.is_empty() {
            // A `$` on its own stays.
            out.push('$');
        } else if let Some(var) = name.strip_prefix("env:").or_else(|| name.strip_prefix("ENV:")).or_else(|| name.strip_prefix("Env:")) {
            out.push_str(&(vars.env)(var).unwrap_or_default());
        } else {
            let value = vars.known.iter().find(|(k, _)| k.eq_ignore_ascii_case(name))?;
            out.push_str(&value.1);
        }
        rest = &after[len..];
    }
    out.push_str(rest);
    Some(out)
}

/// `env_set`, with the values already expanded.
pub fn env_set(system: &dyn System, values: &[(String, String)], global: bool) -> Result<Vec<Change>, String> {
    let mut changes = Vec::new();
    for (name, value) in values {
        set_var(system, name, Some(value), global)?;
        changes.push(Change::Set(name.clone(), value.clone()));
    }
    Ok(changes)
}

/// `env_rm`.
pub fn env_rm(system: &dyn System, manifest: &Manifest, global: bool, arch: Arch) -> Result<Vec<Change>, String> {
    let mut changes = Vec::new();
    for (name, _) in env_set_values(manifest, arch) {
        set_var(system, &name, None, global)?;
        changes.push(Change::Unset(name));
    }
    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::FakeSystem;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn paths_are_added_once_and_removed() {
        let s = FakeSystem::default();
        s.env_put("Path", false, Some((r"C:\a;C:\b", false))).unwrap();
        assert_eq!(add_path(&s, &list(&[r"c:\B"]), "PATH", false, false).unwrap(), []);
        assert_eq!(add_path(&s, &list(&[r"C:\x"]), "PATH", false, false).unwrap(), [Change::Added(r"C:\x".into())]);
        assert_eq!(s.var("PATH", false).unwrap(), r"C:\x;C:\a;C:\b");
        // Forced: moved to the front.
        add_path(&s, &list(&[r"C:\b"]), "PATH", false, true).unwrap();
        assert_eq!(s.var("PATH", false).unwrap(), r"C:\b;C:\x;C:\a");
        assert_eq!(remove_path(&s, &list(&[r"C:\X"]), "PATH", false).unwrap(), [Change::Removed(r"C:\X".into())]);
        assert_eq!(s.var("PATH", false).unwrap(), r"C:\b;C:\a");
        assert_eq!(remove_path(&s, &list(&[r"C:\none"]), "PATH", false).unwrap(), []);
        // A `%` makes it REG_EXPAND_SZ, and a REG_EXPAND_SZ stays one.
        add_path(&s, &list(&["%SCOOP_PATH%"]), "PATH", false, false).unwrap();
        assert!(s.env_get("PATH", false).unwrap().1);
        remove_path(&s, &list(&["%SCOOP_PATH%"]), "PATH", false).unwrap();
        assert_eq!(s.env_get("PATH", false), Some((r"C:\b;C:\a".into(), true)));
        assert!(like(r"C:\Apps\X", r"c:\apps\*") && !like(r"C:\Apps", r"c:\apps\*"));
    }

    #[test]
    fn values_are_expanded_as_powershell_would() {
        let env = |n: &str| (n == "USERPROFILE").then(|| r"C:\Users\u".to_string());
        let vars = Vars { known: vec![("dir", r"C:\s\apps\x\current".into()), ("persist_dir", r"C:\s\persist\x".into())], env: &env };
        assert_eq!(expand(r"$dir\bin", &vars).as_deref(), Some(r"C:\s\apps\x\current\bin"));
        assert_eq!(expand(r"${persist_dir}\d;$env:USERPROFILE\x", &vars).as_deref(), Some(r"C:\s\persist\x\d;C:\Users\u\x"));
        assert_eq!(expand("cost: 5$", &vars).as_deref(), Some("cost: 5$"));
        assert_eq!(expand("$(Get-Date)", &vars), None);
        assert_eq!(expand("$HOME", &vars), None);
    }

    /// Windows paths (..\ is not a parent elsewhere).
    #[cfg(windows)]
    #[test]
    fn variables_of_a_manifest() {
        let s = FakeSystem::default();
        let t = crate::testing::Tree::new("env");
        let scoop = t.scoop();
        let dir = t.dir("root/apps/x/current");
        let m = Manifest(crate::json::parse(r#"{"env_add_path":["bin","..\\..\\outside","."],"env_set":{"X_HOME":"v"}}"#).unwrap());
        let changes = env_add_path(&s, &scoop, &m, &dir, false, Arch::X64).unwrap();
        // Only what is under the app's folder.
        assert_eq!(changes, [Change::Added(dir.join("bin").display().to_string()), Change::Added(dir.display().to_string())]);
        env_set(&s, &env_set_values(&m, Arch::X64), false).unwrap();
        assert_eq!(s.var("X_HOME", false).as_deref(), Some("v"));
        env_rm_path(&s, &scoop, &m, &dir, false, Arch::X64).unwrap();
        env_rm(&s, &m, false, Arch::X64).unwrap();
        // Emptied, the variable goes (`Set-EnvVar` with '').
        assert_eq!(s.var("PATH", false), None);
        assert_eq!(s.var("X_HOME", false), None);
    }
}
