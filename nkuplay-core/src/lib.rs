//! Nkuplay 的引擎：业务逻辑，纯 Rust。它不知道窗口的存在，所以不需要窗口就能测试。

mod danmaku;
mod library;
mod shin;

use bern::CommandError;
use bern::media::MediaSource;
use chrono::Datelike;
use danmaku::{Loader, Outcome};
use library::Library;
use nkuplay_dandanplay::Comment;
use nkuplay_protocol::nkuplay::{
    self, DanmakuStatus, Folder, Recent, ShinAnime, ShinStatus, State, Weekday,
};
use shin::Season;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// 交给别的线程去做的一件事。
type Job = Box<dyn FnOnce() + Send>;

/// 后台做完的事。
enum Arrival {
    /// 一份弹幕结果，带着发起时的 `generation`。
    Danmaku(u64, Outcome),
    /// 取新番列表的结果。
    Shin(Result<Season, String>),
    /// 数出的一个文件夹里的视频数。
    Folder(String, Result<i64, String>),
}

/// 今天是哪一天、属于哪一季；测试时可以固定下来。
#[derive(Clone)]
struct Today {
    weekday: Weekday,
    date: chrono::NaiveDate,
}

impl Today {
    fn now() -> Self {
        let date = chrono::Local::now().date_naive();
        Self {
            weekday: weekday(date.weekday().num_days_from_monday() as u8),
            date,
        }
    }

    fn season(&self) -> String {
        shin::season_of(self.date)
    }
}

/// 0 到 6 是周一到周日。
fn weekday(day: u8) -> Weekday {
    match day {
        0 => Weekday::Monday,
        1 => Weekday::Tuesday,
        2 => Weekday::Wednesday,
        3 => Weekday::Thursday,
        4 => Weekday::Friday,
        5 => Weekday::Saturday,
        _ => Weekday::Sunday,
    }
}

pub struct NkuplayEngine {
    state: State,
    /// 取弹幕的服务；这个构建没有弹弹play 凭证时为 `None`。
    danmaku: Option<Arc<Loader>>,
    /// 当前视频的弹幕，按出现时间排序；没有时为空。
    comments: Arc<[Comment]>,
    /// 每打开或关闭一次视频加一，用来丢弃为上一个视频取回的弹幕。
    generation: u64,
    /// 新番列表的来源。
    shin_source: Arc<shin::Source>,
    /// 当前的新番列表（缓存的或刚取到的）。
    season: Option<Season>,
    /// 新番列表里的作品编号；这些作品的弹幕缓存保留得短一些。
    airing: Arc<HashSet<i64>>,
    /// 继续观看和媒体库，以及存放它们的地方。
    library: Library,
    store: library::Store,
    /// 正在数视频的文件夹。
    scanning: HashSet<String>,
    today: Today,
    /// 后台做完的事从这里回来。
    results: (Sender<Arrival>, Receiver<Arrival>),
    /// 把一件事放到引擎线程之外去做。
    run: fn(Job),
}

/// 没有打开任何视频时 `source` 里放的值；界面不会显示它。
fn no_source() -> MediaSource {
    MediaSource::file("")
}

/// 一个地址当作海报；没有时给一个界面不会显示的空地址。
fn poster(url: Option<&str>) -> (MediaSource, bool) {
    match url {
        Some(url) if !url.is_empty() => (MediaSource::new(url), true),
        _ => (no_source(), false),
    }
}

fn on_a_thread(job: Job) {
    std::thread::spawn(job);
}

impl Default for NkuplayEngine {
    fn default() -> Self {
        // 有 libmpv 时视频由它播放，没有时由平台自带的播放器接手。测试两者都不需要。
        #[cfg(not(test))]
        nkuplay_mpv::install();
        Self::new(
            Loader::built_in().map(Arc::new),
            shin::Source::built_in(),
            library::Store::built_in(),
            Today::now(),
            on_a_thread,
        )
    }
}

impl NkuplayEngine {
    /// `danmaku` 是取弹幕的服务（没有则弹幕功能不可用），`shin_source` 给出新番列表，
    /// `store` 存放继续观看和媒体库，`run` 决定后台的事在哪里做。
    fn new(
        danmaku: Option<Arc<Loader>>,
        shin_source: shin::Source,
        store: library::Store,
        today: Today,
        run: fn(Job),
    ) -> Self {
        let library = store.load();
        let mut engine = Self {
            state: State {
                has_video: false,
                source: no_source(),
                danmaku: DanmakuStatus::Off,
                danmaku_matched: false,
                danmaku_anime: String::new(),
                danmaku_episode: String::new(),
                danmaku_count: 0,
                danmaku_error: String::new(),
                recents: Vec::new(),
                recent_count: 0,
                shin: ShinStatus::NotLoaded,
                shin_outdated: false,
                shin_day: today.weekday.clone(),
                shin_shown: Vec::new(),
                shin_count: 0,
                shin_error: String::new(),
                folders: Vec::new(),
                folder_count: 0,
            },
            danmaku,
            comments: Arc::from([]),
            generation: 0,
            shin_source: Arc::new(shin_source),
            season: None,
            airing: Arc::default(),
            library,
            store,
            scanning: HashSet::new(),
            today,
            results: channel(),
            run,
        };
        engine.publish_library();
        engine
    }

    /// 当前视频的弹幕，按出现时间排序。
    pub fn comments(&self) -> &[Comment] {
        &self.comments
    }

    /// 在后台做 `job`，做完后把结果送回引擎线程，在 `wake` 里收下。
    fn in_background(&self, job: impl FnOnce() -> Arrival + Send + 'static) {
        let results = self.results.0.clone();
        // 命令运行期间才拿得到唤醒器；测试里没有宿主，也就没有它。
        let waker = bern::wake::waker();
        (self.run)(Box::new(move || {
            if results.send(job()).is_ok()
                && let Some(waker) = waker
            {
                waker.wake();
            }
        }));
    }

    /// 忘掉上一个视频的弹幕，并让还在路上的结果作废。
    fn reset_danmaku(&mut self, status: DanmakuStatus) {
        self.generation += 1;
        self.comments = Arc::from([]);
        self.state.danmaku = status;
        self.state.danmaku_matched = false;
        self.state.danmaku_anime.clear();
        self.state.danmaku_episode.clear();
        self.state.danmaku_count = 0;
        self.state.danmaku_error.clear();
    }

    /// 在后台为 `source` 取弹幕。
    fn start_danmaku(&mut self, source: MediaSource) {
        let Some(loader) = self.danmaku.clone() else {
            self.reset_danmaku(DanmakuStatus::Unavailable);
            return;
        };
        self.reset_danmaku(DanmakuStatus::Matching);
        let generation = self.generation;
        let airing = Arc::clone(&self.airing);
        self.in_background(move || Arrival::Danmaku(generation, loader.load(&source, &airing)));
    }

    fn take_in(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Loaded(loaded) => {
                let url = self.state.source.url.clone();
                self.library.identified(
                    &url,
                    &loaded.anime_title,
                    &loaded.episode_title,
                    loaded.poster.as_deref(),
                );
                self.save_library();
                self.state.danmaku = DanmakuStatus::Loaded;
                self.state.danmaku_matched = true;
                self.state.danmaku_anime = loaded.anime_title;
                self.state.danmaku_episode = loaded.episode_title;
                self.state.danmaku_count = loaded.comments.len() as i64;
                self.comments = loaded.comments;
            }
            Outcome::NoMatch => self.state.danmaku = DanmakuStatus::NoMatch,
            Outcome::Failed(reason) => {
                self.state.danmaku = DanmakuStatus::Failed;
                self.state.danmaku_error = reason;
            }
        }
    }

    /// 把继续观看和媒体库放进界面状态。
    fn publish_library(&mut self) {
        self.state.recents = self
            .library
            .recents
            .iter()
            .enumerate()
            .map(|(index, recent)| {
                let (poster, has_poster) = poster(recent.poster.as_deref());
                Recent {
                    id: index as i64 + 1,
                    title: recent.title.clone(),
                    episode: recent.episode.clone(),
                    source: MediaSource::new(recent.url.clone()),
                    poster,
                    has_poster,
                }
            })
            .collect();
        self.state.folders = self
            .library
            .folders
            .iter()
            .enumerate()
            .map(|(index, folder)| Folder {
                id: index as i64 + 1,
                name: folder.name.clone(),
                videos: folder.videos,
                scanning: self.scanning.contains(&folder.url),
            })
            .collect();
        self.state.recent_count = self.state.recents.len() as i64;
        self.state.folder_count = self.state.folders.len() as i64;
    }

    fn save_library(&mut self) {
        self.store.save(&self.library);
        self.publish_library();
    }

    /// 把新番列表里 `shin_day` 那天更新的番放进界面状态。
    fn publish_shin(&mut self) {
        let day = self.state.shin_day.clone();
        self.state.shin_shown = self
            .season
            .iter()
            .flat_map(|season| season.anime.iter())
            .filter(|anime| weekday(anime.day) == day)
            .map(|anime| {
                let (poster, has_poster) = poster(anime.poster.as_deref());
                ShinAnime {
                    id: anime.id,
                    title: anime.title.clone(),
                    poster,
                    has_poster,
                }
            })
            .collect();
        self.state.shin_count = self.state.shin_shown.len() as i64;
    }

    fn take_season(&mut self, season: Season) {
        self.state.shin = ShinStatus::Loaded;
        self.state.shin_outdated = season.id < self.today.season();
        self.state.shin_error.clear();
        self.airing = Arc::new(season.anime.iter().map(|anime| anime.id).collect());
        self.season = Some(season);
        self.publish_shin();
    }

    /// 在后台向弹弹play 取新番列表。
    fn download_shin(&mut self) {
        if !self.shin_source.available() {
            if self.season.is_none() {
                self.state.shin = ShinStatus::Unavailable;
            }
            return;
        }
        self.state.shin = ShinStatus::Loading;
        self.state.shin_error.clear();
        let source = Arc::clone(&self.shin_source);
        let current = self.today.season();
        self.in_background(move || Arrival::Shin(source.download(&current)));
    }

    /// 在后台数文件夹 `url` 里的视频。
    fn scan_folder(&mut self, url: String) {
        self.scanning.insert(url.clone());
        self.in_background(move || {
            let counted = library::count_videos(&url);
            Arrival::Folder(url, counted)
        });
    }
}

impl nkuplay::Commands for NkuplayEngine {
    fn open_file(&mut self, file: MediaSource) -> Result<(), CommandError> {
        self.state.source = file.clone();
        self.state.has_video = true;
        let name = file.file_name().unwrap_or_default();
        self.library.opened(&file.url, &name);
        self.save_library();
        self.start_danmaku(file);
        Ok(())
    }

    fn close(&mut self) -> Result<(), CommandError> {
        self.state.source = no_source();
        self.state.has_video = false;
        self.reset_danmaku(DanmakuStatus::Off);
        Ok(())
    }

    fn load_shin(&mut self) -> Result<(), CommandError> {
        if self.state.shin != ShinStatus::NotLoaded {
            return Ok(());
        }
        match self.shin_source.cached() {
            Some(season) => self.take_season(season),
            None => self.download_shin(),
        }
        Ok(())
    }

    fn refresh_shin(&mut self) -> Result<(), CommandError> {
        if self.state.shin != ShinStatus::Loading {
            self.download_shin();
        }
        Ok(())
    }

    fn show_day(&mut self, day: Weekday) -> Result<(), CommandError> {
        self.state.shin_day = day;
        self.publish_shin();
        Ok(())
    }

    fn add_folder(&mut self, folder: MediaSource) -> Result<(), CommandError> {
        let name = folder.file_name().unwrap_or_else(|| folder.url.clone());
        if self.library.add_folder(&folder.url, &name) {
            self.scan_folder(folder.url);
            self.save_library();
        }
        Ok(())
    }

    fn remove_folder(&mut self, id: i64) -> Result<(), CommandError> {
        let index = usize::try_from(id - 1).ok();
        if let Some(index) = index.filter(|index| *index < self.library.folders.len()) {
            let removed = self.library.folders.remove(index);
            self.scanning.remove(&removed.url);
            self.save_library();
        }
        Ok(())
    }
}

impl nkuplay::Engine for NkuplayEngine {
    fn state(&self) -> &State {
        &self.state
    }

    /// 收下后台做完的事。为已经换掉或关掉的视频取回的弹幕直接丢弃。
    fn wake(&mut self) {
        while let Ok(arrival) = self.results.1.try_recv() {
            match arrival {
                Arrival::Danmaku(generation, outcome) => {
                    if generation == self.generation {
                        self.take_in(outcome);
                    }
                }
                Arrival::Shin(Ok(season)) => self.take_season(season),
                Arrival::Shin(Err(reason)) => {
                    // 取失败时，之前的列表（如果有）照旧显示。
                    self.state.shin = if self.season.is_some() {
                        ShinStatus::Loaded
                    } else {
                        ShinStatus::Failed
                    };
                    self.state.shin_error = reason;
                }
                Arrival::Folder(url, counted) => {
                    self.scanning.remove(&url);
                    if let (Ok(videos), Some(folder)) = (
                        counted,
                        self.library.folders.iter_mut().find(|f| f.url == url),
                    ) {
                        folder.videos = videos;
                    }
                    self.save_library();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::danmaku::tests::FakeApi;
    use nkuplay::{Commands, Engine};
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::Ordering;

    /// 测试里“后台”的事当场做完，这样结果在命令返回时就已经在等着了。
    fn at_once(job: Job) {
        job();
    }

    /// 测试里“后台”的事先攒着不做，由测试决定什么时候做。
    static HELD: Mutex<Vec<Job>> = Mutex::new(Vec::new());

    fn held(job: Job) {
        HELD.lock().unwrap().push(job);
    }

    /// 测试里的“今天”：2026 年 11 月 16 日，星期一，属于 202610 这一季。
    fn today() -> Today {
        Today {
            weekday: Weekday::Monday,
            date: chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap(),
        }
    }

    /// 不存文件、没有新番来源的引擎。
    fn engine(danmaku: Option<Arc<Loader>>, run: fn(Job)) -> NkuplayEngine {
        NkuplayEngine::new(
            danmaku,
            shin::Source::new(None, None),
            library::Store::new(None),
            today(),
            run,
        )
    }

    fn engine_with(api: FakeApi, run: fn(Job)) -> NkuplayEngine {
        engine(Some(Arc::new(Loader::new(api))), run)
    }

    /// 新番来自假弹弹play、缓存放在 `cache` 的引擎。
    fn engine_with_shin(api: shin::tests::FakeApi, cache: Option<PathBuf>) -> NkuplayEngine {
        NkuplayEngine::new(
            None,
            shin::Source::new(Some(Box::new(api)), cache),
            library::Store::new(None),
            today(),
            at_once,
        )
    }

    /// 系统临时文件夹里一个空的子文件夹，销毁时连同内容删除。
    struct Scratch(PathBuf);

    impl Scratch {
        fn named(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("nkuplay-core-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 系统临时文件夹里的一个小视频文件，销毁时删除。
    struct Video(PathBuf);

    impl Video {
        fn named(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("nkuplay-core-{}-{name}", std::process::id()));
            std::fs::File::create(&path)
                .unwrap()
                .write_all(name.as_bytes())
                .unwrap();
            Video(path)
        }

        fn source(&self) -> MediaSource {
            MediaSource::file(&self.0)
        }
    }

    impl Drop for Video {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn opening_shows_the_video_and_closing_returns_to_the_home_page() {
        let mut engine = engine(None, at_once);
        assert!(!engine.state.has_video);
        assert_eq!(engine.state.danmaku, DanmakuStatus::Off);

        let file = MediaSource::file("C:/videos/sample.mkv");
        engine.open_file(file.clone()).unwrap();
        assert!(engine.state.has_video);
        assert_eq!(engine.state.source, file);

        engine.close().unwrap();
        assert!(!engine.state.has_video);
        assert_eq!(engine.state.danmaku, DanmakuStatus::Off);
    }

    #[test]
    fn without_credentials_danmaku_is_unavailable_and_the_video_still_opens() {
        let mut engine = engine(None, at_once);
        engine.open_file(MediaSource::file("a.mkv")).unwrap();
        assert!(engine.state.has_video);
        assert_eq!(engine.state.danmaku, DanmakuStatus::Unavailable);
        engine.wake();
        assert_eq!(engine.state.danmaku, DanmakuStatus::Unavailable);
    }

    #[test]
    fn opening_a_recognised_video_loads_its_danmaku_after_the_wake() {
        let video = Video::named("recognised.mkv");
        let mut engine = engine_with(FakeApi::recognising(), at_once);
        engine.open_file(video.source()).unwrap();
        // 命令返回时还只是“正在识别”：结果要等 `wake` 才收下。
        assert_eq!(engine.state.danmaku, DanmakuStatus::Matching);
        assert!(engine.comments().is_empty());

        engine.wake();
        assert_eq!(engine.state.danmaku, DanmakuStatus::Loaded);
        assert!(engine.state.danmaku_matched);
        assert_eq!(engine.state.danmaku_anime, "某作品");
        assert_eq!(engine.state.danmaku_episode, "第1话");
        assert_eq!(engine.state.danmaku_count, 2);
        assert_eq!(engine.comments().len(), 2);
        assert_eq!(engine.comments()[0].text, "第一条");
    }

    #[test]
    fn an_unrecognised_video_and_a_failure_are_told_apart() {
        let video = Video::named("unknown.mkv");
        let mut engine = engine_with(
            FakeApi {
                matched: false,
                ..FakeApi::recognising()
            },
            at_once,
        );
        engine.open_file(video.source()).unwrap();
        engine.wake();
        assert_eq!(engine.state.danmaku, DanmakuStatus::NoMatch);
        assert_eq!(engine.state.danmaku_error, "");

        let mut engine = engine_with(
            FakeApi {
                match_error: Some("timed out"),
                ..FakeApi::recognising()
            },
            at_once,
        );
        engine.open_file(video.source()).unwrap();
        engine.wake();
        assert_eq!(engine.state.danmaku, DanmakuStatus::Failed);
        assert!(engine.state.danmaku_error.contains("timed out"));
        assert!(!engine.state.danmaku_matched);
    }

    #[test]
    fn closing_forgets_the_danmaku() {
        let video = Video::named("closing.mkv");
        let mut engine = engine_with(FakeApi::recognising(), at_once);
        engine.open_file(video.source()).unwrap();
        engine.wake();
        assert_eq!(engine.comments().len(), 2);

        engine.close().unwrap();
        assert_eq!(engine.state.danmaku, DanmakuStatus::Off);
        assert!(!engine.state.danmaku_matched);
        assert_eq!(engine.state.danmaku_anime, "");
        assert_eq!(engine.state.danmaku_count, 0);
        assert!(engine.comments().is_empty());
    }

    #[test]
    fn danmaku_that_arrives_for_a_video_already_closed_is_dropped() {
        let video = Video::named("late.mkv");
        let mut engine = engine_with(FakeApi::recognising(), held);
        engine.open_file(video.source()).unwrap();
        assert_eq!(engine.state.danmaku, DanmakuStatus::Matching);
        // 结果还没回来，用户就把视频关了。
        engine.close().unwrap();

        // 现在后台的事才做完。
        let jobs = std::mem::take(&mut *HELD.lock().unwrap());
        assert_eq!(jobs.len(), 1);
        for job in jobs {
            job();
        }
        engine.wake();
        assert_eq!(engine.state.danmaku, DanmakuStatus::Off);
        assert!(engine.comments().is_empty());
    }

    #[test]
    fn an_opened_video_is_kept_for_continue_watching_and_named_once_identified() {
        let video = Video::named("[Group] Show - 01.mkv");
        let mut engine = engine_with(FakeApi::recognising(), at_once);
        engine.open_file(video.source()).unwrap();
        assert_eq!(engine.state.recents.len(), 1);
        assert!(engine.state.recents[0].title.ends_with("[Group] Show - 01"));
        assert_eq!(engine.state.recents[0].source, video.source());

        engine.wake();
        let recent = &engine.state.recents[0];
        assert_eq!(recent.title, "某作品");
        assert_eq!(recent.episode, "第1话");
    }

    #[test]
    fn continue_watching_and_the_folders_survive_a_restart() {
        let data = Scratch::named("data");
        let anime = Scratch::named("anime");
        std::fs::write(anime.0.join("01.mkv"), b"x").unwrap();
        let start = || {
            NkuplayEngine::new(
                None,
                shin::Source::new(None, None),
                library::Store::new(Some(data.0.clone())),
                today(),
                at_once,
            )
        };
        let mut engine = start();
        engine
            .open_file(MediaSource::file("C:/videos/a.mkv"))
            .unwrap();
        engine.add_folder(MediaSource::file(&anime.0)).unwrap();
        engine.wake();
        assert_eq!(engine.state.folders[0].videos, 1);
        assert!(!engine.state.folders[0].scanning);

        let engine = start();
        assert_eq!(engine.state.recents.len(), 1);
        assert_eq!(engine.state.folders.len(), 1);
        assert_eq!(engine.state.folders[0].videos, 1);
    }

    #[test]
    fn a_folder_is_removed_without_touching_its_files() {
        let anime = Scratch::named("removed");
        std::fs::write(anime.0.join("01.mkv"), b"x").unwrap();
        let mut engine = engine(None, at_once);
        engine.add_folder(MediaSource::file(&anime.0)).unwrap();
        engine.add_folder(MediaSource::file(&anime.0)).unwrap();
        assert_eq!(engine.state.folders.len(), 1);
        engine.remove_folder(1).unwrap();
        assert!(engine.state.folders.is_empty());
        assert!(anime.0.join("01.mkv").exists());
        // 不存在的编号什么也不做。
        engine.remove_folder(7).unwrap();
    }

    #[test]
    fn the_shin_list_is_asked_once_then_read_from_the_cache() {
        let cache = Scratch::named("shin-cache");
        let api = shin::tests::FakeApi::season();
        let calls = Arc::clone(&api.calls);
        let mut engine = engine_with_shin(api, Some(cache.0.clone()));
        assert_eq!(engine.state.shin, ShinStatus::NotLoaded);

        // 第一次打开首页：没有缓存，取一次。
        engine.load_shin().unwrap();
        assert_eq!(engine.state.shin, ShinStatus::Loading);
        engine.wake();
        assert_eq!(engine.state.shin, ShinStatus::Loaded);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // 再次打开首页不再取。
        engine.load_shin().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // 下次启动：读缓存，不取。
        let api = shin::tests::FakeApi::season();
        let later_calls = Arc::clone(&api.calls);
        let mut engine = engine_with_shin(api, Some(cache.0.clone()));
        engine.load_shin().unwrap();
        assert_eq!(engine.state.shin, ShinStatus::Loaded);
        assert_eq!(later_calls.load(Ordering::SeqCst), 0);

        // 点刷新才重新取。
        engine.refresh_shin().unwrap();
        engine.wake();
        assert_eq!(later_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_shin_list_shows_today_first_and_follows_the_chosen_day() {
        let mut engine = engine_with_shin(shin::tests::FakeApi::season(), None);
        engine.load_shin().unwrap();
        engine.wake();
        assert_eq!(engine.state.shin_day, Weekday::Monday);
        let titles = |engine: &NkuplayEngine| {
            engine
                .state
                .shin_shown
                .iter()
                .map(|anime| anime.title.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(titles(&engine), ["周一的番"]);
        assert!(engine.state.shin_shown[0].has_poster);

        engine.show_day(Weekday::Sunday).unwrap();
        assert_eq!(titles(&engine), ["周日的番"]);
        engine.show_day(Weekday::Tuesday).unwrap();
        assert!(engine.state.shin_shown.is_empty());
    }

    #[test]
    fn a_cached_list_from_an_earlier_season_asks_for_a_refresh() {
        let cache = Scratch::named("shin-old");
        let source = shin::Source::new(
            Some(Box::new(shin::tests::FakeApi::season())),
            Some(cache.0.clone()),
        );
        source.download("202607").unwrap();
        let mut engine = engine_with_shin(shin::tests::FakeApi::season(), Some(cache.0.clone()));
        engine.load_shin().unwrap();
        assert_eq!(engine.state.shin, ShinStatus::Loaded);
        assert!(engine.state.shin_outdated);

        engine.refresh_shin().unwrap();
        engine.wake();
        assert!(!engine.state.shin_outdated);
    }

    #[test]
    fn a_failed_refresh_keeps_the_list_already_shown() {
        let mut engine = engine_with_shin(shin::tests::FakeApi::season(), None);
        engine.load_shin().unwrap();
        engine.wake();
        engine.shin_source = Arc::new(shin::Source::new(
            Some(Box::new(shin::tests::FakeApi {
                error: Some("refused"),
                ..Default::default()
            })),
            None,
        ));
        engine.refresh_shin().unwrap();
        engine.wake();
        assert_eq!(engine.state.shin, ShinStatus::Loaded);
        assert!(engine.state.shin_error.contains("refused"));
        assert_eq!(engine.state.shin_shown.len(), 1);
    }

    #[test]
    fn without_credentials_or_a_cache_the_shin_list_is_unavailable() {
        let mut engine = engine(None, at_once);
        engine.load_shin().unwrap();
        assert_eq!(engine.state.shin, ShinStatus::Unavailable);
    }
}
