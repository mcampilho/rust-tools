//! Renders every view to PNG files in `target/screens` for visual review:
//! `cargo test -- --ignored screenshots`.

use super::gfx::{Color, Gfx};
use super::theme::Theme;
use super::{view, Input, Ui};
use crate::app::{Action, App, ComboId, Mode, Overlay, PanelTab};
use crate::calc::Key;
use crate::expr::BinOp;
use crate::storage::Location;

fn app() -> App {
    App::new(Location { error: None, ..Location::nowhere(crate::storage::PLACE) })
}

fn shot(g: &mut Gfx, app: &App, name: &str, w: u32, h: u32, scale: f32, dark: bool) {
    g.resize((w as f32 * scale) as u32, (h as f32 * scale) as u32);
    let input = Input::default();
    let theme = Theme::new(dark, Color::rgb(0x0078D4));
    let mut ui = Ui::new(g, theme, scale, &input);
    view::build(app, &mut ui);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("screens");
    std::fs::create_dir_all(&dir).unwrap();
    g.pix.save_png(dir.join(format!("{name}.png"))).unwrap();
}

fn keys(a: &mut App, ks: &[Key]) {
    for k in ks {
        a.apply(&Action::Key(*k));
    }
}

/// Drives the app through every mode and overlay, calling `f` after each step.
fn tour(a: &mut App, mut f: impl FnMut(&App, &str)) {
    keys(
        a,
        &[
            Key::Digit(1),
            Key::Digit(2),
            Key::Point,
            Key::Digit(5),
            Key::Bin(BinOp::Add),
            Key::Digit(5),
            Key::Equals,
        ],
    );
    f(a, "standard");
    a.apply(&Action::Mem(crate::app::MemOp::Store));
    a.apply(&Action::Open(Overlay::Panel(PanelTab::History)));
    f(a, "history");
    a.apply(&Action::Open(Overlay::Panel(PanelTab::Memory)));
    f(a, "memory");
    a.apply(&Action::OpenNav);
    f(a, "nav");
    a.apply(&Action::Navigate(Mode::Scientific));
    f(a, "scientific");
    for o in [Overlay::Trig, Overlay::Func] {
        a.apply(&Action::Open(o));
        f(a, "sci-flyout");
        a.apply(&Action::CloseOverlay);
    }
    keys(a, &[Key::Digit(1), Key::Bin(BinOp::Div), Key::Digit(0), Key::Equals]);
    f(a, "error");
    a.apply(&Action::Navigate(Mode::Programmer));
    f(a, "programmer");
    for o in [Overlay::Bitwise, Overlay::Shift] {
        a.apply(&Action::Open(o));
        f(a, "prog-flyout");
        a.apply(&Action::CloseOverlay);
    }
    a.apply(&Action::BitPad(true));
    f(a, "bits");
    a.apply(&Action::Navigate(Mode::Date));
    f(a, "date");
    a.apply(&Action::Open(Overlay::Calendar(0)));
    f(a, "calendar");
    a.apply(&Action::CloseOverlay);
    a.apply(&Action::ComboPick(ComboId::DateMode, 1));
    f(a, "date-add");
    for i in 0..a.categories.len() {
        a.apply(&Action::Navigate(Mode::Converter(i as u8)));
        keys(a, &[Key::Digit(3)]);
        f(a, "converter");
        a.apply(&Action::Open(Overlay::Combo(ComboId::ConvFrom)));
        f(a, "converter-combo");
        a.apply(&Action::CloseOverlay);
    }
    a.apply(&Action::Navigate(Mode::Settings));
    f(a, "settings");
    a.apply(&Action::Open(Overlay::Combo(ComboId::Language)));
    f(a, "language");
    a.apply(&Action::CloseOverlay);
}

/// Every piece of English UI text that reaches the screen has a translation
/// (checked by rendering all views in German).
#[test]
fn rendered_views_are_translated() {
    let Ok(mut g) = Gfx::new() else { return };
    let mut a = app();
    a.lang_pref = Some(crate::i18n::Lang::DeDE);
    a.apply_language();
    crate::i18n::tests::take_misses();
    let input = Input::default();
    tour(&mut a, |a, _| {
        g.resize(400, 1400);
        let mut ui = Ui::new(&mut g, Theme::new(true, Color::rgb(0x0078D4)), 1.0, &input);
        view::build(a, &mut ui);
    });
    let missing: Vec<String> = crate::i18n::tests::take_misses()
        .into_iter()
        .filter(|s| crate::i18n::tests::has_english_leftover(crate::i18n::Lang::DeDE, s))
        // File names, command-line flags, language names and key captions stay as-is.
        .filter(|s| !s.contains("rcalc") && !s.chars().all(|c| !c.is_lowercase()))
        .filter(|s| !crate::i18n::Lang::ALL.iter().any(|l| l.native_name() == s))
        .collect();
    crate::i18n::set_lang(crate::i18n::Lang::EnUS);
    assert!(
        missing.is_empty(),
        "untranslated text on screen:
{}",
        missing.join(
            "
"
        )
    );
}

#[test]
#[ignore]
fn localized_screenshots() {
    let mut g = Gfx::new().expect("fonts");
    for lang in [
        crate::i18n::Lang::DeDE,
        crate::i18n::Lang::JaJP,
        crate::i18n::Lang::ZhCN,
        crate::i18n::Lang::FrFR,
        crate::i18n::Lang::PtPT,
    ] {
        let mut a = app();
        a.lang_pref = Some(lang);
        a.apply_language();
        g.clear_cache();
        let tag = lang.tag();
        let mut n = 0;
        tour(&mut a, |a, step| {
            if ["standard", "scientific", "date-add", "settings", "nav", "calendar", "history"].contains(&step) {
                n += 1;
                shot(&mut g, a, &format!("{tag}_{n:02}_{step}"), 360, 600, 1.0, n % 2 == 0);
            }
        });
    }
    crate::i18n::set_lang(crate::i18n::Lang::EnUS);
}

#[test]
#[ignore]
fn screenshots() {
    let mut g = Gfx::new().expect("fonts");
    let mut a = app();
    keys(
        &mut a,
        &[
            Key::Digit(1),
            Key::Digit(2),
            Key::Digit(3),
            Key::Digit(4),
            Key::Bin(BinOp::Add),
            Key::Digit(5),
            Key::Equals,
        ],
    );
    keys(&mut a, &[Key::Digit(9), Key::Bin(BinOp::Mul), Key::Digit(7)]);
    shot(&mut g, &a, "standard_dark", 340, 560, 1.0, true);
    shot(&mut g, &a, "standard_light", 340, 560, 1.0, false);
    shot(&mut g, &a, "standard_wide", 760, 560, 1.0, true);
    shot(&mut g, &a, "standard_hidpi", 340, 560, 2.0, false);
    a.apply(&Action::Open(Overlay::Panel(PanelTab::History)));
    shot(&mut g, &a, "standard_history_flyout", 340, 560, 1.0, true);
    a.apply(&Action::CloseOverlay);
    a.apply(&Action::OpenNav);
    shot(&mut g, &a, "nav", 340, 560, 1.0, true);
    a.apply(&Action::Navigate(Mode::Scientific));
    keys(&mut a, &[Key::LParen, Key::Digit(2), Key::Bin(BinOp::Add), Key::Digit(3)]);
    shot(&mut g, &a, "scientific", 400, 640, 1.0, true);
    a.apply(&Action::Toggle2nd);
    a.apply(&Action::Open(Overlay::Trig));
    shot(&mut g, &a, "scientific_trig", 400, 640, 1.0, false);
    a.apply(&Action::Navigate(Mode::Programmer));
    keys(&mut a, &[Key::Digit(2), Key::Digit(5), Key::Digit(5)]);
    shot(&mut g, &a, "programmer", 400, 680, 1.0, true);
    a.apply(&Action::BitPad(true));
    shot(&mut g, &a, "programmer_bits", 400, 680, 1.0, true);
    a.apply(&Action::Navigate(Mode::Date));
    shot(&mut g, &a, "date_diff", 340, 560, 1.0, true);
    a.apply(&Action::Open(Overlay::Calendar(0)));
    shot(&mut g, &a, "date_calendar", 340, 560, 1.0, false);
    a.apply(&Action::CloseOverlay);
    a.apply(&Action::ComboPick(ComboId::DateMode, 1));
    shot(&mut g, &a, "date_add", 340, 560, 1.0, true);
    a.apply(&Action::Navigate(Mode::Converter(2)));
    keys(&mut a, &[Key::Digit(1), Key::Digit(0), Key::Digit(0)]);
    shot(&mut g, &a, "converter_length", 340, 560, 1.0, true);
    a.apply(&Action::Open(Overlay::Combo(ComboId::ConvTo)));
    shot(&mut g, &a, "converter_combo", 340, 560, 1.0, false);
    a.apply(&Action::Navigate(Mode::Converter(0)));
    shot(&mut g, &a, "converter_currency", 340, 560, 1.0, false);
    a.apply(&Action::Navigate(Mode::Settings));
    shot(&mut g, &a, "settings", 340, 560, 1.0, true);
}
