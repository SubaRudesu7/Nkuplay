//! Nkuplay 用到的那部分 libmpv C API（`mpv/client.h`、`mpv/render.h`、
//! `mpv/render_d3d11.h`）。在运行时加载，这样库缺失时只是内核不可用，
//! 而不会让应用启动不了。

use libloading::Library;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::{Path, PathBuf};

/// `mpv_handle`。
pub enum Handle {}
/// `mpv_render_context`。
pub enum RenderContext {}

pub const FORMAT_STRING: c_int = 1;
pub const FORMAT_FLAG: c_int = 3;
pub const FORMAT_DOUBLE: c_int = 5;

pub const EVENT_NONE: c_int = 0;
pub const EVENT_SHUTDOWN: c_int = 1;
pub const EVENT_START_FILE: c_int = 6;
pub const EVENT_END_FILE: c_int = 7;
pub const EVENT_FILE_LOADED: c_int = 8;

pub const END_FILE_REASON_ERROR: c_int = 4;

pub const RENDER_PARAM_INVALID: c_int = 0;
pub const RENDER_PARAM_API_TYPE: c_int = 1;
pub const RENDER_PARAM_BLOCK_FOR_TARGET_TIME: c_int = 12;
pub const RENDER_PARAM_D3D11_INIT_PARAMS: c_int = 21;
pub const RENDER_PARAM_D3D11_FBO: c_int = 22;
pub const RENDER_API_TYPE_D3D11: &CStr = c"d3d11";
pub const RENDER_UPDATE_FRAME: u64 = 1;

/// `mpv_event`。
#[repr(C)]
pub struct Event {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

/// `mpv_event_end_file`，只包含 Nkuplay 读取的部分。
#[repr(C)]
pub struct EventEndFile {
    pub reason: c_int,
    pub error: c_int,
}

/// `mpv_render_param`。
#[repr(C)]
pub struct RenderParam {
    pub kind: c_int,
    pub data: *mut c_void,
}

/// `mpv_d3d11_init_params`。
#[repr(C)]
pub struct D3d11InitParams {
    /// `ID3D11Device*`；mpv 会自己再持有一个引用。
    pub device: *mut c_void,
}

/// `mpv_d3d11_fbo`。
#[repr(C)]
pub struct D3d11Fbo {
    /// `ID3D11Texture2D*`，属于交给 mpv 的那个设备；mpv 不保留引用。
    pub tex: *mut c_void,
}

/// libmpv 的函数，以及承载它们的库。
pub struct Mpv {
    pub create: unsafe extern "C" fn() -> *mut Handle,
    pub initialize: unsafe extern "C" fn(*mut Handle) -> c_int,
    pub terminate_destroy: unsafe extern "C" fn(*mut Handle),
    pub set_option_string: unsafe extern "C" fn(*mut Handle, *const c_char, *const c_char) -> c_int,
    pub command: unsafe extern "C" fn(*mut Handle, *mut *const c_char) -> c_int,
    pub get_property: unsafe extern "C" fn(*mut Handle, *const c_char, c_int, *mut c_void) -> c_int,
    pub set_property: unsafe extern "C" fn(*mut Handle, *const c_char, c_int, *mut c_void) -> c_int,
    pub wait_event: unsafe extern "C" fn(*mut Handle, f64) -> *mut Event,
    pub get_property_string: unsafe extern "C" fn(*mut Handle, *const c_char) -> *mut c_char,
    pub free: unsafe extern "C" fn(*mut c_void),
    pub error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    pub render_context_create:
        unsafe extern "C" fn(*mut *mut RenderContext, *mut Handle, *mut RenderParam) -> c_int,
    pub render_context_update: unsafe extern "C" fn(*mut RenderContext) -> u64,
    pub render_context_render: unsafe extern "C" fn(*mut RenderContext, *mut RenderParam) -> c_int,
    pub render_context_report_swap: unsafe extern "C" fn(*mut RenderContext),
    pub render_context_free: unsafe extern "C" fn(*mut RenderContext),
    /// 让上面的函数保持已加载状态。
    _library: Library,
}

const LIBRARY: &str = "libmpv-2.dll";

/// 库可能在的位置，按尝试顺序：`NKUPLAY_LIBMPV` 指定的路径、可执行文件旁边
/// （打包后的应用）、项目的 `third_party/libmpv`（开发构建）。
fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("NKUPLAY_LIBMPV") {
        paths.push(PathBuf::from(path));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        paths.push(dir.join(LIBRARY));
    }
    paths.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../third_party/libmpv")
            .join(LIBRARY),
    );
    paths
}

impl Mpv {
    /// 从第一个找到的位置加载 libmpv。
    pub fn load() -> Result<Self, String> {
        let candidates = candidates();
        let path = candidates
            .iter()
            .find(|path| path.is_file())
            .ok_or_else(|| {
                let tried = candidates
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{LIBRARY} was not found (looked at {tried})")
            })?;
        // SAFETY: 加载 libmpv 会运行它的初始化代码，这些代码对调用方没有要求。
        let library = unsafe { Library::new(path) }
            .map_err(|error| format!("{} did not load: {error}", path.display()))?;
        macro_rules! symbol {
            ($name:literal) => {
                // SAFETY: 每个符号读成的类型，就是 libmpv 头文件为它声明的签名。
                *unsafe { library.get(concat!($name, "\0").as_bytes()) }
                    .map_err(|error| format!("{LIBRARY} has no `{}`: {error}", $name))?
            };
        }
        Ok(Self {
            create: symbol!("mpv_create"),
            initialize: symbol!("mpv_initialize"),
            terminate_destroy: symbol!("mpv_terminate_destroy"),
            set_option_string: symbol!("mpv_set_option_string"),
            command: symbol!("mpv_command"),
            get_property: symbol!("mpv_get_property"),
            set_property: symbol!("mpv_set_property"),
            wait_event: symbol!("mpv_wait_event"),
            get_property_string: symbol!("mpv_get_property_string"),
            free: symbol!("mpv_free"),
            error_string: symbol!("mpv_error_string"),
            render_context_create: symbol!("mpv_render_context_create"),
            render_context_update: symbol!("mpv_render_context_update"),
            render_context_render: symbol!("mpv_render_context_render"),
            render_context_report_swap: symbol!("mpv_render_context_report_swap"),
            render_context_free: symbol!("mpv_render_context_free"),
            _library: library,
        })
    }

    /// mpv 对某个错误码的文字说明。
    pub fn error_text(&self, code: c_int) -> String {
        // SAFETY: `mpv_error_string` 对任何错误码都返回静态字符串。
        unsafe { CStr::from_ptr((self.error_string)(code)) }
            .to_string_lossy()
            .into_owned()
    }
}
