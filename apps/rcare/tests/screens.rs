//! Pictures of the window to look at: `cargo test -p rcare --test screens -- --ignored`
//! writes `apps/rcare/target/screens/*.png` (the sidebar open and collapsed,
//! light and dark, with the cleaning page beside it).

use rcare::pages::PAGES;
use rcare::sidebar::{self, Hit, State};
use rclean::app::App;
use rclean::page::CleanPage;
use rclean::targets::{all, Env};
use rclean::ui::gfx::{Gfx, Rect};
use rclean::ui::theme::Theme;
use std::sync::Arc;

fn frame(dark: bool, collapsed: bool, hot: Option<Hit>, w: u32, h: u32) -> tiny_skia::Pixmap {
    rcare::i18n::set_lang(rcare::i18n::Lang::EnUS);
    let mut g = Gfx::new().expect("fonts");
    g.resize(w, h);
    let mut page = CleanPage::new(App::new(all(&Env::default()), false, &[]), false, std::env::temp_dir(), Arc::new(|_| {}));
    let th = Theme::new(dark, None);
    let s = 1.0;
    let mut rest = Rect::new(0.0, 0.0, w as f32, h as f32);
    let side = rest.take_left(sidebar::width(collapsed, s));
    let l = sidebar::layout(side, PAGES.len(), s);
    page.render(&mut g, rest, &th, s, false);
    sidebar::draw(&mut g, &l, &th, s, State { collapsed, selected: 1, hot }, PAGES);
    let mut buf = vec![0u32; (w * h) as usize];
    g.copy_to(&mut buf);
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    for (d, p) in pm.data_mut().chunks_mut(4).zip(buf) {
        d.copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, 255]);
    }
    pm
}

/// The duplicates page (selected = 1), the disk space page (2) or the uninstaller (3) at its start.
fn other_frame(selected: usize, dark: bool, w: u32, h: u32) -> tiny_skia::Pixmap {
    rcare::i18n::set_lang(rcare::i18n::Lang::EnUS);
    let mut g = Gfx::new().expect("fonts");
    g.resize(w, h);
    let th = Theme::new(dark, None);
    let s = 1.0;
    let mut rest = Rect::new(0.0, 0.0, w as f32, h as f32);
    let side = rest.take_left(sidebar::width(false, s));
    let l = sidebar::layout(side, PAGES.len(), s);
    match selected {
        2 => {
            let mut page = rdup::page::DupPage::new(&rdup::settings::Settings::default(), &[], None, false, Arc::new(|_| {}));
            page.render(&mut g, rest, &rdup::ui::theme::Theme::new(dark, None), s, false);
        }
        3 => {
            let mut page = rsize::page::SizePage::new(rsize::settings::Settings::default(), false, Arc::new(|_| {}));
            page.render(&mut g, rest, &rsize::ui::theme::Theme::new(dark, None), s, false);
        }
        _ => {
            let mut page = rinst::page::InstPage::new(&rinst::settings::Settings::default(), false, std::env::temp_dir(), Arc::new(|_| {}));
            page.render(&mut g, rest, &rinst::ui::theme::Theme::new(dark, None), s, false);
        }
    }
    sidebar::draw(&mut g, &l, &th, s, State { collapsed: false, selected, hot: None }, PAGES);
    let mut buf = vec![0u32; (w * h) as usize];
    g.copy_to(&mut buf);
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    for (d, p) in pm.data_mut().chunks_mut(4).zip(buf) {
        d.copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, 255]);
    }
    pm
}

/// Custom Clean with the three parts at different stages.
fn custom_frame(dark: bool, w: u32, h: u32) -> tiny_skia::Pixmap {
    use rcare::custom::{CustomPage, Status};
    rcare::i18n::set_lang(rcare::i18n::Lang::EnUS);
    let mut g = Gfx::new().expect("fonts");
    g.resize(w, h);
    let th = Theme::new(dark, None);
    let s = 1.0;
    let mut rest = Rect::new(0.0, 0.0, w as f32, h as f32);
    let side = rest.take_left(sidebar::width(false, s));
    let l = sidebar::layout(side, PAGES.len(), s);
    let mut page = CustomPage::default();
    page.view.folder = "C:\\Users\\Miguel".into();
    page.view.can_analyze = true;
    page.view.status =
        [Status::Done { bytes: 4_812_000_000, count: 23_410 }, Status::Done { bytes: 1_250_000_000, count: 87 }, Status::Working];
    page.render(&mut g, rest, &th, s);
    page.pointer(page.layout.analyze.x + 4.0, page.layout.analyze.y + 4.0, false);
    page.render(&mut g, rest, &th, s);
    sidebar::draw(&mut g, &l, &th, s, State { collapsed: false, selected: 0, hot: None }, PAGES);
    let mut buf = vec![0u32; (w * h) as usize];
    g.copy_to(&mut buf);
    let mut pm = tiny_skia::Pixmap::new(w, h).unwrap();
    for (d, p) in pm.data_mut().chunks_mut(4).zip(buf) {
        d.copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, 255]);
    }
    pm
}

#[test]
#[ignore]
fn screenshots() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/screens");
    std::fs::create_dir_all(&dir).unwrap();
    frame(false, false, Some(Hit::Page(0)), 1180, 720).save_png(dir.join("open-light.png")).unwrap();
    frame(true, false, None, 1180, 720).save_png(dir.join("open-dark.png")).unwrap();
    frame(true, true, Some(Hit::Toggle), 1180, 720).save_png(dir.join("collapsed-dark.png")).unwrap();
    other_frame(2, false, 1180, 720).save_png(dir.join("dups-light.png")).unwrap();
    other_frame(3, true, 1180, 720).save_png(dir.join("size-dark.png")).unwrap();
    other_frame(4, false, 1180, 720).save_png(dir.join("inst-light.png")).unwrap();
    custom_frame(false, 1280, 720).save_png(dir.join("custom-light.png")).unwrap();
    custom_frame(true, 800, 720).save_png(dir.join("custom-dark-narrow.png")).unwrap();
    custom_frame(false, 760, 480).save_png(dir.join("custom-smallest.png")).unwrap();
}
