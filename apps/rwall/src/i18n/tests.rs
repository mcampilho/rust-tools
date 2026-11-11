use super::*;
use std::collections::BTreeSet;

/// Starts recording untranslated lookups on this thread, and returns the
/// ones recorded so far.
fn take_misses() -> BTreeSet<String> {
    let misses = core::take_misses();
    core::record_misses(true);
    misses
}

/// Whether rwall's table or the common one translates `key`.
fn has_key(key: &str) -> bool {
    install();
    core::has_key(key)
}

#[test]
fn table_rows_are_well_formed() {
    let problems = core::check_table(TABLE.text(), '\t');
    assert!(problems.is_empty(), "{problems:#?}");
    let rows = TABLE.text().lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).count();
    assert!(rows > 40);
}

#[test]
fn lookups_and_fallbacks() {
    assert_eq!(tr_in(Lang::PtPT, "Every day"), "Todos os dias");
    assert_eq!(tr_in(Lang::EsES, "Every day"), "Cada día");
    assert_eq!(tr_in(Lang::EnGB, "Center"), "Centre");
    // pt-PT falls back to pt-BR where they agree.
    assert_eq!(tr_in(Lang::PtPT, "Fill"), "Preencher");
    set_lang(Lang::PtPT);
    assert_eq!(tf("Next change: {}", &["2026-11-12 00:00"]), "Próxima mudança: 2026-11-12 00:00");
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
        for call in ["tr(\"", "tf(\""] {
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
    assert!(found > 25, "only {found} strings found");
    assert!(missing.is_empty(), "untranslated strings:\n{}", missing.join("\n"));
}

/// The labels chosen at run time (frequencies, sources, positions, the tray
/// menu) are translated.
#[test]
fn labels_have_translations() {
    use crate::commands::TrayCmd;
    let mut keys: Vec<&str> = crate::schedule::Frequency::ALL.iter().map(|f| crate::ui::view::frequency_label(*f)).collect();
    keys.extend(crate::sources::Source::ALL.iter().map(|s| s.label()));
    keys.extend(crate::settings::Fit::ALL.iter().map(|f| f.label()));
    keys.extend(TrayCmd::ALL.iter().map(|c| c.key()));
    let missing: Vec<&str> = keys.into_iter().filter(|k| !has_key(k)).collect();
    assert!(missing.is_empty(), "untranslated labels: {missing:?}");
}

/// Within the tray menu, no two items share an access key, in any language.
#[test]
fn access_keys_differ_in_the_tray_menu() {
    install();
    let problems = rt_core::menu::access_key_clashes::<crate::commands::TrayCmd>(&Vec::new(), &[("tray menu", crate::commands::tray_menu())]);
    assert!(problems.is_empty(), "{problems:#?}");
}

/// Draws the settings window in every language; any string looked up
/// without a translation fails.
#[test]
fn window_renders_fully_translated() {
    use crate::settings::{Current, Settings};
    use crate::sources::Source;
    use crate::ui::gfx::{Gfx, Rect};
    use crate::ui::theme::Theme;
    use crate::ui::view::{draw, Field, Model};
    let Ok(mut g) = Gfx::new() else { return };
    g.resize(640, 900);
    let mut st = Settings { sources: Source::ALL.to_vec(), folders: vec!["C:\\Pictures".into()], ..Settings::default() };
    for l in Lang::ALL {
        set_lang(l);
        take_misses();
        for current in [None, Some(Current { title: "T".into(), credit: "C".into(), source: "bing".into(), link: "https://x".into(), ..Current::default() })] {
            st.current = current;
            let m = Model {
                settings: &st,
                thumb: None,
                busy: false,
                status: tf("Next change: {}", &["2026-11-12 00:00"]),
                error: None,
                autostart: true,
                focus: Some(Field::NasaKey),
                hot: None,
                scroll: 0.0,
            };
            draw(&mut g, &m, &Theme::new(false, None), 1.0, Rect::new(0.0, 0.0, 640.0, 900.0));
        }
        let _ = (tr("Changing…"), tr("Looking for a picture…"));
        let misses: Vec<String> = take_misses().into_iter().filter(|m| m.chars().any(|c| c.is_alphabetic()) && m != "T" && m != "C").collect();
        assert!(misses.is_empty() || l == Lang::EnUS, "untranslated in {}: {misses:#?}", l.tag());
    }
    set_lang(Lang::EnUS);
}

/// The settings window as PNG files in `target/screens` (run on request:
/// `cargo test -p rwall --lib screenshots -- --ignored`).
#[test]
#[ignore]
fn screenshots() {
    use crate::settings::{Current, Settings};
    use crate::sources::Source;
    use crate::ui::gfx::{Gfx, Rect};
    use crate::ui::theme::Theme;
    use crate::ui::view::{draw, Hit, Model};
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
    std::fs::create_dir_all(&out).unwrap();
    let mut g = Gfx::new().unwrap();
    let (w, h) = (640u32, 860u32);
    g.resize(w, h);
    let thumb = {
        let mut pm = tiny_skia::Pixmap::new(480, 270).unwrap();
        for (i, px) in pm.pixels_mut().iter_mut().enumerate() {
            let (x, y) = ((i % 480) as u32, (i / 480) as u32);
            let sky = 255 - (y * 120 / 270) as u8;
            let ground = y > 170 + (x as f32 / 30.0).sin().abs() as u32 * 20;
            *px = if ground { tiny_skia::ColorU8::from_rgba(40, 120, 70, 255) } else { tiny_skia::ColorU8::from_rgba(60, 140, sky, 255) }.premultiply();
        }
        pm
    };
    let st = Settings {
        sources: vec![Source::Bing, Source::Folders],
        folders: vec!["C:\\Users\\Me\\Pictures\\Wallpapers".into(), "D:\\Fotos\\Viagens".into()],
        current: Some(Current {
            title: "Torre de Belém, Lisboa".into(),
            credit: "© Example Photographer / Example Agency".into(),
            source: "bing".into(),
            link: "https://www.bing.com".into(),
            when: "2026-11-11 00:00".into(),
            ..Current::default()
        }),
        ..Settings::default()
    };
    for (l, dark, name) in [(Lang::EnUS, false, "en-light"), (Lang::PtPT, true, "pt-dark"), (Lang::EsES, false, "es-light")] {
        set_lang(l);
        let m = Model {
            settings: &st,
            thumb: Some(&thumb),
            busy: false,
            status: tf("Next change: {}", &["2026-11-12 00:00"]),
            error: None,
            autostart: true,
            focus: None,
            hot: Some(Hit::Next),
            scroll: 0.0,
        };
        draw(&mut g, &m, &Theme::new(dark, None), 1.0, Rect::new(0.0, 0.0, w as f32, h as f32));
        let mut buf = vec![0u32; (w * h) as usize];
        g.copy_to(&mut buf);
        let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
        for (d, s) in pm.pixels_mut().iter_mut().zip(&buf) {
            *d = tiny_skia::ColorU8::from_rgba((s >> 16) as u8, (s >> 8) as u8, *s as u8, 255).premultiply();
        }
        pm.save_png(out.join(format!("{name}.png"))).unwrap();
    }
    set_lang(Lang::EnUS);
}
