//! Keypad-driven calculator engine for Standard, Scientific and Programmer
//! modes.
//!
//! Button presses build a token list that is evaluated with the shared
//! expression parser. Standard mode collapses the pending expression after
//! every operator (immediate execution, like Windows Calculator); the other
//! modes keep the whole expression and honor operator precedence.

pub mod programmer;

use crate::expr::eval::{self, Ctx};
use crate::expr::{parse, pending_operand, BinOp, Constant, Node, Token, Unary};
use crate::format;
use crate::num::func::{self, Angle};
use crate::num::{CalcError, Dec};
use programmer::{ProgCtx, Radix, ShiftMode, WordSize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Digit(u8),
    Point,
    Bin(BinOp),
    Unary(Unary),
    /// Standard-mode percent.
    Percent,
    Equals,
    Clear,
    ClearEntry,
    Back,
    Negate,
    LParen,
    RParen,
    Const(Constant),
    /// Scientific-notation entry (`exp` key).
    Exp,
    Rand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Standard,
    Scientific,
    Programmer,
}

impl Kind {
    /// Maximum number of digits that can be typed / displayed.
    pub fn max_digits(self) -> usize {
        match self {
            Kind::Standard => 16,
            Kind::Scientific => 32,
            Kind::Programmer => 64,
        }
    }
}

/// An entry produced by `=`.
#[derive(Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    pub expression: String,
    pub result: String,
    pub value: Dec,
}

#[derive(Clone, Debug)]
enum Current {
    /// Waiting for an operand; the display shows `Engine::display`.
    None,
    /// Digits being typed.
    Typing(String),
    /// A computed operand (result, function application, constant, recall).
    Value { node: Node, value: Dec },
}

#[derive(Clone, Debug)]
pub struct Engine {
    pub kind: Kind,
    tokens: Vec<Token>,
    current: Current,
    display: Dec,
    error: Option<CalcError>,
    /// Last operator and right operand, for repeated `=`.
    last: Option<(BinOp, Dec)>,
    /// Expression line after `=` (e.g. `2 + 3 =`).
    equals_expr: Option<String>,
    open_parens: usize,
    pub angle: Angle,
    pub fe: bool,
    pub word: WordSize,
    pub radix: Radix,
    pub shift: ShiftMode,
    /// Set when a programmer-mode result wrapped around the word size.
    pub overflowed: bool,
}

impl Engine {
    pub fn new(kind: Kind) -> Engine {
        Engine {
            kind,
            tokens: Vec::new(),
            current: Current::None,
            display: Dec::zero(),
            error: None,
            last: None,
            equals_expr: None,
            open_parens: 0,
            angle: Angle::Deg,
            fe: false,
            word: WordSize::Qword,
            radix: Radix::Dec,
            shift: ShiftMode::Arithmetic,
            overflowed: false,
        }
    }

    pub fn error(&self) -> Option<CalcError> {
        self.error
    }

    pub fn open_parens(&self) -> usize {
        self.open_parens
    }

    pub fn is_typing(&self) -> bool {
        matches!(self.current, Current::Typing(_))
    }

    fn prog_ctx(&self) -> ProgCtx {
        ProgCtx {
            word: self.word,
            shift: self.shift,
        }
    }

    /// Evaluates a tree with this mode's semantics.
    fn eval(&mut self, n: &Node) -> Result<Dec, CalcError> {
        match self.kind {
            Kind::Programmer => {
                let r = programmer::eval(n, &self.prog_ctx())?;
                self.overflowed |= r.overflow;
                Ok(Dec::from_i64(r.value))
            }
            _ => eval::eval(n, &Ctx { angle: self.angle }),
        }
    }

    fn parse_typing(&self, s: &str) -> Dec {
        match self.kind {
            Kind::Programmer => Dec::from_i64(programmer::parse_entry(s, self.radix, self.word).unwrap_or(0)),
            _ => {
                let t = s.trim_end_matches(['+', '-']).trim_end_matches('e');
                t.parse().unwrap_or_else(|_| Dec::zero())
            }
        }
    }

    /// The value currently shown (what MS / copy / unary functions act on).
    pub fn value(&self) -> Dec {
        match &self.current {
            Current::None => self.display.clone(),
            Current::Typing(s) => self.parse_typing(s),
            Current::Value { value, .. } => value.clone(),
        }
    }

    /// Value as a programmer-mode integer.
    pub fn int_value(&self) -> i64 {
        self.value().to_i64().unwrap_or(0)
    }

    fn take_operand(&mut self) -> Node {
        let cur = std::mem::replace(&mut self.current, Current::None);
        match cur {
            Current::None => Node::Num(self.display.clone()),
            Current::Typing(s) => Node::Num(self.parse_typing(&s)),
            Current::Value { node, .. } => node,
        }
    }

    fn set_error(&mut self, e: CalcError) {
        self.error = Some(e);
        self.current = Current::None;
    }

    fn reset(&mut self) {
        self.tokens.clear();
        self.current = Current::None;
        self.display = Dec::zero();
        self.error = None;
        self.last = None;
        self.equals_expr = None;
        self.open_parens = 0;
        self.overflowed = false;
    }

    /// Replaces the current operand (memory recall, history, paste).
    pub fn set_value(&mut self, v: Dec) {
        if self.error.is_some() {
            self.reset();
        }
        if self.equals_expr.take().is_some() {
            self.tokens.clear();
        }
        let v = if self.kind == Kind::Programmer {
            Dec::from_i64(self.word.wrap(v.trunc().to_i128().unwrap_or(0)).0)
        } else {
            v
        };
        self.current = Current::Value {
            node: Node::Num(v.clone()),
            value: v,
        };
    }

    /// Ends digit entry so the next digit starts a new number (after MS, M+, M-).
    pub fn commit(&mut self) {
        if let Current::Typing(t) = &self.current {
            let v = self.parse_typing(t);
            self.current = Current::Value {
                node: Node::Num(v.clone()),
                value: v,
            };
        }
    }

    /// Restores a history entry: its result becomes the current value and
    /// its expression is shown.
    pub fn restore(&mut self, entry: &HistoryEntry) {
        self.reset();
        self.current = Current::Value {
            node: Node::Num(entry.value.clone()),
            value: entry.value.clone(),
        };
        self.equals_expr = Some(entry.expression.clone());
    }

    /// Loads a pasted expression or number. Returns false if it can't be parsed.
    pub fn paste(&mut self, text: &str) -> bool {
        let text = text.trim();
        let mode = match self.kind {
            Kind::Programmer => crate::expr::LexMode::Programmer(self.radix.base()),
            _ => crate::expr::LexMode::Decimal,
        };
        let Ok(node) = crate::expr::parse_str(text, mode) else {
            return false;
        };
        match self.eval(&node) {
            Ok(v) => {
                if self.error.is_some() || self.equals_expr.is_some() {
                    self.reset();
                }
                self.current = Current::Value {
                    node: Node::Num(v.clone()),
                    value: v,
                };
                true
            }
            Err(_) => false,
        }
    }

    /// Changes the programmer word size, truncating the current value.
    pub fn set_word(&mut self, w: WordSize) {
        self.word = w;
        if self.error.is_none() {
            let v = self.value();
            let wrapped = Dec::from_i64(w.wrap(v.to_i128().unwrap_or(0)).0);
            match &self.current {
                Current::None => self.display = wrapped,
                _ => {
                    self.current = Current::Value {
                        node: Node::Num(wrapped.clone()),
                        value: wrapped,
                    }
                }
            }
        }
    }

    pub fn set_radix(&mut self, r: Radix) {
        if let Current::Typing(s) = &self.current {
            let v = self.parse_typing(s);
            self.current = Current::Value {
                node: Node::Num(v.clone()),
                value: v,
            };
        }
        self.radix = r;
    }

    /// Toggles bit `bit` of the current programmer value.
    pub fn toggle_bit(&mut self, bit: u32) {
        if self.error.is_some() {
            self.reset();
        }
        if bit >= self.word.bits() {
            return;
        }
        if self.equals_expr.take().is_some() {
            self.tokens.clear();
        }
        let v = self.int_value() ^ (1i64 << bit);
        let v = Dec::from_i64(self.word.wrap(self.word.pattern(v) as i128).0);
        self.current = Current::Value {
            node: Node::Num(v.clone()),
            value: v,
        };
    }

    /// Handles a key press; returns a history entry when `=` completes a calculation.
    pub fn press(&mut self, key: Key) -> Option<HistoryEntry> {
        if self.error.is_some() {
            match key {
                Key::Digit(_)
                | Key::Point
                | Key::Clear
                | Key::ClearEntry
                | Key::Equals
                | Key::Back
                | Key::Const(_)
                | Key::Rand
                | Key::LParen => {
                    self.reset();
                    if matches!(key, Key::Clear | Key::ClearEntry | Key::Equals | Key::Back) {
                        return None;
                    }
                }
                _ => return None,
            }
        }
        match key {
            Key::Digit(d) => self.digit(d),
            Key::Point => self.point(),
            Key::Exp => self.exp_key(),
            Key::Bin(op) => self.binary(op),
            Key::Unary(u) => self.unary(u),
            Key::Percent => self.percent(),
            Key::Negate => self.negate(),
            Key::Equals => return self.equals(),
            Key::Clear => self.reset(),
            Key::ClearEntry => {
                if self.equals_expr.take().is_some() {
                    self.tokens.clear();
                }
                self.current = Current::Typing("0".into());
            }
            Key::Back => self.back(),
            Key::LParen => self.lparen(),
            Key::RParen => self.rparen(),
            Key::Const(c) => {
                self.start_operand();
                let node = Node::Const(c);
                match self.eval(&node) {
                    Ok(value) => self.current = Current::Value { node, value },
                    Err(e) => self.set_error(e),
                }
            }
            Key::Rand => {
                self.start_operand();
                let v = func::random();
                self.current = Current::Value {
                    node: Node::Num(v.clone()),
                    value: v,
                };
            }
        }
        None
    }

    /// Prepares to replace the current operand with a new one.
    fn start_operand(&mut self) {
        if self.equals_expr.take().is_some() {
            self.tokens.clear();
            self.open_parens = 0;
        }
    }

    fn typed_digits(s: &str) -> usize {
        let mant = s.split('e').next().unwrap_or("");
        let digits = mant.chars().filter(|c| c.is_ascii_alphanumeric()).count();
        // A leading "0." does not count toward the limit.
        if mant.trim_start_matches('-').starts_with("0.") {
            digits.saturating_sub(1)
        } else {
            digits
        }
    }

    fn digit(&mut self, d: u8) {
        let ch = std::char::from_digit(d as u32, 16).unwrap_or('0').to_ascii_uppercase();
        if self.kind == Kind::Programmer && d as u32 >= self.radix.base() {
            return;
        }
        if let Current::Typing(s) = &mut self.current {
            if let Some(epos) = s.find('e') {
                if s.len() - epos - 2 >= 4 {
                    return;
                }
                s.push(ch);
                return;
            }
            let body = s.trim_start_matches('-');
            let candidate = if body == "0" {
                format!("{}{}", if s.starts_with('-') { "-" } else { "" }, ch)
            } else {
                format!("{s}{ch}")
            };
            let ok = match self.kind {
                Kind::Programmer => programmer::parse_entry(&candidate, self.radix, self.word).is_some(),
                k => Self::typed_digits(&candidate) <= k.max_digits(),
            };
            if ok {
                *s = candidate;
            }
            return;
        }
        self.start_operand();
        self.current = Current::Typing(ch.to_string());
    }

    fn point(&mut self) {
        if self.kind == Kind::Programmer {
            return;
        }
        if let Current::Typing(s) = &mut self.current {
            if !s.contains('.') && !s.contains('e') {
                s.push('.');
            }
            return;
        }
        self.start_operand();
        self.current = Current::Typing("0.".into());
    }

    fn exp_key(&mut self) {
        if self.kind == Kind::Programmer {
            return;
        }
        match &mut self.current {
            Current::Typing(s) => {
                if !s.contains('e') {
                    s.push_str("e+");
                }
            }
            _ => {
                let v = self.value();
                let plain = v.format(self.kind.max_digits(), false);
                let base = if plain.contains('e') { "0".to_string() } else { plain };
                self.start_operand();
                self.current = Current::Typing(format!("{base}e+"));
            }
        }
    }

    fn binary(&mut self, op: BinOp) {
        let after_equals = self.equals_expr.take().is_some();
        if matches!(self.current, Current::None) && !after_equals {
            if let Some(Token::Bin(prev)) = self.tokens.last_mut() {
                // Pressing another operator replaces the pending one.
                *prev = op;
                self.update_pending_display(op);
                return;
            }
        }
        let operand = self.take_operand();
        if after_equals {
            self.tokens.clear();
        }
        self.tokens.push(Token::Operand(operand));
        if self.kind == Kind::Standard && self.tokens.len() >= 3 {
            match parse(&self.tokens).and_then(|n| self.eval(&n)) {
                Ok(v) => {
                    self.tokens = vec![Token::Operand(Node::Num(v.clone()))];
                    self.display = v;
                }
                Err(e) => {
                    self.tokens.push(Token::Bin(op));
                    self.set_error(e);
                    return;
                }
            }
        }
        self.tokens.push(Token::Bin(op));
        self.update_pending_display(op);
    }

    /// Shows the value that is already determined once `op` is pending.
    fn update_pending_display(&mut self, op: BinOp) {
        let n = self.tokens.len();
        if n < 2 {
            return;
        }
        if let Some(node) = pending_operand(&self.tokens[..n - 1], op) {
            match self.eval(&node) {
                Ok(v) => self.display = v,
                Err(e) => self.set_error(e),
            }
        }
    }

    fn unary(&mut self, u: Unary) {
        let after_equals = self.equals_expr.take().is_some();
        if after_equals {
            self.tokens.clear();
        }
        let node = Node::unary(u, self.take_operand());
        match self.eval(&node) {
            Ok(value) => self.current = Current::Value { node, value },
            Err(e) => {
                self.current = Current::Value { node, value: Dec::zero() };
                self.error = Some(e);
            }
        }
    }

    fn percent(&mut self) {
        let v = self.value();
        let n = self.tokens.len();
        let r = if n >= 2 {
            if let (Token::Operand(a), Token::Bin(op)) = (&self.tokens[n - 2], &self.tokens[n - 1]) {
                let (a, op) = (a.clone(), *op);
                let av = match self.eval(&a) {
                    Ok(x) => x,
                    Err(e) => return self.set_error(e),
                };
                let hundred = Dec::from_i64(100);
                match op {
                    BinOp::Add | BinOp::Sub => av.mul(&v).div(&hundred),
                    _ => v.div(&hundred),
                }
            } else {
                Ok(Dec::zero())
            }
        } else {
            // A lone percentage has no base value (Windows shows 0).
            Ok(Dec::zero())
        };
        match r {
            Ok(r) => {
                self.start_operand();
                self.current = Current::Value {
                    node: Node::Num(r.clone()),
                    value: r,
                };
            }
            Err(e) => self.set_error(e),
        }
    }

    fn negate(&mut self) {
        if let Current::Typing(s) = &mut self.current {
            if self.kind != Kind::Programmer {
                if let Some(epos) = s.find('e') {
                    let sign = if &s[epos + 1..epos + 2] == "+" { "-" } else { "+" };
                    s.replace_range(epos + 1..epos + 2, sign);
                    return;
                }
                if let Some(rest) = s.strip_prefix('-') {
                    *s = rest.to_string();
                } else if s.as_str() != "0" {
                    s.insert(0, '-');
                }
                return;
            }
        }
        self.unary(Unary::Neg);
    }

    fn back(&mut self) {
        match &mut self.current {
            Current::Typing(s) => {
                if s.ends_with("e+") || s.ends_with("e-") {
                    s.truncate(s.len() - 2);
                } else {
                    s.pop();
                }
                if s.is_empty() || s == "-" {
                    *s = "0".into();
                }
            }
            _ => {
                if self.equals_expr.take().is_some() {
                    self.tokens.clear();
                }
            }
        }
    }

    fn lparen(&mut self) {
        if self.kind == Kind::Standard {
            return;
        }
        if self.equals_expr.take().is_some() {
            self.tokens.clear();
            self.current = Current::None;
        }
        if !matches!(self.current, Current::None) {
            // `2(` means `2 × (`
            let operand = self.take_operand();
            self.tokens.push(Token::Operand(operand));
            self.tokens.push(Token::Bin(BinOp::Mul));
        }
        self.tokens.push(Token::LParen);
        self.open_parens += 1;
    }

    fn rparen(&mut self) {
        if self.open_parens == 0 || self.equals_expr.is_some() {
            return;
        }
        let operand = self.take_operand();
        self.tokens.push(Token::Operand(operand));
        let li = self.tokens.iter().rposition(|t| matches!(t, Token::LParen)).expect("open paren");
        let inner = parse(&self.tokens[li + 1..]);
        self.tokens.truncate(li);
        self.open_parens -= 1;
        match inner {
            Ok(inner) => match self.eval(&inner) {
                Ok(value) => {
                    self.current = Current::Value {
                        node: Node::Group(Box::new(inner)),
                        value,
                    }
                }
                Err(e) => self.set_error(e),
            },
            Err(e) => self.set_error(e),
        }
    }

    fn equals(&mut self) -> Option<HistoryEntry> {
        self.equals_expr = None;
        let node = if self.tokens.is_empty() {
            let operand = self.take_operand();
            match (&operand, &self.last) {
                // "5 + 3 = =" and "7 =" after a calculation repeat the last operation.
                (Node::Num(v), Some((op, b))) => Node::binary(*op, Node::Num(v.clone()), Node::Num(b.clone())),
                (Node::Num(v), None) => {
                    self.equals_expr = Some(format!("{} =", self.format_value(v)));
                    self.current = Current::Value {
                        node: operand.clone(),
                        value: v.clone(),
                    };
                    return None;
                }
                _ => operand,
            }
        } else {
            let operand = self.take_operand();
            self.tokens.push(Token::Operand(operand));
            match parse(&self.tokens) {
                Ok(n) => n,
                Err(e) => {
                    self.set_error(e);
                    return None;
                }
            }
        };
        let expr_text = format!("{} =", self.render_node(&node));
        self.tokens.clear();
        self.open_parens = 0;
        self.overflowed = false;
        let result = self.eval(&node);
        self.equals_expr = Some(expr_text.clone());
        if let Node::Binary(op, _, b) = &node {
            if let Ok(bv) = self.eval(b) {
                self.last = Some((*op, bv));
            }
        }
        match result {
            Ok(v) => {
                self.current = Current::Value {
                    node: Node::Num(v.clone()),
                    value: v.clone(),
                };
                let result = self.format_value(&v);
                (self.kind != Kind::Programmer).then_some(HistoryEntry {
                    expression: expr_text,
                    result,
                    value: v,
                })
            }
            Err(e) => {
                self.set_error(e);
                None
            }
        }
    }

    // ------------------------------------------------------------ display

    pub fn format_value(&self, v: &Dec) -> String {
        match self.kind {
            Kind::Programmer => programmer::format(v.to_i64().unwrap_or(0), self.radix, self.word),
            k => format::display(v, k.max_digits(), self.fe),
        }
    }

    fn render_node(&self, n: &Node) -> String {
        n.to_text(&|d: &Dec| self.format_value(d))
    }

    /// Main display text.
    pub fn display_text(&self) -> String {
        if let Some(e) = self.error {
            return e.message().to_string();
        }
        match &self.current {
            Current::Typing(s) => self.format_typing(s),
            _ => self.format_value(&self.value()),
        }
    }

    fn format_typing(&self, s: &str) -> String {
        match self.kind {
            Kind::Programmer => {
                let (neg, digits) = match s.strip_prefix('-') {
                    Some(d) => ("-", d),
                    None => ("", s),
                };
                let grouped = match self.radix {
                    Radix::Dec => format::group_decimal(digits),
                    Radix::Oct => format::group_radix(digits, 3, ' '),
                    _ => format::group_radix(digits, 4, ' '),
                };
                format!("{neg}{grouped}")
            }
            _ => {
                let mut t = format::group_decimal(s);
                if t.ends_with('+') || t.ends_with('-') {
                    t.push('0');
                }
                t
            }
        }
    }

    /// Expression line above the display.
    pub fn expression_text(&self) -> String {
        if let Some(e) = &self.equals_expr {
            return e.clone();
        }
        let mut out = String::new();
        for t in &self.tokens {
            if !out.is_empty() && !out.ends_with('(') {
                out.push(' ');
            }
            match t {
                Token::Operand(n) => out.push_str(&self.render_node(n)),
                Token::Bin(op) => out.push_str(op.symbol()),
                Token::LParen => out.push('('),
                _ => {}
            }
        }
        if let Current::Value { node, .. } = &self.current {
            if !matches!(node, Node::Num(_)) {
                if !out.is_empty() && !out.ends_with('(') {
                    out.push(' ');
                }
                out.push_str(&self.render_node(node));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
