//! Dates for the clocks and calendars: names of days and months in the
//! language of rwall, and the days of a month laid out in weeks.

use crate::i18n::{lang, tr, Lang};
use crate::schedule::{civil_from_days, days_from_civil};
use rt_sys::LocalTime;

/// Months, as keys of the table of texts.
pub const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// Days of the week from Sunday, as keys of the table of texts.
pub const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// Short days of the week from Sunday (calendar headers).
pub const WEEKDAYS_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// Day of the week, 0 = Sunday.
pub fn weekday(y: i32, m: u32, d: u32) -> usize {
    (days_from_civil(y, m, d) + 4).rem_euclid(7) as usize
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1)) as u32
}

/// The month `delta` months from `y`-`m`.
pub fn add_months(y: i32, m: u32, delta: i32) -> (i32, u32) {
    let n = y * 12 + m as i32 - 1 + delta;
    (n.div_euclid(12), n.rem_euclid(12) as u32 + 1)
}

/// The first day of the week (0 = Sunday): as the regional settings say,
/// else as is the custom of the language.
pub fn first_weekday() -> usize {
    super::region::region().first_day.unwrap_or_else(lang_first_weekday)
}

/// The weeks start on Sunday where that is the custom of the language.
pub fn lang_first_weekday() -> usize {
    match lang() {
        Lang::EnUS | Lang::PtBR | Lang::JaJP => 0,
        _ => 1,
    }
}

/// One square of a month's calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Day {
    pub y: i32,
    pub m: u32,
    pub d: u32,
    /// Of the month shown (the others fill the first and last weeks).
    pub inside: bool,
    /// 0 = Sunday.
    pub weekday: usize,
}

/// Six weeks covering month `y`-`m`, from the first day of the week.
pub fn month_grid(y: i32, m: u32, first: usize) -> Vec<Day> {
    let start = days_from_civil(y, m, 1);
    let lead = (weekday(y, m, 1) + 7 - first) % 7;
    (0..42)
        .map(|i| {
            let z = start - lead as i64 + i;
            let (dy, dm, dd) = civil_from_days(z);
            Day { y: dy, m: dm, d: dd, inside: dm == m && dy == y, weekday: (z + 4).rem_euclid(7) as usize }
        })
        .collect()
}

/// Upper-cases the first letter ("novembro" → "Novembro").
pub fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Fills a date template: `%W` day of the week, `%D` day, `%M` month, `%Y` year.
fn fill_date(template: &str, y: i32, m: u32, d: u32) -> String {
    let wd = tr(WEEKDAYS[weekday(y, m, d)]);
    let month = tr(MONTHS[(m.clamp(1, 12) - 1) as usize]);
    template.replace("%W", &wd).replace("%D", &d.to_string()).replace("%M", &month).replace("%Y", &y.to_string())
}

/// "Thursday, November 12" in the language of rwall.
pub fn long_date(t: &LocalTime) -> String {
    capital(&fill_date(&tr("%W, %M %D"), t.year, t.month, t.day))
}

/// "December 8" (holidays).
pub fn day_month(m: u32, d: u32) -> String {
    fill_date(&tr("%M %D"), 2000, m, d)
}

/// "November 2026".
pub fn month_title(y: i32, m: u32) -> String {
    capital(&fill_date(&tr("%M %Y"), y, m, 1))
}

/// The time as a clock shows it: ("14:05", ":09") or ("2:05", ":09 PM").
pub fn clock_text(t: &LocalTime, h24: bool, seconds: bool) -> (String, String) {
    let secs = if seconds { format!(":{:02}", t.second) } else { String::new() };
    if h24 {
        (format!("{:02}:{:02}", t.hour, t.minute), secs)
    } else {
        let h = match t.hour % 12 {
            0 => 12,
            h => h,
        };
        let ampm = if t.hour < 12 { "AM" } else { "PM" };
        (format!("{h}:{:02}", t.minute), format!("{secs} {ampm}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::set_lang;

    #[test]
    fn days_and_months() {
        assert_eq!(weekday(2026, 11, 12), 4, "a Thursday");
        assert_eq!(weekday(1970, 1, 1), 4);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2026, 12), 31);
        assert_eq!(add_months(2026, 1, -1), (2025, 12));
        assert_eq!(add_months(2026, 12, 1), (2027, 1));
        assert_eq!(add_months(2026, 11, -23), (2024, 12));
        // November 2026 starts on a Sunday.
        let g = month_grid(2026, 11, 1);
        assert_eq!(g.len(), 42);
        assert_eq!((g[0].m, g[0].d, g[0].inside, g[0].weekday), (10, 26, false, 1));
        assert_eq!((g[6].m, g[6].d, g[6].inside), (11, 1, true));
        let g = month_grid(2026, 11, 0);
        assert_eq!((g[0].d, g[0].inside), (1, true));
        assert_eq!(g.iter().filter(|d| d.inside).count(), 30);
    }

    #[test]
    fn texts() {
        let t = LocalTime { year: 2026, month: 11, day: 12, hour: 0, minute: 5, second: 9 };
        assert_eq!(clock_text(&t, true, true), ("00:05".into(), ":09".into()));
        assert_eq!(clock_text(&t, false, false), ("12:05".into(), " AM".into()));
        let pm = LocalTime { hour: 13, ..t };
        assert_eq!(clock_text(&pm, false, true), ("1:05".into(), ":09 PM".into()));
        set_lang(Lang::EnUS);
        assert_eq!(long_date(&t), "Thursday, November 12");
        assert_eq!(month_title(2026, 11), "November 2026");
        set_lang(Lang::PtPT);
        assert_eq!(long_date(&t), "Quinta-feira, 12 de novembro");
        assert_eq!(month_title(2026, 11), "Novembro de 2026");
        assert_eq!(lang_first_weekday(), 1);
        set_lang(Lang::EnUS);
        assert_eq!(lang_first_weekday(), 0);
    }
}
