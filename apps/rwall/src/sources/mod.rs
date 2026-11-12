//! Where pictures come from: four web services and the user's folders.
//!
//! Each source answers a [`Request`] with a [`Pick`]: a picture file on disk
//! and what is known about it. A picture shown lately (its id is in the
//! history) is passed over while there are others.

mod folders;
mod online;

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// Bing's picture of the day (the last eight days).
    Bing,
    /// NASA's Astronomy Picture of the Day.
    Nasa,
    /// Wallhaven search, at random.
    Wallhaven,
    /// Lorem Picsum: a random photo at the size of the screen.
    Picsum,
    /// Pictures in the user's folders.
    Folders,
}

impl Source {
    pub const ALL: [Source; 5] = [Source::Bing, Source::Nasa, Source::Wallhaven, Source::Picsum, Source::Folders];

    /// Name in the settings file and in picture ids.
    pub fn key(self) -> &'static str {
        match self {
            Source::Bing => "bing",
            Source::Nasa => "nasa",
            Source::Wallhaven => "wallhaven",
            Source::Picsum => "picsum",
            Source::Folders => "folders",
        }
    }

    pub fn from_key(k: &str) -> Option<Source> {
        Source::ALL.into_iter().find(|s| s.key() == k)
    }

    /// Text in the window (the service names are not translated).
    pub fn label(self) -> &'static str {
        match self {
            Source::Bing => "Bing – picture of the day",
            Source::Nasa => "NASA – Astronomy Picture of the Day",
            Source::Wallhaven => "Wallhaven – search",
            Source::Picsum => "Picsum – random photo",
            Source::Folders => "My folders",
        }
    }

    pub fn online(self) -> bool {
        self != Source::Folders
    }
}

/// What a source needs to know.
#[derive(Clone, Debug, Default)]
pub struct Request {
    /// Size of the screen, in pixels.
    pub screen: (u32, u32),
    pub market: String,
    pub wallhaven_query: String,
    pub folders: Vec<PathBuf>,
    pub subfolders: bool,
    /// Ids of the pictures shown lately.
    pub history: Vec<String>,
    /// Where downloads are saved.
    pub images: PathBuf,
}

/// A picture ready to use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub source: Source,
    /// Unique among all sources: "bing:20261111-…".
    pub id: String,
    pub title: String,
    pub credit: String,
    pub link: String,
    /// The picture as downloaded or found.
    pub file: PathBuf,
}

/// Gets a picture from `source`.
pub fn fetch(source: Source, req: &Request) -> Result<Pick, String> {
    match source {
        Source::Bing => online::bing(req),
        Source::Nasa => online::nasa(req),
        Source::Wallhaven => online::wallhaven(req),
        Source::Picsum => online::picsum(req),
        Source::Folders => folders::pick(req),
    }
}

/// The first candidate not shown lately, or else the first of all.
fn fresh<'a, T>(items: &'a [T], id: impl Fn(&T) -> String, history: &[String]) -> Option<&'a T> {
    items.iter().find(|it| !history.contains(&id(it))).or(items.first())
}

/// A number that changes from call to call (for random choices).
fn seed() -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let mut x = t.as_nanos() as u64 ^ u64::from(std::process::id()).rotate_left(32) | 1;
    // xorshift64*
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip() {
        for s in Source::ALL {
            assert_eq!(Source::from_key(s.key()), Some(s));
        }
    }

    #[test]
    fn fresh_skips_what_was_shown() {
        let items = ["a", "b", "c"];
        let h = vec!["a".to_string(), "b".to_string()];
        assert_eq!(fresh(&items, |s| s.to_string(), &h), Some(&"c"));
        let all = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(fresh(&items, |s| s.to_string(), &all), Some(&"a"));
        assert_eq!(fresh::<&str>(&[], |s| s.to_string(), &h), None);
        assert_ne!(seed(), seed());
    }
}
