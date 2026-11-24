//! `$(date)` in Markdown, when the user turns it on (View › Replace $(date) with the Date): the current local
//! date and time are written in its place. Only `date`, with the usual `+FORMAT`; nothing else of the shell is
//! understood, and nothing is ever run.

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether Markdown documents get their `$(date)` replaced (set from the View menu).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Day of the week, 0 = Monday.
fn weekday(y: i32, m: u32, d: u32) -> usize {
    // Sakamoto's method gives 0 = Sunday.
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    let sun0 = (y + y / 4 - y / 100 + y / 400 + T[(m as usize).clamp(1, 12) - 1] + d as i32).rem_euclid(7);
    (sun0 as usize + 6) % 7
}

fn day_of_year(y: i32, m: u32, d: u32) -> u32 {
    const DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let before: u32 = (0..(m as usize).clamp(1, 12) - 1).map(|i| DAYS[i] + u32::from(i == 1 && leap)).sum();
    before + d
}

/// Names of the days and months in the language of the interface (English where there is no table).
fn names(lang: &str) -> (&'static [&'static str; 7], &'static [&'static str; 12]) {
    const EN_D: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    const EN_M: [&str; 12] =
        ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    const PT_D: [&str; 7] = ["segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado", "domingo"];
    const PT_M: [&str; 12] =
        ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
    const ES_D: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];
    const ES_M: [&str; 12] =
        ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
    match lang {
        "pt" => (&PT_D, &PT_M),
        "es" => (&ES_D, &ES_M),
        _ => (&EN_D, &EN_M),
    }
}

/// `date +FORMAT` for `t` (`%Y %y %m %d %e %H %M %S %I %p %j %A %a %B %b %F %T %D %R %% %n %t`).
/// `None` when the format has something else.
pub fn format(fmt: &str, t: &rt_sys::LocalTime, lang: &str) -> Option<String> {
    let (days, months) = names(lang);
    let wd = weekday(t.year, t.month, t.day);
    let mo = (t.month as usize).clamp(1, 12) - 1;
    let short = |s: &str| s.chars().take(3).collect::<String>();
    let mut out = String::new();
    let mut it = fmt.chars();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match it.next()? {
            'Y' => out.push_str(&t.year.to_string()),
            'y' => out.push_str(&format!("{:02}", t.year.rem_euclid(100))),
            'm' => out.push_str(&format!("{:02}", t.month)),
            'd' => out.push_str(&format!("{:02}", t.day)),
            'e' => out.push_str(&format!("{:2}", t.day)),
            'H' => out.push_str(&format!("{:02}", t.hour)),
            'I' => out.push_str(&format!("{:02}", (t.hour + 11) % 12 + 1)),
            'M' => out.push_str(&format!("{:02}", t.minute)),
            'S' => out.push_str(&format!("{:02}", t.second)),
            'p' => out.push_str(if t.hour < 12 { "AM" } else { "PM" }),
            'j' => out.push_str(&format!("{:03}", day_of_year(t.year, t.month, t.day))),
            'A' => out.push_str(days[wd]),
            'a' => out.push_str(&short(days[wd])),
            'B' => out.push_str(months[mo]),
            'b' | 'h' => out.push_str(&short(months[mo])),
            'F' => out.push_str(&format!("{}-{:02}-{:02}", t.year, t.month, t.day)),
            'T' => out.push_str(&format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second)),
            'R' => out.push_str(&format!("{:02}:{:02}", t.hour, t.minute)),
            'D' => out.push_str(&format!("{:02}/{:02}/{:02}", t.month, t.day, t.year.rem_euclid(100))),
            'n' => out.push('\n'),
            't' => out.push('\t'),
            '%' => out.push('%'),
            _ => return None,
        }
    }
    Some(out)
}

/// What `$(date ARGS)` stands for, or `None` if the arguments are not understood.
fn command(args: &str, t: &rt_sys::LocalTime, lang: &str) -> Option<String> {
    let args = args.trim();
    if args.is_empty() {
        return format("%Y-%m-%d %H:%M:%S", t, lang);
    }
    if matches!(args, "-I" | "--iso-8601" | "-I=date" | "--iso-8601=date") {
        return format("%F", t, lang);
    }
    if matches!(args, "-R" | "--rfc-email") {
        return None;
    }
    let f = args.strip_prefix('+')?;
    let f = f
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| f.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(f);
    format(f, t, lang)
}

/// `src` with every `$(date …)` outside code replaced by `t`.
pub fn expand(src: &str, t: &rt_sys::LocalTime, lang: &str) -> String {
    if !src.contains("$(date") {
        return src.to_string();
    }
    let mut out = String::with_capacity(src.len() + 16);
    let mut fence = false;
    for line in src.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = !fence;
            out.push_str(line);
            continue;
        }
        if fence {
            out.push_str(line);
            continue;
        }
        let mut rest = line;
        let mut in_code = false;
        while let Some(p) = rest.find(['`', '$']) {
            out.push_str(&rest[..p]);
            rest = &rest[p..];
            if rest.starts_with('`') {
                in_code = !in_code;
                out.push('`');
                rest = &rest[1..];
            } else if !in_code && rest.starts_with("$(date") && rest[6..].starts_with([')', ' ']) {
                match rest.find(')').and_then(|e| command(&rest[6..e], t, lang).map(|s| (e, s))) {
                    Some((e, s)) => {
                        out.push_str(&s);
                        rest = &rest[e + 1..];
                    }
                    None => {
                        out.push('$');
                        rest = &rest[1..];
                    }
                }
            } else {
                out.push('$');
                rest = &rest[1..];
            }
        }
        out.push_str(rest);
    }
    out
}

/// The text of a document with `$(date)` replaced by now, if the user asked for that.
pub fn apply(src: &str) -> std::borrow::Cow<'_, str> {
    if enabled() && src.contains("$(date") {
        let tag = crate::i18n::lang().tag();
        std::borrow::Cow::Owned(expand(src, &rt_sys::local_time(), tag.split('-').next().unwrap_or("en")))
    } else {
        std::borrow::Cow::Borrowed(src)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> rt_sys::LocalTime {
        rt_sys::LocalTime { year: 2026, month: 11, day: 24, hour: 14, minute: 5, second: 9 }
    }

    #[test]
    fn weekdays_and_days_of_the_year() {
        assert_eq!(weekday(2026, 11, 24), 1, "a Tuesday");
        assert_eq!(weekday(2024, 2, 29), 3, "a Thursday");
        assert_eq!(weekday(2000, 1, 1), 5, "a Saturday");
        assert_eq!(day_of_year(2026, 11, 24), 328);
        assert_eq!(day_of_year(2024, 3, 1), 61);
    }

    #[test]
    fn formats_like_date() {
        assert_eq!(format("%Y-%m-%d", &t(), "en").unwrap(), "2026-11-24");
        assert_eq!(format("%d/%m/%Y %H:%M:%S", &t(), "en").unwrap(), "24/11/2026 14:05:09");
        assert_eq!(format("%A, %B %e", &t(), "en").unwrap(), "Tuesday, November 24");
        assert_eq!(format("%A, %d de %B de %Y", &t(), "pt").unwrap(), "terça-feira, 24 de novembro de 2026");
        assert_eq!(format("%a %b %I%p %j %y %%", &t(), "es").unwrap(), "mar nov 02PM 328 26 %");
        assert_eq!(format("%Q", &t(), "en"), None);
        assert_eq!(format("%", &t(), "en"), None);
    }

    #[test]
    fn expands_outside_code_only() {
        let src = "Updated: $(date)\n\nBy $(date +%F) and $(date +\"%d/%m/%Y\") and $(date -I).\n\n`$(date)` stays\n\n```sh\necho $(date)\n```\n\nPrice $5 and $(date -x) and $(dates) stay.\n";
        let got = expand(src, &t(), "en");
        assert!(got.contains("Updated: 2026-11-24 14:05:09\n"), "{got}");
        assert!(got.contains("By 2026-11-24 and 24/11/2026 and 2026-11-24."), "{got}");
        assert!(got.contains("`$(date)` stays") && got.contains("echo $(date)\n"), "{got}");
        assert!(got.contains("Price $5 and $(date -x) and $(dates) stay."), "{got}");
        assert_eq!(expand("nothing here", &t(), "en"), "nothing here");
    }

    #[test]
    fn off_by_default() {
        assert!(!enabled());
        assert_eq!(apply("$(date)"), "$(date)");
    }
}
