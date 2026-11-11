//! What rwall remembers, in `settings.ini` (see `storage`).

use crate::i18n::Lang;
use crate::schedule::Frequency;
use crate::sources::Source;
use crate::storage::Store;
use std::path::PathBuf;

/// How the picture fills the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Covers the screen, cutting what sticks out.
    Fill,
    /// All of it, with bars at the sides or above and below.
    Fit,
    Stretch,
    Center,
    /// One picture across all the screens.
    Span,
}

impl Fit {
    pub const ALL: [Fit; 5] = [Fit::Fill, Fit::Fit, Fit::Stretch, Fit::Center, Fit::Span];

    pub fn key(self) -> &'static str {
        match self {
            Fit::Fill => "fill",
            Fit::Fit => "fit",
            Fit::Stretch => "stretch",
            Fit::Center => "center",
            Fit::Span => "span",
        }
    }

    pub fn from_key(k: &str) -> Option<Fit> {
        Fit::ALL.into_iter().find(|f| f.key() == k)
    }

    /// Text in the settings window (translated by the caller).
    pub fn label(self) -> &'static str {
        match self {
            Fit::Fill => "Fill",
            Fit::Fit => "Fit",
            Fit::Stretch => "Stretch",
            Fit::Center => "Center",
            Fit::Span => "Span",
        }
    }
}

/// The picture on the desktop now.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Current {
    pub file: PathBuf,
    pub source: String,
    pub title: String,
    pub credit: String,
    /// A page about the picture (may be empty).
    pub link: String,
    /// When it was set: "2026-11-11 18:00".
    pub when: String,
}

/// How many pictures to remember as shown, so they are not picked again soon.
pub const HISTORY: usize = 200;

/// Downloaded pictures kept, at least and at most.
pub const KEEP_MIN: u32 = 1;
pub const KEEP_MAX: u32 = 500;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub frequency: Frequency,
    /// Interval of `Frequency::Custom`.
    pub custom_minutes: u32,
    /// Sources turned on (in the order of `Source::ALL`).
    pub sources: Vec<Source>,
    /// Bing market, such as "pt-PT"; empty: the language of rwall.
    pub bing_market: String,
    /// NASA API key; empty: DEMO_KEY (a few requests an hour).
    pub nasa_key: String,
    /// Wallhaven search words; empty: anything.
    pub wallhaven_query: String,
    pub folders: Vec<PathBuf>,
    pub subfolders: bool,
    pub fit: Fit,
    /// Downloaded pictures to keep.
    pub keep: u32,
    /// The period of the last change (see `schedule::period`).
    pub last_period: String,
    /// Which source comes next (they take turns).
    pub next_source: usize,
    /// Pictures shown lately, newest last.
    pub history: Vec<String>,
    pub current: Option<Current>,
    /// None: the system's language.
    pub language: Option<Lang>,
    /// 0 = system, 1 = light, 2 = dark.
    pub theme: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            frequency: Frequency::Daily,
            custom_minutes: 60,
            sources: vec![Source::Bing],
            bing_market: String::new(),
            nasa_key: String::new(),
            wallhaven_query: String::new(),
            folders: Vec::new(),
            subfolders: true,
            fit: Fit::Fill,
            keep: 30,
            last_period: String::new(),
            next_source: 0,
            history: Vec::new(),
            current: None,
            language: None,
            theme: 0,
        }
    }
}

impl Settings {
    pub fn load(s: &Store) -> Settings {
        let d = Settings::default();
        let current = s.get("current.file").filter(|f| !f.is_empty()).map(|f| Current {
            file: PathBuf::from(f),
            source: s.get("current.source").unwrap_or("").to_string(),
            title: s.get("current.title").unwrap_or("").to_string(),
            credit: s.get("current.credit").unwrap_or("").to_string(),
            link: s.get("current.link").unwrap_or("").to_string(),
            when: s.get("current.when").unwrap_or("").to_string(),
        });
        let sources = match s.get("sources") {
            Some(list) => Source::ALL.into_iter().filter(|src| list.split(',').any(|k| k.trim() == src.key())).collect(),
            None => d.sources,
        };
        Settings {
            frequency: s.get("frequency").and_then(Frequency::from_key).unwrap_or(d.frequency),
            custom_minutes: s.get("custom_minutes").and_then(|v| v.parse().ok()).unwrap_or(d.custom_minutes).clamp(1, 7 * 1440),
            sources,
            bing_market: s.get("bing_market").unwrap_or("").trim().to_string(),
            nasa_key: s.get("nasa_key").unwrap_or("").trim().to_string(),
            wallhaven_query: s.get("wallhaven_query").unwrap_or("").to_string(),
            folders: s.get_all("folder").filter(|f| !f.is_empty()).map(PathBuf::from).collect(),
            subfolders: s.get("subfolders").map_or(d.subfolders, |v| v != "0"),
            fit: s.get("fit").and_then(Fit::from_key).unwrap_or(d.fit),
            keep: s.get("keep").and_then(|v| v.parse().ok()).unwrap_or(d.keep).clamp(KEEP_MIN, KEEP_MAX),
            last_period: s.get("last_period").unwrap_or("").to_string(),
            next_source: s.get("next_source").and_then(|v| v.parse().ok()).unwrap_or(0),
            history: s.get_all("shown").map(str::to_string).collect(),
            current,
            language: s.get("language").and_then(Lang::from_tag),
            theme: s.get("theme").and_then(|v| v.parse().ok()).filter(|t| *t <= 2).unwrap_or(0),
        }
    }

    pub fn to_store(&self) -> Store {
        let mut s = Store::default();
        s.set("frequency", self.frequency.key());
        s.set("custom_minutes", self.custom_minutes.to_string());
        s.set("sources", self.sources.iter().map(|x| x.key()).collect::<Vec<_>>().join(","));
        s.set("bing_market", self.bing_market.as_str());
        s.set("nasa_key", self.nasa_key.as_str());
        s.set("wallhaven_query", self.wallhaven_query.as_str());
        for f in &self.folders {
            s.push("folder", f.to_string_lossy());
        }
        s.set("subfolders", if self.subfolders { "1" } else { "0" });
        s.set("fit", self.fit.key());
        s.set("keep", self.keep.to_string());
        s.set("last_period", self.last_period.as_str());
        s.set("next_source", self.next_source.to_string());
        if let Some(c) = &self.current {
            s.set("current.file", c.file.to_string_lossy());
            s.set("current.source", c.source.as_str());
            s.set("current.title", c.title.as_str());
            s.set("current.credit", c.credit.as_str());
            s.set("current.link", c.link.as_str());
            s.set("current.when", c.when.as_str());
        }
        if let Some(l) = self.language {
            s.set("language", l.tag());
        }
        s.set("theme", self.theme.to_string());
        for h in &self.history {
            s.push("shown", h.as_str());
        }
        s
    }

    pub fn source_on(&self, src: Source) -> bool {
        self.sources.contains(&src)
    }

    /// Turns a source on or off, keeping the order of `Source::ALL`.
    pub fn toggle_source(&mut self, src: Source) {
        if self.source_on(src) {
            self.sources.retain(|s| *s != src);
        } else {
            self.sources.push(src);
            self.sources.sort_by_key(|s| Source::ALL.iter().position(|x| x == s));
        }
    }

    /// Remembers a picture as shown.
    pub fn shown(&mut self, id: &str) {
        self.history.retain(|h| h != id);
        self.history.push(id.to_string());
        let extra = self.history.len().saturating_sub(HISTORY);
        self.history.drain(..extra);
    }

    /// The Bing market asked for: the setting, or the language of rwall.
    pub fn market(&self) -> String {
        if self.bing_market.is_empty() {
            crate::i18n::lang().tag().to_string()
        } else {
            self.bing_market.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut s = Settings {
            frequency: Frequency::Custom,
            custom_minutes: 45,
            sources: vec![Source::Nasa, Source::Folders],
            wallhaven_query: "mountains, lakes".into(),
            folders: vec![PathBuf::from(r"C:\Fotos\Férias"), PathBuf::from("/home/a b")],
            subfolders: false,
            fit: Fit::Span,
            keep: 12,
            last_period: "c45-123".into(),
            next_source: 1,
            current: Some(Current { file: "x.jpg".into(), source: "bing".into(), title: "T\tx".into(), ..Current::default() }),
            language: Some(Lang::PtPT),
            theme: 2,
            ..Settings::default()
        };
        s.shown("a");
        s.shown("b");
        s.shown("a");
        let back = Settings::load(&Store::parse(&s.to_store().serialize("rwall")));
        assert_eq!(back, s);
        assert_eq!(back.history, vec!["b", "a"]);
    }

    #[test]
    fn defaults_and_bad_values() {
        let s = Settings::load(&Store::parse("frequency=hourly\nkeep=0\ntheme=9\nsources=bing, picsum,zzz\n"));
        assert_eq!(s.frequency, Frequency::Daily);
        assert_eq!(s.keep, KEEP_MIN);
        assert_eq!(s.theme, 0);
        assert_eq!(s.sources, vec![Source::Bing, Source::Picsum]);
        assert_eq!(Settings::load(&Store::default()).sources, vec![Source::Bing]);
        let mut t = Settings::default();
        t.toggle_source(Source::Folders);
        t.toggle_source(Source::Nasa);
        assert_eq!(t.sources, vec![Source::Bing, Source::Nasa, Source::Folders]);
        t.toggle_source(Source::Bing);
        assert_eq!(t.sources, vec![Source::Nasa, Source::Folders]);
        for _ in 0..HISTORY + 10 {
            t.shown(&t.history.len().to_string());
        }
        assert_eq!(t.history.len(), HISTORY);
    }
}
