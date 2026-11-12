//! National public holidays of some countries, worked out for any year
//! (fixed dates, the ones that move with Easter, and "the third Monday of
//! January" kind). Regional and municipal holidays are left out.
//!
//! To add a country: a row in `COUNTRIES` (its name translated in
//! `strings.txt`) and its rules in `rules`; new holiday names go to
//! `strings.txt` too.

use crate::schedule::{civil_from_days, days_from_civil};

/// Countries whose holidays are known: ISO code and name (a key of the table
/// of texts).
pub const COUNTRIES: [(&str, &str); 9] = [
    ("PT", "Portugal"),
    ("BR", "Brazil"),
    ("ES", "Spain"),
    ("MX", "Mexico"),
    ("US", "United States"),
    ("GB", "United Kingdom"),
    ("FR", "France"),
    ("DE", "Germany"),
    ("IT", "Italy"),
];

/// The name of a country whose holidays are known.
pub fn country_name(code: &str) -> Option<&'static str> {
    COUNTRIES.iter().find(|(c, _)| c.eq_ignore_ascii_case(code)).map(|(_, n)| *n)
}

/// One holiday: its date and its name (a key of the table of texts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holiday {
    pub y: i32,
    pub m: u32,
    pub d: u32,
    pub name: &'static str,
}

/// Easter Sunday (Gregorian), as days since 1970-01-01.
pub fn easter(y: i32) -> i64 {
    let a = y % 19;
    let b = y / 100;
    let c = y % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    days_from_civil(y, month as u32, day as u32)
}

/// Day of the week of a day count, 0 = Sunday.
fn wd(z: i64) -> i64 {
    (z + 4).rem_euclid(7)
}

/// The `n`th (1-based) weekday `w` (0 = Sunday) of a month.
fn nth(y: i32, m: u32, w: i64, n: i64) -> i64 {
    let first = days_from_civil(y, m, 1);
    first + (w - wd(first)).rem_euclid(7) + 7 * (n - 1)
}

/// The last weekday `w` of a month.
fn last(y: i32, m: u32, w: i64) -> i64 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    let end = days_from_civil(ny, nm, 1) - 1;
    end - (wd(end) - w).rem_euclid(7)
}

const MON: i64 = 1;
const THU: i64 = 4;

/// The holidays of `country` in year `y`, by date; none for a country not
/// in `COUNTRIES`.
pub fn of_year(country: &str, y: i32) -> Vec<Holiday> {
    let e = easter(y);
    let fixed = |m: u32, d: u32| days_from_civil(y, m, d);
    let mut list: Vec<(i64, &'static str)> = Vec::new();
    let mut add = |z: i64, name: &'static str| list.push((z, name));
    match country.to_ascii_uppercase().as_str() {
        "PT" => {
            add(fixed(1, 1), "New Year's Day");
            add(e - 47, "Carnival");
            add(e - 2, "Good Friday");
            add(e, "Easter Sunday");
            add(fixed(4, 25), "Freedom Day");
            add(fixed(5, 1), "Workers' Day");
            add(e + 60, "Corpus Christi");
            add(fixed(6, 10), "Portugal Day");
            add(fixed(8, 15), "Assumption of Mary");
            add(fixed(10, 5), "Implantation of the Republic");
            add(fixed(11, 1), "All Saints' Day");
            add(fixed(12, 1), "Restoration of Independence");
            add(fixed(12, 8), "Immaculate Conception");
            add(fixed(12, 25), "Christmas Day");
        }
        "BR" => {
            add(fixed(1, 1), "New Year's Day");
            add(e - 48, "Carnival");
            add(e - 47, "Carnival");
            add(e - 2, "Good Friday");
            add(fixed(4, 21), "Tiradentes");
            add(fixed(5, 1), "Workers' Day");
            add(e + 60, "Corpus Christi");
            add(fixed(9, 7), "Independence Day");
            add(fixed(10, 12), "Our Lady of Aparecida");
            add(fixed(11, 2), "All Souls' Day");
            add(fixed(11, 15), "Proclamation of the Republic");
            if y >= 2024 {
                add(fixed(11, 20), "Black Consciousness Day");
            }
            add(fixed(12, 25), "Christmas Day");
        }
        "ES" => {
            add(fixed(1, 1), "New Year's Day");
            add(fixed(1, 6), "Epiphany");
            add(e - 2, "Good Friday");
            add(fixed(5, 1), "Workers' Day");
            add(fixed(8, 15), "Assumption of Mary");
            add(fixed(10, 12), "National Day of Spain");
            add(fixed(11, 1), "All Saints' Day");
            add(fixed(12, 6), "Constitution Day");
            add(fixed(12, 8), "Immaculate Conception");
            add(fixed(12, 25), "Christmas Day");
        }
        "MX" => {
            add(fixed(1, 1), "New Year's Day");
            add(nth(y, 2, MON, 1), "Constitution Day");
            add(nth(y, 3, MON, 3), "Benito Juárez Day");
            add(fixed(5, 1), "Workers' Day");
            add(fixed(9, 16), "Independence Day");
            add(nth(y, 11, MON, 3), "Revolution Day");
            add(fixed(12, 25), "Christmas Day");
        }
        "US" => {
            add(fixed(1, 1), "New Year's Day");
            add(nth(y, 1, MON, 3), "Martin Luther King Jr. Day");
            add(nth(y, 2, MON, 3), "Presidents' Day");
            add(last(y, 5, MON), "Memorial Day");
            if y >= 2021 {
                add(fixed(6, 19), "Juneteenth");
            }
            add(fixed(7, 4), "Independence Day");
            add(nth(y, 9, MON, 1), "Labor Day");
            add(nth(y, 10, MON, 2), "Columbus Day");
            add(fixed(11, 11), "Veterans Day");
            add(nth(y, 11, THU, 4), "Thanksgiving");
            add(fixed(12, 25), "Christmas Day");
        }
        "GB" => {
            // England and Wales.
            add(fixed(1, 1), "New Year's Day");
            add(e - 2, "Good Friday");
            add(e + 1, "Easter Monday");
            add(nth(y, 5, MON, 1), "Early May Bank Holiday");
            add(last(y, 5, MON), "Spring Bank Holiday");
            add(last(y, 8, MON), "Summer Bank Holiday");
            add(fixed(12, 25), "Christmas Day");
            add(fixed(12, 26), "Boxing Day");
        }
        "FR" => {
            add(fixed(1, 1), "New Year's Day");
            add(e + 1, "Easter Monday");
            add(fixed(5, 1), "Workers' Day");
            add(fixed(5, 8), "Victory in Europe Day");
            add(e + 39, "Ascension Day");
            add(e + 50, "Whit Monday");
            add(fixed(7, 14), "Bastille Day");
            add(fixed(8, 15), "Assumption of Mary");
            add(fixed(11, 1), "All Saints' Day");
            add(fixed(11, 11), "Armistice Day");
            add(fixed(12, 25), "Christmas Day");
        }
        "DE" => {
            add(fixed(1, 1), "New Year's Day");
            add(e - 2, "Good Friday");
            add(e + 1, "Easter Monday");
            add(fixed(5, 1), "Workers' Day");
            add(e + 39, "Ascension Day");
            add(e + 50, "Whit Monday");
            add(fixed(10, 3), "German Unity Day");
            add(fixed(12, 25), "Christmas Day");
            add(fixed(12, 26), "Second Day of Christmas");
        }
        "IT" => {
            add(fixed(1, 1), "New Year's Day");
            add(fixed(1, 6), "Epiphany");
            add(e, "Easter Sunday");
            add(e + 1, "Easter Monday");
            add(fixed(4, 25), "Liberation Day");
            add(fixed(5, 1), "Workers' Day");
            add(fixed(6, 2), "Republic Day");
            add(fixed(8, 15), "Assumption of Mary");
            add(fixed(11, 1), "All Saints' Day");
            add(fixed(12, 8), "Immaculate Conception");
            add(fixed(12, 25), "Christmas Day");
            add(fixed(12, 26), "St. Stephen's Day");
        }
        _ => {}
    }
    list.sort_by_key(|(z, _)| *z);
    list.into_iter()
        .map(|(z, name)| {
            let (y, m, d) = civil_from_days(z);
            Holiday { y, m, d, name }
        })
        .collect()
}

/// The holiday on a day, if any.
pub fn on(list: &[Holiday], y: i32, m: u32, d: u32) -> Option<&'static str> {
    list.iter().find(|h| (h.y, h.m, h.d) == (y, m, d)).map(|h| h.name)
}

/// The first holiday on or after a day (looking into the next year too).
pub fn next(country: &str, y: i32, m: u32, d: u32) -> Option<Holiday> {
    let from = days_from_civil(y, m, d);
    [y, y + 1].into_iter().flat_map(|yy| of_year(country, yy)).find(|h| days_from_civil(h.y, h.m, h.d) >= from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(z: i64) -> (i32, u32, u32) {
        civil_from_days(z)
    }

    #[test]
    fn easter_sundays() {
        assert_eq!(date(easter(2024)), (2024, 3, 31));
        assert_eq!(date(easter(2025)), (2025, 4, 20));
        assert_eq!(date(easter(2026)), (2026, 4, 5));
        assert_eq!(date(easter(2027)), (2027, 3, 28));
        assert_eq!(date(easter(2038)), (2038, 4, 25));
    }

    #[test]
    fn some_countries() {
        let pt = of_year("pt", 2026);
        assert_eq!(pt.len(), 14);
        assert_eq!(on(&pt, 2026, 2, 17), Some("Carnival"));
        assert_eq!(on(&pt, 2026, 4, 3), Some("Good Friday"));
        assert_eq!(on(&pt, 2026, 6, 4), Some("Corpus Christi"));
        assert_eq!(on(&pt, 2026, 12, 1), Some("Restoration of Independence"));
        assert_eq!(on(&pt, 2026, 12, 2), None);
        let us = of_year("US", 2026);
        assert_eq!(on(&us, 2026, 11, 26), Some("Thanksgiving"));
        assert_eq!(on(&us, 2026, 5, 25), Some("Memorial Day"));
        assert_eq!(on(&us, 2026, 1, 19), Some("Martin Luther King Jr. Day"));
        let gb = of_year("GB", 2026);
        assert_eq!(on(&gb, 2026, 8, 31), Some("Summer Bank Holiday"));
        assert_eq!(on(&gb, 2026, 5, 4), Some("Early May Bank Holiday"));
        assert_eq!(on(&of_year("MX", 2026), 2026, 11, 16), Some("Revolution Day"));
        assert_eq!(on(&of_year("FR", 2026), 2026, 5, 14), Some("Ascension Day"));
        assert_eq!(on(&of_year("DE", 2026), 2026, 5, 25), Some("Whit Monday"));
        assert!(of_year("ZZ", 2026).is_empty());
        // Sorted, and the next one crosses into the next year.
        assert!(pt.windows(2).all(|w| (w[0].m, w[0].d) <= (w[1].m, w[1].d)));
        assert_eq!(next("PT", 2026, 11, 12).map(|h| (h.m, h.d)), Some((12, 1)));
        assert_eq!(next("PT", 2026, 12, 26).map(|h| (h.y, h.m, h.d)), Some((2027, 1, 1)));
        assert_eq!(next("ZZ", 2026, 1, 1), None);
        for (c, _) in COUNTRIES {
            assert!(!of_year(c, 2026).is_empty(), "{c}");
        }
        assert_eq!(country_name("br"), Some("Brazil"));
    }
}
