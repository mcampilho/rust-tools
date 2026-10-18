//! Expression tree shared by every calculator mode.

use crate::num::func::Trig;
use crate::num::Dec;

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Num(Dec),
    Const(Constant),
    Unary(Unary, Box<Node>),
    Binary(BinOp, Box<Node>, Box<Node>),
    /// `x%`: `x / 100`, or a percentage of the left operand of `+`/`-`.
    Percent(Box<Node>),
    /// Explicit parentheses (kept for display).
    Group(Box<Node>),
}

impl Node {
    pub fn unary(u: Unary, n: Node) -> Node {
        Node::Unary(u, Box::new(n))
    }

    pub fn binary(op: BinOp, a: Node, b: Node) -> Node {
        Node::Binary(op, Box::new(a), Box::new(b))
    }

    /// Renders the expression the way the history/expression line shows it.
    pub fn render(&self, out: &mut String, fmt_num: &dyn Fn(&Dec) -> String) {
        match self {
            Node::Num(d) => out.push_str(&fmt_num(d)),
            Node::Const(Constant::Pi) => out.push('π'),
            Node::Const(Constant::E) => out.push('e'),
            Node::Group(inner) => {
                out.push('(');
                inner.render(out, fmt_num);
                out.push(')');
            }
            Node::Percent(inner) => {
                inner.render(out, fmt_num);
                out.push('%');
            }
            Node::Unary(u, inner) => {
                out.push_str(u.name());
                out.push('(');
                match &**inner {
                    Node::Group(g) => g.render(out, fmt_num),
                    other => other.render(out, fmt_num),
                }
                out.push(')');
            }
            Node::Binary(op, a, b) => {
                a.render(out, fmt_num);
                out.push(' ');
                out.push_str(op.symbol());
                out.push(' ');
                b.render(out, fmt_num);
            }
        }
    }

    pub fn to_text(&self, fmt_num: &dyn Fn(&Dec) -> String) -> String {
        let mut s = String::new();
        self.render(&mut s, fmt_num);
        s
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Constant {
    Pi,
    E,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unary {
    Neg,
    Square,
    Cube,
    Sqrt,
    Cbrt,
    Recip,
    Abs,
    Fact,
    Pow10,
    Pow2,
    Exp,
    Log,
    Ln,
    Floor,
    Ceil,
    Trig { f: Trig, inv: bool, hyp: bool },
    ToDms,
    FromDms,
    // programmer
    Not,
    Rol,
    Ror,
}

impl Unary {
    pub fn name(self) -> &'static str {
        use Unary::*;
        match self {
            Neg => "negate",
            Square => "sqr",
            Cube => "cube",
            Sqrt => "√",
            Cbrt => "cuberoot",
            Recip => "1/",
            Abs => "abs",
            Fact => "fact",
            Pow10 => "10^",
            Pow2 => "2^",
            Exp => "e^",
            Log => "log",
            Ln => "ln",
            Floor => "floor",
            Ceil => "ceil",
            ToDms => "dms",
            FromDms => "degrees",
            Not => "NOT",
            Rol => "RoL",
            Ror => "RoR",
            Trig { f, inv, hyp } => trig_name(f, inv, hyp),
        }
    }
}

pub fn trig_name(f: Trig, inv: bool, hyp: bool) -> &'static str {
    use Trig::*;
    match (f, inv, hyp) {
        (Sin, false, false) => "sin",
        (Cos, false, false) => "cos",
        (Tan, false, false) => "tan",
        (Sec, false, false) => "sec",
        (Csc, false, false) => "csc",
        (Cot, false, false) => "cot",
        (Sin, true, false) => "sin⁻¹",
        (Cos, true, false) => "cos⁻¹",
        (Tan, true, false) => "tan⁻¹",
        (Sec, true, false) => "sec⁻¹",
        (Csc, true, false) => "csc⁻¹",
        (Cot, true, false) => "cot⁻¹",
        (Sin, false, true) => "sinh",
        (Cos, false, true) => "cosh",
        (Tan, false, true) => "tanh",
        (Sec, false, true) => "sech",
        (Csc, false, true) => "csch",
        (Cot, false, true) => "coth",
        (Sin, true, true) => "sinh⁻¹",
        (Cos, true, true) => "cosh⁻¹",
        (Tan, true, true) => "tanh⁻¹",
        (Sec, true, true) => "sech⁻¹",
        (Csc, true, true) => "csch⁻¹",
        (Cot, true, true) => "coth⁻¹",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    /// `x yroot y` = y-th root of x
    Root,
    /// `x log base y`
    LogBase,
    And,
    Or,
    Xor,
    Nand,
    Nor,
    Shl,
    Shr,
}

impl BinOp {
    /// Pratt binding powers `(left, right)`. Higher binds tighter; a right
    /// power below the left one makes the operator right-associative.
    ///
    /// The bitwise ordering follows C: `|` < `^` < `&` < shifts < `+ -` < `* / %`.
    pub fn binding(self) -> (u8, u8) {
        use BinOp::*;
        match self {
            Or | Nor => (1, 2),
            Xor => (3, 4),
            And | Nand => (5, 6),
            Shl | Shr => (7, 8),
            Add | Sub => (9, 10),
            Mul | Div | Mod => (11, 12),
            Pow | Root | LogBase => (15, 14),
        }
    }

    pub fn precedence(self) -> u8 {
        self.binding().0
    }

    pub fn right_assoc(self) -> bool {
        let (l, r) = self.binding();
        r < l
    }

    pub fn symbol(self) -> &'static str {
        use BinOp::*;
        match self {
            Add => "+",
            Sub => "−",
            Mul => "×",
            Div => "÷",
            Mod => "mod",
            Pow => "^",
            Root => "yroot",
            LogBase => "log base",
            And => "AND",
            Or => "OR",
            Xor => "XOR",
            Nand => "NAND",
            Nor => "NOR",
            Shl => "Lsh",
            Shr => "Rsh",
        }
    }
}

/// Binding power used for the operand of a prefix minus or function.
pub const PREFIX_BP: u8 = 13;
/// Binding power of postfix operators (`!`, `²`, `%`).
pub const POSTFIX_BP: u8 = 17;
