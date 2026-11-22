//! Nkuplay 的引擎：业务逻辑，纯 Rust。它不知道窗口的存在，所以不需要窗口就能测试。

mod danmaku;
mod details;
mod library;
mod naming;
mod shin;
mod tmdb;

use bern::CommandError;
use bern::media::MediaSource;
use chrono::Datelike;
use danmaku::{Loader, Outcome};
use library::Library;
use nkuplay_dandanplay::{Comment, CommentMode};
use nkuplay_mpv::{DanmakuComment, DanmakuMode};
use nkuplay_protocol::nkuplay::{
    self, Backdrop, DanmakuStatus, DetailComment, DetailEpisode, DetailFact, DetailScore,
    DetailTag, Folder, Recent, Series, ShinAnime, ShinStatus, State, Weekday,
};
use shin::Season;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 交给别的线程去做的一件事。
type Job = Box<dyn FnOnce() + Send>;

/// 后台做完的事。
enum Arrival {
    /// 一份弹幕结果，带着发起时的 `generation`。
    Danmaku(u64, Outcome),
    /// 取新番列表的结果。
    Shin(Result<Season, String>),
    /// 扫描一个文件夹归出的番剧（或扫描失败的原因）。
    Grouped(String, Result<Vec<library::SeriesEntry>, String>),
    /// 文件夹里归类键为第二项的那部番，弹弹play 认出了是哪部作品。
    Identified(String, String, library::Identity),
    /// 一个文件夹扫描、识别完了。
    Scanned(String),
    /// 作品（第一项）的详情和短评。
    Details(i64, Result<details::Info, String>),
    /// 一集的剧照（第一项是 [`stills_key`]）。
    Stills(String, Vec<PathBuf>),
    /// 视频（第一项）离开时的画面存好了。
    Shot(String, PathBuf),
}

/// 打开着的详情页。
struct Detail {
    /// 海报墙上的合并键（[`library::ShelfItem::key`]），用来找本地文件；弹弹play 的作品是
    /// `anime:<编号>`，本地没有这部番也一样。
    key: String,
    /// 弹弹play 的作品编号；没认出的番为空，只显示本地信息。
    anime_id: Option<i64>,
    /// 资料到之前显示的作品名和海报。
    title: String,
    poster: Option<String>,
    info: Option<details::Info>,
    loading: bool,
    error: String,
    descending: bool,
    /// 从哪张海报点进来的，见 `State::detail_hero`。
    hero: String,
    /// 评论展开着。
    comments_expanded: bool,
    /// 背景的剧照，正在显示第 `still_at` 张。
    stills: Vec<PathBuf>,
    still_at: usize,
    /// 剧照是为哪一集取的（[`stills_key`]）。
    stills_for: Option<String>,
}

/// 一集剧照的标识：TMDB 剧集编号、季和集。
fn stills_key(tv: i64, season: u32, episode: u32) -> String {
    format!("{tv}-{season}-{episode}")
}

/// 收起时显示几条评论（2×2）。
const FEW_COMMENTS: usize = 4;

/// 视频地址的一个短哈希（FNV-1a），给它的画面文件起名。
fn url_hash(url: &str) -> u64 {
    url.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// 详情页背景最多几张剧照。
const MAX_STILLS: usize = 6;
/// 刚打开视频后多久以内不记进度：播放器报的位置可能还是上一个视频的。
const RECORD_AFTER: Duration = Duration::from_secs(3);
/// 看过不到这么多（毫秒）的不续播，从头开始。
const RESUME_FROM: i64 = 10_000;
/// 续播时往回倒一点（毫秒），接上前面的情节。
const RESUME_REWIND: i64 = 2_000;

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
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
    /// 继续观看和媒体库，以及存放它们的地方。
    library: Library,
    store: library::Store,
    /// 正在数视频的文件夹。
    scanning: HashSet<String>,
    /// 海报墙，和 `state.series` 一一对应。
    shelf: Vec<library::ShelfItem>,
    /// 番剧详情和短评的来源，剧照的来源。
    details_source: Arc<details::Source>,
    stills: Arc<tmdb::Stills>,
    /// 打开着的详情页。
    detail: Option<Detail>,
    /// 背景图上次交给界面的地址和编号；地址变了编号才变，界面借此淡入淡出。
    backdrop: (String, i64),
    /// 正在看（或刚看过）的视频的地址，以及打开它的时刻；记进度用。回到首页后还留着，
    /// 离开播放页时还要记最后一次。
    playing: Option<(String, Instant)>,
    /// 继续观看卡片上的画面存在哪里；没有时不存。
    shots: Option<PathBuf>,
    today: Today,
    /// 后台做完的事从这里回来。
    results: (Sender<Arrival>, Receiver<Arrival>),
    /// 把一件事放到引擎线程之外去做。
    run: fn(Job),
    /// 最近一次要跳到的位置（毫秒）和请求的时刻。播放器跳过去、报回新位置要一点时间，
    /// 这段时间里连按方向键要在这个位置上接着加减，而不是在还没更新的位置上。
    last_seek: Option<(i64, Instant)>,
}

/// 上一次跳转之后多久以内，连按方向键接着上一次的目标算。
const SKIP_CHAIN: Duration = Duration::from_millis(1500);

/// 总时长 `duration_ms` 的 `fraction` 处是多少毫秒；比例不是数或总时长未知时为空。
fn point_of(fraction: f64, duration_ms: Option<i64>) -> Option<i64> {
    let duration = duration_ms.filter(|duration| *duration > 0)?;
    if !fraction.is_finite() {
        return None;
    }
    // 向下取整，结果不会超过总时长：播放器拒绝超出范围的跳转。
    Some((fraction.clamp(0.0, 1.0) * duration as f64).floor() as i64)
}

/// 弹弹play 的弹幕换成内核画的弹幕。
fn for_kernel(comments: &[Comment]) -> Vec<DanmakuComment> {
    comments
        .iter()
        .map(|comment| DanmakuComment {
            time: comment.time,
            mode: match comment.mode {
                CommentMode::Scroll => DanmakuMode::Scroll,
                CommentMode::Top => DanmakuMode::Top,
                CommentMode::Bottom => DanmakuMode::Bottom,
            },
            color: comment.color,
            text: comment.text.clone(),
        })
        .collect()
}

/// 音量限制在 0 到 1；不是数时为空。
fn volume_of(volume: f64) -> Option<f64> {
    volume.is_finite().then(|| volume.clamp(0.0, 1.0))
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
            Sources {
                shin: shin::Source::built_in(),
                details: details::Source::built_in(),
                stills: tmdb::Stills::built_in(),
                shots: bern::storage::cache_dir().map(|dir| dir.join("shots")),
            },
            library::Store::built_in(),
            Today::now(),
            on_a_thread,
        )
    }
}

/// 弹弹play 和 TMDB 的几样资料从哪里来。
struct Sources {
    shin: shin::Source,
    details: details::Source,
    stills: tmdb::Stills,
    /// 继续观看卡片上的画面存在哪个文件夹。
    shots: Option<PathBuf>,
}

impl NkuplayEngine {
    /// `danmaku` 是取弹幕的服务（没有则弹幕功能不可用），`sources` 给出新番列表、番剧详情和剧照，
    /// `store` 存放继续观看、媒体库和观看进度，`run` 决定后台的事在哪里做。
    fn new(
        danmaku: Option<Arc<Loader>>,
        sources: Sources,
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
                danmaku_visible: true,
                opened: 0,
                seek_serial: 0,
                seek_ms: 0,
                volume: 1.0,
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
                series: Vec::new(),
                series_count: 0,
                library_scanning: false,
                detail_open: false,
                detail_title: String::new(),
                detail_other_titles: String::new(),
                detail_poster: no_source(),
                detail_has_poster: false,
                detail_hero: String::new(),
                detail_backdrop: Vec::new(),
                detail_rotates: false,
                detail_known: false,
                detail_loading: false,
                detail_error: String::new(),
                detail_rating: 0.0,
                detail_rating_text: String::new(),
                detail_kind: String::new(),
                detail_year: String::new(),
                detail_month: 0,
                detail_has_premiere: false,
                detail_on_air: false,
                detail_tags: Vec::new(),
                detail_summary: String::new(),
                detail_episodes: Vec::new(),
                detail_main_count: 0,
                detail_special_count: 0,
                detail_local_count: 0,
                detail_has_last: false,
                detail_last_code: String::new(),
                detail_last_number: 0,
                detail_descending: false,
                detail_can_resume: false,
                detail_resume: no_source(),
                detail_started: false,
                detail_comments: Vec::new(),
                detail_comments_more: false,
                detail_comments_expanded: false,
                detail_comment_count: 0,
                detail_comment_source: String::new(),
                detail_scores: Vec::new(),
                detail_facts: Vec::new(),
            },
            danmaku,
            comments: Arc::from([]),
            generation: 0,
            shin_source: Arc::new(sources.shin),
            season: None,
            library,
            store,
            scanning: HashSet::new(),
            shelf: Vec::new(),
            details_source: Arc::new(sources.details),
            stills: Arc::new(sources.stills),
            detail: None,
            backdrop: (String::new(), 0),
            playing: None,
            shots: sources.shots,
            today,
            results: channel(),
            run,
            last_seek: None,
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

    /// 在别的线程上做一件会分几次交回结果的事（`send` 每交一次就唤醒引擎一次）。
    fn in_background_many(&self, job: impl FnOnce(&dyn Fn(Arrival)) + Send + 'static) {
        let results = self.results.0.clone();
        let waker = bern::wake::waker();
        (self.run)(Box::new(move || {
            let send = |arrival: Arrival| {
                if results.send(arrival).is_ok()
                    && let Some(waker) = &waker
                {
                    waker.wake();
                }
            };
            job(&send);
        }));
    }

    /// 忘掉上一个视频的弹幕，并让还在路上的结果作废。
    fn reset_danmaku(&mut self, status: DanmakuStatus) {
        self.generation += 1;
        self.comments = Arc::from([]);
        nkuplay_mpv::clear_danmaku();
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
        self.in_background(move || Arrival::Danmaku(generation, loader.load(&source)));
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
                nkuplay_mpv::set_danmaku(for_kernel(&loaded.comments), loaded.shift);
                self.comments = loaded.comments;
            }
            Outcome::NoMatch => self.state.danmaku = DanmakuStatus::NoMatch,
            Outcome::Failed(reason) => {
                self.state.danmaku = DanmakuStatus::Failed;
                self.state.danmaku_error = reason;
            }
        }
    }

    /// 请界面让播放器跳到 `point` 毫秒处。
    fn seek_to(&mut self, point: i64, now: Instant) {
        self.state.seek_ms = point;
        self.state.seek_serial += 1;
        self.last_seek = Some((point, now));
    }

    /// `skip` 的实现，`now` 是按键的时刻。
    fn skip_at(
        &mut self,
        seconds: f64,
        position_ms: i64,
        duration_ms: Option<i64>,
        now: Instant,
    ) -> Result<(), CommandError> {
        if !seconds.is_finite() {
            return Err(CommandError::new(
                "invalid_seconds",
                "the distance must be a number",
            ));
        }
        let Some(duration) = duration_ms.filter(|duration| *duration > 0) else {
            return Ok(());
        };
        let from = match self.last_seek {
            Some((target, at)) if now.duration_since(at) < SKIP_CHAIN => target,
            _ => position_ms,
        };
        let to = (from as f64 + seconds * 1000.0).round() as i64;
        self.seek_to(to.clamp(0, duration), now);
        Ok(())
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
                let shot = recent.shot.as_ref().filter(|shot| shot.is_file());
                Recent {
                    id: index as i64 + 1,
                    title: recent.title.clone(),
                    episode: recent.episode.clone(),
                    source: MediaSource::new(recent.url.clone()),
                    poster,
                    has_poster,
                    shot: shot.map_or_else(no_source, MediaSource::file),
                    has_shot: shot.is_some(),
                    percent: self
                        .library
                        .progress
                        .get(&recent.url)
                        .map_or(0, |watched| watched.percent()),
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

        self.shelf = self.library.shelf();
        self.state.series = self
            .shelf
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let (poster, has_poster) = poster(item.poster.as_deref());
                Series {
                    id: index as i64 + 1,
                    title: item.title.clone(),
                    poster,
                    has_poster,
                    episodes: item.episodes.len() as i64,
                    hero: format!("shelf-{}", item.key),
                }
            })
            .collect();
        self.state.series_count = self.state.series.len() as i64;
        self.state.library_scanning = !self.scanning.is_empty();
        self.publish_detail();
    }

    /// 打开详情页：先显示缓存的资料（有的话），缓存没有或过期了就在后台向弹弹play 取。
    fn show_detail(
        &mut self,
        key: String,
        anime_id: Option<i64>,
        (title, poster): (String, Option<String>),
        hero: String,
    ) {
        let info = anime_id.and_then(|id| self.details_source.cached(id));
        let stale = info.as_ref().is_none_or(|info| !info.fresh(unix_now()));
        self.detail = Some(Detail {
            key,
            anime_id,
            title,
            poster,
            info,
            loading: false,
            error: String::new(),
            descending: false,
            hero,
            comments_expanded: false,
            stills: Vec::new(),
            still_at: 0,
            stills_for: None,
        });
        if stale {
            self.download_details();
        }
        self.want_stills(true);
        self.publish_detail();
    }

    /// 在后台向弹弹play 取详情页这部番的资料；有 TMDB 密钥时顺便取“继续”那一集的剧照。
    fn download_details(&mut self) {
        if !self.details_source.available() {
            return;
        }
        let episode = self.still_episode();
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        let Some(anime_id) = detail.anime_id else {
            return;
        };
        if detail.loading {
            return;
        }
        detail.loading = true;
        detail.error.clear();
        let source = Arc::clone(&self.details_source);
        let stills = Arc::clone(&self.stills);
        self.in_background_many(move |send| {
            let info = source.download(anime_id, unix_now());
            let target = info
                .as_ref()
                .ok()
                .and_then(|info| info.details.as_ref())
                .and_then(details::tmdb);
            send(Arrival::Details(anime_id, info));
            if let Some((tv, season)) = target.filter(|_| stills.available())
                && let Ok(found) = stills.fetch(tv, season, episode, MAX_STILLS)
            {
                send(Arrival::Stills(stills_key(tv, season, episode), found));
            }
        });
    }

    /// 背景用哪一集的剧照：“继续”指向的那一集；它不是正片时用第 1 集。
    fn still_episode(&self) -> u32 {
        self.detail_rows()
            .and_then(|(rows, summary)| rows.get(summary.next?)?.number)
            .unwrap_or(1)
    }

    /// 详情页这部番的剧集列表和统计；没打开详情页时为空。
    fn detail_rows(&self) -> Option<(Vec<details::Row>, details::Summary)> {
        let detail = self.detail.as_ref()?;
        let local = self
            .shelf
            .iter()
            .find(|item| item.key == detail.key)
            .map_or(&[][..], |item| &item.episodes[..]);
        let info = detail.info.as_ref().and_then(|info| info.details.as_ref());
        let rows = details::rows(info, local, &self.library.progress);
        let summary = details::summarize(&rows, info.map_or(0, |info| info.episodes.len()));
        Some((rows, summary))
    }

    /// 背景该显示的那一集剧照的标识；没有 TMDB 密钥、没打开详情页或作品不在 TMDB 上时为空。
    fn wanted_stills(&self) -> Option<(String, i64, u32, u32)> {
        if !self.stills.available() {
            return None;
        }
        let detail = self.detail.as_ref()?;
        let found = detail.info.as_ref()?.details.as_ref()?;
        let (tv, season) = details::tmdb(found)?;
        let episode = self.still_episode();
        Some((stills_key(tv, season, episode), tv, season, episode))
    }

    /// 背景的剧照换成“继续”那一集的：缓存里有就直接用；`fetch` 为真且缓存里没有时在后台取。
    /// 后台取完要唤醒引擎，所以只在命令里取（[`bern::wake::waker`] 只在命令执行时拿得到）。
    fn want_stills(&mut self, fetch: bool) {
        let Some((key, tv, season, episode)) = self.wanted_stills() else {
            return;
        };
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        if detail.stills_for.as_deref() == Some(key.as_str()) {
            return;
        }
        let cached = self.stills.cached(tv, season, episode);
        if !cached.is_empty() {
            detail.stills_for = Some(key);
            detail.stills = cached;
            detail.still_at = 0;
        } else if fetch {
            detail.stills_for = Some(key.clone());
            let stills = Arc::clone(&self.stills);
            self.in_background(move || {
                let found = stills.fetch(tv, season, episode, MAX_STILLS);
                Arrival::Stills(key, found.unwrap_or_default())
            });
        }
    }

    /// 把详情页放进界面状态；没打开时清空。
    fn publish_detail(&mut self) {
        let Some((rows, summary)) = self.detail_rows() else {
            self.close_detail_state();
            return;
        };
        let Some(detail) = self.detail.as_ref() else {
            return;
        };
        let info = detail.info.as_ref();
        let found = info.and_then(|info| info.details.as_ref());
        let state = &mut self.state;
        state.detail_open = true;
        state.detail_title = found
            .map(|found| found.anime_title.clone())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| detail.title.clone());
        let poster_url = found
            .and_then(|found| found.image_url.clone())
            .filter(|url| !url.is_empty())
            .or_else(|| detail.poster.clone());
        (state.detail_poster, state.detail_has_poster) = poster(poster_url.as_deref());
        state.detail_hero = detail.hero.clone();
        state.detail_known = found.is_some();
        state.detail_loading = detail.loading;
        state.detail_error = detail.error.clone();
        state.detail_other_titles = found.map(details::other_titles).unwrap_or_default();
        state.detail_rating = found.map_or(0.0, |found| found.rating.clamp(0.0, 10.0));
        state.detail_rating_text = details::score(state.detail_rating);
        state.detail_kind = found
            .map(|found| found.type_description.clone())
            .unwrap_or_default();
        let premiere = found.and_then(details::premiere);
        state.detail_has_premiere = premiere.is_some();
        state.detail_year = premiere
            .map(|(year, _)| year.to_string())
            .unwrap_or_default();
        state.detail_month = premiere.map_or(0, |(_, month)| i64::from(month));
        state.detail_on_air = found.is_some_and(|found| found.is_on_air);
        state.detail_tags = found
            .map(|found| details::tags(found, 6))
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(at, name)| DetailTag {
                id: at as i64 + 1,
                name,
            })
            .collect();
        state.detail_summary = found.map(details::summary).unwrap_or_default();

        state.detail_main_count = summary.main as i64;
        state.detail_special_count = summary.special as i64;
        state.detail_local_count = summary.local as i64;
        let last = summary.last.map(|at| &rows[at]);
        state.detail_has_last = last.is_some();
        state.detail_last_code = last.map(|row| row.code.clone()).unwrap_or_default();
        state.detail_last_number = last.and_then(|row| row.number).map_or(0, i64::from);
        let resume = summary.next.and_then(|at| rows[at].url.clone());
        state.detail_can_resume = resume.is_some();
        state.detail_resume = resume.map_or_else(no_source, MediaSource::new);
        state.detail_started = summary.last.is_some();
        state.detail_descending = detail.descending;
        let mut episodes: Vec<DetailEpisode> = rows
            .iter()
            .enumerate()
            .map(|(at, row)| DetailEpisode {
                id: at as i64 + 1,
                code: row.code.clone(),
                number: row.number.map_or(0, i64::from),
                title: row.title.clone(),
                local: row.url.is_some(),
                source: row
                    .url
                    .as_ref()
                    .map_or_else(no_source, |url| MediaSource::new(url.clone())),
                percent: row.watched.map_or(0, |watched| watched.percent()),
                finished: row.watched.is_some_and(|watched| watched.finished()),
                current: summary.next == Some(at),
            })
            .collect();
        if detail.descending {
            episodes.reverse();
        }
        state.detail_episodes = episodes;

        let comments = info.map(|info| &info.comments);
        let written: Vec<_> = comments
            .map_or(&[][..], |comments| &comments.comments[..])
            .iter()
            .filter(|comment| !comment.text.trim().is_empty())
            .collect();
        let shown = if detail.comments_expanded {
            written.len()
        } else {
            FEW_COMMENTS
        };
        state.detail_comments_expanded = detail.comments_expanded;
        state.detail_comments_more = !detail.comments_expanded && written.len() > FEW_COMMENTS;
        state.detail_comments = written
            .into_iter()
            .take(shown)
            .enumerate()
            .map(|(at, comment)| DetailComment {
                id: at as i64 + 1,
                user: comment.user_name.clone(),
                score: comment.rating.clamp(0, 10),
                text: comment
                    .text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            })
            .collect();
        state.detail_comment_count = comments.map_or(0, |comments| comments.count);
        state.detail_comment_source = comments
            .and_then(|comments| comments.comments.first())
            .map(|comment| comment.source.clone())
            .unwrap_or_default();
        state.detail_scores = found
            .map_or(&[][..], |found| &found.rating_details[..])
            .iter()
            // 0 分是那个站还没有评分，不显示。
            .filter(|(_, value)| *value > 0.0)
            .enumerate()
            .map(|(at, (site, value))| DetailScore {
                id: at as i64 + 1,
                site: site.clone(),
                score: details::score(*value),
            })
            .collect();
        state.detail_facts = found
            .map(|found| details::facts(found, 9))
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(at, (key, value))| DetailFact {
                id: at as i64 + 1,
                key,
                value,
            })
            .collect();

        // 背景：有剧照用剧照（清晰），没有用海报（虚化）。换了图编号才变，界面借此淡入淡出。
        let still = detail
            .stills
            .get(detail.still_at % detail.stills.len().max(1));
        let (url, is_still) = match still {
            Some(path) => (MediaSource::file(path).url, true),
            None => (poster_url.unwrap_or_default(), false),
        };
        state.detail_rotates = detail.stills.len() > 1;
        if url.is_empty() {
            state.detail_backdrop.clear();
        } else {
            if self.backdrop.0 != url {
                self.backdrop = (url.clone(), self.backdrop.1 + 1);
            }
            state.detail_backdrop = vec![Backdrop {
                id: self.backdrop.1,
                source: MediaSource::new(url),
                still: is_still,
            }];
        }
    }

    /// 详情页关掉后的界面状态。
    fn close_detail_state(&mut self) {
        let state = &mut self.state;
        state.detail_open = false;
        state.detail_title.clear();
        state.detail_other_titles.clear();
        state.detail_poster = no_source();
        state.detail_has_poster = false;
        state.detail_backdrop.clear();
        state.detail_rotates = false;
        state.detail_known = false;
        state.detail_loading = false;
        state.detail_error.clear();
        state.detail_tags.clear();
        state.detail_summary.clear();
        state.detail_episodes.clear();
        state.detail_can_resume = false;
        state.detail_resume = no_source();
        state.detail_comments.clear();
        state.detail_comments_more = false;
        state.detail_scores.clear();
        state.detail_facts.clear();
    }

    /// 留下正在看的视频此刻的画面，给继续观看的卡片用。请求马上交给播放内核（排在之后的打开前面），
    /// 在后台等它存好再收下。没有播放内核（测试里）或没有地方存时什么也不做。
    fn take_shot(&mut self) {
        let (Some((url, _)), Some(folder)) = (&self.playing, &self.shots) else {
            return;
        };
        let path = folder.join(format!("{:016x}-{}.jpg", url_hash(url), unix_now()));
        let Some(done) = nkuplay_mpv::snapshot(path.clone()) else {
            return;
        };
        let url = url.clone();
        self.in_background_many(move |send| {
            if done.recv_timeout(Duration::from_secs(5)).unwrap_or(false) {
                send(Arrival::Shot(url, path));
            }
        });
    }

    /// `record_progress` 的实现，`now` 是这一刻。
    fn record_progress_at(&mut self, position_ms: i64, duration_ms: Option<i64>, now: Instant) {
        let Some((url, opened_at)) = &self.playing else {
            return;
        };
        if now.duration_since(*opened_at) < RECORD_AFTER {
            return;
        }
        let Some(duration) = duration_ms.filter(|duration| *duration > 0) else {
            return;
        };
        let url = url.clone();
        if self.library.record(&url, position_ms, duration, unix_now()) {
            // 每几秒记一次：只存文件、更新详情页和继续观看的百分比，不重排海报墙。
            self.store.save(&self.library);
            let percent = self
                .library
                .progress
                .get(&url)
                .map_or(0, |watched| watched.percent());
            if let Some(recent) = self
                .state
                .recents
                .iter_mut()
                .find(|recent| recent.source.url == url)
            {
                recent.percent = percent;
            }
            self.publish_detail();
        }
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
                    hero: format!("shin-{}", anime.id),
                }
            })
            .collect();
        self.state.shin_count = self.state.shin_shown.len() as i64;
    }

    fn take_season(&mut self, season: Season) {
        self.state.shin = ShinStatus::Loaded;
        self.state.shin_outdated = season.id < self.today.season();
        self.state.shin_error.clear();
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
    /// 在后台扫描文件夹 `url`：先按文件名归出番剧交回来，再对还没认出的每部番请弹弹play
    /// 认一集（认不出就换下一集再试一次），认出一部交回一部。已经认出的沿用上次的结果。
    /// 没有弹弹play 凭证时只归类、不识别。
    fn scan_folder(&mut self, url: String) {
        if !self.scanning.insert(url.clone()) {
            return;
        }
        let known = self.library.identities(&url);
        let loader = self.danmaku.clone();
        self.in_background_many(move |send| {
            let grouped = library::videos(&url).map(|files| library::group(&files, &known));
            let unknown: Vec<(String, Vec<MediaSource>)> = grouped
                .iter()
                .flatten()
                .filter(|series| series.identity.is_none())
                .map(|series| {
                    let tries = series.episodes.iter().take(2);
                    let sources = tries.map(|episode| MediaSource::new(episode.url.clone()));
                    (series.key.clone(), sources.collect())
                })
                .collect();
            send(Arrival::Grouped(url.clone(), grouped));
            if let Some(loader) = loader {
                for (key, sources) in unknown {
                    for source in sources {
                        match loader.identify(&source) {
                            Ok(Some(identity)) => {
                                send(Arrival::Identified(url.clone(), key.clone(), identity));
                                break;
                            }
                            Ok(None) => continue,
                            // 读不了文件或连不上：这部番先不认了，下次刷新再试。
                            Err(_) => break,
                        }
                    }
                }
            }
            send(Arrival::Scanned(url));
        });
    }
}

impl nkuplay::Commands for NkuplayEngine {
    fn open_file(&mut self, file: MediaSource) -> Result<(), CommandError> {
        // 在播放页里直接换片：先留下正在看的这个的画面。
        if self.state.has_video {
            self.take_shot();
        }
        self.state.source = file.clone();
        self.state.opened += 1;
        // 跳转请求只对当时的视频有效；归零后界面不会把它用在新视频上。
        self.state.seek_serial = 0;
        self.last_seek = None;
        self.state.has_video = true;
        let name = file.file_name().unwrap_or_default();
        self.library.opened(&file.url, &name);
        // 看到一半的接着播：在界面让播放器打开之前告诉播放内核从哪里开始。
        match self.library.progress.get(&file.url) {
            Some(watched) if !watched.finished() && watched.position_ms > RESUME_FROM => {
                nkuplay_mpv::start_at(&file.url, watched.position_ms - RESUME_REWIND);
            }
            _ => nkuplay_mpv::no_start(),
        }
        self.playing = Some((file.url.clone(), Instant::now()));
        self.save_library();
        self.start_danmaku(file);
        Ok(())
    }

    fn close(&mut self) -> Result<(), CommandError> {
        if self.state.has_video {
            self.take_shot();
        }
        self.state.source = no_source();
        self.state.has_video = false;
        self.last_seek = None;
        self.reset_danmaku(DanmakuStatus::Off);
        Ok(())
    }

    fn seek(&mut self, fraction: f64, duration_ms: Option<i64>) -> Result<(), CommandError> {
        if !fraction.is_finite() {
            return Err(CommandError::new(
                "invalid_fraction",
                "the position must be a number",
            ));
        }
        if let Some(point) = point_of(fraction, duration_ms) {
            self.seek_to(point, Instant::now());
        }
        Ok(())
    }

    fn skip(
        &mut self,
        seconds: f64,
        position_ms: i64,
        duration_ms: Option<i64>,
    ) -> Result<(), CommandError> {
        self.skip_at(seconds, position_ms, duration_ms, Instant::now())
    }

    fn set_volume(&mut self, volume: f64) -> Result<(), CommandError> {
        self.state.volume = volume_of(volume)
            .ok_or_else(|| CommandError::new("invalid_volume", "the volume must be a number"))?;
        Ok(())
    }

    fn nudge_volume(&mut self, step: f64) -> Result<(), CommandError> {
        self.set_volume(self.state.volume + step)
    }

    fn toggle_danmaku(&mut self) -> Result<(), CommandError> {
        self.state.danmaku_visible = !self.state.danmaku_visible;
        nkuplay_mpv::show_danmaku(self.state.danmaku_visible);
        Ok(())
    }

    fn refresh_recents(&mut self) -> Result<(), CommandError> {
        // 本机文件打不开了就去掉；网络地址留着。
        self.library
            .prune(|url| !url.starts_with("file:") || MediaSource::new(url).open().is_ok());
        self.save_library();
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

    fn refresh_library(&mut self) -> Result<(), CommandError> {
        let urls: Vec<String> = self.library.folders.iter().map(|f| f.url.clone()).collect();
        for url in urls {
            self.scan_folder(url);
        }
        self.publish_library();
        Ok(())
    }

    fn open_series(&mut self, id: i64) -> Result<(), CommandError> {
        let item = usize::try_from(id - 1)
            .ok()
            .and_then(|at| self.shelf.get(at));
        let Some(item) = item else {
            return Err(CommandError::new(
                "unknown_series",
                format!("there is no series {id}"),
            ));
        };
        let anime_id = item
            .key
            .strip_prefix("anime:")
            .and_then(|id| id.parse().ok());
        let (key, title, poster) = (item.key.clone(), item.title.clone(), item.poster.clone());
        let hero = format!("shelf-{key}");
        self.show_detail(key, anime_id, (title, poster), hero);
        Ok(())
    }

    fn open_anime(&mut self, anime_id: i64) -> Result<(), CommandError> {
        if anime_id <= 0 {
            return Err(CommandError::new(
                "unknown_anime",
                format!("there is no anime {anime_id}"),
            ));
        }
        let key = format!("anime:{anime_id}");
        // 资料到之前先显示新番列表或海报墙上的作品名和海报。
        let known = self
            .season
            .iter()
            .flat_map(|season| &season.anime)
            .find(|anime| anime.id == anime_id)
            .map(|anime| (anime.title.clone(), anime.poster.clone()))
            .or_else(|| {
                let item = self.shelf.iter().find(|item| item.key == key)?;
                Some((item.title.clone(), item.poster.clone()))
            });
        self.show_detail(
            key,
            Some(anime_id),
            known.unwrap_or_default(),
            format!("shin-{anime_id}"),
        );
        Ok(())
    }

    fn close_details(&mut self) -> Result<(), CommandError> {
        self.detail = None;
        self.publish_detail();
        Ok(())
    }

    fn refresh_details(&mut self) -> Result<(), CommandError> {
        self.download_details();
        self.publish_detail();
        Ok(())
    }

    fn toggle_episode_order(&mut self) -> Result<(), CommandError> {
        if let Some(detail) = self.detail.as_mut() {
            detail.descending = !detail.descending;
        }
        self.publish_detail();
        Ok(())
    }

    fn clear_progress(&mut self) -> Result<(), CommandError> {
        let Some((rows, _)) = self.detail_rows() else {
            return Ok(());
        };
        self.library
            .forget(rows.iter().filter_map(|row| row.url.as_deref()));
        self.store.save(&self.library);
        self.want_stills(true);
        self.publish_detail();
        Ok(())
    }

    fn toggle_comments(&mut self) -> Result<(), CommandError> {
        if let Some(detail) = self.detail.as_mut() {
            detail.comments_expanded = !detail.comments_expanded;
        }
        self.publish_detail();
        Ok(())
    }

    fn next_backdrop(&mut self) -> Result<(), CommandError> {
        if let Some(detail) = self.detail.as_mut()
            && !detail.stills.is_empty()
        {
            detail.still_at = (detail.still_at + 1) % detail.stills.len();
        }
        self.publish_detail();
        Ok(())
    }

    fn record_progress(
        &mut self,
        position_ms: i64,
        duration_ms: Option<i64>,
    ) -> Result<(), CommandError> {
        self.record_progress_at(position_ms, duration_ms, Instant::now());
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
                Arrival::Grouped(url, grouped) => {
                    if let (Ok(series), Some(folder)) = (
                        grouped,
                        self.library.folders.iter_mut().find(|f| f.url == url),
                    ) {
                        folder.videos = series.iter().map(|s| s.episodes.len() as i64).sum();
                        folder.series = series;
                    }
                    self.save_library();
                }
                Arrival::Identified(url, key, identity) => {
                    let series = self
                        .library
                        .folders
                        .iter_mut()
                        .filter(|folder| folder.url == url)
                        .flat_map(|folder| folder.series.iter_mut())
                        .find(|series| series.key == key);
                    if let Some(series) = series {
                        series.identity = Some(identity);
                    }
                    self.save_library();
                }
                Arrival::Scanned(url) => {
                    self.scanning.remove(&url);
                    self.publish_library();
                }
                Arrival::Details(anime_id, result) => {
                    let Some(detail) = self
                        .detail
                        .as_mut()
                        .filter(|detail| detail.anime_id == Some(anime_id))
                    else {
                        continue;
                    };
                    detail.loading = false;
                    match result {
                        Ok(info) => detail.info = Some(info),
                        // 取不到时，之前的资料（如果有）照旧显示。
                        Err(reason) => detail.error = reason,
                    }
                    // 这里拿不到唤醒器，只看缓存；缺的剧照由取资料的那件后台的事顺便取。
                    self.want_stills(false);
                    self.publish_detail();
                }
                Arrival::Shot(url, path) => {
                    self.library.shot(&url, path);
                    self.save_library();
                }
                Arrival::Stills(key, found) => {
                    let wanted = self.wanted_stills().map(|(wanted, ..)| wanted);
                    if let Some(detail) = self.detail.as_mut()
                        && wanted.as_deref() == Some(key.as_str())
                        && !found.is_empty()
                    {
                        detail.stills_for = Some(key);
                        detail.stills = found;
                        detail.still_at = 0;
                        self.publish_detail();
                    }
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

    /// 没有新番、详情和剧照来源的。
    fn no_sources() -> Sources {
        Sources {
            shin: shin::Source::new(None, None),
            details: details::Source::new(None, None),
            stills: tmdb::Stills::new(None, None),
            shots: None,
        }
    }

    /// 不存文件、没有新番来源的引擎。
    fn engine(danmaku: Option<Arc<Loader>>, run: fn(Job)) -> NkuplayEngine {
        NkuplayEngine::new(
            danmaku,
            no_sources(),
            library::Store::new(None),
            today(),
            run,
        )
    }

    /// 详情来自假弹弹play（不缓存）的引擎。
    fn engine_with_details(api: details::tests::FakeApi) -> NkuplayEngine {
        NkuplayEngine::new(
            None,
            Sources {
                details: details::Source::new(Some(Box::new(api)), None),
                ..no_sources()
            },
            library::Store::new(None),
            today(),
            at_once,
        )
    }

    fn engine_with(api: FakeApi, run: fn(Job)) -> NkuplayEngine {
        engine(Some(Arc::new(Loader::new(api))), run)
    }

    /// 新番来自假弹弹play、缓存放在 `cache` 的引擎。
    fn engine_with_shin(api: shin::tests::FakeApi, cache: Option<PathBuf>) -> NkuplayEngine {
        NkuplayEngine::new(
            None,
            Sources {
                shin: shin::Source::new(Some(Box::new(api)), cache),
                ..no_sources()
            },
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
    fn refreshing_continue_watching_drops_files_that_are_gone() {
        let video = Video::named("still-here.mkv");
        let mut engine = engine(None, at_once);
        engine.open_file(video.source()).unwrap();
        engine
            .open_file(MediaSource::file("Z:/no/such/file.mkv"))
            .unwrap();
        assert_eq!(engine.state.recent_count, 2);
        engine.refresh_recents().unwrap();
        assert_eq!(engine.state.recent_count, 1);
        assert_eq!(engine.state.recents[0].source, video.source());
    }

    #[test]
    fn continue_watching_and_the_folders_survive_a_restart() {
        let data = Scratch::named("data");
        let anime = Scratch::named("anime");
        std::fs::write(anime.0.join("01.mkv"), b"x").unwrap();
        let start = || {
            NkuplayEngine::new(
                None,
                no_sources(),
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

    /// 放好几个视频文件的文件夹。
    fn shows(name: &str) -> Scratch {
        let folder = Scratch::named(name);
        for file in [
            "[ANi] 再見菈菈 - 11 [1080P].mp4",
            "[ANi] 再見菈菈 - 10 [1080P].mp4",
            "[LoliHouse] Yani Neko - 08 [WebRip].mkv",
        ] {
            std::fs::write(folder.0.join(file), file.as_bytes()).unwrap();
        }
        folder
    }

    #[test]
    fn without_credentials_a_folder_is_grouped_by_file_name_only() {
        let anime = shows("shelf-plain");
        let mut engine = engine(None, at_once);
        engine.add_folder(MediaSource::file(&anime.0)).unwrap();
        assert!(engine.state.library_scanning);
        engine.wake();
        assert!(!engine.state.library_scanning);
        let wall: Vec<(&str, i64, bool)> = engine
            .state
            .series
            .iter()
            .map(|s| (s.title.as_str(), s.episodes, s.has_poster))
            .collect();
        assert_eq!(wall, [("Yani Neko", 1, false), ("再見菈菈", 2, false)]);
        assert_eq!(engine.state.folders[0].videos, 3);
    }

    #[test]
    fn each_show_is_identified_once_and_the_same_work_is_merged() {
        let anime = shows("shelf-identified");
        let api = FakeApi::recognising();
        let calls = Arc::clone(&api.match_calls);
        let mut engine = engine_with(api, at_once);
        engine.add_folder(MediaSource::file(&anime.0)).unwrap();
        engine.wake();
        // 两部番各认一集；假的弹弹play 把什么都认成“某作品”，于是合成一部。
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(engine.state.series_count, 1);
        assert_eq!(engine.state.series[0].title, "某作品");
        assert_eq!(engine.state.series[0].episodes, 3);
        // 刷新：认过的沿用，不再问。
        engine.refresh_library().unwrap();
        engine.wake();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(engine.state.series_count, 1);
    }

    #[test]
    fn a_show_opens_its_details_page_which_stays_open_while_an_episode_plays() {
        let anime = shows("shelf-open");
        let mut engine = engine(None, at_once);
        engine.add_folder(MediaSource::file(&anime.0)).unwrap();
        engine.wake();
        assert!(engine.open_series(9).is_err());
        engine.open_series(2).unwrap();
        assert!(engine.state.detail_open);
        assert_eq!(engine.state.detail_title, "再見菈菈");
        // 没认出的番：没有弹弹play 的资料，只列本地文件。
        assert!(!engine.state.detail_known);
        assert!(!engine.state.detail_loading);
        let episodes: Vec<(i64, bool)> = engine
            .state
            .detail_episodes
            .iter()
            .map(|e| (e.number, e.local))
            .collect();
        assert_eq!(episodes, [(10, true), (11, true)]);
        assert_eq!(engine.state.detail_local_count, 2);
        assert!(engine.state.detail_can_resume);
        assert!(!engine.state.detail_started);
        let first = engine.state.detail_resume.clone();
        assert_eq!(first, engine.state.detail_episodes[0].source);
        // 播放一集，回来时详情页还在。
        engine.open_file(first).unwrap();
        engine.close().unwrap();
        assert!(engine.state.detail_open);
        engine.toggle_episode_order().unwrap();
        assert_eq!(engine.state.detail_episodes[0].number, 11);
        engine.close_details().unwrap();
        assert!(!engine.state.detail_open);
        assert!(engine.state.detail_episodes.is_empty());
    }

    #[test]
    fn a_new_show_opens_with_its_dandanplay_details() {
        let mut engine = engine_with_details(details::tests::FakeApi::lara());
        assert!(engine.open_anime(0).is_err());
        engine.open_anime(18643).unwrap();
        assert!(engine.state.detail_open);
        assert!(engine.state.detail_loading);
        engine.wake();
        let state = &engine.state;
        assert!(state.detail_known && !state.detail_loading);
        assert_eq!(state.detail_title, "再见，拉拉");
        assert_eq!(state.detail_rating_text, "6.9");
        assert_eq!(state.detail_kind, "TV动画");
        assert_eq!(
            (state.detail_year.as_str(), state.detail_month),
            ("2026", 7)
        );
        assert_eq!(state.detail_tags.len(), 6);
        assert_eq!(
            (state.detail_main_count, state.detail_special_count),
            (3, 2)
        );
        assert_eq!(state.detail_local_count, 0);
        assert!(!state.detail_can_resume);
        assert_eq!(state.detail_episodes[3].code, "S1");
        assert_eq!(state.detail_comments[0].score, 5);
        // 假弹弹play 只给一条评论，没有可展开的。
        assert!(!state.detail_comments_more);
        assert_eq!(state.detail_comment_count, 20);
        assert_eq!(state.detail_comment_source, "Bangumi");
        // 没有评分（0 分）的站不显示。
        assert_eq!(state.detail_scores.len(), 1);
        assert_eq!(state.detail_scores[0].score, "6.4");
        assert_eq!(state.detail_facts[0].key, "中文名");
        // 没有剧照时背景是海报，虚化显示。
        assert_eq!(state.detail_backdrop.len(), 1);
        assert!(!state.detail_backdrop[0].still);
        assert!(!state.detail_rotates);
    }

    #[test]
    fn four_comments_show_until_the_rest_of_the_page_is_expanded() {
        let mut engine = engine_with_details(details::tests::FakeApi {
            comment_count: 7,
            ..details::tests::FakeApi::lara()
        });
        engine.open_anime(18643).unwrap();
        engine.wake();
        assert_eq!(engine.state.detail_comments.len(), 4);
        assert!(engine.state.detail_comments_more);
        engine.toggle_comments().unwrap();
        assert_eq!(engine.state.detail_comments.len(), 7);
        assert!(!engine.state.detail_comments_more);
        assert!(engine.state.detail_comments_expanded);
        engine.toggle_comments().unwrap();
        assert_eq!(engine.state.detail_comments.len(), 4);
        // 换一部番重新从收起开始。
        engine.toggle_comments().unwrap();
        engine.open_anime(18643).unwrap();
        assert!(!engine.state.detail_comments_expanded);
    }

    #[test]
    fn a_failed_lookup_says_why_and_keeps_the_page() {
        let mut engine = engine_with_details(details::tests::FakeApi {
            error: Some("timed out"),
            ..details::tests::FakeApi::lara()
        });
        engine.open_anime(18643).unwrap();
        engine.wake();
        assert!(engine.state.detail_open);
        assert!(!engine.state.detail_known);
        assert!(engine.state.detail_error.contains("timed out"));
    }

    #[test]
    fn progress_is_recorded_after_the_first_seconds_and_drives_continue() {
        let anime = shows("shelf-progress");
        let mut engine = engine(None, at_once);
        engine.add_folder(MediaSource::file(&anime.0)).unwrap();
        engine.wake();
        engine.open_series(2).unwrap();
        let tenth = engine.state.detail_episodes[0].source.clone();
        let eleventh = engine.state.detail_episodes[1].source.clone();

        engine.open_file(tenth.clone()).unwrap();
        let opened = engine.playing.as_ref().unwrap().1;
        // 刚打开的几秒里不记。
        engine.record_progress_at(500_000, Some(1_440_000), opened);
        assert!(engine.library.progress.is_empty());
        // 不知道总时长时不记。
        engine.record_progress_at(500_000, None, opened + RECORD_AFTER);
        assert!(engine.library.progress.is_empty());
        engine.record_progress_at(720_000, Some(1_440_000), opened + RECORD_AFTER);
        engine.close().unwrap();
        // 离开播放页后还记在刚看的那个视频上。
        engine.record_progress_at(730_000, Some(1_440_000), opened + RECORD_AFTER * 2);
        let tenth_row = &engine.state.detail_episodes[0];
        assert_eq!(
            (tenth_row.percent, tenth_row.finished, tenth_row.current),
            (50, false, true)
        );
        assert!(engine.state.detail_started);
        assert!(engine.state.detail_has_last);
        assert_eq!(engine.state.detail_last_number, 10);
        assert_eq!(engine.state.detail_resume, tenth);
        // 继续观看的卡片上也是 50%。
        assert_eq!(engine.state.recents[0].percent, 50);
        assert!(!engine.state.recents[0].has_shot);

        // 看完第 10 集：“继续”指向第 11 集。
        engine.open_file(tenth).unwrap();
        let opened = engine.playing.as_ref().unwrap().1;
        engine.record_progress_at(1_400_000, Some(1_440_000), opened + RECORD_AFTER);
        assert!(engine.state.detail_episodes[0].finished);
        assert_eq!(engine.state.detail_resume, eleventh);

        engine.clear_progress().unwrap();
        assert!(engine.library.progress.is_empty());
        assert!(!engine.state.detail_started);
        assert_eq!(engine.state.detail_episodes[0].percent, 0);
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

    #[test]
    fn opening_a_video_asks_the_player_to_load_it() {
        let mut engine = engine(None, at_once);
        engine
            .open_file(MediaSource::file("C:/videos/a.mkv"))
            .unwrap();
        assert_eq!(engine.state.opened, 1);
        // 再打开同一个文件也算一次：界面要让播放器重新载入。
        engine
            .open_file(MediaSource::file("C:/videos/a.mkv"))
            .unwrap();
        assert_eq!(engine.state.opened, 2);
    }

    #[test]
    fn seeking_turns_the_fraction_into_milliseconds() {
        let mut engine = engine(None, at_once);
        engine.seek(0.5, Some(1_441_000)).unwrap();
        assert_eq!(
            (engine.state.seek_serial, engine.state.seek_ms),
            (1, 720_500)
        );
        // 跳到同一处也要再跳一次。
        engine.seek(0.5, Some(1_441_000)).unwrap();
        assert_eq!(engine.state.seek_serial, 2);
        // 超出的比例落在两端，不超过总时长。
        engine.seek(1.5, Some(1_441_000)).unwrap();
        assert_eq!(engine.state.seek_ms, 1_441_000);
        engine.seek(-1.0, Some(1_441_000)).unwrap();
        assert_eq!(engine.state.seek_ms, 0);
        // 不知道总时长时不跳。
        engine.seek(0.5, None).unwrap();
        assert_eq!(engine.state.seek_serial, 4);
        assert!(engine.seek(f64::NAN, Some(1_000)).is_err());
        // 换视频后从零开始。
        engine
            .open_file(MediaSource::file("C:/videos/b.mkv"))
            .unwrap();
        assert_eq!(engine.state.seek_serial, 0);
    }

    #[test]
    fn the_volume_stays_between_zero_and_one() {
        let mut engine = engine(None, at_once);
        assert_eq!(engine.state.volume, 1.0);
        engine.nudge_volume(0.05).unwrap();
        assert_eq!(engine.state.volume, 1.0);
        engine.set_volume(0.5).unwrap();
        engine.nudge_volume(-0.05).unwrap();
        assert!((engine.state.volume - 0.45).abs() < 1e-9);
        engine.set_volume(-3.0).unwrap();
        assert_eq!(engine.state.volume, 0.0);
        assert!(engine.set_volume(f64::NAN).is_err());
        // 音量跟着引擎走，换视频不变。
        engine.set_volume(0.3).unwrap();
        engine
            .open_file(MediaSource::file("C:/videos/b.mkv"))
            .unwrap();
        assert_eq!(engine.state.volume, 0.3);
    }

    #[test]
    fn arrow_keys_skip_from_the_position_and_chain_while_the_player_catches_up() {
        let mut engine = engine(None, at_once);
        let start = Instant::now();
        engine.skip_at(5.0, 60_000, Some(1_440_000), start).unwrap();
        assert_eq!(
            (engine.state.seek_serial, engine.state.seek_ms),
            (1, 65_000)
        );
        // 播放器还没报回新位置时再按：接着上一次的目标加。
        let soon = start + Duration::from_millis(300);
        engine.skip_at(5.0, 60_000, Some(1_440_000), soon).unwrap();
        assert_eq!(engine.state.seek_ms, 70_000);
        // 过了一会儿再按：从播放器报的位置算。
        let later = soon + SKIP_CHAIN;
        engine
            .skip_at(-5.0, 100_000, Some(1_440_000), later)
            .unwrap();
        assert_eq!(engine.state.seek_ms, 95_000);
        // 落在两端。
        let much_later = later + SKIP_CHAIN;
        engine
            .skip_at(-5.0, 2_000, Some(1_440_000), much_later)
            .unwrap();
        assert_eq!(engine.state.seek_ms, 0);
        let end = much_later + SKIP_CHAIN;
        engine
            .skip_at(5.0, 1_438_000, Some(1_440_000), end)
            .unwrap();
        assert_eq!(engine.state.seek_ms, 1_440_000);
        // 不知道总时长时不跳。
        let serial = engine.state.seek_serial;
        engine.skip(5.0, 0, None).unwrap();
        assert_eq!(engine.state.seek_serial, serial);
        assert!(engine.skip(f64::NAN, 0, Some(1_000)).is_err());
    }

    #[test]
    fn danmaku_can_be_hidden_and_stays_hidden_for_the_next_video() {
        let mut engine = engine(None, at_once);
        assert!(engine.state.danmaku_visible);
        engine.toggle_danmaku().unwrap();
        assert!(!engine.state.danmaku_visible);
        engine
            .open_file(MediaSource::file("C:/videos/b.mkv"))
            .unwrap();
        assert!(!engine.state.danmaku_visible);
        engine.toggle_danmaku().unwrap();
        assert!(engine.state.danmaku_visible);
    }

    #[test]
    fn comments_are_handed_to_the_kernel_with_their_mode_and_color() {
        let comments = [
            Comment {
                id: 1,
                time: 1.5,
                mode: CommentMode::Top,
                color: 0xFF0000,
                user: "u".into(),
                text: "顶部".into(),
            },
            Comment {
                id: 2,
                time: 3.0,
                mode: CommentMode::Bottom,
                color: 0x00FF00,
                user: "u".into(),
                text: "底部".into(),
            },
        ];
        assert_eq!(
            for_kernel(&comments),
            [
                DanmakuComment {
                    time: 1.5,
                    mode: DanmakuMode::Top,
                    color: 0xFF0000,
                    text: "顶部".into()
                },
                DanmakuComment {
                    time: 3.0,
                    mode: DanmakuMode::Bottom,
                    color: 0x00FF00,
                    text: "底部".into()
                },
            ]
        );
    }

    #[test]
    fn a_new_video_does_not_chain_onto_the_last_skip() {
        let mut engine = engine(None, at_once);
        engine.skip(5.0, 60_000, Some(1_440_000)).unwrap();
        engine
            .open_file(MediaSource::file("C:/videos/b.mkv"))
            .unwrap();
        engine.skip(5.0, 0, Some(1_440_000)).unwrap();
        assert_eq!(engine.state.seek_ms, 5_000);
    }
}
