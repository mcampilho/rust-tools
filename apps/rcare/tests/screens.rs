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
    sidebar::draw(&mut g, &l, &th, s, State { collapsed, selected: 0, hot }, PAGES);
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
}
