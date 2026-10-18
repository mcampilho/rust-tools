//! Programmer mode arithmetic: fixed-width two's-complement integers.
//!
//! Like Windows Calculator, results wrap to the selected word size. Unlike
//! it, every wrap that loses information is reported to the caller so the UI
//! can show an "overflow" notice instead of silently presenting a truncated
//! value.

use crate::expr::{BinOp, Node, Unary};
use crate::format::group_radix;
use crate::num::CalcError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum WordSize {
    #[default]
    Qword,
    Dword,
    Word,
    Byte,
}

impl WordSize {
    pub fn bits(self) -> u32 {
        match self {
            WordSize::Qword => 64,
            WordSize::Dword => 32,
            WordSize::Word => 16,
            WordSize::Byte => 8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            WordSize::Qword => "QWORD",
            WordSize::Dword => "DWORD",
            WordSize::Word => "WORD",
            WordSize::Byte => "BYTE",
        }
    }

    /// Windows cycles QWORD -> DWORD -> WORD -> BYTE -> QWORD.
    pub fn next(self) -> WordSize {
        match self {
            WordSize::Qword => WordSize::Dword,
            WordSize::Dword => WordSize::Word,
            WordSize::Word => WordSize::Byte,
            WordSize::Byte => WordSize::Qword,
        }
    }

    pub fn mask(self) -> u64 {
        if self.bits() == 64 {
            u64::MAX
        } else {
            (1u64 << self.bits()) - 1
        }
    }

    pub fn min(self) -> i128 {
        -(1i128 << (self.bits() - 1))
    }

    pub fn max(self) -> i128 {
        (1i128 << (self.bits() - 1)) - 1
    }

    /// Truncates to the word and sign-extends. Returns the value and whether
    /// information was lost.
    pub fn wrap(self, v: i128) -> (i64, bool) {
        let bits = self.bits();
        let truncated = (v as u128 as u64) & self.mask();
        let shift = 64 - bits;
        let r = ((truncated << shift) as i64) >> shift;
        (r, r as i128 != v)
    }

    /// Bit pattern of `v` within the word.
    pub fn pattern(self, v: i64) -> u64 {
        (v as u64) & self.mask()
    }

    pub fn from_name(s: &str) -> Option<WordSize> {
        [WordSize::Qword, WordSize::Dword, WordSize::Word, WordSize::Byte]
            .into_iter()
            .find(|w| w.label() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Radix {
    Hex,
    #[default]
    Dec,
    Oct,
    Bin,
}

impl Radix {
    pub fn base(self) -> u32 {
        match self {
            Radix::Hex => 16,
            Radix::Dec => 10,
            Radix::Oct => 8,
            Radix::Bin => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Radix::Hex => "HEX",
            Radix::Dec => "DEC",
            Radix::Oct => "OCT",
            Radix::Bin => "BIN",
        }
    }

    pub fn all() -> [Radix; 4] {
        [Radix::Hex, Radix::Dec, Radix::Oct, Radix::Bin]
    }

    pub fn from_name(s: &str) -> Option<Radix> {
        Radix::all().into_iter().find(|r| r.label() == s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ShiftMode {
    #[default]
    Arithmetic,
    Logical,
    Rotate,
}

impl ShiftMode {
    pub fn label(self) -> &'static str {
        match self {
            ShiftMode::Arithmetic => "Arithmetic shift",
            ShiftMode::Logical => "Logical shift",
            ShiftMode::Rotate => "Rotate circular shift",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ShiftMode::Arithmetic => "arithmetic",
            ShiftMode::Logical => "logical",
            ShiftMode::Rotate => "rotate",
        }
    }

    pub fn from_name(s: &str) -> Option<ShiftMode> {
        [ShiftMode::Arithmetic, ShiftMode::Logical, ShiftMode::Rotate]
            .into_iter()
            .find(|m| m.name() == s)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProgCtx {
    pub word: WordSize,
    pub shift: ShiftMode,
}

/// Evaluation result plus an overflow flag.
pub struct ProgValue {
    pub value: i64,
    pub overflow: bool,
}

pub fn eval(n: &Node, ctx: &ProgCtx) -> Result<ProgValue, CalcError> {
    let mut overflow = false;
    let value = eval_inner(n, ctx, &mut overflow)?;
    Ok(ProgValue { value, overflow })
}

fn eval_inner(n: &Node, ctx: &ProgCtx, of: &mut bool) -> Result<i64, CalcError> {
    let w = ctx.word;
    let wrap = |v: i128, of: &mut bool| {
        let (r, lost) = w.wrap(v);
        *of |= lost;
        r
    };
    match n {
        Node::Num(d) => {
            let v = d.to_i128().ok_or(if d.is_integer() {
                CalcError::Overflow
            } else {
                CalcError::InvalidInput
            })?;
            // Literal bit patterns (e.g. FFFF in WORD) are interpreted as signed.
            let (r, lost) = w.wrap(v);
            if lost && !(v >= 0 && (v as u128) <= w.mask() as u128) {
                *of = true;
            }
            Ok(r)
        }
        Node::Group(x) => eval_inner(x, ctx, of),
        Node::Const(_) | Node::Percent(_) => Err(CalcError::InvalidInput),
        Node::Unary(u, x) => {
            let a = eval_inner(x, ctx, of)?;
            let bits = w.bits();
            let p = w.pattern(a);
            Ok(match u {
                Unary::Neg => wrap(-(a as i128), of),
                Unary::Not => wrap(!a as i128, of),
                Unary::Rol => wrap((((p << 1) | (p >> (bits - 1))) & w.mask()) as i128, &mut false),
                Unary::Ror => wrap((((p >> 1) | (p << (bits - 1))) & w.mask()) as i128, &mut false),
                _ => return Err(CalcError::InvalidInput),
            })
        }
        Node::Binary(op, a, b) => {
            let a = eval_inner(a, ctx, of)?;
            let b = eval_inner(b, ctx, of)?;
            binary(*op, a, b, ctx, of)
        }
    }
}

fn binary(op: BinOp, a: i64, b: i64, ctx: &ProgCtx, of: &mut bool) -> Result<i64, CalcError> {
    let w = ctx.word;
    let bits = w.bits() as i64;
    let mut wrap = |v: i128| {
        let (r, lost) = w.wrap(v);
        *of |= lost;
        r
    };
    let quiet = |v: i128| w.wrap(v).0;
    let (ai, bi) = (a as i128, b as i128);
    Ok(match op {
        BinOp::Add => wrap(ai + bi),
        BinOp::Sub => wrap(ai - bi),
        BinOp::Mul => wrap(ai * bi),
        BinOp::Div => {
            if b == 0 {
                return Err(CalcError::DivideByZero);
            }
            wrap(ai / bi)
        }
        BinOp::Mod => {
            if b == 0 {
                return Err(CalcError::DivideByZero);
            }
            wrap(ai % bi)
        }
        BinOp::And => quiet(ai & bi),
        BinOp::Or => quiet(ai | bi),
        BinOp::Xor => quiet(ai ^ bi),
        BinOp::Nand => quiet(!(ai & bi)),
        BinOp::Nor => quiet(!(ai | bi)),
        BinOp::Shl | BinOp::Shr => {
            if b < 0 {
                return Err(CalcError::InvalidInput);
            }
            let left = op == BinOp::Shl;
            match ctx.shift {
                ShiftMode::Rotate => {
                    let n = (b % bits) as u32;
                    let p = w.pattern(a);
                    let bitsu = bits as u32;
                    let r = if n == 0 {
                        p
                    } else if left {
                        ((p << n) | (p >> (bitsu - n))) & w.mask()
                    } else {
                        ((p >> n) | (p << (bitsu - n))) & w.mask()
                    };
                    quiet(r as i128)
                }
                _ if b >= bits => {
                    if left || ctx.shift == ShiftMode::Logical || a >= 0 {
                        0
                    } else {
                        -1
                    }
                }
                _ if left => quiet((w.pattern(a) as u128 as i128) << b),
                ShiftMode::Logical => quiet((w.pattern(a) >> b) as i128),
                ShiftMode::Arithmetic => quiet(ai >> b),
            }
        }
        BinOp::Pow | BinOp::Root | BinOp::LogBase => return Err(CalcError::InvalidInput),
    })
}

/// Formats a value: DEC is signed with thousands separators; the other
/// radixes show the word's bit pattern, grouped (4 for HEX/BIN, 3 for OCT).
pub fn format(v: i64, radix: Radix, word: WordSize) -> String {
    match radix {
        Radix::Dec => crate::format::group_decimal(&v.to_string()),
        Radix::Hex => group_radix(&format!("{:X}", word.pattern(v)), 4, ' '),
        Radix::Oct => group_radix(&format!("{:o}", word.pattern(v)), 3, ' '),
        Radix::Bin => group_radix(&format!("{:b}", word.pattern(v)), 4, ' '),
    }
}

/// Parses digits typed in `radix`. DEC is signed; other radixes are bit
/// patterns. Returns `None` if the value does not fit the word.
pub fn parse_entry(s: &str, radix: Radix, word: WordSize) -> Option<i64> {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, s),
    };
    let mag = u128::from_str_radix(digits, radix.base()).ok()?;
    if radix == Radix::Dec {
        let v = if neg { -(mag as i128) } else { mag as i128 };
        if v > word.max() || v < word.min() {
            return None;
        }
        Some(v as i64)
    } else {
        if mag > word.mask() as u128 {
            return None;
        }
        let (r, _) = word.wrap(mag as i128);
        Some(if neg { word.wrap(-(r as i128)).0 } else { r })
    }
}
