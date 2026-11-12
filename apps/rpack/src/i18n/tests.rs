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
    assert_eq!(tr_in(Lang::DeDE, "&Extract"), "&Entpacken");
    assert_eq!(tr_in(Lang::PtPT, "Open an archive"), "Abrir um arquivo");
    assert_eq!(tr_in(Lang::EnGB, "Name"), "Name");
    assert_eq!(tr_in(Lang::FrFR, "zzz unknown"), "zzz unknown");
    set_lang(Lang::EsES);
    assert_eq!(tf("{} files", &["3"]), "3 archivos");
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
    walk(&crate::commands::disk_menu(None, true, true), &mut keys);
    let missing: BTreeSet<String> = keys.into_iter().filter(|k| !has_key(k)).collect();
    assert!(missing.is_empty(), "untranslated menu strings: {missing:?}");
}

/// Within each menu, no two items share an access key, in any language.
#[test]
fn access_keys_differ_within_each_menu() {
    use crate::commands::{item_menu, menu_tree};
    install();
    let problems = rt_core::menu::access_key_clashes(&menu_tree(), &[("context menu", item_menu()), ("disk menu", crate::commands::disk_menu(None, true, true))]);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Draws every page in German; any string looked up without a translation fails.
#[test]
fn pages_render_fully_translated() {
    use crate::app::{App, Busy, Task};
    use crate::archive::{open, Meter};
    use crate::ui::gfx::{Gfx, Rect};
    use crate::ui::theme::Theme;
    use crate::ui::view;
    let Ok(mut g) = Gfx::new() else { return };
    set_lang(Lang::DeDE);
    take_misses();
    let d = crate::archive::tests::scratch("i18n");
    let zip = d.join("a.zip");
    crate::archive::tests::make_zip(&zip);
    let listing = open(&zip, &Meter::default()).unwrap();
    let full = Rect::new(0.0, 0.0, 1000.0, 700.0);
    g.resize(1000, 700);
    let theme = Theme::new(false, None);
    let mut app = App::new();
    let mut draw = |app: &App| {
        let l = view::layout(&mut g, app, full, 1.0, true);
        view::draw(&mut g, app, &l, &theme, 1.0, view::Hot::None);
        for i in 0..app.rows.len() {
            let _ = view::status_text(app, Some(i));
        }
        let _ = view::status_text(app, None);
    };
    app.show_drives(crate::app::tests_volumes(), None);
    draw(&app);
    app.show_folder(&d, Some(&zip)).unwrap();
    draw(&app);
    app.select_all();
    draw(&app);
    let _ = view::type_name("a.qqq");
    for kind in [crate::create::Kind::SevenZ, crate::create::Kind::Zip] {
        let mut f = crate::app::AddForm::new(vec![zip.clone(), d.clone()], d.clone(), crate::create::Spec::default());
        f.set_kind(kind);
        f.each = true;
        app.add = Some(f);
        draw(&app);
    }
    app.add = None;
    for task in [Task::Opening, Task::Extracting, Task::Testing, Task::Hashing] {
        app.busy = Some(Busy { task, meter: Default::default(), started: std::time::Instant::now(), total: 0, by_read: false, path: zip.clone(), count: 3 });
        draw(&app);
        let _ = view::busy_title(app.busy.as_ref().unwrap());
    }
    app.busy = None;
    app.set_opened(zip.clone(), listing);
    draw(&app);
    app.click(0, false, false);
    draw(&app);
    app.select_all();
    draw(&app);
    app.enter(app.opened.as_ref().unwrap().tree.nodes[0].dirs[0]);
    draw(&app);
    app.note = Some("x".into());
    draw(&app);
    set_lang(Lang::EnUS);
    let dir = d.to_string_lossy().to_string();
    let misses: Vec<String> = take_misses().into_iter().filter(|m| m.chars().any(|c| c.is_alphabetic()) && !m.contains(&dir)).collect();
    assert!(misses.is_empty(), "untranslated at run time: {misses:#?}");
}
