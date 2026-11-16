//! 继续观看和媒体库：最近打开过的视频、用户加入的文件夹。
//!
//! 两者都存在应用的数据文件夹里（`library.json`），下次启动时还在。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const FILE: &str = "library.json";
/// 继续观看最多记这么多个。
pub const MAX_RECENTS: usize = 12;

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
}

/// 媒体库里的一个文件夹。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderEntry {
    pub url: String,
    pub name: String,
    /// 上次数出的视频数。
    #[serde(default)]
    pub videos: i64,
}

/// 存进文件的全部内容。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
    /// 最近的在前。
    #[serde(default)]
    pub recents: Vec<RecentFile>,
    #[serde(default)]
    pub folders: Vec<FolderEntry>,
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
        });
        self.recents.insert(0, recent);
        self.recents.truncate(MAX_RECENTS);
    }

    /// 弹弹play 认出了 `url` 是哪一集：换上作品名、剧集名和海报。
    pub fn identified(&mut self, url: &str, title: &str, episode: &str, poster: Option<&str>) {
        if let Some(recent) = self.recents.iter_mut().find(|recent| recent.url == url) {
            recent.title = title.to_string();
            recent.episode = episode.to_string();
            recent.poster = poster.map(str::to_string);
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
        });
        true
    }
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

/// 数出文件夹 `url` 里（含子文件夹，跳过隐藏的）有多少个视频。会阻塞。
pub fn count_videos(url: &str) -> Result<i64, String> {
    let entries = bern::files::walk(url, |entry| !entry.name.starts_with('.'))
        .map_err(|error| error.message)?;
    Ok(entries
        .iter()
        .filter(|entry| !entry.folder && is_video(&entry.name))
        .count() as i64)
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
        for index in 0..20 {
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
    fn videos_in_a_folder_and_its_subfolders_are_counted() {
        let root = std::env::temp_dir().join(format!("nkuplay-library-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("S1")).unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        for name in ["01.mkv", "S1/02.mp4", "S1/02.ass", ".hidden/03.mkv"] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        let url = bern::media::MediaSource::file(&root).url;
        assert_eq!(count_videos(&url), Ok(2));
        let _ = std::fs::remove_dir_all(&root);
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
        store.save(&library);
        assert_eq!(Store::new(Some(folder.clone())).load(), library);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
