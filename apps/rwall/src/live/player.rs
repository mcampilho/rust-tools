//! The player: a thread of its own, with one window per screen behind the
//! desktop icons (see `desktop`) and a small hidden one that hears about
//! Explorer restarting, screens changing and the screen going off.
//!
//! Each screen's window has a Direct3D 11 swap chain:
//! - Videos: Media Foundation's media engine decodes on the graphics card and
//!   copies each new frame into every screen's swap chain; no sound, in a loop.
//! - Animated pictures and scenes: frames are made small on the processor and
//!   copied into swap chains of their own size; the graphics card stretches
//!   them to the screen as it shows them, so the processor never scales.
//!
//! Every second the player checks whether it should pause (see
//! [`PauseWhen`]); paused, the last frame stays on the desktop. When it stops,
//! the still wallpaper is painted again.

use super::anim;
use super::desktop::{self, Host, Layout};
use super::scene::Painter;
use super::{compose, composed_size, place, LiveFit, PauseWhen, Px, Reason, Show, Spec, State};
use std::cell::Cell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::{implement, w, Interface, BSTR, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HMODULE, HWND, LPARAM, LRESULT, RECT, S_OK, WPARAM};
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, GetStockObject, BLACK_BRUSH, HBRUSH, PAINTSTRUCT};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{RegisterPowerSettingNotification, UnregisterPowerSettingNotification, POWERBROADCAST_SETTING};
use windows::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Paints the still wallpaper again, where a dynamic one may have been left
/// (rwall ended without stopping it).
pub fn repaint_wallpaper() {
    desktop::repaint_wallpaper();
}

enum Cmd {
    Play(Spec),
    Stop,
    /// Paused from the menu (true) or resumed.
    Pause(bool),
    Quit,
}

/// The player thread, seen from the window.
pub struct Player {
    tx: Sender<Cmd>,
    thread: u32,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Player {
    /// Starts the thread; `report` hears every change of state (from the
    /// player thread).
    pub fn start(report: Box<dyn Fn(State) + Send>) -> Option<Player> {
        let (tx, rx) = channel();
        let (id_tx, id_rx) = channel();
        let join = std::thread::Builder::new()
            .name("rwall-live".into())
            .spawn(move || {
                let mut msg = MSG::default();
                // SAFETY: makes the thread's message queue before telling
                // its id, so no wake-up is lost.
                let id = unsafe {
                    let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                    GetCurrentThreadId()
                };
                let _ = id_tx.send(id);
                let report = std::rc::Rc::new(report);
                let r2 = report.clone();
                let ended = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || run(rx, &**r2)));
                if ended.is_err() {
                    desktop::repaint_wallpaper();
                    report(State::Failed("the player stopped".into()));
                }
            })
            .ok()?;
        let thread = id_rx.recv().ok()?;
        Some(Player { tx, thread, join: Some(join) })
    }

    fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
        // SAFETY: only wakes the thread up.
        let _ = unsafe { PostThreadMessageW(self.thread, WM_APP, WPARAM(0), LPARAM(0)) };
    }

    pub fn play(&self, spec: Spec) {
        self.send(Cmd::Play(spec));
    }

    pub fn stop(&self) {
        self.send(Cmd::Stop);
    }

    pub fn pause(&self, on: bool) {
        self.send(Cmd::Pause(on));
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.send(Cmd::Quit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

// ---------------------------------------------------------------- windows

const WALL_CLASS: PCWSTR = w!("rwall-live");
const CONTROL_CLASS: PCWSTR = w!("rwall-live-control");

thread_local! {
    /// Explorer restarted: attach again.
    static REATTACH: Cell<bool> = const { Cell::new(false) };
    /// Screens changed.
    static LAYOUT: Cell<bool> = const { Cell::new(false) };
    static DISPLAY_OFF: Cell<bool> = const { Cell::new(false) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

unsafe extern "system" fn wall_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_PAINT => {
            // The swap chain keeps what was shown.
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: paired BeginPaint/EndPaint on this window.
            unsafe {
                BeginPaint(h, &mut ps);
                let _ = EndPaint(h, &ps);
            }
            LRESULT(0)
        }
        // SAFETY: default handling.
        _ => unsafe { DefWindowProcW(h, msg, wp, lp) },
    }
}

unsafe extern "system" fn control_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg != 0 && msg == TASKBAR_CREATED.with(Cell::get) {
        REATTACH.with(|c| c.set(true));
        return LRESULT(0);
    }
    match msg {
        WM_DISPLAYCHANGE => LAYOUT.with(|c| c.set(true)),
        WM_POWERBROADCAST if wp.0 == PBT_POWERSETTINGCHANGE as usize && lp.0 != 0 => {
            // SAFETY: Windows passes a POWERBROADCAST_SETTING with this message.
            let s = unsafe { &*(lp.0 as *const POWERBROADCAST_SETTING) };
            if s.PowerSetting == GUID_CONSOLE_DISPLAY_STATE && s.DataLength >= 1 {
                // 0 off, 1 on, 2 dimmed.
                DISPLAY_OFF.with(|c| c.set(s.Data[0] == 0));
            }
        }
        _ => {}
    }
    // SAFETY: default handling.
    unsafe { DefWindowProcW(h, msg, wp, lp) }
}

type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

fn register_classes() {
    // SAFETY: the classes point to static names and procedures.
    unsafe {
        let inst = GetModuleHandleW(None).unwrap_or_default();
        let black = HBRUSH(GetStockObject(BLACK_BRUSH).0);
        for (name, proc_, brush) in [(WALL_CLASS, Some(wall_proc as WndProc), black), (CONTROL_CLASS, Some(control_proc as WndProc), HBRUSH::default())] {
            let wc = WNDCLASSW { lpfnWndProc: proc_, hInstance: inst.into(), lpszClassName: name, hbrBackground: brush, ..Default::default() };
            RegisterClassW(&wc);
        }
        TASKBAR_CREATED.with(|c| c.set(RegisterWindowMessageW(w!("TaskbarCreated"))));
    }
}

fn create(class: PCWSTR, ex: WINDOW_EX_STYLE) -> Option<HWND> {
    // SAFETY: the class was registered by this thread.
    unsafe {
        let inst = GetModuleHandleW(None).unwrap_or_default();
        CreateWindowExW(ex, class, w!("rwall"), WS_POPUP, 0, 0, 1, 1, None, None, Some(inst.into()), None).ok()
    }
}

fn destroy(h: HWND) {
    // SAFETY: a window of this thread.
    let _ = unsafe { DestroyWindow(h) };
}

// ---------------------------------------------------------------- Direct3D

fn device() -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext)> {
    let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_10_0];
    let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
    let mut last = windows::core::Error::empty();
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let (mut device, mut context) = (None, None);
        // SAFETY: out-pointers to locals.
        match unsafe { D3D11CreateDevice(None, driver, HMODULE::default(), flags, Some(&levels), D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context)) } {
            Ok(()) => {
                if let (Some(d), Some(c)) = (device, context) {
                    return Ok((d, c));
                }
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// A swap chain of `w`×`h` pixels on `win`, stretched to the window.
fn swap_chain(device: &ID3D11Device, win: HWND, w: u32, h: u32) -> windows::core::Result<IDXGISwapChain1> {
    // SAFETY: COM calls on the device just made.
    unsafe {
        let dxgi: IDXGIDevice = device.cast()?;
        let factory: IDXGIFactory2 = dxgi.GetAdapter()?.GetParent()?;
        let mut desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: w.max(1),
            Height: h.max(1),
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            ..Default::default()
        };
        let swap = match factory.CreateSwapChainForHwnd(device, win, &desc, None, None) {
            Ok(s) => s,
            Err(_) => {
                // Older systems, or a child window that refuses the flip model.
                desc.BufferCount = 1;
                desc.SwapEffect = DXGI_SWAP_EFFECT_DISCARD;
                factory.CreateSwapChainForHwnd(device, win, &desc, None, None)?
            }
        };
        let _ = factory.MakeWindowAssociation(win, DXGI_MWA_NO_ALT_ENTER | DXGI_MWA_NO_WINDOW_CHANGES);
        Ok(swap)
    }
}

/// One screen: its window behind the icons and its swap chain.
struct Screen {
    win: HWND,
    /// Where it is, from the top left of the whole desktop.
    rect: Px,
    swap: IDXGISwapChain1,
    /// The swap chain's size.
    size: (u32, u32),
    /// The picture made for it (frames only), reused.
    buf: Vec<u32>,
}

// ---------------------------------------------------------------- video

/// Hears the media engine (from its own threads).
#[implement(IMFMediaEngineNotify)]
struct Notify(Arc<AtomicU32>);

impl IMFMediaEngineNotify_Impl for Notify_Impl {
    fn EventNotify(&self, event: u32, param1: usize, _param2: u32) -> windows::core::Result<()> {
        if event == MF_MEDIA_ENGINE_EVENT_ERROR.0 as u32 {
            self.0.store((param1 as u32).max(1), Ordering::SeqCst);
        }
        Ok(())
    }
}

struct Video {
    engine: IMFMediaEngine,
    error: Arc<AtomicU32>,
}

impl Video {
    fn new(device: &ID3D11Device, context: &ID3D11DeviceContext, path: &std::path::Path) -> Result<Video, String> {
        let err = |what: &str, e: windows::core::Error| format!("{what} ({e})");
        let mf = |e| err("Media Foundation is not available", e);
        // SAFETY: COM calls on interfaces just made, on this thread (MTA).
        unsafe {
            if let Ok(mt) = context.cast::<ID3D11Multithread>() {
                let _ = mt.SetMultithreadProtected(true);
            }
            let mut token = 0u32;
            let mut manager = None;
            MFCreateDXGIDeviceManager(&mut token, &mut manager).map_err(mf)?;
            let manager = manager.ok_or("Media Foundation is not available")?;
            manager.ResetDevice(device, token).map_err(mf)?;
            let error = Arc::new(AtomicU32::new(0));
            let notify: IMFMediaEngineNotify = Notify(error.clone()).into();
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 3).map_err(mf)?;
            let attrs = attrs.ok_or("Media Foundation is not available")?;
            attrs.SetUnknown(&MF_MEDIA_ENGINE_DXGI_MANAGER, &manager).map_err(mf)?;
            attrs.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notify).map_err(mf)?;
            attrs.SetUINT32(&MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32).map_err(mf)?;
            let factory: IMFMediaEngineClassFactory =
                CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER).map_err(|e| err("the media engine is not available", e))?;
            let engine = factory.CreateInstance(0, &attrs).map_err(|e| err("the media engine is not available", e))?;
            let _ = engine.SetMuted(true);
            let _ = engine.SetLoop(true);
            let _ = engine.SetAutoPlay(true);
            engine.SetSource(&BSTR::from(path.to_string_lossy().as_ref())).map_err(|e| err("the video cannot be opened", e))?;
            engine.Play().map_err(|e| err("the video cannot be played", e))?;
            Ok(Video { engine, error })
        }
    }

    /// Copies a new frame to every screen, if there is one.
    fn tick(&self, screens: &[Screen], fit: LiveFit, device: &ID3D11Device, context: &ID3D11DeviceContext) -> bool {
        let mut pts = 0i64;
        // SAFETY: the raw call tells S_OK (a new frame) from S_FALSE (none),
        // which the wrapper does not.
        let hr = unsafe { (Interface::vtable(&self.engine).OnVideoStreamTick)(Interface::as_raw(&self.engine), &mut pts) };
        if hr != S_OK {
            return false;
        }
        let (mut vw, mut vh) = (0u32, 0u32);
        // SAFETY: COM calls on the engine and the swap chains of this thread.
        unsafe {
            if self.engine.GetNativeVideoSize(Some(&mut vw), Some(&mut vh)).is_err() || vw == 0 || vh == 0 {
                return false;
            }
            let black = MFARGB { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbAlpha: 255 };
            for s in screens {
                let Ok(tex) = s.swap.GetBuffer::<ID3D11Texture2D>(0) else { continue };
                let whole = Px { x: 0, y: 0, w: s.size.0 as i32, h: s.size.1 as i32 };
                let (src, dst) = place((vw, vh), whole, fit);
                if dst != whole {
                    let mut rtv = None;
                    if device.CreateRenderTargetView(&tex, None, Some(&mut rtv)).is_ok() {
                        if let Some(rtv) = rtv {
                            context.ClearRenderTargetView(&rtv, &[0.0, 0.0, 0.0, 1.0]);
                        }
                    }
                }
                let from = MFVideoNormalizedRect { left: src[0], top: src[1], right: src[2], bottom: src[3] };
                let to = RECT { left: dst.x, top: dst.y, right: dst.x + dst.w, bottom: dst.y + dst.h };
                let _ = self.engine.TransferVideoFrame(&tex, Some(&from), &to, Some(&black));
            }
            // Present waits for the screen once; the others go at once.
            for (i, s) in screens.iter().enumerate() {
                let _ = s.swap.Present(if i == 0 { 1 } else { 0 }, DXGI_PRESENT(0));
            }
        }
        true
    }

    fn set_paused(&self, paused: bool) {
        // SAFETY: COM calls on the engine.
        let _ = unsafe {
            if paused {
                self.engine.Pause()
            } else {
                self.engine.Play()
            }
        };
    }

    /// Why the video cannot play, once it failed.
    fn failure(&self) -> Option<String> {
        match self.error.load(Ordering::SeqCst) {
            0 => None,
            4 => Some("Windows cannot play this kind of video (a codec may be missing)".into()),
            3 => Some("the video could not be decoded".into()),
            n => Some(format!("Windows could not play the video (error {n})")),
        }
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        // SAFETY: the engine's own threads stop before it goes.
        let _ = unsafe { self.engine.Shutdown() };
    }
}

// ---------------------------------------------------------------- frames

enum Source {
    Anim(anim::Frames, usize),
    Scene(Painter, Vec<u32>),
}

impl Source {
    fn size(&self) -> (u32, u32) {
        match self {
            Source::Anim(f, _) => (f.w, f.h),
            Source::Scene(p, _) => p.size(),
        }
    }
}

/// Frames made on the processor.
struct Frames {
    source: Source,
    /// The time of the scene starts here (moved on while paused).
    start: Instant,
    paused_at: Option<Instant>,
    next: Instant,
}

impl Frames {
    fn new(source: Source) -> Frames {
        let now = Instant::now();
        Frames { source, start: now, paused_at: None, next: now }
    }

    fn set_paused(&mut self, paused: bool) {
        match (paused, self.paused_at) {
            (true, None) => self.paused_at = Some(Instant::now()),
            (false, Some(at)) => {
                self.start += at.elapsed();
                self.next = Instant::now();
                self.paused_at = None;
            }
            _ => {}
        }
    }

    /// Shows the next frame when it is time; returns when to come back.
    fn tick(&mut self, screens: &mut [Screen], fit: LiveFit, context: &ID3D11DeviceContext) -> Instant {
        let now = Instant::now();
        if self.paused_at.is_some() {
            return now + Duration::from_secs(1);
        }
        if now < self.next {
            return self.next;
        }
        let (px, w, h): (&[u32], u32, u32) = match &mut self.source {
            Source::Anim(f, i) => {
                *i = (*i + 1) % f.frames.len();
                self.next = now + f.frames[*i].1;
                (&f.frames[*i].0, f.w, f.h)
            }
            Source::Scene(p, buf) => {
                let t = (now - self.start).as_secs_f32();
                p.frame(t, buf);
                let (w, h) = p.size();
                self.next = now + Duration::from_secs_f32(1.0 / p.fps() as f32);
                (buf, w, h)
            }
        };
        for s in screens.iter_mut() {
            let (cw, ch) = compose(px, w, h, (s.rect.w, s.rect.h), fit, &mut s.buf);
            if (cw, ch) != s.size {
                continue;
            }
            // SAFETY: the buffer holds cw×ch pixels, rows of cw*4 bytes, as
            // the swap chain's texture.
            unsafe {
                let Ok(tex) = s.swap.GetBuffer::<ID3D11Texture2D>(0) else { continue };
                context.UpdateSubresource(&tex, 0, None, s.buf.as_ptr().cast(), cw * 4, 0);
                let _ = s.swap.Present(0, DXGI_PRESENT(0));
            }
        }
        self.next
    }
}

// ---------------------------------------------------------------- the thread

enum Output {
    Video(Video),
    Frames(Frames),
}

/// What is on the desktop now.
struct Stage {
    host: Host,
    layout: Layout,
    fit: LiveFit,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    /// Always Some until dropped: the output goes before the windows.
    out: Option<Output>,
    screens: Vec<Screen>,
}

impl Drop for Stage {
    fn drop(&mut self) {
        drop(self.out.take());
        for s in self.screens.drain(..) {
            let win = s.win;
            drop(s);
            destroy(win);
        }
    }
}

struct Run<'a> {
    report: &'a dyn Fn(State),
    spec: Option<Spec>,
    stage: Option<Stage>,
    user_pause: bool,
    paused: Option<Reason>,
    told: Option<State>,
    next_check: Instant,
}

impl Run<'_> {
    fn tell(&mut self, s: State) {
        if self.told.as_ref() != Some(&s) {
            (self.report)(s.clone());
            self.told = Some(s);
        }
    }

    fn play(&mut self, spec: Spec) {
        // Only the pauses changed: keep playing.
        if self.stage.is_some() && self.spec.as_ref().is_some_and(|s| s.show == spec.show && s.fit == spec.fit) {
            self.spec = Some(spec);
            self.check();
            return;
        }
        self.stage = None;
        self.spec = Some(spec);
        self.paused = None;
        self.build();
    }

    fn stop(&mut self) {
        self.spec = None;
        if self.stage.take().is_some() {
            desktop::repaint_wallpaper();
        }
        self.tell(State::Off);
    }

    /// Puts the windows behind the icons and starts what the spec says.
    fn build(&mut self) {
        let Some(spec) = self.spec.clone() else { return };
        match make(&spec) {
            Ok(stage) => {
                self.stage = Some(stage);
                self.paused = None;
                self.check();
            }
            Err(e) => {
                self.spec = None;
                desktop::repaint_wallpaper();
                self.tell(State::Failed(e));
            }
        }
    }

    /// Whether to pause, and why.
    fn reason(&self) -> Option<Reason> {
        if self.user_pause {
            return Some(Reason::User);
        }
        let spec = self.spec.as_ref()?;
        spec.pauses
            .iter()
            .copied()
            .find(|p| match p {
                PauseWhen::FullScreen => desktop::full_screen_app(),
                PauseWhen::Maximized => desktop::desktop_covered(),
                PauseWhen::Locked => DISPLAY_OFF.with(Cell::get) || desktop::locked(),
                PauseWhen::Battery => desktop::on_battery(),
            })
            .map(Reason::When)
    }

    /// Once a second: still attached, still the same screens, pause or play.
    fn check(&mut self) {
        self.next_check = Instant::now() + Duration::from_secs(1);
        let rebuild = REATTACH.with(|c| c.replace(false)) | LAYOUT.with(|c| c.replace(false));
        let Some(st) = &self.stage else { return };
        if rebuild || st.screens.iter().any(|s| !desktop::still_attached(s.win, &st.host)) || desktop::layout() != st.layout {
            self.stage = None;
            self.build();
            return;
        }
        let reason = self.reason();
        if reason != self.paused {
            self.paused = reason;
            if let Some(st) = &mut self.stage {
                match &mut st.out {
                    Some(Output::Video(v)) => v.set_paused(reason.is_some()),
                    Some(Output::Frames(f)) => f.set_paused(reason.is_some()),
                    None => {}
                }
            }
        }
        self.tell(match reason {
            Some(r) => State::Paused(r),
            None => State::Playing,
        });
    }

    /// Shows what is due; returns how long the thread may sleep.
    fn step(&mut self) -> Duration {
        let until_check = self.next_check.saturating_duration_since(Instant::now());
        let Some(st) = &mut self.stage else { return Duration::from_secs(3600) };
        match &mut st.out {
            None => Duration::from_secs(3600),
            Some(Output::Video(v)) => {
                if let Some(e) = v.failure() {
                    self.stage = None;
                    self.spec = None;
                    desktop::repaint_wallpaper();
                    self.tell(State::Failed(e));
                    return Duration::from_secs(3600);
                }
                if self.paused.is_some() {
                    return until_check;
                }
                if v.tick(&st.screens, st.fit, &st.device, &st.context) {
                    // Present waited for the screen already.
                    Duration::ZERO
                } else {
                    Duration::from_millis(4).min(until_check)
                }
            }
            Some(Output::Frames(f)) => {
                let next = f.tick(&mut st.screens, st.fit, &st.context);
                next.saturating_duration_since(Instant::now()).min(until_check)
            }
        }
    }
}

/// Reads what to play, then puts one window per screen behind the icons.
fn make(spec: &Spec) -> Result<Stage, String> {
    // Read the file before touching the desktop.
    let source = match &spec.show {
        Show::Anim(p) => Some(Source::Anim(anim::load(p)?, 0)),
        Show::Scene(s) => Some(Source::Scene(Painter::new(*s), Vec::new())),
        Show::Video(p) => {
            if !p.is_file() {
                return Err(format!("{} was not found", p.display()));
            }
            None
        }
    };
    let host = desktop::find_host().ok_or("the desktop of Windows was not found")?;
    let layout = desktop::layout();
    let (device, context) = device().map_err(|e| format!("no Direct3D 11 device ({e})"))?;
    let mut screens: Vec<Screen> = Vec::new();
    for r in &layout.screens {
        let size = match &source {
            Some(src) => {
                let (w, h) = src.size();
                composed_size(w, h, (r.w, r.h), spec.fit)
            }
            None => (r.w as u32, r.h as u32),
        };
        let made = create(WALL_CLASS, WS_EX_NOACTIVATE | WS_EX_TRANSPARENT).ok_or_else(|| "no window".to_string()).and_then(|win| {
            let abs = Px { x: layout.all.x + r.x, y: layout.all.y + r.y, w: r.w, h: r.h };
            if !desktop::attach(win, &host, abs) {
                destroy(win);
                return Err("the window could not be put behind the desktop icons".to_string());
            }
            match swap_chain(&device, win, size.0, size.1) {
                Ok(swap) => Ok(Screen { win, rect: *r, swap, size, buf: Vec::new() }),
                Err(e) => {
                    destroy(win);
                    Err(format!("no swap chain ({e})"))
                }
            }
        });
        match made {
            Ok(s) => screens.push(s),
            Err(e) => {
                for s in screens.drain(..) {
                    let win = s.win;
                    drop(s);
                    destroy(win);
                }
                return Err(e);
            }
        }
    }
    let out = match (source, &spec.show) {
        (Some(src), _) => Output::Frames(Frames::new(src)),
        (None, Show::Video(p)) => Output::Video(Video::new(&device, &context, p).inspect_err(|_| {
            for s in screens.drain(..) {
                let win = s.win;
                drop(s);
                destroy(win);
            }
        })?),
        (None, _) => unreachable!("only videos have no source"),
    };
    Ok(Stage { host, layout, fit: spec.fit, device, context, out: Some(out), screens })
}

fn run(rx: Receiver<Cmd>, report: &dyn Fn(State)) {
    // SAFETY: set up once on this thread; undone at the end.
    let (control, power) = unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = MFStartup(MF_VERSION, MFSTARTUP_LITE);
        register_classes();
        let control = create(CONTROL_CLASS, WS_EX_TOOLWINDOW);
        let power = control.and_then(|c| RegisterPowerSettingNotification(HANDLE(c.0), &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE).ok());
        (control, power)
    };
    let mut r = Run { report, spec: None, stage: None, user_pause: false, paused: None, told: None, next_check: Instant::now() };
    'main: loop {
        let mut msg = MSG::default();
        // SAFETY: the usual message loop of this thread.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        while let Ok(c) = rx.try_recv() {
            match c {
                Cmd::Play(spec) => r.play(spec),
                Cmd::Stop => r.stop(),
                Cmd::Pause(on) => {
                    r.user_pause = on;
                    r.check();
                }
                Cmd::Quit => {
                    r.stop();
                    break 'main;
                }
            }
        }
        if Instant::now() >= r.next_check {
            r.check();
        }
        let wait = r.step();
        if !wait.is_zero() {
            // SAFETY: waits for a message or the time.
            unsafe {
                MsgWaitForMultipleObjects(None, false, wait.as_millis().min(u32::MAX as u128 - 1) as u32, QS_ALLINPUT);
            }
        }
    }
    // SAFETY: undoes the set-up above.
    unsafe {
        if let Some(p) = power {
            let _ = UnregisterPowerSettingNotification(p);
        }
        if let Some(c) = control {
            destroy(c);
        }
        let _ = MFShutdown();
        CoUninitialize();
    }
}
