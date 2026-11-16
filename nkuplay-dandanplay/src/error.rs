use std::fmt;

/// 调用弹弹play 没有得到结果的原因。
#[derive(Debug)]
pub enum Error {
    /// 构建时没有提供 AppId 和 AppSecret。
    MissingCredentials,
    /// 请求没有到达服务器，或应答没有传回来。
    Transport(String),
    /// 服务器拒绝了请求。403 时 `reason` 是 `X-Error-Message` 里的说明，
    /// 例如 `Invalid Signature` 或 `Invalid Timestamp`。
    Status { code: u16, reason: Option<String> },
    /// 服务器应答说调用失败（`success: false`）。
    Api { code: i32, message: String },
    /// 应答不是 API 文档里描述的 JSON。
    Decode(String),
    /// 本地文件读不了。
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::MissingCredentials => {
                write!(f, "this build has no dandanplay AppId and AppSecret")
            }
            Error::Transport(reason) => write!(f, "dandanplay could not be reached: {reason}"),
            Error::Status {
                code,
                reason: Some(reason),
            } => write!(f, "dandanplay answered {code}: {reason}"),
            Error::Status { code, reason: None } => write!(f, "dandanplay answered {code}"),
            Error::Api { code, message } => write!(f, "dandanplay error {code}: {message}"),
            Error::Decode(reason) => write!(f, "unexpected answer from dandanplay: {reason}"),
            Error::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::Io(error)
    }
}
