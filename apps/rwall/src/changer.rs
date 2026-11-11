//! One change of wallpaper, run on a background thread: a picture from the
//! next source (or, when it fails, from the ones after it), made ready,
//! put on the desktop, and the oldest downloads deleted.

use crate::picture;
use crate::schedule;
use crate::settings::{Current, Fit, Settings};
use crate::sources::{self, Pick, Request, Source};
use std::path::PathBuf;

pub struct Job {
    /// The sources to try, in order.
    pub order: Vec<Source>,
    pub request: Request,
    pub fit: Fit,
    /// Downloaded pictures to keep.
    pub keep: u32,
    /// Size of the preview in the settings window.
    pub thumb: (u32, u32),
}

pub struct Done {
    pub pick: Pick,
    /// The file on the desktop (a PNG copy for WebP).
    pub file: PathBuf,
    pub thumb: Option<tiny_skia::Pixmap>,
}

/// The sources turned on, starting with the one whose turn it is.
pub fn order(on: &[Source], next: usize) -> Vec<Source> {
    if on.is_empty() {
        return Vec::new();
    }
    let start = next % on.len();
    on[start..].iter().chain(&on[..start]).copied().collect()
}

/// The change the settings ask for: None when no source is on.
pub fn job(st: &Settings, images: PathBuf, screen: (u32, u32), thumb: (u32, u32)) -> Option<Job> {
    let order = order(&st.sources, st.next_source);
    if order.is_empty() {
        return None;
    }
    Some(Job {
        order,
        request: Request {
            screen,
            market: st.market(),
            nasa_key: st.nasa_key.clone(),
            wallhaven_query: st.wallhaven_query.clone(),
            folders: st.folders.clone(),
            subfolders: st.subfolders,
            history: st.history.clone(),
            images,
        },
        fit: st.fit,
        keep: st.keep,
        thumb,
    })
}

/// Writes a change into the settings: the picture now on the desktop, the
/// history, the period it was made in, and whose turn is next.
pub fn record(st: &mut Settings, pick: Pick, file: PathBuf, now: &rt_sys::LocalTime) {
    st.current = Some(Current {
        file,
        source: pick.source.key().to_string(),
        title: pick.title,
        credit: pick.credit,
        link: pick.link,
        when: schedule::format(now),
    });
    st.shown(&pick.id);
    st.last_period = schedule::period(st.frequency, now, st.custom_minutes);
    st.next_source = st.sources.iter().position(|s| *s == pick.source).map_or(0, |i| i + 1);
}

/// Where WebP pictures are converted.
pub fn converted_dir(images: &std::path::Path) -> PathBuf {
    images.join("converted")
}

pub fn run(job: &Job) -> Result<Done, String> {
    let mut errors = Vec::new();
    for &source in &job.order {
        match sources::fetch(source, &job.request).and_then(|pick| apply(job, pick)) {
            Ok(done) => return Ok(done),
            Err(e) => errors.push(format!("{}: {e}", source.key())),
        }
    }
    Err(errors.join("; "))
}

fn apply(job: &Job, pick: Pick) -> Result<Done, String> {
    let converted = converted_dir(&job.request.images);
    let file = picture::ready(&pick.file, &converted)?;
    crate::wallpaper::set(&file, job.fit)?;
    let keep = job.keep as usize;
    picture::trim(&job.request.images, keep, &file);
    picture::trim(&converted, keep, &file);
    let thumb = picture::thumbnail(&file, job.thumb.0, job.thumb.1);
    Ok(Done { pick, file, thumb })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_take_turns() {
        let on = [Source::Bing, Source::Nasa, Source::Folders];
        assert_eq!(order(&on, 0), on.to_vec());
        assert_eq!(order(&on, 4), vec![Source::Nasa, Source::Folders, Source::Bing]);
        assert!(order(&[], 3).is_empty());
    }

    #[test]
    fn a_change_is_recorded() {
        let mut st = Settings { sources: vec![Source::Bing, Source::Folders], ..Settings::default() };
        assert!(job(&Settings { sources: Vec::new(), ..Settings::default() }, PathBuf::new(), (1, 1), (1, 1)).is_none());
        assert_eq!(job(&st, PathBuf::new(), (1, 1), (1, 1)).unwrap().order, vec![Source::Bing, Source::Folders]);
        let pick = Pick { source: Source::Bing, id: "bing:1".into(), title: "T".into(), credit: String::new(), link: String::new(), file: "a.jpg".into() };
        let now = rt_sys::LocalTime { year: 2026, month: 11, day: 11, hour: 9, minute: 5, second: 0 };
        record(&mut st, pick, "a.jpg".into(), &now);
        assert_eq!(st.next_source, 1);
        assert_eq!(st.last_period, "d2026-11-11");
        assert_eq!(st.history, vec!["bing:1"]);
        assert_eq!(st.current.as_ref().map(|c| c.when.as_str()), Some("2026-11-11 09:05"));
    }
}
