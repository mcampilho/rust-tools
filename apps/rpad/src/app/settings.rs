//! User preferences, stored as `key=value` lines in `settings.ini`.
//! Missing or invalid values fall back to defaults.

use crate::io::encoding::{Enc, Eol};
use crate::storage::Store;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThemePref {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LastTabClose {
    /// Keep the window open with a new empty document.
    KeepEmpty,
    /// Close the window.
    CloseWindow,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub theme: ThemePref,
    /// Interface language; None follows Windows.
    pub language: Option<crate::i18n::Lang>,
    pub font_face: String,
    pub font_size: f32,
    pub zoom: i32,
    pub word_wrap: bool,
    pub line_numbers: bool,
    pub highlight_line: bool,
    pub show_whitespace: bool,
    pub show_eol: bool,
    pub show_ruler: bool,
    pub ruler_col: usize,
    pub fold_margin: bool,
    pub toolbar: bool,
    pub status_bar: bool,
    pub tab_width: usize,
    pub use_tabs: bool,
    pub auto_indent: bool,
    pub new_eol: Eol,
    pub new_enc: Enc,
    pub last_tab_close: LastTabClose,
    pub restore_session: bool,
    pub recovery: bool,
    pub autosave_on_blur: bool,
    pub local_history: bool,
    pub history_keep: usize,
    pub single_instance: bool,
    pub recent: Vec<PathBuf>,
    pub find_history: Vec<String>,
    pub replace_history: Vec<String>,
    pub fif_dir: String,
    pub fif_filter: String,
    pub outline: bool,
    pub markdown_preview: bool,
    pub window: Option<(f64, f64, bool)>,
}

pub const MAX_RECENT: usize = 15;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: ThemePref::System,
            language: None,
            font_face: String::new(),
            font_size: 11.0,
            zoom: 100,
            word_wrap: false,
            line_numbers: true,
            highlight_line: true,
            show_whitespace: false,
            show_eol: false,
            show_ruler: false,
            ruler_col: 80,
            fold_margin: true,
            toolbar: false,
            status_bar: true,
            tab_width: 4,
            use_tabs: false,
            auto_indent: true,
            new_eol: Eol::platform(),
            new_enc: Enc::Utf8,
            last_tab_close: LastTabClose::KeepEmpty,
            restore_session: true,
            recovery: true,
            autosave_on_blur: false,
            local_history: false,
            history_keep: 10,
            single_instance: true,
            recent: Vec::new(),
            find_history: Vec::new(),
            replace_history: Vec::new(),
            fif_dir: String::new(),
            fif_filter: String::new(),
            outline: false,
            markdown_preview: false,
            window: None,
        }
    }
}

fn b(v: Option<&str>, d: bool) -> bool {
    match v {
        Some("1") | Some("true") => true,
        Some("0") | Some("false") => false,
        _ => d,
    }
}

fn n<T: std::str::FromStr>(v: Option<&str>, d: T) -> T {
    v.and_then(|s| s.parse().ok()).unwrap_or(d)
}

impl Settings {
    pub fn load(s: &Store) -> Settings {
        let d = Settings::default();
        Settings {
            theme: match s.get("theme") {
                Some("light") => ThemePref::Light,
                Some("dark") => ThemePref::Dark,
                _ => ThemePref::System,
            },
            language: s.get("language").and_then(crate::i18n::Lang::from_tag),
            font_face: s.get("font_face").unwrap_or("").to_string(),
            font_size: n(s.get("font_size"), d.font_size).clamp(6.0, 72.0),
            zoom: n(s.get("zoom"), d.zoom).clamp(30, 400),
            word_wrap: b(s.get("word_wrap"), d.word_wrap),
            line_numbers: b(s.get("line_numbers"), d.line_numbers),
            highlight_line: b(s.get("highlight_line"), d.highlight_line),
            show_whitespace: b(s.get("show_whitespace"), d.show_whitespace),
            show_eol: b(s.get("show_eol"), d.show_eol),
            show_ruler: b(s.get("show_ruler"), d.show_ruler),
            ruler_col: n(s.get("ruler_col"), d.ruler_col).clamp(1, 1000),
            fold_margin: b(s.get("fold_margin"), d.fold_margin),
            toolbar: b(s.get("toolbar"), d.toolbar),
            status_bar: b(s.get("status_bar"), d.status_bar),
            tab_width: n(s.get("tab_width"), d.tab_width).clamp(1, 16),
            use_tabs: b(s.get("use_tabs"), d.use_tabs),
            auto_indent: b(s.get("auto_indent"), d.auto_indent),
            new_eol: s.get("new_eol").and_then(Eol::from_name).unwrap_or(d.new_eol),
            new_enc: s.get("new_enc").and_then(Enc::from_id).unwrap_or(d.new_enc),
            last_tab_close: if s.get("last_tab_close") == Some("close") { LastTabClose::CloseWindow } else { LastTabClose::KeepEmpty },
            restore_session: b(s.get("restore_session"), d.restore_session),
            recovery: b(s.get("recovery"), d.recovery),
            autosave_on_blur: b(s.get("autosave_on_blur"), d.autosave_on_blur),
            local_history: b(s.get("local_history"), d.local_history),
            history_keep: n(s.get("history_keep"), d.history_keep).clamp(1, 100),
            single_instance: b(s.get("single_instance"), d.single_instance),
            recent: s.get_all("recent").map(PathBuf::from).take(MAX_RECENT).collect(),
            find_history: s.get_all("find").map(str::to_string).take(20).collect(),
            replace_history: s.get_all("replace").map(str::to_string).take(20).collect(),
            fif_dir: s.get("fif_dir").unwrap_or("").to_string(),
            fif_filter: s.get("fif_filter").unwrap_or("").to_string(),
            outline: b(s.get("outline"), d.outline),
            markdown_preview: b(s.get("markdown_preview"), d.markdown_preview),
            window: (|| {
                let mut it = s.get("window")?.split(',');
                let w = it.next()?.parse().ok()?;
                let h = it.next()?.parse().ok()?;
                let max = it.next() == Some("max");
                Some((w, h, max))
            })(),
        }
    }

    pub fn to_store(&self) -> Store {
        let mut s = Store::default();
        let bv = |v: bool| if v { "1" } else { "0" };
        s.set("theme", match self.theme {
            ThemePref::System => "system",
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
        });
        s.set("language", self.language.map_or("", |l| l.tag()));
        s.set("font_face", self.font_face.clone());
        s.set("font_size", format!("{}", self.font_size));
        s.set("zoom", self.zoom.to_string());
        s.set("word_wrap", bv(self.word_wrap));
        s.set("line_numbers", bv(self.line_numbers));
        s.set("highlight_line", bv(self.highlight_line));
        s.set("show_whitespace", bv(self.show_whitespace));
        s.set("show_eol", bv(self.show_eol));
        s.set("show_ruler", bv(self.show_ruler));
        s.set("ruler_col", self.ruler_col.to_string());
        s.set("fold_margin", bv(self.fold_margin));
        s.set("toolbar", bv(self.toolbar));
        s.set("status_bar", bv(self.status_bar));
        s.set("tab_width", self.tab_width.to_string());
        s.set("use_tabs", bv(self.use_tabs));
        s.set("auto_indent", bv(self.auto_indent));
        s.set("new_eol", self.new_eol.name());
        s.set("new_enc", self.new_enc.id());
        s.set("last_tab_close", if self.last_tab_close == LastTabClose::CloseWindow { "close" } else { "keep" });
        s.set("restore_session", bv(self.restore_session));
        s.set("recovery", bv(self.recovery));
        s.set("autosave_on_blur", bv(self.autosave_on_blur));
        s.set("local_history", bv(self.local_history));
        s.set("history_keep", self.history_keep.to_string());
        s.set("single_instance", bv(self.single_instance));
        s.set("outline", bv(self.outline));
        s.set("markdown_preview", bv(self.markdown_preview));
        s.set("fif_dir", self.fif_dir.clone());
        s.set("fif_filter", self.fif_filter.clone());
        if let Some((w, h, max)) = self.window {
            s.set("window", format!("{w:.0},{h:.0},{}", if max { "max" } else { "normal" }));
        }
        for r in &self.recent {
            s.push("recent", r.display().to_string());
        }
        for f in &self.find_history {
            s.push("find", f.clone());
        }
        for f in &self.replace_history {
            s.push("replace", f.clone());
        }
        s
    }

    pub fn add_recent(&mut self, p: &std::path::Path) {
        self.recent.retain(|r| r != p);
        self.recent.insert(0, p.to_path_buf());
        self.recent.truncate(MAX_RECENT);
    }

    pub fn push_history(list: &mut Vec<String>, item: &str) {
        if item.is_empty() {
            return;
        }
        list.retain(|x| x != item);
        list.insert(0, item.to_string());
        list.truncate(20);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_corrupt_values() {
        let mut s = Settings { word_wrap: true, tab_width: 2, theme: ThemePref::Dark, new_enc: Enc::Utf16Le, ..Settings::default() };
        s.add_recent(std::path::Path::new("C:\\a.txt"));
        s.add_recent(std::path::Path::new("C:\\b.txt"));
        s.add_recent(std::path::Path::new("C:\\a.txt"));
        Settings::push_history(&mut s.find_history, "needle");
        s.window = Some((800.0, 600.0, true));
        let back = Settings::load(&Store::parse(&s.to_store().serialize()));
        assert_eq!(back, s);
        assert_eq!(back.recent[0], PathBuf::from("C:\\a.txt"));
        let bad = Settings::load(&Store::parse("tab_width=abc\nzoom=99999\ntheme=???\nnew_enc=bogus\nfont_size=-3\n"));
        assert_eq!(bad.tab_width, 4);
        assert_eq!(bad.zoom, 400);
        assert_eq!(bad.theme, ThemePref::System);
        assert_eq!(bad.new_enc, Enc::Utf8);
        assert_eq!(bad.font_size, 6.0);
    }
}
