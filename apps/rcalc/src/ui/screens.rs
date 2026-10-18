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
    App::new(Location {
        dir: None,
        portable: false,
        error: None,
    })
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
