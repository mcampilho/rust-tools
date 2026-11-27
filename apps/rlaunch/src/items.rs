//! What the user changed about the programs of the menu, and the programs
//! added one by one. The folders are read as they are; this is laid over what
//! they give, so a change shows at once and the shortcuts are never touched.

use crate::category::{Category, Group};
use crate::settings::{pack, unpack};
use crate::sources::Entry;
use crate::storage::Store;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The changes to one program.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Custom {
    pub name: Option<String>,
    pub category: Option<Group>,
    /// A picture, program or shortcut whose icon is shown instead.
    pub icon: Option<PathBuf>,
    pub favourite: bool,
    pub hidden: bool,
    /// Always opened with administrator rights (Windows).
    pub admin: bool,
    /// Opened in a console that stays, or not; None: console programs are.
    pub console: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Items {
    /// By the program's path.
    custom: BTreeMap<PathBuf, Custom>,
    /// Programs and documents added one by one.
    added: Vec<PathBuf>,
    /// Categories and subgroups of the user renamed or removed: where the
    /// programs that would be in the first one go. Folders keep their names.
    aliases: BTreeMap<Group, Group>,
    /// The order the user gave categories and subgroups.
    order: Vec<Group>,
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

    pub fn order(&self) -> &[Group] {
        &self.order
    }

    /// Keeps `siblings` (categories, or the subgroups of one) in this order.
    pub fn set_order(&mut self, siblings: &[Group]) {
        self.order.retain(|g| !siblings.contains(g));
        self.order.extend_from_slice(siblings);
    }

    /// Where a program that would be in `group` goes.
    fn aliased(&self, mut group: Group) -> Group {
        // A renamed category and then a renamed subgroup of it take two steps.
        for _ in 0..4 {
            let next = self.aliased_once(group);
            if next == group {
                break;
            }
            group = next;
        }
        group
    }

    fn aliased_once(&self, group: Group) -> Group {
        if let Some(to) = self.aliases.get(&group) {
            return *to;
        }
        match self.aliases.get(&group.top()).filter(|_| group.sub.is_some()) {
            // The subgroups of a removed category go with it.
            Some(to) if to.category == Category::Other => *to,
            // Moved inside another category, its subgroups become one.
            Some(to) => Group { category: to.category, sub: to.sub.or(group.sub) },
            None => group,
        }
    }

    /// `group` is chosen: it is no longer sent elsewhere.
    pub fn keep_group(&mut self, group: Group) {
        self.aliases.remove(&group);
        self.aliases.remove(&group.top());
    }

    /// Gives a category or subgroup of the user another name; with the name of
    /// one there already, the two become one.
    pub fn rename_group(&mut self, from: Group, to: Group) {
        if from == to || !from.is_own() {
            return;
        }
        // A subgroup is one group; a category takes its subgroups along.
        let moved =
            |g: Group| from.holds(g).then(|| if from.sub.is_some() { to } else { Group { category: to.category, sub: to.sub.or(g.sub) } });
        for c in self.custom.values_mut() {
            c.category = c.category.map(|g| moved(g).unwrap_or(g));
        }
        for target in self.aliases.values_mut() {
            *target = moved(*target).unwrap_or(*target);
        }
        self.keep_group(to);
        // The subgroups renamed or removed before are so under the new name too.
        let subs: Vec<(Group, Group)> =
            self.aliases.iter().filter(|(k, _)| from.sub.is_none() && k.sub.is_some() && from.holds(**k)).map(|(k, v)| (*k, *v)).collect();
        for (key, target) in subs {
            if let Some(key) = moved(key) {
                self.aliases.entry(key).or_insert(target);
            }
        }
        self.aliases.insert(from, to);
        // It keeps its place when only the name changes.
        for g in &mut self.order {
            *g = moved(*g).unwrap_or(*g);
        }
        let mut seen = std::collections::BTreeSet::new();
        self.order.retain(|g| seen.insert(*g));
    }

    /// Takes a category or subgroup of the user out: the programs of a
    /// subgroup stay in its category, those of a category go to Other (or back
    /// to the guess, when the user had put them there).
    pub fn remove_group(&mut self, group: Group) {
        if !group.is_own() {
            return;
        }
        let to = if group.sub.is_some() { group.top() } else { Category::Other.into() };
        let paths: Vec<PathBuf> = self.custom.keys().cloned().collect();
        for path in paths {
            self.change(&path, |c| {
                if c.category.is_some_and(|g| group.holds(g)) {
                    c.category = group.sub.is_some().then_some(to);
                }
            });
        }
        for target in self.aliases.values_mut() {
            if group.holds(*target) {
                *target = to;
            }
        }
        self.aliases.insert(group, to);
        self.order.retain(|g| !group.holds(*g));
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
            let c = self.custom.get(&e.path);
            let group = c.and_then(|c| c.category).unwrap_or_else(|| e.group());
            e.set_group(self.aliased(group));
            let Some(c) = c else { continue };
            if let Some(name) = &c.name {
                e.name.clone_from(name);
            }
            e.icon.clone_from(&c.icon);
            e.favourite = c.favourite;
            e.hidden = c.hidden;
            e.admin = c.admin;
            e.console = c.console;
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
            items.change(&p, |c| c.category = Group::parse(&category));
        }
        for (from, to) in s.get_all("group").filter_map(|v| v.split_once('\t')) {
            if let (Some(from), Some(to)) = (Group::parse(from), Group::parse(to)) {
                items.aliases.insert(from, to);
            }
        }
        items.order = s.get_all("order").filter_map(Group::parse).collect();
        for (p, on) in pairs("console") {
            items.change(&p, |c| c.console = Some(on == "1"));
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
                s.push("category", format!("{p}\t{}", category.key()));
            }
            if let Some(console) = c.console {
                s.push("console", format!("{p}\t{}", u8::from(console)));
            }
            if let Some(icon) = &c.icon {
                s.push("icon", format!("{p}\t{}", pack(icon, own)));
            }
        }
        for g in &self.order {
            s.push("order", g.key());
        }
        for (from, to) in &self.aliases {
            s.push("group", format!("{}\t{}", from.key(), to.key()));
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
            c.category = Some(Category::Games.into());
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
    fn categories_of_the_user_are_renamed_and_removed() {
        let group = |t: &str| Group::parse(t).unwrap();
        let mut in_folder = found("Alpha");
        in_folder.set_group(group("Tools/Net"));
        let scanned = [in_folder, found("Beta")];
        let mut items = Items::default();
        items.change(&scanned[1].path, |c| c.category = Some(group("Tools")));
        let places = |items: &Items| -> Vec<String> { items.apply(&scanned).iter().map(|e| e.group().key()).collect() };
        items.rename_group(group("Tools"), group("Kit"));
        assert_eq!(places(&items), ["Kit/Net", "Kit"]);
        items.rename_group(group("Kit/Net"), group("Kit/Web"));
        assert_eq!(places(&items), ["Kit/Web", "Kit"]);
        // Renamed again: what came from the folder still follows.
        items.rename_group(group("Kit"), group("Box"));
        assert_eq!(places(&items), ["Box/Web", "Box"]);
        let mut store = Store::default();
        items.save(&mut store, None);
        assert_eq!(Items::load(&Store::parse(&store.serialize("x")), None), items);
        // A subgroup removed leaves its programs in the category.
        items.remove_group(group("Box/Web"));
        assert_eq!(places(&items), ["Box", "Box"]);
        // A category removed: those put there by hand go back to the guess.
        items.remove_group(group("Box"));
        assert_eq!(places(&items), ["Other", "Other"]);
        assert!(items.custom.is_empty());
        // Chosen again, it exists again.
        items.keep_group(group("Box"));
        items.change(&scanned[1].path, |c| c.category = Some(group("Box")));
        assert_eq!(places(&items)[1], "Box");
        // The built-in ones stay.
        items.remove_group(Category::Games.into());
        assert!(!items.aliases.contains_key(&Category::Games.into()));
    }

    #[test]
    fn a_category_goes_inside_another_and_comes_out() {
        let group = |t: &str| Group::parse(t).unwrap();
        let place = |name: &str, g: &str| {
            let mut e = found(name);
            e.set_group(group(g));
            e
        };
        let scanned = [place("Alpha", "SysInternals"), place("Beta", "SysInternals/Net")];
        let mut items = Items::default();
        items.set_order(&[group("SysInternals"), group("System")]);
        let places = |items: &Items| -> Vec<String> { items.apply(&scanned).iter().map(|e| e.group().key()).collect() };
        // Two levels only: its subgroup joins it.
        items.rename_group(group("SysInternals"), group("System/SysInternals"));
        assert_eq!(places(&items), ["System/SysInternals", "System/SysInternals"]);
        // Out again: everything as the folders say.
        items.rename_group(group("System/SysInternals"), group("SysInternals"));
        assert_eq!(places(&items), ["SysInternals", "SysInternals/Net"]);
        assert_eq!(items.order(), [group("SysInternals"), group("System")]);
        let mut store = Store::default();
        items.save(&mut store, None);
        assert_eq!(Items::load(&Store::parse(&store.serialize("x")), None), items);
    }

    #[test]
    fn they_are_kept_and_read_back() {
        let mut items = Items::default();
        items.add(Path::new("/tools/x.exe"));
        items.change(Path::new("/menu/a.lnk"), |c| {
            c.name = Some("A = B".into());
            c.category = Some(Category::Internet.into());
            c.icon = Some("/pictures/a.png".into());
            c.favourite = true;
            c.hidden = true;
            c.admin = true;
            c.console = Some(false);
        });
        let mut store = Store::default();
        items.save(&mut store, None);
        // Through the text of the file, tabs and all.
        let store = Store::parse(&store.serialize("x"));
        assert_eq!(Items::load(&store, None), items);
    }
}
