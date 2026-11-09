//! rpad's file types for the Explorer integration of `rt-sys`
//! (`rt_sys::win::assoc`): "Edit with rpad" on their context menu, and rpad
//! as their default editor when asked (only where the user chose none).

pub use rt_sys::win::assoc::{list, open_default_apps, Defaults, Error, Status};
use rt_sys::win::assoc::{Keys, Spec};

const SPEC: Spec = Spec {
    name: "rpad",
    description: "A portable text and code editor.",
    exts: &[
        (".txt", "rpad.txt"),
        (".log", "rpad.txt"),
        (".md", "rpad.md"),
        (".markdown", "rpad.md"),
        (".ini", "rpad.config"),
        (".cfg", "rpad.config"),
        (".conf", "rpad.config"),
        (".toml", "rpad.config"),
        (".yaml", "rpad.config"),
        (".yml", "rpad.config"),
        (".json", "rpad.config"),
        (".xml", "rpad.xml"),
        (".rs", "rpad.code"),
        (".py", "rpad.code"),
        (".js", "rpad.code"),
        (".ts", "rpad.code"),
        (".c", "rpad.code"),
        (".h", "rpad.code"),
        (".cpp", "rpad.code"),
        (".hpp", "rpad.code"),
        (".cs", "rpad.code"),
        (".java", "rpad.code"),
        (".go", "rpad.code"),
        (".sql", "rpad.code"),
        (".ps1", "rpad.code"),
        (".bat", "rpad.code"),
        (".cmd", "rpad.code"),
        (".sh", "rpad.code"),
        (".css", "rpad.code"),
    ],
    progids: &[
        ("rpad.txt", "Text Document"),
        ("rpad.md", "Markdown Document"),
        ("rpad.config", "Configuration File"),
        ("rpad.xml", "XML Document"),
        ("rpad.code", "Source Code"),
    ],
};

/// The extensions rpad registers for.
pub fn extensions() -> Vec<&'static str> {
    SPEC.exts.iter().map(|(e, _)| *e).collect()
}

pub fn status(exe: &str) -> Status {
    rt_sys::win::assoc::status(&SPEC, &Keys::user(SPEC.name), exe)
}

pub fn register(exe: &str, verb: &str) -> Result<(), Error> {
    rt_sys::win::assoc::register(&SPEC, &Keys::user(SPEC.name), exe, verb)
}

pub fn associate(exe: &str, verb: &str) -> Result<Defaults, Error> {
    rt_sys::win::assoc::associate(&SPEC, &Keys::user(SPEC.name), exe, verb)
}

pub fn unregister() -> Result<(), Error> {
    rt_sys::win::assoc::unregister(&SPEC, &Keys::user(SPEC.name))
}
