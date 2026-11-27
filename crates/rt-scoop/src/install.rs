//! `lib\core.ps1`, "Core Bootstrap": which installation to work on.

use crate::json::{self, Json};
use crate::manifest::Arch;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// What the program was started with, so that tests can make it up.
#[derive(Clone, Debug, Default)]
pub struct Env {
    vars: HashMap<String, String>,
    pub home: PathBuf,
}

impl Env {
    pub fn new<I: IntoIterator<Item = (String, String)>>(vars: I) -> Env {
        let vars: HashMap<String, String> = vars.into_iter().map(|(k, v)| (k.to_ascii_uppercase(), v)).collect();
        let home = PathBuf::from(vars.get("USERPROFILE").map(String::as_str).unwrap_or(""));
        Env { vars, home }
    }

    /// A variable that is set and not empty (names ignore case, as on Windows).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(&name.to_ascii_uppercase()).map(String::as_str).filter(|v| !v.is_empty())
    }

    /// The folders of `PATH`.
    pub fn path(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.get("PATH").unwrap_or("").split(';').filter(|s| !s.is_empty()).map(PathBuf::from)
    }
}

/// Where the root came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Option,
    Variable,
    Config,
    /// The Scoop on `PATH`, or the one this program was installed with.
    Location,
    Default,
}

#[derive(Clone, Debug)]
pub struct Scoop {
    pub root: PathBuf,
    pub root_source: Source,
    pub global: PathBuf,
    pub cache: PathBuf,
    pub config_path: PathBuf,
    pub config: Json,
}

/// Scoop keeps itself in `<root>\apps\scoop\current`.
pub fn has_core(root: &Path) -> bool {
    root.join("apps").join("scoop").join("current").join("bin").join("scoop.ps1").is_file()
}

/// The root of the installation `exe` belongs to (`<root>\apps\<app>\<version>\x.exe`
/// or `<root>\shims\x`).
fn root_of_program(file: &Path) -> Option<PathBuf> {
    let dir = file.parent()?;
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase());
    if name(dir).as_deref() == Some("shims") {
        return dir.parent().map(Path::to_path_buf);
    }
    let apps = dir.parent()?.parent()?;
    (name(apps).as_deref() == Some("apps")).then(|| apps.parent().map(Path::to_path_buf))?
}

/// The installation of the `scoop` on `PATH`, else the one `exe` is in.
pub fn core_root(env: &Env, exe: Option<&Path>) -> Option<PathBuf> {
    let on_path = env
        .path()
        .find_map(|dir| ["scoop.ps1", "scoop.cmd"].iter().map(|n| dir.join(n)).find(|p| p.is_file()))
        .and_then(|shim| root_of_program(&shim))
        .filter(|r| has_core(r));
    on_path.or_else(|| exe.and_then(root_of_program).filter(|r| has_core(r)))
}

fn absolute(path: &str) -> PathBuf {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let p = PathBuf::from(if trimmed.ends_with(':') { path } else { trimmed });
    std::path::absolute(&p).unwrap_or(p)
}

impl Scoop {
    /// The installation to work on. `root_option` is `--root`; `located` is
    /// [`core_root`]. As in Scoop, a `config.json` in the root of the
    /// installation (portable) replaces the one of the user.
    pub fn locate(env: &Env, root_option: Option<&str>, located: Option<PathBuf>) -> Scoop {
        let user_config =
            env.get("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| env.home.join(".config")).join("scoop").join("config.json");
        let core = root_option.or_else(|| env.get("SCOOP")).map(absolute).or_else(|| located.clone());
        let portable = core.map(|r| r.join("config.json")).filter(|p| p.is_file() && p.parent().is_some_and(has_core));
        let config_path = portable.unwrap_or(user_config);
        let config = match json::read(&config_path) {
            Some(j @ Json::Obj(_)) => j,
            _ => Json::Obj(Vec::new()),
        };
        let setting = |key: &str| Some(config.get(key).str()).filter(|s| !s.is_empty());

        let (root, root_source) = if let Some(r) = root_option {
            (absolute(r), Source::Option)
        } else if let Some(r) = env.get("SCOOP") {
            (absolute(r), Source::Variable)
        } else if let Some(r) = setting("root_path") {
            (absolute(r), Source::Config)
        } else if let Some(r) = located {
            (r, Source::Location)
        } else {
            (env.home.join("scoop"), Source::Default)
        };
        let global = env
            .get("SCOOP_GLOBAL")
            .or_else(|| setting("global_path"))
            .map(absolute)
            .unwrap_or_else(|| PathBuf::from(env.get("ProgramData").unwrap_or(r"C:\ProgramData")).join("scoop"));
        let cache = env.get("SCOOP_CACHE").or_else(|| setting("cache_path")).map(absolute).unwrap_or_else(|| root.join("cache"));
        Scoop { root, root_source, global, cache, config_path, config }
    }

    /// `get_config`: names ignore case.
    pub fn setting(&self, name: &str) -> &Json {
        static NULL: Json = Json::Null;
        match &self.config {
            Json::Obj(members) => members.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map_or(&NULL, |(_, v)| v),
            _ => &NULL,
        }
    }

    /// A setting that is on: `true` or the text "true".
    pub fn enabled(&self, name: &str) -> bool {
        match self.setting(name) {
            Json::Bool(b) => *b,
            Json::Str(s) => s.eq_ignore_ascii_case("true"),
            _ => false,
        }
    }

    pub fn base(&self, global: bool) -> &Path {
        if global {
            &self.global
        } else {
            &self.root
        }
    }

    pub fn apps(&self, global: bool) -> PathBuf {
        self.base(global).join("apps")
    }

    pub fn app(&self, name: &str, global: bool) -> PathBuf {
        self.apps(global).join(name)
    }

    pub fn shims(&self, global: bool) -> PathBuf {
        self.base(global).join("shims")
    }

    pub fn persist(&self, name: &str, global: bool) -> PathBuf {
        self.base(global).join("persist").join(name)
    }

    pub fn buckets(&self) -> PathBuf {
        self.root.join("buckets")
    }

    /// `Get-DefaultArchitecture`.
    pub fn default_arch(&self, system: Arch) -> Arch {
        Arch::parse(self.setting("default_architecture").str()).unwrap_or(system)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Tree;

    #[test]
    fn the_root_in_scoops_order() {
        let t = Tree::new("roots");
        let located = t.dir("located");
        t.file("located/apps/scoop/current/bin/scoop.ps1", "");
        t.file("located/shims/scoop.ps1", "");
        let env = Env::new([
            ("USERPROFILE".to_string(), t.dir("home").display().to_string()),
            ("Path".to_string(), located.join("shims").display().to_string()),
        ]);
        assert_eq!(core_root(&env, None), Some(located.clone()));
        assert_eq!(core_root(&Env::default(), Some(&located.join("apps").join("x").join("1.0").join("x.exe"))), Some(located.clone()));

        let s = Scoop::locate(&env, None, core_root(&env, None));
        assert_eq!((s.root.clone(), s.root_source), (located.clone(), Source::Location));
        assert_eq!(s.cache, located.join("cache"));
        assert_eq!(Scoop::locate(&env, None, None).root, env.home.join("scoop"));

        // The user's root_path wins over the location…
        t.file("home/.config/scoop/config.json", &format!(r#"{{"ROOT_PATH":"x","root_path":{:?}}}"#, t.dir("configured")));
        let s = Scoop::locate(&env, None, core_root(&env, None));
        assert_eq!((s.root, s.root_source), (t.dir("configured"), Source::Config));
        // …unless the installation carries its own configuration.
        t.file("located/config.json", r#"{"no_junction":"True","default_architecture":"x86"}"#);
        let s = Scoop::locate(&env, None, core_root(&env, None));
        assert_eq!((s.root.clone(), s.root_source), (located.clone(), Source::Location));
        assert!(s.enabled("NO_JUNCTION"));
        assert_eq!(s.default_arch(Arch::X64), Arch::X86);
        let s = Scoop::locate(&env, Some(&t.dir("other").display().to_string()), core_root(&env, None));
        assert_eq!((s.root, s.root_source), (t.dir("other"), Source::Option));
    }
}
