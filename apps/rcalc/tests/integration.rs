//! End-to-end tests through the public library API: keypad engines, the
//! application state machine (keyboard shortcuts, memory, history,
//! converters, persistence) and text paste.

use rcalc::app::{Action, App, ComboId, KeyIn, MemOp, Mode, Mods, Overlay};
use rcalc::calc::programmer::{Radix, WordSize};
use rcalc::calc::{Engine, Key, Kind};
use rcalc::num::Dec;
use rcalc::storage::{Location, Store};

fn app() -> App {
    App::new(Location { error: None, ..Location::nowhere(rcalc::storage::PLACE) })
}

/// Types text as if on the keyboard (`\n` = Enter, `\x1b` = Esc, `\x08` = Backspace).
fn type_keys(a: &mut App, s: &str) {
    for c in s.chars() {
        let k = match c {
            '\n' => KeyIn::Enter,
            '\x1b' => KeyIn::Escape,
            '\x08' => KeyIn::Backspace,
            c => KeyIn::Char(c),
        };
        if let Some(act) = a.shortcut(&k, Mods::default()) {
            a.apply(&act);
        }
    }
}

fn shown(a: &App) -> String {
    a.engine().unwrap().display_text()
}

#[test]
fn standard_session_with_keyboard() {
    let mut a = app();
    type_keys(&mut a, "1234.5+0.5\n");
    assert_eq!(shown(&a), "1,235");
    type_keys(&mut a, "\x1b");
    assert_eq!(shown(&a), "0");
    type_keys(&mut a, "99\x08\x087*6=");
    assert_eq!(shown(&a), "42");
    type_keys(&mut a, "r");
    assert_eq!(shown(&a), "0.0238095238095238");
    assert_eq!(a.history.len(), 2);
}

#[test]
fn scientific_session() {
    let mut a = app();
    a.apply(&Action::Navigate(Mode::Scientific));
    type_keys(&mut a, "(1+2)*(3+4)\n");
    assert_eq!(shown(&a), "21");
    type_keys(&mut a, "\x1b2y10=");
    assert_eq!(shown(&a), "1,024");
    type_keys(&mut a, "\x1b5!");
    assert_eq!(shown(&a), "120");
    a.apply(&Action::CycleAngle); // radians
    type_keys(&mut a, "\x1bp/2=s");
    assert_eq!(shown(&a), "1");
}

#[test]
fn programmer_session_with_overflow_notice() {
    let mut a = app();
    a.apply(&Action::Navigate(Mode::Programmer));
    a.apply(&Action::Radix(Radix::Hex));
    type_keys(&mut a, "7fffffffffffffff+1=");
    assert!(a.prog.overflowed);
    a.apply(&Action::Radix(Radix::Dec));
    assert_eq!(shown(&a), "-9,223,372,036,854,775,808");
    a.apply(&Action::CycleWord);
    assert_eq!(a.prog.word, WordSize::Dword);
    assert_eq!(shown(&a), "0");
}

#[test]
fn memory_and_history_panel_actions() {
    let mut a = app();
    type_keys(&mut a, "10");
    a.apply(&Action::Mem(MemOp::Store));
    type_keys(&mut a, "5");
    a.apply(&Action::Mem(MemOp::Store));
    assert_eq!(a.memory, vec![Dec::from_i64(5), Dec::from_i64(10)]);
    a.apply(&Action::MemItem(1, MemOp::Add));
    assert_eq!(a.memory[1], Dec::from_i64(15));
    a.apply(&Action::MemItem(1, MemOp::Recall));
    assert_eq!(shown(&a), "15");
    type_keys(&mut a, "*2=");
    a.apply(&Action::Open(Overlay::Panel(rcalc::app::PanelTab::History)));
    a.apply(&Action::HistoryItem(0));
    assert_eq!(a.overlay, Overlay::None);
    assert_eq!(shown(&a), "30");
    a.apply(&Action::ClearHistory);
    a.apply(&Action::ClearMemory);
    assert!(a.history.is_empty() && a.memory.is_empty());
}

#[test]
fn converters_and_combos() {
    let mut a = app();
    let temp = a.categories.iter().position(|c| c.name == "Temperature").unwrap() as u8;
    a.apply(&Action::Navigate(Mode::Converter(temp)));
    type_keys(&mut a, "37");
    assert_eq!(a.conv_texts().1, "98.6");
    a.apply(&Action::ComboPick(ComboId::ConvTo, 2)); // Kelvin
    assert_eq!(a.conv_texts().1, "310.15");
    a.apply(&Action::Key(Key::Negate));
    assert_eq!(a.conv_texts(), ("-37".to_string(), "236.15".to_string()));
}

#[test]
fn persistence_survives_round_trip() {
    let mut a = app();
    type_keys(&mut a, "6*7=");
    a.apply(&Action::Navigate(Mode::Date));
    a.apply(&Action::ComboPick(ComboId::DateMode, 1));
    let text = a.to_store().serialize("rcalc");
    let mut b = app();
    b.load(&Store::parse(&text));
    assert_eq!(b.mode, Mode::Date);
    assert!(b.date.add_mode);
    assert_eq!(b.history[0].result, "42");
    // Corrupt or unknown content is ignored rather than failing.
    let mut c = app();
    c.load(&Store::parse("mode=Nonsense\nhistory=broken\nmemory=xyz\nwidth=abc\n"));
    assert_eq!(c.mode, Mode::Standard);
    assert!(c.history.is_empty() && c.memory.is_empty());
}

#[test]
fn engine_paste_expressions() {
    let mut e = Engine::new(Kind::Scientific);
    assert!(e.paste("sqrt(2)^2"));
    assert_eq!(e.display_text(), "2");
    assert!(e.paste("1e-5 * 3"));
    assert_eq!(e.display_text(), "0.00003");
    let mut p = Engine::new(Kind::Programmer);
    p.set_radix(Radix::Bin);
    assert!(p.paste("1010 1010"));
    p.set_radix(Radix::Dec);
    assert_eq!(p.display_text(), "170");
    p.set_word(WordSize::Byte);
    assert_eq!(p.display_text(), "-86");
}
