//! Elementary and transcendental functions on [`Dec`].
//!
//! Every function works with guard digits beyond the requested precision and
//! rounds once at the end. Exact results are returned for the cases people
//! notice: integer powers, perfect roots, powers of ten in `log`, and the
//! special angles (0, 30, 45, 60, 90, ... degrees) for trigonometry.

use super::{CalcError, CalcResult, Dec, MAX_EXP, PREC};
use num_bigint::{BigInt, BigUint};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::cell::Cell;
use std::sync::OnceLock;

const CONST_P: usize = 150;
const GUARD: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Angle {
    #[default]
    Deg,
    Rad,
    Grad,
}

struct Consts {
    pi: Dec,
    ln2: Dec,
    ln10: Dec,
}

fn consts() -> &'static Consts {
    static C: OnceLock<Consts> = OnceLock::new();
    C.get_or_init(|| {
        let p = CONST_P + 10;
        let inv = |n: i64| Dec::one().div_p(&Dec::from_i64(n), p);
        // Machin: pi = 16 atan(1/5) - 4 atan(1/239)
        let pi = atan_series(&inv(5), p)
            .mul_p(&Dec::from_i64(16), p)
            .sub_p(&atan_series(&inv(239), p).mul_p(&Dec::from_i64(4), p), p);
        // ln 2 = 2 atanh(1/3); ln 10 = 3 ln 2 + ln 1.25 = 3 ln 2 + 2 atanh(1/9)
        let ln2 = atanh_series(&inv(3), p).mul_p(&Dec::from_i64(2), p);
        let ln10 = ln2
            .mul_p(&Dec::from_i64(3), p)
            .add_p(&atanh_series(&inv(9), p).mul_p(&Dec::from_i64(2), p), p);
        Consts {
            pi: pi.round_sig(CONST_P),
            ln2: ln2.round_sig(CONST_P),
            ln10: ln10.round_sig(CONST_P),
        }
    })
}

pub fn pi() -> Dec {
    consts().pi.round_sig(PREC)
}

pub fn euler() -> Dec {
    static E: OnceLock<Dec> = OnceLock::new();
    E.get_or_init(|| exp_p(&Dec::one(), PREC).expect("e is in range")).clone()
}

fn negligible(term: &Dec, sum: &Dec, p: usize) -> bool {
    term.is_zero() || (!sum.is_zero() && term.adjusted_exp() < sum.adjusted_exp() - p as i64 - 2)
}

/// `atanh(z) = z + z^3/3 + z^5/5 + ...`, for small |z|.
fn atanh_series(z: &Dec, p: usize) -> Dec {
    if z.is_zero() {
        return Dec::zero();
    }
    let z2 = z.mul_p(z, p);
    let mut power = z.clone();
    let mut sum = z.clone();
    let mut k = 1i64;
    loop {
        power = power.mul_p(&z2, p);
        let term = power.div_p(&Dec::from_i64(2 * k + 1), p);
        if negligible(&term, &sum, p) {
            break;
        }
        sum = sum.add_p(&term, p);
        k += 1;
    }
    sum
}

/// `atan(z) = z - z^3/3 + z^5/5 - ...`, for small |z|.
fn atan_series(z: &Dec, p: usize) -> Dec {
    if z.is_zero() {
        return Dec::zero();
    }
    let z2 = z.mul_p(z, p);
    let mut power = z.clone();
    let mut sum = z.clone();
    let mut k = 1i64;
    loop {
        power = power.mul_p(&z2, p).neg();
        let term = power.div_p(&Dec::from_i64(2 * k + 1), p);
        if negligible(&term, &sum, p) {
            break;
        }
        sum = sum.add_p(&term, p);
        k += 1;
    }
    sum
}

/// True when `x` is so small that `f(x) = x` to `p` digits for the odd
/// functions sin, tan, asin, atan, sinh, tanh, asinh, atanh.
fn tiny(x: &Dec, p: usize) -> bool {
    !x.is_zero() && x.adjusted_exp() < -(p as i64) - 2
}

/// log10(|x|) as an f64 estimate (for range checks only).
fn log10_estimate(x: &Dec) -> f64 {
    if x.is_zero() {
        return f64::NEG_INFINITY;
    }
    let adj = x.adjusted_exp();
    let mant = x.abs().mul_pow10(-adj).to_f64();
    adj as f64 + mant.log10()
}

// ---------------------------------------------------------------- exp / ln

pub fn exp(x: &Dec) -> CalcResult {
    exp_p(x, PREC)
}

pub fn exp_p(x: &Dec, p: usize) -> CalcResult {
    if x.is_zero() {
        return Ok(Dec::one());
    }
    // e^23026 ~ 1e10000
    if x.adjusted_exp() > 5 || x.abs() > Dec::from_i64(23030) {
        return Err(CalcError::Overflow);
    }
    let g = p + GUARD;
    let c = consts();
    let k = (x.to_f64() / std::f64::consts::LN_10).round() as i64;
    let r = x.sub_p(&c.ln10.mul_p(&Dec::from_i64(k), g + 6), g + 6);
    let s = r.div_p(&Dec::from_i64(256), g);
    let mut term = Dec::one();
    let mut sum = Dec::one();
    let mut n = 1i64;
    loop {
        term = term.mul_p(&s, g).div_p(&Dec::from_i64(n), g);
        if negligible(&term, &sum, g) {
            break;
        }
        sum = sum.add_p(&term, g);
        n += 1;
    }
    for _ in 0..8 {
        sum = sum.mul_p(&sum, g);
    }
    sum.mul_pow10(k).round_sig(p).check()
}

pub fn ln(x: &Dec) -> CalcResult {
    ln_p(x, PREC)
}

pub fn ln_p(x: &Dec, p: usize) -> CalcResult {
    if x.is_neg() || x.is_zero() {
        return Err(CalcError::InvalidInput);
    }
    let one = Dec::one();
    if *x == one {
        return Ok(Dec::zero());
    }
    let g = p + GUARD;
    let c = consts();
    let near_one = x.sub(&one).abs() < Dec::scaled(5, -1);
    let result = if near_one {
        // Direct path avoids cancellation for x close to 1.
        let z = x.sub_p(&one, g + 4).div_p(&x.add_p(&one, g + 4), g);
        atanh_series(&z, g).mul_p(&Dec::from_i64(2), g)
    } else {
        let k = x.adjusted_exp();
        let mut m = x.mul_pow10(-k); // [1, 10)
        let mut j = 0i64;
        let limit = Dec::scaled(15, -1);
        let half = Dec::scaled(5, -1);
        while m > limit {
            m = m.mul_p(&half, g + 8);
            j += 1;
        }
        let z = m.sub_p(&one, g).div_p(&m.add_p(&one, g), g);
        atanh_series(&z, g)
            .mul_p(&Dec::from_i64(2), g)
            .add_p(&c.ln2.mul_p(&Dec::from_i64(j), g), g)
            .add_p(&c.ln10.mul_p(&Dec::from_i64(k), g + 6), g)
    };
    Ok(result.round_sig(p))
}

pub fn log10(x: &Dec) -> CalcResult {
    if x.is_neg() || x.is_zero() {
        return Err(CalcError::InvalidInput);
    }
    if x.coefficient().is_one() {
        return Ok(Dec::from_i64(x.exponent()));
    }
    let g = PREC + GUARD;
    Ok(ln_p(x, g)?.div_p(&consts().ln10, g).round_sig(PREC))
}

/// Logarithm of `x` in base `b`.
pub fn log_base(x: &Dec, b: &Dec) -> CalcResult {
    if b.is_neg() || b.is_zero() || *b == Dec::one() || x.is_neg() || x.is_zero() {
        return Err(CalcError::InvalidInput);
    }
    let g = PREC + GUARD;
    Ok(ln_p(x, g)?.div_p(&ln_p(b, g)?, g).round_sig(PREC))
}

// ---------------------------------------------------------------- powers

/// `x^y`. Integer exponents are computed by repeated squaring (exact for
/// exact inputs within the working precision).
pub fn pow(x: &Dec, y: &Dec) -> CalcResult {
    if y.is_zero() {
        return Ok(Dec::one());
    }
    if x.is_zero() {
        return if y.is_neg() {
            Err(CalcError::DivideByZero)
        } else {
            Ok(Dec::zero())
        };
    }
    if let Some(n) = y.to_bigint() {
        if let Some(r) = pow_int(x, &n) {
            return r;
        }
    }
    if x.is_neg() {
        return Err(CalcError::InvalidInput);
    }
    let est = y.to_f64() * log10_estimate(x);
    if est > (MAX_EXP + 1) as f64 || est < -(MAX_EXP + 1) as f64 {
        return Err(CalcError::Overflow);
    }
    let g = PREC + GUARD + 6;
    let l = ln_p(x, g)?;
    exp_p(&y.mul_p(&l, g), PREC)
}

/// Integer power. Returns `None` when the exponent is too large for
/// repeated squaring (the caller falls back to exp/ln).
fn pow_int(x: &Dec, n: &BigInt) -> Option<CalcResult> {
    let one = Dec::one();
    if x.abs() == one {
        let odd = n.is_odd();
        return Some(Ok(if x.is_neg() && odd { one.neg() } else { one }));
    }
    let est = n.to_f64().unwrap_or(f64::INFINITY) * log10_estimate(x);
    if !est.is_finite() || est > (MAX_EXP + 1) as f64 || est < -(MAX_EXP + 1) as f64 {
        return Some(Err(CalcError::Overflow));
    }
    let e = n.magnitude().to_u64()?;
    let g = PREC + GUARD + 20;
    let mut result = Dec::one();
    let mut base = x.clone();
    let mut k = e;
    while k > 0 {
        if k & 1 == 1 {
            result = result.mul_p(&base, g);
        }
        k >>= 1;
        if k > 0 {
            base = base.mul_p(&base, g);
        }
    }
    let r = if n.is_negative() { one.div_p(&result, g) } else { result };
    Some(r.round_sig(PREC).check())
}

pub fn sqrt(x: &Dec) -> CalcResult {
    x.sqrt_p(PREC)
}

/// `y`-th root of `x` (`x^(1/y)`); odd integer roots of negatives are allowed.
pub fn root(x: &Dec, y: &Dec) -> CalcResult {
    if y.is_zero() {
        return Err(CalcError::InvalidInput);
    }
    let yi = y.to_bigint();
    if x.is_neg() {
        return match &yi {
            Some(n) if n.is_odd() => Ok(root(&x.neg(), y)?.neg()),
            _ => Err(CalcError::InvalidInput),
        };
    }
    if x.is_zero() {
        return if y.is_neg() {
            Err(CalcError::DivideByZero)
        } else {
            Ok(Dec::zero())
        };
    }
    if yi == Some(BigInt::from(2)) {
        return sqrt(x);
    }
    let g = PREC + GUARD;
    let r = exp_p(&ln_p(x, g)?.div_p(y, g), PREC)?;
    // Snap to an exact integer root when one exists (e.g. cube root of 27).
    if let Some(n) = &yi {
        if x.is_integer() && n.is_positive() && *n <= BigInt::from(1000) {
            let cand = r.round_int();
            if let Some(Ok(back)) = pow_int(&cand, n) {
                if back == *x {
                    return Ok(cand);
                }
            }
        }
    }
    Ok(r)
}

// ---------------------------------------------------------------- trigonometry

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trig {
    Sin,
    Cos,
    Tan,
    Sec,
    Csc,
    Cot,
}

/// Reduces an angle in degrees or gradians to exact degrees in `[0, 360)`.
fn reduce_degrees(x: &Dec, unit: Angle) -> Result<Dec, CalcError> {
    match unit {
        Angle::Deg => x.rem_floor(&Dec::from_i64(360)),
        Angle::Grad => Ok(x.rem_floor(&Dec::from_i64(400))?.mul_p(&Dec::scaled(9, -1), PREC + 8)),
        Angle::Rad => unreachable!(),
    }
}

/// Exact sine for multiples of 30 and 90 degrees (when the value is rational).
fn special_sin(deg: i64) -> Option<Dec> {
    let h = Dec::scaled(5, -1);
    Some(match deg {
        0 | 180 => Dec::zero(),
        30 | 150 => h,
        90 => Dec::one(),
        210 | 330 => h.neg(),
        270 => Dec::one().neg(),
        _ => return None,
    })
}

fn special_cos(deg: i64) -> Option<Dec> {
    let h = Dec::scaled(5, -1);
    Some(match deg {
        0 => Dec::one(),
        60 | 300 => h,
        90 | 270 => Dec::zero(),
        120 | 240 => h.neg(),
        180 => Dec::one().neg(),
        _ => return None,
    })
}

fn special_tan(deg: i64) -> Option<Result<Dec, CalcError>> {
    Some(match deg {
        0 | 180 => Ok(Dec::zero()),
        45 | 225 => Ok(Dec::one()),
        135 | 315 => Ok(Dec::one().neg()),
        90 | 270 => Err(CalcError::InvalidInput),
        _ => return None,
    })
}

/// (sin x, cos x) for `x` in radians.
fn sin_cos_rad(x: &Dec, p: usize) -> Result<(Dec, Dec), CalcError> {
    if x.adjusted_exp() > 45 {
        return Err(CalcError::InvalidInput);
    }
    let g = p + GUARD + x.adjusted_exp().max(0) as usize;
    let half_pi = consts().pi.mul_p(&Dec::scaled(5, -1), CONST_P);
    let q = x.div_p(&half_pi, g).round_int();
    let qi = q.to_bigint().unwrap_or_default();
    let t = x.sub_p(&q.mul_p(&half_pi, g + 4), g);
    let t2 = t.mul_p(&t, g);
    // sin
    let mut term = t.clone();
    let mut s = t.clone();
    let mut n = 1i64;
    loop {
        term = term.mul_p(&t2, g).div_p(&Dec::from_i64((2 * n) * (2 * n + 1)), g).neg();
        if negligible(&term, &s, g) {
            break;
        }
        s = s.add_p(&term, g);
        n += 1;
    }
    // cos
    let mut term = Dec::one();
    let mut c = Dec::one();
    let mut n = 1i64;
    loop {
        term = term.mul_p(&t2, g).div_p(&Dec::from_i64((2 * n - 1) * (2 * n)), g).neg();
        if negligible(&term, &c, g) {
            break;
        }
        c = c.add_p(&term, g);
        n += 1;
    }
    let quadrant = qi.mod_floor(&BigInt::from(4)).to_u8().unwrap_or(0);
    let (mut s, mut c) = match quadrant {
        0 => (s, c),
        1 => (c, s.neg()),
        2 => (s.neg(), c.neg()),
        _ => (c.neg(), s),
    };
    if !qi.is_zero() {
        // After range reduction, values below the absolute precision are noise
        // from the finite-precision multiple of pi.
        let eps = -(p as i64) + 6;
        if !s.is_zero() && s.adjusted_exp() < eps {
            s = Dec::zero();
        }
        if !c.is_zero() && c.adjusted_exp() < eps {
            c = Dec::zero();
        }
    }
    Ok((s.round_sig(p), c.round_sig(p)))
}

fn to_radians(x: &Dec, unit: Angle, g: usize) -> Dec {
    let pi = &consts().pi;
    match unit {
        Angle::Rad => x.clone(),
        Angle::Deg => x.mul_p(pi, g).div_p(&Dec::from_i64(180), g),
        Angle::Grad => x.mul_p(pi, g).div_p(&Dec::from_i64(200), g),
    }
}

fn from_radians(x: &Dec, unit: Angle) -> Dec {
    let g = PREC + GUARD;
    let pi = &consts().pi;
    match unit {
        Angle::Rad => x.round_sig(PREC),
        Angle::Deg => x.mul_p(&Dec::from_i64(180), g).div_p(pi, g).round_sig(PREC),
        Angle::Grad => x.mul_p(&Dec::from_i64(200), g).div_p(pi, g).round_sig(PREC),
    }
}

/// (sin, cos) of an angle in the given unit, exact for special angles.
fn sin_cos(x: &Dec, unit: Angle) -> Result<(Dec, Dec), CalcError> {
    if unit == Angle::Rad {
        return sin_cos_rad(x, PREC);
    }
    let deg = reduce_degrees(x, unit)?;
    let g = PREC + GUARD;
    let (mut s, mut c) = sin_cos_rad(&to_radians(&deg, Angle::Deg, g), PREC)?;
    if let Some(d) = deg.to_i64() {
        if let Some(v) = special_sin(d) {
            s = v;
        }
        if let Some(v) = special_cos(d) {
            c = v;
        }
    }
    Ok((s, c))
}

pub fn trig(f: Trig, x: &Dec, unit: Angle) -> CalcResult {
    if unit != Angle::Rad {
        if let Some(d) = reduce_degrees(x, unit)?.to_i64() {
            if f == Trig::Tan {
                if let Some(r) = special_tan(d) {
                    return r;
                }
            }
            if f == Trig::Cot {
                if let Some(r) = special_tan(d) {
                    return match r {
                        Ok(t) if t.is_zero() => Err(CalcError::InvalidInput),
                        Ok(t) => Dec::one().div(&t),
                        Err(_) => Ok(Dec::zero()),
                    };
                }
            }
        }
    } else if tiny(x, PREC) && matches!(f, Trig::Sin | Trig::Tan) {
        return Ok(x.clone());
    }
    let (s, c) = sin_cos(x, unit)?;
    let inv = |v: &Dec| {
        if v.is_zero() {
            Err(CalcError::InvalidInput)
        } else {
            Ok(Dec::one().div_p(v, PREC))
        }
    };
    match f {
        Trig::Sin => Ok(s),
        Trig::Cos => Ok(c),
        Trig::Tan => {
            if c.is_zero() {
                Err(CalcError::InvalidInput)
            } else {
                Ok(s.div_p(&c, PREC))
            }
        }
        Trig::Sec => inv(&c),
        Trig::Csc => inv(&s),
        Trig::Cot => {
            if s.is_zero() {
                Err(CalcError::InvalidInput)
            } else {
                Ok(c.div_p(&s, PREC))
            }
        }
    }
}

fn atan_rad(x: &Dec, p: usize) -> Dec {
    if x.is_zero() {
        return Dec::zero();
    }
    if tiny(x, p) {
        return x.clone();
    }
    let g = p + GUARD;
    let one = Dec::one();
    let mut a = x.abs();
    let invert = a > one;
    if invert {
        a = one.div_p(&a, g);
    }
    // atan(a) = 2 atan(a / (1 + sqrt(1 + a^2))), applied three times.
    for _ in 0..3 {
        let root = one.add_p(&a.mul_p(&a, g), g).sqrt_p(g).expect("positive");
        a = a.div_p(&one.add_p(&root, g), g);
    }
    let mut r = atan_series(&a, g).mul_p(&Dec::from_i64(8), g);
    if invert {
        r = consts().pi.mul_p(&Dec::scaled(5, -1), g).sub_p(&r, g);
    }
    if x.is_neg() {
        r = r.neg();
    }
    r.round_sig(p)
}

fn asin_rad(x: &Dec, p: usize) -> CalcResult {
    let one = Dec::one();
    let a = x.abs();
    if a > one {
        return Err(CalcError::InvalidInput);
    }
    let g = p + GUARD;
    if a == one {
        let hp = consts().pi.mul_p(&Dec::scaled(5, -1), g).round_sig(p);
        return Ok(if x.is_neg() { hp.neg() } else { hp });
    }
    let d = one.sub_p(&x.mul_p(x, 2 * g), 2 * g).sqrt_p(g)?;
    Ok(atan_rad(&x.div_p(&d, g), g).round_sig(p))
}

/// Inverse trigonometric function; the result is expressed in `unit`.
pub fn inverse_trig(f: Trig, x: &Dec, unit: Angle) -> CalcResult {
    let g = PREC + GUARD;
    let one = Dec::one();
    let half_pi = || consts().pi.mul_p(&Dec::scaled(5, -1), g);
    let recip = |v: &Dec| {
        if v.is_zero() {
            Err(CalcError::InvalidInput)
        } else {
            Ok(one.div_p(v, g))
        }
    };
    let rad = match f {
        Trig::Sin => asin_rad(x, g)?,
        Trig::Cos => {
            if *x == one {
                Dec::zero()
            } else {
                half_pi().sub_p(&asin_rad(x, g)?, g)
            }
        }
        Trig::Tan => atan_rad(x, g),
        Trig::Csc => asin_rad(&recip(x)?, g)?,
        Trig::Sec => {
            let r = recip(x)?;
            if r == one {
                Dec::zero()
            } else {
                half_pi().sub_p(&asin_rad(&r, g)?, g)
            }
        }
        Trig::Cot => {
            if x.is_zero() {
                half_pi()
            } else {
                atan_rad(&one.div_p(x, g), g)
            }
        }
    };
    Ok(snap_integer(from_radians(&rad, unit)))
}

/// Rounds values that are within working-precision noise of an integer
/// (e.g. asin(1) in degrees -> 90).
fn snap_integer(v: Dec) -> Dec {
    let r = v.round_int();
    if r != v && !r.is_zero() {
        let diff = v.sub(&r);
        if diff.adjusted_exp() < r.adjusted_exp() - (PREC as i64 - 6) {
            return r;
        }
    }
    v
}

// ---------------------------------------------------------------- hyperbolic

pub fn hyperbolic(f: Trig, x: &Dec) -> CalcResult {
    let one = Dec::one();
    let recip = |v: Dec| {
        if v.is_zero() {
            Err(CalcError::InvalidInput)
        } else {
            Ok(one.div_p(&v, PREC))
        }
    };
    match f {
        Trig::Sin => sinh(x),
        Trig::Cos => cosh(x),
        Trig::Tan => tanh(x),
        Trig::Sec => recip(cosh(x)?),
        Trig::Csc => recip(sinh(x)?),
        Trig::Cot => recip(tanh(x)?),
    }
}

pub fn inverse_hyperbolic(f: Trig, x: &Dec) -> CalcResult {
    let one = Dec::one();
    let recip = |v: &Dec| {
        if v.is_zero() {
            Err(CalcError::InvalidInput)
        } else {
            Ok(one.div_p(v, PREC + GUARD))
        }
    };
    match f {
        Trig::Sin => asinh(x),
        Trig::Cos => acosh(x),
        Trig::Tan => atanh(x),
        Trig::Sec => acosh(&recip(x)?),
        Trig::Csc => asinh(&recip(x)?),
        Trig::Cot => atanh(&recip(x)?),
    }
}

fn sinh(x: &Dec) -> CalcResult {
    if tiny(x, PREC) {
        return Ok(x.clone());
    }
    let g = PREC + GUARD;
    if x.abs() < Dec::one() {
        // Taylor series avoids cancellation in (e^x - e^-x) / 2.
        let x2 = x.mul_p(x, g);
        let mut term = x.clone();
        let mut sum = x.clone();
        let mut n = 1i64;
        loop {
            term = term.mul_p(&x2, g).div_p(&Dec::from_i64((2 * n) * (2 * n + 1)), g);
            if negligible(&term, &sum, g) {
                break;
            }
            sum = sum.add_p(&term, g);
            n += 1;
        }
        return Ok(sum.round_sig(PREC));
    }
    let e = exp_p(x, g)?;
    let ei = Dec::one().div_p(&e, g);
    Ok(e.sub_p(&ei, g).mul_p(&Dec::scaled(5, -1), g).round_sig(PREC))
}

fn cosh(x: &Dec) -> CalcResult {
    let g = PREC + GUARD;
    let e = exp_p(&x.abs(), g)?;
    let ei = Dec::one().div_p(&e, g);
    Ok(e.add_p(&ei, g).mul_p(&Dec::scaled(5, -1), g).round_sig(PREC))
}

fn tanh(x: &Dec) -> CalcResult {
    if tiny(x, PREC) {
        return Ok(x.clone());
    }
    if x.abs() > Dec::from_i64(100) {
        let one = Dec::one();
        return Ok(if x.is_neg() { one.neg() } else { one });
    }
    let g = PREC + GUARD;
    let s = sinh(x)?;
    let c = cosh(x)?;
    Ok(s.div_p(&c, g).round_sig(PREC))
}

fn asinh(x: &Dec) -> CalcResult {
    if tiny(x, PREC) {
        return Ok(x.clone());
    }
    let g = PREC + GUARD;
    let one = Dec::one();
    let a = x.abs();
    let r = if a < Dec::scaled(5, -1) {
        // asinh(x) = atanh(x / sqrt(1 + x^2))
        let d = one.add_p(&a.mul_p(&a, g), g).sqrt_p(g)?;
        atanh_series(&a.div_p(&d, g), g)
    } else {
        let d = a.mul_p(&a, g).add_p(&one, g).sqrt_p(g)?;
        ln_p(&a.add_p(&d, g), g)?
    };
    let r = r.round_sig(PREC);
    Ok(if x.is_neg() { r.neg() } else { r })
}

fn acosh(x: &Dec) -> CalcResult {
    let one = Dec::one();
    if *x < one {
        return Err(CalcError::InvalidInput);
    }
    if *x == one {
        return Ok(Dec::zero());
    }
    let g = PREC + GUARD;
    let d = x.mul_p(x, 2 * g).sub_p(&one, 2 * g).sqrt_p(g)?;
    Ok(ln_p(&x.add_p(&d, g), g)?.round_sig(PREC))
}

fn atanh(x: &Dec) -> CalcResult {
    let one = Dec::one();
    if x.abs() >= one {
        return Err(CalcError::InvalidInput);
    }
    if tiny(x, PREC) {
        return Ok(x.clone());
    }
    let g = PREC + GUARD;
    if x.abs() < Dec::scaled(5, -1) {
        return Ok(atanh_series(x, g).round_sig(PREC));
    }
    let q = one.add_p(x, 2 * g).div_p(&one.sub_p(x, 2 * g), g);
    Ok(ln_p(&q, g)?.mul_p(&Dec::scaled(5, -1), g).round_sig(PREC))
}

// ---------------------------------------------------------------- misc

/// Largest n with n! below 1e10000.
pub const MAX_FACTORIAL: i64 = 3248;

/// `x!`. Exact for non-negative integers; other values use the gamma
/// function in double precision (about 15 significant digits).
pub fn factorial(x: &Dec) -> CalcResult {
    if x.is_integer() {
        let n = x.to_i64().ok_or(CalcError::Overflow)?;
        if n < 0 {
            return Err(CalcError::InvalidInput);
        }
        if n > MAX_FACTORIAL {
            return Err(CalcError::Overflow);
        }
        let mut acc = BigUint::one();
        for i in 2..=n as u64 {
            acc *= i;
        }
        return Ok(Dec::from_parts(false, acc, 0).round_sig(PREC));
    }
    let f = x.to_f64();
    if f > 170.0 {
        return Err(CalcError::Overflow);
    }
    if f < -170.0 {
        return Err(CalcError::InvalidInput);
    }
    let g = gamma_f64(f + 1.0);
    if !g.is_finite() {
        return Err(CalcError::Overflow);
    }
    Dec::from_f64(g).map(|d| d.round_sig(15)).ok_or(CalcError::Overflow)
}

/// Lanczos approximation (g = 7, n = 9).
fn gamma_f64(x: f64) -> f64 {
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        let s = (std::f64::consts::PI * x).sin();
        return std::f64::consts::PI / (s * gamma_f64(1.0 - x));
    }
    let x = x - 1.0;
    let mut a = C[0];
    let t = x + G + 0.5;
    for (i, c) in C.iter().enumerate().skip(1) {
        a += c / (x + i as f64);
    }
    (2.0 * std::f64::consts::PI).sqrt() * t.powf(x + 0.5) * (-t).exp() * a
}

/// Decimal degrees to degrees-minutes-seconds packed as `D.MMSSsss`.
pub fn to_dms(x: &Dec) -> CalcResult {
    let g = PREC + 4;
    let sixty = Dec::from_i64(60);
    let a = x.abs();
    let d = a.trunc();
    let mf = a.sub_p(&d, g).mul_p(&sixty, g);
    let m = mf.trunc();
    let s = mf.sub_p(&m, g).mul_p(&sixty, g);
    let r = d
        .add_p(&m.div_p(&Dec::from_i64(100), g), g)
        .add_p(&s.div_p(&Dec::from_i64(10000), g), g)
        .round_sig(PREC);
    Ok(if x.is_neg() { r.neg() } else { r })
}

/// Degrees-minutes-seconds (`D.MMSSsss`) to decimal degrees.
pub fn from_dms(x: &Dec) -> CalcResult {
    let g = PREC + 4;
    let hundred = Dec::from_i64(100);
    let a = x.abs();
    let d = a.trunc();
    let mf = a.sub_p(&d, g).mul_p(&hundred, g);
    let m = mf.trunc();
    let s = mf.sub_p(&m, g).mul_p(&hundred, g);
    let r = d
        .add_p(&m.div_p(&Dec::from_i64(60), g), g)
        .add_p(&s.div_p(&Dec::from_i64(3600), g), g)
        .round_sig(PREC);
    Ok(if x.is_neg() { r.neg() } else { r })
}

thread_local! {
    static RNG: Cell<u64> = Cell::new({
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        t | 1
    });
}

fn next_u64() -> u64 {
    RNG.with(|s| {
        // xorshift64*
        let mut x = s.get();
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        s.set(x);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    })
}

/// Uniform random value in `[0, 1)` with 32 decimal digits.
pub fn random() -> Dec {
    let hi = (next_u64() % 10_000_000_000_000_000) as u128;
    let lo = (next_u64() % 10_000_000_000_000_000) as u128;
    let v = hi * 10_000_000_000_000_000 + lo;
    Dec::from_u128(v).mul_pow10(-32)
}
