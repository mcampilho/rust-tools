//! Fold ranges: brace pairs, indentation blocks, XML/HTML elements and
//! Markdown sections. Each range is `(first line, last line)` inclusive.

use super::lang::{def, FoldStyle, Lang};
use super::lexer::{lex_line, Span, Tok};
use ropey::Rope;

/// Computes fold ranges for the whole document.
pub fn compute(text: &Rope, lang: Lang) -> Vec<(usize, usize)> {
    let d = def(lang);
    let lines: Vec<String> = (0..text.len_lines())
        .map(|l| {
            let s = text.line(l).to_string();
            s.strip_suffix('\n').map(str::to_string).unwrap_or(s)
        })
        .collect();
    let mut out = match d.fold {
        FoldStyle::None => Vec::new(),
        FoldStyle::Indent => indent_ranges(&lines),
        FoldStyle::Braces | FoldStyle::Tags => token_ranges(&lines, lang),
        FoldStyle::Headings => heading_ranges(&lines),
    };
    out.retain(|(a, b)| b > a);
    out.sort();
    out.dedup();
    out
}

fn indent_of(s: &str) -> Option<usize> {
    if s.trim().is_empty() {
        return None;
    }
    Some(s.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum())
}

fn indent_ranges(lines: &[String]) -> Vec<(usize, usize)> {
    let ind: Vec<Option<usize>> = lines.iter().map(|l| indent_of(l)).collect();
    let mut out = Vec::new();
    let mut stack: Vec<(usize, usize)> = Vec::new(); // (indent, start line)
    let mut last_nonblank = 0;
    for (i, cur) in ind.iter().enumerate() {
        let Some(cur) = *cur else { continue };
        while let Some(&(si, start)) = stack.last() {
            if cur <= si {
                stack.pop();
                if last_nonblank > start {
                    out.push((start, last_nonblank));
                }
            } else {
                break;
            }
        }
        stack.push((cur, i));
        last_nonblank = i;
    }
    while let Some((_, start)) = stack.pop() {
        if last_nonblank > start {
            out.push((start, last_nonblank));
        }
    }
    out
}

fn token_ranges(lines: &[String], lang: Lang) -> Vec<(usize, usize)> {
    let d = def(lang);
    let markup = d.fold == FoldStyle::Tags;
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut comment_start: Option<usize> = None;
    let mut st = 0;
    let mut spans: Vec<Span> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let before = st;
        st = lex_line(d, line, st, &mut spans);
        // Multi-line comments fold as a block.
        let in_comment_after = st & 0xFF == 1 || (markup && st & 0xFF == 4);
        let in_comment_before = before & 0xFF == 1 || (markup && before & 0xFF == 4);
        if in_comment_after && !in_comment_before {
            comment_start = Some(i);
        }
        if !in_comment_after && in_comment_before {
            if let Some(s) = comment_start.take() {
                out.push((s, i));
            }
        }
        for s in &spans {
            let t = &line[s.start..s.end];
            if markup {
                if s.tok != Tok::Tag {
                    continue;
                }
                if t.starts_with("</") {
                    if let Some(open) = stack.pop() {
                        out.push((open, i));
                    }
                } else if t == "/>" {
                    stack.pop();
                } else if t.starts_with('<') && !is_void(&t[1..]) {
                    stack.push(i);
                }
            } else if s.tok == Tok::Punct {
                match t {
                    "{" | "[" => stack.push(i),
                    "}" | "]" => {
                        if let Some(open) = stack.pop() {
                            out.push((open, i));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

/// HTML elements without a closing tag.
fn is_void(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "source" | "track" | "wbr"
    )
}

fn heading_ranges(lines: &[String]) -> Vec<(usize, usize)> {
    let mut heads: Vec<(usize, usize)> = Vec::new(); // (line, level)
    let mut in_fence = false;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let n = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&n) && t[n..].starts_with(' ') {
            heads.push((i, n));
        }
    }
    let mut out = Vec::new();
    for (k, &(line, level)) in heads.iter().enumerate() {
        let next = heads[k + 1..].iter().find(|(_, lv)| *lv <= level).map_or(lines.len(), |(l, _)| *l);
        let mut end = next.saturating_sub(1);
        while end > line && lines[end].trim().is_empty() {
            end -= 1;
        }
        out.push((line, end));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braces_and_comments() {
        let t = Rope::from_str("fn a() {\n    if x {\n        y();\n    }\n}\n/* one\n two */\nlet s = \"{\";");
        assert_eq!(compute(&t, Lang::Rust), vec![(0, 4), (1, 3), (5, 6)]);
    }

    #[test]
    fn indentation() {
        let t = Rope::from_str("def f():\n    a = 1\n\n    if a:\n        b()\nx = 2\n");
        assert_eq!(compute(&t, Lang::Python), vec![(0, 4), (3, 4)]);
    }

    #[test]
    fn tags() {
        let t = Rope::from_str("<root>\n  <a x=\"1\"/>\n  <b>\n    <br>\n  </b>\n</root>");
        assert_eq!(compute(&t, Lang::Xml), vec![(0, 5), (2, 4)]);
    }

    #[test]
    fn headings() {
        let t = Rope::from_str("# A\ntext\n## B\nmore\n\n# C\nend");
        assert_eq!(compute(&t, Lang::Markdown), vec![(0, 3), (2, 3), (5, 6)]);
    }

    #[test]
    fn plain_has_none() {
        assert!(compute(&Rope::from_str("{\n}\n"), Lang::Plain).is_empty());
    }
}
