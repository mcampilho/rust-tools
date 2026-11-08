use super::*;
use std::collections::BTreeSet;

/// Starts recording untranslated lookups on this thread, and returns the
/// ones recorded so far.
fn take_misses() -> BTreeSet<String> {
    let misses = core::take_misses();
    core::record_misses(true);
    misses
}

/// Whether rdup's table or the common one translates `key`.
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
    assert_eq!(tr_in(Lang::DeDE, "Find &Duplicates"), "&Duplikate suchen");
    assert_eq!(tr_in(Lang::PtPT, "Comparing the contents…"), "A comparar o conteúdo…");
    assert_eq!(tr_in(Lang::EnGB, "Folder"), "Folder");
    assert_eq!(tr_in(Lang::FrFR, "zzz unknown"), "zzz unknown");
    set_lang(Lang::EsES);
    assert_eq!(tf("{} copies", &["3"]), "3 copias");
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
    assert!(found > 40, "only {found} strings found");
    assert!(missing.is_empty(), "untranslated strings:\n{}", missing.join("\n"));
}

/// All command labels and menu titles are translated.
#[test]
fn commands_and_menus_have_translations() {
    use crate::commands::{item_menu, menu_tree, M};
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
    walk(&item_menu(), &mut keys);
    let missing: BTreeSet<String> = keys.into_iter().filter(|k| !has_key(k)).collect();
    assert!(missing.is_empty(), "untranslated menu strings: {missing:?}");
}

/// Within each menu, no two items share an access key, in any language.
#[test]
fn access_keys_differ_within_each_menu() {
    use crate::commands::{item_menu, menu_tree};
    install();
    let problems = rt_core::menu::access_key_clashes(&menu_tree(), &[("context menu", item_menu())]);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Draws every page in German; any string looked up without a translation fails.
#[test]
fn pages_render_fully_translated() {
    use crate::app::{App, Searching};
    use crate::search::{Progress, Stage};
    use crate::ui::gfx::{Gfx, Rect};
    use crate::ui::theme::Theme;
    use crate::ui::view;
    let Ok(mut g) = Gfx::new() else { return };
    set_lang(Lang::DeDE);
    take_misses();
    let tt = crate::search::tests::sample("rdup-i18n");
    let found = crate::search::tests::sys(std::slice::from_ref(&tt.0)).unwrap();
    let full = Rect::new(0.0, 0.0, 1000.0, 700.0);
    g.resize(1000, 700);
    let theme = Theme::new(false, None);
    let mut app = App::new();
    let mut draw = |app: &App| {
        let l = view::layout(app, full, 1.0, true);
        view::draw(&mut g, app, &l, &theme, 1.0, view::Hot::None);
        let _ = view::status_text(app, None);
    };
    draw(&app);
    app.volumes = Some(crate::volumes::list());
    app.toggle_root(&tt.0);
    app.error = Some(tr("The search was stopped."));
    draw(&app);
    use crate::filter::Field;
    for (f, bad) in [(Field::MinSize, "lots"), (Field::From, "someday"), (Field::MaxSize, "1 KB"), (Field::To, "1990")] {
        let mut c = crate::filter::Choice::default();
        c.texts[Field::From.index()] = "2000".into();
        c.texts[f.index()] = bad.into();
        app.choice = c;
        app.focus_field(f, None);
        draw(&app);
    }
    app.choice = crate::filter::Choice::default();
    app.choice.toggle_kind(1);
    app.choice.texts[Field::SkipExt.index()] = "mp4".into();
    draw(&app);
    for month in 1..=12 {
        app.calendar = Some(crate::app::Calendar { field: Field::From, year: 2024, month });
        draw(&app);
    }
    app.calendar = None;
    app.choice.texts[Field::SkipExt.index()].clear();
    draw(&app);
    app.compare = [false; 2];
    draw(&app);
    app.compare = [true; 2];
    for (stage, roots) in [(Stage::Reading, 1), (Stage::Reading, 2), (Stage::Edges, 1), (Stage::Contents, 1), (Stage::Folders, 1)] {
        app.searching = Some(Searching {
            roots: vec![tt.0.clone(), tt.0.clone()],
            progress: Progress { stage, roots, total: 5, total_bytes: 5000, read: rt_scan::Progress { errors: 2, ..Default::default() }, ..Progress::default() },
            started: std::time::Instant::now(),
            generation: 1,
        });
        draw(&app);
    }
    app.searching = None;
    app.set_found(found);
    draw(&app);
    app.sel = Some(0);
    draw(&app);
    let _ = app.toggle_mark(1);
    draw(&app);
    let _ = app.csv();
    // The folders: none, then identical and similar ones.
    app.set_view(crate::app::View::Folders);
    draw(&app);
    let ft = crate::folders::tests::folders("rdup-i18n-folders");
    app.set_found(crate::folders::tests::search(&ft, 1000));
    app.set_view(crate::app::View::Folders);
    let mut only = crate::folders::tests::search(&ft, 1000);
    only.look = [false, true];
    app.set_found(only);
    draw(&app);
    app.set_found(crate::folders::tests::search(&ft, 1000));
    app.set_view(crate::app::View::Folders);
    for share in crate::folders::SHARES {
        app.set_share(share);
        for sel in 0..app.rows.len() {
            app.sel = Some(sel);
            draw(&app);
        }
        let _ = app.csv();
    }
    let _ = crate::format::size(5000);
    set_lang(Lang::EnUS);
    let dir_s = tt.0.to_string_lossy().to_string();
    let misses: Vec<String> = take_misses()
        .into_iter()
        .filter(|m| m.chars().any(|c| c.is_alphabetic()) && !m.contains(&dir_s) && !m.contains(&ft.0.to_string_lossy().to_string()))
        .collect();
    assert!(misses.is_empty(), "untranslated at run time: {misses:#?}");
}
