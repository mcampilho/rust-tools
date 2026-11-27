//! What Linux desktops keep in files: the icon of a name in the icon themes,
//! the user's folders under their translated names, and which menus an
//! application belongs to. Plain files, so it is tested anywhere.

use std::path::{Path, PathBuf};

/// The sizes looked in, best first: icons are shown at 18 to 31 pixels.
const SIZES: [&str; 8] = ["48x48", "64x64", "scalable", "128x128", "32x32", "96x96", "256x256", "24x24"];
const CONTEXTS: [&str; 7] = ["apps", "categories", "devices", "preferences", "status", "places", "mimetypes"];
/// Looked in last, for the generic icons, as not every desktop says its theme.
const USUAL: [&str; 3] = ["Adwaita", "Yaru", "breeze"];
const KINDS: [&str; 2] = ["png", "svg"];

/// Where icons are looked for.
#[derive(Debug, Clone, Default)]
pub struct IconDirs {
    /// Folders that hold themes (`…/icons`).
    pub bases: Vec<PathBuf>,
    /// The theme of the desktop, when it says one.
    pub theme: Option<String>,
    /// Folders of loose icons.
    pub pixmaps: Vec<PathBuf>,
}

/// The value of `key` in the ini text, in `section` or (None) anywhere.
fn ini_value<'a>(text: &'a str, section: Option<&str>, key: &str) -> Option<&'a str> {
    let mut inside = section.is_none();
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = section.is_none_or(|s| s == name);
        } else if let Some((k, v)) = line.split_once('=').filter(|_| inside) {
            if k.trim() == key {
                return Some(v.trim().trim_matches('"'));
            }
        }
    }
    None
}

/// The icon theme GTK or KDE are set to, from the user's configuration.
pub fn theme_of(config: &Path) -> Option<String> {
    let read = |file: &str| std::fs::read_to_string(config.join(file)).ok();
    ["gtk-4.0/settings.ini", "gtk-3.0/settings.ini"]
        .iter()
        .find_map(|f| read(f).and_then(|t| ini_value(&t, None, "gtk-icon-theme-name").map(str::to_string)))
        .or_else(|| read("kdeglobals").and_then(|t| ini_value(&t, Some("Icons"), "Theme").map(str::to_string)))
        .filter(|t| !t.is_empty())
}

impl IconDirs {
    /// Those of this system.
    pub fn system() -> IconDirs {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".local/share")));
        let shared = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        let all: Vec<PathBuf> = data
            .into_iter()
            .chain(shared.split(':').filter(|s| !s.is_empty()).map(PathBuf::from))
            .chain([PathBuf::from("/var/lib/flatpak/exports/share"), PathBuf::from("/var/lib/snapd/desktop")])
            .collect();
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".config")));
        let mut bases: Vec<PathBuf> = home.iter().map(|h| h.join(".icons")).collect();
        bases.extend(all.iter().map(|d| d.join("icons")));
        IconDirs { bases, theme: config.and_then(|c| theme_of(&c)), pixmaps: all.iter().map(|d| d.join("pixmaps")).collect() }
    }

    /// The themes to look in: the desktop's, those it builds on, and the one
    /// every application installs into.
    fn themes(&self) -> Vec<String> {
        let mut themes: Vec<String> = self.theme.iter().cloned().collect();
        let inherited = themes.first().and_then(|t| {
            let index = self.bases.iter().find_map(|b| std::fs::read_to_string(b.join(t).join("index.theme")).ok())?;
            ini_value(&index, Some("Icon Theme"), "Inherits").map(|v| v.split(',').map(|s| s.trim().to_string()).collect::<Vec<_>>())
        });
        themes.extend(inherited.unwrap_or_default());
        themes.push("hicolor".into());
        themes.extend(USUAL.map(String::from));
        let mut seen = std::collections::HashSet::new();
        themes.retain(|t| seen.insert(t.clone()));
        themes
    }

    /// The file of the icon a `.desktop` file names: a path, or a name of the
    /// themes.
    pub fn find(&self, name: &str) -> Option<PathBuf> {
        if name.starts_with('/') {
            return Some(PathBuf::from(name)).filter(|p| p.is_file() && is_picture(p));
        }
        if name.is_empty() || name.contains('/') {
            return None;
        }
        let name = KINDS.iter().find_map(|k| name.strip_suffix(&format!(".{k}"))).unwrap_or(name);
        let files = |dir: &Path| KINDS.iter().map(|k| dir.join(format!("{name}.{k}"))).find(|p| p.is_file());
        for theme in self.themes() {
            for size in SIZES {
                // Themes are laid out as size/context (hicolor) or as
                // context/size (Breeze).
                let number = size.split('x').next().unwrap_or(size);
                for context in CONTEXTS {
                    for base in &self.bases {
                        let root = base.join(&theme);
                        if let Some(f) = files(&root.join(size).join(context)).or_else(|| files(&root.join(context).join(number))) {
                            return Some(f);
                        }
                    }
                }
            }
        }
        self.pixmaps.iter().find_map(|d| files(d))
    }
}

/// Whether rlaunch can draw the file.
pub fn is_picture(path: &Path) -> bool {
    path.extension().is_some_and(|e| KINDS.iter().any(|k| e.eq_ignore_ascii_case(k)))
}

/// The `Icon=` of a `.desktop` file.
pub fn icon_name(desktop: &str) -> Option<&str> {
    ini_value(desktop, Some("Desktop Entry"), "Icon").filter(|v| !v.is_empty())
}

/// A folder of the user (`DOCUMENTS`, `DOWNLOAD`, …) as `user-dirs.dirs`
/// writes it: `XDG_DOCUMENTS_DIR="$HOME/Documentos"`.
pub fn user_dir(file: &str, key: &str, home: &Path) -> Option<PathBuf> {
    let value = ini_value(file, None, &format!("XDG_{key}_DIR"))?;
    let path = match value.strip_prefix("$HOME") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => PathBuf::from(value),
    };
    // Set to the home folder when there is none.
    Some(path).filter(|p| p != home && p.is_absolute() || value.starts_with("$HOME/"))
}

/// Whether the application shows on `desktop` (`XDG_CURRENT_DESKTOP`, such as
/// `ubuntu:GNOME`), by its `OnlyShowIn` and `NotShowIn`.
pub fn shown_on(only: &str, not: &str, desktop: &str) -> bool {
    let listed = |list: &str| list.split(';').filter(|s| !s.is_empty()).any(|s| desktop.split(':').any(|d| d == s));
    if desktop.is_empty() {
        return true;
    }
    (only.is_empty() || listed(only)) && !listed(not)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("rlaunch-tests").join(name);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(dir: &Path, file: &str, text: &str) -> PathBuf {
        let p = file.split('/').fold(dir.to_path_buf(), |d, part| d.join(part));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn icons_come_from_the_theme_then_hicolor_then_pixmaps() {
        let d = scratch("icons");
        put(&d, "config/gtk-3.0/settings.ini", "[Settings]\ngtk-icon-theme-name=Mine\n");
        put(&d, "icons/Mine/index.theme", "[Icon Theme]\nName=Mine\nInherits=Base,hicolor\n");
        let own = put(&d, "icons/Mine/48x48/apps/editor.png", "x");
        let inherited = put(&d, "icons/Base/apps/48/player.svg", "x");
        let common = put(&d, "icons/hicolor/scalable/apps/editor.svg", "x");
        let loose = put(&d, "pixmaps/old.png", "x");
        put(&d, "pixmaps/ancient.xpm", "x");
        let mut dirs = IconDirs { bases: vec![d.join("icons")], theme: theme_of(&d.join("config")), pixmaps: vec![d.join("pixmaps")] };
        assert_eq!(dirs.theme.as_deref(), Some("Mine"));
        assert_eq!(dirs.find("editor"), Some(own));
        assert_eq!(dirs.find("player"), Some(inherited));
        assert_eq!(dirs.find("old.png"), Some(loose));
        assert_eq!((dirs.find("ancient"), dirs.find("none"), dirs.find("../x")), (None, None, None));
        dirs.theme = None;
        assert_eq!(dirs.find("editor"), Some(common));
        assert_eq!(icon_name("[Desktop Entry]\nName=X\nIcon=x-app\n[Other]\nIcon=no\n"), Some("x-app"));
    }

    #[test]
    fn user_folders_and_desktops() {
        let home = Path::new("/home/a");
        let file = "# x\nXDG_DOCUMENTS_DIR=\"$HOME/Documentos\"\nXDG_MUSIC_DIR=\"$HOME/\"\n";
        assert_eq!(user_dir(file, "DOCUMENTS", home), Some(home.join("Documentos")));
        assert_eq!(user_dir(file, "VIDEOS", home), None);
        assert!(shown_on("", "", "GNOME"));
        assert!(shown_on("GNOME;Unity;", "", "ubuntu:GNOME"));
        assert!(!shown_on("KDE;", "", "GNOME"));
        assert!(!shown_on("", "GNOME;", "ubuntu:GNOME"));
        assert!(shown_on("KDE;", "", ""));
    }
}
