//! 请求签名（API 的“签名验证模式”）：AppSecret 不离开本进程，发出去的只是一个哈希，
//! 只对一个路径、一小段时间有效。

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::{Digest, Sha256};
use std::fmt;

/// 弹弹play 为每个应用签发的 AppId 和 AppSecret（<https://doc.dandanplay.com/open/>）。
/// 它们不放在本仓库里：构建时从环境变量编译进程序，见 [`Credentials::built_in`]。
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub app_id: String,
    pub app_secret: String,
}

impl Credentials {
    pub fn new(app_id: impl Into<String>, app_secret: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            app_secret: app_secret.into(),
        }
    }

    /// 编译进本程序的凭证：构建时环境变量 `DANDANPLAY_APP_ID` 和
    /// `DANDANPLAY_APP_SECRET` 的值。构建时两者没有都设置（或为空）则没有凭证，
    /// 弹幕功能应当关闭。
    ///
    /// 发版构建由 CI 从仓库机密里取值设置这两个变量；本地开发在自己的终端里设置。
    /// 值是在编译时取的，改了环境变量要重新构建才生效（Cargo 会自动察觉并重编）。
    pub fn built_in() -> Option<Self> {
        Self::from_parts(
            option_env!("DANDANPLAY_APP_ID"),
            option_env!("DANDANPLAY_APP_SECRET"),
        )
    }

    /// 两部分都有且去掉首尾空白后非空时，才算有凭证。
    fn from_parts(app_id: Option<&str>, app_secret: Option<&str>) -> Option<Self> {
        fn present(value: Option<&str>) -> Option<&str> {
            value.map(str::trim).filter(|value| !value.is_empty())
        }
        Some(Self::new(present(app_id)?, present(app_secret)?))
    }

    /// 在 `timestamp`（Unix 秒）时刻请求 `path`（以 `/` 开头，不含查询参数）所用的
    /// `X-Signature`：`base64(sha256(AppId + Timestamp + Path + AppSecret))`。
    pub fn signature(&self, timestamp: u64, path: &str) -> String {
        let mut hash = Sha256::new();
        hash.update(self.app_id.as_bytes());
        hash.update(timestamp.to_string().as_bytes());
        hash.update(path.as_bytes());
        hash.update(self.app_secret.as_bytes());
        STANDARD.encode(hash.finalize())
    }
}

/// 密钥不出现在日志和 panic 信息里。
impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("app_id", &self.app_id)
            .field("app_secret", &"…")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_need_both_parts_and_neither_empty() {
        assert_eq!(
            Credentials::from_parts(Some(" my-app "), Some("my-secret\n")),
            Some(Credentials::new("my-app", "my-secret"))
        );
        assert_eq!(Credentials::from_parts(Some("my-app"), None), None);
        assert_eq!(Credentials::from_parts(None, Some("my-secret")), None);
        assert_eq!(Credentials::from_parts(Some("my-app"), Some("  ")), None);
        assert_eq!(Credentials::from_parts(Some(""), Some("my-secret")), None);
    }

    #[test]
    fn signature_is_the_documented_hash() {
        // base64(sha256("your_app_id" + "1735660800" + "/api/v2/comment/123450001"
        // + "your_app_secret"))，用 Python 的 hashlib 和 base64 算出。
        let credentials = Credentials::new("your_app_id", "your_app_secret");
        assert_eq!(
            credentials.signature(1735660800, "/api/v2/comment/123450001"),
            "MNBT8iOsIplI/GSkJEAH3V1AlpTyH1aMPJ1nedenEsw="
        );
    }

    #[test]
    fn debug_output_hides_the_secret() {
        let shown = format!("{:?}", Credentials::new("id", "hunter2"));
        assert!(shown.contains("id"));
        assert!(!shown.contains("hunter2"));
    }
}
