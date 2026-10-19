//! Find and replace. Uses the `regex` crate, whose matching time is linear
//! in the input (no catastrophic backtracking); patterns are compiled with
//! size limits so pathological expressions fail with an error instead of
//! exhausting memory.

pub mod files;

use regex::{Regex, RegexBuilder};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    pub case: bool,
    pub word: bool,
    pub regex: bool,
}

impl Query {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Compiles the query. Errors are shown to the user.
    pub fn build(&self) -> Result<Regex, String> {
        if self.text.is_empty() {
            return Err(crate::i18n::tr("Empty search"));
        }
        let mut pat = if self.regex { self.text.clone() } else { regex::escape(&self.text) };
        if self.word {
            let first = self.text.chars().next().is_some_and(is_word);
            let last = self.text.chars().last().is_some_and(is_word);
            pat = format!("{}(?:{pat}){}", if first || self.regex { r"\b" } else { "" }, if last || self.regex { r"\b" } else { "" });
        }
        RegexBuilder::new(&pat)
            .case_insensitive(!self.case)
            .multi_line(true)
            .size_limit(16 << 20)
            .dfa_size_limit(16 << 20)
            .build()
            .map_err(|e| match e {
                regex::Error::Syntax(s) => s.lines().last().unwrap_or("Invalid regular expression").trim().to_string(),
                regex::Error::CompiledTooBig(_) => crate::i18n::tr("The regular expression is too large"),
                _ => crate::i18n::tr("Invalid regular expression"),
            })
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// All matches (byte ranges), at most `limit`; also returns the total count
/// (counting stops at `count_limit`).
pub fn find_all(text: &str, re: &Regex, limit: usize, count_limit: usize) -> (Vec<(usize, usize)>, usize) {
    let mut v = Vec::new();
    let mut n = 0;
    for m in re.find_iter(text) {
        if n < limit {
            v.push((m.start(), m.end()));
        }
        n += 1;
        if n >= count_limit {
            break;
        }
    }
    (v, n)
}

/// Next match from byte `from` (forward) or before `from` (backward),
/// optionally wrapping around. Empty matches at `from` are skipped forward.
pub fn find_next(text: &str, re: &Regex, from: usize, forward: bool, wrap: bool) -> Option<(usize, usize)> {
    let from = from.min(text.len());
    if forward {
        let mut start = from;
        while start <= text.len() {
            match re.find_at(text, start) {
                Some(m) if m.start() == m.end() && m.start() == from => {
                    // skip an empty match at the caret
                    start = next_boundary(text, start);
                    if start > text.len() {
                        break;
                    }
                    continue;
                }
                Some(m) => return Some((m.start(), m.end())),
                None => break,
            }
        }
        if wrap {
            return re.find(text).map(|m| (m.start(), m.end())).filter(|m| m.0 < from || from == 0);
        }
        None
    } else {
        let mut last = None;
        for m in re.find_iter(text) {
            if m.end() <= from && !(m.start() == m.end() && m.start() == from) {
                last = Some((m.start(), m.end()));
            } else {
                break;
            }
        }
        if last.is_none() && wrap {
            last = re.find_iter(text).last().map(|m| (m.start(), m.end()));
        }
        last
    }
}

fn next_boundary(text: &str, i: usize) -> usize {
    let mut j = i + 1;
    while j < text.len() && !text.is_char_boundary(j) {
        j += 1;
    }
    j
}

/// Interprets `\n`, `\t`, `\r` and `\\` in a regex-mode replacement.
fn unescape_replacement(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Replacement text for one match. In regex mode `$1`/`${name}` refer to
/// groups and `\n`/`\t` are escapes; otherwise the text is literal.
pub fn replacement_for(text: &str, re: &Regex, m: (usize, usize), replacement: &str, regex_mode: bool) -> String {
    if !regex_mode {
        return replacement.to_string();
    }
    let rep = unescape_replacement(replacement);
    match re.captures_at(text, m.0) {
        Some(caps) if caps.get(0).is_some_and(|c| c.start() == m.0) => {
            let mut out = String::new();
            caps.expand(&rep, &mut out);
            out
        }
        _ => rep,
    }
}

/// One planned replacement in byte offsets with the new text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Plans all replacements within `range` (byte offsets).
pub fn plan_replace_all(text: &str, re: &Regex, replacement: &str, regex_mode: bool, range: (usize, usize)) -> Vec<Planned> {
    let rep = if regex_mode { unescape_replacement(replacement) } else { replacement.to_string() };
    let mut out = Vec::new();
    let hay = &text[..range.1.min(text.len())];
    for caps in re.captures_iter(hay) {
        let m = caps.get(0).expect("group 0");
        if m.start() < range.0 {
            continue;
        }
        let new = if regex_mode {
            let mut s = String::new();
            caps.expand(&rep, &mut s);
            s
        } else {
            rep.clone()
        };
        out.push(Planned { start: m.start(), end: m.end(), text: new });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str, case: bool, word: bool, regex: bool) -> Regex {
        Query { text: text.into(), case, word, regex }.build().unwrap()
    }

    #[test]
    fn options() {
        let t = "Foo foo food FOO";
        assert_eq!(find_all(t, &q("foo", false, false, false), 10, 100).1, 4);
        assert_eq!(find_all(t, &q("foo", true, false, false), 10, 100).1, 2);
        assert_eq!(find_all(t, &q("foo", false, true, false), 10, 100).1, 3);
        assert_eq!(find_all(t, &q("f.o", false, false, false), 10, 100).1, 0);
        assert_eq!(find_all(t, &q("f.o", false, false, true), 10, 100).1, 4);
        assert_eq!(find_all("a.b a.b", &q("a.b", true, true, false), 10, 100).1, 2);
        assert_eq!(find_all("x\ny\n", &q("^y$", true, false, true), 10, 100).1, 1);
    }

    #[test]
    fn invalid_patterns_report_errors() {
        let e = Query { text: "(".into(), regex: true, ..Query::default() }.build().unwrap_err();
        assert!(!e.is_empty());
        assert!(Query::default().build().is_err());
        assert!(Query { text: "a{1000}{1000}".into(), regex: true, ..Query::default() }.build().is_err());
    }

    #[test]
    fn next_previous_and_wrap() {
        let t = "ab ab ab";
        let re = q("ab", true, false, false);
        assert_eq!(find_next(t, &re, 0, true, false), Some((0, 2)));
        assert_eq!(find_next(t, &re, 1, true, false), Some((3, 5)));
        assert_eq!(find_next(t, &re, 7, true, false), None);
        assert_eq!(find_next(t, &re, 7, true, true), Some((0, 2)));
        assert_eq!(find_next(t, &re, 5, false, false), Some((3, 5)));
        assert_eq!(find_next(t, &re, 1, false, false), None);
        assert_eq!(find_next(t, &re, 1, false, true), Some((6, 8)));
        // empty matches advance
        let re = q("^", true, false, true);
        assert_eq!(find_next("a\nb", &re, 0, true, false), Some((2, 2)));
    }

    #[test]
    fn replace_with_groups_and_escapes() {
        let t = "John Smith, Jane Doe";
        let re = q(r"(\w+) (\w+)", true, false, true);
        let plan = plan_replace_all(t, &re, "$2 $1", true, (0, t.len()));
        assert_eq!(plan.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["Smith John", "Doe Jane"]);
        let plan = plan_replace_all("a,b", &q(",", true, false, true), r"\n", true, (0, 3));
        assert_eq!(plan[0].text, "\n");
        // literal mode keeps $ and backslashes
        let plan = plan_replace_all("x", &q("x", true, false, false), "$1\\n", false, (0, 1));
        assert_eq!(plan[0].text, "$1\\n");
        assert_eq!(replacement_for("ab", &q("(a)", true, false, true), (0, 1), "[$1]", true), "[a]");
        // range restriction
        assert_eq!(plan_replace_all("aaaa", &q("a", true, false, false), "b", false, (1, 3)).len(), 2);
    }

}
