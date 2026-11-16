//! Direct3D 11 部分：mpv 渲染所用的设备，以及视频视图的 `SwapChainPanel`
//! 所显示的合成交换链。

use std::ffi::c_void;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_FLAG, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
    D3D11_SDK_VERSION, D3D11CreateDevice, ID3D11Device, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2,
    IDXGIOutput, IDXGISwapChain1,
};
use windows::core::Interface;

/// 供 mpv 使用的硬件设备：BGRA 用于交换链；驱动支持时启用视频支持，用于硬件解码。
pub fn create_device() -> Result<ID3D11Device, String> {
    let create = |flags: D3D11_CREATE_DEVICE_FLAG| {
        let mut device = None;
        // SAFETY: 每个输出指针要么有效，要么不传。
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                flags,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )
        }
        .map(|()| device)
    };
    let device = create(D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT)
        .or_else(|_| create(D3D11_CREATE_DEVICE_BGRA_SUPPORT))
        .map_err(|error| format!("no Direct3D 11 device: {error}"))?;
    device.ok_or_else(|| "no Direct3D 11 device".to_string())
}

/// 设备的地址，用于 `mpv_d3d11_init_params`。
pub fn device_address(device: &ID3D11Device) -> *mut c_void {
    device.as_raw()
}

/// Bern 交过来的 `ISwapChainPanelNative`：先是 `IUnknown`，然后是 `SetSwapChain`。
/// WinUI 3 和系统 XAML 给这个接口的标识符不同，但内存布局相同，所以按布局直接调用。
#[repr(C)]
struct PanelVtbl {
    query_interface: usize,
    add_ref: usize,
    release: usize,
    set_swap_chain: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
}

/// 一个视频视图的交换链。
pub struct Surface {
    chain: IDXGISwapChain1,
    width: u32,
    height: u32,
}

impl Surface {
    /// 创建 `width` × `height` 像素的交换链，交给地址为 `panel` 的面板；
    /// 视图在挂接期间保证面板存活。
    pub fn attach(
        device: &ID3D11Device,
        panel: u64,
        width: u32,
        height: u32,
    ) -> Result<Self, String> {
        let (width, height) = (width.max(1), height.max(1));
        let describe = |error: windows::core::Error| format!("no swap chain: {error}");
        // SAFETY: 对一个存活设备的普通 COM 调用。
        let chain = unsafe {
            let factory: IDXGIFactory2 = device
                .cast::<IDXGIDevice>()
                .map_err(describe)?
                .GetAdapter()
                .map_err(describe)?
                .GetParent()
                .map_err(describe)?;
            let description = DXGI_SWAP_CHAIN_DESC1 {
                Width: width,
                Height: height,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                ..Default::default()
            };
            factory
                .CreateSwapChainForComposition(device, &description, None::<&IDXGIOutput>)
                .map_err(describe)?
        };
        let panel = panel as *mut *const PanelVtbl;
        // SAFETY: `panel` 是视图挂接时给的 `ISwapChainPanelNative`，在视图卸下之前一直存活，
        // 并且 Bern 允许从任意线程调用它。
        let result = unsafe { ((**panel).set_swap_chain)(panel.cast(), chain.as_raw()) };
        if result < 0 {
            return Err(format!(
                "the video view refused the swap chain (0x{result:08X})"
            ));
        }
        Ok(Self {
            chain,
            width,
            height,
        })
    }

    /// 跟随视图的新尺寸。返回尺寸是否真的变了。
    pub fn resize(&mut self, width: u32, height: u32) -> Result<bool, String> {
        let (width, height) = (width.max(1), height.max(1));
        if (width, height) == (self.width, self.height) {
            return Ok(false);
        }
        // SAFETY: 两帧之间不持有任何后缓冲。
        unsafe {
            self.chain.ResizeBuffers(
                0,
                width,
                height,
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            )
        }
        .map_err(|error| format!("the swap chain did not resize: {error}"))?;
        (self.width, self.height) = (width, height);
        Ok(true)
    }

    /// 下一帧要画进去的纹理。
    pub fn back_buffer(&self) -> Result<ID3D11Texture2D, String> {
        // SAFETY: 翻转模型交换链的 0 号缓冲就是当前的后缓冲。
        unsafe { self.chain.GetBuffer(0) }.map_err(|error| format!("no back buffer: {error}"))
    }

    /// 把画好的内容显示出来。视图的帧时钟已经按显示器节奏安排帧，
    /// 所以这里不再等待垂直同步。
    pub fn present(&self) {
        // SAFETY: 普通 COM 调用。呈现失败（设备丢失）表现为画面定格；
        // 下一次挂接会创建新的交换链。
        let _ = unsafe { self.chain.Present(0, DXGI_PRESENT(0)) };
    }
}

/// 纹理的地址，用于 `mpv_d3d11_fbo`。
pub fn texture_address(texture: &ID3D11Texture2D) -> *mut c_void {
    texture.as_raw()
}
