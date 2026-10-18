//! Numeric core: arbitrary-precision decimals and math functions.

mod dec;
pub mod func;

pub use dec::{Dec, MAX_EXP, PREC};

/// Calculation errors, with the messages shown in the display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalcError {
    DivideByZero,
    Undefined,
    InvalidInput,
    Overflow,
}

impl CalcError {
    pub fn message(self) -> &'static str {
        match self {
            CalcError::DivideByZero => "Cannot divide by zero",
            CalcError::Undefined => "Result is undefined",
            CalcError::InvalidInput => "Invalid input",
            CalcError::Overflow => "Overflow",
        }
    }
}

impl std::fmt::Display for CalcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

pub type CalcResult = Result<Dec, CalcError>;

#[cfg(test)]
mod tests;
