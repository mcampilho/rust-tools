//! The web services. Each asks for a list or a picture, picks one not shown
//! lately, downloads it into the pictures folder and checks that it is a
//! picture large enough for a desktop.

use super::{fresh, seed, Pick, Request, Source};
use crate::json::{self, Json};
use crate::net::{self, encode};
use crate::picture;

/// Largest answer read as a list (JSON).
const LIST_LIMIT: usize = 4 << 20;
/// Largest picture downloaded.
const PICTURE_LIMIT: usize = 64 << 20;

fn get_json(url: &str) -> Result<Json, String> {
    let r = net::get(url, LIST_LIMIT)?;
    json::parse(&String::from_utf8_lossy(&r.body))
}

/// `get_json`, asking again after a server error (the NASA API often
/// answers 500 or 503 for a while).
fn get_json_patiently(url: &str) -> Result<Json, String> {
    let mut last = String::new();
    for wait in [0, 3, 10] {
        std::thread::sleep(std::time::Duration::from_secs(wait));
        match get_json(url) {
            Ok(j) => return Ok(j),
            Err(e) if e.contains("HTTP 5") => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// Downloads a picture and saves it as `<images>/<source>-<name>.<ext>`.
fn download(req: &Request, source: Source, name: &str, url: &str) -> Result<std::path::PathBuf, String> {
    let r = net::get(url, PICTURE_LIMIT)?;
    let k = picture::check(&r.body)?;
    picture::save(&req.images, &format!("{}-{name}", source.key()), &r.body, k)
}

// ------------------------------------------------------------------ Bing

pub struct BingImage {
    pub date: String,
    pub hash: String,
    pub url: String,
    pub title: String,
    pub credit: String,
    pub link: String,
}

/// The pictures in an answer of HPImageArchive.aspx, newest first.
pub fn bing_list(j: &Json) -> Vec<BingImage> {
    j.get("images")
        .items()
        .iter()
        .filter(|i| !i.get("urlbase").str().is_empty() || !i.get("url").str().is_empty())
        .map(|i| {
            let base = i.get("urlbase").str();
            // The largest size there is; `url` (1920×1080) when there is no base.
            let path = if base.is_empty() { i.get("url").str().to_string() } else { format!("{base}_UHD.jpg") };
            let link = i.get("copyrightlink").str();
            let (title, credit) = bing_caption(i.get("copyright").str(), i.get("title").str());
            BingImage {
                date: i.get("startdate").str().to_string(),
                hash: i.get("hsh").str().chars().take(12).collect(),
                url: if path.starts_with("http") { path } else { format!("https://www.bing.com{path}") },
                title,
                credit,
                link: if link.starts_with("https://") { link.to_string() } else { String::new() },
            }
        })
        .collect()
}

/// "The Atlantic Forest, Brazil (© FG Trade/Getty Images)" → the place and
/// the credit. Bing's own `title` ("Info" in some markets) is the fallback.
pub fn bing_caption(copyright: &str, title: &str) -> (String, String) {
    match copyright.rfind(" (©") {
        Some(i) if copyright.ends_with(')') => (copyright[..i].trim().to_string(), copyright[i + 2..copyright.len() - 1].trim().to_string()),
        _ if !title.is_empty() && title != "Info" => (title.to_string(), copyright.to_string()),
        _ => (copyright.to_string(), String::new()),
    }
}

fn bing_id(b: &BingImage) -> String {
    format!("bing:{}-{}", b.date, b.hash)
}

pub fn bing(req: &Request) -> Result<Pick, String> {
    let url = format!("https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=8&mkt={}", encode(&req.market));
    let list = bing_list(&get_json(&url)?);
    let b = fresh(&list, bing_id, &req.history).ok_or("Bing: no pictures in the answer")?;
    let file = download(req, Source::Bing, &format!("{}-{}", b.date, b.hash), &b.url)?;
    Ok(Pick { source: Source::Bing, id: bing_id(b), title: b.title.clone(), credit: b.credit.clone(), link: b.link.clone(), file })
}

// ------------------------------------------------------------------ NASA

pub struct Apod {
    pub date: String,
    /// The large picture first, then the one shown on the page.
    pub urls: Vec<String>,
    pub title: String,
    pub credit: String,
}

/// An APOD entry, when it is a picture (some days are videos).
pub fn apod(j: &Json) -> Option<Apod> {
    if j.get("media_type").str() != "image" {
        return None;
    }
    let urls: Vec<String> = [j.get("hdurl").str(), j.get("url").str()].into_iter().filter(|u| u.starts_with("https://")).map(str::to_string).collect();
    if urls.is_empty() {
        return None;
    }
    let credit = j.get("copyright").str().split_whitespace().collect::<Vec<_>>().join(" ");
    Some(Apod {
        date: j.get("date").str().to_string(),
        urls,
        title: j.get("title").str().to_string(),
        credit: if credit.is_empty() { "NASA".to_string() } else { format!("© {credit}") },
    })
}

/// The APOD page of a date: "2026-11-11" → …/ap261111.html.
pub fn apod_page(date: &str) -> String {
    let d: String = date.chars().filter(char::is_ascii_digit).collect();
    if d.len() == 8 {
        format!("https://apod.nasa.gov/apod/ap{}.html", &d[2..])
    } else {
        "https://apod.nasa.gov/apod/".to_string()
    }
}

/// A day of the APOD archive at random, from 2000 to yesterday.
fn random_apod_day() -> String {
    use crate::schedule::{civil_from_days, days_from_civil};
    let t = rt_sys::local_time();
    let (first, last) = (days_from_civil(2000, 1, 1), days_from_civil(t.year, t.month, t.day) - 1);
    let (y, m, d) = civil_from_days(first + (seed() % (last - first) as u64) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn nasa(req: &Request) -> Result<Pick, String> {
    let key = if req.nasa_key.is_empty() { "DEMO_KEY" } else { &req.nasa_key };
    let base = format!("https://api.nasa.gov/planetary/apod?api_key={}", encode(key));
    let id = |a: &Apod| format!("nasa:{}", a.date);
    // Today's picture first; when it was shown (or is a video), a day of
    // the archive at random.
    let mut found = apod(&get_json_patiently(&base)?).filter(|a| !req.history.contains(&id(a)));
    for _ in 0..4 {
        if found.is_some() {
            break;
        }
        found = apod(&get_json_patiently(&format!("{base}&date={}", random_apod_day()))?).filter(|a| !req.history.contains(&id(a)));
    }
    let a = found.ok_or("NASA: no picture found")?;
    let mut result = Err("NASA: no picture address".to_string());
    for url in &a.urls {
        result = download(req, Source::Nasa, &a.date, url);
        if result.is_ok() {
            break;
        }
    }
    let file = result?;
    Ok(Pick { source: Source::Nasa, id: id(&a), title: a.title, credit: a.credit, link: apod_page(&a.date), file })
}
// ------------------------------------------------------------- Wallhaven

pub struct WallhavenImage {
    pub id: String,
    pub url: String,
    pub page: String,
    pub resolution: String,
}

pub fn wallhaven_list(j: &Json) -> Vec<WallhavenImage> {
    j.get("data")
        .items()
        .iter()
        .filter(|i| i.get("path").str().starts_with("https://"))
        .map(|i| WallhavenImage {
            id: i.get("id").str().to_string(),
            url: i.get("path").str().to_string(),
            page: i.get("url").str().to_string(),
            resolution: i.get("resolution").str().to_string(),
        })
        .collect()
}

pub fn wallhaven(req: &Request) -> Result<Pick, String> {
    let (w, h) = req.screen;
    // Safe for work only (purity=100), general, anime and people.
    let url = format!(
        "https://wallhaven.cc/api/v1/search?q={}&categories=111&purity=100&sorting=random&seed={:06x}&atleast={w}x{h}&ratios=landscape",
        encode(req.wallhaven_query.trim()),
        seed() & 0xFF_FFFF
    );
    let list = wallhaven_list(&get_json(&url)?);
    let id = |i: &WallhavenImage| format!("wallhaven:{}", i.id);
    let i = fresh(&list, id, &req.history).ok_or("Wallhaven: nothing found for these words")?;
    let file = download(req, Source::Wallhaven, &i.id, &i.url)?;
    let title = if req.wallhaven_query.trim().is_empty() { format!("Wallhaven {}", i.id) } else { req.wallhaven_query.trim().to_string() };
    Ok(Pick { source: Source::Wallhaven, id: id(i), title, credit: format!("Wallhaven · {}", i.resolution), link: i.page.clone(), file })
}

// ---------------------------------------------------------------- Picsum

/// The photo number in the address a Picsum request was sent to:
/// "https://fastly.picsum.photos/id/237/1920/1080.jpg?hmac=…" → "237".
pub fn picsum_id(url: &str) -> Option<&str> {
    let rest = url.split("/id/").nth(1)?;
    let id = rest.split('/').next()?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

pub fn picsum(req: &Request) -> Result<Pick, String> {
    let (w, h) = req.screen;
    for _ in 0..3 {
        let r = net::get(&format!("https://picsum.photos/{w}/{h}.jpg?random={}", seed() % 1_000_000), PICTURE_LIMIT)?;
        let photo = picsum_id(&r.url).unwrap_or("0").to_string();
        let id = format!("picsum:{photo}");
        if req.history.contains(&id) {
            continue;
        }
        let k = picture::check(&r.body)?;
        let file = picture::save(&req.images, &format!("picsum-{photo}-{w}x{h}"), &r.body, k)?;
        // Who took it, when Picsum says.
        let info = get_json(&format!("https://picsum.photos/id/{photo}/info")).ok();
        let author = info.as_ref().map(|j| j.get("author").str().to_string()).filter(|a| !a.is_empty());
        let link = info.as_ref().map(|j| j.get("url").str().to_string()).filter(|u| u.starts_with("https://")).unwrap_or_default();
        return Ok(Pick {
            source: Source::Picsum,
            id,
            title: format!("Picsum #{photo}"),
            credit: author.map_or_else(|| "Lorem Picsum".to_string(), |a| format!("© {a}")),
            link,
            file,
        });
    }
    Err("Picsum: only pictures shown lately came back".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bing_answers() {
        let j = json::parse(
            r#"{"images":[{"startdate":"20261111","urlbase":"/th?id=OHR.Tower_PT-PT123","url":"/th?id=OHR.Tower_PT-PT123_1920x1080.jpg",
            "copyright":"Torre (© X)","copyrightlink":"https://www.bing.com/search?q=torre","title":"Torre","hsh":"0123456789abcdef"},
            {"startdate":"20261110","url":"/th?id=OHR.Old_1920x1080.jpg","copyrightlink":"javascript:void(0)","hsh":"ff"}]}"#,
        )
        .unwrap();
        let l = bing_list(&j);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].url, "https://www.bing.com/th?id=OHR.Tower_PT-PT123_UHD.jpg");
        assert_eq!(bing_id(&l[0]), "bing:20261111-0123456789ab");
        assert_eq!(l[1].url, "https://www.bing.com/th?id=OHR.Old_1920x1080.jpg");
        assert_eq!(l[1].link, "");
        assert_eq!((l[0].title.as_str(), l[0].credit.as_str()), ("Torre", "© X"));
        assert_eq!(bing_caption("A bay", "Info"), ("A bay".to_string(), String::new()));
        assert_eq!(bing_caption("© Y", "Cliffs"), ("Cliffs".to_string(), "© Y".to_string()));
    }

    #[test]
    fn apod_answers() {
        let pic = json::parse(r#"{"date":"2026-11-11","media_type":"image","hdurl":"https://apod.nasa.gov/a.jpg","title":"M31","copyright":"\nAnn\n Smith "}"#).unwrap();
        let a = apod(&pic).unwrap();
        assert_eq!((a.urls[0].as_str(), a.credit.as_str()), ("https://apod.nasa.gov/a.jpg", "© Ann Smith"));
        assert!(apod(&json::parse(r#"{"media_type":"video","url":"https://youtube.com/x"}"#).unwrap()).is_none());
        assert_eq!(apod_page("2026-11-11"), "https://apod.nasa.gov/apod/ap261111.html");
    }

    #[test]
    fn wallhaven_and_picsum_answers() {
        let j = json::parse(r#"{"data":[{"id":"abc","url":"https://wallhaven.cc/w/abc","path":"https://w.wallhaven.cc/full/ab/wallhaven-abc.jpg","resolution":"3840x2160"}],"meta":{}}"#).unwrap();
        let l = wallhaven_list(&j);
        assert_eq!((l[0].id.as_str(), l[0].resolution.as_str()), ("abc", "3840x2160"));
        assert_eq!(picsum_id("https://fastly.picsum.photos/id/237/1920/1080.jpg?hmac=x"), Some("237"));
        assert_eq!(picsum_id("https://picsum.photos/1920/1080"), None);
    }

    /// Downloads one picture from each service (run on request: it needs the network).
    #[test]
    #[ignore]
    fn live_services() {
        let images = std::env::temp_dir().join(format!("rwall-live-{}", std::process::id()));
        let req = Request { screen: (1920, 1080), market: "pt-PT".into(), images: images.clone(), ..Request::default() };
        for s in [Source::Bing, Source::Nasa, Source::Wallhaven, Source::Picsum] {
            match super::super::fetch(s, &req) {
                Ok(p) => println!("{s:?}: {} | {} | {} | {}", p.title, p.credit, p.link, p.file.display()),
                Err(e) => println!("{s:?}: ERROR {e}"),
            }
        }
        let _ = std::fs::remove_dir_all(images);
    }
}
