//! Portable apps in the PortableApps.com format, and what the user chose in
//! that suite's menu. Each app is a folder with `App\AppInfo\appinfo.ini`,
//! which says its name, category and what starts it. Plain files, so it is
//! tested anywhere. Everything here is only read.

use crate::category::{self, Category};
use crate::items::Items;
use crate::sources::Entry;
use std::path::{Path, PathBuf};

/// The folder of the platform itself, inside the folder of the apps.
pub const PLATFORM: &str = "PortableApps.com";
/// More than any app has.
const MOST_LAUNCHERS: usize = 20;

/// The text of an ini file: UTF-16 with its mark (as the menu writes it) or
/// UTF-8.
fn text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(match bytes.as_slice() {
        [0xFF, 0xFE, rest @ ..] => {
            String::from_utf16_lossy(&rest.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b)).collect::<Vec<_>>())
        }
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// `(section, key, value)` of every line of an ini text.
fn lines(text: &str) -> impl Iterator<Item = (&str, &str, &str)> {
    let mut section = "";
    text.lines().filter_map(move |l| {
        let l = l.trim();
        if let Some(name) = l.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name;
            return None;
        }
        let (k, v) = l.split_once('=')?;
        Some((section, k.trim(), v.trim()))
    })
}

fn category_of(name: &str) -> Option<Category> {
    Some(match name.to_ascii_lowercase().as_str() {
        "internet" => Category::Internet,
        "music & video" | "music and video" => Category::Multimedia,
        "graphics & pictures" | "graphics and pictures" => Category::Graphics,
        "office" => Category::Office,
        "development" => Category::Development,
        "security" => Category::Security,
        "games" => Category::Games,
        "utilities" | "utility" | "accessibility" => Category::Utilities,
        "education" => Category::Other,
        _ => return None,
    })
}

/// `Firefox Portable` is Firefox: all of them are portable.
fn short(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().filter(|w| !w.eq_ignore_ascii_case("portable")).collect();
    if words.is_empty() {
        name.to_string()
    } else {
        words.join(" ")
    }
}

/// A path written in an ini file, below `dir`, whatever its separators.
fn below(dir: &Path, relative: &str) -> PathBuf {
    relative.split(['\\', '/']).filter(|p| !p.is_empty() && *p != "..").fold(dir.to_path_buf(), |d, p| d.join(p))
}

/// The programs of the app in `dir`; None when it is not in this format.
pub fn app(dir: &Path) -> Option<Vec<Entry>> {
    let info = text(&dir.join("App").join("AppInfo").join("appinfo.ini"))?;
    let get = |section: &str, key: &str| {
        lines(&info).find(|(s, k, _)| s.eq_ignore_ascii_case(section) && k.eq_ignore_ascii_case(key)).map(|(_, _, v)| v.to_string())
    };
    let title = get("Details", "Name")?;
    let known = get("Details", "Category").and_then(|c| category_of(&c));
    // `Start` and, when the app has several programs, `Name1`/`Start1`, …
    let count = get("Control", "Icons").and_then(|v| v.parse().ok()).unwrap_or(1).min(MOST_LAUNCHERS);
    let launchers: Vec<(String, String)> = if count > 1 {
        (1..=count).filter_map(|i| Some((get("Control", &format!("Name{i}"))?, get("Control", &format!("Start{i}"))?))).collect()
    } else {
        get("Control", "Start").map(|start| (title.clone(), start)).into_iter().collect()
    };
    let mut found = Vec::new();
    for (name, start) in launchers {
        let path = below(dir, &start);
        if path.is_file() && !found.iter().any(|e: &Entry| e.path == path) {
            let name = short(&name);
            let category = known.unwrap_or_else(|| category::guess(&[&name]));
            found.push(Entry::new(name, path, category));
        }
    }
    Some(found)
}

/// The folder of the apps of a suite, being or being just inside `dir`.
pub fn suite_in(dir: &Path) -> Option<PathBuf> {
    [dir.to_path_buf(), dir.join("PortableApps")].into_iter().find(|d| d.join(PLATFORM).is_dir())
}

/// What the suite's menu keeps about one program.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Choice {
    favourite: bool,
    hidden: bool,
    name: Option<String>,
    category: Option<Category>,
}

/// By `folder\program`, in lower case, as the menu writes them.
fn choices(ini: &str) -> std::collections::BTreeMap<String, Choice> {
    let mut out = std::collections::BTreeMap::<String, Choice>::new();
    for (section, key, value) in lines(ini) {
        let on = !matches!(value.to_ascii_lowercase().as_str(), "" | "0" | "false");
        let key = key.to_lowercase().replace('/', "\\");
        match section {
            "AppsFavorite" if on => out.entry(key).or_default().favourite = true,
            "AppsHidden" if on => out.entry(key).or_default().hidden = true,
            "AppsRenamed" if !value.is_empty() => out.entry(key).or_default().name = Some(value.to_string()),
            "AppsRecategorized" => {
                if let Some(c) = category_of(value) {
                    out.entry(key).or_default().category = Some(c);
                }
            }
            _ => {}
        }
    }
    out
}

/// Takes the favourites, names, categories and hidden programs of the suite's
/// menu for the programs of `suite` (see [`suite_in`]), leaving alone what
/// was already chosen here. Returns how many programs changed.
pub fn import(suite: &Path, items: &mut Items) -> usize {
    let Some(ini) = text(&suite.join(PLATFORM).join("Data").join("PortableAppsMenu.ini")) else { return 0 };
    let choices = choices(&ini);
    if choices.is_empty() {
        return 0;
    }
    let Ok(folders) = std::fs::read_dir(suite) else { return 0 };
    let mut changed = 0;
    for entry in folders.flatten().filter_map(|f| app(&f.path())).flatten() {
        let Ok(relative) = entry.path.strip_prefix(suite) else { continue };
        let key = relative.to_string_lossy().to_lowercase().replace('/', "\\");
        let Some(choice) = choices.get(&key) else { continue };
        let before = items.of(&entry.path);
        items.change(&entry.path, |c| {
            c.favourite |= choice.favourite;
            c.hidden |= choice.hidden;
            c.name = c.name.take().or_else(|| choice.name.clone());
            c.category = c.category.or(choice.category);
        });
        changed += usize::from(items.of(&entry.path) != before);
    }
    changed
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A suite with two apps (one with two programs) and its menu's data.
    pub(crate) fn suite(name: &str) -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("rlaunch-tests").join(name);
        let _ = std::fs::remove_dir_all(&d);
        let put = |file: &str, bytes: &[u8]| {
            let p = below(&d, file);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        };
        put(
            "FirefoxPortable/App/AppInfo/appinfo.ini",
            b"[Details]\nName=Mozilla Firefox, Portable Edition\nName = Firefox Portable\n[Control]\nStart=FirefoxPortable.exe\n",
        );
        put("FirefoxPortable/FirefoxPortable.exe", b"x");
        put("FirefoxPortable/App/Firefox/firefox.exe", b"x");
        put(
            "GitPortable/App/AppInfo/appinfo.ini",
            b"[Details]\r\nName = Git Portable\r\nCategory = Utilities\r\n[Control]\r\nIcons = 2\r\nStart = GitBash.exe\r\nName1 = Git Bash Portable\r\nStart1 = GitBash.exe\r\nName2 = Git GUI Portable\r\nStart2 = GitGui.exe\r\n",
        );
        put("GitPortable/GitBash.exe", b"x");
        put("GitPortable/GitGui.exe", b"x");
        put("Loose/Tool.exe", b"x");
        put("Loose/bin/inner.exe", b"x");
        put("CommonFiles/Java/java.exe", b"x");
        put("PortableApps.com/PortableAppsPlatform.exe", b"x");
        let menu = "[DisplayOptions]\r\nTheme=x\r\n[AppsFavorite]\r\nfirefoxportable\\firefoxportable.exe=1\r\n[AppsRenamed]\r\ngitportable\\gitgui.exe=Git window\r\n[AppsRecategorized]\r\ngitportable\\gitbash.exe=Development\r\n[AppsHidden]\r\ngitportable\\gitgui.exe=true\r\n";
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain(menu.encode_utf16().flat_map(u16::to_le_bytes)).collect();
        put("PortableApps.com/Data/PortableAppsMenu.ini", &utf16);
        d
    }

    #[test]
    fn apps_say_their_name_category_and_programs() {
        let d = suite("pa-apps");
        let firefox = app(&d.join("FirefoxPortable")).unwrap();
        assert_eq!((firefox.len(), firefox[0].name.as_str()), (1, "Mozilla Firefox, Edition"));
        assert_eq!(firefox[0].category, Category::Internet);
        let git = app(&d.join("GitPortable")).unwrap();
        let names: Vec<&str> = git.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Git Bash", "Git GUI"]);
        assert!(git.iter().all(|e| e.category == Category::Utilities));
        assert!(app(&d.join("Loose")).is_none());
        assert_eq!(short("Portable"), "Portable");
    }

    #[test]
    fn the_choices_of_the_menu_are_taken_once() {
        let d = suite("pa-import");
        assert_eq!(suite_in(&d), Some(d.clone()));
        let mut items = Items::default();
        // A name already chosen here stays.
        items.change(&d.join("GitPortable").join("GitGui.exe"), |c| c.name = Some("Mine".into()));
        assert_eq!(import(&d, &mut items), 3);
        assert!(items.of(&d.join("FirefoxPortable").join("FirefoxPortable.exe")).favourite);
        assert_eq!(items.of(&d.join("GitPortable").join("GitBash.exe")).category, Some(Category::Development));
        let gui = items.of(&d.join("GitPortable").join("GitGui.exe"));
        assert!(gui.hidden && gui.name.as_deref() == Some("Mine"));
        assert_eq!(import(&d, &mut items), 0);
    }
}
