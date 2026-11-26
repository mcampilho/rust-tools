use super::*;
use crate::app::{tests::sample, View};
use crate::category::Category;
use crate::settings::Settings;
use crate::ui::gfx::Gfx;
use crate::ui::theme::Theme;
use crate::ui::view::{draw, Model, SIZE};
use std::collections::HashMap;

/// A model of the menu to draw in the tests.
fn model<'a>(
    app: &'a crate::app::App,
    settings: &'a Settings,
    icons: &'a HashMap<std::path::PathBuf, Option<tiny_skia::Pixmap>>,
    note: Option<&'a str>,
) -> Model<'a> {
    Model {
        app,
        settings,
        icons,
        note,
        autostart: true,
        hot: None,
        disk: Some(("E:".to_string(), 70_500_000_000, 500_000_000_000)),
        caret: true,
    }
}

#[test]
fn table_rows_are_well_formed() {
    let problems = rt_core::i18n::check_table(TABLE.text(), '\t');
    assert!(problems.is_empty(), "{problems:#?}");
    let rows = TABLE.text().lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).count();
    assert!(rows > 40);
}

#[test]
fn lookups_and_fallbacks() {
    assert_eq!(tr_in(Lang::PtPT, "Downloads"), "Transferências");
    assert_eq!(tr_in(Lang::PtBR, "Downloads"), "Downloads");
    assert_eq!(tr_in(Lang::PtPT, "Games"), "Jogos");
    assert_eq!(tr_in(Lang::FrFR, "zzz unknown"), "zzz unknown");
}

/// Every view drawn in every language looks up only strings that exist.
#[test]
fn every_view_is_translated() {
    let mut g = Gfx::new().unwrap();
    g.resize(SIZE.0 as u32, SIZE.1 as u32);
    let mut app = sample();
    let settings = Settings { folders: vec!["C:\\Tools".into()], ..Settings::default() };
    let icons = HashMap::new();
    set_lang(Lang::EnUS);
    // The categories are looked up by name, which no literal in the code shows.
    for c in Category::ALL {
        assert!(rt_core::i18n::has_key(c.name()), "{}", c.name());
    }
    rt_core::i18n::record_misses(true);
    set_lang(Lang::DeDE);
    for view in [View::Categories, View::Inside(Category::Internet), View::Options] {
        app.view = view;
        draw(&mut g, &model(&app, &settings, &icons, None), &Theme::new(false, None), 1.0);
    }
    app.view = View::Categories;
    app.query = "zzz".into();
    draw(&mut g, &model(&app, &settings, &icons, None), &Theme::new(true, None), 1.0);
    let misses = rt_core::i18n::take_misses();
    rt_core::i18n::record_misses(false);
    set_lang(Lang::EnUS);
    assert!(misses.is_empty(), "untranslated: {misses:#?}");
}

/// Not a test: `apps/rlaunch/target/screens/*.png`.
#[test]
#[ignore]
fn screenshots() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
    std::fs::create_dir_all(&out).unwrap();
    let mut g = Gfx::new().unwrap();
    g.resize(SIZE.0 as u32, SIZE.1 as u32);
    let settings =
        Settings { folders: vec!["C:\\Tools".into(), "E:\\Portable\\Apps".into(), "D:\\PortableApps".into()], ..Settings::default() };
    // The programs of this PC, with their icons.
    let mut app = crate::app::App::default();
    app.set_entries(crate::sources::scan(&settings));
    #[allow(unused_mut)]
    let mut icons = HashMap::new();
    #[cfg(windows)]
    {
        // SAFETY: initializes COM on this thread, as the icon thread does.
        unsafe { windows_sys::Win32::System::Com::CoInitializeEx(std::ptr::null(), 2) };
        for e in &app.entries {
            icons.insert(e.path.clone(), rt_sys::win::icon::of_file(&e.path).and_then(|(w, h, px)| crate::ui::view::pixmap(w, h, px)));
        }
    }
    let dev = app.entries.iter().find(|e| e.category == Category::Development).map_or(Category::Other, |e| e.category);
    for (name, lang, dark, view, query) in [
        ("pt-categories", Lang::PtPT, false, View::Categories, ""),
        ("pt-development-dark", Lang::PtPT, true, View::Inside(dev), ""),
        ("en-search", Lang::EnUS, false, View::Categories, "co"),
        ("fr-categories", Lang::FrFR, false, View::Categories, ""),
        ("pt-options", Lang::PtPT, false, View::Options, ""),
        ("de-options-dark", Lang::DeDE, true, View::Options, ""),
    ] {
        set_lang(lang);
        app.view = view;
        app.query = query.into();
        app.selected = 1;
        let note = (name == "de-options-dark").then(|| tf("Another program is using {}.", &["Ctrl+Alt+Space"]));
        draw(&mut g, &model(&app, &settings, &icons, note.as_deref()), &Theme::new(dark, None), 1.0);
        let (w, h) = (SIZE.0 as u32, SIZE.1 as u32);
        let mut buf = vec![0u32; (w * h) as usize];
        g.copy_to(&mut buf);
        let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
        for (d, s) in pm.pixels_mut().iter_mut().zip(&buf) {
            *d = tiny_skia::ColorU8::from_rgba((s >> 16) as u8, (s >> 8) as u8, *s as u8, 255).premultiply();
        }
        pm.save_png(out.join(format!("{name}.png"))).unwrap();
    }
    set_lang(Lang::EnUS);
    println!("{} programs", app.entries.len());
    for c in Category::ALL {
        let names: Vec<&str> = app.entries.iter().filter(|e| e.category == c).map(|e| e.name.as_str()).collect();
        println!("{} ({}): {}", c.name(), names.len(), names.join(", "));
    }
}
