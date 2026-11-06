use super::*;
use std::cell::RefCell;
use std::collections::BTreeSet;

thread_local! {
    static MISSES: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
}

pub(super) fn record_miss(s: &str) {
    MISSES.with(|m| {
        m.borrow_mut().insert(s.to_string());
    });
}

fn take_misses() -> BTreeSet<String> {
    MISSES.with(|m| std::mem::take(&mut *m.borrow_mut()))
}

fn rows() -> Vec<(usize, Vec<&'static str>)> {
    TABLE
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|(n, l)| (n + 1, l.split('\t').collect()))
        .collect()
}

/// Number of menu access keys (`&x`, not `&&`).
fn access_keys(s: &str) -> usize {
    let b = s.as_bytes();
    let mut n = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'&' {
            if b.get(i + 1) == Some(&b'&') {
                i += 2;
                continue;
            }
            if s[i + 1..].starts_with(|c: char| c.is_alphanumeric()) {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

#[test]
fn table_rows_are_well_formed() {
    let mut keys = BTreeSet::new();
    for (n, cells) in rows() {
        assert_eq!(cells.len(), 10, "line {n}: expected 10 columns: {cells:?}");
        assert!(keys.insert(cells[0]), "line {n}: duplicate key {}", cells[0]);
        let placeholders = cells[0].matches("{}").count();
        let menu = access_keys(cells[0]) > 0;
        for (i, c) in cells.iter().enumerate().skip(1) {
            // en-GB and pt-PT may be empty (fallback); every other language is required.
            let optional = i == 1 || i == 8;
            assert!(optional || !c.is_empty(), "line {n}: missing translation in column {}", i + 1);
            if c.is_empty() {
                continue;
            }
            assert_eq!(c.matches("{}").count(), placeholders, "line {n}: placeholder mismatch in column {}: {c}", i + 1);
            assert_eq!(c.trim(), *c, "line {n}: stray whitespace in column {}", i + 1);
            assert_eq!(c.matches("\\n").count(), cells[0].matches("\\n").count(), "line {n}: line breaks differ in column {}", i + 1);
            if menu {
                assert_eq!(access_keys(c), 1, "line {n}: menu label needs exactly one access key in column {}: {c}", i + 1);
            }
        }
    }
    assert!(keys.len() > 60);
}

#[test]
fn lookups_and_fallbacks() {
    assert_eq!(tr_in(Lang::DeDE, "&Open..."), "Ö&ffnen...");
    assert_eq!(tr_in(Lang::EnGB, "&Open..."), "&Open...");
    assert_eq!(tr_in(Lang::PtPT, "&Reload"), tr_in(Lang::PtBR, "&Reload"));
    assert_eq!(tr_in(Lang::FrFR, "zzz unknown"), "zzz unknown");
    take_misses();
    set_lang(Lang::EsES);
    assert_eq!(tf("Page {} of {}", &["2", "9"]), "Página 2 de 9");
    set_lang(Lang::EnUS);
    assert_eq!(tf("Page {} of {}", &["2", "9"]), "Page 2 of 9");
}

#[test]
fn language_matching() {
    assert_eq!(Lang::best_match("de-AT"), Lang::DeDE);
    assert_eq!(Lang::best_match("pt-PT"), Lang::PtPT);
    assert_eq!(Lang::best_match("pt-AO"), Lang::PtBR);
    assert_eq!(Lang::best_match("zh-Hans-CN"), Lang::ZhCN);
    assert_eq!(Lang::best_match("en-AU"), Lang::EnGB);
    assert_eq!(Lang::best_match("ko-KR"), Lang::EnUS);
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
                    if !table().contains_key(lit.replace('\n', "\\n").as_str()) {
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
    use crate::commands::{menu_tree, M};
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
                M::Sep => {}
            }
        }
    }
    let mut keys = Vec::new();
    for (title, items) in menu_tree() {
        keys.push(title.to_string());
        walk(&items, &mut keys);
    }
    let missing: BTreeSet<String> = keys.into_iter().filter(|k| !table().contains_key(k.as_str())).collect();
    assert!(missing.is_empty(), "untranslated menu strings: {missing:?}");
}

/// Draws each document type in German; any string looked up without a
/// translation fails.
#[test]
fn views_render_fully_translated() {
    use crate::doc::{self, Kind};
    use crate::ui::gfx::{Gfx, Rect};
    use crate::view::{State, Viewer};
    let Ok(mut g) = Gfx::new() else { return };
    set_lang(Lang::DeDE);
    take_misses();
    let dir = std::env::temp_dir().join(format!("rview-i18n-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let files = [
        ("a.txt", "hello\r\nworld\r\n"),
        ("a.csv", "x;y\n1;2\n"),
        ("a.md", "# T\n\ntext ![x](missing.png)\n"),
        ("a.html", "<p>hi <img src=nothing.png alt=x></p>"),
        ("a.rs", "fn main() { /* x */ }"),
        ("b.html", "<body style='background:#fff;color:#333'><pre><code class=language-js>let x = 1;</code></pre></body>"),
    ];
    let media = crate::media::Media::start(std::sync::Arc::new(|| {}));
    let theme = crate::ui::theme::Theme::new(false);
    let dark_theme = crate::ui::theme::Theme::new(true);
    let area = Rect::new(0.0, 0.0, 800.0, 576.0);
    for (name, body) in files {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        let mut v = Viewer::new(true);
        let gen = v.begin(&p, false, None);
        v.finish(gen, doc::load(&p, Kind::of(&p)));
        assert!(matches!(v.state, State::Ready(_)), "{name}");
        g.resize(800, 600);
        v.draw(&mut g, area, &theme, 1.0, &media);
        v.draw(&mut g, area, &dark_theme, 1.0, &media);
        let _ = v.status();
        if name == "a.rs" {
            assert_eq!(v.language(), Some(crate::syntax::Lang::Rust));
        }
    }
    let mut v = Viewer::new(true);
    v.draw(&mut g, area, &theme, 1.0, &media);
    let missing = dir.join("missing.txt");
    let gen = v.begin(&missing, false, None);
    v.finish(gen, doc::load(&missing, Kind::Text));
    v.draw(&mut g, area, &theme, 1.0, &media);
    set_lang(Lang::EnUS);
    let _ = std::fs::remove_dir_all(&dir);
    let dir_s = dir.to_string_lossy().to_string();
    // Programming language names are proper nouns.
    let names: BTreeSet<&str> = crate::syntax::lang::DEFS.iter().map(|d| d.name).collect();
    let misses: Vec<String> = take_misses().into_iter().filter(|m| m.chars().any(|c| c.is_alphabetic()) && !m.contains(&dir_s) && !names.contains(m.as_str())).collect();
    assert!(misses.is_empty(), "untranslated at run time: {misses:#?}");
}
