//! Mermaid flowcharts (`graph TD`, `flowchart LR`…) drawn without JavaScript: the text is read into nodes and
//! edges here, and `layout/diagram.rs` places and draws them (layers, boxes, lines and arrowheads). Other
//! kinds of Mermaid diagram are not read (`parse` returns `None`) and stay as code.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    TopDown,
    BottomUp,
    LeftRight,
    RightLeft,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Round,
    Stadium,
    Diamond,
    Circle,
    Hexagon,
    /// A rectangle with double sides (`[[text]]`).
    Subroutine,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: Shape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    Solid,
    Dotted,
    Thick,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: String,
    pub line: Line,
    /// Arrowhead at `to`.
    pub arrow: bool,
    /// Arrowhead at `from` too (`<-->`).
    pub back_arrow: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Diagram {
    pub dir: Dir,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Reads a flowchart; `None` for anything else (or nothing drawable).
pub fn parse(src: &str) -> Option<Diagram> {
    let mut lines = src.lines().map(|l| l.split("%%").next().unwrap_or("").trim()).filter(|l| !l.is_empty());
    let head = lines.next()?;
    let mut words = head.trim_end_matches(';').split_whitespace();
    if !matches!(words.next()?, "graph" | "flowchart") {
        return None;
    }
    let dir = match words.next().unwrap_or("TD").to_ascii_uppercase().as_str() {
        "BT" => Dir::BottomUp,
        "LR" => Dir::LeftRight,
        "RL" => Dir::RightLeft,
        _ => Dir::TopDown,
    };
    let mut d = Diagram { dir, nodes: Vec::new(), edges: Vec::new() };
    let mut index: HashMap<String, usize> = HashMap::new();
    for line in lines {
        for stmt in split_statements(line) {
            statement(stmt.trim(), &mut d, &mut index);
        }
    }
    // Very large graphs are left as code: the layout is meant for diagrams a person reads.
    (!d.nodes.is_empty() && d.nodes.len() <= 150 && d.edges.len() <= 400).then_some(d)
}

/// Statements are separated by `;` outside brackets and quotes.
fn split_statements(line: &str) -> Vec<&str> {
    let (mut out, mut depth, mut quote, mut start) = (Vec::new(), 0i32, false, 0);
    for (i, c) in line.char_indices() {
        match c {
            '"' => quote = !quote,
            '[' | '(' | '{' if !quote => depth += 1,
            ']' | ')' | '}' if !quote => depth -= 1,
            ';' if !quote && depth <= 0 => {
                out.push(&line[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&line[start..]);
    out
}

fn statement(s: &str, d: &mut Diagram, index: &mut HashMap<String, usize>) {
    let first = s.split_whitespace().next().unwrap_or("");
    // Styling, subgraph frames and the like do not change the graph itself.
    if matches!(first, "subgraph" | "end" | "classDef" | "class" | "style" | "linkStyle" | "click" | "direction" | "accTitle" | "accDescr")
        || s.is_empty()
    {
        return;
    }
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut group = node_group(&c, &mut i, d, index);
    // `A --> B --> C`: each edge joins the nodes before it to the nodes after it.
    while !group.is_empty() {
        let Some(((line, arrow, back_arrow), label)) = edge(&c, &mut i) else { return };
        let next = node_group(&c, &mut i, d, index);
        for &from in &group {
            for &to in &next {
                d.edges.push(Edge { from, to, label: label.clone(), line, arrow, back_arrow });
            }
        }
        group = next;
    }
}

/// `A & B`: one or more nodes joined by `&`.
fn node_group(c: &[char], i: &mut usize, d: &mut Diagram, index: &mut HashMap<String, usize>) -> Vec<usize> {
    let mut out = Vec::new();
    loop {
        skip_ws(c, i);
        let Some(n) = node(c, i, d, index) else { break };
        out.push(n);
        skip_ws(c, i);
        if c.get(*i) == Some(&'&') {
            *i += 1;
        } else {
            break;
        }
    }
    out
}

fn skip_ws(c: &[char], i: &mut usize) {
    while c.get(*i).is_some_and(|x| x.is_whitespace()) {
        *i += 1;
    }
}

/// `id`, optionally followed by a shape with its text; defines or updates the node.
fn node(c: &[char], i: &mut usize, d: &mut Diagram, index: &mut HashMap<String, usize>) -> Option<usize> {
    let start = *i;
    while c.get(*i).is_some_and(|x| x.is_alphanumeric() || *x == '_') {
        *i += 1;
    }
    if *i == start {
        return None;
    }
    let id: String = c[start..*i].iter().collect();
    let mut shaped: Option<(String, Shape)> = None;
    const OPENERS: &[(&str, &str, Shape)] = &[
        ("[[", "]]", Shape::Subroutine),
        ("[(", ")]", Shape::Round),
        ("([", "])", Shape::Stadium),
        ("((", "))", Shape::Circle),
        ("{{", "}}", Shape::Hexagon),
        ("[/", "/]", Shape::Rect),
        ("[\\", "\\]", Shape::Rect),
        ("[", "]", Shape::Rect),
        ("(", ")", Shape::Round),
        ("{", "}", Shape::Diamond),
        (">", "]", Shape::Rect),
    ];
    let rest: String = c[*i..].iter().take(2).collect();
    if let Some((open, close, shape)) = OPENERS.iter().find(|(o, _, _)| rest.starts_with(o)) {
        let body_start = *i + open.chars().count();
        let tail: String = c[body_start..].iter().collect();
        // Quoted text may hold the closing characters.
        let (text, used) = if let Some(q) = tail.strip_prefix('"') {
            match q.find('"') {
                Some(e) => {
                    let after = &q[e + 1..];
                    match after.trim_start().strip_prefix(close) {
                        Some(r) => (q[..e].to_string(), tail.len() - r.len()),
                        None => return Some(define(d, index, id, None)),
                    }
                }
                None => return Some(define(d, index, id, None)),
            }
        } else {
            match tail.find(close) {
                Some(e) => (tail[..e].to_string(), e + close.len()),
                None => return Some(define(d, index, id, None)),
            }
        };
        *i = body_start + tail[..used].chars().count();
        shaped = Some((clean_label(&text), *shape));
    }
    // `:::class` styling is ignored.
    if c[*i..].starts_with(&[':', ':', ':']) {
        *i += 3;
        while c.get(*i).is_some_and(|x| x.is_alphanumeric() || *x == '_') {
            *i += 1;
        }
    }
    Some(define(d, index, id, shaped))
}

fn clean_label(t: &str) -> String {
    let t = t.trim().trim_matches('"').replace("<br/>", "\n").replace("<br>", "\n").replace("<br />", "\n").replace("\\n", "\n");
    t.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"")
}

fn define(d: &mut Diagram, index: &mut HashMap<String, usize>, id: String, shaped: Option<(String, Shape)>) -> usize {
    match index.get(&id) {
        Some(&n) => {
            if let Some((label, shape)) = shaped {
                d.nodes[n].label = label;
                d.nodes[n].shape = shape;
            }
            n
        }
        None => {
            let (label, shape) = shaped.unwrap_or_else(|| (id.clone(), Shape::Rect));
            d.nodes.push(Node { id: id.clone(), label, shape });
            index.insert(id, d.nodes.len() - 1);
            d.nodes.len() - 1
        }
    }
}

/// An arrow between nodes: `-->`, `---`, `-.->`, `==>`, `--x`, `<-->`, with text as `-->|text|` or `-- text -->`.
fn edge(c: &[char], i: &mut usize) -> Option<((Line, bool, bool), String)> {
    skip_ws(c, i);
    let start = *i;
    let back = c.get(*i) == Some(&'<');
    if back {
        *i += 1;
    }
    let is_run = |x: char| matches!(x, '-' | '=' | '.');
    let run_start = *i;
    while c.get(*i).is_some_and(|x| is_run(*x)) {
        *i += 1;
    }
    if *i - run_start < 2 && !(*i > run_start && c.get(*i) == Some(&'>')) {
        *i = start;
        return None;
    }
    let mut run: String = c[run_start..*i].iter().collect();
    let mut label = String::new();
    let head = |c: &[char], i: &mut usize| -> bool {
        match c.get(*i) {
            Some('>') => {
                *i += 1;
                true
            }
            Some('x' | 'o') if c.get(*i + 1).is_none_or(|n| n.is_whitespace()) => {
                *i += 1;
                true
            }
            _ => false,
        }
    };
    let mut arrow = head(c, i);
    // `-- text -->`: an open run (two characters, no head) is followed by text and the closing run.
    let open_text = !arrow && run.chars().count() == 2 && c.get(*i).is_some_and(|x| x.is_whitespace());
    if open_text {
        let rest: String = c[*i..].iter().collect();
        let closers = ["-->", "---", "==>", "===", ".->", "-.-", "--x", "--o"];
        let found = closers.iter().filter_map(|cl| rest.find(cl).map(|p| (p, *cl))).min();
        let (p, cl) = found?;
        label = rest[..p].trim().to_string();
        *i += rest[..p].chars().count() + cl.chars().count();
        run.push_str(cl);
        arrow = cl.ends_with('>') || cl.ends_with('x') || cl.ends_with('o');
    }
    // `-->|text|`
    skip_inline_ws(c, i);
    if c.get(*i) == Some(&'|') {
        let rest: String = c[*i + 1..].iter().collect();
        if let Some(e) = rest.find('|') {
            label = rest[..e].to_string();
            *i += 1 + rest[..e].chars().count() + 1;
        }
    }
    let line = if run.contains('=') {
        Line::Thick
    } else if run.contains('.') {
        Line::Dotted
    } else {
        Line::Solid
    };
    Some(((line, arrow, back), clean_label(&label)))
}

fn skip_inline_ws(c: &[char], i: &mut usize) {
    while c.get(*i).is_some_and(|x| *x == ' ' || *x == '\t') {
        *i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flowchart_of_the_example() {
        let d = parse(
            "graph TD\n    A[Início] --> B{Processo?}\n    B -->|Sim| C[Executar]\n    B -->|Não| D[Parar]\n    C --> E[Fim]\n    D --> E\n",
        )
        .expect("a flowchart");
        assert_eq!(d.dir, Dir::TopDown);
        let names: Vec<_> = d.nodes.iter().map(|n| (n.id.as_str(), n.label.as_str(), n.shape)).collect();
        assert_eq!(
            names,
            [
                ("A", "Início", Shape::Rect),
                ("B", "Processo?", Shape::Diamond),
                ("C", "Executar", Shape::Rect),
                ("D", "Parar", Shape::Rect),
                ("E", "Fim", Shape::Rect)
            ]
        );
        let edges: Vec<_> = d.edges.iter().map(|e| (e.from, e.to, e.label.as_str())).collect();
        assert_eq!(edges, [(0, 1, ""), (1, 2, "Sim"), (1, 3, "Não"), (2, 4, ""), (3, 4, "")]);
        assert!(d.edges.iter().all(|e| e.arrow && e.line == Line::Solid));
    }

    #[test]
    fn chains_groups_labels_and_line_styles() {
        let d = parse(
            "flowchart LR\nA --> B --> C\nA & B --> D\nC -- yes --> E\nE -.-> F\nF ==> G\nG --- H\nI((round)) <--> J([stadium]); K{{hex}}",
        )
        .unwrap();
        assert_eq!(d.dir, Dir::LeftRight);
        let find = |a: &str, b: &str| {
            let id = |x: &str| d.nodes.iter().position(|n| n.id == x).unwrap();
            d.edges.iter().find(|e| e.from == id(a) && e.to == id(b)).cloned()
        };
        assert!(find("A", "B").is_some() && find("B", "C").is_some() && find("A", "D").is_some() && find("B", "D").is_some());
        assert_eq!(find("C", "E").unwrap().label, "yes");
        assert_eq!(find("E", "F").unwrap().line, Line::Dotted);
        assert_eq!(find("F", "G").unwrap().line, Line::Thick);
        assert!(!find("G", "H").unwrap().arrow);
        let both = find("I", "J").unwrap();
        assert!(both.arrow && both.back_arrow);
        let shape = |id: &str| d.nodes.iter().find(|n| n.id == id).unwrap().shape;
        assert_eq!((shape("I"), shape("J"), shape("K")), (Shape::Circle, Shape::Stadium, Shape::Hexagon));
    }

    #[test]
    fn other_diagrams_and_junk_are_not_flowcharts() {
        assert!(parse("sequenceDiagram\nA->>B: hi").is_none());
        assert!(parse("pie\n\"a\": 1").is_none());
        assert!(parse("graph TD\n").is_none());
        assert!(parse("").is_none());
        // Bad lines are skipped, not fatal.
        let d = parse("graph TD\nA[open --> B\nC --> D").unwrap();
        assert!(d.nodes.iter().any(|n| n.id == "D"));
    }
}
