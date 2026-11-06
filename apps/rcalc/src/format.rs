//! Number formatting for the display (digit grouping, radix output).

use crate::num::Dec;

/// Inserts `,` thousands separators into the integer part of a plain
/// number string (`-1234567.5e+3` -> `-1,234,567.5e+3`).
pub fn group_decimal(s: &str) -> String {
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => ("-", r),
        None => ("", s),
    };
    let int_end = rest.find(['.', 'e']).unwrap_or(rest.len());
    let (int, tail) = rest.split_at(int_end);
    let mut out = String::with_capacity(s.len() + int.len() / 3);
    out.push_str(sign);
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.push_str(tail);
    out
}

/// Display text of a value with at most `digits` significant digits.
pub fn display(d: &Dec, digits: usize, force_sci: bool) -> String {
    group_decimal(&d.format(digits, force_sci))
}

/// Groups radix digits from the right: `group` digits per block separated by `sep`.
pub fn group_radix(digits: &str, group: usize, sep: char) -> String {
    let n = digits.chars().count();
    let mut out = String::with_capacity(n + n / group);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (n - i).is_multiple_of(group) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping() {
        assert_eq!(group_decimal("1234567"), "1,234,567");
        assert_eq!(group_decimal("-1234.5678"), "-1,234.5678");
        assert_eq!(group_decimal("123"), "123");
        assert_eq!(group_decimal("1.5e+20"), "1.5e+20");
        assert_eq!(group_decimal("0."), "0.");
        assert_eq!(group_radix("11110000", 4, ' '), "1111 0000");
        assert_eq!(group_radix("1FFFF", 4, ' '), "1 FFFF");
    }
}
