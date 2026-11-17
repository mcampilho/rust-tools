use super::*;
use std::collections::BTreeSet;

/// Starts recording untranslated lookups on this thread, and returns the
/// ones recorded so far.
fn take_misses() -> BTreeSet<String> {
    let misses = core::take_misses();
    core::record_misses(true);
    misses
}

/// Whether rpack's table or the common one translates `key`.
fn has_key(key: &str) -> bool {
    install();
    core::has_key(key)
}

#[test]
fn table_rows_are_well_formed() {
    let problems = core::check_table(TABLE.text(), '\t');
    assert!(problems.is_empty(), "{problems:#?}");
    let rows = TABLE.text().lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).count();
    assert!(rows > 60);
}

#[test]
fn lookups_and_fallbacks() {
    assert_eq!(tr_in(Lang::DeDE, "&Refresh"), "A&ktualisieren");
    assert_eq!(tr_in(Lang::PtPT, "Everyone"), "Todos");
    assert_eq!(tr_in(Lang::EnGB, "Name"), "Name");
    assert_eq!(tr_in(Lang::FrFR, "zzz unknown"), "zzz unknown");
    set_lang(Lang::EsES);
    assert_eq!(tf("{} shortcuts", &["3"]), "3 accesos directos");
    set_lang(Lang::EnUS);
}

fn rust_literal(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match c {
            '"' => return Some(out),
            '\\' => match it.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                'u' => {
                    let hex: String = it.by_ref().skip(1).take_while(|c| *c != '}').collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
    None
}

/// Every literal passed to tr()/tf() has a row in the table.
#[test]
fn source_strings_have_translations() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![root];
    let mut missing = Vec::new();
    let mut found = 0;
    while let Some(p) = files.pop() {
        if p.is_dir() {
            files.extend(std::fs::read_dir(&p).unwrap().flatten().map(|e| e.path()));
            continue;
        }
        if p.extension().is_none_or(|e| e != "rs") || p.file_name().is_some_and(|n| n == "tests.rs") {
            continue;
        }
        let full = std::fs::read_to_string(&p).unwrap();
        let src = full.split("#[cfg(test)]").next().unwrap_or("");
        for call in ["tr(\"", "tf(\"", "tf(\n                    \""] {
            let mut rest = src;
            while let Some(i) = rest.find(call) {
                let start = src.len() - rest.len() + i;
                let after = &rest[i + call.len()..];
                rest = after;
                if start > 0 && (src.as_bytes()[start - 1].is_ascii_alphanumeric() || src.as_bytes()[start - 1] == b'_') {
                    continue;
                }
                if let Some(lit) = rust_literal(after) {
                    found += 1;
                    if !has_key(&lit) {
                        missing.push(format!("{}: {lit:?}", p.display()));
                    }
                }
            }
        }
    }
    assert!(found > 30, "only {found} strings found");
    assert!(missing.is_empty(), "untranslated strings:\n{}", missing.join("\n"));
}

/// All command labels and menu titles are translated.
#[test]
fn commands_and_menus_have_translations() {
    use crate::commands::{menu_tree, program_menu, search_menu, M};
    fn walk(items: &[M], out: &mut Vec<String>) {
        for it in items {
            match it {
                M::Item(c) => {
                    if let Ok(k) = c.label_key() {
                        out.push(k.to_string());
                    }
                }
                M::Sub(t, children) => {
                    out.push(t.to_string());
                    walk(children, out);
                }
                _ => {}
            }
        }
    }
    let mut keys = Vec::new();
    for (title, items) in menu_tree() {
        keys.push(title.to_string());
        walk(&items, &mut keys);
    }
    walk(&program_menu(), &mut keys);
    walk(&search_menu(), &mut keys);
    let missing: BTreeSet<String> = keys.into_iter().filter(|k| !has_key(k)).collect();
    assert!(missing.is_empty(), "untranslated menu strings: {missing:?}");
}

/// Within each menu, no two items share an access key, in any language.
#[test]
fn access_keys_differ_within_each_menu() {
    use crate::commands::{menu_tree, program_menu, search_menu};
    install();
    let problems = rt_core::menu::access_key_clashes(&menu_tree(), &[("context menu", program_menu()), ("search menu", search_menu())]);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Draws the window in German; any string looked up without a translation fails.
#[test]
fn pages_render_fully_translated() {
    use crate::app::{App, Whose};
    use crate::ui::gfx::{Gfx, Rect};
    use crate::ui::theme::Theme;
    use crate::ui::view;
    let Ok(mut g) = Gfx::new() else { return };
    set_lang(Lang::DeDE);
    take_misses();
    let full = Rect::new(0.0, 0.0, 1200.0, 700.0);
    g.resize(1200, 700);
    let theme = Theme::new(false, None);
    let icons = std::collections::HashMap::new();
    let mut app = App::new();
    let mut draw = |app: &App, admin: bool| {
        let l = view::layout(&mut g, app, full, 1.0, true, admin);
        view::draw(&mut g, app, &l, &theme, 1.0, view::Hot::None, &icons, &|_| true);
        for i in 0..app.rows.len() {
            let _ = view::status_text(app, Some(i));
        }
        let _ = view::status_text(app, None);
        let _ = view::details_text(app);
    };
    draw(&app, false);
    app.set_found(crate::app::tests::sample());
    for w in Whose::ALL {
        app.whose = w;
        app.show_system = true;
        app.show_updates = true;
        app.rebuild();
        for i in 0..app.rows.len() {
            app.select(i);
            draw(&app, false);
        }
    }
    app.type_text("zzz no such program");
    draw(&app, true);
    set_lang(Lang::EnUS);
    let misses: Vec<String> = take_misses().into_iter().filter(|m| m.chars().any(|c| c.is_alphabetic()) && !m.contains("C:\\")).collect();
    assert!(misses.is_empty(), "untranslated at run time: {misses:#?}");
}