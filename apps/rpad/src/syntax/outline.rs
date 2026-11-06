//! Document outline: headings, functions, types and sections found with
//! simple per-language patterns (no full parsing).

use super::lang::Lang;
use regex::Regex;
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub line: usize,
    pub name: String,
    pub kind: &'static str,
    pub depth: usize,
}

struct Pattern {
    re: Regex,
    kind: &'static str,
}

fn patterns(lang: Lang) -> &'static [Pattern] {
    static CACHE: OnceLock<Vec<(Lang, Vec<Pattern>)>> = OnceLock::new();
    let all = CACHE.get_or_init(|| {
        let p = |re: &str, kind| Pattern { re: Regex::new(re).expect("outline pattern"), kind };
        let c_like_fn = r"^\s*(?:[\w:<>\[\],\*&~]+\s+)+\**([A-Za-z_~][\w:~]*)\s*\([^;]*$";
        vec![
            (
                Lang::Rust,
                vec![
                    p(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?(?:extern\s+\S+\s+)?fn\s+(\w+)", "function"),
                    p(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|union|trait|type)\s+(\w+)", "type"),
                    p(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)", "module"),
                    p(r"^\s*(?:unsafe\s+)?impl(?:<[^>]*>)?\s+(.+?)\s*(?:\{|where|$)", "impl"),
                    p(r"^\s*macro_rules!\s*(\w+)", "macro"),
                ],
            ),
            (Lang::Python, vec![p(r"^\s*(?:async\s+)?def\s+(\w+)", "function"), p(r"^\s*class\s+(\w+)", "class")]),
            (
                Lang::JavaScript,
                vec![
                    p(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\*?\s+(\w+)", "function"),
                    p(r"^\s*(?:export\s+)?(?:default\s+)?class\s+(\w+)", "class"),
                    p(r"^\s*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s*)?(?:\([^)]*\)|\w+)\s*=>", "function"),
                ],
            ),
            (
                Lang::TypeScript,
                vec![
                    p(r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\*?\s+(\w+)", "function"),
                    p(r"^\s*(?:export\s+)?(?:default\s+)?(?:abstract\s+)?(?:class|interface|enum|type)\s+(\w+)", "type"),
                    p(r"^\s*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s*)?(?:\([^)]*\)|\w+)\s*=>", "function"),
                ],
            ),
            (Lang::Go, vec![p(r"^func\s+(?:\([^)]*\)\s*)?(\w+)", "function"), p(r"^type\s+(\w+)", "type")]),
            (
                Lang::Java,
                vec![
                    p(r"^\s*(?:[\w@<>]+\s+)*(?:class|interface|enum|record)\s+(\w+)", "class"),
                    p(r"^\s*(?:(?:public|private|protected|static|final|abstract|synchronized|native|default)\s+)+[\w<>\[\],\s]+\s+(\w+)\s*\(", "method"),
                ],
            ),
            (
                Lang::CSharp,
                vec![
                    p(r"^\s*(?:[\w]+\s+)*(?:class|interface|struct|enum|record)\s+(\w+)", "class"),
                    p(r"^\s*namespace\s+([\w.]+)", "namespace"),
                    p(r"^\s*(?:(?:public|private|protected|internal|static|virtual|override|async|abstract|sealed)\s+)+[\w<>\[\],?\s]+\s+(\w+)\s*\(", "method"),
                ],
            ),
            (Lang::C, vec![p(r"^\s*(?:typedef\s+)?(?:struct|enum|union)\s+(\w+)\s*\{?\s*$", "type"), p(c_like_fn, "function")]),
            (
                Lang::Cpp,
                vec![
                    p(r"^\s*(?:template\s*<[^>]*>\s*)?(?:class|struct|enum(?:\s+class)?|union)\s+(\w+)[^;]*$", "type"),
                    p(r"^\s*namespace\s+(\w+)", "namespace"),
                    p(c_like_fn, "function"),
                ],
            ),
            (Lang::Css, vec![p(r"^\s*([^{}/@][^{}]*?)\s*\{", "rule"), p(r"^\s*(@media[^{]*)\{", "media")]),
            (Lang::Sql, vec![p(r"(?i)^\s*create\s+(?:or\s+replace\s+)?(?:table|view|procedure|function|index|trigger)\s+([\w.\[\]`]+)", "object")]),
            (Lang::Shell, vec![p(r"^\s*(?:function\s+)?(\w[\w-]*)\s*\(\)\s*\{?", "function"), p(r"^\s*function\s+(\w[\w-]*)", "function")]),
            (Lang::PowerShell, vec![p(r"(?i)^\s*function\s+([\w-]+)", "function"), p(r"(?i)^\s*class\s+(\w+)", "class")]),
            (Lang::Batch, vec![p(r"^:([^:\s]\S*)", "label")]),
            (Lang::Ini, vec![p(r"^\s*\[([^\]]+)\]", "section")]),
            (Lang::Toml, vec![p(r"^\s*\[\[?([^\]]+)\]\]?", "table")]),
            (Lang::Yaml, vec![p(r"^([\w.-]+)\s*:", "key")]),
        ]
    });
    all.iter().find(|(l, _)| *l == lang).map_or(&[], |(_, v)| v.as_slice())
}

const NOT_FUNCTIONS: &[&str] = &["if", "for", "while", "switch", "return", "else", "catch", "sizeof", "do", "case", "new", "delete"];

/// Extracts outline symbols from the lines of a document.
pub fn outline<'a>(lang: Lang, lines: impl Iterator<Item = &'a str>) -> Vec<Symbol> {
    let mut out = Vec::new();
    if lang == Lang::Markdown {
        let mut in_fence = false;
        for (i, l) in lines.enumerate() {
            let t = l.trim_start();
            if t.starts_with("```") || t.starts_with("~~~") {
                in_fence = !in_fence;
                continue;
            }
            let n = t.chars().take_while(|c| *c == '#').count();
            if !in_fence && (1..=6).contains(&n) && t[n..].starts_with(' ') {
                out.push(Symbol { line: i, name: t[n..].trim().trim_end_matches('#').trim().to_string(), kind: "heading", depth: n - 1 });
            }
        }
        return out;
    }
    let pats = patterns(lang);
    if pats.is_empty() {
        return out;
    }
    for (i, l) in lines.enumerate() {
        if l.len() > 2000 {
            continue;
        }
        for p in pats {
            if let Some(c) = p.re.captures(l) {
                let name = c.get(1).map_or("", |m| m.as_str()).trim().to_string();
                if name.is_empty() || NOT_FUNCTIONS.contains(&name.as_str()) {
                    continue;
                }
                let indent = l.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>();
                out.push(Symbol { line: i, name, kind: p.kind, depth: indent / 4 });
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(lang: Lang, src: &str) -> Vec<String> {
        outline(lang, src.lines()).into_iter().map(|s| s.name).collect()
    }

    #[test]
    fn languages() {
        assert_eq!(names(Lang::Rust, "pub fn a() {}\nstruct B;\nimpl B {\n    async fn c(&self) {}\n}\nmod m;"), ["a", "B", "B", "c", "m"]);
        assert_eq!(names(Lang::Python, "class A:\n    def f(self):\n        pass\nasync def g(): pass"), ["A", "f", "g"]);
        assert_eq!(names(Lang::JavaScript, "function a() {}\nconst b = (x) => x;\nexport class C {}"), ["a", "b", "C"]);
        assert_eq!(names(Lang::C, "int main(int argc, char **argv)\n{\n    if (x) {\n    }\n}\nstatic void helper(void) {"), ["main", "helper"]);
        assert_eq!(names(Lang::Markdown, "# One\ntext\n## Two ##\n```\n# not\n```"), ["One", "Two"]);
        assert_eq!(names(Lang::Ini, "[a]\nx=1\n[b c]"), ["a", "b c"]);
        assert_eq!(names(Lang::Sql, "CREATE TABLE users (id int);\ncreate or replace view v as select 1;"), ["users", "v"]);
        assert!(names(Lang::Plain, "fn a() {}").is_empty());
    }
}
