//! rview's file types for the Explorer integration of `rt-sys`
//! (`rt_sys::win::assoc`): "Open with rview" on their context menu, and
//! rview as their default viewer when asked.

pub use rt_sys::win::assoc::{list, open_default_apps, Error, Status};
use rt_sys::win::assoc::{Keys, Spec};

const SPEC: Spec = Spec {
    name: "rview",
    description: "A small, fast, portable document viewer.",
    exts: &[
        (".pdf", "rview.pdf"),
        (".md", "rview.md"),
        (".markdown", "rview.md"),
        (".html", "rview.html"),
        (".htm", "rview.html"),
        (".txt", "rview.txt"),
        (".log", "rview.txt"),
        (".csv", "rview.csv"),
        (".tsv", "rview.csv"),
    ],
    progids: &[
        ("rview.pdf", "PDF Document"),
        ("rview.md", "Markdown Document"),
        ("rview.html", "HTML Document"),
        ("rview.txt", "Text Document"),
        ("rview.csv", "CSV Document"),
    ],
};

/// The supported extensions.
pub fn extensions() -> Vec<&'static str> {
    SPEC.exts.iter().map(|(e, _)| *e).collect()
}

pub fn status(exe: &str) -> Status {
    rt_sys::win::assoc::status(&SPEC, &Keys::user(SPEC.name), exe)
}

pub fn register(exe: &str, verb: &str) -> Result<(), Error> {
    rt_sys::win::assoc::register(&SPEC, &Keys::user(SPEC.name), exe, verb)
}

pub fn associate(exe: &str, verb: &str) -> Result<rt_sys::win::assoc::Defaults, Error> {
    rt_sys::win::assoc::associate(&SPEC, &Keys::user(SPEC.name), exe, verb)
}

pub fn unregister() -> Result<(), Error> {
    rt_sys::win::assoc::unregister(&SPEC, &Keys::user(SPEC.name))
}
