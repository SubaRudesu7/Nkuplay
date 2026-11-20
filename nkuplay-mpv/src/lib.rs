//! 把 libmpv 作为 Nkuplay 的媒体内核。
//!
//! [`install`] 在运行时加载 `libmpv-2.dll`；库和 Direct3D 11 设备都可用时，
//! 向 Bern 注册一个内核：此后每个 `Video` 视图都会给它一个 `SwapChainPanel` 来画，
//! 标准媒体命令（打开、播放、暂停、跳转、音量）也交给 mpv，而不是系统播放器。
//! 找不到库时应用照常启动，用平台自带的媒体设施播放。
//!
//! 弹幕也由内核画：[`set_danmaku`] 交给它一集的弹幕，它每次屏幕刷新按播放位置算出屏上的弹幕，
//! 用 mpv 的 `osd-overlay` 画在画面上（排版见 `nkuplay-danmaku`）。Bern 的标准媒体命令里
//! 没有弹幕，所以引擎直接调用这几个函数。

#[cfg(windows)]
mod d3d;
#[cfg(windows)]
mod kernel;
#[cfg(windows)]
mod sys;
mod url;

pub use nkuplay_danmaku::{Comment as DanmakuComment, Mode as DanmakuMode};

/// 交给内核一集的弹幕，替换之前的；`shift` 是弹弹play 给的时间偏移（秒）。
/// 排版在内核线程上做。没有装上 libmpv 内核时什么也不做。
pub fn set_danmaku(comments: Vec<DanmakuComment>, shift: f64) {
    #[cfg(windows)]
    kernel::set_danmaku(Some((comments, shift)));
    #[cfg(not(windows))]
    let _ = (comments, shift);
}

/// 去掉弹幕：关闭视频或换片时调用。
pub fn clear_danmaku() {
    #[cfg(windows)]
    kernel::set_danmaku(None);
}

/// 显示或隐藏弹幕；隐藏时弹幕仍留在内核里，再显示时接着画。
pub fn show_danmaku(visible: bool) {
    #[cfg(windows)]
    kernel::show_danmaku(visible);
    #[cfg(not(windows))]
    let _ = visible;
}

/// 断点续播：下一次打开 `url` 时从 `position_ms` 毫秒处开始播。只对紧接着的那次打开有效，
/// 打开的是别的地址就作废。要在界面让播放器打开之前调用。没有装上 libmpv 内核时什么也不做。
pub fn start_at(url: &str, position_ms: i64) {
    #[cfg(windows)]
    kernel::start_at(Some((url.to_string(), position_ms)));
    #[cfg(not(windows))]
    let _ = (url, position_ms);
}

/// 把正在播放的这一帧画面（不含字幕和弹幕）存成 JPEG 图片 `path`。请求马上排进内核的队列，
/// 所以在让播放器打开下一个视频之前调用，存的就是现在这个视频的画面。存好了（为真）或没能存
/// （为假，比如没在播放）从交回的通道回话；没有装上 libmpv 内核时为空。
pub fn snapshot(path: std::path::PathBuf) -> Option<std::sync::mpsc::Receiver<bool>> {
    #[cfg(windows)]
    {
        kernel::snapshot(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// 不再指定下一次打开时从哪里开始。
pub fn no_start() {
    #[cfg(windows)]
    kernel::start_at(None);
}

/// 让 libmpv 成为应用的媒体内核。返回是否成功：`false` 表示库或图形设备不可用，
/// 仍由平台的播放器负责。只调用一次，在创建引擎时调用。
pub fn install() -> bool {
    #[cfg(windows)]
    {
        match kernel::MpvKernel::start() {
            Ok(kernel) => {
                kernel::remember(&kernel);
                bern::media::kernel::install(kernel);
                true
            }
            Err(reason) => {
                eprintln!("nkuplay-mpv: libmpv is not used: {reason}");
                false
            }
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}
