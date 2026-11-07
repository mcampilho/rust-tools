//! Embeds the application icon and version information into the Windows
//! executable (see `rt-build`). The icon is rendered from
//! `src/ui/appicon.rs`, so there is no binary asset in the repository.

#[allow(dead_code)]
#[path = "src/ui/appicon.rs"]
mod appicon;

fn main() {
    rt_build::windows_resources(&rt_build::Resources {
        name: "rcalc",
        description: "rcalc Calculator",
        icon: appicon::render,
        icon_source: "src/ui/appicon.rs",
        manifest: false,
    });
}
