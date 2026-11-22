//! rpack's file types for the Explorer integration of `rt-sys`
//! (`rt_sys::win::assoc`): "Open with rpack" on their context menu, and
//! rpack as their default app when asked.

pub use rt_sys::win::assoc::{list, open_default_apps, Error, Status};
use rt_sys::win::assoc::{Keys, Spec};

static SPEC: Spec = Spec {
    name: "rpack",
    description: "A small, fast, portable archiver.",
    exts: &[
        (".zip", "rpack.zip"),
        (".7z", "rpack.7z"),
        (".tar", "rpack.tar"),
        (".gz", "rpack.compressed"),
        (".tgz", "rpack.compressed"),
        (".bz2", "rpack.compressed"),
        (".tbz2", "rpack.compressed"),
        (".xz", "rpack.compressed"),
        (".txz", "rpack.compressed"),
        (".lzma", "rpack.compressed"),
        (".lz", "rpack.compressed"),
        (".tlz", "rpack.compressed"),
        (".zst", "rpack.compressed"),
        (".tzst", "rpack.compressed"),
        (".lz4", "rpack.compressed"),
        (".br", "rpack.compressed"),
    ],
    progids: &[
        ("rpack.zip", "ZIP Archive"),
        ("rpack.7z", "7z Archive"),
        ("rpack.tar", "TAR Archive"),
        ("rpack.compressed", "Compressed File"),
    ],
};

/// rpack's file types, for the Default app panel (`rt_ui::assoc`).
pub fn spec() -> &'static Spec {
    &SPEC
}

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
