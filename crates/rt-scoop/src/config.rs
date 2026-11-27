//! `set_config` and `save_install_info` (`lib\core.ps1`, `lib\manifest.ps1`):
//! the two JSON files Scoop rewrites.

use crate::disk::write_file;
use crate::json::{self, Json};
use std::path::Path;

/// Changes one member of the JSON object in `path`, keeping the others and
/// their order; names ignore case. None removes it. A file that is there but
/// cannot be read is left alone.
pub fn set_member(path: &Path, name: &str, value: Option<Json>) -> Result<(), String> {
    let mut members = match std::fs::metadata(path) {
        Err(_) => Vec::new(),
        Ok(_) => match json::read(path) {
            Some(Json::Obj(members)) => members,
            _ => return Err(format!("{} is not valid JSON; nothing was changed", path.display())),
        },
    };
    let found = members.iter().position(|(k, _)| k.eq_ignore_ascii_case(name));
    match (found, value) {
        (Some(i), Some(value)) => members[i].1 = value,
        (None, Some(value)) => members.push((name.to_string(), value)),
        (Some(i), None) => {
            members.remove(i);
        }
        (None, None) => return Ok(()),
    }
    let text = format!("{}\r\n", json::pretty(&Json::Obj(members)).replace('\n', "\r\n"));
    write_file(path, &text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The value `scoop config <name> <value>` stores for the text typed.
pub fn typed_value(text: &str) -> Json {
    match text {
        t if t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("$true") => Json::Bool(true),
        t if t.eq_ignore_ascii_case("false") || t.eq_ignore_ascii_case("$false") => Json::Bool(false),
        t => match t.parse::<i32>() {
            Ok(n) if n.to_string() == t => Json::Num(f64::from(n)),
            _ => Json::Str(t.to_string()),
        },
    }
}

/// Seconds since 1970 of `2026-11-27T10:20:30.1234567+01:00`, `…Z`, a date
/// (`2026-11-27` or with `/`), or a date and time; `local_offset` (seconds)
/// applies when the text names no zone.
pub fn parse_time(text: &str, local_offset: i64) -> Option<i64> {
    let text = text.trim();
    let (date, rest) = match text.find(['T', ' ']) {
        Some(i) => (&text[..i], &text[i + 1..]),
        None => (text, ""),
    };
    let mut parts = date.split(['-', '/']).map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    if parts.next().is_some() || !(1..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let zone_at = rest.find(['Z', '+', '-']);
    let (time, zone) = zone_at.map_or((rest, ""), |i| rest.split_at(i));
    let mut clock = time.split(':').map(|p| p.split('.').next().unwrap_or("").parse::<i64>().ok());
    let (h, min, s) = if time.is_empty() { (0, 0, 0) } else { (clock.next()??, clock.next()??, clock.next().unwrap_or(Some(0))?) };
    let offset = match zone {
        "" => local_offset,
        "Z" => 0,
        zone => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let (zh, zm) = zone[1..].split_once(':')?;
            sign * (zh.parse::<i64>().ok()? * 3600 + zm.parse::<i64>().ok()? * 60)
        }
    };
    // Days since 1970 of a civil date (Howard Hinnant's algorithm).
    let y = y - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    Some(days * 86_400 + h * 3600 + min * 60 + s - offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Tree;

    #[test]
    fn members_are_changed_and_the_rest_kept() {
        let t = Tree::new("config");
        let file = t.dir("c").join("sub").join("config.json");
        set_member(&file, "last_update", Some(Json::Str("x".into()))).unwrap();
        std::fs::write(&file, "\u{feff}{\"A\": 1, \"private_hosts\": [{\"match\": \"m\"}], \"b\": true}").unwrap();
        set_member(&file, "a", Some(Json::Num(2.0))).unwrap();
        set_member(&file, "b", None).unwrap();
        set_member(&file, "c", Some(typed_value("True"))).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.ends_with("}\r\n") && text.starts_with("{\r\n    \"A\": 2,"), "{text}");
        let j = json::parse(&text).unwrap();
        assert_eq!(j.get("private_hosts").at(0).get("match").str(), "m");
        assert_eq!((j.get("b"), j.get("c")), (&Json::Null, &Json::Bool(true)));
        // A broken file stays as it is.
        std::fs::write(&file, "{ broken").unwrap();
        assert!(set_member(&file, "a", None).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ broken");
        assert_eq!(typed_value("8"), Json::Num(8.0));
        assert_eq!(typed_value("007"), Json::Str("007".into()));
    }

    #[test]
    fn times_as_scoop_writes_them() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z", 0), Some(0));
        assert_eq!(parse_time("2026-11-27T14:29:10.8473538+00:00", 3600), Some(1_795_789_750));
        assert_eq!(parse_time("2026-11-27T15:29:10+01:00", 0), Some(1_795_789_750));
        assert_eq!(parse_time("2026/11/27", 0), parse_time("2026-11-27 00:00:00", 0));
        assert_eq!(parse_time("2026-11-27", 3600), parse_time("2026-11-26T23:00:00Z", 0));
        assert_eq!(parse_time("tomorrow", 0), None);
        assert_eq!(parse_time("2026-13-01", 0), None);
    }
}
