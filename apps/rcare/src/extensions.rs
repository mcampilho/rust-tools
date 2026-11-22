//! Extensions: what is added to the browsers, read from each browser's own
//! profile files — Chrome, Edge, Brave, Vivaldi, Opera and Chromium
//! (`Secure Preferences` and `Preferences` of each profile), and Firefox
//! (`extensions.json` of each profile). Nothing is changed here: an
//! extension is turned off or removed in the browser's own page, which rcare
//! opens (the user's choice, 2026-11-22). The browsers' built-in parts are
//! left out.

use rt_core::json::{self, Json};
use std::path::{Path, PathBuf};

/// A browser rcare knows how to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Browser {
    Chrome,
    Edge,
    Brave,
    Vivaldi,
    Opera,
    Chromium,
    Firefox,
}

impl Browser {
    pub const ALL: [Browser; 7] =
        [Browser::Chrome, Browser::Edge, Browser::Brave, Browser::Vivaldi, Browser::Opera, Browser::Chromium, Browser::Firefox];

    /// Its name (the same in every language).
    pub fn name(self) -> &'static str {
        match self {
            Browser::Chrome => "Google Chrome",
            Browser::Edge => "Microsoft Edge",
            Browser::Brave => "Brave",
            Browser::Vivaldi => "Vivaldi",
            Browser::Opera => "Opera",
            Browser::Chromium => "Chromium",
            Browser::Firefox => "Firefox",
        }
    }

    /// Its program's file name (as in `App Paths`).
    pub fn exe(self) -> &'static str {
        match self {
            Browser::Chrome | Browser::Chromium => "chrome.exe",
            Browser::Edge => "msedge.exe",
            Browser::Brave => "brave.exe",
            Browser::Vivaldi => "vivaldi.exe",
            Browser::Opera => "opera.exe",
            Browser::Firefox => "firefox.exe",
        }
    }

    /// The page of its extensions (of one, when `id` is given and the browser can).
    pub fn page(self, id: Option<&str>) -> String {
        let scheme = match self {
            Browser::Firefox => return "about:addons".into(),
            Browser::Edge => "edge",
            Browser::Brave => "brave",
            Browser::Vivaldi => "vivaldi",
            Browser::Opera => "opera",
            Browser::Chrome | Browser::Chromium => "chrome",
        };
        match id {
            Some(id) => format!("{scheme}://extensions/?id={id}"),
            None => format!("{scheme}://extensions/"),
        }
    }

    /// Where its profiles are: the user data folder (Chromium) or `Profiles` (Firefox).
    pub fn data_dir(self, local: &Path, roaming: &Path) -> PathBuf {
        match self {
            Browser::Chrome => local.join("Google\\Chrome\\User Data"),
            Browser::Edge => local.join("Microsoft\\Edge\\User Data"),
            Browser::Brave => local.join("BraveSoftware\\Brave-Browser\\User Data"),
            Browser::Vivaldi => local.join("Vivaldi\\User Data"),
            Browser::Chromium => local.join("Chromium\\User Data"),
            Browser::Opera => roaming.join("Opera Software"),
            Browser::Firefox => roaming.join("Mozilla\\Firefox\\Profiles"),
        }
    }
}

/// Who put the extension there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// The browser's store (or the user, from a file).
    Store,
    /// Another program on the computer.
    Program,
    /// The organisation's policy: the browser does not let it be removed.
    Policy,
    /// Loaded from a folder (a developer's).
    Unpacked,
}

impl Source {
    /// What the list says (a translation key).
    pub fn label(self) -> &'static str {
        match self {
            Source::Store => "Store",
            Source::Program => "Another program",
            Source::Policy => "Policy",
            Source::Unpacked => "Unpacked",
        }
    }

    /// Chromium's install location number (`location`); None for the browser's own parts.
    pub fn of_chromium(location: i64) -> Option<Source> {
        match location {
            5 | 10 => None,
            2 | 3 | 6 => Some(Source::Program),
            7 | 9 => Some(Source::Policy),
            4 | 8 => Some(Source::Unpacked),
            _ => Some(Source::Store),
        }
    }
}

/// One extension of one profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Extension {
    pub id: String,
    pub name: String,
    pub version: String,
    pub browser: Browser,
    /// The profile's folder name (`Default`, `Profile 1`, `abcd.default-release`).
    pub profile_dir: String,
    /// The profile's name as the browser shows it.
    pub profile: String,
    pub enabled: bool,
    pub source: Source,
    /// Its files, when known.
    pub folder: Option<PathBuf>,
}

// ------------------------------------------------------------ Chromium

/// A `__MSG_name__` of a manifest, from the extension's `_locales` (its default language, then English).
fn localized(text: &str, folder: Option<&Path>, default_locale: &str) -> Option<String> {
    let key = text.strip_prefix("__MSG_")?.strip_suffix("__")?;
    let dir = folder?.join("_locales");
    for lang in [default_locale, "en", "en_US"] {
        if lang.is_empty() {
            continue;
        }
        let Ok(t) = std::fs::read_to_string(dir.join(lang).join("messages.json")) else { continue };
        let Ok(j) = json::parse(t.trim_start_matches('\u{feff}')) else { continue };
        // Message names are case-insensitive.
        if let Json::Obj(members) = &j {
            if let Some((_, m)) = members.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
                let s = m.get("message").str();
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

fn number(j: &Json) -> Option<i64> {
    j.num().map(|n| n as i64)
}

/// The extensions in a Chromium profile's preferences (`extensions.settings`).
/// `profile` is the profile's folder (where `Extensions` is).
pub fn chromium_from(prefs: &Json, browser: Browser, profile: &Path, profile_name: &str) -> Vec<Extension> {
    let Json::Obj(settings) = prefs.get("extensions").get("settings") else { return Vec::new() };
    let mut out = Vec::new();
    for (id, s) in settings {
        let Some(source) = Source::of_chromium(number(s.get("location")).unwrap_or(1)) else { continue };
        let m = s.get("manifest");
        // Themes and hosted apps are not extensions.
        if !matches!(m.get("theme"), Json::Null) || !matches!(m.get("app"), Json::Null) {
            continue;
        }
        let folder = match s.get("path").str() {
            "" => None,
            p if Path::new(p).is_absolute() => Some(PathBuf::from(p)),
            p => Some(profile.join("Extensions").join(p)),
        };
        let mut name = m.get("name").str().to_string();
        if name.starts_with("__MSG_") {
            name = localized(&name, folder.as_deref(), m.get("default_locale").str()).unwrap_or(name);
        }
        if name.is_empty() {
            name = id.clone();
        }
        // Off: a reason to be off (newer), or state 0 (older).
        let reasons = match s.get("disable_reasons") {
            Json::Arr(a) => !a.is_empty(),
            Json::Num(n) => *n != 0.0,
            _ => false,
        };
        let enabled = !reasons && number(s.get("state")) != Some(0);
        out.push(Extension {
            id: id.clone(),
            name,
            version: m.get("version").str().to_string(),
            browser,
            profile_dir: profile.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            profile: profile_name.to_string(),
            enabled,
            source,
            folder,
        });
    }
    out
}

/// The names the browser gives its profiles (`Local State`: `profile.info_cache`).
pub fn chromium_profile_names(local_state: &Json) -> Vec<(String, String)> {
    match local_state.get("profile").get("info_cache") {
        Json::Obj(m) => m.iter().map(|(dir, v)| (dir.clone(), v.get("name").str().to_string())).collect(),
        _ => Vec::new(),
    }
}

fn read_json(path: &Path) -> Option<Json> {
    let t = std::fs::read_to_string(path).ok()?;
    json::parse(t.trim_start_matches('\u{feff}')).ok()
}

/// Every profile of a Chromium browser: both preference files are read
/// (newer versions keep extensions in `Secure Preferences`).
fn chromium(browser: Browser, data: &Path) -> Vec<Extension> {
    let names = read_json(&data.join("Local State")).map(|j| chromium_profile_names(&j)).unwrap_or_default();
    // Opera keeps its one profile in the data folder's `Opera Stable`, as the profile itself.
    let profiles: Vec<PathBuf> = if browser == Browser::Opera {
        ["Opera Stable", "Opera GX Stable"].iter().map(|p| data.join(p)).filter(|p| p.join("Preferences").is_file()).collect()
    } else {
        std::fs::read_dir(data)
            .map(|it| {
                it.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        let n = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        p.join("Preferences").is_file() && n != "System Profile" && n != "Guest Profile"
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut out: Vec<Extension> = Vec::new();
    for p in profiles {
        let dir = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let shown = names.iter().find(|(d, _)| *d == dir).map(|(_, n)| n.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| dir.clone());
        for file in ["Secure Preferences", "Preferences"] {
            let Some(j) = read_json(&p.join(file)) else { continue };
            for e in chromium_from(&j, browser, &p, &shown) {
                if !out.iter().any(|o| o.browser == e.browser && o.profile_dir == e.profile_dir && o.id == e.id) {
                    out.push(e);
                }
            }
        }
    }
    out
}

// ------------------------------------------------------------ Firefox

/// The extensions of a Firefox profile's `extensions.json` (the built-in ones left out).
pub fn firefox_from(j: &Json, profile_dir: &str) -> Vec<Extension> {
    // `abcd1234.default-release` → `default-release`.
    let shown = profile_dir.split_once('.').map_or(profile_dir, |(_, n)| n).to_string();
    j.get("addons")
        .items()
        .iter()
        .filter(|a| a.get("type").str() == "extension")
        .filter(|a| !matches!(a.get("location").str(), "app-builtin" | "app-system-defaults" | "app-system-addons" | "app-builtin-addons"))
        .map(|a| {
            let name = a.get("defaultLocale").get("name").str();
            let location = a.get("location").str();
            let source = match location {
                "app-profile" | "app-temporary" if a.get("installTelemetryInfo").get("source").str() == "temporary-addon" => {
                    Source::Unpacked
                }
                "app-profile" => Source::Store,
                "app-temporary" => Source::Unpacked,
                l if l.contains("policy") || a.get("installTelemetryInfo").get("source").str() == "enterprise-policy" => Source::Policy,
                _ => Source::Program,
            };
            let path = a.get("path").str();
            Extension {
                id: a.get("id").str().to_string(),
                name: if name.is_empty() { a.get("id").str().to_string() } else { name.to_string() },
                version: a.get("version").str().to_string(),
                browser: Browser::Firefox,
                profile_dir: profile_dir.to_string(),
                profile: shown.clone(),
                enabled: a.get("active") == &Json::Bool(true),
                source,
                folder: (!path.is_empty()).then(|| PathBuf::from(path)),
            }
        })
        .collect()
}

fn firefox(data: &Path) -> Vec<Extension> {
    let Ok(it) = std::fs::read_dir(data) else { return Vec::new() };
    let mut out = Vec::new();
    for e in it.flatten() {
        let dir = e.file_name().to_string_lossy().into_owned();
        if let Some(j) = read_json(&e.path().join("extensions.json")) {
            out.extend(firefox_from(&j, &dir));
        }
    }
    out
}

// ------------------------------------------------------------ all of them

/// Every browser's extensions, by browser, profile and name.
pub fn read() -> Vec<Extension> {
    let (Some(local), Some(roaming)) = (std::env::var_os("LOCALAPPDATA"), std::env::var_os("APPDATA")) else { return Vec::new() };
    let (local, roaming) = (PathBuf::from(local), PathBuf::from(roaming));
    let mut out = Vec::new();
    for b in Browser::ALL {
        let dir = b.data_dir(&local, &roaming);
        if !dir.is_dir() {
            continue;
        }
        out.extend(if b == Browser::Firefox { firefox(&dir) } else { chromium(b, &dir) });
    }
    sort(&mut out);
    out
}

pub fn sort(list: &mut [Extension]) {
    list.sort_by(|a, b| {
        (Browser::ALL.iter().position(|x| *x == a.browser), a.profile.to_lowercase(), a.name.to_lowercase()).cmp(&(
            Browser::ALL.iter().position(|x| *x == b.browser),
            b.profile.to_lowercase(),
            b.name.to_lowercase(),
        ))
    });
}

/// The browser's program: `App Paths` of the user, then of the machine, then where it usually is.
#[cfg(windows)]
pub fn program_of(b: Browser) -> Option<PathBuf> {
    use rt_sys::win::reg::{Key, Root, Value, View};
    let key = format!("Software\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{}", b.exe());
    for root in [Root::CurrentUser, Root::LocalMachine] {
        for view in [View::Bits64, View::Bits32] {
            if let Some(k) = Key::open(root, &key, view) {
                let p = match k.value("") {
                    Some(Value::Str(s)) => s,
                    Some(Value::Expand(s)) => rt_sys::win::reg::expand(&s),
                    _ => continue,
                };
                let p = PathBuf::from(p.trim_matches('"'));
                // Chromium shares `chrome.exe` with Chrome: only a Chromium folder is Chromium's.
                let chromium_dir = p.to_string_lossy().to_lowercase().contains("chromium");
                if p.is_file() && (b != Browser::Chromium || chromium_dir) && (b != Browser::Chrome || !chromium_dir) {
                    return Some(p);
                }
            }
        }
    }
    let env = |v: &str| std::env::var_os(v).map(PathBuf::from);
    let tries: Vec<PathBuf> = match b {
        Browser::Chrome => [env("LOCALAPPDATA"), env("ProgramFiles"), env("ProgramFiles(x86)")]
            .into_iter()
            .flatten()
            .map(|d| d.join("Google\\Chrome\\Application\\chrome.exe"))
            .collect(),
        Browser::Edge => [env("ProgramFiles(x86)"), env("ProgramFiles")]
            .into_iter()
            .flatten()
            .map(|d| d.join("Microsoft\\Edge\\Application\\msedge.exe"))
            .collect(),
        Browser::Brave => [env("LOCALAPPDATA"), env("ProgramFiles")]
            .into_iter()
            .flatten()
            .map(|d| d.join("BraveSoftware\\Brave-Browser\\Application\\brave.exe"))
            .collect(),
        Browser::Vivaldi => env("LOCALAPPDATA").map(|d| d.join("Vivaldi\\Application\\vivaldi.exe")).into_iter().collect(),
        Browser::Opera => env("LOCALAPPDATA").map(|d| d.join("Programs\\Opera\\opera.exe")).into_iter().collect(),
        Browser::Chromium => env("LOCALAPPDATA").map(|d| d.join("Chromium\\Application\\chrome.exe")).into_iter().collect(),
        Browser::Firefox => {
            [env("ProgramFiles"), env("ProgramFiles(x86)")].into_iter().flatten().map(|d| d.join("Mozilla Firefox\\firefox.exe")).collect()
        }
    };
    tries.into_iter().find(|p| p.is_file())
}

#[cfg(not(windows))]
pub fn program_of(_b: Browser) -> Option<PathBuf> {
    None
}

/// The arguments that open the extensions page in the right profile.
pub fn open_args(e: &Extension) -> Vec<String> {
    match e.browser {
        Browser::Firefox => vec!["-new-tab".into(), e.browser.page(None)],
        Browser::Opera => vec![e.browser.page(None)],
        b => vec![format!("--profile-directory={}", e.profile_dir), b.page(Some(&e.id))],
    }
}

/// Opens the extensions page of the browser of `e`. Err: the browser's program was not found or did not start.
pub fn open_page(e: &Extension) -> Result<(), String> {
    let exe = program_of(e.browser).ok_or_else(|| format!("{} ({})", e.browser.name(), e.browser.exe()))?;
    std::process::Command::new(&exe).args(open_args(e)).spawn().map(|_| ()).map_err(|why| why.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_preferences() {
        let dir = std::env::temp_dir().join(format!("rcare-ext-{}", std::process::id()));
        let profile = dir.join("Profile 1");
        let loc = profile.join("Extensions\\bbb\\1.0_0\\_locales\\en");
        std::fs::create_dir_all(&loc).unwrap();
        std::fs::write(loc.join("messages.json"), r#"{"AppName":{"message":"Bee Blocker"}}"#).unwrap();
        let prefs = json::parse(
            r#"{"extensions":{"settings":{
                "aaa":{"location":1,"manifest":{"name":"Alpha","version":"2.1"},"path":"aaa\\2.1_0"},
                "bbb":{"location":1,"disable_reasons":[1],"manifest":{"name":"__MSG_appName__","default_locale":"en","version":"1.0"},"path":"bbb\\1.0_0"},
                "ccc":{"location":5,"manifest":{"name":"Built-in PDF"}},
                "ddd":{"location":7,"state":0,"manifest":{"name":"Company Tool"}},
                "eee":{"location":1,"manifest":{"name":"A theme","theme":{}}},
                "fff":{"location":2,"manifest":{"name":"Helper"},"path":"C:\\Programs\\Helper\\ext"}
            }}}"#,
        )
        .unwrap();
        let mut list = chromium_from(&prefs, Browser::Chrome, &profile, "Work");
        sort(&mut list);
        let got: Vec<_> = list.iter().map(|e| (e.name.as_str(), e.enabled, e.source)).collect();
        assert_eq!(
            got,
            vec![
                ("Alpha", true, Source::Store),
                ("Bee Blocker", false, Source::Store),
                ("Company Tool", false, Source::Policy),
                ("Helper", true, Source::Program)
            ]
        );
        let a = &list[0];
        assert_eq!((a.profile.as_str(), a.profile_dir.as_str(), a.version.as_str()), ("Work", "Profile 1", "2.1"));
        assert_eq!(a.folder.as_deref(), Some(profile.join("Extensions\\aaa\\2.1_0").as_path()));
        assert_eq!(list[3].folder.as_deref(), Some(Path::new("C:\\Programs\\Helper\\ext")));
        assert_eq!(open_args(a), vec!["--profile-directory=Profile 1".to_string(), "chrome://extensions/?id=aaa".to_string()]);
        std::fs::remove_dir_all(&dir).unwrap();
        let ls = json::parse(r#"{"profile":{"info_cache":{"Default":{"name":"Miguel"},"Profile 1":{"name":"Work"}}}}"#).unwrap();
        assert_eq!(chromium_profile_names(&ls)[1], ("Profile 1".to_string(), "Work".to_string()));
        assert!(chromium_from(&Json::Null, Browser::Edge, &profile, "x").is_empty());
    }

    #[test]
    fn firefox_extensions() {
        let j = json::parse(
            r#"{"addons":[
                {"id":"ublock@x","type":"extension","location":"app-profile","active":true,"version":"1.6","defaultLocale":{"name":"uBlock Origin"},"path":"C:\\p\\ublock@x.xpi"},
                {"id":"off@x","type":"extension","location":"app-profile","active":false,"defaultLocale":{"name":"Sleeper"}},
                {"id":"screenshots@mozilla.org","type":"extension","location":"app-builtin","active":true,"defaultLocale":{"name":"Screenshots"}},
                {"id":"fr@dictionaries","type":"dictionary","location":"app-profile","active":true,"defaultLocale":{"name":"French"}},
                {"id":"corp@x","type":"extension","location":"app-system-share","active":true,"installTelemetryInfo":{"source":"enterprise-policy"},"defaultLocale":{"name":"Corp"}}
            ]}"#,
        )
        .unwrap();
        let list = firefox_from(&j, "ab12cd.default-release");
        let got: Vec<_> = list.iter().map(|e| (e.name.as_str(), e.enabled, e.source)).collect();
        assert_eq!(got, vec![("uBlock Origin", true, Source::Store), ("Sleeper", false, Source::Store), ("Corp", true, Source::Policy)]);
        assert_eq!(list[0].profile, "default-release");
        assert_eq!(open_args(&list[0]), vec!["-new-tab".to_string(), "about:addons".to_string()]);
    }

    #[test]
    fn names_and_pages() {
        for b in Browser::ALL {
            assert!(!b.name().is_empty() && b.exe().ends_with(".exe"));
            assert!(b.page(None).contains("extensions") || b == Browser::Firefox);
        }
        assert_eq!(Browser::Edge.page(Some("x")), "edge://extensions/?id=x");
        assert_eq!(Source::of_chromium(10), None);
        assert_eq!(Source::of_chromium(9), Some(Source::Policy));
        for s in [Source::Store, Source::Program, Source::Policy, Source::Unpacked] {
            assert!(!s.label().is_empty());
        }
    }

    /// The extensions of the browsers on this PC, and where each browser is.
    #[test]
    #[ignore]
    fn here() {
        for e in read() {
            println!("{:?} [{}] {:?} on={} {:?} v{} {:?}", e.browser, e.profile, e.name, e.enabled, e.source, e.version, e.folder);
        }
        for b in Browser::ALL {
            println!("{:?}: {:?}", b, program_of(b));
        }
    }
}
