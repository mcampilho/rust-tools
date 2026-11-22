//! Embeds the application icon, version information and manifest into the
//! Windows executable (see `rt-build`). The icon is rendered from
//! `src/appicon.rs`, so there is no binary asset in the repository.

#[allow(dead_code)]
#[path = "src/appicon.rs"]
mod appicon;

fn main() {
    rt_build::windows_resources(&rt_build::Resources {
        name: "rcare",
        description: "rcare Windows maintenance",
        icon: appicon::render,
        icon_source: "src/appicon.rs",
        manifest: true,
    });
}
