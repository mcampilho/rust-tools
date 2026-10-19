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

/// Number of menu access keys (`&x`, not `&&` or an entity like `&amp;`).
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
            let rest = &s[i + 1..];
            let entity = rest.find(';').is_some_and(|p| p > 0 && p < 8 && rest[..p].chars().all(|c| c.is_ascii_alphanumeric() || c == '#' || c == '{' || c == '}'));
            if !entity && rest.starts_with(|c: char| c.is_alphanumeric() || c == '{') {
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
        let keys_in_key = access_keys(cells[0]);
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
            if keys_in_key > 0 {
                assert_eq!(access_keys(c), 1, "line {n}: menu label needs exactly one access key in column {}: {c}", i + 1);
            }
        }
    }
    assert!(keys.len() > 400);
}

#[test]
fn lookups_and_fallbacks() {
    assert_eq!(tr_in(Lang::DeDE, "&Save"), "&Speichern");
    assert_eq!(tr_in(Lang::JaJP, "&New"), "新規(&N)");
    // en-GB falls back to en-US, pt-PT to pt-BR.
    assert_eq!(tr_in(Lang::EnGB, "&Save"), "&Save");
    assert_eq!(tr_in(Lang::PtPT, "Copy"), "Copiar");
    assert_eq!(tr_in(Lang::PtPT, "&Save"), "&Guardar");
    assert_eq!(tr_in(Lang::PtBR, "&Save"), "&Salvar");
    // Unknown strings pass through.
    assert_eq!(tr_in(Lang::FrFR, "zzz unknown"), "zzz unknown");
    take_misses();
    // Multi-line messages keep their line breaks; a literal "\n" in hints stays.
    let m = tr_in(Lang::DeDE, "Cannot open {}:\n\n{}");
    assert!(m.contains("\n\n"), "{m:?}");
    let h = tr_in(Lang::DeDE, "Use $1, ${name} for groups and \\n, \\t for line breaks and tabs");
    assert!(h.contains("\\n") && !h.contains('\n'), "{h:?}");
    set_lang(Lang::EsES);
    assert_eq!(tf("Saved {}", &["a.txt"]), "Se guardó a.txt");
    assert_eq!(tf("Ln {}, Col {}", &["3", "7"]), "Lín. 3, col. 7");
    set_lang(Lang::EnUS);
    assert_eq!(tf("Saved {}", &["a.txt"]), "Saved a.txt");
}

#[test]
fn mnemonics_are_stripped() {
    assert_eq!(strip_mnemonic("&Open..."), "Open...");
    assert_eq!(strip_mnemonic("開く(&O)..."), "開く...");
    assert_eq!(strip_mnemonic("Fish && Chips"), "Fish & Chips");
    assert_eq!(strip_mnemonic("Save A&ll"), "Save All");
}

#[test]
fn language_matching() {
    assert_eq!(Lang::best_match("de-AT"), Lang::DeDE);
    assert_eq!(Lang::best_match("pt-PT"), Lang::PtPT);
    assert_eq!(Lang::best_match("pt-AO"), Lang::PtBR);
    assert_eq!(Lang::best_match("zh-Hans-CN"), Lang::ZhCN);
    assert_eq!(Lang::best_match("en-AU"), Lang::EnGB);
    assert_eq!(Lang::best_match("ko-KR"), Lang::EnUS);
    for l in Lang::ALL {
        assert_eq!(Lang::from_tag(l.tag()), Some(l));
    }
}

/// Parses the Rust string literal that starts just after the opening quote.
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

/// Every literal passed to tr()/tf() (and to the text tools, which
/// translate their names) has a row in the table.
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
        for call in ["tr(\"", "tf(\"", "line_tool(\"", "text_tool(\"", "tr(if ", "Msg::new(\"", "Msg::with(\n                \""] {
            let mut rest = src;
            while let Some(i) = rest.find(call) {
                let start = src.len() - rest.len() + i;
                let after = &rest[i + call.len()..];
                rest = after;
                let ident_before = start > 0 && (src.as_bytes()[start - 1].is_ascii_alphanumeric() || src.as_bytes()[start - 1] == b'_');
                if ident_before && !call.starts_with("Msg") {
                    continue; // e.g. `str("` or `my_tr("`
                }
                let lits: Vec<String> = if call == "tr(if " {
                    // tr(if cond { "a" } else { "b" })
                    let end = after.find("})").map_or(0, |e| e + 1);
                    after[..end].split('"').skip(1).step_by(2).filter_map(|q| rust_literal(&format!("{q}\""))).collect()
                } else {
                    rust_literal(after).into_iter().collect()
                };
                for lit in lits {
                    found += 1;
                    if !table().contains_key(lit.replace('\n', "\\n").as_str()) {
                        missing.push(format!("{}: {lit:?}", p.display()));
                    }
                }
            }
        }
    }
    assert!(found > 200, "only {found} strings found");
    assert!(missing.is_empty(), "untranslated strings:\n{}", missing.join("\n"));
}

/// All command labels, menu titles and palette categories are translated.
#[test]
fn commands_and_menus_have_translations() {
    use crate::app::commands::{menu_tree, Command, M};
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
    for (title, items) in menu_tree(&[]) {
        keys.push(title.to_string());
        walk(&items, &mut keys);
    }
    for (c, cat) in Command::palette() {
        keys.push(cat.to_string());
        if let Ok(k) = c.label_key() {
            keys.push(k.to_string());
        }
    }
    let missing: BTreeSet<String> = keys.into_iter().filter(|k| !table().contains_key(k.as_str())).collect();
    assert!(missing.is_empty(), "untranslated menu strings: {missing:?}");
}

/// Renders the whole window in German with every panel open and runs
/// common commands; any string looked up without a translation fails.
#[test]
fn ui_renders_fully_translated() {
    use crate::app::commands::Command;
    use crate::app::settings::Settings;
    use crate::app::{Answer, App, Focus, Host};
    use crate::storage::Location;
    use crate::ui::gfx::Gfx;
    use crate::ui::{Input, Ui};
    use std::path::{Path, PathBuf};
    struct Quiet;
    impl Host for Quiet {
        fn ask_save(&self, _: &str) -> Answer {
            Answer::Cancel
        }
        fn confirm(&self, _: &str, _: &str) -> bool {
            false
        }
        fn info(&self, _: &str, _: &str) {}
        fn error(&self, _: &str, _: &str) {}
        fn open_dialog(&self, _: Option<&Path>) -> Vec<PathBuf> {
            Vec::new()
        }
        fn save_dialog(&self, _: &str, _: Option<&Path>) -> Option<PathBuf> {
            None
        }
        fn pick_folder(&self) -> Option<PathBuf> {
            None
        }
        fn choose_font(&self, _: &str, _: f32) -> Option<(String, f32)> {
            None
        }
        fn clipboard_get(&self) -> Option<String> {
            None
        }
        fn clipboard_set(&self, _: &str) {}
        fn reveal(&self, _: &Path) {}
    }
    // Needs the system fonts; skip quietly where they are unavailable.
    let Ok(mut gfx) = Gfx::new() else { return };
    set_lang(Lang::DeDE);
    take_misses();
    let dir = std::env::temp_dir().join(format!("rpad-i18n-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let loc = Location { dir: Some(dir.clone()), portable: true, error: None };
    let s = Settings { toolbar: true, outline: true, markdown_preview: true, language: Some(Lang::DeDE), ..Settings::default() };
    let mut app = App::new(loc, s);
    let h = Quiet;
    let md = dir.join("n.md");
    std::fs::write(&md, "# Title\r\n\r\ntext\n![img](x.png)\n").unwrap();
    app.open_path(&md, &h);
    app.dispatch(Command::New, &h);
    app.edit(|b| b.insert_text("fn main() {\n    let x = 1;\n}\n", false));
    app.dispatch(Command::SelectAll, &h);
    app.dispatch(Command::Replace, &h);
    app.find.find.set("x");
    app.find.regex = true;
    app.dispatch(Command::FindInFiles, &h);
    app.focus = Focus::Editor;
    let inp = Input { caret_on: true, ..Input::default() };
    let mut render = |app: &mut App| {
        gfx.resize(1280, 800);
        let theme = crate::ui::theme::Theme::new(true, crate::ui::gfx::Color::rgb(0x0067C0));
        let mut ui = Ui::new(&mut gfx, theme, 1.0, &inp);
        crate::ui::view::build(app, &mut ui, (10.0, 10.0));
    };
    render(&mut app);
    app.activate(0);
    render(&mut app);
    app.dispatch(Command::CommandPalette, &h);
    render(&mut app);
    app.palette = None;
    for c in [Command::GotoLine, Command::Shortcuts, Command::About, Command::XmlCheck, Command::SortAsc, Command::Upper, Command::CopyPath, Command::GotoBracket] {
        app.dispatch(c, &h);
        render(&mut app);
        app.palette = None;
    }
    set_lang(Lang::EnUS);
    let _ = std::fs::remove_dir_all(&dir);
    let names: BTreeSet<&str> = crate::syntax::lang::DEFS.iter().map(|d| d.name).collect();
    let misses: Vec<String> = take_misses().into_iter().filter(|m| !names.contains(m.as_str()) && m.chars().any(|c| c.is_alphabetic())).collect();
    assert!(misses.is_empty(), "untranslated at run time: {misses:#?}");
}
