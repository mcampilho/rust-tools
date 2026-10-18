//! Expression representation, parsing and evaluation.

pub mod ast;
pub mod eval;
pub mod parse;

pub use ast::{BinOp, Constant, Node, Unary};
pub use parse::{parse, parse_str, pending_operand, LexMode, Token};

#[cfg(test)]
mod tests;
