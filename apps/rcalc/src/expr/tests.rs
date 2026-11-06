use super::eval::{eval, Ctx};
use super::{parse_str, LexMode};
use crate::num::func::Angle;
use crate::num::CalcError;

fn calc(s: &str) -> Result<String, CalcError> {
    let n = parse_str(s, LexMode::Decimal)?;
    Ok(eval(&n, &Ctx::default())?.format(32, false))
}

fn ok(s: &str) -> String {
    calc(s).unwrap_or_else(|e| panic!("{s}: {e}"))
}

#[test]
fn precedence_and_associativity() {
    assert_eq!(ok("2+3*4"), "14");
    assert_eq!(ok("(2+3)*4"), "20");
    assert_eq!(ok("10-4-3"), "3");
    assert_eq!(ok("100/10/5"), "2");
    assert_eq!(ok("2^3^2"), "512");
    assert_eq!(ok("2*3^2"), "18");
    assert_eq!(ok("7 mod 4 * 2"), "6");
}

#[test]
fn unary_operators() {
    assert_eq!(ok("-2^2"), "-4");
    assert_eq!(ok("(-2)^2"), "4");
    assert_eq!(ok("3--2"), "5");
    assert_eq!(ok("-(-3)"), "3");
    assert_eq!(ok("+5"), "5");
    assert_eq!(ok("2*-3"), "-6");
    assert_eq!(ok("5!"), "120");
    assert_eq!(ok("3²+4²"), "25");
    assert_eq!(ok("√16"), "4");
    assert_eq!(ok("√(9+16)"), "5");
}

#[test]
fn decimals_scientific_notation_and_grouping() {
    assert_eq!(ok("0.1+0.2"), "0.3");
    assert_eq!(ok(".5*4"), "2");
    assert_eq!(ok("1.5e3"), "1500");
    assert_eq!(ok("2E-3*1000"), "2");
    assert_eq!(ok("1,234,567+1"), "1234568");
    assert_eq!(ok("1e+2"), "100");
}

#[test]
fn percentages() {
    assert_eq!(ok("50%"), "0.5");
    assert_eq!(ok("200+10%"), "220");
    assert_eq!(ok("200-25%"), "150");
    assert_eq!(ok("200*10%"), "20");
}

#[test]
fn functions_constants_and_implicit_multiplication() {
    assert_eq!(ok("sin 30"), "0.5");
    assert_eq!(ok("sin(30)+cos(60)"), "1");
    assert_eq!(ok("asin(1)"), "90");
    assert_eq!(ok("log(1000)"), "3");
    assert_eq!(ok("ln(e)"), "1");
    assert_eq!(ok("2π"), "6.283185307179586476925286766559");
    assert_eq!(ok("2(3+4)"), "14");
    assert_eq!(ok("(1+1)(2+2)"), "8");
    assert_eq!(ok("abs(-5)+floor(2.7)+ceil(2.1)"), "10");
    assert_eq!(ok("27 yroot 3"), "3");
    assert_eq!(ok("sinh(0)"), "0");
    let n = parse_str("sin(pi/2)", LexMode::Decimal).unwrap();
    assert_eq!(eval(&n, &Ctx { angle: Angle::Rad }).unwrap().format(32, false), "1");
}

#[test]
fn unbalanced_and_invalid_input() {
    assert_eq!(ok("(2+3"), "5"); // trailing parentheses close implicitly
    assert_eq!(calc("2+3)"), Err(CalcError::InvalidInput));
    assert_eq!(calc("2+"), Err(CalcError::InvalidInput));
    assert_eq!(calc("*3"), Err(CalcError::InvalidInput));
    assert_eq!(calc(""), Err(CalcError::InvalidInput));
    assert_eq!(calc("2 $ 3"), Err(CalcError::InvalidInput));
    assert_eq!(calc("foo(2)"), Err(CalcError::InvalidInput));
    assert_eq!(calc("1/0"), Err(CalcError::DivideByZero));
    assert_eq!(calc("tan(90)"), Err(CalcError::InvalidInput));
    assert_eq!(calc("(-1)^0.5"), Err(CalcError::InvalidInput));
}

#[test]
fn rendering_for_history() {
    let n = parse_str("2*(3+4)-sqrt(9)", LexMode::Decimal).unwrap();
    assert_eq!(n.to_text(&|d| d.format(16, false)), "2 × (3 + 4) − √(9)");
}

#[test]
fn programmer_lexing() {
    use crate::calc::programmer::{eval as peval, ProgCtx, WordSize};
    let p = |s: &str, radix| {
        let n = parse_str(s, LexMode::Programmer(radix)).unwrap();
        peval(&n, &ProgCtx::default()).unwrap().value
    };
    assert_eq!(p("FF + 1", 16), 256);
    assert_eq!(p("ff & 0F", 16), 15);
    assert_eq!(p("1111 0000", 2), 240);
    assert_eq!(p("0x10 + 0b11", 10), 19);
    assert_eq!(p("1 << 4 | 1", 10), 17);
    assert_eq!(p("6 ^ 3", 10), 5);
    assert_eq!(p("~0", 10), -1);
    assert_eq!(p("7 % 3", 10), 1);
    assert_eq!(p("bad and f", 16), 0xBAD & 0xF);
    let ctx = ProgCtx {
        word: WordSize::Byte,
        ..ProgCtx::default()
    };
    let r = peval(&parse_str("100 + 100", LexMode::Programmer(10)).unwrap(), &ctx).unwrap();
    assert_eq!((r.value, r.overflow), (-56, true));
    assert!(parse_str("1.5", LexMode::Programmer(10)).is_err());
}
