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

/// `get_json`, asking again after a server error (a busy service may
/// answer 500 or 503 for a while).
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

/// The APOD list of science.nasa.gov (WordPress REST), newest first, with
/// only the fields rwall reads. No key is needed.
const APOD_LIST: &str = "https://science.nasa.gov/wp-json/wp/v2/apod-basic?_fields=date,title,media_type,hdurl,credit,copyright,permalink";
/// Pages of 20 entries reached at random: the archive holds more than
/// 11 000 days, so 500 pages stay inside it.
const APOD_PAGES: u64 = 500;

pub struct Apod {
    pub date: String,
    pub url: String,
    pub title: String,
    pub credit: String,
    /// The page of the day on science.nasa.gov.
    pub page: String,
}

/// The entries of an apod-basic answer that are pictures (some days are
/// videos).
pub fn apod_list(j: &Json) -> Vec<Apod> {
    j.items()
        .iter()
        .filter(|i| i.get("media_type").str() == "image" && i.get("hdurl").str().starts_with("https://"))
        .map(|i| {
            let credit = [i.get("credit").str(), i.get("copyright").str()].into_iter().map(plain_text).find(|c| !c.is_empty()).unwrap_or_default();
            let page = i.get("permalink").str();
            Apod {
                date: i.get("date").str().to_string(),
                url: i.get("hdurl").str().to_string(),
                title: plain_text(i.get("title").str()),
                credit: if credit.is_empty() { "NASA".to_string() } else { format!("© {credit}") },
                page: if page.starts_with("https://") { page.to_string() } else { "https://science.nasa.gov/apod/".to_string() },
            }
        })
        .collect()
}

/// Text of a WordPress field: tags taken out, the usual entities read,
/// spaces collapsed.
pub fn plain_text(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find(['<', '&']) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        if rest.starts_with('<') {
            rest = rest.find('>').map_or("", |j| &rest[j + 1..]);
            continue;
        }
        let end = rest.find(';').filter(|j| *j <= 10);
        let entity = end.map(|j| &rest[1..j]);
        let ch = match entity {
            Some("amp") => Some('&'),
            Some("lt") => Some('<'),
            Some("gt") => Some('>'),
            Some("quot") => Some('"'),
            Some("apos") => Some('\''),
            Some("nbsp") => Some(' '),
            Some(e) if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            Some(e) if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match (ch, end) {
            (Some(c), Some(j)) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn nasa(req: &Request) -> Result<Pick, String> {
    let id = |a: &Apod| format!("nasa:{}", a.date);
    // The latest days first; when they were all shown, a page of the
    // archive at random.
    let mut list = apod_list(&get_json_patiently(&format!("{APOD_LIST}&per_page=10"))?);
    let mut pos = list.iter().position(|a| !req.history.contains(&id(a)));
    for _ in 0..3 {
        if pos.is_some() {
            break;
        }
        let page = 2 + seed() % APOD_PAGES;
        list = apod_list(&get_json_patiently(&format!("{APOD_LIST}&per_page=20&page={page}"))?);
        pos = list.iter().position(|a| !req.history.contains(&id(a)));
    }
    let a = pos.map(|p| list.swap_remove(p)).ok_or("NASA: no picture found")?;
    let file = download(req, Source::Nasa, &a.date, &a.url)?;
    Ok(Pick { source: Source::Nasa, id: id(&a), title: a.title, credit: a.credit, link: a.page, file })
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
        let j = json::parse(
            r#"[{"date":"2026-11-11","title":"NGC 4449: Small Galaxy, Big&#8217;s &amp; More","media_type":"image",
            "hdurl":"https://assets.science.nasa.gov/a.jpg?w=2048&h=1320","credit":"NASA, <a href=\"x\">ESA</a>\n and  Hubble","permalink":"https://science.nasa.gov/image-article/apod-2026-november-11/"},
            {"date":"2026-11-10","media_type":"video","hdurl":"https://assets.science.nasa.gov/t.png"},
            {"date":"2026-11-09","media_type":"image","hdurl":"https://assets.science.nasa.gov/b.jpg","credit":"","copyright":""}]"#,
        )
        .unwrap();
        let l = apod_list(&j);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].title, "NGC 4449: Small Galaxy, Big’s & More");
        assert_eq!(l[0].credit, "© NASA, ESA and Hubble");
        assert_eq!(l[0].url, "https://assets.science.nasa.gov/a.jpg?w=2048&h=1320");
        assert_eq!((l[1].credit.as_str(), l[1].page.as_str()), ("NASA", "https://science.nasa.gov/apod/"));
        assert_eq!(plain_text("R&D &unknown; 5 &lt; 6 &#x263A;"), "R&D &unknown; 5 < 6 ☺");
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
        // With the latest days shown, NASA takes a page of the archive at random.
        let recent: Vec<String> = (1..=30).map(|d| format!("nasa:2026-10-{d:02}")).chain((1..=30).map(|d| format!("nasa:2026-11-{d:02}"))).collect();
        match super::super::fetch(Source::Nasa, &Request { history: recent, ..req }) {
            Ok(p) => println!("Nasa (archive): {} | {} | {}", p.id, p.title, p.link),
            Err(e) => println!("Nasa (archive): ERROR {e}"),
        }
        let _ = std::fs::remove_dir_all(images);
    }
}
