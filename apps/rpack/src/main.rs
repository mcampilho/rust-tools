//! rpack: a portable archiver.
//!
//! The window is driven by winit and drawn on the CPU (tiny-skia + ab_glyph)
//! into a softbuffer surface. It browses the drives and folders, and the
//! archives in them. Opening, extracting, testing and hashing run on
//! background threads; the window reads how far they got from their
//! `Meter` and is told when they end through the event loop.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rpack::app::{AddForm, App, Busy, Crumb, Place, Row, Task};
use rpack::create::{self, Kind as NewKind, Made, Spec};
use rpack::archive::{self, Error, Listing, Meter};
use rpack::commands::{self, Command, ALL_HASHES};
use rpack::extract::{self, Each, EachDest, Job, Overwrite, Report};
use rpack::format;
use rpack::hash::{self, Algo, Sums};
use rpack::i18n::{self, tf, tr};
use rpack::settings::Settings;
use rpack::storage;
use rpack::ui::gfx::{Color, Gfx, Rect};
use rpack::ui::theme::Theme;
use rpack::ui::view::{self, Hot, Layout, Page};
use rt_scan::volumes::{self, Volume};
use rt_ui::Message;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Theme as WinTheme, Window, WindowId};

mod input;
mod menus;
mod render;

enum UserEvent {
    A11y(accesskit_winit::Event),
    /// The drives, and the one to select (coming up from it).
    Drives(Vec<Volume>, Option<PathBuf>),
    /// An archive listed (or not), for the work of this generation.
    Opened(u64, PathBuf, Result<Listing, Error>),
    /// Extracting ended: how it went, where to, and the file to open after
    /// (a file opened from the list).
    Extracted(u64, Result<Report, Error>, PathBuf, Option<PathBuf>),
    Tested(u64, Result<Report, Error>),
    /// Several archives extracted (or tested) from the browser.
    EachDone(u64, Task, Vec<Each>),
    Hashed(u64, Vec<Algo>, Result<Vec<Sums>, Error>),
    /// Archives made: each with where it went and how it went.
    Created(u64, Vec<(PathBuf, Result<Made, Error>)>),
    /// The work of this generation ended in a panic (a damaged archive that
    /// a decoder did not expect).
    Failed(u64, String),
    #[cfg(windows)]
    Win(rt_sys::win::WinMsg),
}

impl From<accesskit_winit::Event> for UserEvent {
    fn from(e: accesskit_winit::Event) -> Self {
        UserEvent::A11y(e)
    }
}

#[cfg(windows)]
const IDENT: rt_sys::win::Ident = rt_sys::win::Ident { name: "rpack", copydata: 0x5250_414B }; // "RPAK"

struct Win {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    adapter: accesskit_winit::Adapter,
    #[cfg(windows)]
    hwnd: rt_sys::win::HWND,
    #[cfg(windows)]
    menu: Option<rt_sys::win::menu::MenuBar<Command>>,
}

struct Shell {
    app: App,
    settings: Settings,
    location: storage::Location,
    gfx: Gfx,
    win: Option<Win>,
    proxy: EventLoopProxy<UserEvent>,
    mods: ModifiersState,
    cursor: Option<(f32, f32)>,
    system_dark: bool,
    accent: Option<Color>,
    menu_dark: Option<bool>,
    menu_dirty: bool,
    menu_rebuild: bool,
    layout: Layout,
    hot: Hot,
    /// Dragging the scroll bar's thumb.
    dragging: bool,
    /// The last click on a row, to see a double click.
    last_click: Option<(Instant, usize)>,
    cursor_icon: CursorIcon,
    /// The work running is of this generation; events of older ones are ignored.
    generation: u64,
    /// Files opened from the list are extracted into numbered folders here.
    temp_count: u32,
    /// An in-app message (About, Keyboard shortcuts, a report), and the text
    /// Ctrl+C copies from it when it is not the whole message.
    dialog: Option<Message>,
    dialog_copy: Option<String>,
    /// The last hashes calculated, for Save Hashes.
    last_sums: Option<(Vec<Algo>, Vec<Sums>)>,
    #[cfg(not(windows))]
    menu: rpack::ui::menu::Menu,
}

/// The folder for files opened from the list, emptied when rpack ends.
fn temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("rpack-{}", std::process::id()))
}

/// What went wrong, as shown.
fn error_text(e: &Error) -> String {
    match e {
        Error::Io(why) => tf("Could not read or write: {}", &[why]),
        Error::Unknown => tr("This is not an archive that rpack can open."),
        Error::Corrupt(why) => tf("The archive is damaged: {}", &[why]),
        Error::Unsupported(what) => tf("Not supported: {}", &[what]),
        Error::Password => tr("Encrypted: a password is needed, which rpack does not ask for yet."),
        Error::Stopped => tr("Stopped."),
    }
}

/// The errors of a report, one per line (the first ones).
fn errors_text(r: &Report) -> String {
    const SHOWN: usize = 15;
    let mut lines: Vec<String> = r
        .errors
        .iter()
        .take(SHOWN)
        .map(|(p, e)| if p.is_empty() { error_text(e) } else { format!("{p}: {}", error_text(e)) })
        .collect();
    if r.errors.len() > SHOWN {
        lines.push(tf("…and {} more.", &[&format::count((r.errors.len() - SHOWN) as u64)]));
    }
    lines.join("\n")
}

fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// What happened to extracted files already there, and links left out.
fn extract_notes(r: &Report) -> String {
    let mut note = String::new();
    let mut add = |s: String| {
        note.push(' ');
        note.push_str(&s);
    };
    if r.skipped > 0 {
        add(tf("{} were already there and were left as they were.", &[&format::count(r.skipped)]));
    }
    if r.renamed > 0 {
        add(tf("{} were already there: the new ones have a number added.", &[&format::count(r.renamed)]));
    }
    if r.left_out > 0 {
        add(tf("{} links were left out.", &[&format::count(r.left_out)]));
    }
    note
}

impl Shell {
    fn scale(&self) -> f32 {
        self.win.as_ref().map_or(1.0, |w| w.window.scale_factor() as f32)
    }

    fn dark(&self) -> bool {
        match self.settings.theme {
            1 => false,
            2 => true,
            _ => self.system_dark,
        }
    }

    fn redraw(&self) {
        if let Some(w) = &self.win {
            w.window.request_redraw();
        }
    }

    fn apply_language(&mut self) {
        i18n::set_lang(self.settings.language.unwrap_or_else(i18n::system_lang));
        self.gfx.clear_cache();
        self.menu_rebuild = true;
        self.menu_dirty = true;
    }

    fn save_settings(&mut self) {
        if let Some(w) = &self.win {
            rt_ui::window::remember_size(&w.window, &mut self.settings.window);
        }
        self.settings.overwrite = self.app.overwrite;
        self.settings.open_after = self.app.open_after;
        self.settings.spec = self.app.spec.clone();
        if let Some(f) = self.app.disk_folder() {
            self.settings.folder = Some(f);
        }
        let store = self.settings.to_store();
        self.location.save(&store);
    }

    // ------------------------------------------------------------ browsing

    /// Lists the drives on a thread (a sleeping or network drive can take
    /// seconds); `select` is the drive to select.
    fn load_drives(&self, select: Option<PathBuf>) {
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let _ = proxy.send_event(UserEvent::Drives(volumes::list(), select));
        });
    }

    /// Goes to a place of the path; `select` is the row to select there.
    fn go(&mut self, to: Crumb, select: Option<PathBuf>) {
        match to {
            Crumb::Drives => self.load_drives(select),
            Crumb::Folder(dir) => self.show_folder(&dir, select.as_deref()),
            Crumb::Node(n) => self.app.enter(n),
        }
        self.app.note = None;
        self.menu_dirty = true;
        self.redraw();
    }

    fn show_folder(&mut self, dir: &Path, select: Option<&Path>) {
        if let Err(why) = self.app.show_folder(dir, select) {
            self.app.note = Some(tf("Could not open {}: {}", &[&dir.display().to_string(), &why]));
        }
        self.redraw();
    }

    fn go_up(&mut self) {
        if let Some((to, select)) = self.app.up_target() {
            self.go(to, select);
        }
    }

    /// Opens the row at the cursor: a drive or folder is shown, an archive
    /// opened, any other file opened with its program.
    fn open_row(&mut self) {
        match self.app.cursor.and_then(|c| self.app.rows.get(c).copied()) {
            Some(Row::Up) => self.go_up(),
            Some(Row::Dir(n)) => self.app.enter(n),
            Some(Row::File(e)) => self.open_entry(e),
            Some(Row::Disk(i)) => {
                let d = self.app.disk[i].clone();
                if d.dir {
                    self.show_folder(&d.path, None);
                } else if is_archive(&d.path) {
                    self.open_archive(d.path);
                } else {
                    open_path(&d.path);
                }
            }
            None => {}
        }
    }

    // ------------------------------------------------------------ work

    /// Starts work on a background thread with a meter of its own.
    #[allow(clippy::too_many_arguments)]
    fn start(&mut self, task: Task, path: PathBuf, count: usize, total: u64, by_read: bool, work: impl FnOnce(&Meter, u64, EventLoopProxy<UserEvent>) + Send + 'static) {
        self.generation += 1;
        let gen = self.generation;
        let meter = Arc::new(Meter::default());
        self.app.busy = Some(Busy { task, meter: meter.clone(), started: Instant::now(), total, by_read, path, count });
        self.app.note = None;
        self.dragging = false;
        let proxy = self.proxy.clone();
        let lang = i18n::lang();
        std::thread::spawn(move || {
            i18n::set_lang(lang);
            let p = proxy.clone();
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&meter, gen, p)));
            if let Err(why) = run {
                let why = why.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| why.downcast_ref::<String>().cloned()).unwrap_or_default();
                let _ = proxy.send_event(UserEvent::Failed(gen, why));
            }
        });
        self.menu_dirty = true;
        self.redraw();
    }

    /// Ends the work of `gen`; false when it is not the one running.
    fn finish(&mut self, gen: u64) -> bool {
        if gen != self.generation {
            return false;
        }
        self.app.busy = None;
        self.menu_dirty = true;
        self.redraw();
        true
    }

    fn open_archive(&mut self, path: PathBuf) {
        if self.app.busy.is_some() {
            return;
        }
        self.dialog = None;
        let size = std::fs::metadata(&path).map_or(0, |m| m.len());
        let p = path.clone();
        self.start(Task::Opening, path, 1, size, true, move |meter, gen, proxy| {
            let result = archive::open(&p, meter);
            let _ = proxy.send_event(UserEvent::Opened(gen, p, result));
        });
    }

    fn opened(&mut self, gen: u64, path: PathBuf, result: Result<Listing, Error>) {
        if !self.finish(gen) {
            return;
        }
        match result {
            Ok(listing) => {
                // The folder on disk stays behind it: "up" from its root goes back there.
                if let Some(dir) = path.parent() {
                    if self.app.disk_folder().as_deref() != Some(dir) {
                        let _ = self.app.show_folder(dir, Some(&path));
                    }
                }
                self.app.set_opened(path, listing);
                self.save_settings();
            }
            Err(Error::Stopped) => {}
            Err(e) => self.app.note = Some(format!("{}: {}", file_name(&path), error_text(&e))),
        }
    }

    fn extract(&mut self, job: Job, open: Option<PathBuf>) {
        let Some(o) = &self.app.opened else { return };
        if self.app.busy.is_some() {
            return;
        }
        let (path, listing) = (o.path.clone(), o.listing.clone());
        let total = self.app.job_size(job.wanted.as_deref());
        let dest = job.dest.clone();
        self.start(Task::Extracting, path.clone(), 1, total, false, move |meter, gen, proxy| {
            let result = extract::extract(&path, &listing, &job, meter);
            let _ = proxy.send_event(UserEvent::Extracted(gen, result, dest, open));
        });
    }

    fn extracted(&mut self, gen: u64, result: Result<Report, Error>, dest: PathBuf, open: Option<PathBuf>) {
        if !self.finish(gen) {
            return;
        }
        let r = match result {
            Ok(r) => r,
            Err(Error::Stopped) => {
                self.app.note = Some(tr("Extracting was stopped."));
                return;
            }
            Err(e) => {
                self.info(&tr("Extract"), &error_text(&e));
                return;
            }
        };
        // A file opened from the list.
        if let Some(file) = open {
            match r.errors.first() {
                None if file.exists() => open_path(&file),
                _ => self.info(&tr("Open"), &errors_text(&r)),
            }
            return;
        }
        let note = tf("{} files extracted ({}) to {}.", &[&format::count(r.files), &format::size(r.bytes), &dest.display().to_string()]) + &extract_notes(&r);
        self.app.note = Some(note.clone());
        if !r.errors.is_empty() {
            let text = format!("{}\n\n{}\n\n{}", tf("{} files could not be extracted:", &[&format::count(r.errors.len() as u64)]), errors_text(&r), note);
            self.info(&tr("Extract"), &text);
        } else if self.app.open_after && r.files + r.dirs > 0 {
            open_path(&dest);
        }
    }

    fn test(&mut self) {
        let Some(o) = &self.app.opened else { return };
        if self.app.busy.is_some() {
            return;
        }
        let (path, listing) = (o.path.clone(), o.listing.clone());
        let total = listing.size();
        self.start(Task::Testing, path.clone(), 1, total, false, move |meter, gen, proxy| {
            let result = extract::test(&path, &listing, meter);
            let _ = proxy.send_event(UserEvent::Tested(gen, result));
        });
    }

    fn tested(&mut self, gen: u64, result: Result<Report, Error>) {
        if !self.finish(gen) {
            return;
        }
        let text = match result {
            Ok(r) if r.errors.is_empty() => tf("No errors were found.\n\n{} files, {}.", &[&format::count(r.files), &format::size(r.bytes)]),
            Ok(r) => format!(
                "{}\n\n{}\n\n{}",
                tf("{} errors were found.", &[&format::count(r.errors.len() as u64)]),
                errors_text(&r),
                tf("{} files are fine.", &[&format::count(r.files)])
            ),
            Err(Error::Stopped) => {
                self.app.note = Some(tr("The test was stopped."));
                return;
            }
            Err(e) => error_text(&e),
        };
        self.info(&tr("Test"), &text);
    }

    /// Extracts (or tests) the archives selected in a folder, each on its own.
    fn each(&mut self, task: Task, to: EachDest) {
        let paths = self.app.chosen_archives();
        if paths.is_empty() || self.app.busy.is_some() {
            return;
        }
        let total = paths.iter().map(|p| std::fs::metadata(p).map_or(0, |m| m.len())).sum();
        let overwrite = self.app.overwrite;
        let first = paths[0].clone();
        let count = paths.len();
        self.start(task, first, count, total, true, move |meter, gen, proxy| {
            let results = if task == Task::Testing { extract::test_each(&paths, meter) } else { extract::extract_each(&paths, &to, overwrite, meter) };
            let _ = proxy.send_event(UserEvent::EachDone(gen, task, results));
        });
    }

    fn each_done(&mut self, gen: u64, task: Task, results: Vec<Each>) {
        if !self.finish(gen) {
            return;
        }
        if results.last().is_some_and(|e| e.result == Err(Error::Stopped)) {
            self.app.note = Some(if task == Task::Testing { tr("The test was stopped.") } else { tr("Extracting was stopped.") });
        }
        let mut lines = Vec::new();
        let mut trouble = false;
        for e in results.iter().filter(|e| e.result != Err(Error::Stopped)) {
            let name = file_name(&e.archive);
            lines.push(match &e.result {
                Ok(r) if task == Task::Testing && r.errors.is_empty() => {
                    format!("{name}: {}", tf("no errors ({} files).", &[&format::count(r.files)]))
                }
                Ok(r) if task == Task::Testing => {
                    trouble = true;
                    format!("{name}: {}\n{}", tf("{} errors were found.", &[&format::count(r.errors.len() as u64)]), errors_text(r))
                }
                Ok(r) => {
                    trouble |= !r.errors.is_empty();
                    let mut l = format!(
                        "{name}: {}",
                        tf("{} files extracted ({}) to {}.", &[&format::count(r.files), &format::size(r.bytes), &e.dest.display().to_string()])
                    ) + &extract_notes(r);
                    if !r.errors.is_empty() {
                        l.push('\n');
                        l.push_str(&errors_text(r));
                    }
                    l
                }
                Err(err) => {
                    trouble = true;
                    format!("{name}: {}", error_text(err))
                }
            });
        }
        if lines.is_empty() {
            return;
        }
        let title = if task == Task::Testing { tr("Test") } else { tr("Extract") };
        if task == Task::Testing || trouble || lines.len() > 1 {
            self.info(&title, &lines.join("\n\n"));
        } else {
            self.app.note = Some(lines.remove(0));
        }
        if task == Task::Extracting {
            if let (true, Some(dir)) = (self.app.open_after && !trouble && results.len() == 1, results.first().map(|e| e.dest.clone())) {
                open_path(&dir);
            }
            // The new folders show in the folder on disk.
            if let Place::Folder(dir) = self.app.place.clone() {
                let select = results.first().map(|e| e.archive.clone());
                let _ = self.app.show_folder(&dir, select.as_deref());
            }
        }
    }

    /// Calculates hashes of what is chosen: files and folders on disk, or
    /// entries of the archive (all of it when nothing is selected).
    fn hash(&mut self, algos: Vec<Algo>) {
        if self.app.busy.is_some() || !view::can_hash(&self.app) {
            return;
        }
        if self.app.in_archive() {
            let Some(o) = &self.app.opened else { return };
            let (path, listing) = (o.path.clone(), o.listing.clone());
            let wanted = self.app.wanted();
            let total = self.app.job_size(wanted.as_deref());
            let count = hash::count_files(&listing, wanted.as_deref());
            self.start(Task::Hashing, path.clone(), count, total, false, move |meter, gen, proxy| {
                let want = |i: usize| wanted.as_ref().is_none_or(|w| w[i]);
                let result = hash::entries(&path, &listing, &want, &algos, meter);
                let _ = proxy.send_event(UserEvent::Hashed(gen, algos, result));
            });
        } else {
            let paths = self.app.disk_chosen();
            let Some(first) = paths.first().cloned() else { return };
            let count = paths.len();
            self.start(Task::Hashing, first, count, 0, false, move |meter, gen, proxy| {
                let result = hash::disk(&paths, &algos, meter);
                let _ = proxy.send_event(UserEvent::Hashed(gen, algos, result));
            });
        }
    }

    fn hashed(&mut self, gen: u64, algos: Vec<Algo>, result: Result<Vec<Sums>, Error>) {
        if !self.finish(gen) {
            return;
        }
        let sums = match result {
            Ok(s) => s,
            Err(Error::Stopped) => {
                self.app.note = Some(tr("Stopped."));
                return;
            }
            Err(e) => {
                self.info(&tr("Hashes"), &error_text(&e));
                return;
            }
        };
        // Shown: the first files; copied with Ctrl+C: all of them.
        const SHOWN: usize = 8;
        let block = |s: &Sums| -> String {
            let name = if s.name.is_empty() { tr("(the rest of the archive)") } else { s.name.clone() };
            let mut t = format!("{name}   ({})", format::size(s.size));
            match &s.sums {
                Ok(v) => {
                    for (a, h) in v {
                        t.push_str(&format!("\n{}\t{h}", a.name()));
                    }
                }
                Err(e) => t.push_str(&format!("\n{}", error_text(e))),
            }
            t
        };
        let all: Vec<String> = sums.iter().map(block).collect();
        let mut shown: Vec<String> = all.iter().take(SHOWN).cloned().collect();
        if all.len() > SHOWN {
            shown.push(tf("…and {} more.", &[&format::count((all.len() - SHOWN) as u64)]));
        }
        if sums.is_empty() {
            shown.push(tr("There are no files to hash."));
        } else {
            shown.push(tr("Ctrl+C copies the hashes; Save Hashes, under the Hashes button, writes them to a file."));
        }
        let total: u64 = sums.iter().map(|s| s.size).sum();
        let title = tf("Hashes · {} files · {}", &[&format::count(sums.len() as u64), &format::size(total)]);
        self.info(&title, &shown.join("\n\n"));
        self.dialog_copy = Some(all.join("\n\n"));
        self.last_sums = Some((algos, sums));
    }

    /// Writes the last hashes in the form `sha256sum -c` reads.
    fn save_hashes(&mut self) {
        let Some((algos, sums)) = &self.last_sums else { return };
        let name = format!("{}.{}", if sums.len() == 1 { sums[0].name.rsplit('/').next().unwrap_or("hashes").to_string() } else { "hashes".into() }, if algos.len() == 1 { algos[0].ext() } else { "txt" });
        let dir = self.app.disk_folder();
        #[cfg(windows)]
        let chosen = self.win.as_ref().and_then(|w| {
            let filter = rt_sys::win::dialogs::filter(&[(tr("All files"), "*.*")]);
            rt_sys::win::dialogs::save_file(w.hwnd, &name, dir.as_deref(), &filter)
        });
        #[cfg(not(windows))]
        let chosen = rt_sys::linux::dialogs::save_file(&name, dir.as_deref());
        let Some(path) = chosen else { return };
        let text = hash::sum_file(sums, algos);
        self.app.note = Some(match std::fs::write(&path, text) {
            Ok(()) => tf("The hashes were saved to {}.", &[&path.display().to_string()]),
            Err(e) => tf("Could not save {}: {}", &[&path.display().to_string(), &e.to_string()]),
        });
        if let Place::Folder(d) = self.app.place.clone() {
            let _ = self.app.show_folder(&d, Some(&path));
        }
    }

    /// Opens the "Add to archive" form for what is chosen on disk.
    fn open_add_form(&mut self) {
        let paths = self.app.disk_chosen();
        let Some(dir) = self.app.disk_folder() else { return };
        if paths.is_empty() {
            return;
        }
        self.app.add = Some(AddForm::new(paths, dir, self.app.spec.clone()));
        self.hot = Hot::None;
        self.redraw();
    }

    /// Adds what is chosen to an archive of `kind` beside it, at once.
    fn quick_add(&mut self, kind: NewKind) {
        let paths = self.app.disk_chosen();
        let Some(dir) = self.app.disk_folder() else { return };
        if paths.is_empty() {
            return;
        }
        let spec = Spec { kind, ..self.app.spec.clone() };
        let dest = rpack::app::free_path(&dir.join(create::suggested_name(&paths, kind)));
        self.create(vec![(paths, dest)], spec);
    }

    /// Each file or folder chosen into an archive of its own, as last chosen.
    fn add_each(&mut self) {
        let paths = self.app.disk_chosen();
        let Some(dir) = self.app.disk_folder() else { return };
        let mut form = AddForm::new(paths, dir, self.app.spec.clone());
        form.each = true;
        if form.spec.kind.needs_one_file() && form.paths.iter().any(|p| !p.is_file()) {
            form.spec.kind = NewKind::SevenZ;
        }
        let spec = form.spec.clone();
        self.create(form.targets(), spec);
    }

    /// Makes the archive the form describes (asking before replacing one).
    fn submit_form(&mut self) {
        let Some(f) = self.app.add.clone() else { return };
        if !f.ready() {
            return;
        }
        let targets = f.targets();
        if let [(_, dest)] = &targets[..] {
            if dest.exists() && !self.confirm(&tr("Add to Archive"), &tf("{} is already there. Replace it?", &[&file_name(dest)])) {
                return;
            }
        }
        self.app.spec = f.spec.clone();
        self.app.add = None;
        self.create(targets, f.spec);
    }

    /// Makes archives in the background: (what to put in, where).
    fn create(&mut self, targets: Vec<(Vec<PathBuf>, PathBuf)>, spec: Spec) {
        if targets.is_empty() || self.app.busy.is_some() {
            return;
        }
        let first = targets[0].1.clone();
        let count = targets.len();
        self.start(Task::Creating, first, count, 0, false, move |meter, gen, proxy| {
            let total: u64 = targets.iter().map(|(p, _)| create::gather(p).0.iter().map(|i| i.size).sum::<u64>()).sum();
            meter.total.store(total, std::sync::atomic::Ordering::Relaxed);
            let mut results = Vec::new();
            for (paths, dest) in &targets {
                let r = create::create(paths, dest, &spec, meter);
                let stop = r == Err(Error::Stopped);
                results.push((dest.clone(), r));
                if stop {
                    break;
                }
            }
            let _ = proxy.send_event(UserEvent::Created(gen, results));
        });
    }

    fn created(&mut self, gen: u64, results: Vec<(PathBuf, Result<Made, Error>)>) {
        if !self.finish(gen) {
            return;
        }
        let mut lines = Vec::new();
        let mut trouble = false;
        for (dest, r) in &results {
            let name = file_name(dest);
            lines.push(match r {
                Ok(m) => {
                    let mut l = tf("{} created: {} files, {} → {}.", &[&name, &format::count(m.files), &format::size(m.bytes), &format::size(m.size)]);
                    if m.links > 0 {
                        l.push(' ');
                        l.push_str(&tf("{} links were left out.", &[&format::count(m.links)]));
                    }
                    if !m.skipped.is_empty() {
                        trouble = true;
                        l.push('\n');
                        l.push_str(&tf("{} files could not be read and were left out:", &[&format::count(m.skipped.len() as u64)]));
                        for (p, why) in m.skipped.iter().take(10) {
                            l.push_str(&format!("\n{p}: {why}"));
                        }
                    }
                    l
                }
                Err(Error::Stopped) => tr("Stopped."),
                Err(e) => {
                    trouble = true;
                    format!("{name}: {}", error_text(e))
                }
            });
        }
        // The new archives show in the folder.
        if let Place::Folder(dir) = self.app.place.clone() {
            let select = results.iter().find(|(_, r)| r.is_ok()).map(|(d, _)| d.clone());
            let _ = self.app.show_folder(&dir, select.as_deref());
        }
        if trouble || lines.len() > 1 {
            self.info(&tr("Add to Archive"), &lines.join("\n\n"));
        } else if let Some(l) = lines.pop() {
            self.app.note = Some(l);
        }
    }

    /// Asks before doing something that cannot be undone from here.
    fn confirm(&self, title: &str, question: &str) -> bool {
        #[cfg(windows)]
        {
            use rt_sys::win::dialogs::{message, Answer, Buttons, Icon};
            let hwnd = self.win.as_ref().map_or(std::ptr::null_mut(), |w| w.hwnd);
            message(hwnd, title, question, Buttons::OkCancel, Icon::Warning) == Answer::Ok
        }
        #[cfg(not(windows))]
        rt_sys::linux::dialogs::confirm(title, question)
    }

    /// The paths of what is chosen: files on disk, or entries of the archive.
    fn chosen_paths(&self) -> Vec<String> {
        if self.app.in_archive() {
            let Some(o) = &self.app.opened else { return Vec::new() };
            let mut rows = self.app.chosen();
            if rows.is_empty() {
                rows.extend(self.app.cursor.and_then(|c| self.app.rows.get(c).copied()));
            }
            rows.into_iter()
                .filter_map(|r| match r {
                    Row::Dir(n) => Some(o.tree.path(n)),
                    Row::File(e) => Some(o.listing.entries[e].path.clone()),
                    _ => None,
                })
                .collect()
        } else {
            self.app.disk_chosen().iter().map(|p| p.display().to_string()).collect()
        }
    }

    /// Moves what is chosen on disk to the Recycle Bin, after asking.
    fn recycle(&mut self) {
        let paths = self.app.disk_chosen();
        if paths.is_empty() || self.app.place == Place::Drives {
            return;
        }
        let what = if paths.len() == 1 { file_name(&paths[0]) } else { tf("{} items", &[&format::count(paths.len() as u64)]) };
        let question = if cfg!(windows) { tf("Move {} to the Recycle Bin?", &[&what]) } else { tf("Move {} to the trash?", &[&what]) };
        if !self.confirm(&rt_core::i18n::strip_mnemonic(&Command::Recycle.label()), &question) {
            return;
        }
        let failed: Vec<String> = paths.iter().filter_map(|p| rt_sys::recycle(p).err().map(|e| format!("{}: {e}", file_name(p)))).collect();
        if let Place::Folder(dir) = self.app.place.clone() {
            let _ = self.app.show_folder(&dir, None);
        }
        if !failed.is_empty() {
            self.info(&rt_core::i18n::strip_mnemonic(&Command::Recycle.label()), &failed.join("\n"));
        }
    }

    /// Opens the entry of a file row with its program: extracted first into
    /// a folder of its own in the temporary folder.
    fn open_entry(&mut self, e: usize) {
        let Some(o) = &self.app.opened else { return };
        let entry = &o.listing.entries[e];
        self.temp_count += 1;
        let dest = temp_root().join(self.temp_count.to_string());
        let Some(file) = extract::safe_path(&dest, entry.name()) else { return };
        let mut wanted = vec![false; o.listing.entries.len()];
        wanted[e] = true;
        let job = Job { dest, wanted: Some(wanted), strip: o.tree.path(self.app.folder), overwrite: Overwrite::Replace };
        self.extract(job, Some(file));
    }

    fn update_title(&self) {
        let Some(w) = &self.win else { return };
        let title = match (&self.app.place, &self.app.opened) {
            (Place::Archive, Some(o)) => format!("{} - rpack", file_name(&o.path)),
            (Place::Folder(d), _) => format!("{} - rpack", file_name(d)),
            _ => "rpack".to_string(),
        };
        if w.window.title() != title {
            w.window.set_title(&title);
        }
    }

    // ------------------------------------------------------------ commands

    fn command(&mut self, el: &ActiveEventLoop, cmd: Command) {
        if !self.enabled(cmd) {
            return;
        }
        match cmd {
            Command::OpenArchive => {
                if let Some(p) = self.pick_archive() {
                    self.open_archive(p);
                }
            }
            Command::CloseArchive => {
                if let Some(o) = self.app.opened.as_ref() {
                    let (dir, path) = (o.path.parent().map(Path::to_path_buf), o.path.clone());
                    match dir {
                        Some(d) => self.go(Crumb::Folder(d), Some(path)),
                        None => self.go(Crumb::Drives, None),
                    }
                }
            }
            Command::Exit => self.quit(el),
            Command::OpenItem => self.open_row(),
            Command::Up => self.go_up(),
            Command::Refresh => match self.app.place.clone() {
                Place::Folder(d) => {
                    let select = self.app.cursor.and_then(|c| match self.app.rows.get(c) {
                        Some(Row::Disk(i)) => Some(self.app.disk[*i].path.clone()),
                        _ => None,
                    });
                    self.show_folder(&d, select.as_deref());
                }
                Place::Drives => self.load_drives(None),
                Place::Archive => {
                    if let Some(p) = self.app.opened.as_ref().map(|o| o.path.clone()) {
                        self.open_archive(p);
                    }
                }
            },
            Command::SelectAll => self.app.select_all(),
            Command::AddDialog => self.open_add_form(),
            Command::AddTo7z => self.quick_add(NewKind::SevenZ),
            Command::AddToZip => self.quick_add(NewKind::Zip),
            Command::AddEach => self.add_each(),
            Command::Extract => {
                if self.app.in_archive() {
                    if let Some(job) = self.app.dest().and_then(|d| self.app.job(d)) {
                        self.extract(job, None);
                    }
                } else {
                    self.each(Task::Extracting, EachDest::Smart);
                }
            }
            Command::ExtractHere => self.each(Task::Extracting, EachDest::Here),
            Command::ExtractNamed => self.each(Task::Extracting, EachDest::Named),
            Command::ExtractTo => {
                if let Some(dir) = self.pick_folder(&tr("Choose where to extract")) {
                    if self.app.in_archive() {
                        self.app.dest = Some(dir.clone());
                        if let Some(job) = self.app.job(dir) {
                            self.extract(job, None);
                        }
                    } else {
                        self.each(Task::Extracting, EachDest::Into(dir));
                    }
                }
            }
            Command::Reveal => {
                if let Some(p) = self.app.disk_chosen().first() {
                    rt_sys::reveal(p);
                } else if let Some(o) = &self.app.opened {
                    rt_sys::reveal(&o.path);
                }
            }
            Command::CopyPath => {
                let paths = self.chosen_paths();
                if !paths.is_empty() {
                    rt_sys::set_clipboard_text(&paths.join("\n"));
                    self.app.note = Some(tr("Copied to the clipboard."));
                }
            }
            Command::Recycle => self.recycle(),
            Command::ChooseFolder => {
                if let Some(dir) = self.pick_folder(&tr("Choose where to extract")) {
                    self.app.dest = Some(dir);
                }
            }
            Command::Test => {
                if self.app.in_archive() {
                    self.test();
                } else {
                    self.each(Task::Testing, EachDest::Smart);
                }
            }
            Command::Stop => {
                if let Some(b) = &self.app.busy {
                    b.meter.stop();
                }
            }
            Command::Overwrite(i) => self.app.overwrite = Overwrite::ALL[(i as usize).min(2)],
            Command::OpenAfter => self.app.open_after = !self.app.open_after,
            Command::Hash(ALL_HASHES) => self.hash(Algo::ALL.to_vec()),
            Command::Hash(i) => {
                if let Some(&a) = Algo::ALL.get(i as usize) {
                    self.hash(vec![a]);
                }
            }
            Command::SaveHashes => self.save_hashes(),
            Command::Theme(t) => {
                self.settings.theme = t;
                self.apply_window_theme();
            }
            Command::UiLanguage(i) => {
                self.settings.language = (i as usize).checked_sub(1).and_then(|k| i18n::Lang::ALL.get(k)).copied();
                self.apply_language();
            }
            Command::Shortcuts => self.info(&tr("Keyboard shortcuts"), &shortcuts_text()),
            Command::About => {
                let text = tf(
                    "rpack {}\n\nOpens, tests and extracts archives, written in Rust.\nNo telemetry, no network access.\n\nMIT License.",
                    &[env!("CARGO_PKG_VERSION")],
                );
                self.info(&tr("About rpack"), &text);
            }
        }
        self.menu_dirty = true;
        self.redraw();
    }

    fn checked(&self, c: Command) -> bool {
        match c {
            Command::Theme(t) => self.settings.theme == t,
            Command::UiLanguage(0) => self.settings.language.is_none(),
            Command::UiLanguage(i) => self.settings.language == i18n::Lang::ALL.get(i as usize - 1).copied(),
            Command::Overwrite(i) => Overwrite::ALL.get(i as usize) == Some(&self.app.overwrite),
            Command::OpenAfter => self.app.open_after,
            _ => false,
        }
    }

    fn enabled(&self, c: Command) -> bool {
        let idle = self.app.busy.is_none();
        let archive = idle && self.app.in_archive();
        match c {
            Command::OpenArchive | Command::Refresh => idle,
            Command::CloseArchive | Command::ChooseFolder => archive,
            Command::Extract | Command::Test | Command::ExtractTo => idle && view::can_extract(&self.app),
            Command::ExtractHere | Command::ExtractNamed => idle && !self.app.in_archive() && !self.app.chosen_archives().is_empty(),
            Command::AddDialog | Command::AddTo7z | Command::AddToZip => idle && view::can_add(&self.app),
            Command::AddEach => idle && view::can_add(&self.app) && self.app.disk_chosen().len() > 1,
            Command::Reveal => idle && (self.app.opened.is_some() || !self.app.disk_chosen().is_empty()),
            Command::CopyPath => idle && !self.chosen_paths().is_empty(),
            Command::Recycle => idle && matches!(self.app.place, Place::Folder(_)) && !self.app.disk_chosen().is_empty(),
            Command::SelectAll => idle && !self.app.rows.is_empty(),
            Command::OpenItem => idle && self.app.cursor.is_some(),
            Command::Up => idle && self.app.up_target().is_some(),
            Command::Hash(_) => idle && view::can_hash(&self.app),
            Command::SaveHashes => idle && self.last_sums.is_some(),
            Command::Stop => !idle,
            _ => true,
        }
    }

    fn info(&mut self, title: &str, text: &str) {
        self.dialog = Some(Message::new(title, text));
        self.dialog_copy = None;
        self.dragging = false;
        self.redraw();
    }

    fn quit(&mut self, el: &ActiveEventLoop) {
        if let Some(b) = &self.app.busy {
            b.meter.stop();
        }
        self.save_settings();
        let _ = std::fs::remove_dir_all(temp_root());
        el.exit();
    }

    fn pick_archive(&mut self) -> Option<PathBuf> {
        let dir = self.app.disk_folder();
        #[cfg(windows)]
        {
            let filter = rt_sys::win::dialogs::filter(&[(tr("Archives"), ARCHIVE_PATTERNS), (tr("All files"), "*.*")]);
            self.win.as_ref().and_then(|w| rt_sys::win::dialogs::open_file(w.hwnd, dir.as_deref(), &filter))
        }
        #[cfg(not(windows))]
        rt_sys::linux::dialogs::open_files(dir.as_deref(), &[(tr("Archives"), ARCHIVE_PATTERNS), (tr("All files"), "*")], false).into_iter().next()
    }

    fn pick_folder(&mut self, title: &str) -> Option<PathBuf> {
        #[cfg(windows)]
        return self.win.as_ref().and_then(|w| rt_sys::win::dialogs::pick_folder(w.hwnd, title));
        #[cfg(not(windows))]
        rt_sys::linux::dialogs::pick_folder(title)
    }

    fn open_window(&mut self, el: &ActiveEventLoop) -> bool {
        let (w, h, maximized) = self.settings.window.unwrap_or((980.0, 680.0, false));
        let attrs = Window::default_attributes()
            .with_title("rpack")
            .with_inner_size(LogicalSize::new(w.max(480.0), h.max(360.0)))
            .with_min_inner_size(LogicalSize::new(480.0, 360.0))
            .with_maximized(maximized)
            .with_window_icon(rt_ui::window::icon(rpack::ui::appicon::render))
            .with_theme(Some(rt_ui::window::theme(self.dark())))
            .with_visible(false);
        let Some((window, surface)) = rt_ui::window::create(el, attrs, "rpack") else { return false };
        let adapter = accesskit_winit::Adapter::with_event_loop_proxy(el, &window, self.proxy.clone());
        #[cfg(windows)]
        let hwnd = rt_ui::window::hwnd_of(&window);
        #[cfg(windows)]
        {
            let proxy = self.proxy.clone();
            rt_sys::win::subclass(
                hwnd,
                &IDENT,
                &rt_ui::window::mnemonics_of(&commands::menu_tree()),
                Box::new(move |m| {
                    let _ = proxy.send_event(UserEvent::Win(m));
                }),
            );
        }
        self.win = Some(Win {
            window,
            surface,
            adapter,
            #[cfg(windows)]
            hwnd,
            #[cfg(windows)]
            menu: None,
        });
        true
    }
}

/// The archives the open dialog shows.
const ARCHIVE_PATTERNS: &str = "*.zip;*.7z;*.tar;*.gz;*.tgz;*.bz2;*.tbz2;*.xz;*.txz;*.lzma;*.lz;*.tlz;*.zst;*.tzst;*.lz4;*.br;*.jar;*.apk";

/// Whether a file on disk is an archive rpack opens: by its first bytes (and
/// the extension for lzma and brotli).
fn is_archive(p: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 1024];
    let Ok(mut f) = std::fs::File::open(p) else { return false };
    let n = f.read(&mut head).unwrap_or(0);
    let name = file_name(p).to_lowercase();
    archive::detect::sniff(&head[..n], &name).is_some() || (rpack::app::looks_like_archive(&name) && head.starts_with(b"PK"))
}

/// Opens a file with its program, or a folder in the file manager.
fn open_path(p: &Path) {
    #[cfg(windows)]
    rt_sys::win::shell_open(&p.to_string_lossy());
    #[cfg(not(windows))]
    rt_sys::linux::xdg_open(p.as_os_str().to_os_string());
}

fn shortcuts_text() -> String {
    let rows = [
        ("Enter", tr("Open the folder, archive or file selected")),
        ("Backspace", tr("Back to the folder above")),
        ("F5", tr("Read the folder again")),
        ("Ctrl+O", tr("Open an archive")),
        ("Ctrl+W", tr("Close the archive")),
        ("Ctrl+A", tr("Select everything in the folder")),
        ("Ctrl+E", tr("Extract the selection, or everything")),
        ("Ctrl+Shift+E", tr("Extract to a folder you choose")),
        ("Ctrl+T", tr("Test the archive")),
        ("Esc", tr("Stop")),
        ("↑ ↓  PgUp PgDn  Home End", tr("Move in the list (Shift selects)")),
    ];
    rows.iter().map(|(k, v)| format!("{k}\t{v}")).collect::<Vec<_>>().join("\n")
}

impl ApplicationHandler<UserEvent> for Shell {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.win.is_some() || !self.open_window(el) {
            return;
        }
        self.sync_menu();
        if let Some(w) = &self.win {
            w.window.set_visible(true);
            w.window.focus_window();
        }
        self.redraw();
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Drives(v, select) => {
                if self.app.busy.is_none() {
                    self.app.show_drives(v, select.as_deref());
                }
                self.menu_dirty = true;
                self.redraw();
            }
            UserEvent::Opened(gen, path, result) => self.opened(gen, path, result),
            UserEvent::Extracted(gen, result, dest, open) => self.extracted(gen, result, dest, open),
            UserEvent::Tested(gen, result) => self.tested(gen, result),
            UserEvent::EachDone(gen, task, results) => self.each_done(gen, task, results),
            UserEvent::Hashed(gen, algos, result) => self.hashed(gen, algos, result),
            UserEvent::Created(gen, results) => self.created(gen, results),
            UserEvent::Failed(gen, why) => {
                if self.finish(gen) {
                    self.info("rpack", &error_text(&Error::Corrupt(why)));
                }
            }
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::Command(id)) => {
                let cmd = self.win.as_ref().and_then(|w| w.menu.as_ref()).and_then(|m| m.command(id));
                if let Some(c) = cmd {
                    self.command(el, c);
                }
            }
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::SystemTheme) => {
                let dark = rt_sys::system_dark().unwrap_or(false);
                self.accent = rt_sys::accent().map(Color::rgb);
                if dark != self.system_dark {
                    self.system_dark = dark;
                    self.apply_window_theme();
                }
                self.redraw();
            }
            #[cfg(windows)]
            UserEvent::Win(rt_sys::win::WinMsg::OpenFiles(paths)) => {
                if let Some(p) = paths.into_iter().next() {
                    self.open_archive(p);
                }
            }
            #[cfg(windows)]
            UserEvent::Win(_) => {}
            UserEvent::A11y(ev) => match ev.window_event {
                accesskit_winit::WindowEvent::ActionRequested(req) if req.action == accesskit::Action::Click => {
                    let id = req.target_node.0;
                    if id == 6 {
                        self.dialog = None;
                    } else if let Some(i) = id.checked_sub(100) {
                        self.app.click(i as usize, false, false);
                    }
                    let _ = el;
                    self.redraw();
                }
                accesskit_winit::WindowEvent::InitialTreeRequested => {
                    let tree = self.tree();
                    if let Some(w) = &mut self.win {
                        w.adapter.update_if_active(|| tree);
                    }
                }
                _ => {}
            },
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let Some(w) = &mut self.win {
            w.adapter.process_event(&w.window, &event);
        }
        match event {
            WindowEvent::CloseRequested => self.quit(el),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.redraw(),
            WindowEvent::ThemeChanged(t) => {
                self.system_dark = t == WinTheme::Dark;
                self.redraw();
            }
            WindowEvent::Focused(false) => self.save_settings(),
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::DroppedFile(p) if self.app.busy.is_none() => {
                if p.is_dir() {
                    self.show_folder(&p, None);
                } else {
                    self.open_archive(p);
                }
            }
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if event.state == ElementState::Pressed {
                    self.key(el, &event);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let (x, y) = (position.x as f32, position.y as f32);
                self.cursor = Some((x, y));
                #[cfg(not(windows))]
                if self.menu_pointer(el, x, y) {
                    return;
                }
                if self.dragging {
                    self.drag_to(y);
                }
                self.pointer(x, y);
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor = None;
                if !self.dragging && self.hot != Hot::None {
                    self.hot = Hot::None;
                    self.redraw();
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                ElementState::Pressed => {
                    if let Some((x, y)) = self.cursor {
                        self.press(el, x, y);
                    }
                }
                ElementState::Released => self.dragging = false,
            },
            #[cfg(not(windows))]
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } if self.menu.is_open() => {
                self.menu.close();
                self.redraw();
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Right, .. } if self.dialog.is_none() && self.app.add.is_none() => {
                if let Hot::Row(i) = self.hot {
                    if !self.app.selected.get(i).copied().unwrap_or(false) {
                        self.app.click(i, false, false);
                    }
                    self.app.cursor = Some(i);
                    self.menu_dirty = true;
                    self.redraw();
                    if self.app.rows[i] != Row::Up {
                        self.context_menu(el);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } if self.dialog.is_none() && self.app.add.is_none() => {
                #[cfg(not(windows))]
                {
                    let down = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y < 0.0,
                        MouseScrollDelta::PixelDelta(p) => p.y < 0.0,
                    };
                    let step = rt_ui::menus::wheel(&mut self.menu, self.cursor, down);
                    if self.menu_step(el, step) {
                        return;
                    }
                }
                if view::page(&self.app) != Page::Browse {
                    return;
                }
                let rows = match delta {
                    MouseScrollDelta::LineDelta(_, l) => (-l * 3.0).round() as isize,
                    MouseScrollDelta::PixelDelta(p) => (-p.y as f32 / self.layout.row_h.max(20.0)).round() as isize,
                };
                self.app.scroll_by(rows);
                self.redraw();
                if let Some((x, y)) = self.cursor {
                    self.pointer(x, y);
                }
            }
            WindowEvent::RedrawRequested => self.render(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // The work page moves even between events.
        if self.app.busy.is_some() {
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(150)));
            self.redraw();
        } else {
            el.set_control_flow(ControlFlow::Wait);
        }
    }
}

const USAGE: &str = "rpack [archive | folder]

  Browses drives and folders, and opens, tests and extracts archives: zip,
  tar, and files compressed whole with gzip, bzip2, xz, lzma, lzip, zstd, lz4
  or brotli (also around a tar: .tar.gz, .tar.xz, .tar.zst...). The format is
  found from the file's contents, whatever its name. Also calculates hashes:
  CRC-32, CRC-64, XXH64, MD5, SHA-1, SHA-256, SHA-384, SHA-512, SHA3-256,
  BLAKE2sp and BLAKE3.

  archive       Open this archive.
  folder        Show this folder. Without either, rpack shows the folder
                shown last.

Settings are stored in the user's settings folder (%APPDATA%\\rpack on
Windows, ~/.config/rpack on Linux), or next to the program when a file named
rpack.portable exists there (portable mode).";

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // The help goes to the console it was asked from; without one, rpack
    // starts and shows it in its own window, in its theme.
    let help = raw.iter().any(|a| a == "--help" || a == "-h" || a == "/?");
    #[cfg(windows)]
    if help && rt_sys::win::print_to_console(USAGE) {
        return;
    }
    #[cfg(not(windows))]
    if help {
        println!("{USAGE}");
        return;
    }
    let arg = raw.iter().find(|a| !a.starts_with("--")).map(PathBuf::from);
    let mut location = storage::detect();
    let store = location.load();
    let settings = Settings::load(&store);
    let gfx = match Gfx::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("rpack: {e}");
            std::process::exit(1);
        }
    };
    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("rpack: cannot start the event loop: {e}");
            std::process::exit(1);
        }
    };
    let mut app = App::new();
    app.overwrite = settings.overwrite;
    app.open_after = settings.open_after;
    app.spec = settings.spec.clone();
    let start_folder = settings.folder.clone();
    let mut shell = Shell {
        app,
        settings,
        location,
        gfx,
        win: None,
        proxy: event_loop.create_proxy(),
        mods: ModifiersState::empty(),
        cursor: None,
        system_dark: rt_sys::system_dark().unwrap_or(false),
        accent: rt_sys::accent().map(Color::rgb),
        menu_dark: None,
        menu_dirty: true,
        menu_rebuild: true,
        layout: Layout::default(),
        hot: Hot::None,
        dragging: false,
        last_click: None,
        cursor_icon: CursorIcon::Default,
        generation: 0,
        temp_count: 0,
        dialog: help.then(|| Message::new("rpack", USAGE)),
        dialog_copy: None,
        last_sums: None,
        #[cfg(not(windows))]
        menu: rpack::ui::menu::Menu::default(),
    };
    shell.apply_language();
    // Where to start: the folder or archive asked for, the folder shown last,
    // the user's home, or the drives.
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    match arg {
        Some(p) if p.is_dir() => shell.show_folder(&p, None),
        Some(p) => {
            if let Some(dir) = p.parent().filter(|d| d.is_dir()) {
                let _ = shell.app.show_folder(dir, Some(&p));
            }
            shell.open_archive(p);
        }
        None => {
            let shown = [start_folder, home].into_iter().flatten().any(|d| shell.app.show_folder(&d, None).is_ok());
            if !shown {
                shell.load_drives(None);
            }
        }
    }
    if let Err(e) = event_loop.run_app(&mut shell) {
        eprintln!("rpack: {e}");
    }
    let _ = std::fs::remove_dir_all(temp_root());
}
