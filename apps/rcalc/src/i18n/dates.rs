//! Locale-aware date text: month and weekday names, long/short date
//! patterns, calendar week start and the "difference" phrase.

use super::{lang, Lang};
use crate::date::{Date, Difference};

fn months(l: Lang) -> [&'static str; 12] {
    match l {
        Lang::EnUS | Lang::EnGB => [
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
        ],
        Lang::DeDE => [
            "Januar",
            "Februar",
            "März",
            "April",
            "Mai",
            "Juni",
            "Juli",
            "August",
            "September",
            "Oktober",
            "November",
            "Dezember",
        ],
        Lang::EsES => [
            "enero",
            "febrero",
            "marzo",
            "abril",
            "mayo",
            "junio",
            "julio",
            "agosto",
            "septiembre",
            "octubre",
            "noviembre",
            "diciembre",
        ],
        Lang::FrFR => [
            "janvier",
            "février",
            "mars",
            "avril",
            "mai",
            "juin",
            "juillet",
            "août",
            "septembre",
            "octobre",
            "novembre",
            "décembre",
        ],
        Lang::ItIT => [
            "gennaio",
            "febbraio",
            "marzo",
            "aprile",
            "maggio",
            "giugno",
            "luglio",
            "agosto",
            "settembre",
            "ottobre",
            "novembre",
            "dicembre",
        ],
        Lang::PtBR | Lang::PtPT => [
            "janeiro",
            "fevereiro",
            "março",
            "abril",
            "maio",
            "junho",
            "julho",
            "agosto",
            "setembro",
            "outubro",
            "novembro",
            "dezembro",
        ],
        Lang::JaJP | Lang::ZhCN => [
            "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
        ],
    }
}

/// Weekday names starting with Sunday.
fn weekdays(l: Lang) -> [&'static str; 7] {
    match l {
        Lang::EnUS | Lang::EnGB => ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"],
        Lang::DeDE => ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"],
        Lang::EsES => ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"],
        Lang::FrFR => ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"],
        Lang::ItIT => ["domenica", "lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato"],
        Lang::PtBR | Lang::PtPT => [
            "domingo",
            "segunda-feira",
            "terça-feira",
            "quarta-feira",
            "quinta-feira",
            "sexta-feira",
            "sábado",
        ],
        Lang::JaJP => ["日曜日", "月曜日", "火曜日", "水曜日", "木曜日", "金曜日", "土曜日"],
        Lang::ZhCN => ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"],
    }
}

/// Two-letter weekday headers for the calendar, starting with Sunday.
fn weekday_short(l: Lang) -> [&'static str; 7] {
    match l {
        Lang::EnUS | Lang::EnGB => ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"],
        Lang::DeDE => ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"],
        Lang::EsES => ["do", "lu", "ma", "mi", "ju", "vi", "sá"],
        Lang::FrFR => ["di", "lu", "ma", "me", "je", "ve", "sa"],
        Lang::ItIT => ["do", "lu", "ma", "me", "gi", "ve", "sa"],
        Lang::PtBR | Lang::PtPT => ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"],
        Lang::JaJP => ["日", "月", "火", "水", "木", "金", "土"],
        Lang::ZhCN => ["日", "一", "二", "三", "四", "五", "六"],
    }
}

/// First day of the week (0 = Sunday, 1 = Monday) per CLDR.
pub fn first_weekday() -> usize {
    first_weekday_in(lang())
}

pub fn first_weekday_in(l: Lang) -> usize {
    match l {
        Lang::EnUS | Lang::JaJP | Lang::PtBR => 0,
        _ => 1,
    }
}

/// Calendar header labels in display order.
pub fn weekday_headers() -> [&'static str; 7] {
    let s = weekday_short(lang());
    let f = first_weekday();
    std::array::from_fn(|i| s[(i + f) % 7])
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// "Saturday, October 17, 2026" in the current language.
pub fn long_date(d: Date) -> String {
    long_date_in(lang(), d)
}

pub fn long_date_in(l: Lang, d: Date) -> String {
    let m = months(l)[d.m as usize - 1];
    let w = weekdays(l)[d.weekday()];
    match l {
        Lang::EnUS => format!("{w}, {m} {}, {}", d.d, d.y),
        Lang::EnGB => format!("{w}, {} {m} {}", d.d, d.y),
        Lang::DeDE => format!("{w}, {}. {m} {}", d.d, d.y),
        Lang::EsES | Lang::PtBR | Lang::PtPT => format!("{w}, {} de {m} de {}", d.d, d.y),
        Lang::FrFR | Lang::ItIT => format!("{w} {} {m} {}", d.d, d.y),
        Lang::JaJP => format!("{}年{}月{}日{w}", d.y, d.m, d.d),
        Lang::ZhCN => format!("{}年{}月{}日 {w}", d.y, d.m, d.d),
    }
}

/// "October 17, 2026" in the current language (date fields).
pub fn short_date(d: Date) -> String {
    short_date_in(lang(), d)
}

pub fn short_date_in(l: Lang, d: Date) -> String {
    let m = months(l)[d.m as usize - 1];
    match l {
        Lang::EnUS => format!("{m} {}, {}", d.d, d.y),
        Lang::EnGB => format!("{} {m} {}", d.d, d.y),
        Lang::DeDE => format!("{}. {m} {}", d.d, d.y),
        Lang::EsES | Lang::PtBR | Lang::PtPT => format!("{} de {m} de {}", d.d, d.y),
        Lang::FrFR | Lang::ItIT => format!("{} {m} {}", d.d, d.y),
        Lang::JaJP | Lang::ZhCN => format!("{}年{}月{}日", d.y, d.m, d.d),
    }
}

/// Calendar title, e.g. "October 2026", "Oktober 2026", "2026年10月".
pub fn month_year(y: i32, m: u32) -> String {
    month_year_in(lang(), y, m)
}

pub fn month_year_in(l: Lang, y: i32, m: u32) -> String {
    let name = months(l)[m as usize - 1];
    match l {
        Lang::EsES | Lang::PtBR | Lang::PtPT => format!("{} de {y}", capitalize(name)),
        Lang::JaJP | Lang::ZhCN => format!("{y}年{m}月"),
        _ => format!("{} {y}", capitalize(name)),
    }
}

/// (singular, plural) unit words for year, month, week, day.
fn units(l: Lang) -> [(&'static str, &'static str); 4] {
    match l {
        Lang::EnUS | Lang::EnGB => [("year", "years"), ("month", "months"), ("week", "weeks"), ("day", "days")],
        Lang::DeDE => [("Jahr", "Jahre"), ("Monat", "Monate"), ("Woche", "Wochen"), ("Tag", "Tage")],
        Lang::EsES => [("año", "años"), ("mes", "meses"), ("semana", "semanas"), ("día", "días")],
        Lang::FrFR => [("an", "ans"), ("mois", "mois"), ("semaine", "semaines"), ("jour", "jours")],
        Lang::ItIT => [("anno", "anni"), ("mese", "mesi"), ("settimana", "settimane"), ("giorno", "giorni")],
        Lang::PtBR | Lang::PtPT => [("ano", "anos"), ("mês", "meses"), ("semana", "semanas"), ("dia", "dias")],
        Lang::JaJP => [("年", "年"), ("か月", "か月"), ("週間", "週間"), ("日", "日")],
        Lang::ZhCN => [("年", "年"), ("个月", "个月"), ("周", "周"), ("天", "天")],
    }
}

fn count(l: Lang, n: i64, unit: (&str, &str)) -> String {
    let one = if l == Lang::FrFR { n <= 1 } else { n == 1 };
    let word = if one { unit.0 } else { unit.1 };
    let n = super::num_in(l, &crate::format::group_decimal(&n.to_string()));
    format!("{n} {word}")
}

/// "1 year, 2 months, 3 weeks, 4 days" or "Same dates".
pub fn difference(diff: &Difference) -> String {
    difference_in(lang(), diff)
}

pub fn difference_in(l: Lang, diff: &Difference) -> String {
    if diff.total_days == 0 {
        return super::tr_in(l, "Same dates").to_string();
    }
    let u = units(l);
    let parts: Vec<String> = [diff.years, diff.months, diff.weeks, diff.days]
        .into_iter()
        .zip(u)
        .filter(|(n, _)| *n != 0)
        .map(|(n, unit)| count(l, n, unit))
        .collect();
    let sep = match l {
        Lang::JaJP => "、",
        Lang::ZhCN => "，",
        _ => ", ",
    };
    parts.join(sep)
}

/// "443 days".
pub fn total_days(diff: &Difference) -> String {
    total_days_in(lang(), diff)
}

pub fn total_days_in(l: Lang, diff: &Difference) -> String {
    count(l, diff.total_days, units(l)[3])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, dd: u32) -> Date {
        Date::new(y, m, dd).unwrap()
    }

    #[test]
    fn long_dates_per_locale() {
        let x = d(2026, 10, 17);
        assert_eq!(long_date_in(Lang::EnUS, x), "Saturday, October 17, 2026");
        assert_eq!(long_date_in(Lang::EnGB, x), "Saturday, 17 October 2026");
        assert_eq!(long_date_in(Lang::DeDE, x), "Samstag, 17. Oktober 2026");
        assert_eq!(long_date_in(Lang::EsES, x), "sábado, 17 de octubre de 2026");
        assert_eq!(long_date_in(Lang::FrFR, x), "samedi 17 octobre 2026");
        assert_eq!(long_date_in(Lang::ItIT, x), "sabato 17 ottobre 2026");
        assert_eq!(long_date_in(Lang::PtBR, x), "sábado, 17 de outubro de 2026");
        assert_eq!(long_date_in(Lang::JaJP, x), "2026年10月17日土曜日");
        assert_eq!(long_date_in(Lang::ZhCN, x), "2026年10月17日 星期六");
        assert_eq!(long_date_in(Lang::EnUS, d(2024, 2, 29)), "Thursday, February 29, 2024");
        assert_eq!(short_date_in(Lang::DeDE, x), "17. Oktober 2026");
        assert_eq!(month_year_in(Lang::EsES, 2026, 10), "Octubre de 2026");
        assert_eq!(month_year_in(Lang::JaJP, 2026, 10), "2026年10月");
        assert_eq!(first_weekday_in(Lang::DeDE), 1);
        assert_eq!(first_weekday_in(Lang::EnUS), 0);
    }

    #[test]
    fn difference_phrases() {
        let diff = crate::date::difference(d(2026, 1, 1), d(2027, 3, 20));
        assert_eq!(difference_in(Lang::EnUS, &diff), "1 year, 2 months, 2 weeks, 5 days");
        assert_eq!(difference_in(Lang::DeDE, &diff), "1 Jahr, 2 Monate, 2 Wochen, 5 Tage");
        assert_eq!(difference_in(Lang::FrFR, &diff), "1 an, 2 mois, 2 semaines, 5 jours");
        assert_eq!(difference_in(Lang::JaJP, &diff), "1 年、2 か月、2 週間、5 日");
        assert_eq!(total_days_in(Lang::EnUS, &diff), "443 days");
        let one = crate::date::difference(d(2026, 5, 5), d(2026, 5, 12));
        assert_eq!(difference_in(Lang::EnUS, &one), "1 week");
        let same = crate::date::difference(d(2026, 5, 5), d(2026, 5, 5));
        assert_eq!(difference_in(Lang::EnUS, &same), "Same dates");
        assert_eq!(difference_in(Lang::DeDE, &same), "Gleiche Daten");
        let long = crate::date::difference(d(2000, 1, 1), d(2026, 1, 1));
        assert_eq!(total_days_in(Lang::DeDE, &long), "9.497 Tage");
    }
}
