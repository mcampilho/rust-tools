use super::*;
use std::collections::BTreeSet;

/// Starts recording untranslated lookups on this thread, and returns the
/// ones recorded so far.
fn take_misses() -> BTreeSet<String> {
    let misses = core::take_misses();
    core::record_misses(true);
    misses
}

/// Whether rpad's table or the common one translates `key`.
fn has_key(key: &str) -> bool {
    install();
    core::has_key(key)
}

#[test]
fn table_rows_are_well_formed() {
    let problems = core::check_table(TABLE.text(), '\t');
    assert!(problems.is_empty(), "{problems:#?}");
    let rows = TABLE.text().lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).count();
    assert!(rows > 400);
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
                    if !has_key(&lit) {
                        missing.push(format!("{}: {lit:?}", p.display()));
                    }
                }
            }
        }
    }
    assert!(found > 200, "only {found} strings found");
    assert!(missing.is_empty(), "untranslated strings:\n{}", missing.join("\n"));
}

/// Within each menu, no two items share an access key, in any language.
#[test]
fn access_keys_differ_within_each_menu() {
    use crate::app::commands::{editor_menu, menu_tree, tab_menu, M};
    install();
    let encodings = vec![M::Sub("&Save with encoding", Vec::new()), M::Sub("&Reopen with encoding", Vec::new())];
    let context = [("editor menu", editor_menu()), ("tab menu", tab_menu()), ("encoding menu", encodings)];
    let problems = rt_core::menu::access_key_clashes(&menu_tree(&[]), &context);
    assert!(problems.is_empty(), "{} clashes: {problems:#?}", problems.len());
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
    let missing: BTreeSet<String> = keys.into_iter().filter(|k| !has_key(k)).collect();
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
    let loc = Location::at(crate::storage::PLACE, dir.clone());
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
