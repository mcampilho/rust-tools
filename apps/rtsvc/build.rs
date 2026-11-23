//! Embeds the icon, version information and manifest into the Windows
//! executable (see `rt-build`): Explorer and the service's file properties
//! show its version, and the release workflow checks it. The icon is rendered
//! from `src/appicon.rs`, so there is no binary asset in the repository.

#[allow(dead_code)]
#[path = "src/appicon.rs"]
mod appicon;

fn main() {
    rt_build::windows_resources(&rt_build::Resources {
        name: "rtsvc",
        description: "rust-tools service",
        icon: appicon::render,
        icon_source: "src/appicon.rs",
        manifest: true,
    });
}
