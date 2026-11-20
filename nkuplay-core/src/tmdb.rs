//! TMDB 的剧集剧照，铺在详情页背景上。
//!
//! 要 TMDB 的 API 密钥：构建时从环境变量 `NKUPLAY_TMDB_KEY` 编译进程序（v3 的 API Key，
//! 或 v4 的 Read Access Token 都行），没有时不取剧照，背景用海报虚化。
//!
//! 在中国大陆 `api.tmdb.org` 直连能用（`api.themoviedb.org` 不通）；`image.tmdb.org` 直连
//! 握手常被掐断，所以每张图多试几次（连上后连接复用，后面的就顺了）。剧照列表和图片下载后一直保留（`stills/`），
//! 剧照不会变。

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

const API: &str = "https://api.tmdb.org/3";
const IMAGES: &str = "https://image.tmdb.org/t/p/w1280";
/// 一张图最多试几次。实测（2026-11-20）直连 `image.tmdb.org` 头几次握手常被掐断，连上一次以后
/// 同一个连接接着用，后面的图都能拿到，所以第一张多试几次。
const ATTEMPTS: usize = 10;
/// 一张图最大多少字节；超过的不要。
const MAX_IMAGE: u64 = 16 * 1024 * 1024;

/// TMDB 的调用里这里用到的那两个，测试时可以换成假的。
pub trait Api: Send + Sync {
    /// 一集的剧照，`file_path`（例如 `/abc.jpg`），按 TMDB 给的顺序。
    fn stills(&self, tv: i64, season: u32, episode: u32) -> Result<Vec<String>, String>;
    /// 一张图片的内容。
    fn image(&self, file_path: &str) -> Result<Vec<u8>, String>;
}

/// 真的 TMDB。
struct Http {
    key: String,
    agent: ureq::Agent,
}

#[derive(Deserialize)]
struct ImagesBody {
    #[serde(default)]
    stills: Vec<Still>,
}

#[derive(Deserialize)]
struct Still {
    file_path: String,
}

impl Http {
    fn new(key: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .user_agent(concat!("nkuplay/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self {
            key: key.to_string(),
            agent,
        }
    }

    /// v4 的令牌很长（JWT），放在请求头里；v3 的密钥是 32 位，放在查询参数里。
    fn bearer(&self) -> bool {
        self.key.len() > 40
    }
}

impl Api for Http {
    fn stills(&self, tv: i64, season: u32, episode: u32) -> Result<Vec<String>, String> {
        let url = format!("{API}/tv/{tv}/season/{season}/episode/{episode}/images");
        let mut request = self.agent.get(url);
        request = if self.bearer() {
            request.header("Authorization", format!("Bearer {}", self.key))
        } else {
            request.query("api_key", &self.key)
        };
        let mut response = request.call().map_err(|error| error.to_string())?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|error| error.to_string())?;
        let body: ImagesBody = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        Ok(body
            .stills
            .into_iter()
            .map(|still| still.file_path)
            .collect())
    }

    fn image(&self, file_path: &str) -> Result<Vec<u8>, String> {
        let response = self
            .agent
            .get(format!("{IMAGES}{file_path}"))
            .call()
            .map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(MAX_IMAGE)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        Ok(bytes)
    }
}

/// 缓存里的一集剧照列表。
#[derive(Serialize, Deserialize)]
struct Listed {
    stills: Vec<String>,
}

/// `/abc_1.jpg` 当文件名用：只允许字母、数字、点、下划线和连字符，否则不用它。
fn file_name(file_path: &str) -> Option<&str> {
    let name = file_path.trim_start_matches('/');
    let safe = !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte));
    safe.then_some(name)
}

/// 剧照的来源：向 TMDB 取，存进缓存文件夹。
pub struct Stills {
    /// 这个构建没有 TMDB 密钥时为 `None`。
    api: Option<Box<dyn Api>>,
    cache: Option<PathBuf>,
}

impl Stills {
    pub fn new(api: Option<Box<dyn Api>>, cache: Option<PathBuf>) -> Self {
        Self { api, cache }
    }

    /// 用编译进程序的密钥，缓存放在应用的缓存文件夹里。
    pub fn built_in() -> Self {
        let api = option_env!("NKUPLAY_TMDB_KEY")
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(|key| Box::new(Http::new(key)) as Box<dyn Api>);
        Self::new(
            api,
            bern::storage::cache_dir().map(|dir| dir.join("stills")),
        )
    }

    /// 能不能向 TMDB 取。
    pub fn available(&self) -> bool {
        self.api.is_some()
    }

    fn list_file(&self, tv: i64, season: u32, episode: u32) -> Option<PathBuf> {
        Some(
            self.cache
                .as_ref()?
                .join(format!("{tv}-{season}-{episode}.json")),
        )
    }

    fn image_file(&self, file_path: &str) -> Option<PathBuf> {
        Some(
            self.cache
                .as_ref()?
                .join("images")
                .join(file_name(file_path)?),
        )
    }

    /// 已经下载好的剧照，不发请求；没有时为空。只读一个小文件。
    pub fn cached(&self, tv: i64, season: u32, episode: u32) -> Vec<PathBuf> {
        let listed: Option<Listed> = self
            .list_file(tv, season, episode)
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        listed
            .map(|listed| listed.stills)
            .unwrap_or_default()
            .iter()
            .filter_map(|still| self.image_file(still))
            .filter(|path| path.is_file())
            .collect()
    }

    /// 一集的剧照，最多 `limit` 张，下载好存进缓存，交回图片文件。会阻塞。
    /// 有几张下载不下来不算失败，交回下载好的那些。
    pub fn fetch(
        &self,
        tv: i64,
        season: u32,
        episode: u32,
        limit: usize,
    ) -> Result<Vec<PathBuf>, String> {
        let api = self
            .api
            .as_ref()
            .ok_or_else(|| "this build has no TMDB key".to_string())?;
        let list_file = self.list_file(tv, season, episode);
        let listed: Option<Listed> = list_file
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let stills = match listed {
            Some(listed) => listed.stills,
            None => {
                let stills = api.stills(tv, season, episode)?;
                if let Some(path) = &list_file {
                    write(
                        path,
                        &serde_json::to_vec(&Listed {
                            stills: stills.clone(),
                        })
                        .unwrap_or_default(),
                    );
                }
                stills
            }
        };
        let mut found = Vec::new();
        for still in stills
            .iter()
            .filter(|still| file_name(still).is_some())
            .take(limit)
        {
            let Some(path) = self.image_file(still) else {
                continue;
            };
            if !path.is_file() {
                let bytes =
                    (0..ATTEMPTS).find_map(|_| api.image(still).ok().filter(|b| !b.is_empty()));
                match bytes {
                    Some(bytes) => write(&path, &bytes),
                    None => continue,
                }
            }
            if path.is_file() {
                found.push(path);
            }
        }
        Ok(found)
    }
}

/// 写一个缓存文件，先写到旁边再改名，免得留下写了一半的图片。写不进去就算了。
fn write(path: &Path, bytes: &[u8]) {
    if let Some(folder) = path.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let partial = path.with_extension("part");
    if std::fs::write(&partial, bytes).is_ok() {
        let _ = std::fs::rename(&partial, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 假的 TMDB：每张图前 `failures` 次失败。
    struct FakeApi {
        stills: Vec<String>,
        failures: usize,
        tries: Mutex<std::collections::HashMap<String, usize>>,
        list_calls: AtomicUsize,
    }

    impl Api for FakeApi {
        fn stills(&self, _tv: i64, _season: u32, _episode: u32) -> Result<Vec<String>, String> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.stills.clone())
        }

        fn image(&self, file_path: &str) -> Result<Vec<u8>, String> {
            let mut tries = self.tries.lock().unwrap();
            let count = tries.entry(file_path.to_string()).or_default();
            *count += 1;
            if *count <= self.failures {
                Err("handshake failed".into())
            } else {
                Ok(file_path.as_bytes().to_vec())
            }
        }
    }

    #[test]
    fn only_plain_file_names_are_used_for_the_cache() {
        assert_eq!(file_name("/abc_1-x.jpg"), Some("abc_1-x.jpg"));
        assert_eq!(file_name("/../secret"), None);
        assert_eq!(file_name("/a/b.jpg"), None);
        assert_eq!(file_name(""), None);
    }

    #[test]
    fn stills_are_retried_and_kept() {
        let folder = std::env::temp_dir().join(format!("nkuplay-stills-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let api = FakeApi {
            stills: vec![
                "/a.jpg".into(),
                "/../bad".into(),
                "/b.jpg".into(),
                "/c.jpg".into(),
            ],
            failures: 2,
            tries: Mutex::default(),
            list_calls: AtomicUsize::new(0),
        };
        let stills = Stills::new(Some(Box::new(api)), Some(folder.clone()));
        assert!(stills.cached(1, 1, 7).is_empty());
        let found = stills.fetch(1, 1, 7, 2).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(std::fs::read(&found[1]).unwrap(), b"/b.jpg");
        assert_eq!(stills.cached(1, 1, 7), found);
        // 再取一次：列表和图片都在缓存里。
        assert_eq!(stills.fetch(1, 1, 7, 2).unwrap(), found);
        assert!(!Stills::new(None, Some(folder.clone())).available());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
