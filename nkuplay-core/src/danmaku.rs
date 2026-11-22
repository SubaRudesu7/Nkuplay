//! 为一个视频取弹幕：算指纹，问弹弹play 这是哪一集，再取那一集的弹幕。
//!
//! 这些调用会阻塞（读文件、发网络请求），所以引擎把 [`Loader::load`] 放到别的线程上跑。
//! 只在开始播放视频时调用。两样结果分开缓存，都记在内存里，也存进缓存文件夹：
//!
//! - 文件是哪一集：不会变，一直保留（`match/<文件哈希>.json`）。
//! - 那一集的弹幕：保留 [`KEEP`]（`comments/<剧集编号>.json`），所有作品一样（用户定的）。

use bern::media::MediaSource;
use nkuplay_dandanplay::{
    ChineseConversion, Client, Comment, CommentMode, Credentials, EpisodeMatch, Error, Fingerprint,
    MatchRequest, MatchResult, fingerprint_reader,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 一集的弹幕保留多久。
pub const KEEP: Duration = Duration::from_secs(30 * 60);

/// 弹弹play 的调用里引擎用到的那两个，测试时可以换成假的。
pub trait Api: Send + Sync {
    fn match_file(&self, request: &MatchRequest) -> Result<MatchResult, Error>;
    /// 某一集的全部弹幕，含弹弹play 从其他网站合并来的。
    fn comments(&self, episode_id: i64) -> Result<Vec<Comment>, Error>;
}

impl Api for Client {
    fn match_file(&self, request: &MatchRequest) -> Result<MatchResult, Error> {
        Client::match_file(self, request)
    }

    fn comments(&self, episode_id: i64) -> Result<Vec<Comment>, Error> {
        Client::comments(self, episode_id, true, ChineseConversion::None)
    }
}

/// 一个视频的弹幕，以及它被识别成了哪一集。
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub episode_id: i64,
    pub anime_id: i64,
    pub anime_title: String,
    pub episode_title: String,
    /// 弹幕应延后出现的秒数；负数表示提前。
    pub shift: f64,
    /// 作品的海报地址。
    pub poster: Option<String>,
    /// 按出现时间排序。
    pub comments: Arc<[Comment]>,
}

/// 一个文件被识别成的那一集。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Episode {
    episode_id: i64,
    anime_id: i64,
    anime_title: String,
    episode_title: String,
    shift: f64,
    poster: Option<String>,
}

impl From<EpisodeMatch> for Episode {
    fn from(episode: EpisodeMatch) -> Self {
        Self {
            episode_id: episode.episode_id,
            anime_id: episode.anime_id,
            anime_title: episode.anime_title,
            episode_title: episode.episode_title,
            shift: episode.shift,
            poster: episode.image_url,
        }
    }
}

/// 某一集的弹幕，以及取到的时间（Unix 秒）。
#[derive(Clone)]
struct Fetched {
    at: u64,
    comments: Arc<[Comment]>,
}

/// 存进缓存文件的一份弹幕。
#[derive(Serialize, Deserialize)]
struct StoredComments {
    saved_at: u64,
    comments: Vec<StoredComment>,
}

#[derive(Serialize, Deserialize)]
struct StoredComment {
    id: i64,
    time: f64,
    /// 0 滚动，1 底部，2 顶部。
    mode: u8,
    color: u32,
    user: String,
    text: String,
}

impl StoredComments {
    fn of(fetched: &Fetched) -> Self {
        Self {
            saved_at: fetched.at,
            comments: fetched
                .comments
                .iter()
                .map(|comment| StoredComment {
                    id: comment.id,
                    time: comment.time,
                    mode: match comment.mode {
                        CommentMode::Scroll => 0,
                        CommentMode::Bottom => 1,
                        CommentMode::Top => 2,
                    },
                    color: comment.color,
                    user: comment.user.clone(),
                    text: comment.text.clone(),
                })
                .collect(),
        }
    }

    fn into_fetched(self) -> Fetched {
        Fetched {
            at: self.saved_at,
            comments: self
                .comments
                .into_iter()
                .map(|comment| Comment {
                    id: comment.id,
                    time: comment.time,
                    mode: match comment.mode {
                        1 => CommentMode::Bottom,
                        2 => CommentMode::Top,
                        _ => CommentMode::Scroll,
                    },
                    color: comment.color,
                    user: comment.user,
                    text: comment.text,
                })
                .collect(),
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// 取弹幕的结果。
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Loaded(Loaded),
    /// 弹弹play 没有精确认出这个文件。
    NoMatch,
    /// 出错了，附带给用户看的原因。
    Failed(String),
}

pub struct Loader {
    api: Box<dyn Api>,
    /// 文件哈希到它是哪一集。
    episodes: Mutex<HashMap<String, Episode>>,
    /// 剧集编号到它的弹幕。
    comments: Mutex<HashMap<i64, Fetched>>,
    /// 存放缓存文件的文件夹；没有时只记在内存里。
    folder: Option<PathBuf>,
}

impl Loader {
    /// 不存缓存文件的；测试用。
    #[cfg(test)]
    pub fn new(api: impl Api + 'static) -> Self {
        Self::with_folder(api, None)
    }

    pub fn with_folder(api: impl Api + 'static, folder: Option<PathBuf>) -> Self {
        Self {
            api: Box::new(api),
            episodes: Mutex::new(HashMap::new()),
            comments: Mutex::new(HashMap::new()),
            folder,
        }
    }

    /// 用编译进程序的凭证创建，缓存放在应用的缓存文件夹里；这个构建没有凭证时为 `None`。
    pub fn built_in() -> Option<Self> {
        let folder = bern::storage::cache_dir().map(|dir| dir.join("danmaku"));
        Credentials::built_in()
            .map(|credentials| Self::with_folder(Client::new(credentials), folder))
    }

    /// 缓存文件 `kind/<name>.json` 的位置。`name` 只能是十六进制哈希或数字，
    /// 其他东西不拿来拼路径。
    fn file(&self, kind: &str, name: &str) -> Option<PathBuf> {
        let safe = !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_hexdigit());
        let folder = self.folder.as_ref().filter(|_| safe)?;
        Some(folder.join(kind).join(format!("{name}.json")))
    }

    fn read<T: for<'de> Deserialize<'de>>(&self, kind: &str, name: &str) -> Option<T> {
        serde_json::from_slice(&std::fs::read(self.file(kind, name)?).ok()?).ok()
    }

    /// 写不进去不影响这次播放。
    fn write(&self, kind: &str, name: &str, value: &impl Serialize) {
        let Some(path) = self.file(kind, name) else {
            return;
        };
        if let Some(folder) = path.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        if let Ok(bytes) = serde_json::to_vec(value) {
            let _ = std::fs::write(path, bytes);
        }
    }

    /// 为 `source` 取弹幕。会阻塞：读文件的前 16 MiB，并发零到两次网络请求。
    pub fn load(&self, source: &MediaSource) -> Outcome {
        let file = match source.open() {
            Ok(file) => file,
            Err(error) => return Outcome::Failed(error.message),
        };
        let size = file.len();
        let name = source.file_name().unwrap_or_default();
        match fingerprint_reader(&name, file, size) {
            Ok(print) => self.load_fingerprint(print),
            Err(error) => Outcome::Failed(error.to_string()),
        }
    }

    /// 为指纹是 `print` 的文件取弹幕。
    /// 只认出 `source` 是哪部作品，不取弹幕：媒体库的海报墙用。认不出时为空。
    /// 和开始播放时共用同一份识别缓存，认过的文件不再发请求。会阻塞。
    pub fn identify(
        &self,
        source: &MediaSource,
    ) -> Result<Option<crate::library::Identity>, String> {
        let file = source.open().map_err(|error| error.message)?;
        let size = file.len();
        let name = source.file_name().unwrap_or_default();
        let print = fingerprint_reader(&name, file, size).map_err(|error| error.to_string())?;
        Ok(self
            .episode(print)?
            .map(|episode| crate::library::Identity {
                anime_id: episode.anime_id,
                title: episode.anime_title,
                poster: episode.poster,
            }))
    }

    pub fn load_fingerprint(&self, print: Fingerprint) -> Outcome {
        let episode = match self.episode(print) {
            Ok(Some(episode)) => episode,
            Ok(None) => return Outcome::NoMatch,
            Err(reason) => return Outcome::Failed(reason),
        };
        let comments = match self.episode_comments(episode.episode_id, KEEP) {
            Ok(comments) => comments,
            Err(reason) => return Outcome::Failed(reason),
        };
        Outcome::Loaded(Loaded {
            episode_id: episode.episode_id,
            anime_id: episode.anime_id,
            anime_title: episode.anime_title,
            episode_title: episode.episode_title,
            shift: episode.shift,
            poster: episode.poster,
            comments,
        })
    }

    /// 文件是哪一集：先看内存和缓存文件，都没有才问弹弹play。
    /// 只接受精确匹配；候选结果需要用户来选，不替用户猜。没认出来的不缓存。
    fn episode(&self, print: Fingerprint) -> Result<Option<Episode>, String> {
        let hash = print.file_hash.clone();
        if let Some(episode) = self.episodes.lock().expect("episodes").get(&hash) {
            return Ok(Some(episode.clone()));
        }
        let episode = match self.read::<Episode>("match", &hash) {
            Some(episode) => episode,
            None => {
                let result = self
                    .api
                    .match_file(&MatchRequest::from(print))
                    .map_err(|error| error.to_string())?;
                let Some(found) = result
                    .matches
                    .into_iter()
                    .next()
                    .filter(|_| result.is_matched)
                else {
                    return Ok(None);
                };
                let episode = Episode::from(found);
                self.write("match", &hash, &episode);
                episode
            }
        };
        self.episodes
            .lock()
            .expect("episodes")
            .insert(hash, episode.clone());
        Ok(Some(episode))
    }

    /// 一集的弹幕：内存或缓存文件里有 `keep` 之内取到的就用它，否则问弹弹play。
    fn episode_comments(&self, episode_id: i64, keep: Duration) -> Result<Arc<[Comment]>, String> {
        let fresh = |fetched: &Fetched| now().saturating_sub(fetched.at) < keep.as_secs();
        let name = episode_id.to_string();
        let remembered = self
            .comments
            .lock()
            .expect("comments")
            .get(&episode_id)
            .cloned()
            .or_else(|| {
                self.read::<StoredComments>("comments", &name)
                    .map(StoredComments::into_fetched)
            });
        if let Some(fetched) = remembered.filter(|fetched| fresh(fetched)) {
            self.comments
                .lock()
                .expect("comments")
                .insert(episode_id, fetched.clone());
            return Ok(fetched.comments);
        }
        let comments: Arc<[Comment]> = self
            .api
            .comments(episode_id)
            .map_err(|error| error.to_string())?
            .into();
        let fetched = Fetched {
            at: now(),
            comments: Arc::clone(&comments),
        };
        self.write("comments", &name, &StoredComments::of(&fetched));
        self.comments
            .lock()
            .expect("comments")
            .insert(episode_id, fetched);
        Ok(comments)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 假的弹弹play：按设定好的内容应答，并数被调用了几次。
    #[derive(Default)]
    pub(crate) struct FakeApi {
        pub matched: bool,
        pub episodes: Vec<EpisodeMatch>,
        pub comments: Vec<Comment>,
        pub match_error: Option<&'static str>,
        pub comments_error: Option<&'static str>,
        pub match_calls: Arc<AtomicUsize>,
        pub comment_calls: Arc<AtomicUsize>,
    }

    impl FakeApi {
        /// 精确认出一集（作品编号 12345）、带两条弹幕。
        pub(crate) fn recognising() -> Self {
            Self {
                matched: true,
                episodes: vec![episode(123450001, "某作品", "第1话")],
                comments: vec![comment(1, 1.0, "第一条"), comment(2, 2.5, "第二条")],
                ..Self::default()
            }
        }
    }

    impl Api for FakeApi {
        fn match_file(&self, _request: &MatchRequest) -> Result<MatchResult, Error> {
            self.match_calls.fetch_add(1, Ordering::SeqCst);
            match self.match_error {
                Some(reason) => Err(Error::Transport(reason.into())),
                None => Ok(MatchResult {
                    is_matched: self.matched,
                    matches: self.episodes.clone(),
                }),
            }
        }

        fn comments(&self, _episode_id: i64) -> Result<Vec<Comment>, Error> {
            self.comment_calls.fetch_add(1, Ordering::SeqCst);
            match self.comments_error {
                Some(reason) => Err(Error::Transport(reason.into())),
                None => Ok(self.comments.clone()),
            }
        }
    }

    pub(crate) fn episode(id: i64, anime: &str, title: &str) -> EpisodeMatch {
        EpisodeMatch {
            episode_id: id,
            anime_id: id / 10000,
            anime_title: anime.into(),
            episode_title: title.into(),
            type_description: String::new(),
            shift: 0.0,
            image_url: None,
        }
    }

    fn comment(id: i64, time: f64, text: &str) -> Comment {
        Comment {
            id,
            time,
            mode: CommentMode::Scroll,
            color: 0xFFFFFF,
            user: String::new(),
            text: text.into(),
        }
    }

    fn print(hash: &str) -> Fingerprint {
        Fingerprint {
            file_name: "Show - 01".into(),
            file_hash: hash.into(),
            file_size: 100,
        }
    }

    /// 系统临时文件夹里一个空的子文件夹，销毁时删除。
    struct Scratch(PathBuf);

    impl Scratch {
        fn named(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("nkuplay-danmaku-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            Scratch(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 把缓存文件里第 `episode_id` 集弹幕的取到时间改到 `age` 之前。
    fn age_comments(folder: &std::path::Path, episode_id: i64, age: Duration) {
        let path = folder.join("comments").join(format!("{episode_id}.json"));
        let mut stored: StoredComments =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        stored.saved_at = now() - age.as_secs();
        std::fs::write(path, serde_json::to_vec(&stored).unwrap()).unwrap();
    }

    #[test]
    fn an_exact_match_gives_the_episode_and_its_comments() {
        let loader = Loader::new(FakeApi::recognising());
        let Outcome::Loaded(loaded) = loader.load_fingerprint(print("aa")) else {
            panic!("should load");
        };
        assert_eq!(loaded.episode_id, 123450001);
        assert_eq!(loaded.anime_id, 12345);
        assert_eq!(loaded.anime_title, "某作品");
        assert_eq!(loaded.episode_title, "第1话");
        assert_eq!(loaded.comments.len(), 2);
    }

    #[test]
    fn candidates_without_an_exact_match_are_not_guessed_from() {
        let api = FakeApi {
            matched: false,
            ..FakeApi::recognising()
        };
        let comment_calls = Arc::clone(&api.comment_calls);
        let loader = Loader::new(api);
        assert_eq!(loader.load_fingerprint(print("aa")), Outcome::NoMatch);
        assert_eq!(comment_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn the_same_file_is_asked_about_once() {
        let api = FakeApi::recognising();
        let (match_calls, comment_calls) =
            (Arc::clone(&api.match_calls), Arc::clone(&api.comment_calls));
        let loader = Loader::new(api);
        let first = loader.load_fingerprint(print("aa"));
        let second = loader.load_fingerprint(print("aa"));
        assert_eq!(first, second);
        assert_eq!(match_calls.load(Ordering::SeqCst), 1);
        assert_eq!(comment_calls.load(Ordering::SeqCst), 1);
        // 另一个文件要另外识别；它是同一集的话，弹幕不再取。
        loader.load_fingerprint(print("bb"));
        assert_eq!(match_calls.load(Ordering::SeqCst), 2);
        assert_eq!(comment_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failures_carry_their_reason_and_are_asked_again_next_time() {
        let api = FakeApi {
            match_error: Some("timed out"),
            ..FakeApi::recognising()
        };
        let match_calls = Arc::clone(&api.match_calls);
        let loader = Loader::new(api);
        let Outcome::Failed(reason) = loader.load_fingerprint(print("aa")) else {
            panic!("should fail");
        };
        assert!(reason.contains("timed out"), "{reason}");
        loader.load_fingerprint(print("aa"));
        assert_eq!(match_calls.load(Ordering::SeqCst), 2);

        let loader = Loader::new(FakeApi {
            comments_error: Some("refused"),
            ..FakeApi::recognising()
        });
        assert!(matches!(
            loader.load_fingerprint(print("aa")),
            Outcome::Failed(reason) if reason.contains("refused")
        ));
    }

    #[test]
    fn results_kept_on_disk_are_used_by_the_next_run_without_asking() {
        let folder = Scratch::named("disk");
        let first = Loader::with_folder(FakeApi::recognising(), Some(folder.0.clone()));
        let Outcome::Loaded(loaded) = first.load_fingerprint(print("cc")) else {
            panic!("should load");
        };
        assert!(folder.0.join("match").join("cc.json").exists());
        assert!(folder.0.join("comments").join("123450001.json").exists());

        // 下一次运行：内存里没有，从缓存文件读，不再问弹弹play。
        let api = FakeApi::recognising();
        let (match_calls, comment_calls) =
            (Arc::clone(&api.match_calls), Arc::clone(&api.comment_calls));
        let next = Loader::with_folder(api, Some(folder.0.clone()));
        assert_eq!(next.load_fingerprint(print("cc")), Outcome::Loaded(loaded));
        assert_eq!(match_calls.load(Ordering::SeqCst), 0);
        assert_eq!(comment_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn danmaku_is_kept_half_an_hour() {
        let folder = Scratch::named("keep");
        Loader::with_folder(FakeApi::recognising(), Some(folder.0.clone()))
            .load_fingerprint(print("ee"));
        let comment_calls = || {
            let api = FakeApi::recognising();
            let calls = Arc::clone(&api.comment_calls);
            let match_calls = Arc::clone(&api.match_calls);
            Loader::with_folder(api, Some(folder.0.clone())).load_fingerprint(print("ee"));
            // 是哪一集一直记着，不用再识别。
            assert_eq!(match_calls.load(Ordering::SeqCst), 0);
            calls.load(Ordering::SeqCst)
        };
        // 20 分钟前取的还能用，40 分钟前的要重新取。
        age_comments(&folder.0, 123450001, Duration::from_secs(20 * 60));
        assert_eq!(comment_calls(), 0);
        age_comments(&folder.0, 123450001, Duration::from_secs(40 * 60));
        assert_eq!(comment_calls(), 1);
    }

    #[test]
    fn a_source_that_is_not_a_readable_file_fails_without_asking() {
        let api = FakeApi::recognising();
        let match_calls = Arc::clone(&api.match_calls);
        let loader = Loader::new(api);
        assert!(matches!(
            loader.load(&MediaSource::file("no/such/file.mkv")),
            Outcome::Failed(_)
        ));
        assert_eq!(match_calls.load(Ordering::SeqCst), 0);
    }
}
