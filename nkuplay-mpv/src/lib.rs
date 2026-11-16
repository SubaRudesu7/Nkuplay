//! 把 libmpv 作为 Nkuplay 的媒体内核。
//!
//! [`install`] 在运行时加载 `libmpv-2.dll`；库和 Direct3D 11 设备都可用时，
//! 向 Bern 注册一个内核：此后每个 `Video` 视图都会给它一个 `SwapChainPanel` 来画，
//! 标准媒体命令（打开、播放、暂停、跳转、音量）也交给 mpv，而不是系统播放器。
//! 找不到库时应用照常启动，用平台自带的媒体设施播放。

#[cfg(windows)]
mod d3d;
#[cfg(windows)]
mod kernel;
#[cfg(windows)]
mod sys;
mod url;

/// 让 libmpv 成为应用的媒体内核。返回是否成功：`false` 表示库或图形设备不可用，
/// 仍由平台的播放器负责。只调用一次，在创建引擎时调用。
pub fn install() -> bool {
    #[cfg(windows)]
    {
        match kernel::MpvKernel::start() {
            Ok(kernel) => {
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
