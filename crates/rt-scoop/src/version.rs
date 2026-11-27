//! `lib\versions.ps1`: `Compare-Version` and `SplitVersion`, ported with their
//! quirks, because "outdated" must mean the same as in Scoop.

use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq)]
enum Part {
    Num(i64),
    Text(String),
}

impl Part {
    fn text(&self) -> String {
        match self {
            Part::Num(n) => n.to_string(),
            Part::Text(s) => s.clone(),
        }
    }

    fn is_pre_release(&self) -> bool {
        let s = self.text().to_ascii_lowercase();
        ["alpha", "beta", "rc", "pre"].iter().any(|p| s.contains(p))
    }
}

/// Letters are set apart by the delimiter, then the text is split at it.
fn split(version: &str, delimiter: char) -> Vec<Part> {
    let mut spaced = String::new();
    let mut letters = false;
    for c in version.chars() {
        if c.is_ascii_alphabetic() != letters {
            spaced.push(delimiter);
            letters = !letters;
        }
        spaced.push(c);
    }
    spaced
        .split(delimiter)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let digits = s.bytes().all(|b| b.is_ascii_digit());
            match s.parse::<i64>() {
                Ok(n) if digits => Part::Num(n),
                _ => Part::Text(s.to_string()),
            }
        })
        .collect()
}

/// Greater when `difference` comes after `reference` (Scoop's 1), Less when it
/// comes before (-1). `today` (`yyyyMMdd`) is only used with `update_nightly`.
pub fn compare(reference: &str, difference: &str, update_nightly: Option<&str>) -> Ordering {
    compare_at(reference, difference, '-', update_nightly)
}

fn compare_at(reference: &str, difference: &str, delimiter: char, nightly: Option<&str>) -> Ordering {
    // '+' marks a post-release.
    let (reference, difference) = (reference.replace('+', "-"), difference.replace('+', "-"));
    if reference.eq_ignore_ascii_case(&difference) {
        return Ordering::Equal;
    }
    let a = split(&reference, delimiter);
    let b = split(&difference, delimiter);
    let is_nightly = |v: &[Part]| v.first().is_some_and(|p| p.text().eq_ignore_ascii_case("nightly"));
    if is_nightly(&a) && is_nightly(&b) {
        let Some(today) = nightly else { return Ordering::Equal };
        let date = |v: &[Part]| v.get(1).map_or_else(|| today.to_string(), Part::text).parse::<i64>().unwrap_or(0);
        return date(&b).cmp(&date(&a));
    }
    for i in 0..a.len().max(b.len()) {
        // '1.1-alpha' is less than '1.1', and '1.1' greater than '1.1-beta'.
        let Some(x) = a.get(i) else {
            return if b[i].is_pre_release() { Ordering::Less } else { Ordering::Greater };
        };
        let Some(y) = b.get(i) else {
            return if x.is_pre_release() { Ordering::Greater } else { Ordering::Less };
        };
        let (xs, ys) = (x.text(), y.text());
        if let Some(inner) = ['.', '_'].into_iter().find(|c| xs.contains(*c) || ys.contains(*c)) {
            match compare_at(&xs, &ys, inner, nightly) {
                Ordering::Equal => continue,
                other => return other,
            }
        }
        let order = match (x, y) {
            (Part::Num(x), Part::Num(y)) => y.cmp(x),
            _ => ys.to_ascii_lowercase().cmp(&xs.to_ascii_lowercase()),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

#[cfg(test)]
mod tests {
    use super::*;
    use Ordering::{Equal, Greater, Less};

    /// The cases of Scoop's `test\Scoop-Versions.Tests.ps1`.
    #[test]
    fn as_scoop_compares() {
        let cases = [
            ("0.1.0", "0.1.1", Greater),
            ("0.1.1", "0.2.0", Greater),
            ("0.2.0", "1.0.0", Greater),
            ("0.4.0", "0.5.0-alpha.1", Greater),
            ("0.5.0-alpha.1", "0.5.0-alpha.2", Greater),
            ("0.5.0-alpha.2", "0.5.0-alpha.10", Greater),
            ("0.5.0-alpha.10", "0.5.0-beta", Greater),
            ("0.5.0-beta", "0.5.0-alpha.10", Less),
            ("0.5.0-beta", "0.5.0-beta.0", Greater),
            ("0.5.0-rc.1", "0.5.0-z", Greater),
            ("0.5.0-rc.1", "0.5.0-howdy", Less),
            ("0.5.0-howdy", "0.5.0-rc.1", Greater),
            ("0.0.0.0", "0.0.0.1", Greater),
            ("0.0.1.2", "0.0.2.0", Greater),
            ("0.2.0.0", "1.0.0.0", Greater),
            ("1", "1.1", Greater),
            ("1", "1.0", Greater),
            ("1.1.0.0", "1.1", Less),
            ("1.4", "1.3.0", Less),
            ("1.4", "1.3.255.255", Less),
            ("1.4", "1.4.4", Greater),
            ("1.1.1_8", "1.1.1", Less),
            ("1.1.1_8", "1.1.1_9", Greater),
            ("1.1.1_10", "1.1.1_9", Less),
            ("1.1.1b", "1.1.1a", Less),
            ("1.1.1a", "1.1.1b", Greater),
            ("1.1a2", "1.1a3", Greater),
            ("1.1.1a10", "1.1.1b1", Greater),
            ("1.8.9", "1.8.5-1", Less),
            ("7.0.4-9", "7.0.4-10", Greater),
            ("7.0.4-9", "7.0.4-8", Less),
            ("2019-01-01", "2019-01-02", Greater),
            ("2019-01-02", "2019-01-01", Less),
            ("2018-01-01", "2019-01-01", Greater),
            ("1", "1+hotfix.0", Greater),
            ("1.0.0", "1.0.0+hotfix.0", Greater),
            ("1.0.0+hotfix.0", "1.0.0+hotfix.1", Greater),
            ("1.0.0+hotfix.1", "1.0.1", Greater),
            ("1.0.0+1.1", "1.0.0+1", Less),
            ("latest", "20150405", Less),
            ("0.5alpha", "0.5", Greater),
            ("0.5", "0.5Beta", Less),
            ("0.4", "0.5Beta", Greater),
            ("7.0.4-9", "", Less),
            ("12.0", "12.0", Equal),
            ("7.0.4-9", "7.0.4-9", Equal),
            ("nightly-20190801", "nightly", Equal),
            ("nightly-20190801", "nightly-20200801", Equal),
        ];
        for (a, b, want) in cases {
            assert_eq!(compare(a, b, None), want, "{a} -> {b}");
        }
    }

    #[test]
    fn nightly_by_date_when_asked() {
        let today = Some("20200801");
        assert_eq!(compare("nightly-20200801", "nightly", today), Equal);
        assert_eq!(compare("nightly-20200730", "nightly", today), Greater);
        assert_eq!(compare("nightly-20200730", "nightly-20200801", today), Greater);
        assert_eq!(compare("nightly-20200802", "nightly", today), Less);
        assert_eq!(compare("nightly-20200802", "nightly-20200801", today), Less);
    }
}
