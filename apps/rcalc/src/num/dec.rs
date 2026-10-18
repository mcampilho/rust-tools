//! Arbitrary-precision decimal floating point.
//!
//! A value is `(-1)^neg * coef * 10^exp`, where `coef` is an unbounded
//! integer. Values are kept canonical (no trailing zeros in `coef`, zero is
//! always `+0 * 10^0`), so structural equality is numeric equality.
//!
//! Decimal input such as `0.1` is represented exactly. Operations that cannot
//! be exact (division, roots, transcendental functions) are rounded
//! half-to-even to a caller-chosen number of significant digits.

use num_bigint::{BigInt, BigUint, Sign};
use num_integer::Integer;
use num_traits::{Signed, ToPrimitive, Zero};
use std::cmp::Ordering;
use std::fmt;

use super::CalcError;

/// Significant digits kept by arithmetic results. The UI shows at most 32,
/// so the extra digits absorb rounding error from chained operations.
pub const PREC: usize = 64;

/// Largest decimal exponent accepted before reporting overflow (as Windows
/// Calculator does, results are limited to roughly 1e±10000).
pub const MAX_EXP: i64 = 9999;

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Dec {
    neg: bool,
    coef: BigUint,
    exp: i64,
}

fn pow10(n: u32) -> BigUint {
    BigUint::from(10u32).pow(n)
}

fn digit_count(c: &BigUint) -> usize {
    if c.is_zero() {
        return 1;
    }
    // Estimate from the bit length, then correct by at most one.
    let bits = c.bits();
    let est = ((bits - 1) as f64 * std::f64::consts::LOG10_2) as u32 + 1;
    if *c >= pow10(est) {
        est as usize + 1
    } else {
        est as usize
    }
}

impl Dec {
    pub fn zero() -> Dec {
        Dec {
            neg: false,
            coef: BigUint::zero(),
            exp: 0,
        }
    }

    pub fn one() -> Dec {
        Dec::from_i64(1)
    }

    pub fn from_i64(v: i64) -> Dec {
        Dec::from_parts(v < 0, BigUint::from(v.unsigned_abs()), 0)
    }

    pub fn from_u128(v: u128) -> Dec {
        Dec::from_parts(false, BigUint::from(v), 0)
    }

    /// Builds a canonical value from raw parts.
    pub fn from_parts(neg: bool, mut coef: BigUint, mut exp: i64) -> Dec {
        if coef.is_zero() {
            return Dec::zero();
        }
        let ten = BigUint::from(10u32);
        // Strip trailing zeros; fast path for large runs.
        loop {
            let (q, r) = coef.div_rem(&ten);
            if !r.is_zero() {
                break;
            }
            coef = q;
            exp += 1;
        }
        Dec { neg, coef, exp }
    }

    /// Exact `m * 10^e` with integer `m`.
    pub fn scaled(m: i64, e: i64) -> Dec {
        let mut d = Dec::from_i64(m);
        if !d.is_zero() {
            d.exp += e;
        }
        d
    }

    pub fn from_f64(v: f64) -> Option<Dec> {
        if !v.is_finite() {
            return None;
        }
        // 17 significant digits round-trip any f64.
        format!("{:.16e}", v).parse().ok()
    }

    pub fn is_zero(&self) -> bool {
        self.coef.is_zero()
    }

    pub fn is_neg(&self) -> bool {
        self.neg
    }

    pub fn is_integer(&self) -> bool {
        self.exp >= 0 || self.is_zero()
    }

    /// Exponent of the most significant digit (`123.4` -> 2).
    pub fn adjusted_exp(&self) -> i64 {
        if self.is_zero() {
            0
        } else {
            self.exp + digit_count(&self.coef) as i64 - 1
        }
    }

    pub fn neg(&self) -> Dec {
        if self.is_zero() {
            return self.clone();
        }
        Dec {
            neg: !self.neg,
            coef: self.coef.clone(),
            exp: self.exp,
        }
    }

    pub fn abs(&self) -> Dec {
        Dec {
            neg: false,
            coef: self.coef.clone(),
            exp: self.exp,
        }
    }

    /// Rounds to `p` significant digits (half to even).
    pub fn round_sig(&self, p: usize) -> Dec {
        round_parts(self.neg, self.coef.clone(), self.exp, p, false)
    }

    /// Rounds to `places` digits after the decimal point (half away from zero).
    pub fn round_places(&self, places: i64) -> Dec {
        if self.is_zero() || self.exp >= -places {
            return self.clone();
        }
        let drop = (-places - self.exp) as u32;
        let div = pow10(drop);
        let (mut q, r) = self.coef.div_rem(&div);
        if r.clone() * 2u32 >= div {
            q += 1u32;
        }
        Dec::from_parts(self.neg, q, -places)
    }

    /// Returns an error if the magnitude is outside the supported range.
    pub fn check(self) -> Result<Dec, CalcError> {
        if self.is_zero() {
            return Ok(self);
        }
        let adj = self.adjusted_exp();
        if !(-MAX_EXP..=MAX_EXP).contains(&adj) {
            Err(CalcError::Overflow)
        } else {
            Ok(self)
        }
    }

    pub fn add_p(&self, other: &Dec, p: usize) -> Dec {
        if self.is_zero() {
            return other.round_sig(p);
        }
        if other.is_zero() {
            return self.round_sig(p);
        }
        let (a_adj, b_adj) = (self.adjusted_exp(), other.adjusted_exp());
        // If one operand is far below the precision of the other it cannot
        // affect the rounded result; skip the (potentially huge) alignment.
        let gap = p as i64 + 3;
        if a_adj - b_adj > gap && self.exp <= b_adj {
            return self.round_sig(p);
        }
        if b_adj - a_adj > gap && other.exp <= a_adj {
            return other.round_sig(p);
        }
        let e = self.exp.min(other.exp);
        let ca = &self.coef * pow10((self.exp - e) as u32);
        let cb = &other.coef * pow10((other.exp - e) as u32);
        let (neg, coef) = if self.neg == other.neg {
            (self.neg, ca + cb)
        } else {
            match ca.cmp(&cb) {
                Ordering::Equal => return Dec::zero(),
                Ordering::Greater => (self.neg, ca - cb),
                Ordering::Less => (other.neg, cb - ca),
            }
        };
        round_parts(neg, coef, e, p, false)
    }

    pub fn sub_p(&self, other: &Dec, p: usize) -> Dec {
        self.add_p(&other.neg(), p)
    }

    pub fn mul_p(&self, other: &Dec, p: usize) -> Dec {
        if self.is_zero() || other.is_zero() {
            return Dec::zero();
        }
        round_parts(self.neg != other.neg, &self.coef * &other.coef, self.exp + other.exp, p, false)
    }

    /// Division rounded to `p` digits. The caller must reject a zero divisor.
    pub fn div_p(&self, other: &Dec, p: usize) -> Dec {
        assert!(!other.is_zero(), "division by zero");
        if self.is_zero() {
            return Dec::zero();
        }
        let da = digit_count(&self.coef) as i64;
        let db = digit_count(&other.coef) as i64;
        let shift = (p as i64 + 3 + db - da).max(0) as u32;
        let num = &self.coef * pow10(shift);
        let (q, r) = num.div_rem(&other.coef);
        let exp = self.exp - other.exp - shift as i64;
        round_parts(self.neg != other.neg, q, exp, p, !r.is_zero())
    }

    pub fn add(&self, o: &Dec) -> Dec {
        self.add_p(o, PREC)
    }
    pub fn sub(&self, o: &Dec) -> Dec {
        self.sub_p(o, PREC)
    }
    pub fn mul(&self, o: &Dec) -> Dec {
        self.mul_p(o, PREC)
    }

    pub fn div(&self, o: &Dec) -> Result<Dec, CalcError> {
        if o.is_zero() {
            return Err(if self.is_zero() {
                CalcError::Undefined
            } else {
                CalcError::DivideByZero
            });
        }
        Ok(self.div_p(o, PREC))
    }

    /// Integer part, truncated toward zero.
    pub fn trunc(&self) -> Dec {
        if self.is_integer() {
            return self.clone();
        }
        let (q, _) = self.coef.div_rem(&pow10((-self.exp) as u32));
        Dec::from_parts(self.neg, q, 0)
    }

    pub fn floor(&self) -> Dec {
        let t = self.trunc();
        if self.neg && t != *self {
            t.sub(&Dec::one())
        } else {
            t
        }
    }

    pub fn ceil(&self) -> Dec {
        let t = self.trunc();
        if !self.neg && t != *self {
            t.add(&Dec::one())
        } else {
            t
        }
    }

    /// Nearest integer, ties away from zero.
    pub fn round_int(&self) -> Dec {
        self.round_places(0)
    }

    /// Exact integer conversion; `None` if the value is not an integer.
    pub fn to_bigint(&self) -> Option<BigInt> {
        if !self.is_integer() {
            return None;
        }
        let mag = &self.coef * pow10(self.exp.max(0) as u32);
        let sign = if self.is_zero() {
            Sign::NoSign
        } else if self.neg {
            Sign::Minus
        } else {
            Sign::Plus
        };
        Some(BigInt::from_biguint(sign, mag))
    }

    pub fn to_i64(&self) -> Option<i64> {
        if !self.is_integer() || self.adjusted_exp() > 19 {
            return None;
        }
        self.to_bigint()?.to_i64()
    }

    pub fn to_i128(&self) -> Option<i128> {
        if !self.is_integer() || self.adjusted_exp() > 39 {
            return None;
        }
        self.to_bigint()?.to_i128()
    }

    pub fn to_f64(&self) -> f64 {
        if self.is_zero() {
            return 0.0;
        }
        let r = self.round_sig(17);
        format!("{}{}e{}", if r.neg { "-" } else { "" }, r.coef, r.exp)
            .parse()
            .unwrap_or(f64::NAN)
    }

    /// Exact floored remainder: `a - b * floor(a / b)` (result has the sign of `b`).
    pub fn rem_floor(&self, b: &Dec) -> Result<Dec, CalcError> {
        self.rem_impl(b, true)
    }

    fn rem_impl(&self, b: &Dec, floored: bool) -> Result<Dec, CalcError> {
        if b.is_zero() {
            return Err(CalcError::DivideByZero);
        }
        if self.is_zero() {
            return Ok(Dec::zero());
        }
        if self.adjusted_exp() - b.adjusted_exp() > 2 * MAX_EXP + 64 {
            return Err(CalcError::Overflow);
        }
        let e = self.exp.min(b.exp);
        let to_int = |d: &Dec| {
            let m = BigInt::from_biguint(Sign::Plus, &d.coef * pow10((d.exp - e) as u32));
            if d.neg {
                -m
            } else {
                m
            }
        };
        let (ia, ib) = (to_int(self), to_int(b));
        let r = if floored { ia.mod_floor(&ib) } else { ia % &ib };
        Ok(Dec::from_parts(r.is_negative(), r.magnitude().clone(), e))
    }

    /// Exact square root when possible, otherwise rounded to `p` digits.
    pub fn sqrt_p(&self, p: usize) -> Result<Dec, CalcError> {
        if self.neg {
            return Err(CalcError::InvalidInput);
        }
        if self.is_zero() {
            return Ok(Dec::zero());
        }
        let d = digit_count(&self.coef) as i64;
        let mut s = (2 * (p as i64 + 2) - d).max(0);
        if (self.exp - s).rem_euclid(2) != 0 {
            s += 1;
        }
        let n = &self.coef * pow10(s as u32);
        let root = n.sqrt();
        let exact = &root * &root == n;
        Ok(round_parts(false, root, (self.exp - s) / 2, p, !exact))
    }

    /// Splits into (negative, significant digits, adjusted exponent), rounded
    /// to at most `p` significant digits. Zero yields `("0", 0)`.
    pub fn sig_digits(&self, p: usize) -> (bool, String, i64) {
        if self.is_zero() {
            return (false, "0".into(), 0);
        }
        let r = self.round_sig(p);
        let s = r.coef.to_str_radix(10);
        let adj = r.exp + s.len() as i64 - 1;
        (r.neg, s, adj)
    }

    /// Plain (non-grouped) string. Uses fixed notation when the value fits
    /// in `max_digits` digit positions (leading zeros of small numbers count
    /// against the width, as in Windows Calculator), otherwise scientific
    /// notation (`1.5e+20`).
    pub fn format(&self, max_digits: usize, force_sci: bool) -> String {
        let max = max_digits as i64;
        let (_, _, adj0) = self.sig_digits(max_digits);
        let fixed = !force_sci && adj0 < max && adj0 >= -(max / 2).max(5);
        let p = if fixed && adj0 < 0 {
            (max + adj0 + 1).max(1) as usize
        } else {
            max_digits
        };
        let (neg, digits, adj) = self.sig_digits(p);
        let mut out = String::new();
        if neg {
            out.push('-');
        }
        if fixed && adj < max {
            if adj >= 0 {
                let int_len = (adj + 1) as usize;
                if digits.len() <= int_len {
                    out.push_str(&digits);
                    out.extend(std::iter::repeat_n('0', int_len - digits.len()));
                } else {
                    out.push_str(&digits[..int_len]);
                    out.push('.');
                    out.push_str(&digits[int_len..]);
                }
            } else {
                out.push_str("0.");
                out.extend(std::iter::repeat_n('0', (-adj - 1) as usize));
                out.push_str(&digits);
            }
        } else {
            out.push_str(&digits[..1]);
            if digits.len() > 1 {
                out.push('.');
                out.push_str(&digits[1..]);
            } else if force_sci {
                out.push('.');
            }
            out.push('e');
            out.push(if adj < 0 { '-' } else { '+' });
            out.push_str(&adj.abs().to_string());
        }
        out
    }

    /// Lossless string (scientific when the exponent is large).
    pub fn to_exact_string(&self) -> String {
        if self.is_zero() {
            return "0".into();
        }
        let s = self.coef.to_str_radix(10);
        let sign = if self.neg { "-" } else { "" };
        if self.exp >= 0 && self.exp <= 40 {
            format!("{sign}{s}{}", "0".repeat(self.exp as usize))
        } else if self.exp < 0 && -self.exp <= 60 {
            let frac = (-self.exp) as usize;
            if s.len() > frac {
                format!("{sign}{}.{}", &s[..s.len() - frac], &s[s.len() - frac..])
            } else {
                format!("{sign}0.{}{s}", "0".repeat(frac - s.len()))
            }
        } else {
            format!("{sign}{s}e{}", self.exp)
        }
    }

    /// Multiplies by `10^k` exactly.
    pub fn mul_pow10(&self, k: i64) -> Dec {
        if self.is_zero() {
            return Dec::zero();
        }
        Dec {
            neg: self.neg,
            coef: self.coef.clone(),
            exp: self.exp + k,
        }
    }

    pub fn coefficient(&self) -> &BigUint {
        &self.coef
    }

    pub fn exponent(&self) -> i64 {
        self.exp
    }
}

/// Rounds `coef * 10^exp` to `p` significant digits. `sticky` marks that the
/// true value is slightly larger in magnitude than `coef` (a nonzero
/// remainder was discarded), which matters for exact half-way ties.
fn round_parts(neg: bool, coef: BigUint, exp: i64, p: usize, sticky: bool) -> Dec {
    let d = digit_count(&coef);
    if d <= p {
        // Values produced with a sticky remainder always carry p+3 digits,
        // so this branch only sees exact results.
        return Dec::from_parts(neg, coef, exp);
    }
    let drop = (d - p) as u32;
    let div = pow10(drop);
    let (mut q, r) = coef.div_rem(&div);
    let twice = r * 2u32;
    let round_up = match twice.cmp(&div) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => sticky || q.is_odd(),
    };
    if round_up {
        q += 1u32;
    }
    Dec::from_parts(neg, q, exp + drop as i64)
}

impl PartialOrd for Dec {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Dec {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.is_zero(), other.is_zero()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return if other.neg { Ordering::Greater } else { Ordering::Less },
            (false, true) => return if self.neg { Ordering::Less } else { Ordering::Greater },
            _ => {}
        }
        if self.neg != other.neg {
            return if self.neg { Ordering::Less } else { Ordering::Greater };
        }
        let mag = {
            let (a, b) = (self.adjusted_exp(), other.adjusted_exp());
            if a != b {
                a.cmp(&b)
            } else {
                let e = self.exp.min(other.exp);
                let ca = &self.coef * pow10((self.exp - e) as u32);
                let cb = &other.coef * pow10((other.exp - e) as u32);
                ca.cmp(&cb)
            }
        };
        if self.neg {
            mag.reverse()
        } else {
            mag
        }
    }
}

impl std::str::FromStr for Dec {
    type Err = CalcError;

    /// Parses `[-+]digits[.digits][e[-+]digits]`. Grouping commas are not accepted here.
    fn from_str(s: &str) -> Result<Dec, CalcError> {
        let s = s.trim();
        let (neg, body) = match s.as_bytes().first() {
            Some(b'-') => (true, &s[1..]),
            Some(b'+') => (false, &s[1..]),
            _ => (false, s),
        };
        let (mant, exp_part) = match body.find(['e', 'E']) {
            Some(i) => (&body[..i], Some(&body[i + 1..])),
            None => (body, None),
        };
        let (int_part, frac_part) = match mant.find('.') {
            Some(i) => (&mant[..i], &mant[i + 1..]),
            None => (mant, ""),
        };
        if int_part.is_empty() && frac_part.is_empty() {
            return Err(CalcError::InvalidInput);
        }
        if !int_part.bytes().chain(frac_part.bytes()).all(|b| b.is_ascii_digit()) {
            return Err(CalcError::InvalidInput);
        }
        let mut exp: i64 = match exp_part {
            Some(e) => {
                let e = e.strip_prefix('+').unwrap_or(e);
                if e.is_empty() || e.len() > 12 {
                    return Err(CalcError::InvalidInput);
                }
                e.parse().map_err(|_| CalcError::InvalidInput)?
            }
            None => 0,
        };
        exp -= frac_part.len() as i64;
        let digits: String = format!("{int_part}{frac_part}");
        let coef = BigUint::parse_bytes(digits.as_bytes(), 10).unwrap_or_default();
        Ok(Dec::from_parts(neg, coef, exp))
    }
}

impl fmt::Debug for Dec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Dec({})", self.to_exact_string())
    }
}

impl fmt::Display for Dec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format(32, false))
    }
}

impl From<i64> for Dec {
    fn from(v: i64) -> Dec {
        Dec::from_i64(v)
    }
}
