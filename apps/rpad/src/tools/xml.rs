//! XML well-formedness checking and safe pretty-printing.
//!
//! The checker reports the first error with line and column. The formatter
//! only re-indents whitespace between elements of element-only content;
//! elements with text, CDATA or `xml:space="preserve"` are copied verbatim,
//! so the document's text content never changes.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Line {}, column {}: {}", self.line, self.col, self.message)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    /// Prolog/misc markup: declaration, PI, comment, doctype (raw text).
    Misc(String),
    Element(Element),
    Text(String),
    CData(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Element {
    /// Raw start tag (`<a x="1">` or `<a/>`).
    start_tag: String,
    end_tag: Option<String>,
    children: Vec<Node>,
    /// Byte span of the whole element in the source.
    span: (usize, usize),
    preserve: bool,
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
    has_doctype: bool,
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == ':' || (c as u32) > 0x7F
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-' || c == '.' || c == '\u{B7}'
}

impl<'a> Parser<'a> {
    fn err(&self, at: usize, msg: impl Into<String>) -> XmlError {
        let before = &self.s[..at.min(self.s.len())];
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
        XmlError { line, col, message: msg.into() }
    }

    fn rest(&self) -> &'a str {
        &self.s[self.i..]
    }

    fn name(&mut self) -> Result<&'a str, XmlError> {
        let start = self.i;
        let mut chars = self.rest().char_indices();
        match chars.next() {
            Some((_, c)) if is_name_start(c) => {}
            _ => return Err(self.err(start, "Expected a name")),
        }
        let end = self.rest().char_indices().find(|(_, c)| !is_name_char(*c)).map_or(self.rest().len(), |(p, _)| p);
        self.i += end;
        Ok(&self.s[start..self.i])
    }

    fn skip_ws(&mut self) {
        let n = self.rest().len() - self.rest().trim_start_matches([' ', '\t', '\r', '\n']).len();
        self.i += n;
    }

    /// Validates `&...;` references inside text or attribute values.
    fn check_refs(&self, text: &str, at: usize) -> Result<(), XmlError> {
        let mut rest = text;
        let mut off = 0;
        while let Some(p) = rest.find('&') {
            let after = &rest[p + 1..];
            let Some(semi) = after.find(';') else {
                return Err(self.err(at + off + p, "'&' must start an entity reference such as &amp;"));
            };
            let name = &after[..semi];
            let ok = match name {
                "amp" | "lt" | "gt" | "quot" | "apos" => true,
                n if n.starts_with("#x") => n.len() > 2 && u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32).is_some(),
                n if n.starts_with('#') => n.len() > 1 && n[1..].parse::<u32>().ok().and_then(char::from_u32).is_some(),
                n => self.has_doctype && !n.is_empty() && n.chars().next().is_some_and(is_name_start) && n.chars().all(is_name_char),
            };
            if !ok {
                return Err(self.err(at + off + p, format!("Unknown or invalid entity reference '&{name};'")));
            }
            off += p + 1 + semi + 1;
            rest = &after[semi + 1..];
        }
        Ok(())
    }

    fn misc(&mut self, at_start: bool) -> Result<Option<Node>, XmlError> {
        let start = self.i;
        let r = self.rest();
        if let Some(after) = r.strip_prefix("<!--") {
            let end = after.find("-->").ok_or_else(|| self.err(start, "Comment is not closed (missing -->)"))?;
            let body = &r[4..4 + end];
            if let Some(p) = body.find("--") {
                return Err(self.err(start + 4 + p, "'--' is not allowed inside a comment"));
            }
            self.i += 4 + end + 3;
            return Ok(Some(Node::Misc(self.s[start..self.i].to_string())));
        }
        if let Some(after) = r.strip_prefix("<?") {
            let end = r.find("?>").ok_or_else(|| self.err(start, "Processing instruction is not closed (missing ?>)"))?;
            let target: String = after.chars().take_while(|c| is_name_char(*c)).collect();
            if target.eq_ignore_ascii_case("xml") && !at_start {
                return Err(self.err(start, "The XML declaration is only allowed at the very beginning of the document"));
            }
            self.i += end + 2;
            return Ok(Some(Node::Misc(self.s[start..self.i].to_string())));
        }
        if r.starts_with("<!DOCTYPE") {
            let mut depth = 0i32;
            let mut quote: Option<char> = None;
            for (p, c) in r.char_indices() {
                match (quote, c) {
                    (Some(q), c) if c == q => quote = None,
                    (Some(_), _) => {}
                    (None, '"' | '\'') => quote = Some(c),
                    (None, '[') => depth += 1,
                    (None, ']') => depth -= 1,
                    (None, '>') if depth == 0 => {
                        self.i += p + 1;
                        self.has_doctype = true;
                        return Ok(Some(Node::Misc(self.s[start..self.i].to_string())));
                    }
                    _ => {}
                }
            }
            return Err(self.err(start, "DOCTYPE is not closed"));
        }
        Ok(None)
    }

    fn element(&mut self) -> Result<Element, XmlError> {
        let start = self.i;
        self.i += 1; // '<'
        let name = self.name()?.to_string();
        let mut attrs: Vec<String> = Vec::new();
        let mut preserve = false;
        loop {
            let before_ws = self.i;
            self.skip_ws();
            let r = self.rest();
            if r.starts_with("/>") {
                self.i += 2;
                return Ok(Element { start_tag: self.s[start..self.i].to_string(), end_tag: None, children: Vec::new(), span: (start, self.i), preserve });
            }
            if r.starts_with('>') {
                self.i += 1;
                break;
            }
            if r.is_empty() {
                return Err(self.err(start, format!("Start tag <{name}> is not closed")));
            }
            if self.i == before_ws {
                return Err(self.err(self.i, "Expected whitespace, '>' or '/>'"));
            }
            let astart = self.i;
            let aname = self.name()?.to_string();
            if attrs.contains(&aname) {
                return Err(self.err(astart, format!("Duplicate attribute '{aname}'")));
            }
            self.skip_ws();
            if !self.rest().starts_with('=') {
                return Err(self.err(self.i, format!("Attribute '{aname}' needs a value (=\"...\")")));
            }
            self.i += 1;
            self.skip_ws();
            let q = self.rest().chars().next();
            let Some(q @ ('"' | '\'')) = q else {
                return Err(self.err(self.i, "Attribute values must be quoted"));
            };
            let vstart = self.i + 1;
            let end = self.s[vstart..].find(q).ok_or_else(|| self.err(self.i, "Attribute value is not closed"))?;
            let value = &self.s[vstart..vstart + end];
            if let Some(p) = value.find('<') {
                return Err(self.err(vstart + p, "'<' is not allowed in attribute values"));
            }
            self.check_refs(value, vstart)?;
            if aname == "xml:space" && value == "preserve" {
                preserve = true;
            }
            attrs.push(aname);
            self.i = vstart + end + 1;
        }
        let start_tag = self.s[start..self.i].to_string();
        let mut children = Vec::new();
        loop {
            let r = self.rest();
            if r.is_empty() {
                return Err(self.err(start, format!("Element <{name}> is not closed")));
            }
            if r.starts_with("</") {
                let estart = self.i;
                self.i += 2;
                let ename = self.name()?;
                if ename != name {
                    return Err(self.err(estart, format!("Expected </{name}> but found </{ename}>")));
                }
                self.skip_ws();
                if !self.rest().starts_with('>') {
                    return Err(self.err(self.i, "Expected '>'"));
                }
                self.i += 1;
                let end_tag = self.s[estart..self.i].to_string();
                return Ok(Element { start_tag, end_tag: Some(end_tag), children, span: (start, self.i), preserve });
            }
            if r.starts_with("<![CDATA[") {
                let end = r.find("]]>").ok_or_else(|| self.err(self.i, "CDATA section is not closed"))?;
                children.push(Node::CData(r[..end + 3].to_string()));
                self.i += end + 3;
                continue;
            }
            if r.starts_with("<!DOCTYPE") {
                return Err(self.err(self.i, "DOCTYPE is not allowed inside an element"));
            }
            if let Some(n) = self.misc(false)? {
                children.push(n);
                continue;
            }
            if r.starts_with('<') {
                children.push(Node::Element(self.element()?));
                continue;
            }
            let tstart = self.i;
            let end = r.find('<').unwrap_or(r.len());
            let text = &r[..end];
            if let Some(p) = text.find("]]>") {
                return Err(self.err(tstart + p, "']]>' is not allowed in text"));
            }
            self.check_refs(text, tstart)?;
            children.push(Node::Text(text.to_string()));
            self.i += end;
        }
    }

    fn document(&mut self) -> Result<Vec<Node>, XmlError> {
        let mut out = Vec::new();
        if self.s.starts_with('\u{feff}') {
            self.i = 3;
        }
        let mut root_seen = false;
        let mut first = true;
        loop {
            let ws_start = self.i;
            self.skip_ws();
            if self.i > ws_start {
                first = false;
            }
            if self.i >= self.s.len() {
                break;
            }
            if let Some(n) = self.misc(first)? {
                if matches!(&n, Node::Misc(m) if m.starts_with("<!DOCTYPE")) && root_seen {
                    return Err(self.err(self.i, "DOCTYPE must come before the root element"));
                }
                out.push(n);
                first = false;
                continue;
            }
            first = false;
            if self.rest().starts_with("</") {
                return Err(self.err(self.i, "Closing tag without a matching start tag"));
            }
            if self.rest().starts_with('<') {
                if root_seen {
                    return Err(self.err(self.i, "Only one root element is allowed"));
                }
                out.push(Node::Element(self.element()?));
                root_seen = true;
                continue;
            }
            return Err(self.err(self.i, "Text is not allowed outside the root element"));
        }
        if !root_seen {
            return Err(self.err(self.s.len(), "The document has no root element"));
        }
        Ok(out)
    }
}

fn parse(s: &str) -> Result<Vec<Node>, XmlError> {
    Parser { s, i: 0, has_doctype: false }.document()
}

/// Checks well-formedness. Returns the number of elements on success.
pub fn check(s: &str) -> Result<usize, XmlError> {
    fn count(nodes: &[Node]) -> usize {
        nodes.iter().map(|n| if let Node::Element(e) = n { 1 + count(&e.children) } else { 0 }).sum()
    }
    parse(s).map(|n| count(&n))
}

/// Pretty-prints a well-formed document with `indent` per level.
pub fn format(s: &str, indent: &str) -> Result<String, XmlError> {
    let nodes = parse(s)?;
    let mut out = String::with_capacity(s.len() + s.len() / 4);
    let src = s;
    for n in &nodes {
        write_node(n, 0, indent, src, &mut out);
    }
    if s.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    if !s.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    Ok(out)
}

fn write_node(n: &Node, depth: usize, indent: &str, src: &str, out: &mut String) {
    let pad = indent.repeat(depth);
    match n {
        Node::Misc(m) => {
            out.push_str(&pad);
            out.push_str(m);
            out.push('\n');
        }
        Node::Text(t) => {
            if !t.trim().is_empty() {
                out.push_str(&pad);
                out.push_str(t.trim());
                out.push('\n');
            }
        }
        Node::CData(c) => {
            out.push_str(&pad);
            out.push_str(c);
            out.push('\n');
        }
        Node::Element(e) => {
            let mixed = e.preserve
                || e.children.iter().any(|c| matches!(c, Node::Text(t) if !t.trim().is_empty()) || matches!(c, Node::CData(_)));
            out.push_str(&pad);
            if mixed || e.end_tag.is_none() || e.children.is_empty() {
                // Copy verbatim: whitespace here may be significant.
                out.push_str(&src[e.span.0..e.span.1]);
                out.push('\n');
                return;
            }
            out.push_str(&e.start_tag);
            out.push('\n');
            for c in &e.children {
                write_node(c, depth + 1, indent, src, out);
            }
            out.push_str(&pad);
            out.push_str(e.end_tag.as_deref().unwrap_or(""));
            out.push('\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(s: &str) -> (usize, usize) {
        let e = check(s).unwrap_err();
        (e.line, e.col)
    }

    #[test]
    fn well_formed_documents() {
        assert_eq!(check("<a/>"), Ok(1));
        assert_eq!(check("<?xml version=\"1.0\"?>\n<!-- c -->\n<a x='1' y=\"&amp;\"><b>t &#65; &#x42;</b><![CDATA[<raw>]]></a>\n"), Ok(2));
        assert_eq!(check("<!DOCTYPE r [<!ENTITY e \"x\">]><r>&e;</r>"), Ok(1));
        assert!(check("\u{feff}<a/>").is_ok());
    }

    #[test]
    fn errors_with_locations() {
        assert_eq!(err("<a><b></a>"), (1, 7));
        assert_eq!(err("<a>\n  <b x=1/></a>"), (2, 8));
        assert_eq!(err("<a x=\"1\" x=\"2\"/>"), (1, 10));
        assert_eq!(err("<a>&nbsp;</a>"), (1, 4));
        assert_eq!(err("<a/><b/>"), (1, 5));
        assert_eq!(err("<a>"), (1, 1));
        assert_eq!(err(""), (1, 1));
        assert_eq!(err("text"), (1, 1));
        assert_eq!(err("<a>x & y</a>"), (1, 6));
        assert_eq!(err(" <?xml version=\"1.0\"?><a/>"), (1, 2));
        assert_eq!(err("<a><!-- a -- b --></a>"), (1, 11));
    }

    #[test]
    fn formatting_reindents_element_only_content() {
        let src = "<?xml version=\"1.0\"?>\n<root><a  x=\"1\"><b/></a><c>text <i>keep</i>  as is</c><d xml:space=\"preserve\">\n  x\n</d><e></e></root>\n";
        let out = format(src, "  ").unwrap();
        assert_eq!(
            out,
            "<?xml version=\"1.0\"?>\n<root>\n  <a  x=\"1\">\n    <b/>\n  </a>\n  <c>text <i>keep</i>  as is</c>\n  <d xml:space=\"preserve\">\n  x\n</d>\n  <e></e>\n</root>\n"
        );
        // Formatting is idempotent and the result stays well-formed.
        assert_eq!(format(&out, "  ").unwrap(), out);
        assert!(check(&out).is_ok());
        assert!(format("<a><b></a>", "  ").is_err());
    }
}
