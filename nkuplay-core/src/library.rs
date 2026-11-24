//! 继续观看和媒体库：最近打开过的视频、用户加入的文件夹，以及文件夹里的番剧。
//!
//! 每个视频看到哪里（[`Watched`]）也记在这里，供详情页和断点续播用。
//!
//! 都存在应用的数据文件夹里（`library.json`），下次启动时还在。
//!
//! 文件夹里的视频按文件名归成一部一部的番（见 [`crate::naming`]）；每部番请弹弹play 认一集，
//! 拿到正式的作品名和海报（[`Identity`]）。不同文件夹、不同写法的同一部番，认出来是同一个作品的，
//! 在海报墙上合成一部（[`Library::shelf`]）。

use crate::naming;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

const FILE: &str = "library.json";
/// 最多记这么多个最近打开过的视频。
pub const MAX_RECENTS: usize = 48;
/// 继续播放最多显示这么多部。
pub const MAX_WATCHING: usize = 12;

/// 算作视频的扩展名（小写）。
const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "wmv", "flv", "webm", "ts", "m2ts", "rmvb", "mpg", "mpeg",
];

/// 最近打开过的一个视频。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentFile {
    /// 视频的地址（`file:///…`）。
    pub url: String,
    /// 作品名；弹弹play 认出之前是文件名。
    pub title: String,
    /// 剧集名；没认出时为空。
    #[serde(default)]
    pub episode: String,
    /// 作品的海报地址。
    #[serde(default)]
    pub poster: Option<String>,
    /// 上次离开时的画面（缓存文件夹里的图片）。
    #[serde(default)]
    pub shot: Option<PathBuf>,
    /// 那张画面几乎全黑；还没看过时为空。
    #[serde(default)]
    pub shot_black: Option<bool>,
    /// 弹弹play 的作品编号；没认出时为空。
    #[serde(default)]
    pub anime_id: Option<i64>,
}

impl RecentFile {
    /// 合并用的键：认出来的视频按作品名（每部番只留最近看的一集），没认出的各算各的。
    fn show(&self) -> &str {
        if self.anime_id.is_some() || self.poster.is_some() || !self.episode.is_empty() {
            &self.title
        } else {
            &self.url
        }
    }
}

/// 一部番详情页的背景怎么显示。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackdropChoice {
    /// 固定一张；否则轮换。
    pub fixed: bool,
    /// 固定的那张剧照的文件名；还没选过时为空。
    #[serde(default)]
    pub still: Option<String>,
}

/// 媒体库里的一个文件夹。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderEntry {
    pub url: String,
    pub name: String,
    /// 上次数出的视频数。
    #[serde(default)]
    pub videos: i64,
    /// 上次扫描时归出的番剧。
    #[serde(default)]
    pub series: Vec<SeriesEntry>,
}

/// 弹弹play 认出的一部作品。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub anime_id: i64,
    pub title: String,
    /// 海报地址。
    pub poster: Option<String>,
}

/// 文件夹里的一集。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeFile {
    /// 视频的地址（`file:///…`）。
    pub url: String,
    /// 文件名（带扩展名）。
    pub name: String,
    /// 第几集；文件名里认不出时为空。
    pub episode: Option<u32>,
}

/// 一个文件夹里按文件名归到一起的一部番。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesEntry {
    /// 归类用的键（[`naming::key`]）。
    pub key: String,
    /// 从文件名里认出的作品名。
    pub title: String,
    /// 弹弹play 认出的作品；还没认或认不出时为空。
    pub identity: Option<Identity>,
    /// 按集数排好。
    pub episodes: Vec<EpisodeFile>,
}

/// 海报墙上的一部番：同一作品在各个文件夹里的剧集合在一起。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShelfItem {
    /// 合并用的键：认出的作品按作品编号，没认出的按作品名。界面打开它时靠这个找回来。
    pub key: String,
    pub title: String,
    pub poster: Option<String>,
    /// 按集数排好，同一个文件只出现一次。
    pub episodes: Vec<EpisodeFile>,
}

/// 一个视频看到了哪里。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watched {
    /// 看到的位置，毫秒。
    pub position_ms: i64,
    /// 视频的总时长，毫秒。
    pub duration_ms: i64,
    /// 什么时候记下的（Unix 秒）；“上次观看”取最近的。
    pub at: u64,
}

impl Watched {
    /// 播到 90% 以上算看完。
    pub fn finished(&self) -> bool {
        self.duration_ms > 0
            && self.position_ms.saturating_mul(10) >= self.duration_ms.saturating_mul(9)
    }

    /// 看了百分之几，0 到 100。
    pub fn percent(&self) -> i64 {
        if self.duration_ms <= 0 {
            return 0;
        }
        (self.position_ms.clamp(0, self.duration_ms) * 100 / self.duration_ms).clamp(0, 100)
    }
}

/// 存进文件的全部内容。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    /// 最近的在前。
    #[serde(default)]
    pub recents: Vec<RecentFile>,
    #[serde(default)]
    pub folders: Vec<FolderEntry>,
    /// 视频地址到看到哪里。
    #[serde(default)]
    pub progress: BTreeMap<String, Watched>,
    /// 详情页的合并键到它的背景设置。
    #[serde(default)]
    pub backdrops: BTreeMap<String, BackdropChoice>,
    /// 从媒体库隐藏了的番（海报墙的合并键）。
    #[serde(default)]
    pub hidden: Vec<String>,
}

impl Library {
    /// 记下刚打开的视频：放到最前面，同一个视频不重复记，超出上限的丢掉。
    pub fn opened(&mut self, url: &str, file_name: &str) {
        let previous = self
            .recents
            .iter()
            .position(|recent| recent.url == url)
            .map(|at| self.recents.remove(at));
        let recent = previous.unwrap_or_else(|| RecentFile {
            url: url.to_string(),
            title: title_from_file_name(file_name),
            episode: String::new(),
            poster: None,
            shot: None,
            shot_black: None,
            anime_id: None,
        });
        self.recents.insert(0, recent);
        // 挤出去的那几个的画面也删掉。
        for dropped in self.recents.drain(MAX_RECENTS.min(self.recents.len())..) {
            if let Some(shot) = dropped.shot {
                let _ = std::fs::remove_file(shot);
            }
        }
    }

    /// 继续播放：每部番只留最近看的一集，最近的在前，最多 [`MAX_WATCHING`] 部。
    pub fn watching(&self) -> Vec<&RecentFile> {
        let mut seen = HashSet::new();
        self.recents
            .iter()
            .filter(|recent| seen.insert(recent.show()))
            .take(MAX_WATCHING)
            .collect()
    }

    /// 视频 `url` 是弹弹play 的哪部作品：开始播放时认出来的，没有时看媒体库里归到哪一部。
    pub fn anime_of(&self, recent: &RecentFile) -> Option<i64> {
        recent.anime_id.or_else(|| {
            self.folders
                .iter()
                .flat_map(|folder| &folder.series)
                .find(|series| series.episodes.iter().any(|e| e.url == recent.url))
                .and_then(|series| Some(series.identity.as_ref()?.anime_id))
        })
    }

    /// 把 `url` 记成看完了（时刻是 `at`）。总时长沿用以前记下的，不知道时随便给一个。
    pub fn finish(&mut self, url: &str, at: u64) {
        let duration = self
            .progress
            .get(url)
            .map_or(1, |watched| watched.duration_ms.max(1));
        self.record(url, duration, duration, at);
    }

    /// 去掉继续观看里 `keep` 说不要的视频，连同它们的画面。
    pub fn prune(&mut self, keep: impl Fn(&str) -> bool) {
        let (kept, dropped): (Vec<_>, Vec<_>) =
            self.recents.drain(..).partition(|recent| keep(&recent.url));
        self.recents = kept;
        for shot in dropped.into_iter().filter_map(|recent| recent.shot) {
            let _ = std::fs::remove_file(shot);
        }
    }

    /// 记下 `url` 上次离开时的画面 `shot`（`black`：几乎全黑），删掉之前的那张。
    /// 不在继续观看里的不记，删掉这张图。
    pub fn shot(&mut self, url: &str, shot: PathBuf, black: bool) {
        match self.recents.iter_mut().find(|recent| recent.url == url) {
            Some(recent) => {
                recent.shot_black = Some(black);
                if let Some(old) = recent.shot.replace(shot.clone())
                    && old != shot
                {
                    let _ = std::fs::remove_file(old);
                }
            }
            None => {
                let _ = std::fs::remove_file(shot);
            }
        }
    }

    /// 弹弹play 认出了 `url` 是作品 `anime_id` 的哪一集：换上作品名、剧集名和海报。
    pub fn identified(
        &mut self,
        url: &str,
        anime_id: i64,
        title: &str,
        episode: &str,
        poster: Option<&str>,
    ) {
        if let Some(recent) = self.recents.iter_mut().find(|recent| recent.url == url) {
            recent.anime_id = Some(anime_id);
            recent.title = title.to_string();
            recent.episode = episode.to_string();
            recent.poster = poster.map(str::to_string);
        }
    }

    /// 记下 `url` 看到了 `position_ms`（总时长 `duration_ms`），时刻是 `at`（Unix 秒）。
    /// 位置或时长不像样时不记。记下了则为真。
    pub fn record(&mut self, url: &str, position_ms: i64, duration_ms: i64, at: u64) -> bool {
        if url.is_empty() || duration_ms <= 0 || position_ms <= 0 {
            return false;
        }
        let watched = Watched {
            position_ms: position_ms.min(duration_ms),
            duration_ms,
            at,
        };
        self.progress.insert(url.to_string(), watched);
        true
    }

    /// 忘掉这些视频看到哪里。
    pub fn forget<'a>(&mut self, urls: impl IntoIterator<Item = &'a str>) {
        for url in urls {
            self.progress.remove(url);
        }
    }

    /// 加入一个文件夹；已经在库里的不重复加。加入了则为真。
    pub fn add_folder(&mut self, url: &str, name: &str) -> bool {
        if self.folders.iter().any(|folder| folder.url == url) {
            return false;
        }
        self.folders.push(FolderEntry {
            url: url.to_string(),
            name: name.to_string(),
            videos: 0,
            series: Vec::new(),
        });
        true
    }

    /// 海报墙：所有文件夹里的番，同一作品合成一部，按作品名排好。
    pub fn shelf(&self) -> Vec<ShelfItem> {
        let mut items: Vec<ShelfItem> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        for series in self.folders.iter().flat_map(|folder| &folder.series) {
            let (key, title, poster) = match &series.identity {
                Some(identity) => (
                    format!("anime:{}", identity.anime_id),
                    identity.title.clone(),
                    identity.poster.clone(),
                ),
                None => (format!("title:{}", series.key), series.title.clone(), None),
            };
            let at = *index.entry(key.clone()).or_insert_with(|| {
                items.push(ShelfItem {
                    key,
                    title,
                    poster,
                    episodes: Vec::new(),
                });
                items.len() - 1
            });
            items[at].episodes.extend(series.episodes.iter().cloned());
        }
        for item in &mut items {
            let mut seen = HashSet::new();
            item.episodes
                .retain(|episode| seen.insert(episode.url.clone()));
            sort_episodes(&mut item.episodes);
        }
        items.sort_by(|a, b| a.title.cmp(&b.title).then_with(|| a.key.cmp(&b.key)));
        items
    }

    /// 文件夹 `url` 里认得出作品的番，按归类键，供重新扫描时沿用，不必再问弹弹play。
    pub fn identities(&self, url: &str) -> HashMap<String, Identity> {
        self.folders
            .iter()
            .filter(|folder| folder.url == url)
            .flat_map(|folder| &folder.series)
            .filter_map(|series| Some((series.key.clone(), series.identity.clone()?)))
            .collect()
    }
}

/// 先按集数（认不出的排在最后），同集数再按文件名。
fn sort_episodes(episodes: &mut [EpisodeFile]) {
    episodes.sort_by(|a, b| {
        a.episode
            .unwrap_or(u32::MAX)
            .cmp(&b.episode.unwrap_or(u32::MAX))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// 一个视频文件：地址、文件名，以及它所在文件夹的名字（直接放在扫描的文件夹里时为空）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoFile {
    pub url: String,
    pub name: String,
    pub parent: String,
}

/// 把视频按文件名归成一部一部的番，沿用 `known` 里已经认出的作品。文件名里认不出作品名的
/// （例如 `01.mkv`），用所在文件夹的名字；都没有时用文件名本身。
pub fn group(files: &[VideoFile], known: &HashMap<String, Identity>) -> Vec<SeriesEntry> {
    let mut series: Vec<SeriesEntry> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for file in files {
        let parsed = naming::parse(&title_from_file_name(&file.name));
        let title = if !parsed.title.is_empty() {
            parsed.title
        } else {
            Some(naming::parse(&file.parent).title)
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| title_from_file_name(&file.name))
        };
        let key = naming::key(&title);
        let key = if key.is_empty() { title.clone() } else { key };
        let at = *index.entry(key.clone()).or_insert_with(|| {
            series.push(SeriesEntry {
                identity: known.get(&key).cloned(),
                key,
                title,
                episodes: Vec::new(),
            });
            series.len() - 1
        });
        series[at].episodes.push(EpisodeFile {
            url: file.url.clone(),
            name: file.name.clone(),
            episode: parsed.episode,
        });
    }
    for entry in &mut series {
        sort_episodes(&mut entry.episodes);
    }
    series
}

/// 不带扩展名的文件名，当作没认出时显示的标题。
fn title_from_file_name(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && extension.len() <= 5 => stem.to_string(),
        _ => name.to_string(),
    }
}

/// `name` 是不是视频文件。
pub fn is_video(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| {
        VIDEO_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
    })
}

/// 文件夹 `url` 里（含子文件夹，跳过隐藏的）的所有视频。会阻塞。
pub fn videos(url: &str) -> Result<Vec<VideoFile>, String> {
    let entries = bern::files::walk(url, |entry| !entry.name.starts_with('.'))
        .map_err(|error| error.message)?;
    Ok(entries
        .into_iter()
        .filter(|entry| !entry.folder && is_video(&entry.name))
        .map(|entry| {
            // `path` 是 `子文件夹/…/文件名`：倒数第二段就是所在文件夹。
            let mut parts = entry.path.rsplit('/');
            parts.next();
            VideoFile {
                parent: parts.next().unwrap_or_default().to_string(),
                url: entry.url,
                name: entry.name,
            }
        })
        .collect())
}

/// 读写 `library.json`。
pub struct Store {
    /// 数据文件夹；没有时不存。
    folder: Option<PathBuf>,
}

impl Store {
    pub fn new(folder: Option<PathBuf>) -> Self {
        Self { folder }
    }

    /// 存放数据的文件夹；没有时不存。
    pub fn folder(&self) -> Option<&PathBuf> {
        self.folder.as_ref()
    }

    /// 应用的数据文件夹。
    pub fn built_in() -> Self {
        Self::new(bern::storage::data_dir())
    }

    /// 存下的内容；没有或读不出来时是空的。
    pub fn load(&self) -> Library {
        self.folder
            .as_ref()
            .and_then(|folder| std::fs::read(folder.join(FILE)).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// 存下 `library`。写不进去不影响使用，只是下次启动时没有。
    pub fn save(&self, library: &Library) {
        let Some(folder) = &self.folder else {
            return;
        };
        let _ = std::fs::create_dir_all(folder);
        if let Ok(bytes) = serde_json::to_vec_pretty(library) {
            let _ = std::fs::write(folder.join(FILE), bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_video_comes_first_once_and_the_list_is_bounded() {
        let mut library = Library::default();
        library.opened("file:///a.mkv", "a.mkv");
        library.opened("file:///b.mkv", "b.mkv");
        library.opened("file:///a.mkv", "a.mkv");
        let urls: Vec<_> = library.recents.iter().map(|r| r.url.as_str()).collect();
        assert_eq!(urls, ["file:///a.mkv", "file:///b.mkv"]);
        assert_eq!(library.recents[0].title, "a");
        for index in 0..MAX_RECENTS + 8 {
            library.opened(&format!("file:///{index}.mp4"), "x.mp4");
        }
        assert_eq!(library.recents.len(), MAX_RECENTS);
    }

    #[test]
    fn an_identified_video_keeps_its_title_when_opened_again() {
        let mut library = Library::default();
        library.opened("file:///a.mkv", "[Group] Show - 01.mkv");
        library.identified(
            "file:///a.mkv",
            12345,
            "某作品",
            "第1话",
            Some("https://img/p.jpg"),
        );
        library.opened("file:///b.mkv", "b.mkv");
        library.opened("file:///a.mkv", "[Group] Show - 01.mkv");
        let first = &library.recents[0];
        assert_eq!(
            (first.title.as_str(), first.episode.as_str()),
            ("某作品", "第1话")
        );
        assert_eq!(first.poster.as_deref(), Some("https://img/p.jpg"));
    }

    #[test]
    fn a_new_shot_replaces_the_old_file_and_shots_of_unknown_videos_are_dropped() {
        let folder = std::env::temp_dir().join(format!("nkuplay-shots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = |name: &str| {
            let path = folder.join(name);
            std::fs::write(&path, b"jpg").unwrap();
            path
        };
        let mut library = Library::default();
        library.opened("file:///a.mkv", "a.mkv");
        let (first, second, stray) = (file("1.jpg"), file("2.jpg"), file("3.jpg"));
        library.shot("file:///a.mkv", first.clone(), true);
        library.shot("file:///a.mkv", second.clone(), false);
        assert!(!first.exists());
        assert_eq!(library.recents[0].shot.as_ref(), Some(&second));
        assert_eq!(library.recents[0].shot_black, Some(false));
        library.shot("file:///gone.mkv", stray.clone(), false);
        assert!(!stray.exists());
        // 挤出继续观看的，画面一起删。
        for index in 0..MAX_RECENTS {
            library.opened(&format!("file:///{index}.mkv"), "x.mkv");
        }
        assert!(!second.exists());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn only_the_latest_episode_of_each_show_is_kept_watching() {
        let mut library = Library::default();
        for (url, title, episode) in [
            ("file:///4.mkv", "尼古喵喵", "第4话"),
            ("file:///7.mkv", "尼古喵喵", "第7话"),
            ("file:///x.mkv", "别的", "第1话"),
        ] {
            library.opened(url, url);
            library.identified(url, 1, title, episode, None);
        }
        // 没认出的视频各算各的。
        library.opened("file:///a.mkv", "a.mkv");
        library.opened("file:///b/a.mkv", "a.mkv");
        let urls: Vec<&str> = library.watching().iter().map(|r| r.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "file:///b/a.mkv",
                "file:///a.mkv",
                "file:///x.mkv",
                "file:///7.mkv"
            ]
        );
        // 再看第 4 话，它就换到前面。
        library.opened("file:///4.mkv", "4.mkv");
        assert_eq!(library.watching()[0].url, "file:///4.mkv");
        // 以前的记录没有作品编号：看媒体库里归到哪一部。
        library.recents[0].anime_id = None;
        assert_eq!(library.anime_of(&library.recents[0]), None);
        library.add_folder("file:///D:/A", "A");
        let mut shelf = group(&[file("尼古喵喵 - 04.mkv", "x")], &HashMap::new());
        shelf[0].episodes[0].url = "file:///4.mkv".into();
        shelf[0].identity = Some(Identity {
            anime_id: 7,
            title: "尼古喵喵".into(),
            poster: None,
        });
        library.folders[0].series = shelf;
        assert_eq!(library.anime_of(&library.recents[0]), Some(7));
        assert_eq!(library.watching().len(), 4);
    }

    #[test]
    fn a_video_can_be_marked_as_finished() {
        let mut library = Library::default();
        library.finish("file:///a.mkv", 1);
        assert!(library.progress["file:///a.mkv"].finished());
        library.record("file:///b.mkv", 100, 1_000, 1);
        library.finish("file:///b.mkv", 2);
        assert_eq!(library.progress["file:///b.mkv"].duration_ms, 1_000);
        assert!(library.progress["file:///b.mkv"].finished());
    }

    #[test]
    fn a_folder_is_added_once() {
        let mut library = Library::default();
        assert!(library.add_folder("file:///D:/Anime", "Anime"));
        assert!(!library.add_folder("file:///D:/Anime", "Anime"));
        assert_eq!(library.folders.len(), 1);
    }

    #[test]
    fn videos_are_told_by_extension() {
        assert!(is_video("a.MKV"));
        assert!(is_video("x.y.mp4"));
        assert!(!is_video("a.ass"));
        assert!(!is_video("mkv"));
    }

    #[test]
    fn videos_in_a_folder_and_its_subfolders_are_found_with_their_folder() {
        let root = std::env::temp_dir().join(format!("nkuplay-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("S1")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        for name in ["01.mkv", "S1/02.mp4", "S1/02.ass", ".hidden/03.mkv"] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        let url = bern::media::MediaSource::file(&root).url;
        let mut found = videos(&url).unwrap();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        let found: Vec<(&str, &str)> = found
            .iter()
            .map(|v| (v.name.as_str(), v.parent.as_str()))
            .collect();
        assert_eq!(found, [("01.mkv", ""), ("02.mp4", "S1")]);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn file(name: &str, parent: &str) -> VideoFile {
        VideoFile {
            url: format!("file:///D:/Anime/{parent}/{name}"),
            name: name.into(),
            parent: parent.into(),
        }
    }

    #[test]
    fn episodes_of_one_show_are_grouped_and_ordered() {
        let files = [
            file("[LoliHouse] Yani Neko - 09 [WebRip 1080p].mkv", "a"),
            file("[ANi] 再見菈菈 - 11 [1080P].mp4", "b"),
            file("[SFBgm] Yani Neko - 08 [WebRip 1080p AVC].mp4", "c"),
            file("01.mkv", "[Group] 某作品 S01 [1080P]"),
            file("02.mkv", "[Group] 某作品 S01 [1080P]"),
            file("extra.mkv", ""),
        ];
        let series = group(&files, &HashMap::new());
        let summary: Vec<(&str, Vec<Option<u32>>)> = series
            .iter()
            .map(|s| {
                (
                    s.title.as_str(),
                    s.episodes.iter().map(|e| e.episode).collect(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("Yani Neko", vec![Some(8), Some(9)]),
                ("再見菈菈", vec![Some(11)]),
                ("某作品", vec![Some(1), Some(2)]),
                ("extra", vec![None]),
            ]
        );
    }

    #[test]
    fn known_shows_keep_their_identity_and_the_same_work_is_merged_on_the_shelf() {
        let lara = Identity {
            anime_id: 18643,
            title: "再见，拉拉".into(),
            poster: Some("https://img/lara.jpg".into()),
        };
        let known = HashMap::from([(naming::key("再見菈菈"), lara.clone())]);
        let mut library = Library::default();
        library.add_folder("file:///D:/A", "A");
        library.add_folder("file:///D:/B", "B");
        library.folders[0].series = group(
            &[
                file("[ANi] 再見菈菈 - 11 [1080P].mp4", "x"),
                file("[ANi] Other - 01.mp4", "x"),
            ],
            &known,
        );
        assert_eq!(library.folders[0].series[0].identity.as_ref(), Some(&lara));
        // 另一个文件夹里英文名的同一部番，认出来也是这个作品。
        let mut english = group(
            &[file("[Nix-Raws] Sayonara Lara S01E06 [CR].mkv", "y")],
            &HashMap::new(),
        );
        english[0].identity = Some(lara.clone());
        library.folders[1].series = english;
        let shelf = library.shelf();
        assert_eq!(shelf.len(), 2);
        let merged = shelf.iter().find(|item| item.key == "anime:18643").unwrap();
        assert_eq!(merged.title, "再见，拉拉");
        assert_eq!(merged.poster.as_deref(), Some("https://img/lara.jpg"));
        let episodes: Vec<Option<u32>> = merged.episodes.iter().map(|e| e.episode).collect();
        assert_eq!(episodes, [Some(6), Some(11)]);
        assert_eq!(library.identities("file:///D:/A").len(), 1);
    }

    #[test]
    fn progress_is_kept_per_video_and_ninety_percent_counts_as_finished() {
        let mut library = Library::default();
        assert!(!library.record("file:///a.mkv", 0, 1_000, 1));
        assert!(!library.record("file:///a.mkv", 500, 0, 1));
        assert!(library.record("file:///a.mkv", 400, 1_000, 1));
        let watched = library.progress["file:///a.mkv"];
        assert_eq!((watched.percent(), watched.finished()), (40, false));
        library.record("file:///a.mkv", 900, 1_000, 2);
        assert!(library.progress["file:///a.mkv"].finished());
        // 超过总时长的位置按总时长记。
        library.record("file:///b.mkv", 5_000, 1_000, 3);
        assert_eq!(library.progress["file:///b.mkv"].percent(), 100);
        library.forget(["file:///a.mkv"]);
        assert_eq!(library.progress.len(), 1);
    }

    #[test]
    fn the_library_survives_a_restart() {
        let folder =
            std::env::temp_dir().join(format!("nkuplay-library-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let store = Store::new(Some(folder.clone()));
        assert_eq!(store.load(), Library::default());
        let mut library = Library::default();
        library.opened("file:///a.mkv", "a.mkv");
        library.add_folder("file:///D:/Anime", "Anime");
        library.record("file:///a.mkv", 60_000, 1_440_000, 1);
        store.save(&library);
        assert_eq!(Store::new(Some(folder.clone())).load(), library);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
