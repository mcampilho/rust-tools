//! The widgets' windows (Windows): a thread of its own, with one layered
//! window per widget, drawn by `paint` with per-pixel transparency.
//!
//! Pinned widgets are owned by the window holding the desktop icons (see
//! `live::desktop::icons_window`): owned windows always stay above their
//! owner, so a widget sits over the wallpaper and the icons, while every
//! other window stays above it (its z-order is held where it is, even when
//! it takes the keyboard to write in a note). "Always on top" widgets are
//! plain topmost windows instead.
//!
//! The windows' procedure only queues what happened; the thread's loop
//! handles it, so nothing is re-entered while Windows calls back.

use super::edit::{line_of, Editor};
use super::paint::{self, Item, Look, Painted, View, Zone, CELL};
use super::weather::{self, Forecast, Place};
use super::{WKind, Widget, NOTE_COLORS, TEXT_COLORS, TEXT_SIZES};
use crate::i18n::{self, tf, tr, Lang};
use crate::live::desktop;
use rt_gfx::{Color, Gfx};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::core::{implement, w, Ref, BOOL, HRESULT, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, LPARAM, LRESULT, POINT, POINTL, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{IBindCtx, IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::{MK_CONTROL, MK_LBUTTON, MK_SHIFT, MODIFIERKEYS_FLAGS};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// What the widgets tell the settings window.
#[derive(Debug)]
pub enum Event {
    /// Moved, resized, written in, or an option changed from its menu.
    Changed(Widget),
    /// "Remove the widget" from its menu.
    Remove(u32),
    /// "New note" from a note's menu: where to put it.
    NewNote { x: i32, y: i32, w: i32, h: i32 },
    /// "Widget settings…".
    Settings,
    /// A word about a widget for the settings window (the weather), or None.
    Status(u32, Option<String>),
}

/// How the widgets look and speak.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub dark: bool,
    pub accent: Color,
    pub lang: Lang,
}

enum Cmd {
    Set(Vec<Widget>, Style),
    Quit,
}

/// The widgets' thread, seen from the settings window.
pub struct Host {
    tx: Sender<Cmd>,
    waker: Waker,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Host {
    /// Starts the thread; `report` hears what the widgets do (from that thread).
    pub fn start(report: Box<dyn Fn(Event) + Send>) -> Option<Host> {
        let (tx, rx) = channel();
        let (id_tx, id_rx) = channel();
        let join = std::thread::Builder::new()
            .name("rwall-widgets".into())
            .spawn(move || {
                let mut msg = MSG::default();
                // SAFETY: makes the thread's message queue before telling its
                // id, so no wake-up is lost.
                let id = unsafe {
                    let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                    GetCurrentThreadId()
                };
                let _ = id_tx.send(id);
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(rx, &*report, Waker(id))));
                cleanup();
            })
            .ok()?;
        let id = id_rx.recv().ok()?;
        Some(Host { tx, waker: Waker(id), join: Some(join) })
    }

    pub fn set(&self, list: Vec<Widget>, style: Style) {
        let _ = self.tx.send(Cmd::Set(list, style));
        self.waker.wake();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        self.waker.wake();
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Asks a yes-or-no question over `owner` (the settings window).
pub fn confirm(owner: rt_sys::win::HWND, text: &str) -> bool {
    // SAFETY: a plain message box; the window may be null.
    unsafe { MessageBoxW(Some(HWND(owner as _)), &HSTRING::from(text), w!("rwall"), MB_YESNO | MB_ICONQUESTION) == IDYES }
}

/// Wakes the widgets' thread up.
#[derive(Clone, Copy)]
struct Waker(u32);

impl Waker {
    fn wake(self) {
        // SAFETY: only posts a message to the thread.
        let _ = unsafe { PostThreadMessageW(self.0, WM_APP, WPARAM(0), LPARAM(0)) };
    }
}

mod fence;
mod input;
mod timers;

use fence::*;

// ---------------------------------------------------------------- window procedure

const WIDGET_CLASS: PCWSTR = w!("rwall-widget");
const CONTROL_CLASS: PCWSTR = w!("rwall-widget-control");

/// What a window heard, for the thread's loop.
#[derive(Clone, Copy, Debug)]
enum Ev {
    Down(i32, i32, bool),
    Up,
    Move(i32, i32),
    Menu(i32, i32),
    Leave,
    Wheel(i32),
    Char(u16),
    Key(u16),
    Blur,
    Dpi,
}

/// The pointer's shape over a widget.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pointer {
    Arrow,
    Resize,
    Text,
    Hand,
    Moving,
}

thread_local! {
    static QUEUE: RefCell<VecDeque<(isize, Ev)>> = const { RefCell::new(VecDeque::new()) };
    /// Windows whose place among the others must not change.
    static PINNED: RefCell<HashSet<isize>> = RefCell::new(HashSet::new());
    /// Windows that take the keyboard.
    static EDITABLE: RefCell<HashSet<isize>> = RefCell::new(HashSet::new());
    static POINTER: RefCell<HashMap<isize, Pointer>> = RefCell::new(HashMap::new());
    /// Set while rwall itself moves a pinned window among the others.
    static ALLOW_Z: Cell<bool> = const { Cell::new(false) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    /// Explorer restarted.
    static RESET: Cell<bool> = const { Cell::new(false) };
    /// Screens changed.
    static LAYOUT: Cell<bool> = const { Cell::new(false) };
}

fn key(h: HWND) -> isize {
    h.0 as isize
}

fn lo(v: isize) -> i32 {
    (v & 0xFFFF) as u16 as i16 as i32
}

fn hi(v: isize) -> i32 {
    ((v >> 16) & 0xFFFF) as u16 as i16 as i32
}

unsafe extern "system" fn widget_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ev = match msg {
        WM_LBUTTONDOWN => Some(Ev::Down(lo(lp.0), hi(lp.0), false)),
        WM_LBUTTONDBLCLK => Some(Ev::Down(lo(lp.0), hi(lp.0), true)),
        WM_LBUTTONUP => Some(Ev::Up),
        WM_MOUSEMOVE => Some(Ev::Move(lo(lp.0), hi(lp.0))),
        WM_RBUTTONUP => Some(Ev::Menu(lo(lp.0), hi(lp.0))),
        WM_MOUSELEAVE => Some(Ev::Leave),
        WM_MOUSEWHEEL => Some(Ev::Wheel(hi(wp.0 as isize))),
        WM_CHAR => Some(Ev::Char(wp.0 as u16)),
        WM_KEYDOWN => Some(Ev::Key(wp.0 as u16)),
        WM_KILLFOCUS => Some(Ev::Blur),
        WM_CAPTURECHANGED => Some(Ev::Up),
        WM_DPICHANGED => Some(Ev::Dpi),
        _ => None,
    };
    if let Some(ev) = ev {
        QUEUE.with(|q| q.borrow_mut().push_back((key(h), ev)));
    }
    match msg {
        WM_MOUSEACTIVATE => {
            let editable = EDITABLE.with(|s| s.borrow().contains(&key(h)));
            return LRESULT(if editable { MA_ACTIVATE } else { MA_NOACTIVATE } as isize);
        }
        WM_WINDOWPOSCHANGING if lp.0 != 0 => {
            if PINNED.with(|s| s.borrow().contains(&key(h))) && !ALLOW_Z.with(Cell::get) {
                // SAFETY: Windows passes a WINDOWPOS with this message.
                unsafe { (*(lp.0 as *mut WINDOWPOS)).flags |= SWP_NOZORDER };
            }
        }
        WM_SETCURSOR if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
            let p = POINTER.with(|m| m.borrow().get(&key(h)).copied()).unwrap_or(Pointer::Arrow);
            let id = match p {
                Pointer::Arrow => IDC_ARROW,
                Pointer::Resize => IDC_SIZENWSE,
                Pointer::Text => IDC_IBEAM,
                Pointer::Hand => IDC_HAND,
                Pointer::Moving => IDC_SIZEALL,
            };
            // SAFETY: a system cursor.
            unsafe {
                let _ = SetCursor(LoadCursorW(None, id).ok());
            }
            return LRESULT(1);
        }
        // Alt+F4 on a note must not close it.
        WM_CLOSE => return LRESULT(0),
        WM_ERASEBKGND => return LRESULT(1),
        _ => {}
    }
    // SAFETY: default handling.
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

unsafe extern "system" fn control_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg != 0 && msg == TASKBAR_CREATED.with(Cell::get) {
        RESET.with(|c| c.set(true));
        return LRESULT(0);
    }
    if msg == WM_DISPLAYCHANGE || msg == WM_SETTINGCHANGE {
        LAYOUT.with(|c| c.set(true));
    }
    // SAFETY: default handling.
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

/// From `Win32_UI_Controls`, not worth a feature for one number.
const WM_MOUSELEAVE: u32 = 0x02A3;

type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

fn register_classes() {
    // SAFETY: the classes point to static names and procedures.
    unsafe {
        let inst = GetModuleHandleW(None).unwrap_or_default();
        for (name, proc_, style) in [(WIDGET_CLASS, widget_proc as WndProc, CS_DBLCLKS), (CONTROL_CLASS, control_proc as WndProc, WNDCLASS_STYLES(0))] {
            let wc = WNDCLASSW { style, lpfnWndProc: Some(proc_), hInstance: inst.into(), lpszClassName: name, ..Default::default() };
            RegisterClassW(&wc);
        }
        TASKBAR_CREATED.with(|c| c.set(RegisterWindowMessageW(w!("TaskbarCreated"))));
    }
}

/// Moves a pinned window among the others (they are held in place otherwise).
fn with_z<T>(f: impl FnOnce() -> T) -> T {
    ALLOW_Z.with(|c| c.set(true));
    let r = f();
    ALLOW_Z.with(|c| c.set(false));
    r
}

fn dpi_scale(h: HWND) -> f32 {
    // SAFETY: plain query (0 for a window that is gone).
    let d = unsafe { GetDpiForWindow(h) };
    if d == 0 {
        1.0
    } else {
        d as f32 / 96.0
    }
}

/// Shows `pix` (premultiplied RGBA) as the window's content, at (`x`, `y`).
fn present(h: HWND, pix: &tiny_skia::Pixmap, x: i32, y: i32) {
    let (w, ht) = (pix.width() as i32, pix.height() as i32);
    // SAFETY: GDI objects made here are released before returning; the DIB
    // is as large as the copy written into it.
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -ht,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        if let Ok(dib) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            if !bits.is_null() {
                let out = std::slice::from_raw_parts_mut(bits as *mut u8, (w * ht * 4) as usize);
                for (o, i) in out.as_chunks_mut::<4>().0.iter_mut().zip(pix.data().as_chunks::<4>().0) {
                    *o = [i[2], i[1], i[0], i[3]];
                }
                let old = SelectObject(mem, HGDIOBJ(dib.0));
                let pos = POINT { x, y };
                let size = SIZE { cx: w, cy: ht };
                let src = POINT { x: 0, y: 0 };
                let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
                let _ = UpdateLayeredWindow(h, Some(screen), Some(&pos), Some(&size), Some(mem), Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
                SelectObject(mem, old);
            }
            let _ = DeleteObject(HGDIOBJ(dib.0));
        }
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
}

/// The work area of the screen holding `p` (screen coordinates).
fn work_area(p: POINT) -> RECT {
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    // SAFETY: plain queries.
    unsafe {
        let m = MonitorFromPoint(p, MONITOR_DEFAULTTOPRIMARY);
        if GetMonitorInfoW(m, &mut mi).as_bool() {
            return mi.rcWork;
        }
    }
    RECT { left: 0, top: 0, right: 1920, bottom: 1040 }
}

/// Whether some of `r` is on a screen.
fn on_a_screen(r: &RECT) -> bool {
    // SAFETY: plain query.
    !unsafe { MonitorFromRect(r, MONITOR_DEFAULTTONULL) }.is_invalid()
}

fn cursor() -> POINT {
    let mut p = POINT::default();
    // SAFETY: writes the local.
    let _ = unsafe { GetCursorPos(&mut p) };
    p
}

fn ctrl_down() -> bool {
    // SAFETY: plain query.
    unsafe { GetKeyState(VK_CONTROL.0 as i32) < 0 }
}

fn shift_down() -> bool {
    // SAFETY: plain query.
    unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 }
}

fn open(path: &Path) {
    rt_sys::win::shell_open(&path.to_string_lossy());
}

// ---------------------------------------------------------------- one widget

#[derive(Clone, Copy)]
enum DragKind {
    Move,
    Resize,
    /// Selecting text with the pointer.
    Select,
    /// A file of a fence, until the pointer moves far enough.
    Item(usize),
}

struct Drag {
    kind: DragKind,
    start: POINT,
    rect: (i32, i32, i32, i32),
    moved: bool,
}

/// A file or folder in a fence.
struct Entry {
    path: PathBuf,
    name: String,
}

#[derive(Debug)]
enum WeatherError {
    NotFound,
    Net(String),
}

/// What the helper threads bring back.
enum Done {
    Weather(u32, String, Result<(Option<Place>, Forecast), WeatherError>),
    Photos(u32, PathBuf, Vec<PathBuf>),
    Photo(u32, PathBuf, Option<tiny_skia::Pixmap>),
    Icons(Vec<(PathBuf, Option<tiny_skia::Pixmap>)>),
}

struct W {
    cfg: Widget,
    hwnd: HWND,
    painted: Painted,
    hover: bool,
    hot: Option<Zone>,
    editor: Option<Editor>,
    caret_on: bool,
    /// When to tell the settings window about the text.
    text_due: Option<Instant>,
    scroll: f32,
    month: (i32, u32),
    drag: Option<Drag>,
    surrogate: Option<u16>,
    dirty: bool,
    // The weather.
    forecast: Option<Forecast>,
    status: Option<String>,
    next_weather: Instant,
    asking: bool,
    // Photos.
    photos: Vec<PathBuf>,
    photo: Option<tiny_skia::Pixmap>,
    photo_path: PathBuf,
    next_photo: Instant,
    next_list: Instant,
    loading: bool,
    // Fences.
    entries: Vec<Entry>,
    items: Vec<Item>,
    selected: Option<usize>,
    signature: u64,
    drop: Option<IDropTarget>,
    drop_folder: Rc<RefCell<PathBuf>>,
}

impl W {
    fn new(cfg: Widget, now: Instant) -> W {
        let t = rt_sys::local_time();
        W {
            cfg,
            hwnd: HWND::default(),
            painted: Painted::default(),
            hover: false,
            hot: None,
            editor: None,
            caret_on: true,
            text_due: None,
            scroll: 0.0,
            month: (t.year, t.month),
            drag: None,
            surrogate: None,
            dirty: true,
            forecast: None,
            status: None,
            next_weather: now,
            asking: false,
            photos: Vec::new(),
            photo: None,
            photo_path: PathBuf::new(),
            next_photo: now,
            next_list: now,
            loading: false,
            entries: Vec::new(),
            items: Vec::new(),
            selected: None,
            signature: 0,
            drop: None,
            drop_folder: Rc::new(RefCell::new(PathBuf::new())),
        }
    }

    fn set_pointer(&self, p: Pointer) {
        POINTER.with(|m| m.borrow_mut().insert(key(self.hwnd), p));
    }

    fn rect(&self) -> RECT {
        RECT { left: self.cfg.x, top: self.cfg.y, right: self.cfg.x + self.cfg.w, bottom: self.cfg.y + self.cfg.h }
    }
}

// ---------------------------------------------------------------- the thread

struct Host2<'a> {
    gfx: Gfx,
    ws: Vec<W>,
    style: Style,
    owner: Option<HWND>,
    control: HWND,
    report: &'a dyn Fn(Event),
    waker: Waker,
    done_tx: Sender<Done>,
    icons: HashMap<PathBuf, Option<tiny_skia::Pixmap>>,
    asked_icons: HashSet<PathBuf>,
    last_second: u64,
    last_minute: u64,
    next_check: Instant,
    next_blink: Instant,
    seed: u64,
}

fn run(rx: Receiver<Cmd>, report: &dyn Fn(Event), waker: Waker) {
    // SAFETY: thread setup; the control window is this thread's.
    let control = unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = OleInitialize(None);
        register_classes();
        let inst = GetModuleHandleW(None).unwrap_or_default();
        CreateWindowExW(WINDOW_EX_STYLE(0), CONTROL_CLASS, w!("rwall widgets"), WS_POPUP, 0, 0, 0, 0, None, None, Some(inst.into()), None).unwrap_or_default()
    };
    let Ok(gfx) = Gfx::new() else { return };
    let (done_tx, done_rx) = channel();
    let now = Instant::now();
    let mut h = Host2 {
        gfx,
        ws: Vec::new(),
        style: Style { dark: true, accent: Color::rgb(0x2F7DE1), lang: Lang::EnUS },
        owner: find_owner(true),
        control,
        report,
        waker,
        done_tx,
        icons: HashMap::new(),
        asked_icons: HashSet::new(),
        last_second: 0,
        last_minute: 0,
        next_check: now,
        next_blink: now,
        seed: SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1,
    };
    let mut msg = MSG::default();
    loop {
        let wait = h.wait_ms();
        // SAFETY: waits for this thread's messages; the loop dispatches them
        // to this thread's windows.
        unsafe {
            MsgWaitForMultipleObjects(None, false, wait, QS_ALLINPUT);
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
                h.events();
            }
        }
        h.events();
        while let Ok(c) = rx.try_recv() {
            match c {
                Cmd::Set(list, style) => h.set(list, style),
                Cmd::Quit => {
                    h.close_all();
                    // SAFETY: this thread's window.
                    let _ = unsafe { DestroyWindow(control) };
                    return;
                }
            }
        }
        while let Ok(d) = done_rx.try_recv() {
            h.done(d);
        }
        h.tick();
        h.paint_dirty();
    }
}

/// Clears what the thread left (after it ends, or panics).
fn cleanup() {
    QUEUE.with(|q| q.borrow_mut().clear());
    PINNED.with(|s| s.borrow_mut().clear());
    EDITABLE.with(|s| s.borrow_mut().clear());
    // SAFETY: pairs the OleInitialize of this thread.
    unsafe { OleUninitialize() };
}

/// The owner of pinned widgets; `layer`: make sure the layer behind the
/// icons exists first (so the icons do not move later).
fn find_owner(layer: bool) -> Option<HWND> {
    if layer {
        let _ = desktop::find_host();
    }
    desktop::icons_window()
}

impl Host2<'_> {
    fn send(&self, e: Event) {
        (self.report)(e);
    }

    fn find(&self, h: isize) -> Option<usize> {
        self.ws.iter().position(|w| key(w.hwnd) == h)
    }

    fn look(&self, i: usize) -> Look {
        Look { dark: self.style.dark, accent: self.style.accent, s: dpi_scale(self.ws[i].hwnd) }
    }

    /// Milliseconds until something is due.
    fn wait_ms(&self) -> u32 {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        let mut ms = 1000 - now.subsec_millis() + 5;
        if self.ws.iter().any(|w| w.editor.is_some()) {
            ms = ms.min(self.next_blink.saturating_duration_since(Instant::now()).as_millis() as u32 + 1);
        }
        ms.clamp(1, 1000)
    }

    fn random(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    // ------------------------------------------------------------ windows

    fn create(&mut self, i: usize) {
        let cfg = &self.ws[i].cfg;
        let mut ex = WS_EX_LAYERED | WS_EX_TOOLWINDOW;
        if cfg.on_top {
            ex |= WS_EX_TOPMOST;
        }
        if !cfg.kind.editable() {
            ex |= WS_EX_NOACTIVATE;
        }
        let owner = if cfg.on_top { None } else { self.owner };
        // SAFETY: the class was registered by this thread.
        let hwnd = unsafe {
            let inst = GetModuleHandleW(None).unwrap_or_default();
            CreateWindowExW(ex, WIDGET_CLASS, w!("rwall"), WS_POPUP, cfg.x, cfg.y, cfg.w.max(1), cfg.h.max(1), owner, None, Some(inst.into()), None)
        };
        let Ok(hwnd) = hwnd else { return };
        if !cfg.on_top {
            PINNED.with(|s| s.borrow_mut().insert(key(hwnd)));
        }
        if cfg.kind.editable() {
            EDITABLE.with(|s| s.borrow_mut().insert(key(hwnd)));
        }
        let w = &mut self.ws[i];
        w.hwnd = hwnd;
        w.dirty = true;
        if w.cfg.kind == WKind::Fence {
            *w.drop_folder.borrow_mut() = w.cfg.folder.clone();
            let target: IDropTarget = FenceDrop { folder: w.drop_folder.clone(), files: RefCell::new(Vec::new()), waker: self.waker }.into();
            // SAFETY: a window of this OLE thread.
            if unsafe { RegisterDragDrop(hwnd, &target) }.is_ok() {
                w.drop = Some(target);
            }
        }
        self.render(i);
        // SAFETY: a window of this thread.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            if !self.ws[i].cfg.on_top {
                // Just above the icons: under every other window.
                with_z(|| SetWindowPos(hwnd, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)).ok();
            }
        }
    }

    fn destroy(&mut self, i: usize) {
        let w = &mut self.ws[i];
        if w.hwnd.is_invalid() {
            return;
        }
        let k = key(w.hwnd);
        // SAFETY: a window of this thread.
        unsafe {
            if w.drop.take().is_some() {
                let _ = RevokeDragDrop(w.hwnd);
            }
            let _ = DestroyWindow(w.hwnd);
        }
        PINNED.with(|s| s.borrow_mut().remove(&k));
        EDITABLE.with(|s| s.borrow_mut().remove(&k));
        POINTER.with(|m| m.borrow_mut().remove(&k));
        w.hwnd = HWND::default();
    }

    fn close_all(&mut self) {
        for i in 0..self.ws.len() {
            self.commit_text(i);
            self.destroy(i);
        }
        self.ws.clear();
    }

    /// Puts a widget not placed yet on the main screen: the first free place
    /// from the top right corner.
    fn place(&mut self, i: usize, nth: i32) {
        let wa = work_area(POINT { x: 0, y: 0 });
        let s = {
            // SAFETY: plain query.
            let d = unsafe { GetDpiForSystem() };
            if d == 0 {
                1.0
            } else {
                d as f32 / 96.0
            }
        };
        let (dw, dh) = self.ws[i].cfg.kind.default_size();
        let (w, h) = ((dw as f32 * s).round() as i32, (dh as f32 * s).round() as i32);
        // The others, a little larger, so the new one leaves a gap.
        let gap = (12.0 * s) as i32;
        let others: Vec<RECT> = self
            .ws
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != i && o.cfg.placed())
            .map(|(_, o)| RECT { left: o.cfg.x - gap, top: o.cfg.y - gap, right: o.cfg.x + o.cfg.w + gap, bottom: o.cfg.y + o.cfg.h + gap })
            .collect();
        let free = |x: i32, y: i32| others.iter().all(|o| x >= o.right || x + w <= o.left || y >= o.bottom || y + h <= o.top);
        // From the top right corner, down each column, then the column to
        // the left: the first place that is free.
        let margin = (24.0 * s) as i32;
        let step = (16.0 * s).max(8.0) as i32;
        let mut spot = None;
        let mut x = wa.right - w - margin;
        'search: while x >= wa.left {
            let mut y = wa.top + margin;
            while y + h <= wa.bottom {
                if free(x, y) {
                    spot = Some((x, y));
                    break 'search;
                }
                y += step;
            }
            x -= step;
        }
        // No room: one under the other, a little to the side.
        let (x, y) = spot.unwrap_or_else(|| {
            let o = (28.0 * s) as i32 * nth;
            ((wa.right - w - margin - o).max(wa.left), (wa.top + margin + o).min(wa.bottom - h).max(wa.top))
        });
        let cfg = &mut self.ws[i].cfg;
        (cfg.x, cfg.y, cfg.w, cfg.h) = (x, y, w, h);
    }

    /// Brings a widget left outside every screen back onto the main one.
    fn keep_on_screen(&mut self, i: usize) -> bool {
        if on_a_screen(&self.ws[i].rect()) {
            return false;
        }
        let wa = work_area(POINT { x: 0, y: 0 });
        let cfg = &mut self.ws[i].cfg;
        cfg.x = (wa.right - cfg.w - 24).max(wa.left);
        cfg.y = wa.top + 24;
        true
    }

    // ------------------------------------------------------------ commands

    fn set(&mut self, list: Vec<Widget>, style: Style) {
        if style != self.style {
            self.style = style;
            i18n::set_lang(style.lang);
            self.gfx.clear_cache();
            self.ws.iter_mut().for_each(|w| w.dirty = true);
        }
        // Gone.
        let ids: HashSet<u32> = list.iter().map(|w| w.id).collect();
        let mut i = 0;
        while i < self.ws.len() {
            if ids.contains(&self.ws[i].cfg.id) {
                i += 1;
            } else {
                self.destroy(i);
                self.ws.remove(i);
            }
        }
        let now = Instant::now();
        let mut placed = 0;
        for cfg in list {
            match self.ws.iter().position(|w| w.cfg.id == cfg.id) {
                Some(i) => {
                    let w = &mut self.ws[i];
                    let old = w.cfg.clone();
                    let mut cfg = cfg;
                    if w.editor.is_some() {
                        cfg.text = old.text.clone();
                    }
                    if w.drag.is_some() || !cfg.placed() {
                        (cfg.x, cfg.y, cfg.w, cfg.h) = (old.x, old.y, old.w, old.h);
                    }
                    if cfg == old {
                        continue;
                    }
                    let recreate = cfg.on_top != old.on_top || cfg.kind != old.kind;
                    if cfg.folder != old.folder {
                        w.entries.clear();
                        w.items.clear();
                        w.selected = None;
                        w.signature = 0;
                        w.scroll = 0.0;
                        w.photos.clear();
                        w.photo = None;
                        w.status = None;
                        w.next_list = now;
                        w.next_photo = now;
                        *w.drop_folder.borrow_mut() = cfg.folder.clone();
                    }
                    if cfg.city != old.city || cfg.fahrenheit != old.fahrenheit || (cfg.place.is_empty() && !old.place.is_empty()) {
                        w.forecast = None;
                        w.status = None;
                        w.next_weather = now;
                    }
                    if cfg.minutes != old.minutes {
                        w.next_photo = now + Duration::from_secs(u64::from(cfg.minutes) * 60);
                    }
                    w.cfg = cfg;
                    w.dirty = true;
                    if recreate {
                        self.destroy(i);
                        self.create(i);
                    }
                }
                None => {
                    self.ws.push(W::new(cfg, now));
                    let i = self.ws.len() - 1;
                    let fresh = !self.ws[i].cfg.placed();
                    if fresh {
                        self.place(i, placed);
                        placed += 1;
                    }
                    let moved = self.keep_on_screen(i);
                    self.create(i);
                    if fresh || moved {
                        self.send(Event::Changed(self.ws[i].cfg.clone()));
                    }
                }
            }
        }
    }

    fn done(&mut self, d: Done) {
        let now = Instant::now();
        match d {
            Done::Weather(id, city, result) => {
                let Some(i) = self.ws.iter().position(|w| w.cfg.id == id) else { return };
                let w = &mut self.ws[i];
                w.asking = false;
                if w.cfg.city.trim() != city {
                    return;
                }
                match result {
                    Ok((place, fc)) => {
                        w.forecast = Some(fc);
                        w.status = None;
                        w.next_weather = now + Duration::from_secs(weather::REFRESH_MINUTES * 60);
                        if let Some(p) = place {
                            w.cfg.place = p.name;
                            w.cfg.lat = p.lat;
                            w.cfg.lon = p.lon;
                            let cfg = w.cfg.clone();
                            self.send(Event::Changed(cfg));
                        }
                        self.send(Event::Status(id, None));
                    }
                    Err(e) => {
                        let msg = match e {
                            WeatherError::NotFound => tf("City not found: {}", &[&city]),
                            WeatherError::Net(e) => tf("The weather could not be read: {}", &[&e]),
                        };
                        w.status = Some(msg.clone());
                        w.next_weather = now + Duration::from_secs(5 * 60);
                        self.send(Event::Status(id, Some(msg)));
                    }
                }
                self.ws[i].dirty = true;
            }
            Done::Photos(id, folder, list) => {
                let Some(w) = self.ws.iter_mut().find(|w| w.cfg.id == id) else { return };
                w.loading = false;
                if w.cfg.folder != folder {
                    return;
                }
                w.status = list.is_empty().then(|| tr("There are no pictures in the folder."));
                w.photos = list;
                w.next_photo = now;
                w.dirty = true;
            }
            Done::Photo(id, path, pix) => {
                let Some(w) = self.ws.iter_mut().find(|w| w.cfg.id == id) else { return };
                w.loading = false;
                if let Some(p) = pix {
                    w.photo = Some(p);
                    w.photo_path = path;
                    w.dirty = true;
                }
            }
            Done::Icons(list) => {
                for (p, pix) in list {
                    self.icons.insert(p, pix);
                }
                for i in 0..self.ws.len() {
                    if self.ws[i].cfg.kind == WKind::Fence {
                        self.rebuild_items(i);
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ time

    // ------------------------------------------------------------ painting

    // ------------------------------------------------------------ input

    // ------------------------------------------------------------ keyboard

    // ------------------------------------------------------------ the menu

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops() {
        let none = MODIFIERKEYS_FLAGS(0);
        let f = |s: &str| PathBuf::from(s);
        assert_eq!(drop_effect(&[f(r"C:\Users\a\Desktop\x.txt")], &f(r"C:\Users\a\Box"), none), DROPEFFECT_MOVE);
        assert_eq!(drop_effect(&[f(r"D:\x.txt")], &f(r"c:\Box"), none), DROPEFFECT_COPY);
        assert_eq!(drop_effect(&[f(r"C:\x.txt")], &f(r"C:\Box"), MK_CONTROL), DROPEFFECT_COPY);
        assert_eq!(drop_effect(&[f(r"D:\x.txt")], &f(r"C:\Box"), MK_SHIFT), DROPEFFECT_MOVE);
        assert_eq!(drop_effect(&[f(r"C:\Box\x.txt")], &f(r"C:\Box"), none), DROPEFFECT_NONE, "already there");
        assert_eq!(drop_effect(&[f(r"C:\Box")], &f(r"C:\Box\Inner"), none), DROPEFFECT_NONE, "into itself");
        assert_eq!(drop_effect(&[], &f(r"C:\Box"), none), DROPEFFECT_NONE);
        assert_eq!(drop_effect(&[f(r"C:\x")], &f(""), none), DROPEFFECT_NONE);
    }

    /// Many widgets at once: how long the thread takes to answer
    /// (`cargo test -p rwall --lib many_widgets -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn many_widgets() {
        use std::sync::{Arc, Mutex};
        let got: Arc<Mutex<Vec<(Instant, String)>>> = Arc::default();
        let g2 = got.clone();
        let host = Host::start(Box::new(move |e| {
            let s = match e {
                Event::Changed(w) => format!("changed {}", w.id),
                Event::Status(id, s) => format!("status {id} {s:?}"),
                _ => "other".into(),
            };
            g2.lock().unwrap().push((Instant::now(), s));
        }))
        .unwrap();
        let style = Style { dark: true, accent: Color::rgb(0x2F7DE1), lang: Lang::EnUS };
        let mut list = Vec::new();
        let t0 = Instant::now();
        for n in 1..=24u32 {
            let kind = WKind::ALL[(n as usize - 1) % WKind::ALL.len()];
            let mut w = Widget::new(n, kind);
            w.folder = std::env::current_dir().unwrap();
            list.push(w);
            let sent = Instant::now();
            host.set(list.clone(), style);
            loop {
                std::thread::sleep(Duration::from_millis(5));
                let g = got.lock().unwrap();
                if g.iter().any(|(_, s)| *s == format!("changed {n}")) || sent.elapsed() > Duration::from_secs(10) {
                    break;
                }
            }
            println!("{n:2} {:?}: {:?}", kind, sent.elapsed());
        }
        println!("all: {:?}", t0.elapsed());
        std::thread::sleep(Duration::from_secs(3));
        drop(host);
    }

    /// A pinned window ends up above the icons and below the other windows
    /// (`cargo test -p rwall --lib pinned_window_order -- --ignored --nocapture`).
    /// The window is invisible (a layered window that is never drawn).
    #[test]
    #[ignore]
    fn pinned_window_order() {
        register_classes();
        let owner = find_owner(true).expect("the desktop icons' window");
        // SAFETY: test window of this thread, destroyed at the end.
        unsafe {
            let inst = GetModuleHandleW(None).unwrap_or_default();
            let h = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, WIDGET_CLASS, w!("rwall test"), WS_POPUP, 0, 0, 10, 10, Some(owner), None, Some(inst.into()), None).unwrap();
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
            with_z(|| SetWindowPos(h, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE)).unwrap();
            let mut below = Vec::new();
            let mut n = GetWindow(h, GW_HWNDNEXT).unwrap_or_default();
            while !n.is_invalid() && below.len() < 400 {
                let mut buf = [0u16; 64];
                let len = GetClassNameW(n, &mut buf);
                below.push((n, String::from_utf16_lossy(&buf[..len.max(0) as usize])));
                n = GetWindow(n, GW_HWNDNEXT).unwrap_or_default();
            }
            let mut above = 0;
            let mut p = GetWindow(h, GW_HWNDPREV).unwrap_or_default();
            while !p.is_invalid() && above < 2000 {
                above += 1;
                p = GetWindow(p, GW_HWNDPREV).unwrap_or_default();
            }
            println!("owner {owner:?}; {above} windows above; below: {:?}", below.iter().take(8).map(|b| &b.1).collect::<Vec<_>>());
            let owner_below = below.iter().position(|b| b.0 == owner);
            let _ = DestroyWindow(h);
            assert!(owner_below.is_some(), "the icons' window must be under the widget");
            assert!(above > 0, "other windows stay above");
        }
    }
}
