//! The icons of Linux: those the `.desktop` files name, found in the icon
//! themes, in PNG or SVG.

use crate::xdg::{self, IconDirs};
use std::path::Path;
use tiny_skia::Pixmap;

/// The icon of a `.desktop` file, or of a picture chosen for a program.
pub fn of(dirs: &IconDirs, path: &Path) -> Option<Pixmap> {
    let picture = if path.extension().is_some_and(|e| e == "desktop") {
        let text = std::fs::read_to_string(path).ok()?;
        dirs.find(xdg::icon_name(&text)?)?
    } else if xdg::is_picture(path) {
        path.to_path_buf()
    } else {
        return None;
    };
    let bytes = std::fs::read(&picture).ok()?;
    if picture.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")) {
        svg(&bytes)
    } else {
        Pixmap::decode_png(&bytes).ok()
    }
}

/// An SVG drawn at the size of the largest icons shown.
fn svg(bytes: &[u8]) -> Option<Pixmap> {
    use resvg::{tiny_skia as skia, usvg};
    const SIDE: u32 = 48;
    let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).ok()?;
    let mut drawn = skia::Pixmap::new(SIDE, SIDE)?;
    let size = tree.size();
    let scale = SIDE as f32 / size.width().max(size.height());
    resvg::render(&tree, skia::Transform::from_scale(scale, scale), &mut drawn.as_mut());
    // resvg draws with its own tiny-skia: the pixels are the same.
    let mut pm = Pixmap::new(SIDE, SIDE)?;
    pm.data_mut().copy_from_slice(drawn.data());
    Some(pm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_desktop_file_gives_its_svg_icon() {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target").join("rlaunch-tests").join("svg");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let picture = d.join("x.svg");
        std::fs::write(
            &picture,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#ff0000"/></svg>"##,
        )
        .unwrap();
        let desktop = d.join("x.desktop");
        std::fs::write(&desktop, format!("[Desktop Entry]\nIcon={}\n", picture.display())).unwrap();
        let pm = of(&IconDirs::default(), &desktop).unwrap();
        assert_eq!((pm.width(), pm.pixel(24, 24).unwrap().red()), (48, 255));
    }
}
