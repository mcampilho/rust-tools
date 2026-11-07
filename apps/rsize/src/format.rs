//! Sizes, counts, percentages and dates as shown, with the decimal and
//! thousands separators of the interface language.

use crate::i18n::{lang, tr, Lang};
use std::sync::OnceLock;

const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

/// A size with three significant digits, in binary units: `1.23 GB`,
/// `45.6 MB`, `789 KB`, `12 B`.
pub fn size(bytes: u64) -> String {
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    let num = if unit == 0 {
        bytes.to_string()
    } else {
        let decimals = if v < 9.995 {
            2
        } else if v < 99.95 {
            1
        } else {
            0
        };
        decimal(v, decimals)
    };
    format!("{num} {}", tr(UNITS[unit]))
}

/// `v` with `decimals` digits after the separator of the language.
pub fn decimal(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    let (dec, _) = lang().separators();
    s.replace('.', &dec.to_string())
}

/// An integer with thousands separators: `1,234,567`.
pub fn count(n: u64) -> String {
    let (_, sep) = lang().separators();
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 * 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

/// A share as a percentage with one decimal: `45.2%` (`45,2 %` where the
/// language puts a space before the sign).
pub fn percent(part: u64, whole: u64) -> String {
    let p = if whole == 0 { 0.0 } else { part as f64 * 100.0 / whole as f64 };
    let space = match lang() {
        Lang::DeDE | Lang::FrFR | Lang::EsES | Lang::ItIT | Lang::PtPT => "\u{00A0}",
        _ => "",
    };
    format!("{}{space}%", decimal(p, 1))
}

/// A date as `2026-11-07` in local time (`secs` since 1970, UTC).
pub fn date(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    let (y, m, d) = civil(((secs + utc_offset()).div_euclid(86_400)) as i32);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A date and time as `2026-11-07 13:40:05` in local time (empty for none).
pub fn datetime(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    let local = secs + utc_offset();
    let (y, m, d) = civil(local.div_euclid(86_400) as i32);
    let t = local.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
}

/// Days since 1970 to (year, month, day) (Howard Hinnant's algorithm).
fn civil(days: i32) -> (i32, u32, u32) {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((yoe + era * 400 + i64::from(m <= 2)) as i32, m, d)
}

/// (year, month, day) to days since 1970.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64 - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Seconds to add to UTC for local time, measured once.
fn utc_offset() -> i64 {
    static OFFSET: OnceLock<i64> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let t = rt_sys::local_time();
        let local = days_from_civil(t.year, t.month, t.day) * 86_400 + (t.hour * 3600 + t.minute * 60 + t.second) as i64;
        // Time zones are whole quarter hours.
        ((local - now) as f64 / 900.0).round() as i64 * 900
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::set_lang;

    #[test]
    fn sizes_have_three_digits() {
        set_lang(Lang::EnUS);
        assert_eq!(size(0), "0 B");
        assert_eq!(size(999), "999 B");
        assert_eq!(size(1000), "0.98 KB");
        assert_eq!(size(1536), "1.50 KB");
        assert_eq!(size(10 * 1024 * 1024), "10.0 MB");
        assert_eq!(size(500 * 1024 * 1024 * 1024), "500 GB");
        assert_eq!(size(u64::MAX), "16384 PB");
        set_lang(Lang::FrFR);
        assert_eq!(size(1536), "1,50 Ko");
        set_lang(Lang::EnUS);
    }

    #[test]
    fn counts_and_percentages() {
        set_lang(Lang::EnUS);
        assert_eq!(count(0), "0");
        assert_eq!(count(1234567), "1,234,567");
        assert_eq!(count(100), "100");
        assert_eq!(percent(1, 3), "33.3%");
        assert_eq!(percent(5, 0), "0.0%");
        set_lang(Lang::PtPT);
        assert_eq!(count(1234567), "1\u{a0}234\u{a0}567");
        assert_eq!(percent(1, 2), "50,0\u{a0}%");
        set_lang(Lang::EnUS);
    }

    #[test]
    fn calendar_round_trip() {
        for days in [-1000, 0, 1, 365, 10_957, 20_000, 30_000] {
            let (y, m, d) = civil(days);
            assert_eq!(days_from_civil(y, m, d), days as i64);
        }
        assert_eq!(civil(20_399), (2025, 11, 7));
        assert!(utc_offset().abs() <= 14 * 3600);
        assert_eq!(date(0), "");
        assert_eq!(date(1_577_880_000).len(), 10);
    }
}
