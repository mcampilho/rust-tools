use super::*;
use std::collections::BTreeSet;

#[test]
fn table_rows_are_well_formed() {
    let problems = core::check_table(TABLE.text(), '\t');
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn lookups_and_fallbacks() {
    install();
    assert_eq!(core::tr_in(Lang::PtPT, "Disk Cleaner"), "Limpeza de disco");
    assert_eq!(core::tr_in(Lang::EnGB, "Disk Cleaner"), "Disk Cleaner");
    // A string of the cleaner's own table.
    assert_eq!(core::tr_in(Lang::PtPT, "Recycle Bin"), "Reciclagem");
}

/// Every label of the menus has a row, in rcare's table or in a page's.
#[test]
fn menus_are_translated() {
    install();
    let mut keys = BTreeSet::new();
    for page in 0..crate::pages::PAGES.len() {
        for (title, items) in crate::commands::menu_tree(page) {
            keys.insert(title.to_string());
            rt_core::menu::M::each_command(&items, &mut |c| {
                if let Ok(k) = c.label_key() {
                    keys.insert(k.to_string());
                }
            });
        }
    }
    keys.extend(crate::pages::PAGES.iter().map(|p| p.label.to_string()));
    let missing: Vec<&String> = keys.iter().filter(|k| !core::has_key(k)).collect();
    assert!(missing.is_empty(), "untranslated: {missing:#?}");
}

#[test]
fn access_keys_differ_within_each_menu() {
    install();
    for page in 0..crate::pages::PAGES.len() {
        let clashes = rt_core::menu::access_key_clashes(&crate::commands::menu_tree(page), &[]);
        assert!(clashes.is_empty(), "{clashes:#?}");
    }
}
