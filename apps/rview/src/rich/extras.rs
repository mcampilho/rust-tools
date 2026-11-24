//! Markdown extras that CommonMark does not have, as markdown-it and GitHub read them: emoji shortcodes
//! (`:wink:`) and emoticons (`:-)`), `++inserted++`, `==marked==` and abbreviations (`*[HTML]: …`).
//! They work on the text of a run, so they never touch code, links' targets or HTML.

use super::{Inline, Style};
use crate::ui::gfx::Color;

/// Shortcodes of the common emoji (GitHub names).
const EMOJI: &[(&str, &str)] = &[
    ("smile", "😄"),
    ("smiley", "😃"),
    ("grinning", "😀"),
    ("grin", "😁"),
    ("laughing", "😆"),
    ("satisfied", "😆"),
    ("joy", "😂"),
    ("rofl", "🤣"),
    ("wink", "😉"),
    ("blush", "😊"),
    ("innocent", "😇"),
    ("heart_eyes", "😍"),
    ("kissing_heart", "😘"),
    ("yum", "😋"),
    ("stuck_out_tongue", "😛"),
    ("stuck_out_tongue_winking_eye", "😜"),
    ("sunglasses", "😎"),
    ("smirk", "😏"),
    ("neutral_face", "😐"),
    ("expressionless", "😑"),
    ("unamused", "😒"),
    ("sweat_smile", "😅"),
    ("sweat", "😓"),
    ("pensive", "😔"),
    ("confused", "😕"),
    ("slightly_smiling_face", "🙂"),
    ("upside_down_face", "🙃"),
    ("relaxed", "☺️"),
    ("disappointed", "😞"),
    ("worried", "😟"),
    ("angry", "😠"),
    ("rage", "😡"),
    ("cry", "😢"),
    ("sob", "😭"),
    ("fearful", "😨"),
    ("scream", "😱"),
    ("flushed", "😳"),
    ("dizzy_face", "😵"),
    ("mask", "😷"),
    ("sleeping", "😴"),
    ("zzz", "💤"),
    ("thinking", "🤔"),
    ("hugs", "🤗"),
    ("hushed", "😯"),
    ("open_mouth", "😮"),
    ("astonished", "😲"),
    ("relieved", "😌"),
    ("sleepy", "😪"),
    ("triumph", "😤"),
    ("persevere", "😣"),
    ("weary", "😩"),
    ("tired_face", "😫"),
    ("skull", "💀"),
    ("poop", "💩"),
    ("ghost", "👻"),
    ("alien", "👽"),
    ("robot", "🤖"),
    ("see_no_evil", "🙈"),
    ("hear_no_evil", "🙉"),
    ("speak_no_evil", "🙊"),
    ("+1", "👍"),
    ("thumbsup", "👍"),
    ("-1", "👎"),
    ("thumbsdown", "👎"),
    ("ok_hand", "👌"),
    ("v", "✌️"),
    ("wave", "👋"),
    ("clap", "👏"),
    ("raised_hands", "🙌"),
    ("pray", "🙏"),
    ("muscle", "💪"),
    ("point_up", "☝️"),
    ("point_down", "👇"),
    ("point_left", "👈"),
    ("point_right", "👉"),
    ("fist", "✊"),
    ("handshake", "🤝"),
    ("eyes", "👀"),
    ("brain", "🧠"),
    ("heart", "❤️"),
    ("yellow_heart", "💛"),
    ("green_heart", "💚"),
    ("blue_heart", "💙"),
    ("purple_heart", "💜"),
    ("broken_heart", "💔"),
    ("sparkling_heart", "💖"),
    ("100", "💯"),
    ("boom", "💥"),
    ("collision", "💥"),
    ("fire", "🔥"),
    ("sparkles", "✨"),
    ("star", "⭐"),
    ("star2", "🌟"),
    ("zap", "⚡"),
    ("sunny", "☀️"),
    ("cloud", "☁️"),
    ("umbrella", "☔"),
    ("snowflake", "❄️"),
    ("rainbow", "🌈"),
    ("ocean", "🌊"),
    ("earth_africa", "🌍"),
    ("tada", "🎉"),
    ("confetti_ball", "🎊"),
    ("balloon", "🎈"),
    ("gift", "🎁"),
    ("trophy", "🏆"),
    ("medal_sports", "🏅"),
    ("dart", "🎯"),
    ("rocket", "🚀"),
    ("airplane", "✈️"),
    ("car", "🚗"),
    ("bike", "🚲"),
    ("checkered_flag", "🏁"),
    ("bulb", "💡"),
    ("book", "📖"),
    ("books", "📚"),
    ("memo", "📝"),
    ("pencil", "📝"),
    ("pencil2", "✏️"),
    ("page_facing_up", "📄"),
    ("clipboard", "📋"),
    ("pushpin", "📌"),
    ("paperclip", "📎"),
    ("link", "🔗"),
    ("lock", "🔒"),
    ("unlock", "🔓"),
    ("key", "🔑"),
    ("mag", "🔍"),
    ("bell", "🔔"),
    ("email", "📧"),
    ("envelope", "✉️"),
    ("phone", "☎️"),
    ("computer", "💻"),
    ("keyboard", "⌨️"),
    ("desktop_computer", "🖥️"),
    ("floppy_disk", "💾"),
    ("package", "📦"),
    ("hammer", "🔨"),
    ("wrench", "🔧"),
    ("gear", "⚙️"),
    ("nut_and_bolt", "🔩"),
    ("bug", "🐛"),
    ("construction", "🚧"),
    ("warning", "⚠️"),
    ("no_entry", "⛔"),
    ("stop_sign", "🛑"),
    ("x", "❌"),
    ("heavy_check_mark", "✔️"),
    ("white_check_mark", "✅"),
    ("ballot_box_with_check", "☑️"),
    ("question", "❓"),
    ("exclamation", "❗"),
    ("heavy_plus_sign", "➕"),
    ("heavy_minus_sign", "➖"),
    ("arrow_right", "➡️"),
    ("arrow_left", "⬅️"),
    ("arrow_up", "⬆️"),
    ("arrow_down", "⬇️"),
    ("recycle", "♻️"),
    ("hourglass", "⌛"),
    ("watch", "⌚"),
    ("alarm_clock", "⏰"),
    ("calendar", "📆"),
    ("chart_with_upwards_trend", "📈"),
    ("bar_chart", "📊"),
    ("moneybag", "💰"),
    ("dollar", "💵"),
    ("coffee", "☕"),
    ("beer", "🍺"),
    ("pizza", "🍕"),
    ("apple", "🍎"),
    ("cake", "🍰"),
    ("dog", "🐶"),
    ("cat", "🐱"),
    ("octocat", "🐙"),
    ("penguin", "🐧"),
    ("snake", "🐍"),
    ("crab", "🦀"),
];

/// Emoticons and the shortcode they stand for (markdown-it-emoji's shortcuts).
const EMOTICONS: &[(&str, &str)] = &[
    (":-)", "slightly_smiling_face"),
    (":)", "slightly_smiling_face"),
    ("=)", "slightly_smiling_face"),
    ("=-)", "slightly_smiling_face"),
    (":-(", "disappointed"),
    (":(", "disappointed"),
    ("=(", "disappointed"),
    (":-D", "smiley"),
    (":D", "smiley"),
    ("8-)", "sunglasses"),
    ("B-)", "sunglasses"),
    (";-)", "wink"),
    (";)", "wink"),
    (":-P", "stuck_out_tongue"),
    (":P", "stuck_out_tongue"),
    (":-O", "open_mouth"),
    (":-/", "confused"),
    (":'(", "cry"),
    (":'-(", "cry"),
    (":-|", "neutral_face"),
    ("<3", "heart"),
];

/// The emoji for a shortcode name (without colons).
pub fn emoji(name: &str) -> Option<&'static str> {
    EMOJI.iter().find(|(n, _)| *n == name).map(|(_, e)| *e)
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Emoticons are only read as such when set apart by space or at the ends of the text.
fn emoticon(s: &str, i: usize) -> Option<(usize, &'static str)> {
    let before = s[..i].chars().next_back();
    if before.is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | '[')) {
        return None;
    }
    let rest = &s[i..];
    EMOTICONS.iter().find_map(|(e, name)| {
        let tail = rest.strip_prefix(e)?;
        if tail.chars().next().is_some_and(word_char) {
            return None;
        }
        Some((e.len(), emoji(name)?))
    })
}

fn shortcode(s: &str, i: usize) -> Option<(usize, &'static str)> {
    let rest = s[i..].strip_prefix(':')?;
    let end = rest.find(':')?;
    let name = &rest[..end];
    if name.is_empty() || name.len() > 40 || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-')) {
        return None;
    }
    Some((end + 2, emoji(name)?))
}

/// `++x++` / `==x==`: the length taken and the inside, when the marks hug the text.
fn delimited<'a>(s: &'a str, i: usize, mark: &str) -> Option<(usize, &'a str)> {
    let rest = s[i..].strip_prefix(mark)?;
    if rest.chars().next().is_none_or(char::is_whitespace) {
        return None;
    }
    let mut from = 0;
    while let Some(p) = rest[from..].find(mark) {
        let end = from + p;
        if end > 0 && !rest[..end].ends_with(char::is_whitespace) && !rest[..end].contains('\n') {
            // Longer runs of the mark belong to something else.
            if rest[end + mark.len()..].starts_with(mark.chars().next().unwrap_or('+')) {
                from = end + 1;
                continue;
            }
            return Some((mark.len() * 2 + end, &rest[..end]));
        }
        from = end + 1;
    }
    None
}

/// `^sup^` / `~sub~`: one mark each side and no space inside (as markdown-it-sup and -sub read them).
fn single(s: &str, i: usize, mark: char) -> Option<(usize, &str)> {
    let rest = s[i..].strip_prefix(mark)?;
    let end = rest.find(mark)?;
    let inner = &rest[..end];
    let bad = inner.is_empty() || inner.contains(char::is_whitespace) || rest[end + 1..].starts_with(mark);
    (!bad).then_some((inner.len() + 2, inner))
}

fn abbreviation(s: &str, i: usize, abbrs: &[(String, String)]) -> Option<usize> {
    if s[..i].chars().next_back().is_some_and(word_char) {
        return None;
    }
    abbrs.iter().find_map(|(k, _)| {
        let tail = s[i..].strip_prefix(k.as_str())?;
        (!tail.chars().next().is_some_and(word_char)).then_some(k.len())
    })
}

/// A run of Markdown text as inline pieces, with the extras applied. `abbrs` are the `*[HTML]: …` definitions.
pub fn decorate(text: &str, style: Style, abbrs: &[(String, String)]) -> Vec<Inline> {
    let maybe =
        text.contains([':', ';', '^', '~']) || text.contains("++") || text.contains("==") || text.contains("<3") || !abbrs.is_empty();
    if !maybe || text.len() > 200_000 {
        return vec![Inline::Text(text.to_string(), style)];
    }
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    while i < text.len() {
        let c = text[i..].chars().next().unwrap_or(' ');
        let hit: Option<(usize, Vec<Inline>)> = match c {
            ':' | ';' | '8' | 'B' | '=' | '<' if c != '=' || !text[i..].starts_with("==") => {
                shortcode(text, i).or_else(|| emoticon(text, i)).map(|(n, e)| (n, vec![Inline::Text(e.to_string(), style)]))
            }
            _ => None,
        }
        .or_else(|| match c {
            '+' => delimited(text, i, "++").map(|(n, inner)| (n, decorate(inner, Style { underline: true, ..style }, abbrs))),
            '=' => {
                delimited(text, i, "==").map(|(n, inner)| (n, decorate(inner, Style { bg: Some(Color::rgb(0xFFF3A3)), ..style }, abbrs)))
            }
            '^' | '~' => single(text, i, c).map(|(n, inner)| {
                let (valign, size) = if c == '^' { (1, style.size * 0.8) } else { (-1, style.size * 0.8) };
                (n, decorate(inner, Style { valign, size, ..style }, abbrs))
            }),
            _ => None,
        })
        .or_else(|| {
            abbreviation(text, i, abbrs).map(|n| (n, vec![Inline::Text(text[i..i + n].to_string(), Style { underline: true, ..style })]))
        });
        match hit {
            Some((n, pieces)) => {
                if !plain.is_empty() {
                    out.push(Inline::Text(std::mem::take(&mut plain), style));
                }
                out.extend(pieces);
                i += n;
            }
            None => {
                plain.push(c);
                i += c.len_utf8();
            }
        }
    }
    if !plain.is_empty() {
        out.push(Inline::Text(plain, style));
    }
    out
}

/// Takes the `*[ABBR]: meaning` definition lines out of the source (the ones outside code fences) and returns them.
pub fn take_abbreviations(src: &str) -> (String, Vec<(String, String)>) {
    if !src.contains("*[") {
        return (src.to_string(), Vec::new());
    }
    let mut out = String::with_capacity(src.len());
    let mut abbrs = Vec::new();
    let mut fence: Option<char> = None;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        if let Some(f) = t.chars().next().filter(|c| matches!(c, '`' | '~')) {
            if t.starts_with(&f.to_string().repeat(3)) {
                fence = if fence == Some(f) { None } else { fence.or(Some(f)) };
            }
        }
        if fence.is_none() {
            if let Some(def) = t.strip_prefix("*[").and_then(|r| r.split_once("]:")) {
                let (key, meaning) = (def.0.trim(), def.1.trim());
                if !key.is_empty() && !key.contains('[') {
                    abbrs.push((key.to_string(), meaning.to_string()));
                    out.push('\n');
                    continue;
                }
            }
        }
        out.push_str(line);
    }
    // Longest first, so `HTML5` wins over `HTML`.
    abbrs.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
    (out, abbrs)
}

/// One part of a source: plain Markdown, or the inside of a `::: kind` container.
#[derive(Debug, PartialEq)]
pub enum Part {
    Text(String),
    Container(String, String),
}

/// Splits the source at `::: kind` … `:::` containers (not inside code fences). Without containers, one part.
pub fn split_containers(src: &str) -> Vec<Part> {
    if !src.contains(":::") {
        return vec![Part::Text(src.to_string())];
    }
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut open: Option<(String, String)> = None;
    let mut fence: Option<char> = None;
    for line in src.split_inclusive('\n') {
        let t = line.trim();
        if let Some(f) = t.chars().next().filter(|c| matches!(c, '`' | '~')) {
            if t.starts_with(&f.to_string().repeat(3)) {
                fence = if fence == Some(f) { None } else { fence.or(Some(f)) };
            }
        }
        if fence.is_none() && t.starts_with(":::") {
            let name = t.trim_start_matches(':').trim();
            match (&mut open, name.is_empty()) {
                (Some(_), true) => {
                    let (kind, body) = open.take().unwrap_or_default();
                    parts.push(Part::Container(kind, body));
                    continue;
                }
                (None, false) => {
                    if !text.is_empty() {
                        parts.push(Part::Text(std::mem::take(&mut text)));
                    }
                    open = Some((name.to_string(), String::new()));
                    continue;
                }
                _ => {}
            }
        }
        match &mut open {
            Some((_, body)) => body.push_str(line),
            None => text.push_str(line),
        }
    }
    if let Some((kind, body)) = open {
        parts.push(Part::Container(kind, body));
    }
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(v: &[Inline]) -> Vec<String> {
        v.iter()
            .map(|i| match i {
                Inline::Text(t, s) => format!("{t}{}{}", if s.underline { "_" } else { "" }, if s.bg.is_some() { "#" } else { "" }),
                _ => "?".into(),
            })
            .collect()
    }

    #[test]
    fn shortcodes_and_emoticons() {
        let v = decorate("a :wink: b :nope: :-) c", Style::default(), &[]);
        assert_eq!(texts(&v), ["a ", "😉", " b :nope: ", "🙂", " c"]);
        let v = decorate("(8-) and ;) too, :D.", Style::default(), &[]);
        assert_eq!(texts(&v), ["(", "😎", " and ", "😉", " too, ", "😃", "."]);
    }

    #[test]
    fn urls_and_plain_colons_are_left_alone() {
        for s in ["see http://example.com:8080/x", "a:b:c", "time 12:30:45", "x;)y", "C:\\dir", ":Debug"] {
            let v = decorate(s, Style::default(), &[]);
            assert_eq!(texts(&v), [s.to_string()], "{s}");
        }
    }

    #[test]
    fn superscript_and_subscript() {
        let v = decorate("19^th^ and H~2~O, not a ~ b ~ c or ~/bin and ~/x", Style::default(), &[]);
        let marks: Vec<(String, i8)> = v
            .iter()
            .filter_map(|i| match i {
                Inline::Text(t, s) if s.valign != 0 => Some((t.clone(), s.valign)),
                _ => None,
            })
            .collect();
        assert_eq!(marks, [("th".to_string(), 1), ("2".to_string(), -1)]);
    }

    #[test]
    fn inserted_and_marked() {
        let v = decorate("an ++added++ and ==marked :wink:== word", Style::default(), &[]);
        assert_eq!(texts(&v), ["an ", "added_", " and ", "marked #", "😉#", " word"]);
        let v = decorate("a == b and 1 ++ 2 and x==y", Style::default(), &[]);
        assert_eq!(texts(&v), ["a == b and 1 ++ 2 and x==y"]);
    }

    #[test]
    fn abbreviations_by_whole_word() {
        let (src, abbrs) = take_abbreviations("This is HTML.\n\n*[HTML]: Hyper Text Markup Language\n");
        assert_eq!(src, "This is HTML.\n\n\n");
        assert_eq!(abbrs, [("HTML".to_string(), "Hyper Text Markup Language".to_string())]);
        let v = decorate("an HTML page, xxxHTMLyyy, HTML5", Style::default(), &abbrs);
        assert_eq!(texts(&v), ["an ", "HTML_", " page, xxxHTMLyyy, HTML5"]);
        let (src, abbrs) = take_abbreviations("```\n*[A]: b\n```\n");
        assert!(abbrs.is_empty() && src.contains("*[A]"));
    }

    #[test]
    fn containers_are_split_out() {
        let p = split_containers("before\n\n::: warning\nhere be dragons\n:::\n\nafter\n");
        assert_eq!(
            p,
            [
                Part::Text("before\n\n".into()),
                Part::Container("warning".into(), "here be dragons\n".into()),
                Part::Text("\nafter\n".into())
            ]
        );
        assert_eq!(split_containers("no containers\n"), [Part::Text("no containers\n".into())]);
        let p = split_containers("```\n::: x\n```\n");
        assert_eq!(p, [Part::Text("```\n::: x\n```\n".into())]);
        let p = split_containers("::: tip\nunclosed\n");
        assert_eq!(p, [Part::Container("tip".into(), "unclosed\n".into())]);
    }
}
