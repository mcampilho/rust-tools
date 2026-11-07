use super::*;
use std::collections::BTreeSet;

/// Starts recording untranslated lookups on this thread, and returns the
/// ones recorded so far.
fn take_misses() -> BTreeSet<String> {
    let misses = core::take_misses();
    core::record_misses(true);
    misses
}

/// Whether rsize's table or the common one translates `key`.
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
    assert_eq!(tr_in(Lang::DeDE, "&Rescan"), "&Erneut einlesen");
    assert_eq!(tr_in(Lang::EnGB, "Choose a folder to analyze"), "Choose a folder to analyse");
    assert_eq!(tr_in(Lang::PtPT, "Reading {}…"), "A ler {}…");
    assert_eq!(tr_in(Lang::FrFR, "zzz unknown"), "zzz unknown");
    set_lang(Lang::EsES);
    assert_eq!(tf("{} free of {}", &["1", "2"]), "1 libres de 2");
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
    use crate::commands::{item_menu, menu_tree, M};
    fn key_of(label: &str) -> Option<char> {
        let b: Vec<char> = label.chars().collect();
        b.windows(2).find(|w| w[0] == '&' && w[1] != '&').and_then(|w| w[1].to_lowercase().next())
    }
    fn check(title: &str, items: &[M], problems: &mut Vec<String>) {
        for l in Lang::ALL {
            let mut seen = std::collections::HashMap::new();
            for it in items {
                let key = match it {
                    M::Item(c) => c.label_key().ok().map(str::to_string),
                    M::Sub(t, _) => Some(t.to_string()),
                    _ => None,
                };
                let Some(key) = key else { continue };
                let text = tr_in(l, &key);
                if let Some(k) = key_of(&text) {
                    if let Some(other) = seen.insert(k, text.clone()) {
                        problems.push(format!("{title} [{}]: {other} / {text}", l.tag()));
                    }
                }
            }
        }
        for it in items {
            if let M::Sub(t, children) = it {
                check(t, children, problems);
            }
        }
    }
    let mut problems = Vec::new();
    let tree = menu_tree();
    let titles: Vec<M> = tree.iter().map(|(t, _)| M::Sub(t, Vec::new())).collect();
    check("menu bar", &titles, &mut problems);
    for (title, items) in &tree {
        check(title, items, &mut problems);
    }
    check("context menu", &item_menu(), &mut problems);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Draws every page in German; any string looked up without a translation fails.
#[test]
fn pages_render_fully_translated() {
    use crate::app::{App, Scanning};
    use crate::model::SizeMode;
    use crate::ui::gfx::{Gfx, Rect};
    use crate::ui::theme::Theme;
    use crate::ui::view;
    let Ok(mut g) = Gfx::new() else { return };
    set_lang(Lang::DeDE);
    take_misses();
    let tt = crate::scan::tests::TempTree::new("i18n");
    tt.file("a.txt", 100);
    tt.file("sub/b.bin", 5000);
    let scan = crate::scan::scan(&tt.0, &std::sync::atomic::AtomicBool::new(false), &|_| {}).unwrap();
    let full = Rect::new(0.0, 0.0, 1000.0, 700.0);
    g.resize(1000, 700);
    let theme = Theme::new(false, None);
    let mut app = App::new();
    let mut draw = |app: &App| {
        use crate::chart::{Chart, Hit};
        use crate::settings::ChartType;
        let l = view::layout(app, 0.45, full, 1.0, true);
        let (w, h) = (l.map.w as u32, l.map.h as u32);
        for kind in ChartType::ALL {
            let map = app.scan.as_ref().map(|s| crate::treemap::Map::new(&s.tree, app.zoom, w, h));
            let chart = app.scan.as_ref().map(|s| match kind {
                ChartType::Pie => Chart::pie(&s.tree, app.zoom, w, h, 1.0, 0xFFFFFF),
                _ => Chart::rings(&s.tree, app.zoom, w, h, 1.0, 0xFFFFFF),
            });
            let hover_seg = chart.as_ref().filter(|c| !c.segments.is_empty()).map(|_| Hit::Segment(0));
            let frame = view::Frame { mode: SizeMode::Allocated, kind, map: map.as_ref(), chart: chart.as_ref(), hover_seg, hot: view::Hot::None, hover: None };
            view::draw(&mut g, app, &l, &theme, 1.0, &frame);
        }
        let _ = view::status_text(app, None);
    };
    draw(&app);
    app.volumes = Some(crate::volumes::list());
    app.error = Some(tf("Could not read {}: {}", &["X:\\", "?"]));
    draw(&app);
    app.scanning = Some(Scanning {
        root: tt.0.clone(),
        progress: crate::scan::Progress { errors: 2, ..Default::default() },
        started: std::time::Instant::now(),
        generation: 1,
    });
    draw(&app);
    app.scanning = None;
    app.set_scan(scan, SizeMode::Allocated);
    app.toggle(app.rows[1].0);
    draw(&app);
    let _ = crate::format::size(5000);
    set_lang(Lang::EnUS);
    let dir_s = tt.0.to_string_lossy().to_string();
    let misses: Vec<String> = take_misses()
        .into_iter()
        .filter(|m| m.chars().any(|c| c.is_alphabetic()) && !m.contains(&dir_s))
        .collect();
    assert!(misses.is_empty(), "untranslated at run time: {misses:#?}");
}
