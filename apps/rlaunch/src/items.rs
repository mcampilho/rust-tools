//! What the user changed about the programs of the menu, and the programs
//! added one by one. The folders are read as they are; this is laid over what
//! they give, so a change shows at once and the shortcuts are never touched.

use crate::category::Category;
use crate::settings::{pack, unpack};
use crate::sources::Entry;
use crate::storage::Store;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The changes to one program.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Custom {
    pub name: Option<String>,
    pub category: Option<Category>,
    /// A picture, program or shortcut whose icon is shown instead.
    pub icon: Option<PathBuf>,
    pub favourite: bool,
    pub hidden: bool,
    /// Always opened with administrator rights (Windows).
    pub admin: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Items {
    /// By the program's path.
    custom: BTreeMap<PathBuf, Custom>,
    /// Programs and documents added one by one.
    added: Vec<PathBuf>,
}

impl Items {
    pub fn of(&self, path: &Path) -> Custom {
        self.custom.get(path).cloned().unwrap_or_default()
    }

    /// Changes what is kept about `path`; nothing is kept when nothing differs
    /// from what the folders give.
    pub fn change(&mut self, path: &Path, edit: impl FnOnce(&mut Custom)) {
        let mut c = self.of(path);
        edit(&mut c);
        if c == Custom::default() {
            self.custom.remove(path);
        } else {
            self.custom.insert(path.to_path_buf(), c);
        }
    }

    /// Adds a program of its own, shown again if it was hidden.
    pub fn add(&mut self, path: &Path) {
        if !self.added.iter().any(|p| p == path) {
            self.added.push(path.to_path_buf());
        }
        self.change(path, |c| c.hidden = false);
    }

    /// Takes an added program out, with what was changed about it.
    pub fn remove(&mut self, path: &Path) {
        self.added.retain(|p| p != path);
        self.custom.remove(path);
    }

    /// The programs of the menu: those the folders gave (`found`) and those
    /// added, with the user's changes, by name.
    pub fn apply(&self, found: &[Entry]) -> Vec<Entry> {
        let mut all: Vec<Entry> = found.iter().filter(|e| !self.added.contains(&e.path)).cloned().collect();
        for path in &self.added {
            // One already there with the same name gives way to the added one.
            if let Some(e) = crate::sources::added_entry(path) {
                all.retain(|x| x.added || !x.name.eq_ignore_ascii_case(&e.name));
                all.push(e);
            }
        }
        for e in &mut all {
            let Some(c) = self.custom.get(&e.path) else { continue };
            if let Some(name) = &c.name {
                e.name.clone_from(name);
            }
            e.category = c.category.unwrap_or(e.category);
            e.icon.clone_from(&c.icon);
            e.favourite = c.favourite;
            e.hidden = c.hidden;
            e.admin = c.admin;
        }
        all.sort_by_cached_key(|e| e.name.to_lowercase());
        all
    }

    /// `own` is the folder of rlaunch in portable mode (see [`pack`]).
    pub fn load(s: &Store, own: Option<&Path>) -> Items {
        let mut items = Items { added: s.get_all("add").map(|v| unpack(v, own)).collect(), ..Items::default() };
        let pairs = |key: &'static str| s.get_all(key).filter_map(|v| v.split_once('\t')).map(|(p, v)| (unpack(p, own), v.to_string()));
        for p in s.get_all("favourite") {
            items.change(&unpack(p, own), |c| c.favourite = true);
        }
        for p in s.get_all("hidden") {
            items.change(&unpack(p, own), |c| c.hidden = true);
        }
        for p in s.get_all("admin") {
            items.change(&unpack(p, own), |c| c.admin = true);
        }
        for (p, name) in pairs("name") {
            items.change(&p, |c| c.name = Some(name));
        }
        for (p, category) in pairs("category") {
            items.change(&p, |c| c.category = Category::ALL.into_iter().find(|x| x.name() == category));
        }
        for (p, icon) in pairs("icon") {
            items.change(&p, |c| c.icon = Some(unpack(&icon, own)));
        }
        items
    }

    pub fn save(&self, s: &mut Store, own: Option<&Path>) {
        for p in &self.added {
            s.push("add", pack(p, own));
        }
        for (path, c) in &self.custom {
            let p = pack(path, own);
            if c.favourite {
                s.push("favourite", p.clone());
            }
            if c.hidden {
                s.push("hidden", p.clone());
            }
            if c.admin {
                s.push("admin", p.clone());
            }
            if let Some(name) = &c.name {
                s.push("name", format!("{p}\t{name}"));
            }
            if let Some(category) = c.category {
                s.push("category", format!("{p}\t{}", category.name()));
            }
            if let Some(icon) = &c.icon {
                s.push("icon", format!("{p}\t{}", pack(icon, own)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(name: &str) -> Entry {
        Entry::new(name.into(), PathBuf::from(format!("/menu/{name}.lnk")), Category::Other)
    }

    #[test]
    fn changes_are_laid_over_what_the_folders_give() {
        let scanned = [found("Alpha"), found("Beta"), found("Gamma")];
        let mut items = Items::default();
        items.change(&scanned[0].path, |c| {
            c.name = Some("Zulu".into());
            c.category = Some(Category::Games);
            c.favourite = true;
        });
        items.change(&scanned[1].path, |c| c.hidden = true);
        let all = items.apply(&scanned);
        let names: Vec<&str> = all.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Beta", "Gamma", "Zulu"]);
        assert!(all[0].hidden && all[2].favourite && all[2].category == Category::Games);
        // Back to what the folder gives: nothing is kept.
        items.change(&scanned[1].path, |c| c.hidden = false);
        assert!(!items.custom.contains_key(&scanned[1].path));
    }

    #[test]
    fn added_programs_come_and_go() {
        let scanned = [found("Alpha")];
        let mut items = Items::default();
        items.add(Path::new("/tools/Alpha.exe"));
        items.add(Path::new("/tools/Alpha.exe"));
        let all = items.apply(&scanned);
        assert_eq!(all.len(), 1);
        assert!(all[0].added);
        assert_eq!(all[0].path, Path::new("/tools/Alpha.exe"));
        items.remove(Path::new("/tools/Alpha.exe"));
        assert_eq!(items.apply(&scanned), scanned);
    }

    #[test]
    fn they_are_kept_and_read_back() {
        let mut items = Items::default();
        items.add(Path::new("/tools/x.exe"));
        items.change(Path::new("/menu/a.lnk"), |c| {
            c.name = Some("A = B".into());
            c.category = Some(Category::Internet);
            c.icon = Some("/pictures/a.png".into());
            c.favourite = true;
            c.hidden = true;
            c.admin = true;
        });
        let mut store = Store::default();
        items.save(&mut store, None);
        // Through the text of the file, tabs and all.
        let store = Store::parse(&store.serialize("x"));
        assert_eq!(Items::load(&store, None), items);
    }
}
