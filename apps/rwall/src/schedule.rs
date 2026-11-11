//! When the wallpaper changes.
//!
//! Each frequency cuts local time into periods (a day, half a day, a week
//! starting on Monday, ...). The wallpaper changes once in each period: when
//! the period of now differs from the one of the last change. Checking that
//! once a minute also covers a computer that slept through a change, a
//! clock or time zone change, and a start long after the last change.

use rt_sys::LocalTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frequency {
    /// Once a day, at midnight.
    Daily,
    /// At 00:00 and 12:00.
    HalfDay,
    /// At 00:00, 06:00, 12:00 and 18:00.
    Quarter,
    /// On Mondays at midnight.
    Weekly,
    /// On the first of each month at midnight.
    Monthly,
    /// Every so many minutes.
    Custom,
}

impl Frequency {
    pub const ALL: [Frequency; 6] = [Frequency::Daily, Frequency::HalfDay, Frequency::Quarter, Frequency::Weekly, Frequency::Monthly, Frequency::Custom];

    /// Name in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Frequency::Daily => "daily",
            Frequency::HalfDay => "half-day",
            Frequency::Quarter => "quarter",
            Frequency::Weekly => "weekly",
            Frequency::Monthly => "monthly",
            Frequency::Custom => "custom",
        }
    }

    pub fn from_key(k: &str) -> Option<Frequency> {
        Frequency::ALL.into_iter().find(|f| f.key() == k)
    }
}

/// Custom intervals offered, in minutes.
pub const CUSTOM_STEPS: [u32; 13] = [5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240, 360, 480];

/// The next (`up`) or previous custom interval from `m`.
pub fn custom_step(m: u32, up: bool) -> u32 {
    if up {
        CUSTOM_STEPS.into_iter().find(|s| *s > m).unwrap_or(CUSTOM_STEPS[CUSTOM_STEPS.len() - 1])
    } else {
        CUSTOM_STEPS.into_iter().rev().find(|s| *s < m).unwrap_or(CUSTOM_STEPS[0])
    }
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar.
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day counted from 1970-01-01.
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((yoe + era * 400 + i64::from(m <= 2)) as i32, m, d)
}

/// Minutes from 1970-01-01 00:00 local time (the clock as shown).
pub fn local_minutes(t: &LocalTime) -> i64 {
    days_from_civil(t.year, t.month, t.day) * 1440 + i64::from(t.hour) * 60 + i64::from(t.minute)
}

/// The local time `minutes` after 1970-01-01 00:00.
pub fn from_minutes(minutes: i64) -> LocalTime {
    let (year, month, day) = civil_from_days(minutes.div_euclid(1440));
    let m = minutes.rem_euclid(1440) as u32;
    LocalTime { year, month, day, hour: m / 60, minute: m % 60, second: 0 }
}

/// The period `t` falls in, as stored in the settings.
pub fn period(f: Frequency, t: &LocalTime, custom_minutes: u32) -> String {
    let date = format!("{:04}-{:02}-{:02}", t.year, t.month, t.day);
    match f {
        Frequency::Daily => format!("d{date}"),
        Frequency::HalfDay => format!("h{date}-{}", t.hour / 12),
        Frequency::Quarter => format!("q{date}-{}", t.hour / 6),
        // 1970-01-01 was a Thursday: day 4 was the first Monday.
        Frequency::Weekly => format!("w{}", (days_from_civil(t.year, t.month, t.day) - 4).div_euclid(7)),
        Frequency::Monthly => format!("m{:04}-{:02}", t.year, t.month),
        Frequency::Custom => {
            let n = i64::from(custom_minutes.max(1));
            format!("c{n}-{}", local_minutes(t).div_euclid(n))
        }
    }
}

/// When the period after the one `t` falls in starts.
pub fn next_change(f: Frequency, t: &LocalTime, custom_minutes: u32) -> LocalTime {
    let day = days_from_civil(t.year, t.month, t.day);
    let now = local_minutes(t);
    let minutes = match f {
        Frequency::Daily => (day + 1) * 1440,
        Frequency::HalfDay => (now.div_euclid(720) + 1) * 720,
        Frequency::Quarter => (now.div_euclid(360) + 1) * 360,
        Frequency::Weekly => ((day - 4).div_euclid(7) * 7 + 4 + 7) * 1440,
        Frequency::Monthly => {
            let (y, m) = if t.month == 12 { (t.year + 1, 1) } else { (t.year, t.month + 1) };
            days_from_civil(y, m, 1) * 1440
        }
        Frequency::Custom => {
            let n = i64::from(custom_minutes.max(1));
            (now.div_euclid(n) + 1) * n
        }
    };
    from_minutes(minutes)
}

/// "2026-11-11 18:00": dates and times as the settings window shows them.
pub fn format(t: &LocalTime) -> String {
    format!("{:04}-{:02}-{:02} {:02}:{:02}", t.year, t.month, t.day, t.hour, t.minute)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> LocalTime {
        LocalTime { year, month, day, hour, minute, second: 0 }
    }

    #[test]
    fn calendar_round_trips() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        for z in [-800_000, -1, 0, 59, 60, 11_016, 20_768, 100_000] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        let t = at(2026, 11, 11, 23, 59);
        assert_eq!(from_minutes(local_minutes(&t)), t);
    }

    #[test]
    fn periods_change_at_their_boundaries() {
        let p = |f, t: LocalTime| period(f, &t, 30);
        assert_eq!(p(Frequency::Daily, at(2026, 11, 11, 0, 0)), p(Frequency::Daily, at(2026, 11, 11, 23, 59)));
        assert_ne!(p(Frequency::Daily, at(2026, 11, 11, 23, 59)), p(Frequency::Daily, at(2026, 11, 12, 0, 0)));
        assert_eq!(p(Frequency::HalfDay, at(2026, 11, 11, 0, 0)), p(Frequency::HalfDay, at(2026, 11, 11, 11, 59)));
        assert_ne!(p(Frequency::HalfDay, at(2026, 11, 11, 11, 59)), p(Frequency::HalfDay, at(2026, 11, 11, 12, 0)));
        assert_ne!(p(Frequency::Quarter, at(2026, 11, 11, 5, 59)), p(Frequency::Quarter, at(2026, 11, 11, 6, 0)));
        assert_eq!(p(Frequency::Quarter, at(2026, 11, 11, 18, 0)), p(Frequency::Quarter, at(2026, 11, 11, 23, 0)));
        // 2026-11-09 is a Monday; 2026-11-15 a Sunday.
        assert_eq!(p(Frequency::Weekly, at(2026, 11, 9, 0, 0)), p(Frequency::Weekly, at(2026, 11, 15, 23, 59)));
        assert_ne!(p(Frequency::Weekly, at(2026, 11, 15, 23, 59)), p(Frequency::Weekly, at(2026, 11, 16, 0, 0)));
        assert_eq!(p(Frequency::Monthly, at(2026, 11, 1, 0, 0)), p(Frequency::Monthly, at(2026, 11, 30, 23, 0)));
        assert_ne!(p(Frequency::Monthly, at(2026, 11, 30, 23, 0)), p(Frequency::Monthly, at(2026, 12, 1, 0, 0)));
        assert_eq!(p(Frequency::Custom, at(2026, 11, 11, 10, 0)), p(Frequency::Custom, at(2026, 11, 11, 10, 29)));
        assert_ne!(p(Frequency::Custom, at(2026, 11, 11, 10, 29)), p(Frequency::Custom, at(2026, 11, 11, 10, 30)));
    }

    #[test]
    fn next_changes() {
        let n = |f, t: LocalTime| format(&next_change(f, &t, 90));
        let t = at(2026, 11, 11, 13, 5); // a Wednesday
        assert_eq!(n(Frequency::Daily, t), "2026-11-12 00:00");
        assert_eq!(n(Frequency::HalfDay, t), "2026-11-12 00:00");
        assert_eq!(n(Frequency::HalfDay, at(2026, 11, 11, 9, 0)), "2026-11-11 12:00");
        assert_eq!(n(Frequency::Quarter, t), "2026-11-11 18:00");
        assert_eq!(n(Frequency::Weekly, t), "2026-11-16 00:00");
        assert_eq!(n(Frequency::Weekly, at(2026, 11, 16, 0, 0)), "2026-11-23 00:00");
        assert_eq!(n(Frequency::Monthly, at(2026, 12, 31, 23, 0)), "2027-01-01 00:00");
        assert_eq!(n(Frequency::Custom, t), "2026-11-11 13:30");
        // The next change starts a new period.
        for f in Frequency::ALL {
            assert_ne!(period(f, &t, 90), period(f, &next_change(f, &t, 90), 90), "{f:?}");
        }
    }

    #[test]
    fn keys_and_steps() {
        for f in Frequency::ALL {
            assert_eq!(Frequency::from_key(f.key()), Some(f));
        }
        assert_eq!(custom_step(60, true), 90);
        assert_eq!(custom_step(60, false), 45);
        assert_eq!(custom_step(7, false), 5);
        assert_eq!(custom_step(480, true), 480);
    }
}
