//! 把 libmpv 作为 Bern 的媒体内核。
//!
//! 一个线程独占 mpv 实例、它的渲染上下文、Direct3D 设备，以及当前显示视频的那个视图的
//! 交换链。Bern 的调用来自其他线程（请求在发起它的线程上，表面在 UI 线程上，
//! 帧在帧时钟线程上），都以消息的形式发给这个线程，所以 mpv 和 Direct3D 的东西
//! 不会被多个线程共享。
//!
//! 只有一个 mpv 实例，所以同一时间只有一个播放器在播：最后加载的那个。
//! 它的画面显示在最后挂接的视图里。
//!
//! 弹幕：每次帧时钟来一帧，按平滑后的播放时间算出屏上的弹幕，用 `osd-overlay` 交给 mpv。
//! 弹幕变了就把当前这一帧视频重画一遍，所以 24 帧的片子上弹幕也按屏幕刷新率滚动。

use crate::d3d::{self, Surface};
use crate::sys::{self, Mpv};
use crate::url::to_mpv_location;
use bern::CommandError;
use bern::media::kernel::{
    ExecOp, ExecRequest, FrameTiming, KernelHub, MediaCapabilities, MediaExecutor, MediaKernel,
    Observation, Phase, SurfaceHandle, SurfaceKind, SurfaceSize, VideoSurface,
};
use nkuplay_danmaku::{CANVAS_HEIGHT, Clock, Comment, Config, Layout, Measure, Track};
use std::ffi::{CStr, CString, c_int, c_void};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Direct3D11::ID3D11Device;

/// 没有请求时，多久读取并上报一次播放器状态。
const REPORT_EVERY: Duration = Duration::from_millis(100);
/// 视图等待内核放开它的表面的最长时间。
const DETACH_TIMEOUT: Duration = Duration::from_secs(2);
/// 应用退出时等 mpv 关掉的最长时间。
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

enum Message {
    Hub(Option<Arc<dyn KernelHub>>),
    Execute(ExecRequest),
    Attach {
        view: String,
        panel: u64,
        size: SurfaceSize,
    },
    Resize {
        view: String,
        size: SurfaceSize,
    },
    Detach {
        view: String,
        done: Sender<()>,
    },
    Frame,
    /// 一集的弹幕和时间偏移；为空时去掉弹幕。
    Danmaku(Option<(Vec<Comment>, f64)>),
    /// 显示或隐藏弹幕。
    DanmakuVisible(bool),
    /// 下一次打开这个地址时从这个位置（毫秒）开始播；为空时不指定。
    StartAt(Option<(String, i64)>),
    /// 把正在显示的这一帧画面（不含字幕和弹幕）存成图片，存好了（或没能存）回话。
    Snapshot(PathBuf, Sender<bool>),
    /// 应用要退出了：关掉 mpv，结束内核线程，然后回话。
    Shutdown(Sender<()>),
}

/// 画弹幕用的 OSD 叠加层编号。
const DANMAKU_OVERLAY: &CStr = c"1";

/// 装上的内核，供引擎交弹幕用。
static KERNEL: OnceLock<Arc<MpvKernel>> = OnceLock::new();

/// 记下装上的内核。
pub fn remember(kernel: &Arc<MpvKernel>) {
    let _ = KERNEL.set(Arc::clone(kernel));
}

/// 把弹幕交给内核线程；没有内核时什么也不做。
pub fn set_danmaku(danmaku: Option<(Vec<Comment>, f64)>) {
    if let Some(kernel) = KERNEL.get() {
        kernel.send(Message::Danmaku(danmaku));
    }
}

/// 显示或隐藏弹幕；没有内核时什么也不做。
pub fn show_danmaku(visible: bool) {
    if let Some(kernel) = KERNEL.get() {
        kernel.send(Message::DanmakuVisible(visible));
    }
}

/// 下一次打开 `url` 时从 `position_ms` 开始播；没有内核时什么也不做。
pub fn start_at(start: Option<(String, i64)>) {
    if let Some(kernel) = KERNEL.get() {
        kernel.send(Message::StartAt(start));
    }
}

/// 请内核把正在显示的画面存到 `path`（JPEG）。请求马上排进内核的队列（排在之后的打开前面），
/// 存好了（或没能存）从交回的通道回话。没有内核时为空。
pub fn snapshot(path: PathBuf) -> Option<Receiver<bool>> {
    let kernel = KERNEL.get()?;
    let (done, finished) = channel();
    kernel.send(Message::Snapshot(path, done));
    Some(finished)
}

/// 正在显示的一集弹幕。
struct Danmaku {
    track: Arc<Track>,
    /// 按当前画面宽高比排好的；画面宽高比变了才重排。
    layout: Option<Layout>,
    clock: Clock,
    /// 上次交给 mpv 的文本，没变就不再交。
    shown: String,
}

pub struct MpvKernel {
    messages: Mutex<Sender<Message>>,
    /// 已经有一条帧消息在排队，帧时钟不必再排更多。
    frame_pending: Arc<AtomicBool>,
    /// 正在使用的 mpv 的版本，由它自己报告，例如 `v0.42.0`。
    version: String,
}

impl MpvKernel {
    /// 加载 libmpv，启动内核线程，等 mpv 和它的渲染器就绪；不成功则说明原因。
    pub fn start() -> Result<Arc<Self>, String> {
        let mpv = Mpv::load()?;
        let (messages, inbox) = channel();
        let (ready, started) = channel();
        let frame_pending = Arc::new(AtomicBool::new(false));
        let pending = Arc::clone(&frame_pending);
        std::thread::Builder::new()
            .name("Nkuplay mpv".into())
            .spawn(move || match Worker::new(mpv, pending) {
                Ok(mut worker) => {
                    let _ = ready.send(Ok(worker.version()));
                    let finished = worker.run(inbox);
                    // 先释放 mpv（`Drop` 里 `mpv_terminate_destroy`，等它自己的线程都结束），再回话。
                    drop(worker);
                    if let Some(finished) = finished {
                        let _ = finished.send(());
                    }
                }
                Err(reason) => {
                    let _ = ready.send(Err(reason));
                }
            })
            .map_err(|error| format!("the kernel thread did not start: {error}"))?;
        let version = started
            .recv()
            .map_err(|_| "the kernel thread stopped while starting".to_string())??;
        Ok(Arc::new(Self {
            messages: Mutex::new(messages),
            frame_pending,
            version,
        }))
    }

    fn send(&self, message: Message) {
        // 这个线程只随进程结束，所以发送失败说明它 panic 了；已经没有谁能播放了。
        let _ = self.messages.lock().expect("kernel messages").send(message);
    }
}

impl MediaExecutor for MpvKernel {
    fn execute(&self, request: ExecRequest) {
        self.send(Message::Execute(request));
    }
}

impl MediaKernel for MpvKernel {
    fn capabilities(&self) -> MediaCapabilities {
        MediaCapabilities {
            backend: "libmpv".into(),
            backend_version: self.version.clone(),
            background_playback: true,
            multi_view: false,
        }
    }

    fn surfaces(&self) -> Vec<SurfaceKind> {
        vec![SurfaceKind::SwapChainPanel]
    }

    fn attached(&self, hub: Arc<dyn KernelHub>) {
        self.send(Message::Hub(Some(hub)));
    }

    /// 只在应用退出时调用（`bern_app_stop`）。除了停下播放，还要把 mpv 整个关掉并等它关完：
    /// 应用的播放器（`#[bern::players]`）在窗口关闭后仍留着，mpv 还开着文件、音频和解码线程；
    /// 不关就让进程直接结束的话，进程退出时会卡住，窗口没了、进程却一直留在后台。
    fn detached(&self) {
        self.send(Message::Hub(None));
        let (finished, done) = channel();
        self.send(Message::Shutdown(finished));
        let _ = done.recv_timeout(SHUTDOWN_TIMEOUT);
    }

    fn attach_surface(&self, view: &str, _player: &str, surface: VideoSurface) {
        if let SurfaceHandle::SwapChainPanel(panel) = surface.handle {
            self.send(Message::Attach {
                view: view.into(),
                panel,
                size: surface.size,
            });
        }
    }

    fn resize_surface(&self, view: &str, size: SurfaceSize) {
        self.send(Message::Resize {
            view: view.into(),
            size,
        });
    }

    fn detach_surface(&self, view: &str) {
        let (done, released) = channel();
        self.send(Message::Detach {
            view: view.into(),
            done,
        });
        // 本函数返回后视图就会释放面板，所以要等到线程丢掉交换链。
        let _ = released.recv_timeout(DETACH_TIMEOUT);
    }

    fn frame(&self, _view: &str, _timing: FrameTiming) {
        if !self.frame_pending.swap(true, Ordering::AcqRel) {
            self.send(Message::Frame);
        }
    }
}

/// 面板正在显示画面的那个视图。
struct Shown {
    view: String,
    surface: Surface,
}

struct Worker {
    mpv: Mpv,
    handle: *mut sys::Handle,
    render: *mut sys::RenderContext,
    device: ID3D11Device,
    frame_pending: Arc<AtomicBool>,
    hub: Option<Arc<dyn KernelHub>>,
    shown: Option<Shown>,
    /// mpv 正在为哪个播放器播放，以及它的片源版本。
    player: Option<String>,
    source_version: u64,
    /// 已发出 `loadfile`，mpv 还没有打开文件。
    loading: bool,
    /// mpv 已打开一个文件。
    loaded: bool,
    /// 上一个文件没能播放的原因。
    error: Option<String>,
    /// 上次上报的内容，用来只上报变化。
    reported: Option<Observation>,
    last_report: Instant,
    /// 即使 mpv 没有新画面也要画下一帧：表面是新的，或者尺寸变了，或者弹幕动了。
    redraw: bool,
    /// 正在显示视频的表面的像素尺寸。
    canvas: (u32, u32),
    danmaku: Option<Danmaku>,
    danmaku_visible: bool,
    /// 下一次打开这个地址时从哪里（毫秒）开始播。
    start_at: Option<(String, i64)>,
}

impl Worker {
    fn new(mpv: Mpv, frame_pending: Arc<AtomicBool>) -> Result<Self, String> {
        let device = d3d::create_device()?;
        // SAFETY: `mpv_create` 没有前置条件。
        let handle = unsafe { (mpv.create)() };
        if handle.is_null() {
            return Err("mpv could not be created".into());
        }
        let option = |name: &CStr, value: &CStr| {
            // SAFETY: 两个字符串都以 NUL 结尾，并且在调用期间一直有效。
            let code = unsafe { (mpv.set_option_string)(handle, name.as_ptr(), value.as_ptr()) };
            if code < 0 {
                eprintln!(
                    "nkuplay-mpv: option {} was refused: {}",
                    name.to_string_lossy(),
                    mpv.error_text(code)
                );
            }
        };
        // 画面交给下面的渲染上下文，绝不交给 mpv 自己的窗口；
        // 应用自己画控制栏、自己读按键。
        option(c"vo", c"libmpv");
        option(c"hwdec", c"auto-safe");
        option(c"config", c"no");
        option(c"terminal", c"no");
        option(c"osc", c"no");
        option(c"input-default-bindings", c"no");
        option(c"input-vo-keyboard", c"no");
        // 文件播完后停在最后一帧；没有文件时也保持运行。
        option(c"keep-open", c"yes");
        // 继续播放卡片上的画面：存成 JPEG，小一些。
        option(c"screenshot-format", c"jpg");
        option(c"screenshot-jpeg-quality", c"85");
        option(c"idle", c"yes");
        // 一帧何时显示由应用的帧时钟决定。
        option(c"video-timing-offset", c"0");
        if let Some(path) = std::env::var_os("NKUPLAY_MPV_LOG")
            && let Ok(path) = CString::new(path.to_string_lossy().as_bytes())
        {
            option(c"log-file", &path);
        }
        // SAFETY: `handle` 是已创建、尚未初始化的实例。
        let code = unsafe { (mpv.initialize)(handle) };
        if code < 0 {
            let reason = format!("mpv did not initialize: {}", mpv.error_text(code));
            // SAFETY: `handle` 仍存活，此后不再使用。
            unsafe { (mpv.terminate_destroy)(handle) };
            return Err(reason);
        }

        let mut init = sys::D3d11InitParams {
            device: d3d::device_address(&device),
        };
        let mut params = [
            sys::RenderParam {
                kind: sys::RENDER_PARAM_API_TYPE,
                data: sys::RENDER_API_TYPE_D3D11.as_ptr() as *mut c_void,
            },
            sys::RenderParam {
                kind: sys::RENDER_PARAM_D3D11_INIT_PARAMS,
                data: (&raw mut init).cast(),
            },
            sys::RenderParam {
                kind: sys::RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        let mut render = std::ptr::null_mut();
        // SAFETY: `params` 以无效项结尾，它指向的所有内容在调用返回前都有效。
        let code = unsafe { (mpv.render_context_create)(&mut render, handle, params.as_mut_ptr()) };
        if code < 0 {
            let reason = format!(
                "this libmpv has no Direct3D 11 renderer: {}",
                mpv.error_text(code)
            );
            // SAFETY: 同上。
            unsafe { (mpv.terminate_destroy)(handle) };
            return Err(reason);
        }

        Ok(Self {
            mpv,
            handle,
            render,
            device,
            frame_pending,
            hub: None,
            shown: None,
            player: None,
            source_version: 0,
            loading: false,
            loaded: false,
            error: None,
            reported: None,
            last_report: Instant::now(),
            redraw: false,
            canvas: (0, 0),
            danmaku: None,
            danmaku_visible: true,
            start_at: None,
        })
    }

    /// 处理消息直到应用退出；退出时返回要回话的那一头。
    fn run(&mut self, inbox: Receiver<Message>) -> Option<Sender<()>> {
        loop {
            match inbox.recv_timeout(REPORT_EVERY) {
                Ok(Message::Shutdown(finished)) => {
                    self.danmaku = None;
                    self.shown = None;
                    let _ = self.command(&[c"stop"]);
                    return Some(finished);
                }
                Ok(message) => self.handle(message),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return None,
            }
            self.drain_events();
            if self.last_report.elapsed() >= REPORT_EVERY {
                self.report();
            }
        }
    }

    fn handle(&mut self, message: Message) {
        match message {
            Message::Hub(hub) => {
                if hub.is_none() {
                    // 停止失败也没有谁可以通知了。
                    let _ = self.command(&[c"stop"]);
                    self.player = None;
                }
                self.hub = hub;
                self.reported = None;
            }
            Message::Execute(request) => {
                let id = request.request;
                let result = self.execute(request);
                if let Some(hub) = &self.hub {
                    hub.complete(id, result);
                }
                self.drain_events();
                self.report();
            }
            Message::Attach { view, panel, size } => {
                // 先放开上一个视图的交换链，再创建这一个。
                self.shown = None;
                match Surface::attach(&self.device, panel, size.width, size.height) {
                    Ok(surface) => {
                        self.shown = Some(Shown { view, surface });
                        self.canvas = (size.width, size.height);
                        self.redraw = true;
                    }
                    Err(reason) => eprintln!("nkuplay-mpv: {reason}"),
                }
            }
            Message::Resize { view, size } => {
                if let Some(shown) = &mut self.shown
                    && shown.view == view
                {
                    self.canvas = (size.width, size.height);
                    match shown.surface.resize(size.width, size.height) {
                        Ok(changed) => self.redraw |= changed,
                        Err(reason) => eprintln!("nkuplay-mpv: {reason}"),
                    }
                }
            }
            Message::Detach { view, done } => {
                if self.shown.as_ref().is_some_and(|shown| shown.view == view) {
                    self.shown = None;
                }
                let _ = done.send(());
            }
            Message::Frame => {
                self.frame_pending.store(false, Ordering::Release);
                self.update_danmaku();
                self.draw();
            }
            Message::Danmaku(danmaku) => {
                self.clear_danmaku_overlay();
                self.danmaku = danmaku.map(|(comments, shift)| {
                    let track = Track::new(&comments, shift, Config::default(), &Measure::system());
                    Danmaku {
                        track: Arc::new(track),
                        layout: None,
                        clock: Clock::new(),
                        shown: String::new(),
                    }
                });
            }
            Message::DanmakuVisible(visible) => {
                self.danmaku_visible = visible;
                if !visible {
                    self.clear_danmaku_overlay();
                }
            }
            Message::StartAt(start) => self.start_at = start,
            Message::Snapshot(path, done) => {
                let _ = done.send(self.snapshot(&path));
            }
            // `run` 自己处理，到不了这里。
            Message::Shutdown(_) => {}
        }
    }

    /// 按现在的播放时间更新屏上的弹幕；变了就让这一帧重画。
    fn update_danmaku(&mut self) {
        let showing = self.danmaku_visible && self.loaded && self.shown.is_some();
        let (width, height) = self.canvas;
        if self.danmaku.is_none() || !showing || width == 0 || height == 0 {
            self.clear_danmaku_overlay();
            return;
        }
        let Some(position) = self.double(c"time-pos") else {
            return;
        };
        let paused = self.flag(c"pause").unwrap_or(false)
            || self.flag(c"paused-for-cache").unwrap_or(false)
            || self.flag(c"eof-reached").unwrap_or(false);
        let speed = self.double(c"speed").unwrap_or(1.0);
        let canvas_width = CANVAS_HEIGHT * f64::from(width) / f64::from(height);
        let Some(danmaku) = self.danmaku.as_mut() else {
            return;
        };
        if danmaku
            .layout
            .as_ref()
            .is_none_or(|layout| (layout.width() - canvas_width).abs() > 0.5)
        {
            danmaku.layout = Some(Layout::new(Arc::clone(&danmaku.track), canvas_width));
        }
        let time = danmaku.clock.tick(position, !paused, speed, Instant::now());
        let events = danmaku
            .layout
            .as_ref()
            .map(|layout| layout.events(time))
            .unwrap_or_default();
        if events == danmaku.shown {
            return;
        }
        let result = if events.is_empty() {
            self.command(&[c"osd-overlay", DANMAKU_OVERLAY, c"none", c""])
        } else {
            match CString::new(events.as_str()) {
                Ok(data) => {
                    let res_x = CString::new(format!("{}", canvas_width.round() as i64))
                        .expect("a number has no NUL");
                    let res_y = CString::new(format!("{}", CANVAS_HEIGHT as i64))
                        .expect("a number has no NUL");
                    self.command(&[
                        c"osd-overlay",
                        DANMAKU_OVERLAY,
                        c"ass-events",
                        &data,
                        &res_x,
                        &res_y,
                        c"0",
                        c"no",
                        c"no",
                    ])
                }
                // 弹幕文字里有 NUL：这一帧不画。
                Err(_) => return,
            }
        };
        match result {
            Ok(()) => {
                if let Some(danmaku) = self.danmaku.as_mut() {
                    danmaku.shown = events;
                }
                self.redraw = true;
            }
            Err(error) => eprintln!("nkuplay-mpv: {}", error.message),
        }
    }

    /// 从画面上去掉弹幕（弹幕本身留着）。
    fn clear_danmaku_overlay(&mut self) {
        let Some(danmaku) = self.danmaku.as_mut() else {
            return;
        };
        // 下次再显示时从 mpv 的位置重新开始算。
        danmaku.clock.reset();
        if danmaku.shown.is_empty() {
            return;
        }
        danmaku.shown.clear();
        let _ = self.command(&[c"osd-overlay", DANMAKU_OVERLAY, c"none", c""]);
        self.redraw = true;
    }

    /// 有新画面或表面需要重画时，把 mpv 当前的画面画进正在显示的视图。
    fn draw(&mut self) {
        let Some(shown) = &self.shown else {
            return;
        };
        // SAFETY: `render` 在 worker 的整个生命周期内都有效。
        let update = unsafe { (self.mpv.render_context_update)(self.render) };
        if update & sys::RENDER_UPDATE_FRAME == 0 && !self.redraw {
            return;
        }
        let texture = match shown.surface.back_buffer() {
            Ok(texture) => texture,
            Err(reason) => {
                eprintln!("nkuplay-mpv: {reason}");
                return;
            }
        };
        let mut target = sys::D3d11Fbo {
            tex: d3d::texture_address(&texture),
        };
        // 绝不在渲染调用里等待：这个线程还要应答命令，而且帧时钟已经在安排帧的节奏。
        let mut block: c_int = 0;
        let mut params = [
            sys::RenderParam {
                kind: sys::RENDER_PARAM_D3D11_FBO,
                data: (&raw mut target).cast(),
            },
            sys::RenderParam {
                kind: sys::RENDER_PARAM_BLOCK_FOR_TARGET_TIME,
                data: (&raw mut block).cast(),
            },
            sys::RenderParam {
                kind: sys::RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        // SAFETY: `params` 以无效项结尾，纹理属于创建 mpv 时给它的那个设备，
        // 并且 mpv 不保留对它的引用。
        let code = unsafe { (self.mpv.render_context_render)(self.render, params.as_mut_ptr()) };
        drop(texture);
        if code < 0 {
            eprintln!(
                "nkuplay-mpv: a frame did not render: {}",
                self.mpv.error_text(code)
            );
            return;
        }
        shown.surface.present();
        // SAFETY: 同上。
        unsafe { (self.mpv.render_context_report_swap)(self.render) };
        self.redraw = false;
    }

    fn execute(&mut self, request: ExecRequest) -> Result<(), CommandError> {
        if request.source_version < self.source_version
            && self.player.as_deref() == Some(request.player.as_str())
        {
            return Err(CommandError::new(
                "stale_source",
                "the player already runs a newer source",
            ));
        }
        match request.op {
            ExecOp::Load { url } => {
                let location = CString::new(to_mpv_location(&url)).map_err(|_| {
                    CommandError::new("media_failed", "the media location contains a NUL")
                })?;
                self.player = Some(request.player);
                self.source_version = request.source_version;
                self.loading = true;
                self.loaded = false;
                self.error = None;
                self.reported = None;
                self.set_flag(c"pause", false)?;
                // 断点续播：引擎在让界面打开之前说过从哪里开始。只用一次，而且只对那个地址。
                let start = self.start_at.take().filter(|(wanted, _)| *wanted == url);
                match start {
                    Some((_, position_ms)) => {
                        let options =
                            CString::new(format!("start={:.3}", position_ms as f64 / 1000.0))
                                .expect("a number has no NUL");
                        // `loadfile <地址> <方式> <位置> <选项>`；位置 -1 是 mpv 0.38 起要求的占位。
                        self.command(&[c"loadfile", &location, c"replace", c"-1", &options])
                    }
                    None => self.command(&[c"loadfile", &location]),
                }
            }
            ExecOp::Unload | ExecOp::Release => {
                self.loading = false;
                self.loaded = false;
                self.error = None;
                self.command(&[c"stop"])
            }
            ExecOp::Play => {
                // 在文件末尾再次播放，就从头开始。
                if self.flag(c"eof-reached").unwrap_or(false) {
                    self.command(&[c"seek", c"0", c"absolute"])?;
                }
                self.set_flag(c"pause", false)
            }
            ExecOp::Pause => self.set_flag(c"pause", true),
            ExecOp::Seek { position_ms } => {
                let seconds = CString::new(format!("{:.3}", position_ms as f64 / 1000.0))
                    .expect("a number has no NUL");
                self.command(&[c"seek", &seconds, c"absolute"])
            }
            ExecOp::SetVolume { volume } => {
                self.set_double(c"volume", volume.clamp(0.0, 1.0) * 100.0)
            }
            ExecOp::SetMuted { muted } => self.set_flag(c"mute", muted),
        }
    }

    /// 把这一帧画面存到 `path`；`video` 表示只要视频画面，不要字幕和 OSD（弹幕画在 OSD 上）。
    fn snapshot(&self, path: &std::path::Path) -> bool {
        if !self.loaded {
            return false;
        }
        let Some(location) = path.to_str().and_then(|path| CString::new(path).ok()) else {
            return false;
        };
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        self.command(&[c"screenshot-to-file", &location, c"video"])
            .is_ok()
            && path.is_file()
    }

    /// 取走 mpv 自上次以来要说的所有内容，不等待。
    fn drain_events(&mut self) {
        loop {
            // SAFETY: `handle` 仍存活；事件在本线程下一次调用 `mpv_wait_event` 之前有效。
            let event = unsafe { &*(self.mpv.wait_event)(self.handle, 0.0) };
            match event.event_id {
                sys::EVENT_NONE | sys::EVENT_SHUTDOWN => break,
                sys::EVENT_START_FILE => {
                    self.loading = true;
                    self.loaded = false;
                }
                sys::EVENT_FILE_LOADED => {
                    self.loading = false;
                    self.loaded = true;
                    self.error = None;
                }
                sys::EVENT_END_FILE => {
                    self.loading = false;
                    self.loaded = false;
                    if !event.data.is_null() {
                        // SAFETY: 文件结束事件携带的是 `mpv_event_end_file`。
                        let end = unsafe { &*event.data.cast::<sys::EventEndFile>() };
                        if end.reason == sys::END_FILE_REASON_ERROR {
                            self.error = Some(self.mpv.error_text(end.error));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// 播放器的状态变了时，告诉 hub 它在做什么。
    fn report(&mut self) {
        self.last_report = Instant::now();
        let (Some(hub), Some(player)) = (&self.hub, &self.player) else {
            return;
        };
        let observation = self.observe();
        if self.reported.as_ref() != Some(&observation) {
            hub.observe(player, self.source_version, observation.clone());
            self.reported = Some(observation);
        }
    }

    fn observe(&self) -> Observation {
        let volume = self.double(c"volume").map_or(1.0, |volume| volume / 100.0);
        let muted = self.flag(c"mute").unwrap_or(false);
        let phase = if self.error.is_some() {
            Phase::Failed
        } else if self.loading {
            Phase::Loading
        } else if !self.loaded {
            Phase::Idle
        } else if self.flag(c"eof-reached").unwrap_or(false) {
            Phase::Ended
        } else if self.flag(c"paused-for-cache").unwrap_or(false) {
            Phase::Buffering
        } else if self.flag(c"pause").unwrap_or(false) {
            Phase::Paused
        } else {
            Phase::Playing
        };
        let milliseconds = |seconds: f64| (seconds * 1000.0).round() as i64;
        Observation {
            phase,
            position_ms: self
                .double(c"time-pos")
                .filter(|_| self.loaded)
                .map_or(0, milliseconds),
            duration_ms: self
                .double(c"duration")
                .filter(|_| self.loaded)
                .map(milliseconds),
            seekable: self.loaded && self.flag(c"seekable").unwrap_or(false),
            volume: volume.clamp(0.0, 1.0),
            muted,
            error: self.error.clone(),
        }
    }

    fn failed(&self, what: &str, code: c_int) -> CommandError {
        CommandError::new(
            "media_failed",
            format!("{what}: {}", self.mpv.error_text(code)),
        )
    }

    fn command(&self, words: &[&CStr]) -> Result<(), CommandError> {
        let mut arguments = words
            .iter()
            .map(|word| word.as_ptr())
            .chain([std::ptr::null()])
            .collect::<Vec<_>>();
        // SAFETY: `arguments` 以 NULL 结尾，其中的字符串在调用期间一直有效。
        let code = unsafe { (self.mpv.command)(self.handle, arguments.as_mut_ptr()) };
        if code < 0 {
            return Err(self.failed(&words[0].to_string_lossy(), code));
        }
        Ok(())
    }

    fn flag(&self, name: &CStr) -> Option<bool> {
        let mut value: c_int = 0;
        // SAFETY: 标志读入一个 C int。
        let code = unsafe {
            (self.mpv.get_property)(
                self.handle,
                name.as_ptr(),
                sys::FORMAT_FLAG,
                (&raw mut value).cast(),
            )
        };
        (code >= 0).then_some(value != 0)
    }

    /// mpv 报告的自身版本：`mpv-version` 属性去掉开头的 `mpv `，读不到时为 `unknown`。
    fn version(&self) -> String {
        // SAFETY: `handle` 仍存活；返回的字符串由 mpv 分配，用完交还给 `mpv_free`。
        let text = unsafe {
            let raw = (self.mpv.get_property_string)(self.handle, c"mpv-version".as_ptr());
            if raw.is_null() {
                return "unknown".into();
            }
            let text = CStr::from_ptr(raw).to_string_lossy().into_owned();
            (self.mpv.free)(raw.cast());
            text
        };
        text.strip_prefix("mpv ").unwrap_or(&text).to_string()
    }

    fn double(&self, name: &CStr) -> Option<f64> {
        let mut value: f64 = 0.0;
        // SAFETY: double 读入一个 double。
        let code = unsafe {
            (self.mpv.get_property)(
                self.handle,
                name.as_ptr(),
                sys::FORMAT_DOUBLE,
                (&raw mut value).cast(),
            )
        };
        (code >= 0 && value.is_finite()).then_some(value)
    }

    fn set_flag(&self, name: &CStr, value: bool) -> Result<(), CommandError> {
        let mut value = c_int::from(value);
        // SAFETY: 标志从一个 C int 写入。
        let code = unsafe {
            (self.mpv.set_property)(
                self.handle,
                name.as_ptr(),
                sys::FORMAT_FLAG,
                (&raw mut value).cast(),
            )
        };
        if code < 0 {
            return Err(self.failed(&name.to_string_lossy(), code));
        }
        Ok(())
    }

    fn set_double(&self, name: &CStr, mut value: f64) -> Result<(), CommandError> {
        // SAFETY: double 从一个 double 写入。
        let code = unsafe {
            (self.mpv.set_property)(
                self.handle,
                name.as_ptr(),
                sys::FORMAT_DOUBLE,
                (&raw mut value).cast(),
            )
        };
        if code < 0 {
            return Err(self.failed(&name.to_string_lossy(), code));
        }
        Ok(())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shown = None;
        // SAFETY: 渲染上下文先于它所属的实例释放，此后两者都不再使用。
        unsafe {
            (self.mpv.render_context_free)(self.render);
            (self.mpv.terminate_destroy)(self.handle);
        }
    }
}
