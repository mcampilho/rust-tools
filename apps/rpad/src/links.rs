//! Web and mail links in the text: found per line, underlined by the editor and opened with a double click.

/// Longest line searched (a minified file would make every frame slow).
const MAX_CHARS: usize = 20_000;

/// Char ranges `(start, end)` of the links in `chars`: `http://`, `https://`, `mailto:` and `www.`.
pub fn find(chars: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if chars.len() > MAX_CHARS {
        return out;
    }
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let starts_word = i == 0 || !chars[i - 1].is_alphanumeric();
        let prefix = if !starts_word || !(c == 'h' || c == 'H' || c == 'm' || c == 'M' || c == 'w' || c == 'W') {
            0
        } else {
            prefix_len(&chars[i..])
        };
        if prefix == 0 {
            i += 1;
            continue;
        }
        let mut end = i + prefix;
        while end < chars.len() && is_link_char(chars[end]) {
            end += 1;
        }
        end = trim_end(chars, i, end);
        if end > i + prefix {
            out.push((i, end));
            i = end;
        } else {
            i += prefix;
        }
    }
    out
}

/// Length of the link prefix at the start of `s` (0: none).
fn prefix_len(s: &[char]) -> usize {
    ["https://", "http://", "mailto:", "www."]
        .iter()
        .find(|p| s.len() >= p.len() && s.iter().zip(p.chars()).all(|(a, b)| a.to_ascii_lowercase() == b))
        .map_or(0, |p| p.len())
}

fn is_link_char(c: char) -> bool {
    !c.is_whitespace() && !c.is_control() && !matches!(c, '<' | '>' | '"' | '`' | '|' | '\\' | '^' | '{' | '}')
}

/// Drops the punctuation that ends a sentence, and a closing bracket without its opening one.
fn trim_end(chars: &[char], start: usize, mut end: usize) -> usize {
    while end > start {
        let c = chars[end - 1];
        let drop = match c {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*' | '_' | '~' => true,
            ')' | ']' => {
                let open = if c == ')' { '(' } else { '[' };
                let slice = &chars[start..end];
                slice.iter().filter(|&&x| x == c).count() > slice.iter().filter(|&&x| x == open).count()
            }
            _ => false,
        };
        if !drop {
            break;
        }
        end -= 1;
    }
    end
}

/// The link in `chars` that contains (or ends at) char index `idx`, as the address to open.
pub fn at(chars: &[char], idx: usize) -> Option<String> {
    let (a, b) = find(chars).into_iter().find(|&(a, b)| idx >= a && idx < b)?;
    Some(address(&chars[a..b]))
}

/// What to open for a link as written: `www.` gets `https://`.
pub fn address(link: &[char]) -> String {
    let s: String = link.iter().collect();
    if s.len() >= 4 && s[..4].eq_ignore_ascii_case("www.") {
        format!("https://{s}")
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links(s: &str) -> Vec<String> {
        let c: Vec<char> = s.chars().collect();
        find(&c).into_iter().map(|(a, b)| c[a..b].iter().collect()).collect()
    }

    #[test]
    fn finds_web_and_mail_links() {
        assert_eq!(links("see https://example.com/a?b=1#c now"), ["https://example.com/a?b=1#c"]);
        assert_eq!(links("HTTP://EXAMPLE.COM and www.example.org"), ["HTTP://EXAMPLE.COM", "www.example.org"]);
        assert_eq!(links("write to mailto:a@b.pt."), ["mailto:a@b.pt"]);
    }

    #[test]
    fn leaves_out_punctuation_around_a_link() {
        assert_eq!(links("(https://example.com/x)"), ["https://example.com/x"]);
        assert_eq!(links("https://en.wikipedia.org/wiki/Rust_(language)."), ["https://en.wikipedia.org/wiki/Rust_(language)"]);
        assert_eq!(links("<https://example.com>, \"https://a.b/c\"!"), ["https://example.com", "https://a.b/c"]);
        assert_eq!(links("[x](https://example.com/y)"), ["https://example.com/y"]);
    }

    #[test]
    fn ignores_what_is_not_a_link() {
        assert!(links("https:// alone, www. alone, awww.not, nohttps://x").is_empty());
        assert!(links("ftp://example.com file:///c:/x").is_empty());
    }

    #[test]
    fn at_gives_the_address_to_open() {
        let c: Vec<char> = "go www.example.com/x now".chars().collect();
        assert_eq!(at(&c, 5).as_deref(), Some("https://www.example.com/x"));
        assert_eq!(at(&c, 0), None);
        assert_eq!(at(&c, 22), None);
    }
}
