//! Evaluation of expression trees with decimal arithmetic
//! (Standard and Scientific modes, converters).

use super::ast::{BinOp, Constant, Node, Unary};
use crate::num::func::{self, Angle};
use crate::num::{CalcError, CalcResult, Dec, PREC};

#[derive(Clone, Copy, Debug, Default)]
pub struct Ctx {
    pub angle: Angle,
}

pub fn eval(n: &Node, ctx: &Ctx) -> CalcResult {
    let v = match n {
        Node::Num(d) => Ok(d.clone()),
        Node::Const(Constant::Pi) => Ok(func::pi()),
        Node::Const(Constant::E) => Ok(func::euler()),
        Node::Group(x) => eval(x, ctx),
        Node::Percent(x) => Ok(eval(x, ctx)?.div_p(&Dec::from_i64(100), PREC)),
        Node::Unary(u, x) => unary(*u, &eval(x, ctx)?, ctx),
        Node::Binary(op @ (BinOp::Add | BinOp::Sub), a, b) if matches!(**b, Node::Percent(_)) => {
            // a ± b%  means  a ± (a × b / 100)
            let Node::Percent(inner) = &**b else { unreachable!() };
            let av = eval(a, ctx)?;
            let pct = av.mul(&eval(inner, ctx)?).div_p(&Dec::from_i64(100), PREC);
            binary(*op, &av, &pct)
        }
        Node::Binary(op, a, b) => binary(*op, &eval(a, ctx)?, &eval(b, ctx)?),
    }?;
    v.check()
}

/// Addition that snaps catastrophic cancellation to zero: if the result is
/// smaller than the operands by more than the working precision, the
/// remaining digits are rounding noise (e.g. `sqrt(2)^2 - 2`).
pub fn add_snap(a: &Dec, b: &Dec) -> Dec {
    let r = a.add(b);
    if !r.is_zero() && !a.is_zero() && !b.is_zero() {
        let big = a.adjusted_exp().max(b.adjusted_exp());
        if r.adjusted_exp() < big - (PREC as i64 - 4) {
            return Dec::zero();
        }
    }
    r
}

pub fn binary(op: BinOp, a: &Dec, b: &Dec) -> CalcResult {
    match op {
        BinOp::Add => Ok(add_snap(a, b)),
        BinOp::Sub => Ok(add_snap(a, &b.neg())),
        BinOp::Mul => Ok(a.mul(b)),
        BinOp::Div => a.div(b),
        BinOp::Mod => a.rem_floor(b),
        BinOp::Pow => func::pow(a, b),
        BinOp::Root => func::root(a, b),
        BinOp::LogBase => func::log_base(a, b),
        _ => Err(CalcError::InvalidInput),
    }
}

pub fn unary(u: Unary, x: &Dec, ctx: &Ctx) -> CalcResult {
    use Unary::*;
    match u {
        Neg => Ok(x.neg()),
        Square => Ok(x.mul(x)),
        Cube => Ok(x.mul(x).mul(x)),
        Sqrt => func::sqrt(x),
        Cbrt => func::root(x, &Dec::from_i64(3)),
        Recip => {
            if x.is_zero() {
                Err(CalcError::DivideByZero)
            } else {
                Dec::one().div(x)
            }
        }
        Abs => Ok(x.abs()),
        Fact => func::factorial(x),
        Pow10 => func::pow(&Dec::from_i64(10), x),
        Pow2 => func::pow(&Dec::from_i64(2), x),
        Exp => func::exp(x),
        Log => func::log10(x),
        Ln => func::ln(x),
        Floor => Ok(x.floor()),
        Ceil => Ok(x.ceil()),
        ToDms => func::to_dms(x),
        FromDms => func::from_dms(x),
        Trig { f, inv, hyp } => match (inv, hyp) {
            (false, false) => func::trig(f, x, ctx.angle),
            (true, false) => func::inverse_trig(f, x, ctx.angle),
            (false, true) => func::hyperbolic(f, x),
            (true, true) => func::inverse_hyperbolic(f, x),
        },
        Not | Rol | Ror => Err(CalcError::InvalidInput),
    }
}
