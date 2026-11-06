//! End-to-end workflow tests: the application logic driven through a mock
//! host (no window), with real files in a temporary folder.

use super::*;
use crate::app::input::{Key, KeyInput};
use std::cell::RefCell;
use std::collections::VecDeque;

#[derive(Default)]
struct MockHost {
    answers: RefCell<VecDeque<Answer>>,
    confirms: RefCell<VecDeque<bool>>,
    save_paths: RefCell<VecDeque<PathBuf>>,
    errors: RefCell<Vec<String>>,
    infos: RefCell<Vec<String>>,
    asked: RefCell<Vec<String>>,
    clipboard: RefCell<Option<String>>,
}

impl MockHost {
    fn answer(&self, a: Answer) -> &Self {
        self.answers.borrow_mut().push_back(a);
        self
    }
    fn confirm_next(&self, b: bool) -> &Self {
        self.confirms.borrow_mut().push_back(b);
        self
    }
    fn save_to(&self, p: &Path) -> &Self {
        self.save_paths.borrow_mut().push_back(p.to_path_buf());
        self
    }
}

impl Host for MockHost {
    fn ask_save(&self, name: &str) -> Answer {
        self.asked.borrow_mut().push(name.to_string());
        self.answers.borrow_mut().pop_front().expect("unexpected save prompt")
    }
    fn confirm(&self, title: &str, _text: &str) -> bool {
        self.asked.borrow_mut().push(title.to_string());
        self.confirms.borrow_mut().pop_front().expect("unexpected confirmation")
    }
    fn info(&self, _title: &str, text: &str) {
        self.infos.borrow_mut().push(text.to_string());
    }
    fn error(&self, _title: &str, text: &str) {
        self.errors.borrow_mut().push(text.to_string());
    }
    fn open_dialog(&self, _dir: Option<&Path>) -> Vec<PathBuf> {
        Vec::new()
    }
    fn save_dialog(&self, _name: &str, _dir: Option<&Path>) -> Option<PathBuf> {
        self.save_paths.borrow_mut().pop_front()
    }
    fn pick_folder(&self) -> Option<PathBuf> {
        None
    }
    fn choose_font(&self, _face: &str, _size: f32) -> Option<(String, f32)> {
        None
    }
    fn clipboard_get(&self) -> Option<String> {
        self.clipboard.borrow().clone()
    }
    fn clipboard_set(&self, text: &str) {
        *self.clipboard.borrow_mut() = Some(text.to_string());
    }
    fn reveal(&self, _path: &Path) {}
}

/// A fresh temporary folder for one test.
fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rpad-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn app_in(dir: &Path) -> App {
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let loc = Location { dir: Some(data), portable: true, error: None };
    App::new(loc, Settings::default())
}

fn text(app: &App) -> String {
    app.doc().buf.text.to_string()
}

/// Inserts text at the carets like typing (or a paste) would.
fn type_text(app: &mut App, s: &str, host: &dyn Host) {
    let _ = host;
    let s = s.to_string();
    app.edit(|b| b.insert_text(&s, false));
}

fn press(app: &mut App, key: Key, host: &dyn Host) {
    app.key(&KeyInput { key, ctrl: false, shift: false, alt: false, text: None }, host);
}

// ---------------------------------------------------------------- encodings and EOLs

#[test]
fn round_trips_preserve_bytes_for_each_encoding() {
    let dir = tmp("roundtrip");
    let h = MockHost::default();
    let mut utf16le = vec![0xFF, 0xFE];
    utf16le.extend("héllo\r\nwörld\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()));
    let mut utf16be = vec![0xFE, 0xFF];
    utf16be.extend("日本\nテキスト".encode_utf16().flat_map(|u| u.to_be_bytes()));
    let cases: Vec<(&str, Vec<u8>, Enc, Eol)> = vec![
        ("utf8.txt", "plain ascii\r\nsecond\r\n".as_bytes().to_vec(), Enc::Utf8, Eol::CrLf),
        ("utf8lf.txt", "ünïcödé\nline\n".as_bytes().to_vec(), Enc::Utf8, Eol::Lf),
        ("bom.txt", b"\xEF\xBB\xBFbom text\r\n".to_vec(), Enc::Utf8Bom, Eol::CrLf),
        ("le.txt", utf16le, Enc::Utf16Le, Eol::CrLf),
        ("be.txt", utf16be, Enc::Utf16Be, Eol::Lf),
        ("cr.txt", b"old\rmac\r".to_vec(), Enc::Utf8, Eol::Cr),
        ("1252.txt", b"caf\xE9 cr\xE8me\r\n".to_vec(), Enc::Legacy(encoding_rs::WINDOWS_1252), Eol::CrLf),
    ];
    for (name, bytes, enc, eol) in cases {
        let p = dir.join(name);
        std::fs::write(&p, &bytes).unwrap();
        let mut app = app_in(&dir);
        app.settings.local_history = false;
        assert!(app.open_path(&p, &h), "{name}");
        let d = app.doc();
        assert_eq!(d.enc, enc, "{name} encoding");
        assert_eq!(d.eol, eol, "{name} eol");
        assert!(!d.buf.text.to_string().contains('\r'), "{name}: text is stored with LF");
        // Edit and undo so the document is unchanged, then force a save.
        type_text(&mut app, "x", &h);
        app.dispatch(Command::Undo, &h);
        app.doc_mut().buf.mark_unsaved();
        assert!(app.save_doc(0, false, &h), "{name} save");
        assert_eq!(std::fs::read(&p).unwrap(), bytes, "{name}: bytes preserved");
    }
    assert!(h.errors.borrow().is_empty(), "{:?}", h.errors.borrow());
}

#[test]
fn eol_conversion_on_save() {
    let dir = tmp("eol");
    let h = MockHost::default();
    let p = dir.join("a.txt");
    std::fs::write(&p, "a\r\nb\r\nc").unwrap();
    let mut app = app_in(&dir);
    app.open_path(&p, &h);
    app.dispatch(Command::SetEol(1), &h);
    assert!(app.doc().is_modified(), "changing the EOL marks the document modified");
    assert!(app.save_doc(0, false, &h));
    assert_eq!(std::fs::read(&p).unwrap(), b"a\nb\nc");
    app.dispatch(Command::SetEol(0), &h);
    app.save_doc(0, false, &h);
    assert_eq!(std::fs::read(&p).unwrap(), b"a\r\nb\r\nc");
}

#[test]
fn mixed_eol_is_reported_and_normalized_to_majority() {
    let dir = tmp("mixed");
    let h = MockHost::default();
    let p = dir.join("m.txt");
    std::fs::write(&p, "a\r\nb\r\nc\nd").unwrap();
    let mut app = app_in(&dir);
    app.open_path(&p, &h);
    assert_eq!(app.doc().eol, Eol::CrLf);
    assert!(matches!(app.doc().notice, Some(Notice::MixedEol(_))), "mixed line endings are reported");
}

#[test]
fn unmappable_characters_refuse_to_save_and_keep_the_file() {
    let dir = tmp("unmappable");
    let h = MockHost::default();
    let p = dir.join("l.txt");
    std::fs::write(&p, b"caf\xE9\r\n").unwrap();
    let mut app = app_in(&dir);
    app.settings.local_history = false;
    app.open_path(&p, &h);
    app.doc_mut().buf.set_caret(0);
    type_text(&mut app, "日本", &h);
    assert!(!app.save_doc(0, false, &h));
    assert_eq!(h.errors.borrow().len(), 1);
    assert!(h.errors.borrow()[0].contains("line 1"), "{}", h.errors.borrow()[0]);
    assert_eq!(std::fs::read(&p).unwrap(), b"caf\xE9\r\n", "file untouched");
    assert!(app.doc().is_modified());
    // Switching to UTF-8 makes it savable.
    app.dispatch(Command::SetEncoding(0), &h);
    assert!(app.save_doc(0, false, &h));
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "日本café\r\n");
}

#[test]
fn changing_to_an_encoding_that_cannot_hold_the_text_is_refused() {
    let dir = tmp("setenc");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "€ and 日本", &h);
    let before = app.doc().enc;
    app.dispatch(Command::SetEncoding(4), &h); // Windows-1252
    assert_eq!(app.doc().enc, before);
    assert_eq!(h.errors.borrow().len(), 1);
}

#[test]
fn reopen_with_encoding() {
    let dir = tmp("reopen");
    let h = MockHost::default();
    let p = dir.join("k.txt");
    // "Привет" in KOI8-R; not valid UTF-8, so a legacy code page is guessed first.
    let (bytes, _, _) = encoding_rs::KOI8_R.encode("Привет");
    std::fs::write(&p, &bytes).unwrap();
    let mut app = app_in(&dir);
    app.open_path(&p, &h);
    let koi = Enc::all().iter().position(|e| *e == Enc::Legacy(encoding_rs::KOI8_R)).unwrap();
    app.dispatch(Command::ReopenWithEncoding(koi as u8), &h);
    assert_eq!(text(&app), "Привет");
    assert!(!app.doc().is_modified());
}

#[test]
fn binary_files_open_read_only() {
    let dir = tmp("binary");
    let h = MockHost::default();
    let p = dir.join("b.bin");
    std::fs::write(&p, b"MZ\x00\x01\x02binary\x00data").unwrap();
    let mut app = app_in(&dir);
    app.open_path(&p, &h);
    assert!(app.doc().view_only);
    type_text(&mut app, "x", &h);
    assert!(!app.doc().is_modified(), "edits are blocked");
    assert!(!app.save_doc(0, false, &h));
    assert_eq!(std::fs::read(&p).unwrap(), b"MZ\x00\x01\x02binary\x00data");
}

#[test]
fn missing_file_reports_an_error() {
    let dir = tmp("missing");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    assert!(!app.open_path(&dir.join("nope.txt"), &h));
    assert_eq!(h.errors.borrow().len(), 1);
    // A command-line argument for a new file opens an empty tab instead.
    assert!(app.open_arg(&dir.join("new.txt"), &h));
    assert!(app.doc().path.is_some() && app.doc().buf.is_empty());
    assert!(!dir.join("new.txt").exists(), "nothing is written before saving");
    type_text(&mut app, "hi", &h);
    assert!(app.save_doc(app.active, false, &h));
    assert_eq!(std::fs::read_to_string(dir.join("new.txt")).unwrap(), "hi");
}

// ---------------------------------------------------------------- tabs and indentation

#[test]
fn tab_key_inserts_spaces_or_tabs() {
    let dir = tmp("tabs");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    app.settings.use_tabs = false;
    app.settings.tab_width = 4;
    press(&mut app, Key::Tab, &h);
    assert_eq!(text(&app), "    ");
    press(&mut app, Key::Backspace, &h);
    assert_eq!(text(&app), "", "backspace removes a whole soft tab");
    app.settings.use_tabs = true;
    press(&mut app, Key::Tab, &h);
    type_text(&mut app, "x", &h);
    assert_eq!(text(&app), "\tx");
    // Auto-indent keeps the indentation on Enter.
    press(&mut app, Key::Enter, &h);
    type_text(&mut app, "y", &h);
    assert_eq!(text(&app), "\tx\n\ty");
}

#[test]
fn indent_conversion_commands() {
    let dir = tmp("indentconv");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    app.settings.tab_width = 4;
    type_text(&mut app, "\ta\n\t\tb", &h);
    app.dispatch(Command::IndentToSpaces, &h);
    assert_eq!(text(&app), "    a\n        b");
    app.dispatch(Command::IndentToTabs, &h);
    assert_eq!(text(&app), "\ta\n\t\tb");
}

// ---------------------------------------------------------------- prompts

#[test]
fn closing_a_modified_tab_prompts() {
    let dir = tmp("prompt");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "draft", &h);
    // Cancel keeps the tab.
    h.answer(Answer::Cancel);
    assert!(!app.close_doc(0, &h));
    assert_eq!(text(&app), "draft");
    // Yes with a cancelled Save As keeps it too.
    h.answer(Answer::Yes);
    assert!(!app.close_doc(0, &h));
    assert_eq!(app.docs.len(), 1);
    assert_eq!(text(&app), "draft");
    // Yes and a path saves and closes.
    let p = dir.join("draft.txt");
    h.answer(Answer::Yes).save_to(&p);
    assert!(app.close_doc(0, &h));
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "draft");
    // No discards.
    type_text(&mut app, "throwaway", &h);
    h.answer(Answer::No);
    assert!(app.close_doc(0, &h));
    assert!(text(&app).is_empty());
    assert_eq!(h.asked.borrow().len(), 4);
}

#[test]
fn unmodified_tabs_close_without_prompt() {
    let dir = tmp("noprompt");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    app.dispatch(Command::New, &h);
    assert_eq!(app.docs.len(), 2);
    app.dispatch(Command::CloseTab, &h);
    assert_eq!(app.docs.len(), 1);
    assert!(h.asked.borrow().is_empty());
}

#[test]
fn last_tab_close_behaviour() {
    let dir = tmp("lasttab");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    app.settings.last_tab_close = LastTabClose::KeepEmpty;
    app.dispatch(Command::CloseTab, &h);
    assert_eq!(app.docs.len(), 1);
    assert!(!app.close_window);
    app.settings.last_tab_close = LastTabClose::CloseWindow;
    app.dispatch(Command::CloseTab, &h);
    assert!(app.close_window, "closing the last tab closes the window");
}

#[test]
fn confirm_all_stops_on_cancel() {
    let dir = tmp("confirmall");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "one", &h);
    app.dispatch(Command::New, &h);
    type_text(&mut app, "two", &h);
    h.answer(Answer::No).answer(Answer::Cancel);
    assert!(!app.confirm_all(&h));
    assert_eq!(h.asked.borrow().len(), 2);
}

#[test]
fn reopen_closed_tab() {
    let dir = tmp("reopenclosed");
    let h = MockHost::default();
    let p = dir.join("r.txt");
    std::fs::write(&p, "hello").unwrap();
    let mut app = app_in(&dir);
    app.open_path(&p, &h);
    app.dispatch(Command::CloseTab, &h);
    assert!(app.doc().path.is_none());
    app.dispatch(Command::ReopenClosed, &h);
    assert_eq!(app.doc().path.as_deref(), Some(p.as_path()));
    assert_eq!(text(&app), "hello");
}

#[test]
fn save_over_externally_changed_file_asks() {
    let dir = tmp("external");
    let h = MockHost::default();
    let p = dir.join("e.txt");
    std::fs::write(&p, "original").unwrap();
    let mut app = app_in(&dir);
    app.settings.local_history = false;
    app.open_path(&p, &h);
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&p, "changed elsewhere, and longer").unwrap();
    app.check_external_changes();
    assert_eq!(app.doc().notice, Some(Notice::ChangedOnDisk));
    // Reload brings the new text (no local changes, so no prompt).
    app.reload(0, None, &h);
    assert_eq!(text(&app), "changed elsewhere, and longer");
    assert!(app.doc().notice.is_none());
    // Change again, then try to save local edits: declining keeps the disk file.
    app.doc_mut().buf.set_caret(0);
    type_text(&mut app, "mine ", &h);
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(&p, "third version").unwrap();
    h.confirm_next(false);
    assert!(!app.save_doc(0, false, &h));
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "third version");
    h.confirm_next(true);
    assert!(app.save_doc(0, false, &h));
    assert!(std::fs::read_to_string(&p).unwrap().starts_with("mine "));
}

#[test]
fn deleted_file_is_detected() {
    let dir = tmp("deleted");
    let h = MockHost::default();
    let p = dir.join("d.txt");
    std::fs::write(&p, "x").unwrap();
    let mut app = app_in(&dir);
    app.open_path(&p, &h);
    std::fs::remove_file(&p).unwrap();
    app.check_external_changes();
    assert_eq!(app.doc().notice, Some(Notice::Deleted));
}

// ---------------------------------------------------------------- search and replace

#[test]
fn find_and_replace_all_with_preview_and_undo() {
    let dir = tmp("replace");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "cat Cat cat\ncatalog", &h);
    app.find.visible = true;
    app.find.find.set("cat");
    app.find.word = true;
    app.find.invalidate();
    app.update_matches();
    assert_eq!(app.find.total, 3, "case-insensitive whole words");
    app.find.case = true;
    app.find.invalidate();
    app.update_matches();
    assert_eq!(app.find.total, 2);
    app.find.replace.set("dog");
    // Declining the preview changes nothing.
    h.confirm_next(false);
    app.replace_all(&h);
    assert_eq!(text(&app), "cat Cat cat\ncatalog");
    h.confirm_next(true);
    app.replace_all(&h);
    assert_eq!(text(&app), "dog Cat dog\ncatalog");
    app.dispatch(Command::Undo, &h);
    assert_eq!(text(&app), "cat Cat cat\ncatalog", "one undo step");
}

#[test]
fn regex_replace_with_groups() {
    let dir = tmp("regex");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "2024-01-31\n1999-12-01", &h);
    app.find.find.set(r"(\d+)-(\d+)-(\d+)");
    app.find.regex = true;
    app.find.replace.set("$3/$2/$1");
    h.confirm_next(true);
    app.replace_all(&h);
    assert_eq!(text(&app), "31/01/2024\n01/12/1999");
}

#[test]
fn invalid_regex_is_reported_not_applied() {
    let dir = tmp("badregex");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "abc", &h);
    app.find.visible = true;
    app.find.find.set("(unclosed");
    app.find.regex = true;
    app.find.invalidate();
    app.update_matches();
    assert!(app.find.error.is_some());
    app.find.replace.set("x");
    app.replace_all(&h);
    assert_eq!(text(&app), "abc");
    assert_eq!(h.errors.borrow().len(), 1);
}

#[test]
fn find_next_wraps_around() {
    let dir = tmp("findnext");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "one two one", &h);
    app.find.find.set("one");
    app.find.invalidate();
    app.find_step(true, &h);
    let s = app.doc().buf.primary_sel();
    assert_eq!((s.start(), s.end()), (0, 3), "wraps to the first match");
    app.find_step(true, &h);
    let s = app.doc().buf.primary_sel();
    assert_eq!((s.start(), s.end()), (8, 11));
}

// ---------------------------------------------------------------- recovery and session

#[test]
fn crash_recovery_round_trip() {
    let dir = tmp("recovery");
    let h = MockHost::default();
    let p = dir.join("file.txt");
    std::fs::write(&p, "on disk").unwrap();
    {
        let mut app = app_in(&dir);
        app.settings.recovery = true;
        type_text(&mut app, "unsaved note", &h);
        app.open_path(&p, &h);
        app.doc_mut().buf.set_caret(0);
        type_text(&mut app, "edited ", &h);
        app.write_recovery(true);
        // Simulates a crash: no save, no prompt.
    }
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "on disk", "recovery never touches the original");
    let mut app = app_in(&dir);
    app.settings.recovery = true;
    assert!(app.restore_session(&h));
    assert_eq!(app.docs.len(), 2);
    let untitled = app.docs.iter().find(|d| d.path.is_none()).unwrap();
    assert_eq!(untitled.buf.text.to_string(), "unsaved note");
    assert!(untitled.is_modified());
    let file = app.docs.iter().find(|d| d.path.is_some()).unwrap();
    assert_eq!(file.buf.text.to_string(), "edited on disk");
    assert!(file.is_modified());
}

#[test]
fn recovery_can_be_disabled() {
    let dir = tmp("norecovery");
    let h = MockHost::default();
    {
        let mut app = app_in(&dir);
        app.settings.recovery = false;
        app.settings.restore_session = false;
        type_text(&mut app, "secret", &h);
        app.write_recovery(true);
    }
    let rdir = dir.join("data").join(session::RECOVERY_DIR);
    let files = std::fs::read_dir(&rdir).map(|r| r.count()).unwrap_or(0);
    assert_eq!(files, 0, "no recovery files are written");
    let mut app = app_in(&dir);
    app.settings.recovery = false;
    app.settings.restore_session = false;
    assert!(!app.restore_session(&h));
}

#[test]
fn saving_removes_recovery_file() {
    let dir = tmp("recoveryclean");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    app.settings.recovery = true;
    type_text(&mut app, "draft", &h);
    app.write_recovery(true);
    let rdir = dir.join("data").join(session::RECOVERY_DIR);
    assert_eq!(std::fs::read_dir(&rdir).unwrap().count(), 1);
    h.save_to(&dir.join("x.txt"));
    assert!(app.save_doc(0, false, &h));
    assert_eq!(std::fs::read_dir(&rdir).unwrap().count(), 0);
}

#[test]
fn session_restores_saved_files_and_caret() {
    let dir = tmp("session");
    let h = MockHost::default();
    let p = dir.join("s.txt");
    std::fs::write(&p, "line1\nline2\nline3").unwrap();
    {
        let mut app = app_in(&dir);
        app.settings.restore_session = true;
        app.open_path(&p, &h);
        app.goto_line(2, None);
        app.write_recovery(true);
    }
    let mut app = app_in(&dir);
    app.settings.restore_session = true;
    assert!(app.restore_session(&h));
    assert_eq!(app.doc().path.as_deref(), Some(p.as_path()));
    assert_eq!(app.doc().buf.line_of(app.doc().buf.primary_sel().head), 2);
}

#[test]
fn local_history_snapshot_before_save() {
    let dir = tmp("history");
    let h = MockHost::default();
    let p = dir.join("h.txt");
    std::fs::write(&p, "version 1").unwrap();
    let mut app = app_in(&dir);
    app.settings.local_history = true;
    app.open_path(&p, &h);
    app.dispatch(Command::SelectAll, &h);
    type_text(&mut app, "version 2", &h);
    assert!(app.save_doc(0, false, &h));
    let snaps = session::list_snapshots(&app.location, &p);
    assert_eq!(snaps.len(), 1);
    assert_eq!(std::fs::read_to_string(&snaps[0].0).unwrap(), "version 1");
}

// ---------------------------------------------------------------- text tools and editing

#[test]
fn text_tools_are_undoable() {
    let dir = tmp("tools");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "pear\napple\npear\n  banana  ", &h);
    let original = text(&app);
    app.dispatch(Command::SelectAll, &h);
    app.dispatch(Command::SortAsc, &h);
    assert_eq!(text(&app), "  banana  \napple\npear\npear");
    app.dispatch(Command::Undo, &h);
    assert_eq!(text(&app), original);
    app.dispatch(Command::SelectAll, &h);
    app.dispatch(Command::Dedupe, &h);
    assert_eq!(text(&app), "pear\napple\n  banana  ");
    app.dispatch(Command::Undo, &h);
    app.dispatch(Command::SelectAll, &h);
    app.dispatch(Command::Upper, &h);
    assert_eq!(text(&app), "PEAR\nAPPLE\nPEAR\n  BANANA  ");
    app.dispatch(Command::Undo, &h);
    app.dispatch(Command::SelectAll, &h);
    app.dispatch(Command::TrimTrailing, &h);
    assert_eq!(text(&app), "pear\napple\npear\n  banana");
    app.dispatch(Command::Undo, &h);
    assert_eq!(text(&app), original);
}

#[test]
fn escaping_tools() {
    let dir = tmp("escape");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "<a href=\"x\">&</a>", &h);
    app.dispatch(Command::SelectAll, &h);
    app.dispatch(Command::XmlEscape, &h);
    assert_eq!(text(&app), "&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;");
    app.dispatch(Command::SelectAll, &h);
    app.dispatch(Command::XmlUnescape, &h);
    assert_eq!(text(&app), "<a href=\"x\">&</a>");
}

#[test]
fn line_operations() {
    let dir = tmp("lines");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "a\nb\nc", &h);
    app.goto_line(1, None);
    app.dispatch(Command::Duplicate, &h);
    assert_eq!(text(&app), "a\nb\nb\nc");
    app.dispatch(Command::DeleteLine, &h);
    assert_eq!(text(&app), "a\nb\nc");
    app.goto_line(0, None);
    app.dispatch(Command::MoveLineDown, &h);
    assert_eq!(text(&app), "b\na\nc");
    app.dispatch(Command::MoveLineUp, &h);
    assert_eq!(text(&app), "a\nb\nc");
    app.dispatch(Command::JoinLines, &h);
    assert_eq!(text(&app), "a b\nc");
}

#[test]
fn multi_caret_typing() {
    let dir = tmp("multicaret");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "x = 1\nx = 2\ny = 3", &h);
    app.doc_mut().buf.set_sels(vec![Sel::new(0, 1)], 0);
    app.dispatch(Command::AddNextOccurrence, &h);
    assert_eq!(app.doc().buf.sels.len(), 2);
    type_text(&mut app, "value", &h);
    assert_eq!(text(&app), "value = 1\nvalue = 2\ny = 3");
    app.dispatch(Command::Undo, &h);
    assert_eq!(text(&app), "x = 1\nx = 2\ny = 3");
}

#[test]
fn copy_cut_paste_through_host_clipboard() {
    let dir = tmp("clip");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "hello world", &h);
    app.doc_mut().buf.set_sels(vec![Sel::new(0, 5)], 0);
    app.dispatch(Command::Cut, &h);
    assert_eq!(h.clipboard.borrow().as_deref(), Some("hello"));
    assert_eq!(text(&app), " world");
    app.doc_mut().buf.set_caret(6);
    // Windows clipboard text uses CRLF; it is normalized on paste.
    *h.clipboard.borrow_mut() = Some("!\r\nnext".into());
    app.dispatch(Command::Paste, &h);
    assert_eq!(text(&app), " world!\nnext");
}

#[test]
fn xml_check_and_format() {
    let dir = tmp("xml");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "<a><b>text</b>\n<c/></x>", &h);
    app.dispatch(Command::XmlCheck, &h);
    let msg = h.errors.borrow().last().cloned().or_else(|| h.infos.borrow().last().cloned()).unwrap_or_default();
    assert!(msg.contains("line 2") || msg.contains("Line 2"), "error location reported: {msg}");
    let before = text(&app);
    app.dispatch(Command::XmlFormat, &h);
    assert_eq!(text(&app), before, "invalid XML is not reformatted");
    app.dispatch(Command::SelectAll, &h);
    type_text(&mut app, "<a><b>x</b><c/></a>", &h);
    app.settings.use_tabs = false;
    app.settings.tab_width = 2;
    h.confirm_next(true);
    app.dispatch(Command::XmlFormat, &h);
    assert_eq!(text(&app), "<a>\n  <b>x</b>\n  <c/>\n</a>");
}

// ---------------------------------------------------------------- large files

#[test]
fn large_file_opens_with_heavy_features_off() {
    let dir = tmp("large");
    let h = MockHost::default();
    let p = dir.join("big.log");
    let line = "2024-01-01 12:00:00 INFO request handled in 12ms id=abcdef0123456789\n";
    let n = (file::LARGE_FILE_SIZE as usize / line.len()) + 1000;
    std::fs::write(&p, line.repeat(n)).unwrap();
    let t = Instant::now();
    let mut app = app_in(&dir);
    assert!(app.open_path(&p, &h));
    let open_time = t.elapsed();
    assert!(app.doc().large);
    assert!(!app.doc().hl.enabled, "highlighting is off for large files");
    assert_eq!(app.doc().buf.line_count(), n + 1);
    // Editing at the end and undo work on the rope.
    app.doc_mut().buf.move_doc(true, false);
    type_text(&mut app, "tail", &h);
    assert!(app.doc().is_modified());
    app.dispatch(Command::Undo, &h);
    assert!(!app.doc().is_modified());
    eprintln!("opened {} MB in {open_time:?}", line.len() * n / (1 << 20));
}

// ---------------------------------------------------------------- palette

#[test]
fn palette_typing_filters_and_runs_commands() {
    let dir = tmp("palette");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "b\na", &h);
    app.dispatch(Command::SelectAll, &h);
    app.dispatch(Command::CommandPalette, &h);
    assert_eq!(app.focus, Focus::Palette);
    // Printable keys are not consumed by the key handler; they arrive as text.
    let k = KeyInput { key: Key::Char('s'), ctrl: false, shift: false, alt: false, text: Some("s".into()) };
    assert!(!app.key(&k, &h));
    app.text_input("ort asc", &h);
    let p = app.palette.as_ref().unwrap();
    assert!(!p.shown.is_empty());
    assert!(p.items[p.shown[0]].label.contains("Ascending"), "{}", p.items[p.shown[0]].label);
    press(&mut app, Key::Enter, &h);
    assert_eq!(text(&app), "a\nb");
    assert!(app.palette.is_none());
}

#[test]
fn goto_line_palette_moves_while_typing() {
    let dir = tmp("gotoline");
    let h = MockHost::default();
    let mut app = app_in(&dir);
    type_text(&mut app, "1\n2\n3\n4", &h);
    app.dispatch(Command::GotoLine, &h);
    app.text_input("3", &h);
    assert_eq!(app.doc().buf.line_of(app.doc().buf.primary_sel().head), 2);
}
