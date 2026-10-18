//! Proleptic Gregorian calendar math for the Date Calculation mode.

use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Date {
    pub y: i32,
    pub m: u32,
    pub d: u32,
}

pub const MIN_YEAR: i32 = 1;
pub const MAX_YEAR: i32 = 9999;

pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
pub const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

pub fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}

impl Date {
    pub fn new(y: i32, m: u32, d: u32) -> Option<Date> {
        if !(MIN_YEAR..=MAX_YEAR).contains(&y) || !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
            return None;
        }
        Some(Date { y, m, d })
    }

    /// Days since 1970-01-01 (Howard Hinnant's algorithm).
    pub fn to_days(self) -> i64 {
        let y = self.y as i64 - if self.m <= 2 { 1 } else { 0 };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = self.m as i64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + self.d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(z: i64) -> Option<Date> {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let y = (yoe + era * 400 + if m <= 2 { 1 } else { 0 }) as i32;
        Date::new(y, m, d)
    }

    /// 0 = Sunday.
    pub fn weekday(self) -> usize {
        (self.to_days() + 4).rem_euclid(7) as usize
    }

    /// Adds years and months, clamping the day to the end of the month
    /// (Jan 31 + 1 month = Feb 28/29), as Windows Calculator does.
    pub fn add_months(self, months: i64) -> Option<Date> {
        let total = self.y as i64 * 12 + (self.m as i64 - 1) + months;
        let y = total.div_euclid(12);
        let m = (total.rem_euclid(12) + 1) as u32;
        if y < MIN_YEAR as i64 || y > MAX_YEAR as i64 {
            return None;
        }
        let y = y as i32;
        Date::new(y, m, self.d.min(days_in_month(y, m)))
    }

    pub fn add_days(self, days: i64) -> Option<Date> {
        Date::from_days(self.to_days() + days)
    }

    /// Adds (or subtracts) years, months and days, in that order.
    pub fn add(self, years: i64, months: i64, days: i64, subtract: bool) -> Option<Date> {
        let s = if subtract { -1 } else { 1 };
        self.add_months(s * (years * 12 + months))?.add_days(s * days)
    }

    pub fn long_text(self) -> String {
        format!(
            "{}, {} {}, {}",
            WEEKDAYS[self.weekday()],
            MONTHS[self.m as usize - 1],
            self.d,
            self.y
        )
    }
}

impl PartialOrd for Date {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Date {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.y, self.m, self.d).cmp(&(other.y, other.m, other.d))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Difference {
    pub years: i64,
    pub months: i64,
    pub weeks: i64,
    pub days: i64,
    pub total_days: i64,
}

impl Difference {
    /// "1 year, 2 months, 3 weeks, 4 days", or "Same dates".
    pub fn text(&self) -> String {
        if self.total_days == 0 {
            return "Same dates".into();
        }
        let mut parts = Vec::new();
        for (n, one, many) in [
            (self.years, "year", "years"),
            (self.months, "month", "months"),
            (self.weeks, "week", "weeks"),
            (self.days, "day", "days"),
        ] {
            if n != 0 {
                parts.push(format!("{n} {}", if n == 1 { one } else { many }));
            }
        }
        parts.join(", ")
    }

    pub fn total_text(&self) -> String {
        let n = self.total_days;
        format!(
            "{} {}",
            crate::format::group_decimal(&n.to_string()),
            if n == 1 { "day" } else { "days" }
        )
    }
}

/// Calendar difference between two dates (order does not matter).
pub fn difference(a: Date, b: Date) -> Difference {
    let (start, end) = if a <= b { (a, b) } else { (b, a) };
    let total_days = end.to_days() - start.to_days();
    let mut months = (end.y as i64 - start.y as i64) * 12 + (end.m as i64 - start.m as i64);
    while months > 0 && start.add_months(months).is_none_or(|d| d > end) {
        months -= 1;
    }
    let anchor = start.add_months(months).unwrap_or(start);
    let rest = end.to_days() - anchor.to_days();
    Difference {
        years: months / 12,
        months: months % 12,
        weeks: rest / 7,
        days: rest % 7,
        total_days,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, dd: u32) -> Date {
        Date::new(y, m, dd).unwrap()
    }

    #[test]
    fn days_round_trip() {
        assert_eq!(d(1970, 1, 1).to_days(), 0);
        assert_eq!(d(2000, 3, 1).to_days(), 11017);
        for z in [-719_162i64, -1, 0, 1, 10_000, 2_932_896] {
            assert_eq!(Date::from_days(z).unwrap().to_days(), z);
        }
        assert_eq!(Date::from_days(d(9999, 12, 31).to_days() + 1), None);
    }

    #[test]
    fn weekdays() {
        assert_eq!(WEEKDAYS[d(2026, 10, 17).weekday()], "Saturday");
        assert_eq!(WEEKDAYS[d(2000, 1, 1).weekday()], "Saturday");
        assert_eq!(d(2024, 2, 29).long_text(), "Thursday, February 29, 2024");
    }

    #[test]
    fn leap_years() {
        assert!(is_leap(2000));
        assert!(!is_leap(1900));
        assert!(is_leap(2024));
        assert!(Date::new(2023, 2, 29).is_none());
    }

    #[test]
    fn add_and_subtract() {
        assert_eq!(d(2024, 1, 31).add(0, 1, 0, false), Some(d(2024, 2, 29)));
        assert_eq!(d(2023, 1, 31).add(0, 1, 0, false), Some(d(2023, 2, 28)));
        assert_eq!(d(2024, 2, 29).add(1, 0, 0, false), Some(d(2025, 2, 28)));
        assert_eq!(d(2026, 10, 17).add(0, 0, 100, false), Some(d(2027, 1, 25)));
        assert_eq!(d(2026, 3, 1).add(0, 0, 1, true), Some(d(2026, 2, 28)));
        assert_eq!(d(9999, 12, 31).add(0, 0, 1, false), None);
    }

    #[test]
    fn differences() {
        let diff = difference(d(2026, 1, 1), d(2027, 3, 20));
        assert_eq!((diff.years, diff.months, diff.weeks, diff.days), (1, 2, 2, 5));
        assert_eq!(diff.total_days, 443);
        assert_eq!(diff.text(), "1 year, 2 months, 2 weeks, 5 days");
        assert_eq!(difference(d(2026, 5, 5), d(2026, 5, 5)).text(), "Same dates");
        assert_eq!(difference(d(2026, 5, 12), d(2026, 5, 5)).text(), "1 week");
        // month-end anchoring
        let diff = difference(d(2024, 1, 31), d(2024, 2, 29));
        assert_eq!((diff.months, diff.days), (1, 0));
        assert_eq!(difference(d(2024, 1, 1), d(2025, 1, 1)).total_text(), "366 days");
    }
}
