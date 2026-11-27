//! `lib\manifest.ps1`: a manifest and its architecture rules.

use crate::json::{self, Json};
use std::path::Path;

/// A processor architecture, as Scoop names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X64,
    X86,
    Arm64,
}

impl Arch {
    pub fn name(self) -> &'static str {
        match self {
            Arch::X64 => "64bit",
            Arch::X86 => "32bit",
            Arch::Arm64 => "arm64",
        }
    }

    /// `Format-ArchitectureString`.
    pub fn parse(s: &str) -> Option<Arch> {
        match s.to_ascii_lowercase().as_str() {
            "64bit" | "64" | "x64" | "amd64" | "x86_64" | "x86-64" => Some(Arch::X64),
            "32bit" | "32" | "x86" | "i386" | "386" | "i686" => Some(Arch::X86),
            "arm64" | "arm" | "aarch64" => Some(Arch::Arm64),
            _ => None,
        }
    }
}

/// What PowerShell takes as true: `arch_specific` and most `if ($manifest.x)`.
pub fn truthy(v: &Json) -> bool {
    match v {
        Json::Null => false,
        Json::Bool(b) => *b,
        Json::Num(n) => *n != 0.0,
        Json::Str(s) => !s.is_empty(),
        Json::Arr(items) => items.len() > 1 || items.first().is_some_and(truthy),
        Json::Obj(_) => true,
    }
}

/// A string or a list of strings, as a list.
pub fn strings(v: &Json) -> Vec<String> {
    match v {
        Json::Str(s) if !s.is_empty() => vec![s.clone()],
        Json::Arr(items) => items.iter().filter(|i| !i.str().is_empty()).map(|i| i.str().to_string()).collect(),
        _ => Vec::new(),
    }
}

/// One entry of `bin`: the target, the shim's name and its arguments.
#[derive(Debug, PartialEq, Eq)]
pub struct Bin {
    pub target: String,
    pub alias: Option<String>,
    pub args: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Manifest(pub Json);

impl Manifest {
    pub fn read(path: &Path) -> Option<Manifest> {
        match json::read(path) {
            Some(j @ Json::Obj(_)) => Some(Manifest(j)),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> &Json {
        self.0.get(key)
    }

    pub fn version(&self) -> &str {
        self.get("version").str()
    }

    /// `arch_specific`: the property of the architecture, else the general one.
    pub fn arch(&self, key: &str, arch: Arch) -> &Json {
        let specific = self.get("architecture").get(arch.name()).get(key);
        if truthy(specific) {
            specific
        } else {
            self.get(key)
        }
    }

    pub fn urls(&self, arch: Arch) -> Vec<String> {
        strings(self.arch("url", arch))
    }

    /// `Get-SupportedArchitecture`; `windows_11` decides what ARM64 falls back to.
    pub fn supported_arch(&self, wanted: Arch, windows_11: bool) -> Option<Arch> {
        let mut arch = wanted;
        if arch == Arch::Arm64 && !mentions_arm64(&self.0) {
            arch = if windows_11 { Arch::X64 } else { Arch::X86 };
        }
        (!self.urls(arch).is_empty()).then_some(arch)
    }

    pub fn bins(&self, arch: Arch) -> Vec<Bin> {
        let one = |v: &Json| -> Option<Bin> {
            let part = |i: usize| match v.at(i) {
                Json::Str(s) => Some(s.clone()),
                _ => None,
            };
            match v {
                Json::Str(s) if !s.is_empty() => Some(Bin { target: s.clone(), alias: None, args: None }),
                Json::Arr(_) => Some(Bin { target: part(0)?, alias: part(1), args: part(2) }),
                _ => None,
            }
        };
        match self.arch("bin", arch) {
            Json::Arr(items) => items.iter().filter_map(one).collect(),
            other => one(other).into_iter().collect(),
        }
    }

    /// The names of the Start menu shortcuts.
    pub fn shortcut_names(&self, arch: Arch) -> Vec<String> {
        self.arch("shortcuts", arch).items().iter().map(|s| s.at(1).str().to_string()).filter(|s| !s.is_empty()).collect()
    }

    pub fn depends(&self) -> Vec<String> {
        strings(self.get("depends"))
    }
}

fn mentions_arm64(v: &Json) -> bool {
    match v {
        Json::Str(s) => s == "arm64",
        Json::Arr(items) => items.iter().any(mentions_arm64),
        Json::Obj(members) => members.iter().any(|(k, v)| k == "arm64" || mentions_arm64(v)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(text: &str) -> Manifest {
        Manifest(json::parse(text).unwrap())
    }

    #[test]
    fn the_architecture_comes_first() {
        let x = m(r#"{"url":"g","bin":"a.exe","architecture":{"64bit":{"url":"s64"},"32bit":{"url":"s32","bin":["b.exe"]}}}"#);
        assert_eq!(x.urls(Arch::X64), ["s64"]);
        assert_eq!(x.urls(Arch::Arm64), ["g"]);
        assert_eq!(x.bins(Arch::X64)[0].target, "a.exe");
        assert_eq!(x.bins(Arch::X86)[0].target, "b.exe");
        assert_eq!(x.supported_arch(Arch::Arm64, true), Some(Arch::X64));
        assert_eq!(x.supported_arch(Arch::Arm64, false), Some(Arch::X86));
        assert_eq!(m(r#"{"architecture":{"64bit":{"url":"u"}}}"#).supported_arch(Arch::X86, false), None);
    }

    #[test]
    fn bins_with_names_and_arguments() {
        let x = m(r#"{"bin":["a.exe",["b\\c.exe","c","-x $dir"]],"shortcuts":[["a.exe","App\\A"]]}"#);
        assert_eq!(
            x.bins(Arch::X64),
            [
                Bin { target: "a.exe".into(), alias: None, args: None },
                Bin { target: "b\\c.exe".into(), alias: Some("c".into()), args: Some("-x $dir".into()) }
            ]
        );
        assert_eq!(x.shortcut_names(Arch::X64), ["App\\A"]);
        assert_eq!(Arch::parse("AMD64"), Some(Arch::X64));
        assert_eq!(Arch::parse("mips"), None);
    }
}
