use super::*;
use crate::num::func::Trig;

/// Presses keys described by a compact script, e.g. `"12 + 3 ="`.
/// Multi-character words name special keys.
fn run_on(e: &mut Engine, script: &str) {
    for word in script.split_whitespace() {
        let keys: Vec<Key> = match word {
            "+" => vec![Key::Bin(BinOp::Add)],
            "-" => vec![Key::Bin(BinOp::Sub)],
            "*" => vec![Key::Bin(BinOp::Mul)],
            "/" => vec![Key::Bin(BinOp::Div)],
            "^" => vec![Key::Bin(BinOp::Pow)],
            "mod" => vec![Key::Bin(BinOp::Mod)],
            "yroot" => vec![Key::Bin(BinOp::Root)],
            "and" => vec![Key::Bin(BinOp::And)],
            "or" => vec![Key::Bin(BinOp::Or)],
            "xor" => vec![Key::Bin(BinOp::Xor)],
            "<<" => vec![Key::Bin(BinOp::Shl)],
            ">>" => vec![Key::Bin(BinOp::Shr)],
            "=" => vec![Key::Equals],
            "%" => vec![Key::Percent],
            "(" => vec![Key::LParen],
            ")" => vec![Key::RParen],
            "C" => vec![Key::Clear],
            "CE" => vec![Key::ClearEntry],
            "BS" => vec![Key::Back],
            "neg" => vec![Key::Negate],
            "sqr" => vec![Key::Unary(Unary::Square)],
            "sqrt" => vec![Key::Unary(Unary::Sqrt)],
            "inv" => vec![Key::Unary(Unary::Recip)],
            "fact" => vec![Key::Unary(Unary::Fact)],
            "not" => vec![Key::Unary(Unary::Not)],
            "sin" => vec![Key::Unary(Unary::Trig {
                f: Trig::Sin,
                inv: false,
                hyp: false,
            })],
            "pi" => vec![Key::Const(Constant::Pi)],
            "exp" => vec![Key::Exp],
            "." => vec![Key::Point],
            num => num
                .chars()
                .map(|c| {
                    if c == '.' {
                        Key::Point
                    } else {
                        Key::Digit(c.to_digit(16).unwrap() as u8)
                    }
                })
                .collect(),
        };
        for k in keys {
            e.press(k);
        }
    }
}

fn run(kind: Kind, script: &str) -> Engine {
    let mut e = Engine::new(kind);
    run_on(&mut e, script);
    e
}

fn std_result(script: &str) -> String {
    run(Kind::Standard, script).display_text()
}

fn sci_result(script: &str) -> String {
    run(Kind::Scientific, script).display_text()
}

#[test]
fn standard_is_immediate_execution() {
    assert_eq!(std_result("2 + 3 * 4 ="), "20");
    let e = run(Kind::Standard, "2 + 3 *");
    assert_eq!(e.display_text(), "5");
    assert_eq!(e.expression_text(), "5 ×");
}

#[test]
fn scientific_uses_precedence() {
    assert_eq!(sci_result("2 + 3 * 4 ="), "14");
    let e = run(Kind::Scientific, "2 * 3 +");
    assert_eq!(e.display_text(), "6");
    let e = run(Kind::Scientific, "2 + 3 * 4 +");
    assert_eq!(e.display_text(), "14");
    assert_eq!(sci_result("2 ^ 3 ^ 2 ="), "512");
    assert_eq!(sci_result("( 2 + 3 ) * 4 ="), "20");
    assert_eq!(sci_result("2 * ( 3 + 4"), "4"); // still typing
    assert_eq!(sci_result("2 * ( 3 + 4 ="), "14"); // auto-closed
}

#[test]
fn decimal_precision() {
    assert_eq!(std_result("0.1 + 0.2 ="), "0.3");
    assert_eq!(std_result("1 / 3 * 3 ="), "1");
    assert_eq!(std_result("2 sqrt sqr - 2 ="), "0");
    assert_eq!(std_result("10 / 4 ="), "2.5");
    assert_eq!(std_result("1 / 3 ="), "0.3333333333333333");
    assert_eq!(sci_result("1 / 3 ="), "0.33333333333333333333333333333333");
}

#[test]
fn repeated_equals_and_operator_replacement() {
    assert_eq!(std_result("5 + 3 = = ="), "14");
    assert_eq!(std_result("5 + * 2 ="), "10");
    assert_eq!(std_result("5 + ="), "10");
    assert_eq!(std_result("2 * 3 = 7 ="), "21");
}

#[test]
fn errors() {
    assert_eq!(std_result("5 / 0 ="), "Cannot divide by zero");
    assert_eq!(std_result("0 / 0 ="), "Result is undefined");
    assert_eq!(std_result("0 inv"), "Cannot divide by zero");
    assert_eq!(std_result("9 neg sqrt"), "Invalid input");
    // A digit clears the error.
    assert_eq!(std_result("5 / 0 = 7"), "7");
    assert_eq!(sci_result("10000 fact"), "Overflow");
}

#[test]
fn percent() {
    assert_eq!(std_result("200 + 10 % ="), "220");
    assert_eq!(std_result("200 - 10 % ="), "180");
    assert_eq!(std_result("200 * 10 % ="), "20");
    assert_eq!(std_result("50 %"), "0");
}

#[test]
fn entry_editing() {
    assert_eq!(std_result("123 BS"), "12");
    assert_eq!(std_result("1 BS BS"), "0");
    assert_eq!(std_result("1234567"), "1,234,567");
    assert_eq!(std_result("1 . 5 neg"), "-1.5");
    assert_eq!(std_result("1 . ."), "1.");
    assert_eq!(std_result("12345678901234567890"), "1,234,567,890,123,456"); // 16 digit limit
    assert_eq!(std_result("5 + 3 CE 4 ="), "9");
    assert_eq!(sci_result("1 . 5 exp 3 ="), "1,500");
    assert_eq!(sci_result("2 exp 3 neg ="), "0.002");
}

#[test]
fn unary_functions_and_expression_text() {
    let e = run(Kind::Standard, "5 + 9 sqrt");
    assert_eq!(e.display_text(), "3");
    assert_eq!(e.expression_text(), "5 + √(9)");
    assert_eq!(std_result("5 + 9 sqrt ="), "8");
    assert_eq!(sci_result("30 sin"), "0.5");
    assert_eq!(sci_result("pi * 2 ="), "6.283185307179586476925286766559");
}

#[test]
fn history_entries() {
    let mut e = Engine::new(Kind::Standard);
    run_on(&mut e, "2 + 3");
    let h = e.press(Key::Equals).expect("history");
    assert_eq!(h.expression, "2 + 3 =");
    assert_eq!(h.result, "5");
    // A bare number produces no history.
    let mut e = Engine::new(Kind::Standard);
    run_on(&mut e, "7");
    assert!(e.press(Key::Equals).is_none());
}

#[test]
fn large_numbers_switch_to_scientific() {
    assert_eq!(std_result("9999999999999999 * 10 ="), "9.999999999999999e+16");
    assert_eq!(sci_result("2 ^ 200 ="), "1.6069380442589902755419620923412e+60");
}

#[test]
fn paste() {
    let mut e = Engine::new(Kind::Scientific);
    assert!(e.paste("2+3*4"));
    assert_eq!(e.display_text(), "14");
    assert!(e.paste("1,234.5"));
    assert_eq!(e.display_text(), "1,234.5");
    assert!(!e.paste("hello"));
}

// ---------------------------------------------------------------- programmer

fn prog(script: &str) -> Engine {
    run(Kind::Programmer, script)
}

#[test]
fn programmer_basic() {
    assert_eq!(prog("7 / 2 =").display_text(), "3");
    assert_eq!(prog("7 neg / 2 =").display_text(), "-3");
    assert_eq!(prog("7 mod 3 =").display_text(), "1");
    assert_eq!(prog("12 and 10 =").display_text(), "8");
    assert_eq!(prog("12 or 3 =").display_text(), "15");
    assert_eq!(prog("12 xor 10 =").display_text(), "6");
    assert_eq!(prog("1 << 10 =").display_text(), "1,024");
    assert_eq!(prog("0 not").display_text(), "-1");
    // precedence: & binds tighter than |
    assert_eq!(prog("1 or 6 and 3 =").display_text(), "3");
    assert_eq!(prog("5 / 0 =").display_text(), "Cannot divide by zero");
    // decimal point is ignored
    assert_eq!(prog("1 . 5").display_text(), "15");
}

#[test]
fn programmer_overflow_is_reported() {
    let e = prog("9223372036854775807 + 1 =");
    assert_eq!(e.display_text(), "-9,223,372,036,854,775,808");
    assert!(e.overflowed);
    let e = prog("2 + 2 =");
    assert!(!e.overflowed);
    // Typing beyond the word size is refused (i64::MAX has 19 digits).
    assert_eq!(prog("99999999999999999999").display_text(), "999,999,999,999,999,999");
}

#[test]
fn programmer_radix_and_word() {
    let mut e = Engine::new(Kind::Programmer);
    e.set_radix(Radix::Hex);
    run_on(&mut e, "F F");
    assert_eq!(e.int_value(), 255);
    e.set_radix(Radix::Bin);
    assert_eq!(e.display_text(), "1111 1111");
    e.set_word(WordSize::Byte);
    assert_eq!(e.int_value(), -1);
    e.set_radix(Radix::Dec);
    assert_eq!(e.display_text(), "-1");
    e.set_radix(Radix::Hex);
    assert_eq!(e.display_text(), "FF");
    // In BYTE, 127 + 1 wraps to -128 with an overflow notice.
    let mut e = Engine::new(Kind::Programmer);
    e.set_word(WordSize::Byte);
    run_on(&mut e, "127 + 1 =");
    assert_eq!(e.display_text(), "-128");
    assert!(e.overflowed);
    // Hex digits beyond the word are refused: BYTE allows two.
    let mut e = Engine::new(Kind::Programmer);
    e.set_word(WordSize::Byte);
    e.set_radix(Radix::Hex);
    run_on(&mut e, "F F F");
    assert_eq!(e.display_text(), "FF");
}

#[test]
fn programmer_shifts_and_rotate() {
    let mut e = Engine::new(Kind::Programmer);
    e.set_word(WordSize::Byte);
    e.set_value(Dec::from_i64(-128));
    run_on(&mut e, ">> 1 =");
    assert_eq!(e.int_value(), -64); // arithmetic
    let mut e = Engine::new(Kind::Programmer);
    e.set_word(WordSize::Byte);
    e.shift = ShiftMode::Logical;
    e.set_value(Dec::from_i64(-128));
    run_on(&mut e, ">> 1 =");
    assert_eq!(e.int_value(), 64);
    let mut e = Engine::new(Kind::Programmer);
    e.set_word(WordSize::Byte);
    e.set_value(Dec::from_i64(-128));
    e.press(Key::Unary(Unary::Rol));
    assert_eq!(e.int_value(), 1);
    e.press(Key::Unary(Unary::Ror));
    assert_eq!(e.int_value(), -128);
}

#[test]
fn programmer_bit_toggle() {
    let mut e = Engine::new(Kind::Programmer);
    e.toggle_bit(4);
    assert_eq!(e.int_value(), 16);
    e.toggle_bit(63);
    assert_eq!(e.int_value(), 16 + i64::MIN);
    e.toggle_bit(4);
    e.toggle_bit(63);
    assert_eq!(e.int_value(), 0);
}
