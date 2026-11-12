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

// ---------------------------------------------------------------- drag and drop

/// The files named by a dragged data object.
fn dropped_files(d: &IDataObject) -> Vec<PathBuf> {
    let fmt = FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
    let mut out = Vec::new();
    // SAFETY: the medium is released after its list is read; the buffers
    // are as long as said.
    unsafe {
        let Ok(mut med) = d.GetData(&fmt) else { return out };
        let drop = HDROP(med.u.hGlobal.0);
        let n = DragQueryFileW(drop, u32::MAX, None);
        for i in 0..n.min(10_000) {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
            out.push(PathBuf::from(std::ffi::OsString::from_wide(&buf[..got.min(len)])));
        }
        ReleaseStgMedium(&mut med);
    }
    out
}

fn same_volume(a: &Path, b: &Path) -> bool {
    match (a.components().next(), b.components().next()) {
        (Some(x), Some(y)) => x.as_os_str().to_string_lossy().eq_ignore_ascii_case(&y.as_os_str().to_string_lossy()),
        _ => false,
    }
}

/// What dropping `files` into `folder` does: move on the same drive, copy
/// from another (Ctrl copies, Shift moves), nothing for what is there already.
fn drop_effect(files: &[PathBuf], folder: &Path, keys: MODIFIERKEYS_FLAGS) -> DROPEFFECT {
    if folder.as_os_str().is_empty() || files.is_empty() || files.iter().all(|f| f.parent() == Some(folder)) || files.iter().any(|f| folder.starts_with(f)) {
        return DROPEFFECT_NONE;
    }
    if keys.0 & MK_CONTROL.0 != 0 {
        DROPEFFECT_COPY
    } else if keys.0 & MK_SHIFT.0 != 0 || files.iter().all(|f| same_volume(f, folder)) {
        DROPEFFECT_MOVE
    } else {
        DROPEFFECT_COPY
    }
}

/// Copies or moves `files` into `folder` with Explorer's own dialogs (and
/// undo), on a thread of its own.
fn file_operation(files: Vec<PathBuf>, folder: PathBuf, effect: DROPEFFECT, waker: Waker) {
    std::thread::spawn(move || {
        let mut from: Vec<u16> = Vec::new();
        for f in &files {
            from.extend(f.as_os_str().encode_wide());
            from.push(0);
        }
        from.push(0);
        let mut to: Vec<u16> = folder.as_os_str().encode_wide().collect();
        to.extend([0, 0]);
        let mut op = SHFILEOPSTRUCTW {
            wFunc: if effect == DROPEFFECT_MOVE { FO_MOVE } else { FO_COPY },
            pFrom: PCWSTR(from.as_ptr()),
            pTo: PCWSTR(to.as_ptr()),
            fFlags: FOF_ALLOWUNDO.0 as u16,
            ..Default::default()
        };
        // SAFETY: both lists end with two zeros and outlive the call.
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            SHFileOperationW(&mut op);
        }
        waker.wake();
    });
}


/// A fence takes files dropped on it.
#[implement(IDropTarget)]
struct FenceDrop {
    /// Shared with the widget, which changes it.
    folder: Rc<RefCell<PathBuf>>,
    files: RefCell<Vec<PathBuf>>,
    waker: Waker,
}

impl IDropTarget_Impl for FenceDrop_Impl {
    fn DragEnter(&self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        let files = data.as_ref().map(dropped_files).unwrap_or_default();
        let e = drop_effect(&files, &self.folder.borrow(), keys);
        *self.files.borrow_mut() = files;
        // SAFETY: OLE passes a valid pointer.
        unsafe { *effect = e };
        Ok(())
    }

    fn DragOver(&self, keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        let e = drop_effect(&self.files.borrow(), &self.folder.borrow(), keys);
        // SAFETY: OLE passes a valid pointer.
        unsafe { *effect = e };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.files.borrow_mut().clear();
        Ok(())
    }

    fn Drop(&self, data: Ref<IDataObject>, keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        let files = data.as_ref().map(dropped_files).unwrap_or_default();
        let folder = self.folder.borrow().clone();
        let e = drop_effect(&files, &folder, keys);
        if e != DROPEFFECT_NONE {
            file_operation(files, folder, e, self.waker);
        }
        // A move done here is not for the source to finish (it would
        // delete what it moved): it is told nothing happened.
        // SAFETY: OLE passes a valid pointer.
        unsafe { *effect = if e == DROPEFFECT_MOVE { DROPEFFECT_NONE } else { e } };
        self.files.borrow_mut().clear();
        Ok(())
    }
}

/// Dragging a file out of a fence.
#[implement(IDropSource)]
struct FenceSource;

impl IDropSource_Impl for FenceSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if keys.0 & MK_LBUTTON.0 == 0 {
            DRAGDROP_S_DROP
        } else {
            HRESULT(0)
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

fn drag_out(path: &Path) {
    // SAFETY: COM calls on objects made here, on this OLE thread.
    unsafe {
        let _ = ReleaseCapture();
        let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(path.as_os_str()), None) else { return };
        let Ok(data) = item.BindToHandler::<_, IDataObject>(None::<&IBindCtx>, &BHID_DataObject) else { return };
        let source: IDropSource = FenceSource.into();
        let mut effect = DROPEFFECT_NONE;
        let _ = DoDragDrop(&data, &source, DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK, &mut effect);
    }
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

    fn tick(&mut self) {
        let now = Instant::now();
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        if secs != self.last_second {
            self.last_second = secs;
            let minute = secs / 60;
            let new_minute = minute != self.last_minute;
            self.last_minute = minute;
            for w in &mut self.ws {
                let k = w.cfg.kind;
                if (k.has_clock() && (w.cfg.seconds || new_minute)) || (new_minute && k == WKind::Calendar) {
                    w.dirty = true;
                }
            }
        }
        if now >= self.next_blink {
            self.next_blink = now + Duration::from_millis(530);
            for w in self.ws.iter_mut().filter(|w| w.editor.is_some()) {
                w.caret_on = !w.caret_on;
                w.dirty = true;
            }
        }
        for i in 0..self.ws.len() {
            if self.ws[i].text_due.is_some_and(|t| now >= t) {
                self.ws[i].text_due = None;
                if let Some(e) = &self.ws[i].editor {
                    self.ws[i].cfg.text = e.text.clone();
                }
                self.send(Event::Changed(self.ws[i].cfg.clone()));
            }
            match self.ws[i].cfg.kind {
                WKind::Weather => self.weather_due(i, now),
                WKind::Photos => self.photos_due(i, now),
                _ => {}
            }
        }
        if RESET.with(|c| c.replace(false)) {
            self.owner = find_owner(true);
            self.recreate_pinned();
        }
        if LAYOUT.with(|c| c.replace(false)) {
            for i in 0..self.ws.len() {
                if self.keep_on_screen(i) {
                    self.send(Event::Changed(self.ws[i].cfg.clone()));
                }
                self.ws[i].dirty = true;
            }
        }
        if now >= self.next_check {
            self.next_check = now + Duration::from_secs(2);
            // The icons may have moved to another window (Explorer, or the
            // dynamic wallpaper making its layer).
            let owner = find_owner(false);
            if owner.is_some() && owner != self.owner {
                self.owner = owner;
                self.recreate_pinned();
            }
            for i in 0..self.ws.len() {
                // SAFETY: plain query.
                if !unsafe { IsWindow(Some(self.ws[i].hwnd)) }.as_bool() {
                    self.destroy(i);
                    self.create(i);
                }
                if self.ws[i].cfg.kind == WKind::Fence {
                    self.scan_fence(i);
                }
            }
        }
    }

    fn recreate_pinned(&mut self) {
        for i in 0..self.ws.len() {
            if !self.ws[i].cfg.on_top {
                self.commit_text(i);
                self.destroy(i);
                self.create(i);
            }
        }
    }

    fn weather_due(&mut self, i: usize, now: Instant) {
        let w = &mut self.ws[i];
        let city = w.cfg.city.trim().to_string();
        if city.is_empty() || w.asking || now < w.next_weather {
            return;
        }
        w.asking = true;
        if w.forecast.is_none() {
            w.status = Some(tr("Looking for the weather…"));
            w.dirty = true;
        }
        let (id, known, lat, lon, f) = (w.cfg.id, w.cfg.has_place(), w.cfg.lat, w.cfg.lon, w.cfg.fahrenheit);
        let lang = self.style.lang.tag().split('-').next().unwrap_or("en").to_string();
        let (tx, waker) = (self.done_tx.clone(), self.waker);
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                let place = if known {
                    None
                } else {
                    match weather::find(&city, &lang) {
                        Ok(Some(p)) => Some(p),
                        Ok(None) => return Err(WeatherError::NotFound),
                        Err(e) => return Err(WeatherError::Net(e)),
                    }
                };
                let (lat, lon) = place.as_ref().map_or((lat, lon), |p| (p.lat, p.lon));
                weather::fetch(lat, lon, f).map(|fc| (place, fc)).map_err(WeatherError::Net)
            })
            .unwrap_or_else(|_| Err(WeatherError::Net("unexpected answer".into())));
            let _ = tx.send(Done::Weather(id, city, result));
            waker.wake();
        });
    }

    fn photos_due(&mut self, i: usize, now: Instant) {
        let folder = self.ws[i].cfg.folder.clone();
        if folder.as_os_str().is_empty() || self.ws[i].loading {
            return;
        }
        let (tx, waker, id) = (self.done_tx.clone(), self.waker, self.ws[i].cfg.id);
        if now >= self.ws[i].next_list {
            let w = &mut self.ws[i];
            w.loading = true;
            w.next_list = now + Duration::from_secs(30 * 60);
            std::thread::spawn(move || {
                let list = crate::sources::folders::list_with(std::slice::from_ref(&folder), true, crate::picture::has_picture_ext);
                let _ = tx.send(Done::Photos(id, folder, list));
                waker.wake();
            });
            return;
        }
        if now < self.ws[i].next_photo || self.ws[i].photos.is_empty() {
            return;
        }
        let n = self.ws[i].photos.len();
        let mut k = (self.random() % n as u64) as usize;
        if n > 1 && self.ws[i].photos[k] == self.ws[i].photo_path {
            k = (k + 1) % n;
        }
        let w = &mut self.ws[i];
        let path = w.photos[k].clone();
        w.loading = true;
        w.next_photo = now + Duration::from_secs(u64::from(w.cfg.minutes.max(1)) * 60);
        let (pw, ph) = (((w.cfg.w * 2) as u32).clamp(64, 1600), ((w.cfg.h * 2) as u32).clamp(64, 1600));
        std::thread::spawn(move || {
            let pix = std::panic::catch_unwind(|| crate::picture::thumbnail(&path, pw, ph)).ok().flatten();
            let _ = tx.send(Done::Photo(id, path, pix));
            waker.wake();
        });
    }

    /// Reads a fence's folder again; asks for the icons it has not got.
    fn scan_fence(&mut self, i: usize) {
        let folder = self.ws[i].cfg.folder.clone();
        if folder.as_os_str().is_empty() {
            return;
        }
        let Ok(rd) = std::fs::read_dir(&folder) else {
            let msg = tr("The folder cannot be read.");
            let w = &mut self.ws[i];
            if w.status.as_deref() != Some(msg.as_str()) {
                w.status = Some(msg);
                w.entries.clear();
                w.items.clear();
                w.dirty = true;
            }
            return;
        };
        // Folders first, then by name; (is a file, sort key, name, path, stamp).
        let mut found: Vec<(bool, String, String, PathBuf, u64)> = Vec::new();
        for e in rd.flatten().take(2000) {
            let Ok(m) = e.metadata() else { continue };
            // Hidden and system files stay hidden, as on the desktop.
            if m.file_attributes() & 0x6 != 0 {
                continue;
            }
            let path = e.path();
            let is_dir = m.is_dir();
            let ext = path.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            // Shortcuts show without their extension, as Explorer shows them.
            let name = if !is_dir && (ext == "lnk" || ext == "url") {
                path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            } else {
                e.file_name().to_string_lossy().to_string()
            };
            let stamp = m.last_write_time() ^ m.len().rotate_left(17);
            found.push((!is_dir, name.to_lowercase(), name, path, stamp));
        }
        found.sort();
        let sig = found.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, (d, k, _, _, st)| {
            let h = k.bytes().fold(h, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
            (h ^ st ^ u64::from(*d)).wrapping_mul(0x0100_0000_01b3)
        });
        let w = &mut self.ws[i];
        if w.status.is_some() {
            w.status = None;
            w.dirty = true;
        }
        if sig == w.signature && !w.entries.is_empty() {
            return;
        }
        w.signature = sig;
        w.entries = found.into_iter().map(|(_, _, name, path, _)| Entry { path, name }).collect();
        let wanted: Vec<PathBuf> = w.entries.iter().map(|e| e.path.clone()).filter(|p| !self.asked_icons.contains(p)).collect();
        if !wanted.is_empty() {
            self.asked_icons.extend(wanted.iter().cloned());
            let px = (40.0 * dpi_scale(self.ws[i].hwnd)).round() as u32;
            let (tx, waker) = (self.done_tx.clone(), self.waker);
            std::thread::spawn(move || {
                for chunk in wanted.chunks(16) {
                    let got: Vec<(PathBuf, Option<tiny_skia::Pixmap>)> = chunk.iter().map(|p| (p.clone(), crate::live::thumb::shell_image(p, px))).collect();
                    let _ = tx.send(Done::Icons(got));
                    waker.wake();
                }
            });
        }
        self.rebuild_items(i);
    }

    fn rebuild_items(&mut self, i: usize) {
        let w = &mut self.ws[i];
        w.items = w.entries.iter().map(|e| Item { name: e.name.clone(), icon: self.icons.get(&e.path).cloned().flatten() }).collect();
        if w.selected.is_some_and(|s| s >= w.items.len()) {
            w.selected = None;
        }
        w.dirty = true;
    }

    // ------------------------------------------------------------ painting

    fn paint_dirty(&mut self) {
        for i in 0..self.ws.len() {
            if self.ws[i].dirty && !self.ws[i].hwnd.is_invalid() {
                self.render(i);
            }
        }
    }

    fn render(&mut self, i: usize) {
        let look = self.look(i);
        let now = rt_sys::local_time();
        for _ in 0..3 {
            let w = &self.ws[i];
            let caption = w.photo_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let view = View {
                now,
                hover: w.hover,
                hot: w.hot,
                editor: w.editor.as_ref(),
                caret_on: w.caret_on,
                month: w.month,
                country: &super::region::region().country,
                weather: w.forecast.as_ref(),
                status: w.status.as_deref(),
                photo: w.photo.as_ref(),
                caption: &caption,
                items: &w.items,
                selected: w.selected,
                scroll: w.scroll,
            };
            self.gfx.resize(w.cfg.w.max(1) as u32, w.cfg.h.max(1) as u32);
            let painted = paint::paint(&mut self.gfx, &w.cfg, &view, &look);
            let w = &mut self.ws[i];
            w.painted = painted;
            // Annotations grow to hold their text.
            if w.cfg.kind == WKind::Annotation {
                let need = (w.painted.content_h + 12.0 * look.s).ceil() as i32;
                if need > w.cfg.h && w.editor.is_some() {
                    w.cfg.h = need;
                    continue;
                }
            }
            // The caret stays in view.
            if let (Some(e), Some(tb)) = (&w.editor, &w.painted.text) {
                let n = line_of(&tb.lines, e.caret) as f32;
                let (top, bottom) = (n * tb.line_h, (n + 1.0) * tb.line_h);
                let room = tb.rect.h;
                let mut s = w.scroll;
                if top < s {
                    s = top;
                } else if bottom > s + room {
                    s = bottom - room;
                }
                if (s - w.scroll).abs() > 0.5 {
                    w.scroll = s;
                    continue;
                }
            }
            break;
        }
        let w = &mut self.ws[i];
        w.dirty = false;
        present(w.hwnd, &self.gfx.pix, w.cfg.x, w.cfg.y);
    }

    // ------------------------------------------------------------ input

    fn events(&mut self) {
        loop {
            let Some((h, ev)) = QUEUE.with(|q| q.borrow_mut().pop_front()) else { return };
            let Some(i) = self.find(h) else { continue };
            match ev {
                Ev::Down(x, y, dbl) => self.down(i, x as f32, y as f32, dbl),
                Ev::Move(x, y) => self.moved(i, x as f32, y as f32),
                Ev::Up => self.up(i),
                Ev::Menu(x, y) => self.menu(i, x as f32, y as f32),
                Ev::Leave => {
                    let w = &mut self.ws[i];
                    w.hover = false;
                    w.hot = None;
                    w.dirty = true;
                }
                Ev::Wheel(d) => self.wheel(i, d),
                Ev::Char(c) => self.typed(i, c),
                Ev::Key(k) => self.key_down(i, k),
                Ev::Blur => self.stop_editing(i),
                Ev::Dpi => self.ws[i].dirty = true,
            }
        }
    }

    fn start_drag(&mut self, i: usize, kind: DragKind) {
        let w = &mut self.ws[i];
        w.drag = Some(Drag { kind, start: cursor(), rect: (w.cfg.x, w.cfg.y, w.cfg.w, w.cfg.h), moved: false });
        // SAFETY: a window of this thread.
        unsafe { SetCapture(w.hwnd) };
    }

    /// The caret position under (`x`, `y`).
    fn text_index(&mut self, i: usize, x: f32, y: f32) -> Option<usize> {
        let w = &self.ws[i];
        let (e, tb) = (w.editor.as_ref()?, w.painted.text.as_ref()?);
        let line = *tb.lines.get(tb.line_at(y))?;
        Some(e.index_at(&mut self.gfx, &line, x - tb.rect.x, tb.px, tb.font?))
    }

    fn start_editing(&mut self, i: usize) {
        let w = &mut self.ws[i];
        if w.editor.is_none() {
            w.editor = Some(Editor::new(&w.cfg.text));
            w.caret_on = true;
            self.next_blink = Instant::now() + Duration::from_millis(530);
        }
        // SAFETY: a window of this thread; the click made rwall the
        // foreground program.
        unsafe {
            let _ = SetForegroundWindow(w.hwnd);
            let _ = SetFocus(Some(w.hwnd));
        }
        w.dirty = true;
        // Lay the text out for the caret.
        self.render(i);
    }

    /// Keeps what was written.
    fn commit_text(&mut self, i: usize) {
        let w = &mut self.ws[i];
        if let Some(e) = &w.editor {
            if e.text != w.cfg.text || w.text_due.is_some() {
                w.cfg.text = e.text.clone();
                w.text_due = None;
                let cfg = w.cfg.clone();
                self.send(Event::Changed(cfg));
            }
        }
    }

    fn stop_editing(&mut self, i: usize) {
        if self.ws[i].editor.is_none() {
            return;
        }
        self.commit_text(i);
        let w = &mut self.ws[i];
        w.editor = None;
        w.scroll = 0.0;
        w.dirty = true;
    }

    fn down(&mut self, i: usize, x: f32, y: f32, dbl: bool) {
        let w = &self.ws[i];
        let zone = w.painted.zone(x, y).unwrap_or(Zone::Move);
        let (kind, locked) = (w.cfg.kind, w.cfg.locked);
        match zone {
            Zone::Resize if !locked => self.start_drag(i, DragKind::Resize),
            Zone::Prev | Zone::Next | Zone::Today => {
                let w = &mut self.ws[i];
                w.month = match zone {
                    Zone::Prev => super::calendar::add_months(w.month.0, w.month.1, -1),
                    Zone::Next => super::calendar::add_months(w.month.0, w.month.1, 1),
                    _ => {
                        let t = rt_sys::local_time();
                        (t.year, t.month)
                    }
                };
                w.dirty = true;
            }
            Zone::Text => {
                let editing = self.ws[i].editor.is_some();
                if !editing {
                    self.start_editing(i);
                }
                if let Some(at) = self.text_index(i, x, y) {
                    let shift = shift_down();
                    if let Some(e) = self.ws[i].editor.as_mut() {
                        if dbl {
                            e.select_word(at);
                        } else {
                            e.move_to(at, shift);
                            e.goal_x = None;
                        }
                    }
                }
                self.ws[i].caret_on = true;
                self.ws[i].dirty = true;
                if !dbl {
                    self.start_drag(i, DragKind::Select);
                }
            }
            Zone::Item(n) => {
                self.ws[i].selected = Some(n);
                self.ws[i].dirty = true;
                if dbl {
                    if let Some(e) = self.ws[i].entries.get(n) {
                        open(&e.path);
                    }
                } else {
                    self.start_drag(i, DragKind::Item(n));
                }
            }
            _ => {
                if dbl {
                    match kind {
                        WKind::Annotation => {
                            self.start_editing(i);
                            return;
                        }
                        WKind::Photos if !self.ws[i].photo_path.as_os_str().is_empty() => {
                            open(&self.ws[i].photo_path.clone());
                            return;
                        }
                        _ => {}
                    }
                }
                if zone == Zone::Blank && self.ws[i].selected.take().is_some() {
                    self.ws[i].dirty = true;
                }
                if !locked {
                    self.start_drag(i, DragKind::Move);
                }
            }
        }
    }

    fn moved(&mut self, i: usize, x: f32, y: f32) {
        let w = &mut self.ws[i];
        if !w.hover {
            w.hover = true;
            w.dirty = true;
            let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: w.hwnd, dwHoverTime: 0 };
            // SAFETY: a window of this thread.
            let _ = unsafe { TrackMouseEvent(&mut t) };
        }
        let Some(d) = w.drag.as_mut() else {
            let zone = w.painted.zone(x, y).filter(|z| *z != Zone::Resize || !w.cfg.locked);
            if zone != w.hot {
                if matches!(zone, Some(Zone::Prev | Zone::Next | Zone::Today | Zone::Item(_) | Zone::Day(_))) || matches!(w.hot, Some(Zone::Prev | Zone::Next | Zone::Today | Zone::Item(_) | Zone::Day(_))) {
                    w.dirty = true;
                }
                w.hot = zone;
            }
            let p = match zone {
                Some(Zone::Resize) => Pointer::Resize,
                Some(Zone::Text) => Pointer::Text,
                Some(Zone::Prev | Zone::Next | Zone::Today) => Pointer::Hand,
                _ => Pointer::Arrow,
            };
            w.set_pointer(p);
            return;
        };
        let c = cursor();
        let (dx, dy) = (c.x - d.start.x, c.y - d.start.y);
        if !d.moved && dx.abs() + dy.abs() < 4 {
            return;
        }
        d.moved = true;
        let (rx, ry, rw, rh) = d.rect;
        let kind = d.kind;
        match kind {
            DragKind::Move => {
                let (mut nx, mut ny) = (rx + dx, ry + dy);
                // Snaps to the edges of the screen's work area.
                let wa = work_area(POINT { x: nx + rw / 2, y: ny + rh / 2 });
                let snap = 12;
                if (nx - wa.left).abs() < snap {
                    nx = wa.left;
                } else if (nx + rw - wa.right).abs() < snap {
                    nx = wa.right - rw;
                }
                if (ny - wa.top).abs() < snap {
                    ny = wa.top;
                } else if (ny + rh - wa.bottom).abs() < snap {
                    ny = wa.bottom - rh;
                }
                w.cfg.x = nx;
                w.cfg.y = ny;
                w.set_pointer(Pointer::Moving);
                // SAFETY: a window of this thread.
                unsafe {
                    let _ = SetWindowPos(w.hwnd, None, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                }
            }
            DragKind::Resize => {
                let s = dpi_scale(w.hwnd);
                let (mw, mh) = w.cfg.kind.min_size();
                let (mw, mh) = ((mw as f32 * s) as i32, (mh as f32 * s) as i32);
                let (mut nw, mut nh) = ((rw + dx).max(mw), (rh + dy).max(mh));
                if w.cfg.kind.square() {
                    nw = nw.max(nh);
                    nh = nw;
                }
                let limit = (2000.0 * s) as i32;
                w.cfg.w = nw.min(limit);
                w.cfg.h = nh.min(limit);
                w.dirty = true;
                self.render(i);
            }
            DragKind::Select => {
                if let Some(at) = self.text_index(i, x, y) {
                    if let Some(e) = self.ws[i].editor.as_mut() {
                        e.move_to(at, true);
                    }
                    self.ws[i].dirty = true;
                }
            }
            DragKind::Item(n) => {
                w.drag = None;
                if let Some(path) = w.entries.get(n).map(|e| e.path.clone()) {
                    drag_out(&path);
                }
            }
        }
    }

    fn up(&mut self, i: usize) {
        let Some(d) = self.ws[i].drag.take() else { return };
        // SAFETY: releases this thread's capture.
        let _ = unsafe { ReleaseCapture() };
        self.ws[i].set_pointer(Pointer::Arrow);
        if d.moved && matches!(d.kind, DragKind::Move | DragKind::Resize) {
            self.send(Event::Changed(self.ws[i].cfg.clone()));
        }
    }

    fn wheel(&mut self, i: usize, delta: i32) {
        let s = dpi_scale(self.ws[i].hwnd);
        let w = &mut self.ws[i];
        let step = match w.cfg.kind {
            WKind::Fence => CELL.1 * s * 0.5,
            WKind::Note | WKind::Annotation => 20.0 * s,
            _ => return,
        };
        let max = (w.painted.content_h - w.painted.room_h).max(0.0);
        let new = (w.scroll - delta as f32 / 120.0 * step * 1.5).clamp(0.0, max);
        if (new - w.scroll).abs() > 0.1 {
            w.scroll = new;
            w.dirty = true;
        }
    }

    // ------------------------------------------------------------ keyboard

    fn edited(&mut self, i: usize, changed: bool) {
        let w = &mut self.ws[i];
        w.caret_on = true;
        self.next_blink = Instant::now() + Duration::from_millis(530);
        w.dirty = true;
        if changed {
            w.text_due = Some(Instant::now() + Duration::from_secs(1));
        }
    }

    fn typed(&mut self, i: usize, c: u16) {
        let w = &mut self.ws[i];
        let Some(e) = w.editor.as_mut() else { return };
        let text = if (0xD800..0xDC00).contains(&c) {
            w.surrogate = Some(c);
            return;
        } else if (0xDC00..0xE000).contains(&c) {
            match w.surrogate.take() {
                Some(hi) => String::from_utf16_lossy(&[hi, c]),
                None => return,
            }
        } else if c == 13 {
            "\n".to_string()
        } else if c < 32 || c == 127 {
            return;
        } else {
            String::from_utf16_lossy(&[c])
        };
        e.insert(&text);
        self.edited(i, true);
    }

    fn key_down(&mut self, i: usize, k: u16) {
        let (ctrl, shift) = (ctrl_down(), shift_down());
        let vk = VIRTUAL_KEY(k);
        if self.ws[i].editor.is_none() {
            return;
        }
        if matches!(vk, VK_UP | VK_DOWN | VK_HOME | VK_END | VK_PRIOR | VK_NEXT) {
            self.line_move(i, vk, shift, ctrl);
            self.edited(i, false);
            return;
        }
        if vk == VK_ESCAPE {
            self.stop_editing(i);
            return;
        }
        let Some(e) = self.ws[i].editor.as_mut() else { return };
        let changed = match vk {
            VK_BACK => {
                e.backspace(ctrl);
                true
            }
            VK_DELETE => {
                e.delete(ctrl);
                true
            }
            VK_LEFT => {
                e.left(shift, ctrl);
                false
            }
            VK_RIGHT => {
                e.right(shift, ctrl);
                false
            }
            _ if ctrl && k == u16::from(b'A') => {
                e.select_all();
                false
            }
            _ if ctrl && (k == u16::from(b'C') || k == u16::from(b'X')) => {
                if let Some(t) = e.selected_text() {
                    rt_sys::set_clipboard_text(&t.replace('\n', "\r\n"));
                }
                if k == u16::from(b'X') {
                    e.backspace(false);
                    true
                } else {
                    false
                }
            }
            _ if ctrl && k == u16::from(b'V') => {
                if let Some(t) = rt_sys::clipboard_text() {
                    e.insert(&t.replace("\r\n", "\n"));
                }
                true
            }
            _ => return,
        };
        self.edited(i, changed);
    }

    /// Up, Down, Home, End, Page Up and Page Down, along the lines as drawn.
    fn line_move(&mut self, i: usize, vk: VIRTUAL_KEY, shift: bool, ctrl: bool) {
        let w = &self.ws[i];
        let (Some(e), Some(tb)) = (w.editor.as_ref(), w.painted.text.as_ref()) else { return };
        let Some(font) = tb.font else { return };
        if tb.lines.is_empty() {
            return;
        }
        let n = line_of(&tb.lines, e.caret);
        let line = tb.lines[n];
        let goal = e.goal_x.unwrap_or_else(|| self.gfx.measure(&e.text[line.start..e.caret.clamp(line.start, line.end)], tb.px, font));
        let page = ((tb.rect.h / tb.line_h).floor() as usize).max(1);
        let target = match vk {
            VK_HOME if ctrl => Some(0),
            VK_END if ctrl => Some(e.text.len()),
            VK_HOME => Some(line.start),
            VK_END => Some(line.end),
            _ => None,
        };
        let (lines, px) = (tb.lines.clone(), tb.px);
        let at = match target {
            Some(t) => t,
            None => {
                let m = match vk {
                    VK_UP => n.checked_sub(1),
                    VK_DOWN => (n + 1 < lines.len()).then_some(n + 1),
                    VK_PRIOR => Some(n.saturating_sub(page)),
                    _ => Some((n + page).min(lines.len() - 1)),
                };
                match m {
                    Some(m) => {
                        let e = self.ws[i].editor.as_ref().expect("editing");
                        e.index_at(&mut self.gfx, &lines[m], goal, px, font)
                    }
                    None if vk == VK_UP => 0,
                    None => self.ws[i].editor.as_ref().map_or(0, |e| e.text.len()),
                }
            }
        };
        if let Some(e) = self.ws[i].editor.as_mut() {
            e.move_to(at, shift);
            e.goal_x = if matches!(vk, VK_UP | VK_DOWN | VK_PRIOR | VK_NEXT) { Some(goal) } else { None };
        }
    }

    // ------------------------------------------------------------ the menu

    fn menu(&mut self, i: usize, x: f32, y: f32) {
        const NEW_NOTE: u32 = 1;
        const NOTE_COLOR: u32 = 10;
        const TEXT_COLOR: u32 = 20;
        const TEXT_SIZE: u32 = 30;
        const EDIT: u32 = 40;
        const SECONDS: u32 = 41;
        const H24: u32 = 42;
        const REFRESH: u32 = 43;
        const FAHRENHEIT: u32 = 44;
        const OPEN_FOLDER: u32 = 45;
        const NEXT_PHOTO: u32 = 46;
        const SHOW_PHOTO: u32 = 47;
        const OPEN_ITEM: u32 = 48;
        const SHOW_ITEM: u32 = 49;
        const ON_TOP: u32 = 50;
        const LOCK: u32 = 51;
        const SETTINGS: u32 = 52;
        const REMOVE: u32 = 53;

        let item = match self.ws[i].painted.zone(x, y) {
            Some(Zone::Item(n)) => {
                self.ws[i].selected = Some(n);
                self.ws[i].dirty = true;
                self.render(i);
                Some(n)
            }
            _ => None,
        };
        let cfg = self.ws[i].cfg.clone();
        // SAFETY: the menus are made, shown and destroyed here; the texts
        // outlive the calls that copy them.
        let chosen = unsafe {
            let Ok(m) = CreatePopupMenu() else { return };
            let add = |m: HMENU, id: u32, text: &str, checked: bool| {
                let flags = if checked { MF_STRING | MF_CHECKED } else { MF_STRING };
                let _ = AppendMenuW(m, flags, id as usize, &HSTRING::from(text));
            };
            let sep = |m: HMENU| {
                let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
            };
            let sub = |m: HMENU, text: &str, items: &[(u32, String, bool)]| {
                if let Ok(s) = CreatePopupMenu() {
                    for (id, t, c) in items {
                        add(s, *id, t, *c);
                    }
                    let _ = AppendMenuW(m, MF_POPUP, s.0 as usize, &HSTRING::from(text));
                }
            };
            if item.is_some() {
                add(m, OPEN_ITEM, &tr("Open"), false);
                add(m, SHOW_ITEM, &tr("Show the file"), false);
                sep(m);
            }
            match cfg.kind {
                WKind::Note => {
                    add(m, NEW_NOTE, &tr("New note"), false);
                    let colors: Vec<(u32, String, bool)> = NOTE_COLORS.iter().enumerate().map(|(k, (n, _))| (NOTE_COLOR + k as u32, tr(n), cfg.color as usize == k)).collect();
                    sub(m, &tr("Color"), &colors);
                    sep(m);
                }
                WKind::Annotation => {
                    add(m, EDIT, &tr("Edit the text"), false);
                    let sizes: Vec<(u32, String, bool)> = TEXT_SIZES.iter().enumerate().map(|(k, (n, _))| (TEXT_SIZE + k as u32, tr(n), cfg.size as usize == k)).collect();
                    sub(m, &tr("Size"), &sizes);
                    let colors: Vec<(u32, String, bool)> = TEXT_COLORS.iter().enumerate().map(|(k, (n, _))| (TEXT_COLOR + k as u32, tr(n), cfg.color as usize == k)).collect();
                    sub(m, &tr("Color"), &colors);
                    sep(m);
                }
                WKind::Analog | WKind::Digital | WKind::ClockCalendar => {
                    add(m, SECONDS, &tr("Seconds"), cfg.seconds);
                    if cfg.kind != WKind::Analog {
                        add(m, H24, &tr("24-hour clock"), cfg.h24);
                    }
                    sep(m);
                }
                WKind::Weather => {
                    add(m, REFRESH, &tr("Refresh"), false);
                    add(m, FAHRENHEIT, &tr("Fahrenheit (°F)"), cfg.fahrenheit);
                    sep(m);
                }
                WKind::Fence if !cfg.folder.as_os_str().is_empty() => {
                    add(m, OPEN_FOLDER, &tr("Open the folder"), false);
                    sep(m);
                }
                WKind::Photos if !cfg.folder.as_os_str().is_empty() => {
                    add(m, NEXT_PHOTO, &tr("Next photo"), false);
                    if !self.ws[i].photo_path.as_os_str().is_empty() {
                        add(m, SHOW_PHOTO, &tr("Show the file"), false);
                    }
                    add(m, OPEN_FOLDER, &tr("Open the folder"), false);
                    sep(m);
                }
                _ => {}
            }
            add(m, ON_TOP, &tr("Always on top"), cfg.on_top);
            add(m, LOCK, &tr("Lock position"), cfg.locked);
            sep(m);
            add(m, SETTINGS, &tr("Widget settings…"), false);
            add(m, REMOVE, &tr("Remove the widget"), false);
            let p = cursor();
            let _ = SetForegroundWindow(self.control);
            let r = TrackPopupMenuEx(m, (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY).0, p.x, p.y, self.control, None);
            let _ = PostMessageW(Some(self.control), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(m);
            r.0 as u32
        };
        let s = dpi_scale(self.ws[i].hwnd);
        let w = &mut self.ws[i];
        let mut changed = true;
        match chosen {
            0 => changed = false,
            NEW_NOTE => {
                changed = false;
                let (dw, dh) = WKind::Note.default_size();
                let off = (30.0 * s) as i32;
                (self.report)(Event::NewNote { x: w.cfg.x + off, y: w.cfg.y + off, w: (dw as f32 * s) as i32, h: (dh as f32 * s) as i32 });
            }
            c if (NOTE_COLOR..NOTE_COLOR + NOTE_COLORS.len() as u32).contains(&c) => w.cfg.color = (c - NOTE_COLOR) as u8,
            c if (TEXT_COLOR..TEXT_COLOR + TEXT_COLORS.len() as u32).contains(&c) => w.cfg.color = (c - TEXT_COLOR) as u8,
            c if (TEXT_SIZE..TEXT_SIZE + TEXT_SIZES.len() as u32).contains(&c) => w.cfg.size = (c - TEXT_SIZE) as u8,
            EDIT => {
                changed = false;
                self.start_editing(i);
            }
            SECONDS => w.cfg.seconds = !w.cfg.seconds,
            H24 => w.cfg.h24 = !w.cfg.h24,
            REFRESH => {
                changed = false;
                w.next_weather = Instant::now();
            }
            FAHRENHEIT => {
                w.cfg.fahrenheit = !w.cfg.fahrenheit;
                w.forecast = None;
                w.next_weather = Instant::now();
            }
            OPEN_FOLDER => {
                changed = false;
                open(&w.cfg.folder.clone());
            }
            NEXT_PHOTO => {
                changed = false;
                w.next_photo = Instant::now();
            }
            SHOW_PHOTO => {
                changed = false;
                rt_sys::reveal(&w.photo_path);
            }
            OPEN_ITEM | SHOW_ITEM => {
                changed = false;
                if let Some(e) = item.and_then(|n| w.entries.get(n)) {
                    if chosen == OPEN_ITEM {
                        open(&e.path);
                    } else {
                        rt_sys::reveal(&e.path);
                    }
                }
            }
            ON_TOP => {
                w.cfg.on_top = !w.cfg.on_top;
                self.commit_text(i);
                self.destroy(i);
                self.create(i);
            }
            LOCK => w.cfg.locked = !w.cfg.locked,
            SETTINGS => {
                changed = false;
                self.send(Event::Settings);
            }
            REMOVE => {
                changed = false;
                self.commit_text(i);
                let w = &self.ws[i];
                let ask = w.cfg.kind.editable() && !w.cfg.text.trim().is_empty();
                // SAFETY: a plain message box.
                let yes = !ask || unsafe { MessageBoxW(Some(w.hwnd), &HSTRING::from(tr("Remove this widget? What is written in it is lost.")), w!("rwall"), MB_YESNO | MB_ICONQUESTION) } == IDYES;
                if yes {
                    self.send(Event::Remove(w.cfg.id));
                }
            }
            _ => changed = false,
        }
        if changed {
            self.ws[i].dirty = true;
            self.send(Event::Changed(self.ws[i].cfg.clone()));
        }
    }
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
