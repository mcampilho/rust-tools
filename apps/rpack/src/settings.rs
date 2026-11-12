//! User preferences in `settings.ini`. Missing or invalid values fall back
//! to defaults.

use crate::create::{Kind, Level, Spec, ZipMethod};
use crate::extract::Overwrite;
use crate::i18n::Lang;
use crate::storage::Store;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 0 follows the system, 1 light, 2 dark.
    pub theme: u8,
    /// Interface language; None follows the system.
    pub language: Option<Lang>,
    /// Window size (logical pixels) and maximized state.
    pub window: Option<(f64, f64, bool)>,
    pub overwrite: Overwrite,
    pub open_after: bool,
    /// The folder shown last (on disk), where the next start opens.
    pub folder: Option<PathBuf>,
    /// How archives are made, as last chosen.
    pub spec: Spec,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { theme: 0, language: None, window: None, overwrite: Overwrite::default(), open_after: true, folder: None, spec: Spec::default() }
    }
}


impl Settings {
    pub fn load(s: &Store) -> Settings {
        Settings {
            theme: match s.get("theme") {
                Some("light") => 1,
                Some("dark") => 2,
                _ => 0,
            },
            language: s.get("language").and_then(Lang::from_tag),
            window: s.get("window").and_then(|v| {
                let mut it = v.split(',');
                let w: f64 = it.next()?.trim().parse().ok()?;
                let h: f64 = it.next()?.trim().parse().ok()?;
                let max = it.next() == Some("max");
                (w >= 200.0 && h >= 150.0 && w < 20000.0 && h < 20000.0).then_some((w, h, max))
            }),
            overwrite: s.get("if_exists").and_then(Overwrite::from_key).unwrap_or_default(),
            open_after: s.get("open_after") != Some("no"),
            folder: s.get("folder").filter(|f| !f.trim().is_empty()).map(PathBuf::from),
            spec: Spec {
                kind: s.get("new_kind").and_then(Kind::from_key).filter(|k| !k.needs_one_file()).unwrap_or(Kind::SevenZ),
                level: s.get("new_level").and_then(|v| v.parse::<usize>().ok()).and_then(|i| Level::ALL.get(i).copied()).unwrap_or_default(),
                zip_method: s.get("zip_method").and_then(|v| ZipMethod::ALL.into_iter().find(|m| m.label() == v)).unwrap_or_default(),
                solid: s.get("solid") != Some("no"),
            },
        }
    }

    pub fn to_store(&self) -> Store {
        let mut s = Store::default();
        s.set("theme", ["system", "light", "dark"][self.theme.min(2) as usize]);
        s.set("language", self.language.map_or("", |l| l.tag()));
        if let Some((w, h, max)) = self.window {
            s.set("window", format!("{w:.0},{h:.0},{}", if max { "max" } else { "normal" }));
        }
        s.set("if_exists", self.overwrite.key());
        s.set("open_after", if self.open_after { "yes" } else { "no" });
        s.set("folder", self.folder.as_ref().map_or(String::new(), |f| f.display().to_string()));
        s.set("new_kind", self.spec.kind.ext());
        s.set("new_level", Level::ALL.iter().position(|l| *l == self.spec.level).unwrap_or(3).to_string());
        s.set("zip_method", self.spec.zip_method.label());
        s.set("solid", if self.spec.solid { "yes" } else { "no" });
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_bad_values() {
        let s = Settings {
            theme: 2,
            language: Some(Lang::PtPT),
            window: Some((900.0, 700.0, true)),
            overwrite: Overwrite::Skip,
            open_after: false,
            folder: Some(PathBuf::from("\\\\nas\\cópias de família")),
            spec: Spec { kind: Kind::Tar(Some(crate::archive::Codec::Zstd)), level: Level::Ultra, zip_method: ZipMethod::Xz, solid: false },
        };
        assert_eq!(Settings::load(&Store::parse(&s.to_store().serialize("rpack"))), s);
        let bad = Settings::load(&Store::parse("theme=??\nwindow=abc\nlanguage=xx-YY\nfolder=\nif_exists=maybe\n"));
        assert_eq!(bad, Settings::default());
    }
}
