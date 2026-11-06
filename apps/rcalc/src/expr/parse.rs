//! Tokenizer and Pratt parser.
//!
//! The keypad engines feed already-built tokens (`Token::Operand`) to the
//! same parser that handles pasted text, so precedence rules are defined in
//! exactly one place.

use super::ast::{BinOp, Constant, Node, Unary, POSTFIX_BP, PREFIX_BP};
use crate::num::func::Trig;
use crate::num::{CalcError, Dec};
use num_bigint::BigUint;

#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Operand(Node),
    Bin(BinOp),
    /// `+` / `-` from text: binary or unary depending on position.
    Plus,
    Minus,
    LParen,
    RParen,
    Prefix(Unary),
    Postfix(Unary),
    Percent,
}

/// How numbers and ambiguous symbols are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexMode {
    /// Decimal numbers, `^` is power.
    Decimal,
    /// Integers in the given radix, `^` is XOR.
    Programmer(u32),
}

pub fn tokenize(src: &str, mode: LexMode) -> Result<Vec<Token>, CalcError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        match mode {
            LexMode::Decimal if c.is_ascii_digit() || c == '.' => {
                let (n, next) = lex_decimal(&chars, i)?;
                out.push(Token::Operand(Node::Num(n)));
                i = next;
                continue;
            }
            LexMode::Programmer(radix) if c.is_ascii_alphanumeric() => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                // Merge digit groups separated by single spaces ("1111 0000"),
                // but only when both sides are made of digits ("BAD AND F" is
                // a number, an operator and a number).
                while i + 1 < chars.len() && chars[i] == ' ' && chars[start..i].iter().all(|c| c.is_digit(radix) || *c == ' ') {
                    let mut j = i + 1;
                    while j < chars.len() && chars[j].is_ascii_alphanumeric() {
                        j += 1;
                    }
                    if j == i + 1 || !chars[i + 1..j].iter().all(|c| c.is_digit(radix)) {
                        break;
                    }
                    i = j;
                }
                let word: String = chars[start..i].iter().filter(|c| **c != ' ' && **c != '_').collect();
                if let Some(v) = parse_radix_literal(&word, radix) {
                    out.push(Token::Operand(Node::Num(Dec::from_parts(false, v, 0))));
                } else {
                    out.push(word_token(&word.to_ascii_lowercase(), mode)?);
                }
                continue;
            }
            _ => {}
        }
        if c.is_alphabetic() {
            let start = i;
            while i < chars.len() && (chars[i].is_alphabetic() || chars[i] == '⁻' || chars[i] == '¹') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect::<String>().to_lowercase();
            out.push(word_token(&word, mode)?);
            continue;
        }
        let two: String = chars[i..chars.len().min(i + 2)].iter().collect();
        if two == "<<" || two == "**" || two == ">>" {
            out.push(match two.as_str() {
                "<<" => Token::Bin(BinOp::Shl),
                ">>" => Token::Bin(BinOp::Shr),
                _ => Token::Bin(BinOp::Pow),
            });
            i += 2;
            continue;
        }
        let tok = match c {
            '+' => Token::Plus,
            '-' | '−' | '–' => Token::Minus,
            '*' | '×' | '·' | '∙' => Token::Bin(BinOp::Mul),
            '/' | '÷' => Token::Bin(BinOp::Div),
            '^' => match mode {
                LexMode::Decimal => Token::Bin(BinOp::Pow),
                LexMode::Programmer(_) => Token::Bin(BinOp::Xor),
            },
            '%' => match mode {
                LexMode::Decimal => Token::Percent,
                LexMode::Programmer(_) => Token::Bin(BinOp::Mod),
            },
            '&' => Token::Bin(BinOp::And),
            '|' => Token::Bin(BinOp::Or),
            '~' => Token::Prefix(Unary::Not),
            '!' => Token::Postfix(Unary::Fact),
            '²' => Token::Postfix(Unary::Square),
            '³' => Token::Postfix(Unary::Cube),
            '√' => Token::Prefix(Unary::Sqrt),
            '∛' => Token::Prefix(Unary::Cbrt),
            'π' => Token::Operand(Node::Const(Constant::Pi)),
            '(' | '[' => Token::LParen,
            ')' | ']' => Token::RParen,
            '=' if i == chars.len() - 1 => {
                i += 1;
                continue;
            }
            _ => return Err(CalcError::InvalidInput),
        };
        out.push(tok);
        i += 1;
    }
    Ok(out)
}

fn parse_radix_literal(word: &str, radix: u32) -> Option<BigUint> {
    let lower = word.to_ascii_lowercase();
    let (digits, r) = if let Some(h) = lower.strip_prefix("0x") {
        (h.to_string(), 16)
    } else if let Some(b) = lower.strip_prefix("0b").filter(|_| radix != 16) {
        (b.to_string(), 2)
    } else if let Some(o) = lower.strip_prefix("0o") {
        (o.to_string(), 8)
    } else {
        (lower, radix)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(r)) {
        return None;
    }
    BigUint::parse_bytes(digits.as_bytes(), r)
}

/// Reads `123`, `1,234.5`, `.5`, `1e-7`; `e` without exponent digits is left
/// for the constant.
fn lex_decimal(chars: &[char], start: usize) -> Result<(Dec, usize), CalcError> {
    let mut s = String::new();
    let mut i = start;
    let mut seen_point = false;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            s.push(c);
        } else if c == '.' && !seen_point {
            seen_point = true;
            s.push(c);
        } else if c == ',' && !seen_point && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            // thousands separator
        } else {
            break;
        }
        i += 1;
    }
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        let mut j = i + 1;
        if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
            j += 1;
        }
        if j < chars.len() && chars[j].is_ascii_digit() {
            s.push('e');
            s.extend(&chars[i + 1..j]);
            while j < chars.len() && chars[j].is_ascii_digit() {
                s.push(chars[j]);
                j += 1;
            }
            i = j;
        }
    }
    if s == "." {
        return Err(CalcError::InvalidInput);
    }
    Ok((s.parse()?, i))
}

fn word_token(word: &str, mode: LexMode) -> Result<Token, CalcError> {
    use Trig::*;
    let trig = |f, inv, hyp| Token::Prefix(Unary::Trig { f, inv, hyp });
    let programmer = matches!(mode, LexMode::Programmer(_));
    Ok(match word {
        "pi" | "π" if !programmer => Token::Operand(Node::Const(Constant::Pi)),
        "e" if !programmer => Token::Operand(Node::Const(Constant::E)),
        "mod" => Token::Bin(BinOp::Mod),
        "yroot" => Token::Bin(BinOp::Root),
        "and" => Token::Bin(BinOp::And),
        "or" => Token::Bin(BinOp::Or),
        "xor" => Token::Bin(BinOp::Xor),
        "nand" => Token::Bin(BinOp::Nand),
        "nor" => Token::Bin(BinOp::Nor),
        "lsh" => Token::Bin(BinOp::Shl),
        "rsh" => Token::Bin(BinOp::Shr),
        "not" => Token::Prefix(Unary::Not),
        "negate" => Token::Prefix(Unary::Neg),
        _ if programmer => return Err(CalcError::InvalidInput),
        "sqrt" => Token::Prefix(Unary::Sqrt),
        "cbrt" | "cuberoot" => Token::Prefix(Unary::Cbrt),
        "sqr" => Token::Prefix(Unary::Square),
        "cube" => Token::Prefix(Unary::Cube),
        "abs" => Token::Prefix(Unary::Abs),
        "fact" => Token::Prefix(Unary::Fact),
        "ln" => Token::Prefix(Unary::Ln),
        "log" => Token::Prefix(Unary::Log),
        "exp" => Token::Prefix(Unary::Exp),
        "floor" => Token::Prefix(Unary::Floor),
        "ceil" => Token::Prefix(Unary::Ceil),
        "dms" => Token::Prefix(Unary::ToDms),
        "degrees" => Token::Prefix(Unary::FromDms),
        _ => {
            let (base, inv) = if let Some(b) = word.strip_suffix("⁻¹") {
                (b, true)
            } else if let Some(b) = word.strip_prefix("arc") {
                (b, true)
            } else if let Some(b) = word.strip_prefix('a').filter(|b| b.len() >= 3) {
                (b, true)
            } else {
                (word, false)
            };
            let (base, hyp) = match base.strip_suffix('h') {
                Some(b) if b.len() == 3 => (b, true),
                _ => (base, false),
            };
            let f = match base {
                "sin" => Sin,
                "cos" => Cos,
                "tan" => Tan,
                "sec" => Sec,
                "csc" => Csc,
                "cot" => Cot,
                _ => return Err(CalcError::InvalidInput),
            };
            trig(f, inv, hyp)
        }
    })
}

/// Parses a token stream into an expression tree. Missing closing
/// parentheses at the end are closed implicitly.
pub fn parse(tokens: &[Token]) -> Result<Node, CalcError> {
    if tokens.is_empty() {
        return Err(CalcError::InvalidInput);
    }
    let mut p = Parser { t: tokens, i: 0 };
    let node = p.expr(0)?;
    if p.i != tokens.len() {
        return Err(CalcError::InvalidInput);
    }
    Ok(node)
}

/// Tokenizes and parses text.
pub fn parse_str(src: &str, mode: LexMode) -> Result<Node, CalcError> {
    parse(&tokenize(src, mode)?)
}

struct Parser<'a> {
    t: &'a [Token],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.t.get(self.i)
    }

    fn next(&mut self) -> Option<&Token> {
        let t = self.t.get(self.i);
        self.i += 1;
        t
    }

    fn expr(&mut self, min_bp: u8) -> Result<Node, CalcError> {
        let mut lhs = self.prefix()?;
        while let Some(tok) = self.peek() {
            match tok {
                Token::RParen => break,
                Token::Postfix(u) => {
                    if POSTFIX_BP < min_bp {
                        break;
                    }
                    let u = *u;
                    self.i += 1;
                    lhs = Node::unary(u, lhs);
                }
                Token::Percent => {
                    if POSTFIX_BP < min_bp {
                        break;
                    }
                    self.i += 1;
                    lhs = Node::Percent(Box::new(lhs));
                }
                Token::Bin(_) | Token::Plus | Token::Minus => {
                    let op = match tok {
                        Token::Bin(op) => *op,
                        Token::Plus => BinOp::Add,
                        _ => BinOp::Sub,
                    };
                    let (l, r) = op.binding();
                    if l < min_bp {
                        break;
                    }
                    self.i += 1;
                    let rhs = self.expr(r)?;
                    lhs = Node::binary(op, lhs, rhs);
                }
                Token::Operand(_) | Token::LParen | Token::Prefix(_) => {
                    // Implicit multiplication: `2π`, `3(4+5)`, `2 sin 30`.
                    let (l, r) = BinOp::Mul.binding();
                    if l < min_bp {
                        break;
                    }
                    let rhs = self.expr(r)?;
                    lhs = Node::binary(BinOp::Mul, lhs, rhs);
                }
            }
        }
        Ok(lhs)
    }

    fn prefix(&mut self) -> Result<Node, CalcError> {
        match self.next().cloned() {
            Some(Token::Operand(n)) => Ok(n),
            Some(Token::LParen) => {
                let inner = self.expr(0)?;
                match self.peek() {
                    Some(Token::RParen) => {
                        self.i += 1;
                    }
                    None => {}
                    Some(_) => return Err(CalcError::InvalidInput),
                }
                Ok(Node::Group(Box::new(inner)))
            }
            Some(Token::Minus) => {
                let operand = self.expr(PREFIX_BP)?;
                Ok(match operand {
                    Node::Num(d) => Node::Num(d.neg()),
                    other => Node::unary(Unary::Neg, other),
                })
            }
            Some(Token::Plus) => self.expr(PREFIX_BP),
            Some(Token::Prefix(u)) => Ok(Node::unary(u, self.expr(PREFIX_BP)?)),
            _ => Err(CalcError::InvalidInput),
        }
    }
}

/// For immediate display in Scientific/Programmer mode: the part of
/// `tokens` (ending with an operand) that is complete once `next` is
/// appended, honoring precedence. `2 + 3 × 4` followed by `+` gives the whole
/// expression; followed by `×` gives just `3 × 4`.
pub fn pending_operand(tokens: &[Token], next: BinOp) -> Option<Node> {
    let start = tokens.iter().rposition(|t| matches!(t, Token::LParen)).map_or(0, |i| i + 1);
    let seg = &tokens[start..];
    if seg.is_empty() {
        return None;
    }
    let mut idx = seg.len() - 1;
    let np = next.precedence();
    while idx >= 2 {
        match &seg[idx - 1] {
            Token::Bin(op) if op.precedence() > np || (op.precedence() == np && !next.right_assoc()) => idx -= 2,
            _ => break,
        }
    }
    parse(&seg[idx..]).ok()
}
