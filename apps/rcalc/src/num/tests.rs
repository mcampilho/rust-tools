use super::func::{self, Angle, Trig};
use super::{CalcError, Dec};

fn d(s: &str) -> Dec {
    s.parse().unwrap()
}

fn f32d(x: &Dec) -> String {
    x.format(32, false)
}

#[test]
fn parse_and_canonical_form() {
    assert_eq!(d("1.50"), d("1.5"));
    assert_eq!(d("100"), d("1e2"));
    assert_eq!(d("-0"), Dec::zero());
    assert_eq!(d(".5"), d("0.5"));
    assert_eq!(d("1."), Dec::one());
    assert_eq!(d("2.5e-3").to_exact_string(), "0.0025");
    assert!("".parse::<Dec>().is_err());
    assert!("1.2.3".parse::<Dec>().is_err());
    assert!("1e".parse::<Dec>().is_err());
    assert!("abc".parse::<Dec>().is_err());
}

#[test]
fn exact_decimal_arithmetic() {
    assert_eq!(d("0.1").add(&d("0.2")), d("0.3"));
    assert_eq!(d("1.1").mul(&d("1.1")), d("1.21"));
    assert_eq!(d("1e20").add(&Dec::one()).sub(&d("1e20")), Dec::one());
    assert_eq!(d("10").div(&d("4")).unwrap(), d("2.5"));
    assert_eq!(Dec::one().div(&Dec::zero()), Err(CalcError::DivideByZero));
    assert_eq!(Dec::zero().div(&Dec::zero()), Err(CalcError::Undefined));
}

#[test]
fn rounding_is_half_even_and_correct() {
    assert_eq!(d("2.5").round_sig(1), d("2"));
    assert_eq!(d("3.5").round_sig(1), d("4"));
    assert_eq!(d("2.51").round_sig(1), d("3"));
    assert_eq!(d("0.125").round_places(2), d("0.13"));
    assert_eq!(d("-0.125").round_places(2), d("-0.13"));
    let third = Dec::one().div(&d("3")).unwrap();
    assert_eq!(f32d(&third), "0.33333333333333333333333333333333");
    assert_eq!(f32d(&d("2").div(&d("3")).unwrap()), "0.66666666666666666666666666666667");
}

#[test]
fn ordering_and_integer_ops() {
    assert!(d("-2") < d("-1"));
    assert!(d("0.001") > Dec::zero());
    assert!(d("1e10") > d("9999999999.9"));
    assert_eq!(d("-2.5").floor(), d("-3"));
    assert_eq!(d("-2.5").ceil(), d("-2"));
    assert_eq!(d("2.5").trunc(), d("2"));
    assert_eq!(d("2.5").round_int(), d("3"));
    assert_eq!(d("-7").rem_floor(&d("3")).unwrap(), d("2"));
    assert_eq!(d("7.5").rem_floor(&d("2")).unwrap(), d("1.5"));
    assert_eq!(d("123456789012345678901234567890").to_i64(), None);
    assert_eq!(d("-42").to_i64(), Some(-42));
}

#[test]
fn formatting() {
    assert_eq!(d("1234.5").format(16, false), "1234.5");
    assert_eq!(d("1e16").format(16, false), "1e+16");
    assert_eq!(d("123e14").format(16, false), "1.23e+16");
    assert_eq!(d("0.0001").format(16, false), "0.0001");
    assert_eq!(d("1.5e-20").format(16, false), "1.5e-20");
    assert_eq!(d("5").format(16, true), "5.e+0");
    assert_eq!(d("-12.75").format(16, true), "-1.275e+1");
    assert_eq!(d("0.1").format(16, false), "0.1");
    // Leading zeros use up display width instead of switching to e-notation.
    assert_eq!(Dec::one().div(&d("42")).unwrap().format(16, false), "0.0238095238095238");
    assert_eq!(d("1e-8").format(16, false), "0.00000001");
    assert_eq!(d("0.09999999999999999").format(16, false), "0.1");
}

#[test]
fn range_limits() {
    assert!(d("1e9999").check().is_ok());
    assert_eq!(d("1e10000").check(), Err(CalcError::Overflow));
    assert_eq!(func::pow(&d("10"), &d("10000")), Err(CalcError::Overflow));
    assert_eq!(func::exp(&d("30000")), Err(CalcError::Overflow));
    assert!(func::factorial(&d("3248")).is_ok());
    assert_eq!(func::factorial(&d("3249")), Err(CalcError::Overflow));
}

#[test]
fn roots_and_powers() {
    assert_eq!(func::sqrt(&d("144")).unwrap(), d("12"));
    assert_eq!(func::sqrt(&d("0.0625")).unwrap(), d("0.25"));
    assert_eq!(f32d(&func::sqrt(&d("2")).unwrap()), "1.4142135623730950488016887242097");
    assert_eq!(func::sqrt(&d("-1")), Err(CalcError::InvalidInput));
    assert_eq!(func::root(&d("27"), &d("3")).unwrap(), d("3"));
    assert_eq!(func::root(&d("-32"), &d("5")).unwrap(), d("-2"));
    assert_eq!(func::root(&d("-16"), &d("4")), Err(CalcError::InvalidInput));
    assert_eq!(func::pow(&d("2"), &d("10")).unwrap(), d("1024"));
    assert_eq!(func::pow(&d("2"), &d("-2")).unwrap(), d("0.25"));
    assert_eq!(func::pow(&d("-2"), &d("3")).unwrap(), d("-8"));
    assert_eq!(func::pow(&d("0"), &d("0")).unwrap(), Dec::one());
    assert_eq!(func::pow(&d("0"), &d("-1")), Err(CalcError::DivideByZero));
    assert_eq!(func::pow(&d("-8"), &d("0.5")), Err(CalcError::InvalidInput));
    assert_eq!(f32d(&func::pow(&d("2"), &d("0.5")).unwrap()), "1.4142135623730950488016887242097");
    assert_eq!(
        f32d(&func::pow(&d("1.0000001"), &d("1e7")).unwrap()),
        "2.7182816925449662711985502257778"
    );
}

#[test]
fn logs_and_exponentials() {
    assert_eq!(func::log10(&d("1000")).unwrap(), d("3"));
    assert_eq!(func::log10(&d("0.01")).unwrap(), d("-2"));
    assert_eq!(f32d(&func::log10(&d("2")).unwrap()), "0.30102999566398119521373889472449");
    assert_eq!(f32d(&func::ln(&func::euler()).unwrap()), "1");
    assert_eq!(f32d(&func::exp(&func::ln(&d("12345.678")).unwrap()).unwrap()), "12345.678");
    assert_eq!(func::ln(&d("0")), Err(CalcError::InvalidInput));
    assert_eq!(f32d(&func::ln(&d("0.999999")).unwrap()), "-0.00000100000050000033333358333353");
    assert_eq!(f32d(&func::log_base(&d("8"), &d("2")).unwrap()), "3");
    assert_eq!(func::log_base(&d("8"), &d("1")), Err(CalcError::InvalidInput));
}

#[test]
fn trigonometry_degrees_radians_gradians() {
    let t = |f, x: &str, a| f32d(&func::trig(f, &d(x), a).unwrap());
    assert_eq!(t(Trig::Sin, "30", Angle::Deg), "0.5");
    assert_eq!(t(Trig::Sin, "180", Angle::Deg), "0");
    assert_eq!(t(Trig::Cos, "60", Angle::Deg), "0.5");
    assert_eq!(t(Trig::Cos, "-90", Angle::Deg), "0");
    assert_eq!(t(Trig::Tan, "45", Angle::Deg), "1");
    assert_eq!(t(Trig::Sin, "100", Angle::Grad), "1");
    assert_eq!(t(Trig::Sin, "390", Angle::Deg), "0.5");
    assert_eq!(t(Trig::Sin, "1", Angle::Rad), "0.8414709848078965066525023216303");
    assert_eq!(t(Trig::Cos, "1", Angle::Rad), "0.54030230586813971740093660744298");
    assert_eq!(func::trig(Trig::Tan, &d("90"), Angle::Deg), Err(CalcError::InvalidInput));
    assert_eq!(func::trig(Trig::Cot, &d("0"), Angle::Deg), Err(CalcError::InvalidInput));
    let pi = func::pi();
    assert_eq!(f32d(&func::trig(Trig::Sin, &pi, Angle::Rad).unwrap()), "0");
    assert_eq!(f32d(&func::trig(Trig::Sec, &d("60"), Angle::Deg).unwrap()), "2");
}

#[test]
fn inverse_and_hyperbolic() {
    let it = |f, x: &str, a| f32d(&func::inverse_trig(f, &d(x), a).unwrap());
    assert_eq!(it(Trig::Sin, "0.5", Angle::Deg), "30");
    assert_eq!(it(Trig::Sin, "1", Angle::Deg), "90");
    assert_eq!(it(Trig::Cos, "-1", Angle::Deg), "180");
    assert_eq!(it(Trig::Tan, "1", Angle::Rad), "0.78539816339744830961566084581988");
    assert_eq!(it(Trig::Sin, "1", Angle::Grad), "100");
    assert_eq!(func::inverse_trig(Trig::Sin, &d("1.5"), Angle::Deg), Err(CalcError::InvalidInput));
    let h = |f, x: &str| f32d(&func::hyperbolic(f, &d(x)).unwrap());
    assert_eq!(h(Trig::Sin, "1"), "1.1752011936438014568823818505956");
    assert_eq!(h(Trig::Cos, "0"), "1");
    assert_eq!(h(Trig::Tan, "1000"), "1");
    let ih = |f, x: &str| f32d(&func::inverse_hyperbolic(f, &d(x)).unwrap());
    assert_eq!(ih(Trig::Sin, "1"), "0.88137358701954302523260932497979");
    assert_eq!(ih(Trig::Cos, "1"), "0");
    assert_eq!(ih(Trig::Tan, "0.5"), "0.54930614433405484569762261846126");
    assert_eq!(func::inverse_hyperbolic(Trig::Tan, &d("1")), Err(CalcError::InvalidInput));
}

#[test]
fn factorial_gamma_and_dms() {
    assert_eq!(func::factorial(&d("0")).unwrap(), Dec::one());
    assert_eq!(func::factorial(&d("20")).unwrap(), d("2432902008176640000"));
    assert_eq!(func::factorial(&d("-1")), Err(CalcError::InvalidInput));
    // Non-integers use the gamma function (about 15 significant digits).
    assert_eq!(func::factorial(&d("0.5")).unwrap().format(10, false), "0.8862269255");
    assert_eq!(func::to_dms(&d("1.5")).unwrap(), d("1.3"));
    assert_eq!(func::from_dms(&d("1.3")).unwrap(), d("1.5"));
    assert_eq!(func::to_dms(&d("-10.2575")).unwrap(), d("-10.1527"));
}

#[test]
fn random_is_in_unit_interval() {
    for _ in 0..100 {
        let r = func::random();
        assert!(r >= Dec::zero() && r < Dec::one());
    }
}
