//! LaTeX maths (`$…$`, `$$…$$`) as styled text, without a typesetting engine: Greek letters and symbols
//! become Unicode, variables are italic, `^` and `_` become raised and lowered text, `\frac` is written
//! `a/b` (with parentheses when needed), `\sqrt` is `√`, and matrices and `cases` are lines of cells.
//! Nothing is stacked or drawn: what does not fit that model is left as plain text.

use super::{Inline, Style};

#[derive(Clone, Copy, PartialEq)]
enum Last {
    Start,
    Operand,
    Op,
}

#[derive(Clone, Copy, PartialEq)]
enum Font {
    Math,
    Roman,
    Bold,
    Double,
}

/// Thin space (before and after binary operators) and a little wider one (around relations).
const THIN: &str = "\u{2009}";
const MED: &str = "\u{2005}";
const NBSP: &str = "\u{a0}";

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Ord,
    Bin,
    Rel,
}

const SYMBOLS: &[(&str, &str, Kind)] = &[
    ("leq", "≤", Kind::Rel),
    ("le", "≤", Kind::Rel),
    ("geq", "≥", Kind::Rel),
    ("ge", "≥", Kind::Rel),
    ("neq", "≠", Kind::Rel),
    ("ne", "≠", Kind::Rel),
    ("approx", "≈", Kind::Rel),
    ("equiv", "≡", Kind::Rel),
    ("sim", "∼", Kind::Rel),
    ("simeq", "≃", Kind::Rel),
    ("cong", "≅", Kind::Rel),
    ("propto", "∝", Kind::Rel),
    ("ll", "≪", Kind::Rel),
    ("gg", "≫", Kind::Rel),
    ("in", "∈", Kind::Rel),
    ("notin", "∉", Kind::Rel),
    ("ni", "∋", Kind::Rel),
    ("subset", "⊂", Kind::Rel),
    ("supset", "⊃", Kind::Rel),
    ("subseteq", "⊆", Kind::Rel),
    ("supseteq", "⊇", Kind::Rel),
    ("to", "→", Kind::Rel),
    ("rightarrow", "→", Kind::Rel),
    ("leftarrow", "←", Kind::Rel),
    ("leftrightarrow", "↔", Kind::Rel),
    ("Rightarrow", "⇒", Kind::Rel),
    ("Leftarrow", "⇐", Kind::Rel),
    ("Leftrightarrow", "⇔", Kind::Rel),
    ("implies", "⟹", Kind::Rel),
    ("iff", "⟺", Kind::Rel),
    ("mapsto", "↦", Kind::Rel),
    ("perp", "⊥", Kind::Rel),
    ("parallel", "∥", Kind::Rel),
    ("mid", "∣", Kind::Rel),
    ("pm", "±", Kind::Bin),
    ("mp", "∓", Kind::Bin),
    ("times", "×", Kind::Bin),
    ("div", "÷", Kind::Bin),
    ("cdot", "⋅", Kind::Bin),
    ("ast", "∗", Kind::Bin),
    ("star", "⋆", Kind::Bin),
    ("circ", "∘", Kind::Bin),
    ("cup", "∪", Kind::Bin),
    ("cap", "∩", Kind::Bin),
    ("setminus", "∖", Kind::Bin),
    ("oplus", "⊕", Kind::Bin),
    ("otimes", "⊗", Kind::Bin),
    ("wedge", "∧", Kind::Bin),
    ("land", "∧", Kind::Bin),
    ("vee", "∨", Kind::Bin),
    ("lor", "∨", Kind::Bin),
    ("bullet", "•", Kind::Bin),
    ("infty", "∞", Kind::Ord),
    ("partial", "∂", Kind::Ord),
    ("nabla", "∇", Kind::Ord),
    ("forall", "∀", Kind::Ord),
    ("exists", "∃", Kind::Ord),
    ("nexists", "∄", Kind::Ord),
    ("emptyset", "∅", Kind::Ord),
    ("varnothing", "∅", Kind::Ord),
    ("neg", "¬", Kind::Ord),
    ("lnot", "¬", Kind::Ord),
    ("ldots", "…", Kind::Ord),
    ("dots", "…", Kind::Ord),
    ("cdots", "⋯", Kind::Ord),
    ("vdots", "⋮", Kind::Ord),
    ("ddots", "⋱", Kind::Ord),
    ("hbar", "ℏ", Kind::Ord),
    ("ell", "ℓ", Kind::Ord),
    ("Re", "ℜ", Kind::Ord),
    ("Im", "ℑ", Kind::Ord),
    ("aleph", "ℵ", Kind::Ord),
    ("prime", "′", Kind::Ord),
    ("angle", "∠", Kind::Ord),
    ("degree", "°", Kind::Ord),
    ("therefore", "∴", Kind::Ord),
    ("because", "∵", Kind::Ord),
    ("top", "⊤", Kind::Ord),
    ("bot", "⊥", Kind::Ord),
    ("sum", "∑", Kind::Ord),
    ("prod", "∏", Kind::Ord),
    ("coprod", "∐", Kind::Ord),
    ("int", "∫", Kind::Ord),
    ("iint", "∬", Kind::Ord),
    ("iiint", "∭", Kind::Ord),
    ("oint", "∮", Kind::Ord),
    ("bigcup", "⋃", Kind::Ord),
    ("bigcap", "⋂", Kind::Ord),
    ("langle", "⟨", Kind::Ord),
    ("rangle", "⟩", Kind::Ord),
    ("lbrace", "{", Kind::Ord),
    ("rbrace", "}", Kind::Ord),
    ("lceil", "⌈", Kind::Ord),
    ("rceil", "⌉", Kind::Ord),
    ("lfloor", "⌊", Kind::Ord),
    ("rfloor", "⌋", Kind::Ord),
    ("vert", "|", Kind::Ord),
    ("Vert", "‖", Kind::Ord),
    ("lVert", "‖", Kind::Ord),
    ("rVert", "‖", Kind::Ord),
    ("lvert", "|", Kind::Ord),
    ("rvert", "|", Kind::Ord),
    ("backslash", "\\", Kind::Ord),
    ("dagger", "†", Kind::Ord),
];

/// Greek letters: lowercase ones are italic, capitals upright.
const GREEK: &[(&str, &str)] = &[
    ("alpha", "α"),
    ("beta", "β"),
    ("gamma", "γ"),
    ("delta", "δ"),
    ("epsilon", "ε"),
    ("varepsilon", "ε"),
    ("zeta", "ζ"),
    ("eta", "η"),
    ("theta", "θ"),
    ("vartheta", "ϑ"),
    ("iota", "ι"),
    ("kappa", "κ"),
    ("lambda", "λ"),
    ("mu", "μ"),
    ("nu", "ν"),
    ("xi", "ξ"),
    ("pi", "π"),
    ("varpi", "ϖ"),
    ("rho", "ρ"),
    ("varrho", "ϱ"),
    ("sigma", "σ"),
    ("varsigma", "ς"),
    ("tau", "τ"),
    ("upsilon", "υ"),
    ("phi", "φ"),
    ("varphi", "ϕ"),
    ("chi", "χ"),
    ("psi", "ψ"),
    ("omega", "ω"),
    ("Gamma", "Γ"),
    ("Delta", "Δ"),
    ("Theta", "Θ"),
    ("Lambda", "Λ"),
    ("Xi", "Ξ"),
    ("Pi", "Π"),
    ("Sigma", "Σ"),
    ("Upsilon", "Υ"),
    ("Phi", "Φ"),
    ("Psi", "Ψ"),
    ("Omega", "Ω"),
];

/// Operators written upright, with a thin space after.
const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "sinh", "cosh", "tanh", "coth", "arcsin", "arccos", "arctan", "log", "ln", "lg", "exp",
    "lim", "limsup", "liminf", "max", "min", "sup", "inf", "det", "dim", "ker", "gcd", "lcm", "deg", "arg", "Pr", "mod", "hom",
];

fn double_struck(c: char) -> char {
    match c {
        'C' => 'ℂ',
        'H' => 'ℍ',
        'N' => 'ℕ',
        'P' => 'ℙ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'Z' => 'ℤ',
        c => c,
    }
}

struct P {
    c: Vec<char>,
    i: usize,
    out: Vec<Inline>,
    last: Last,
    font: Font,
}

/// The maths in `src` as inline runs, drawn in `base`.
pub fn render(src: &str, base: Style) -> Vec<Inline> {
    let c: Vec<char> = src.chars().collect();
    let mut p = P::new(c, Font::Math);
    p.seq(base, false);
    merge(p.out)
}

/// Neighbouring runs with the same style become one.
fn merge(v: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(v.len());
    for i in v {
        match (out.last_mut(), i) {
            (Some(Inline::Text(a, sa)), Inline::Text(b, sb)) if *sa == sb => a.push_str(&b),
            (_, i) => out.push(i),
        }
    }
    out
}

fn text_of(v: &[Inline]) -> String {
    v.iter()
        .filter_map(|i| match i {
            Inline::Text(t, _) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

impl P {
    fn new(c: Vec<char>, font: Font) -> P {
        P { c, i: 0, out: Vec::new(), last: Last::Start, font }
    }

    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.i += 1;
        }
    }

    fn put(&mut self, s: &str, st: Style, last: Last) {
        self.out.push(Inline::Text(s.to_string(), st));
        self.last = last;
    }

    /// Style of digits, operators and names: upright (bold in `\mathbf`).
    fn plain(&self, st: Style) -> Style {
        match self.font {
            Font::Bold => Style { bold: true, italic: false, ..st },
            _ => Style { italic: false, ..st },
        }
    }

    /// Style of variables.
    fn var(&self, st: Style) -> Style {
        match self.font {
            Font::Math => Style { italic: true, ..st },
            _ => self.plain(st),
        }
    }

    fn binary(&mut self, s: &str, st: Style) {
        let spaced = self.last == Last::Operand && st.valign == 0;
        let st = self.plain(st);
        if spaced {
            self.put(&format!("{THIN}{s}{THIN}"), st, Last::Op);
        } else {
            self.put(s, st, Last::Op);
        }
    }

    /// Scripts are small already: no space around their operators.
    fn relation(&mut self, s: &str, st: Style) {
        let spaced = st.valign == 0;
        let st = self.plain(st);
        if spaced {
            self.put(&format!("{MED}{s}{MED}"), st, Last::Op);
        } else {
            self.put(s, st, Last::Op);
        }
    }

    /// Runs `f` and returns what it wrote, leaving the output as it was.
    fn capture(&mut self, st: Style, f: impl FnOnce(&mut P, Style)) -> Vec<Inline> {
        let saved = std::mem::take(&mut self.out);
        let last = std::mem::replace(&mut self.last, Last::Start);
        f(self, st);
        self.last = last;
        std::mem::replace(&mut self.out, saved)
    }

    /// Everything up to the end of the source, or up to the `}` that closes the group being read.
    fn seq(&mut self, st: Style, in_group: bool) {
        while let Some(ch) = self.peek() {
            match ch {
                '}' if in_group => break,
                '}' => self.i += 1,
                '{' => {
                    self.i += 1;
                    self.seq(st, true);
                    if self.peek() == Some('}') {
                        self.i += 1;
                    }
                    self.last = Last::Operand;
                }
                '\\' => self.command(st),
                '^' | '_' => {
                    self.i += 1;
                    self.script(ch == '^', st);
                }
                '&' => {
                    self.i += 1;
                    self.put(&NBSP.repeat(2), st, Last::Start);
                }
                c if c.is_whitespace() => self.i += 1,
                c => {
                    self.i += 1;
                    self.symbol(c, st);
                }
            }
        }
    }

    /// One argument: a `{group}`, a command or a single character.
    fn atom(&mut self, st: Style) {
        self.skip_ws();
        match self.peek() {
            Some('{') => {
                self.i += 1;
                self.seq(st, true);
                if self.peek() == Some('}') {
                    self.i += 1;
                }
            }
            Some('\\') => self.command(st),
            Some(c) => {
                self.i += 1;
                self.symbol(c, st);
            }
            None => {}
        }
        self.last = Last::Operand;
    }

    fn script(&mut self, up: bool, st: Style) {
        let (valign, size) = if st.valign != 0 { (st.valign, st.size) } else { (if up { 1 } else { -1 }, st.size * 0.75) };
        let s = Style { valign, size, ..st };
        let got = self.capture(s, |p, s| p.atom(s));
        self.out.extend(got);
        self.last = Last::Operand;
    }

    /// The text of a `{group}` as written (`\text{…}`), spaces included.
    fn raw_group(&mut self) -> String {
        self.skip_ws();
        let mut s = String::new();
        if self.peek() != Some('{') {
            if let Some(c) = self.peek() {
                self.i += 1;
                s.push(c);
            }
            return s;
        }
        self.i += 1;
        let mut depth = 1;
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            s.push(c);
        }
        s
    }

    /// A value in a fraction or root: bare when it is one word or number, else in parentheses.
    fn wrap(&mut self, v: Vec<Inline>, st: Style) {
        let plain = text_of(&v);
        if plain.chars().count() <= 1 || plain.chars().all(char::is_alphanumeric) {
            self.out.extend(v);
        } else if !plain.is_empty() {
            let st = self.plain(st);
            self.put("(", st, Last::Start);
            self.out.extend(v);
            self.put(")", st, Last::Operand);
        }
        self.last = Last::Operand;
    }

    fn symbol(&mut self, c: char, st: Style) {
        match c {
            c if c.is_alphabetic() => {
                let ch = if self.font == Font::Double { double_struck(c) } else { c };
                let s = self.var(st);
                self.put(&ch.to_string(), s, Last::Operand);
            }
            '-' => self.binary("−", st),
            '+' => self.binary("+", st),
            '*' => self.binary("∗", st),
            '=' | '<' | '>' => self.relation(&c.to_string(), st),
            '\'' => {
                let s = self.plain(st);
                self.put("′", s, Last::Operand);
            }
            '~' => {
                let s = self.plain(st);
                self.put(NBSP, s, Last::Start);
            }
            '(' | '[' => {
                let s = self.plain(st);
                self.put(&c.to_string(), s, Last::Start);
            }
            c => {
                let s = self.plain(st);
                self.put(&c.to_string(), s, Last::Operand);
            }
        }
    }

    fn command(&mut self, st: Style) {
        self.i += 1;
        let Some(c) = self.peek() else { return };
        let plain = self.plain(st);
        if !c.is_ascii_alphabetic() {
            self.i += 1;
            let last = self.last;
            match c {
                ',' | ';' | ':' | '>' => self.put(THIN, plain, last),
                '!' => {}
                ' ' => self.put(NBSP, plain, last),
                '\\' => {
                    self.out.push(Inline::Break);
                    self.last = Last::Start;
                }
                '|' => self.put("‖", plain, Last::Operand),
                c => self.put(&c.to_string(), plain, Last::Operand),
            }
            return;
        }
        let start = self.i;
        while matches!(self.peek(), Some(c) if c.is_ascii_alphabetic()) {
            self.i += 1;
        }
        let name: String = self.c[start..self.i].iter().collect();
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let a = self.capture(st, |p, s| p.atom(s));
                let b = self.capture(st, |p, s| p.atom(s));
                self.wrap(a, st);
                self.put("/", plain, Last::Op);
                self.wrap(b, st);
            }
            "binom" => {
                let a = self.capture(st, |p, s| p.atom(s));
                let b = self.capture(st, |p, s| p.atom(s));
                self.put("C(", plain, Last::Start);
                self.out.extend(a);
                self.put(", ", plain, Last::Start);
                self.out.extend(b);
                self.put(")", plain, Last::Operand);
            }
            "sqrt" => {
                self.skip_ws();
                let mut index = String::new();
                if self.peek() == Some('[') {
                    self.i += 1;
                    while let Some(c) = self.peek() {
                        self.i += 1;
                        if c == ']' {
                            break;
                        }
                        index.push(c);
                    }
                }
                let body = self.capture(st, |p, s| p.atom(s));
                if !index.is_empty() {
                    self.put(&index, Style { valign: 1, size: st.size * 0.6, ..plain }, Last::Operand);
                }
                self.put("√", plain, Last::Op);
                self.wrap(body, st);
            }
            "text" | "textrm" | "textnormal" | "mbox" | "mathrm" | "operatorname" | "textit" | "textbf" => {
                let t = self.raw_group();
                let s = match name.as_str() {
                    "textbf" => Style { bold: true, italic: false, ..st },
                    "textit" => Style { italic: true, ..st },
                    _ => Style { italic: false, ..st },
                };
                self.put(&t, s, Last::Operand);
                if name == "operatorname" {
                    self.put(THIN, plain, Last::Operand);
                }
            }
            "mathbf" | "boldsymbol" | "bm" | "mathbb" | "mathit" | "mathcal" | "mathscr" | "mathfrak" | "mathsf" | "mathtt"
            | "mathnormal" => {
                let font = match name.as_str() {
                    "mathbf" | "boldsymbol" | "bm" => Font::Bold,
                    "mathbb" => Font::Double,
                    "mathsf" | "mathtt" => Font::Roman,
                    _ => Font::Math,
                };
                let saved = std::mem::replace(&mut self.font, font);
                self.atom(st);
                self.font = saved;
            }
            "left" | "right" | "bigl" | "bigr" | "big" | "Big" | "Bigl" | "Bigr" | "bigg" | "Bigg" | "biggl" | "biggr" => {
                if self.peek() == Some('.') {
                    self.i += 1;
                }
            }
            "limits" | "nolimits" | "displaystyle" | "textstyle" | "scriptstyle" | "mathop" | "mathbin" | "mathrel" => {}
            "quad" => self.put("\u{2003}", plain, Last::Start),
            "qquad" => self.put("\u{2003}\u{2003}", plain, Last::Start),
            "begin" => self.environment(st),
            "end" => {
                self.raw_group();
            }
            "hat" | "widehat" | "check" | "tilde" | "widetilde" | "vec" | "dot" | "ddot" | "bar" | "overline" | "not" => {
                let mark = match name.as_str() {
                    "hat" | "widehat" => '\u{302}',
                    "check" => '\u{30c}',
                    "tilde" | "widetilde" => '\u{303}',
                    "vec" => '\u{20d7}',
                    "dot" => '\u{307}',
                    "ddot" => '\u{308}',
                    "not" => '\u{338}',
                    _ => '\u{305}',
                };
                let each = matches!(name.as_str(), "bar" | "overline");
                let got = self.capture(st, |p, s| p.atom(s));
                for i in got {
                    match i {
                        Inline::Text(t, s) if each => {
                            let t: String = t.chars().flat_map(|c| [c, mark]).collect();
                            self.out.push(Inline::Text(t, s));
                        }
                        Inline::Text(mut t, s) => {
                            t.push(mark);
                            self.out.push(Inline::Text(t, s));
                        }
                        i => self.out.push(i),
                    }
                }
                self.last = Last::Operand;
            }
            "underline" | "underbrace" | "overbrace" | "boxed" | "cancel" | "phantom" | "hphantom" | "vphantom" => self.atom(st),
            n => {
                if let Some((_, s)) = GREEK.iter().find(|(g, _)| *g == n) {
                    let style = if s.chars().next().is_some_and(char::is_lowercase) { self.var(st) } else { plain };
                    self.put(s, style, Last::Operand);
                } else if let Some((_, s, kind)) = SYMBOLS.iter().find(|(g, _, _)| *g == n) {
                    match kind {
                        Kind::Rel => self.relation(s, st),
                        Kind::Bin => self.binary(s, st),
                        Kind::Ord => self.put(s, plain, Last::Operand),
                    }
                } else if FUNCTIONS.contains(&n) {
                    self.put(n, Style { italic: false, ..plain }, Last::Operand);
                    self.put(THIN, plain, Last::Start);
                } else {
                    self.put(n, plain, Last::Operand);
                }
            }
        }
    }

    /// `\begin{pmatrix} a & b \\ c & d \end{pmatrix}`: rows on lines, cells apart, the brackets around.
    fn environment(&mut self, st: Style) {
        let name = self.raw_group();
        let rest: String = self.c[self.i..].iter().collect();
        let end = format!("\\end{{{name}}}");
        let (body, used) = match rest.find(&end) {
            Some(p) => (&rest[..p], rest[..p].chars().count() + end.chars().count()),
            None => (rest.as_str(), rest.chars().count()),
        };
        let body: Vec<char> = body.chars().collect();
        self.i += used;
        let (open, close) = match name.trim_end_matches('*') {
            "pmatrix" => ("(", ")"),
            "bmatrix" => ("[", "]"),
            "Bmatrix" => ("{", "}"),
            "vmatrix" => ("|", "|"),
            "Vmatrix" => ("‖", "‖"),
            "cases" => ("{", ""),
            _ => ("", ""),
        };
        let aligned = matches!(name.trim_end_matches('*'), "aligned" | "align" | "split" | "gathered" | "gather" | "equation");
        let plain = self.plain(st);
        // Matrices: each row in its own brackets (one bracket cannot span lines); `cases`: one brace in front.
        let per_row = !close.is_empty();
        if !open.is_empty() && !per_row {
            self.put(open, plain, Last::Start);
        }
        let rows = split_top(&body, true);
        for (r, row) in rows.iter().enumerate() {
            if r > 0 {
                self.out.push(Inline::Break);
            }
            if per_row {
                self.put(open, plain, Last::Start);
            }
            for (k, cell) in split_top(row, false).into_iter().enumerate() {
                if k > 0 && !aligned {
                    self.put(&NBSP.repeat(3), plain, Last::Start);
                }
                let mut q = P::new(cell, self.font);
                q.seq(st, false);
                self.out.extend(q.out);
            }
            if per_row {
                self.put(close, plain, Last::Operand);
            }
        }
        self.last = Last::Operand;
    }
}

/// Splits at `\\` (rows) or `&` (cells) outside braces and nested environments; empty rows are dropped.
fn split_top(s: &[char], rows: bool) -> Vec<Vec<char>> {
    let mut out = vec![Vec::new()];
    let mut depth = 0i32;
    let mut i = 0;
    let starts = |i: usize, w: &str| s[i..].iter().take(w.chars().count()).copied().eq(w.chars());
    while i < s.len() {
        let c = s[i];
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
        } else if starts(i, "\\begin") {
            depth += 1;
        } else if starts(i, "\\end") {
            depth -= 1;
        }
        if depth == 0 && rows && starts(i, "\\\\") {
            out.push(Vec::new());
            i += 2;
            continue;
        }
        if depth == 0 && !rows && c == '&' {
            out.push(Vec::new());
            i += 1;
            continue;
        }
        out.last_mut().expect("one row").push(c);
        i += 1;
    }
    if rows {
        out.retain(|r| r.iter().any(|c| !c.is_whitespace()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(src: &str) -> String {
        text_of(&render(src, Style::default()))
    }

    #[test]
    fn symbols_and_greek() {
        assert_eq!(text(r"\alpha + \beta"), format!("α{THIN}+{THIN}β"));
        assert_eq!(text(r"\infty"), "∞");
        assert_eq!(text(r"x \leq y"), format!("x{MED}≤{MED}y"));
        assert_eq!(text(r"\Gamma"), "Γ");
    }

    #[test]
    fn variables_are_italic_and_numbers_upright() {
        let v = render("x2", Style::default());
        assert_eq!(v.len(), 2);
        assert!(matches!(&v[0], Inline::Text(t, s) if t == "x" && s.italic));
        assert!(matches!(&v[1], Inline::Text(t, s) if t == "2" && !s.italic));
    }

    #[test]
    fn scripts_raise_and_lower() {
        let v = render("x^2_i", Style::default());
        assert!(matches!(&v[1], Inline::Text(t, s) if t == "2" && s.valign == 1 && s.size < 1.0));
        assert!(matches!(&v[2], Inline::Text(t, s) if t == "i" && s.valign == -1));
        let v = render("e^{-x^2}", Style::default());
        // Nested: all raised, none lowered.
        assert!(v.iter().skip(1).all(|i| matches!(i, Inline::Text(_, s) if s.valign == 1)));
    }

    #[test]
    fn fractions_and_roots() {
        assert_eq!(text(r"\frac{a}{b}"), "a/b");
        assert_eq!(text(r"\frac{a+b}{2}"), format!("(a{THIN}+{THIN}b)/2"));
        assert_eq!(text(r"\sqrt{x}"), "√x");
        assert_eq!(text(r"\sqrt{x^2+1}"), format!("√(x2{THIN}+{THIN}1)"));
    }

    #[test]
    fn text_fonts_and_functions() {
        assert_eq!(text(r"\text{if } x"), "if x");
        assert_eq!(text(r"\mathbb{R}"), "ℝ");
        assert_eq!(text(r"\sin x"), format!("sin{THIN}x"));
    }

    #[test]
    fn matrices_become_lines() {
        let v = render(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}", Style::default());
        assert_eq!(v.iter().filter(|i| matches!(i, Inline::Break)).count(), 1);
        let t = text_of(&v);
        assert!(t.starts_with('(') && t.ends_with(')') && t.contains('a') && t.contains('d'));
    }

    #[test]
    fn nothing_hangs_on_bad_input() {
        for s in [r"\frac{a", r"{{{", "}}}", r"\begin{pmatrix} a", r"x^", r"\", r"\sqrt[3", r"\left", r"\end{x}"] {
            let _ = render(s, Style::default());
        }
    }
}
