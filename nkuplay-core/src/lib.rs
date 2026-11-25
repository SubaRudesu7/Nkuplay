//! Nkuplay 的引擎：业务逻辑，纯 Rust。它不知道窗口的存在，所以不需要窗口就能测试。

mod airtimes;
mod danmaku;
mod details;
mod library;
mod library_page;
mod naming;
mod picture;
mod player;
mod settings;
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
    self, AfterEnd, Amount, Appearance, Aspect, Backdrop, BarStay, DanmakuFont, DanmakuStatus,
    DayChip, DetailComment, DetailEpisode, DetailFact, DetailGroup, DetailScore, DetailTag, Fill,
    Folder, HeroDot, HoldRate, HomeMark, KeyAction, LibraryFilter, LibrarySort, Link, Page, Recent,
    Section, SeekStep, Series, ShinAnime, ShinStatus, SkipMode, State, Toggle, Upscale, Weekday,
};
use shin::Season;
use std::collections::{HashMap, HashSet};
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
    /// 一集的缩略图（第一项是 [`stills_key`]，第二项是第几集）。
    Thumb(String, u32, PathBuf),
    /// 首页大图要的剧照下载好了。
    HomeStills,
    /// 视频（第一项）离开时的画面存好了；第三项是它几乎全黑。
    Shot(String, PathBuf, bool),
    /// 以前存下的画面，各是不是几乎全黑。
    ShotLooks(Vec<(PathBuf, bool)>),
    /// 新番的播出时间。
    Airtimes(airtimes::Table),
    /// 播放内核报回的章节和编码信息，带着发起时的 `generation`。
    Media(u64, nkuplay_mpv::MediaInfo),
    /// 播放页那部番的资料或一集缩略图取到了，带着发起时的 `generation`。
    PlayImages(u64),
    /// 手动匹配搜到的作品，带着发起时的 `generation`。
    Searched(u64, Result<Vec<nkuplay_dandanplay::SearchedAnime>, String>),
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
    group: details::Group,
    /// 剧集条该滚到哪一行（行号从 1 开始）。
    strip_at: i64,
    /// 评论展开着。
    comments_expanded: bool,
    /// 背景的剧照，正在显示第 `still_at` 张。
    stills: Vec<PathBuf>,
    still_at: usize,
    /// 背景剧照是为哪一集取的（[`stills_key`]）。
    stills_for: Option<String>,
    /// 各集的缩略图，按集数。
    thumbs: HashMap<u32, PathBuf>,
    /// 已经在后台取缩略图了。
    thumbs_wanted: bool,
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

/// 一集最多取几张背景剧照（TMDB 上一集通常六七张，全取）。
const MAX_STILLS: usize = 20;
/// 剧集条翻一屏是几集。
const STRIP_PAGE: i64 = 4;

/// 一共 `count` 集时，剧集条最多滚到第几集（从 0 数）在最前面：后面还看得到一屏。
fn last_page(count: usize) -> usize {
    count.saturating_sub(STRIP_PAGE as usize)
}

/// 看了 `percent`（百分之几）时，进度条画多长，0 到 1。
fn bar_of(percent: i64) -> f64 {
    (percent as f64 / 100.0).clamp(0.02, 1.0)
}

/// 剧集名开头的“第N话”，例如 `第5话 救救我` 是 `第5话`；不是这样写的为空。
fn part_of(episode: &str) -> String {
    episode
        .split_whitespace()
        .next()
        .filter(|first| first.starts_with('第'))
        .unwrap_or_default()
        .to_string()
}

/// 写成北京时间的几点几分。弹弹play 的星期按北京时间算，用别的时区会和星期对不上。
fn beijing_time(at: chrono::DateTime<chrono::Utc>) -> String {
    (at + chrono::Duration::hours(8))
        .format("%H:%M")
        .to_string()
}

fn group_of(group: details::Group) -> DetailGroup {
    match group {
        details::Group::Main => DetailGroup::Main,
        details::Group::Special => DetailGroup::Special,
        details::Group::Extra => DetailGroup::Extra,
    }
}

/// 文字里的第一个数字，例如 `第11话 魔女克芮丝` 是 11。
fn first_number(text: &str) -> Option<u32> {
    text.split(|c: char| !c.is_ascii_digit())
        .find(|part| !part.is_empty())?
        .parse()
        .ok()
}
/// 刚打开视频后多久以内不记进度：播放器报的位置可能还是上一个视频的。
const RECORD_AFTER: Duration = Duration::from_secs(3);
/// 看过不到这么多（毫秒）的不续播，从头开始。
const RESUME_FROM: i64 = 10_000;
/// 续播时往回倒一点（毫秒），接上前面的情节。
const RESUME_REWIND: i64 = 2_000;

/// 取第 `episode` 集的背景剧照和各集的缩略图，取到一样交回一样。会阻塞。
fn fetch_images(
    stills: &tmdb::Stills,
    found: &nkuplay_dandanplay::BangumiDetails,
    episode: u32,
    send: &dyn Fn(Arrival),
) {
    let Some((tv, season)) = details::tmdb(found).filter(|_| stills.available()) else {
        return;
    };
    if let Ok(images) = stills.fetch(tv, season, episode, MAX_STILLS) {
        send(Arrival::Stills(stills_key(tv, season, episode), images));
    }
    let numbers = found
        .episodes
        .iter()
        .filter_map(|episode| episode.episode_number.parse::<u32>().ok());
    for number in numbers {
        if let Some(path) = stills.fetch_thumb(tv, season, number) {
            send(Arrival::Thumb(stills_key(tv, season, number), number, path));
        }
    }
}

pub(crate) fn unix_now() -> u64 {
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
    /// 新番播出时间的来源，和现在有的播出时间。
    airtimes_source: Arc<airtimes::Source>,
    airtimes: airtimes::Table,
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
    /// 首页大图正在显示的那个视频的地址。
    hero_url: Option<String>,
    /// 首页大图用的剧照：作品编号和集数到图片；查过没有的记成空，免得反复读文件。
    hero_stills: HashMap<(i64, u32), Option<PathBuf>>,
    /// 作品的类别，也是查过就记下。
    kinds: HashMap<i64, String>,
    /// 首页的大图已经在后台准备过。
    home_loaded: bool,
    today: Today,
    /// 后台做完的事从这里回来。
    results: (Sender<Arrival>, Receiver<Arrival>),
    /// 把一件事放到引擎线程之外去做。
    run: fn(Job),
    /// 最近一次要跳到的位置（毫秒）和请求的时刻。播放器跳过去、报回新位置要一点时间，
    /// 这段时间里连按方向键要在这个位置上接着加减，而不是在还没更新的位置上。
    last_seek: Option<(i64, Instant)>,
    /// 设置，以及存它的文件夹（没有时不存）。
    settings: settings::Settings,
    settings_folder: Option<PathBuf>,
    /// 播放页要记着的。
    play: player::Play,
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
                airtimes: airtimes::Source::built_in(),
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
    airtimes: airtimes::Source,
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
        let settings_folder = store.folder().cloned();
        let settings = settings::load(settings_folder.as_ref());
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
                hero: Vec::new(),
                hero_dots: Vec::new(),
                // 记号在 0、380 和 410：滚到 400 左右报 102。“继续播放”的标题在 450，再滚就到顶栏下面了。
                home_marks: [(100, 380.0), (101, 30.0), (102, 1.0)]
                    .map(|(id, height)| HomeMark { id, height })
                    .into(),
                today: Vec::new(),
                today_count: 0,
                today_day: today.weekday.clone(),
                shin: ShinStatus::NotLoaded,
                shin_outdated: false,
                shin_year: String::new(),
                shin_month: 1,
                shin_day: today.weekday.clone(),
                shin_days: Vec::new(),
                shin_shown: Vec::new(),
                shin_count: 0,
                shin_error: String::new(),
                folders: Vec::new(),
                folder_count: 0,
                series: Vec::new(),
                series_count: 0,
                library_shown: Vec::new(),
                library_filter: LibraryFilter::All,
                library_sort: LibrarySort::Added,
                count_all: 0,
                count_watching: 0,
                count_unwatched: 0,
                count_finished: 0,
                library_scanning: false,
                detail_open: false,
                detail_title: String::new(),
                detail_native_title: String::new(),
                detail_poster: no_source(),
                detail_has_poster: false,
                detail_backdrop: Vec::new(),
                detail_rotates: false,
                detail_fixed: false,
                detail_can_switch: false,
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
                detail_group: DetailGroup::Main,
                detail_episodes: Vec::new(),
                detail_strip_at: 0,
                detail_strip_serial: 0,
                detail_main_count: 0,
                detail_special_count: 0,
                detail_extra_count: 0,
                detail_episode_count: 0,
                detail_minutes: 0,
                detail_can_resume: false,
                detail_resume: no_source(),
                detail_started: false,
                detail_resume_fraction: 0.0,
                detail_comments: Vec::new(),
                detail_comments_more: false,
                detail_comments_expanded: false,
                detail_comment_count: 0,
                detail_comment_source: String::new(),
                detail_scores: Vec::new(),
                detail_facts: Vec::new(),
                // 设置的值在 `publish_settings` 里填。
                appearance: Appearance::System,
                start_page: Page::Home,
                nav: Vec::new(),
                nav_shown: Vec::new(),
                resume: true,
                auto_fullscreen: false,
                after_end: AfterEnd::Next,
                countdown: 10.0,
                skip_mode: SkipMode::Button,
                skip_seconds: 90.0,
                seek_step: 5,
                hold_speed: 20,
                keep_pitch: true,
                bar_stay: 1,
                seek_choice: SeekStep::Five,
                hold_choice: HoldRate::Two,
                stay_choice: BarStay::One,
                chapter_marks: true,
                heat_curve: true,
                hover_preview: true,
                hwdec: true,
                cache_mb: 150.0,
                upscale: Upscale::Off,
                aspect: Aspect::Auto,
                fill: Fill::Fit,
                d_opacity: 70.0,
                d_size: 34.0,
                d_outline: 2.0,
                d_font: DanmakuFont::YaHei,
                d_area: 85.0,
                d_seconds: 8.0,
                d_gap: 15.0,
                d_overlap: false,
                d_scroll: true,
                d_top: true,
                d_bottom: true,
                d_colored: true,
                d_merge: false,
                blocked: Vec::new(),
                blocked_count: 0,
                keys: settings.keys(),
                play_title: String::new(),
                play_subtitle: String::new(),
                speed: 1.0,
                speed_text: "1.0×".into(),
                fast: false,
                has_previous: false,
                has_next: false,
                episodes: Vec::new(),
                episode_count: 0,
                volume_percent: 100,
                chapters: Vec::new(),
                heat: Vec::new(),
                skip_shown: false,
                skip_label: String::new(),
                skip_outro: false,
                next_shown: false,
                next_left: 0,
                next_code: String::new(),
                next_title: String::new(),
                next_thumb: no_source(),
                next_has_thumb: false,
                info: Vec::new(),
                subtitles: Vec::new(),
                subtitle_count: 0,
                danmaku_shift: 0.0,
                danmaku_shift_text: "0.0 秒".into(),
                danmaku_lines: Vec::new(),
                danmaku_source: String::new(),
                danmaku_from_file: false,
                preview: no_source(),
                preview_ready: false,
                match_open: false,
                match_file: String::new(),
                match_busy: false,
                match_animes: Vec::new(),
                match_episodes: Vec::new(),
                match_folder: true,
                match_ready: false,
                fullscreen_pending: false,
            },
            danmaku,
            comments: Arc::from([]),
            generation: 0,
            shin_source: Arc::new(sources.shin),
            airtimes_source: Arc::new(sources.airtimes),
            airtimes: airtimes::Table::default(),
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
            hero_url: None,
            hero_stills: HashMap::new(),
            kinds: HashMap::new(),
            home_loaded: false,
            today,
            results: channel(),
            run,
            last_seek: None,
            settings,
            settings_folder,
            play: player::Play::default(),
        };
        engine.publish_library();
        engine.publish_shin();
        engine.publish_settings();
        engine.apply_settings();
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
                    loaded.anime_id,
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
                self.danmaku_arrived(loaded.shift);
            }
            Outcome::NoMatch => {
                self.state.danmaku = DanmakuStatus::NoMatch;
                self.publish_play();
            }
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

    /// 继续播放的每一部番。
    fn watching(&mut self) -> Vec<Recent> {
        let recents: Vec<library::RecentFile> = self
            .library
            .watching()
            .into_iter()
            .map(|recent| library::RecentFile {
                anime_id: self.library.anime_of(recent),
                ..recent.clone()
            })
            .collect();
        recents
            .iter()
            .enumerate()
            .map(|(index, recent)| {
                let (poster_source, has_poster) = poster(recent.poster.as_deref());
                let shot = recent.shot.as_ref().filter(|shot| shot.is_file());
                // 大图：这一集的剧照，没有用离开时的画面，再没有用虚化的海报。
                let still = self.hero_still(recent);
                let picture = still.clone().or_else(|| shot.cloned());
                // 卡片：离开时的画面；没有，或者几乎全黑，用这一集的剧照。
                let shot = match still {
                    Some(still) if shot.is_none() || recent.shot_black == Some(true) => Some(still),
                    _ => shot.cloned(),
                };
                let percent = self
                    .library
                    .progress
                    .get(&recent.url)
                    .map_or(0, |watched| watched.percent());
                Recent {
                    id: index as i64 + 1,
                    title: recent.title.clone(),
                    episode: recent.episode.clone(),
                    source: MediaSource::new(recent.url.clone()),
                    has_shot: shot.is_some(),
                    shot: shot.map_or_else(no_source, MediaSource::file),
                    percent,
                    bar: bar_of(percent),
                    part: part_of(&recent.episode),
                    kind: self.kind_of(recent),
                    anime_id: recent.anime_id.unwrap_or(0),
                    has_backdrop: picture.is_some() || has_poster,
                    sharp: picture.is_some(),
                    backdrop: picture.map_or_else(|| poster_source.clone(), MediaSource::file),
                    poster: poster_source,
                    has_poster,
                }
            })
            .collect()
    }

    /// 视频所在那一集的剧照，只看缓存：详情和剧照都已经下载过才有。
    fn hero_still(&mut self, recent: &library::RecentFile) -> Option<PathBuf> {
        let key = (recent.anime_id?, first_number(&recent.episode)?);
        if let Some(known) = self.hero_stills.get(&key) {
            return known.clone();
        }
        let found = self
            .tmdb_of(key.0)
            .and_then(|(tv, season)| self.stills.cached(tv, season, key.1).into_iter().next());
        self.hero_stills.insert(key, found.clone());
        found
    }

    /// 作品的类别（例如“TV动画”），只看缓存里的详情；不知道时为空。
    fn kind_of(&mut self, recent: &library::RecentFile) -> String {
        let Some(anime_id) = recent.anime_id else {
            return String::new();
        };
        let source = &self.details_source;
        self.kinds
            .entry(anime_id)
            .or_insert_with(|| {
                source
                    .cached(anime_id)
                    .and_then(|info| info.details)
                    .map(|details| details.type_description)
                    .unwrap_or_default()
            })
            .clone()
    }

    /// 缓存里的详情或剧照变了：首页记下的重新查。
    fn forget_looked_up(&mut self) {
        self.hero_stills.clear();
        self.kinds.clear();
    }

    /// 作品在 TMDB 上的编号和季，只看缓存里的详情。
    fn tmdb_of(&self, anime_id: i64) -> Option<(i64, u32)> {
        let info = self.details_source.cached(anime_id)?;
        details::tmdb(info.details.as_ref()?)
    }

    /// 把首页大图放进界面状态；正在显示的那部不在了就换成第一部。
    fn publish_hero(&mut self) {
        let recents = &self.state.recents;
        let at = self
            .hero_url
            .as_ref()
            .and_then(|url| recents.iter().position(|recent| &recent.source.url == url))
            .unwrap_or(0);
        let shown = recents.get(at).cloned();
        self.hero_url = shown.as_ref().map(|recent| recent.source.url.clone());
        self.state.hero_dots = recents
            .iter()
            .enumerate()
            .map(|(index, recent)| HeroDot {
                id: recent.id,
                on: index == at,
            })
            .collect();
        self.state.hero = shown.into_iter().collect();
    }

    /// 把今天更新的新番放进界面状态。
    fn publish_today(&mut self) {
        self.state.today = self.shin_on(&self.today.weekday);
        self.state.today_count = self.state.today.len() as i64;
    }

    /// 新番列表里 `day` 那天更新的番，按播出时间排，不知道时间的在后面。
    fn shin_on(&self, day: &Weekday) -> Vec<ShinAnime> {
        let mut shown: Vec<ShinAnime> = self
            .season
            .iter()
            .flat_map(|season| season.anime.iter())
            .filter(|anime| weekday(anime.day) == *day)
            .map(|anime| {
                let (poster, has_poster) = poster(anime.poster.as_deref());
                ShinAnime {
                    id: anime.id,
                    title: anime.title.clone(),
                    poster,
                    has_poster,
                    time: self
                        .airtimes
                        .first_airing(&anime.title)
                        .map(beijing_time)
                        .unwrap_or_default(),
                }
            })
            .collect();
        shown.sort_by(|a, b| (a.time.is_empty(), &a.time).cmp(&(b.time.is_empty(), &b.time)));
        shown
    }

    /// 把继续观看和媒体库放进界面状态。
    fn publish_library(&mut self) {
        self.state.recents = self.watching();
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
        self.publish_hero();
        self.state.folder_count = self.state.folders.len() as i64;

        // 隐藏了的番不上海报墙。
        let hidden = self.library.hidden.clone();
        self.shelf = self.library.shelf();
        self.shelf.retain(|item| !hidden.contains(&item.key));
        self.state.series = self
            .shelf
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let (poster, has_poster) = poster(item.poster.as_deref());
                let watch = library_page::watch_of(item, &self.library.progress);
                Series {
                    id: index as i64 + 1,
                    title: item.title.clone(),
                    poster,
                    has_poster,
                    episodes: item.episodes.len() as i64,
                    watched_up_to: watch.up_to,
                    finished: watch.finished,
                    bar: watch.bar,
                    resume_number: watch.resume_number,
                }
            })
            .collect();
        self.state.series_count = self.state.series.len() as i64;
        self.publish_library_page();
        self.state.library_scanning = !self.scanning.is_empty();
        self.publish_detail();
    }

    /// 打开详情页：先显示缓存的资料（有的话），缓存没有或过期了就在后台向弹弹play 取。
    fn show_detail(
        &mut self,
        key: String,
        anime_id: Option<i64>,
        (title, poster): (String, Option<String>),
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
            group: details::Group::Main,
            strip_at: 0,
            comments_expanded: false,
            stills: Vec::new(),
            still_at: 0,
            stills_for: None,
            thumbs: HashMap::new(),
            thumbs_wanted: false,
        });
        self.read_thumbs();
        self.reset_strip();
        if stale {
            self.download_details();
        }
        self.want_images(true);
        self.publish_detail();
    }

    /// 在后台向弹弹play 取详情页这部番的资料；有 TMDB 密钥时顺便取剧照和缩略图。
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
            let found = info.as_ref().ok().and_then(|info| info.details.clone());
            send(Arrival::Details(anime_id, info));
            if let Some(found) = found {
                fetch_images(&stills, &found, episode, send);
            }
        });
    }

    /// 背景用哪一集的剧照：选中的那一集；它不是正片时用第 1 集。
    fn still_episode(&self) -> u32 {
        self.detail_rows()
            .and_then(|(rows, summary)| {
                rows[details::selected(&rows, &summary, self.playing_now())?].number
            })
            .unwrap_or(1)
    }

    /// 正在播放的视频的地址；没在播放时为空。
    fn playing_now(&self) -> Option<&str> {
        self.state
            .has_video
            .then_some(self.state.source.url.as_str())
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
        let summary = details::summarize(&rows);
        Some((rows, summary))
    }

    /// 剧集条滚到“继续”那一集附近（它前面露出两集）；没有时从头开始。
    fn reset_strip(&mut self) {
        let Some((rows, summary)) = self.detail_rows() else {
            return;
        };
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        let shown: Vec<usize> = (0..rows.len())
            .filter(|&at| rows[at].group == detail.group)
            .collect();
        let target = summary
            .next
            .and_then(|next| shown.iter().position(|&at| at == next))
            .unwrap_or(0);
        detail.strip_at = shown
            .get(target.saturating_sub(2).min(last_page(shown.len())))
            .map_or(0, |&at| at as i64 + 1);
        self.state.detail_strip_at = detail.strip_at;
        self.state.detail_strip_serial += 1;
    }

    /// 详情页这部番在 TMDB 上的编号和季；没有 TMDB 密钥、没打开详情页或作品不在 TMDB 上时为空。
    fn detail_tmdb(&self) -> Option<(i64, u32)> {
        if !self.stills.available() {
            return None;
        }
        let detail = self.detail.as_ref()?;
        details::tmdb(detail.info.as_ref()?.details.as_ref()?)
    }

    /// 背景该显示的那一集剧照的标识。
    fn wanted_stills(&self) -> Option<(String, i64, u32, u32)> {
        let (tv, season) = self.detail_tmdb()?;
        let episode = self.still_episode();
        Some((stills_key(tv, season, episode), tv, season, episode))
    }

    /// 把缓存里已经有的缩略图读出来。
    fn read_thumbs(&mut self) {
        let Some((tv, season)) = self.detail_tmdb() else {
            return;
        };
        let Some((rows, _)) = self.detail_rows() else {
            return;
        };
        let found: HashMap<u32, PathBuf> = rows
            .iter()
            .filter_map(|row| row.number)
            .filter_map(|number| Some((number, self.stills.cached_thumb(tv, season, number)?)))
            .collect();
        if let Some(detail) = self.detail.as_mut() {
            detail.thumbs = found;
        }
    }

    /// 背景的剧照换成选中那一集的：缓存里有的先用；`fetch` 为真时在后台把这一集缺的剧照和各集的
    /// 缩略图取回来。后台取完要唤醒引擎，所以只在命令里取
    /// （[`bern::wake::waker`] 只在命令执行时拿得到）。
    fn want_images(&mut self, fetch: bool) {
        let Some((key, tv, season, episode)) = self.wanted_stills() else {
            return;
        };
        let Some(detail) = self.detail.as_mut() else {
            return;
        };
        if detail.stills_for.as_deref() != Some(key.as_str()) {
            let cached = self.stills.cached(tv, season, episode);
            if !cached.is_empty() {
                detail.stills = cached;
                detail.still_at = 0;
            }
        } else if detail.thumbs_wanted {
            return;
        }
        if !fetch {
            return;
        }
        detail.stills_for = Some(key);
        detail.thumbs_wanted = true;
        let Some(found) = detail.info.as_ref().and_then(|info| info.details.clone()) else {
            return;
        };
        let stills = Arc::clone(&self.stills);
        self.in_background_many(move |send| fetch_images(&stills, &found, episode, send));
    }

    /// 这部番的背景设置。
    fn backdrop_choice(&self) -> library::BackdropChoice {
        self.detail
            .as_ref()
            .and_then(|detail| self.library.backdrops.get(&detail.key))
            .cloned()
            .unwrap_or_default()
    }

    /// 背景显示哪一张剧照：固定的那一张，没固定时是轮到的那一张。
    fn shown_still(&self) -> Option<PathBuf> {
        let detail = self.detail.as_ref()?;
        let choice = self.backdrop_choice();
        let fixed = choice
            .still
            .filter(|_| choice.fixed)
            .and_then(|name| self.stills.named(&name));
        fixed.or_else(|| {
            detail
                .stills
                .get(detail.still_at % detail.stills.len().max(1))
                .cloned()
        })
    }

    /// 把详情页放进界面状态；没打开时清空。
    fn publish_detail(&mut self) {
        let Some((rows, summary)) = self.detail_rows() else {
            self.close_detail_state();
            return;
        };
        let choice = self.backdrop_choice();
        let still = self.shown_still();
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
        state.detail_known = found.is_some();
        state.detail_loading = detail.loading;
        state.detail_error = detail.error.clone();
        state.detail_native_title = found.map(details::native_title).unwrap_or_default();
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
            .map(|found| details::tags(found, 4))
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
        state.detail_extra_count = summary.extra as i64;
        state.detail_episode_count = found.map_or(0, |found| details::episode_count(found) as i64);
        state.detail_minutes = details::minutes(&rows).unwrap_or(0);
        let next = summary.next.map(|at| &rows[at]);
        let resume = next.and_then(|row| row.url.clone());
        state.detail_can_resume = resume.is_some();
        state.detail_resume = resume.map_or_else(no_source, MediaSource::new);
        state.detail_started = summary.last.is_some();
        state.detail_resume_fraction = next
            .and_then(|row| row.watched)
            .map_or(0.0, |watched| watched.percent() as f64 / 100.0);
        state.detail_group = group_of(detail.group);
        state.detail_episodes = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.group == detail.group)
            .map(|(at, row)| {
                let thumb = row.number.and_then(|number| detail.thumbs.get(&number));
                DetailEpisode {
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
                    thumb: thumb.map_or_else(no_source, MediaSource::file),
                    has_thumb: thumb.is_some(),
                }
            })
            .collect();

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

        // 背景：有剧照用剧照（清晰），没有用海报（虚化）。换了图编号才变，界面淡入淡出。
        let (url, is_still) = match still {
            Some(path) => (MediaSource::file(path).url, true),
            None => (poster_url.unwrap_or_default(), false),
        };
        state.detail_fixed = choice.fixed;
        state.detail_can_switch = detail.stills.len() > 1;
        state.detail_rotates = !choice.fixed && detail.stills.len() > 1;
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
        state.detail_native_title.clear();
        state.detail_poster = no_source();
        state.detail_has_poster = false;
        state.detail_backdrop.clear();
        state.detail_rotates = false;
        state.detail_fixed = false;
        state.detail_can_switch = false;
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
                let black = picture::is_black(&path);
                send(Arrival::Shot(url, path, black));
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
            let shown = self.state.recents.iter_mut().chain(&mut self.state.hero);
            for recent in shown.filter(|recent| recent.source.url == url) {
                recent.percent = percent;
                recent.bar = bar_of(percent);
            }
            // 选中的那一集可能因此变了：背景跟着换。
            self.want_images(true);
            self.publish_detail();
        }
    }

    fn save_library(&mut self) {
        self.store.save(&self.library);
        self.publish_library();
    }

    /// 把新番列表里 `shin_day` 那天更新的番放进界面状态。
    fn publish_shin(&mut self) {
        self.state.shin_shown = self.shin_on(&self.state.shin_day);
        self.state.shin_count = self.state.shin_shown.len() as i64;
        self.state.shin_days = (0..7)
            .map(|at| DayChip {
                id: i64::from(at) + 201,
                day: weekday(at),
                on: weekday(at) == self.state.shin_day,
            })
            .collect();
    }

    fn take_season(&mut self, season: Season) {
        self.state.shin = ShinStatus::Loaded;
        self.state.shin_outdated = season.id < self.today.season();
        let (year, month) = season.id.split_at_checked(4).unwrap_or(("", ""));
        self.state.shin_year = year.to_string();
        self.state.shin_month = month.parse().unwrap_or(1);
        self.state.shin_error.clear();
        self.season = Some(season);
        self.publish_shin();
        self.publish_today();
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

    /// 准备新番的播出时间：先用缓存的；没有或旧了，在后台重新取。
    fn load_airtimes(&mut self) {
        let cached = self.airtimes_source.cached();
        let fresh = cached.as_ref().is_some_and(|table| table.fresh(unix_now()));
        if let Some(table) = cached {
            self.airtimes = table;
            self.publish_shin();
            self.publish_today();
        }
        if fresh || !self.airtimes_source.available() {
            return;
        }
        let source = Arc::clone(&self.airtimes_source);
        self.in_background_many(move |send| {
            // 取不到就不显示时间，下次启动再试。
            if let Ok(table) = source.download(unix_now()) {
                send(Arrival::Airtimes(table));
            }
        });
    }

    /// 在后台看以前存下的画面是不是几乎全黑（以前的版本没看）。
    fn look_at_shots(&mut self) {
        let unknown: Vec<PathBuf> = self
            .library
            .recents
            .iter()
            .filter(|recent| recent.shot_black.is_none())
            .filter_map(|recent| recent.shot.clone())
            .collect();
        if unknown.is_empty() {
            return;
        }
        self.in_background(move || {
            let looks = unknown.into_iter().map(|path| {
                let black = picture::is_black(&path);
                (path, black)
            });
            Arrival::ShotLooks(looks.collect())
        });
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
        self.begin_play(&file.url);
        // 看到一半的接着播：在界面让播放器打开之前告诉播放内核从哪里开始。
        match self.library.progress.get(&file.url) {
            Some(watched)
                if self.settings.resume
                    && !watched.finished()
                    && watched.position_ms > RESUME_FROM =>
            {
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
        self.state.next_shown = false;
        self.state.skip_shown = false;
        self.state.match_open = false;
        if self.state.fast {
            self.set_fast_to(false);
        }
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
        self.state.volume_percent = (self.state.volume * 100.0).round() as i64;
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
        self.show_detail(key, anime_id, (title, poster));
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
        self.show_detail(key, Some(anime_id), known.unwrap_or_default());
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

    fn show_group(&mut self, group: DetailGroup) -> Result<(), CommandError> {
        if let Some(detail) = self.detail.as_mut() {
            detail.group = match group {
                DetailGroup::Main => details::Group::Main,
                DetailGroup::Special => details::Group::Special,
                DetailGroup::Extra => details::Group::Extra,
            };
        }
        self.reset_strip();
        self.publish_detail();
        Ok(())
    }

    fn page_episodes(&mut self, at: i64, pages: i64) -> Result<(), CommandError> {
        let ids: Vec<i64> = self.state.detail_episodes.iter().map(|e| e.id).collect();
        if ids.is_empty() {
            return Ok(());
        }
        let from = ids.iter().position(|id| *id == at).unwrap_or(0) as i64;
        let to = from
            .saturating_add(pages.saturating_mul(STRIP_PAGE))
            .clamp(0, last_page(ids.len()) as i64);
        self.state.detail_strip_at = ids[to as usize];
        self.state.detail_strip_serial += 1;
        if let Some(detail) = self.detail.as_mut() {
            detail.strip_at = ids[to as usize];
        }
        Ok(())
    }

    fn mark_watched(&mut self) -> Result<(), CommandError> {
        let url = self
            .detail_rows()
            .and_then(|(rows, summary)| rows[summary.next?].url.clone());
        if let Some(url) = url {
            self.library.finish(&url, unix_now());
            self.want_images(true);
            self.save_library();
        }
        Ok(())
    }

    fn set_backdrop_fixed(&mut self, fixed: bool) -> Result<(), CommandError> {
        // 固定住的是现在看到的这一张。
        let still = self.shown_still();
        let name = still.and_then(|path| Some(path.file_name()?.to_str()?.to_string()));
        if let Some(detail) = &self.detail {
            let choice = library::BackdropChoice { fixed, still: name };
            self.library.backdrops.insert(detail.key.clone(), choice);
            self.store.save(&self.library);
        }
        self.publish_detail();
        Ok(())
    }

    fn load_home(&mut self) -> Result<(), CommandError> {
        if self.home_loaded {
            return Ok(());
        }
        self.home_loaded = true;
        self.load_airtimes();
        self.look_at_shots();
        if !self.stills.available() {
            return Ok(());
        }
        // 详情已经在缓存里的番，取它最近看的那一集的一张剧照；不为此向弹弹play 发请求。
        let wanted: Vec<(i64, u32, u32)> = self
            .library
            .watching()
            .into_iter()
            .filter_map(|recent| {
                let episode = first_number(&recent.episode)?;
                let (tv, season) = self.tmdb_of(self.library.anime_of(recent)?)?;
                Some((tv, season, episode))
            })
            .collect();
        if wanted.is_empty() {
            return Ok(());
        }
        let stills = Arc::clone(&self.stills);
        self.in_background(move || {
            for (tv, season, episode) in wanted {
                let _ = stills.fetch(tv, season, episode, 1);
            }
            Arrival::HomeStills
        });
        Ok(())
    }

    fn show_hero(&mut self, id: i64) -> Result<(), CommandError> {
        if let Some(recent) = self.state.recents.iter().find(|recent| recent.id == id) {
            self.hero_url = Some(recent.source.url.clone());
            self.publish_hero();
        }
        Ok(())
    }

    fn step_hero(&mut self, step: i64) -> Result<(), CommandError> {
        let count = self.state.recents.len() as i64;
        if count > 0 {
            let at = self.state.hero.first().map_or(0, |hero| hero.id - 1);
            self.show_hero(at.saturating_add(step).rem_euclid(count) + 1)?;
        }
        Ok(())
    }

    fn clear_progress(&mut self) -> Result<(), CommandError> {
        let Some((rows, _)) = self.detail_rows() else {
            return Ok(());
        };
        self.library
            .forget(rows.iter().filter_map(|row| row.url.as_deref()));
        self.want_images(true);
        self.save_library();
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
        let shown = self.shown_still();
        let fixed = self.backdrop_choice().fixed;
        if let Some(detail) = self.detail.as_mut()
            && !detail.stills.is_empty()
        {
            // 从现在看到的这一张往后数；它不在这一集里（固定的是别的集）就从头开始。
            let at = detail
                .stills
                .iter()
                .position(|path| Some(path) == shown.as_ref());
            detail.still_at = at.map_or(0, |at| (at + 1) % detail.stills.len());
            if fixed {
                let name = detail.stills[detail.still_at]
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string);
                let choice = library::BackdropChoice { fixed, still: name };
                self.library.backdrops.insert(detail.key.clone(), choice);
                self.store.save(&self.library);
            }
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

    fn set_appearance(&mut self, appearance: Appearance) -> Result<(), CommandError> {
        self.edit_settings(|s| s.appearance = settings::appearance_name(&appearance).into());
        Ok(())
    }

    fn set_start_page(&mut self, page: Page) -> Result<(), CommandError> {
        self.edit_settings(|s| s.start_page = settings::page_name(&page).into());
        Ok(())
    }

    fn show_page(&mut self, page: Page, on: bool) -> Result<(), CommandError> {
        self.edit_settings(|s| s.show_page(&page, on));
        Ok(())
    }

    fn move_page(&mut self, from: i64, to: i64) -> Result<(), CommandError> {
        self.edit_settings(|s| s.move_page(from, to));
        Ok(())
    }

    fn set_toggle(&mut self, toggle: Toggle, on: bool) -> Result<(), CommandError> {
        self.edit_settings(|s| *s.toggle(&toggle) = on);
        Ok(())
    }

    fn set_amount(&mut self, amount: Amount, value: f64) -> Result<(), CommandError> {
        if !value.is_finite() {
            return Err(CommandError::new(
                "invalid_value",
                "the value must be a number",
            ));
        }
        self.edit_settings(|s| s.set_amount(&amount, value));
        Ok(())
    }

    fn set_seek_step(&mut self, step: SeekStep) -> Result<(), CommandError> {
        let seconds = match step {
            SeekStep::One => 1,
            SeekStep::Five => 5,
            SeekStep::Ten => 10,
            SeekStep::Fifteen => 15,
            SeekStep::Thirty => 30,
        };
        self.edit_settings(|s| s.seek_step = seconds);
        Ok(())
    }

    fn set_hold_rate(&mut self, rate: HoldRate) -> Result<(), CommandError> {
        let tenths = match rate {
            HoldRate::OneAndHalf => 15,
            HoldRate::Two => 20,
            HoldRate::Three => 30,
            HoldRate::Four => 40,
        };
        self.edit_settings(|s| s.hold_speed = tenths);
        Ok(())
    }

    fn set_bar_stay(&mut self, stay: BarStay) -> Result<(), CommandError> {
        let seconds = match stay {
            BarStay::One => 1,
            BarStay::Two => 2,
            BarStay::Three => 3,
            BarStay::Five => 5,
        };
        self.edit_settings(|s| s.bar_stay = seconds);
        Ok(())
    }

    fn set_after_end(&mut self, after: AfterEnd) -> Result<(), CommandError> {
        self.edit_settings(|s| s.after_end = settings::after_end_name(&after).into());
        Ok(())
    }

    fn set_skip_mode(&mut self, mode: SkipMode) -> Result<(), CommandError> {
        self.edit_settings(|s| s.skip_mode = settings::skip_mode_name(&mode).into());
        Ok(())
    }

    fn set_upscale(&mut self, upscale: Upscale) -> Result<(), CommandError> {
        self.edit_settings(|s| s.upscale = settings::upscale_name(&upscale).into());
        Ok(())
    }

    fn set_aspect(&mut self, aspect: Aspect) -> Result<(), CommandError> {
        self.edit_settings(|s| s.aspect = settings::aspect_name(&aspect).into());
        Ok(())
    }

    fn set_fill(&mut self, fill: Fill) -> Result<(), CommandError> {
        self.edit_settings(|s| s.fill = settings::fill_name(&fill).into());
        Ok(())
    }

    fn set_danmaku_font(&mut self, font: DanmakuFont) -> Result<(), CommandError> {
        self.edit_settings(|s| s.d_font = settings::font_name(&font).into());
        Ok(())
    }

    fn add_blocked(&mut self, text: String) -> Result<(), CommandError> {
        if self.settings.add_blocked(&text) {
            self.settings_changed();
        }
        Ok(())
    }

    fn remove_blocked(&mut self, id: i64) -> Result<(), CommandError> {
        if (0..self.settings.blocked.len() as i64).contains(&id) {
            self.edit_settings(|s| {
                s.blocked.remove(id as usize);
            });
        }
        Ok(())
    }

    fn set_key(&mut self, action: KeyAction, keys: String) -> Result<(), CommandError> {
        let keys = if keys.trim().is_empty() {
            String::new()
        } else {
            bern::shortcut::parse_shortcut(&keys)
                .map_err(|reason| CommandError::new("invalid_shortcut", reason.to_string()))?
                .to_string()
        };
        self.edit_settings(|s| s.set_key(&action, &keys));
        Ok(())
    }

    fn reset_section(&mut self, section: Section) -> Result<(), CommandError> {
        self.edit_settings(|s| s.reset(&section));
        Ok(())
    }

    fn open_link(&mut self, link: Link) -> Result<(), CommandError> {
        self.open_link_at(&link)
    }

    fn set_speed(&mut self, speed: f64) -> Result<(), CommandError> {
        self.set_speed_to(speed)
    }

    fn nudge_speed(&mut self, step: f64) -> Result<(), CommandError> {
        self.set_speed_to(self.state.speed + step)
    }

    fn set_fast(&mut self, fast: bool) -> Result<(), CommandError> {
        self.set_fast_to(fast);
        Ok(())
    }

    fn tick(&mut self, position_ms: i64, duration_ms: Option<i64>) -> Result<(), CommandError> {
        self.tick_at(position_ms, duration_ms);
        Ok(())
    }

    fn skip_part(&mut self) -> Result<(), CommandError> {
        self.skip_part_now();
        Ok(())
    }

    fn skip_ahead(
        &mut self,
        position_ms: i64,
        duration_ms: Option<i64>,
    ) -> Result<(), CommandError> {
        let seconds = self.settings.skip_seconds;
        self.skip_at(seconds, position_ms, duration_ms, Instant::now())
    }

    fn play_previous(&mut self) -> Result<(), CommandError> {
        self.play_offset(-1)
    }

    fn play_next(&mut self) -> Result<(), CommandError> {
        self.play_offset(1)
    }

    fn cancel_next(&mut self) -> Result<(), CommandError> {
        self.cancel_next_now();
        Ok(())
    }

    fn play_episode(&mut self, id: i64) -> Result<(), CommandError> {
        self.play_row(id)
    }

    fn cycle_subtitle(&mut self) -> Result<(), CommandError> {
        nkuplay_mpv::command(&["cycle", "sub"]);
        self.load_info_now();
        Ok(())
    }

    fn cycle_audio(&mut self) -> Result<(), CommandError> {
        nkuplay_mpv::command(&["cycle", "audio"]);
        Ok(())
    }

    fn add_subtitle(&mut self, file: MediaSource) -> Result<(), CommandError> {
        let location = nkuplay_mpv::location(&file.url);
        nkuplay_mpv::command(&["sub-add", &location, "select"]);
        // 字幕页的列表跟着加上这一条。
        self.load_info_now();
        Ok(())
    }

    fn select_subtitle(&mut self, id: i64) -> Result<(), CommandError> {
        nkuplay_mpv::select_subtitle(Some(id));
        self.load_info_now();
        Ok(())
    }

    fn load_info(&mut self) -> Result<(), CommandError> {
        self.load_info_now();
        Ok(())
    }

    fn list_danmaku(&mut self, position_ms: i64, query: String) -> Result<(), CommandError> {
        self.list_danmaku_at(position_ms, &query);
        Ok(())
    }

    fn block_line(&mut self, id: i64) -> Result<(), CommandError> {
        self.block_line_now(id);
        Ok(())
    }

    fn nudge_shift(&mut self, step: f64) -> Result<(), CommandError> {
        self.nudge_shift_by(step);
        Ok(())
    }

    fn refetch_danmaku(&mut self) -> Result<(), CommandError> {
        self.refetch();
        Ok(())
    }

    fn load_danmaku_file(&mut self, file: MediaSource) -> Result<(), CommandError> {
        self.load_file_comments(&file)
    }

    fn peek(&mut self, _fraction: f64, _duration_ms: Option<i64>) -> Result<(), CommandError> {
        // 悬停预览图还没做：要另开一个播放内核取帧。
        Ok(())
    }

    fn open_match(&mut self) -> Result<(), CommandError> {
        self.open_match_now();
        Ok(())
    }

    fn close_match(&mut self) -> Result<(), CommandError> {
        self.state.match_open = false;
        Ok(())
    }

    fn search_match(&mut self, query: String) -> Result<(), CommandError> {
        self.search_match_for(&query);
        Ok(())
    }

    fn choose_anime(&mut self, id: i64) -> Result<(), CommandError> {
        self.choose_anime_now(id);
        Ok(())
    }

    fn choose_episode(&mut self, id: i64) -> Result<(), CommandError> {
        self.choose_episode_now(id);
        Ok(())
    }

    fn set_match_folder(&mut self, on: bool) -> Result<(), CommandError> {
        self.state.match_folder = on;
        Ok(())
    }

    fn confirm_match(&mut self) -> Result<(), CommandError> {
        self.confirm_match_now();
        Ok(())
    }

    fn skip_danmaku(&mut self) -> Result<(), CommandError> {
        self.skip_danmaku_now();
        Ok(())
    }

    fn seek_steps(
        &mut self,
        steps: i64,
        position_ms: i64,
        duration_ms: Option<i64>,
    ) -> Result<(), CommandError> {
        let seconds = steps as f64 * self.settings.seek_step as f64;
        self.skip_at(seconds, position_ms, duration_ms, Instant::now())
    }

    fn fullscreen_done(&mut self) -> Result<(), CommandError> {
        self.state.fullscreen_pending = false;
        Ok(())
    }

    fn set_library_filter(&mut self, filter: LibraryFilter) -> Result<(), CommandError> {
        self.state.library_filter = filter;
        self.publish_library_page();
        Ok(())
    }

    fn set_library_sort(&mut self, sort: LibrarySort) -> Result<(), CommandError> {
        self.edit_settings(|s| s.library_sort = settings::library_sort_name(&sort).into());
        self.publish_library_page();
        Ok(())
    }

    fn resume_series(&mut self, id: i64) -> Result<(), CommandError> {
        let url = self.resume_url(id)?;
        <Self as nkuplay::Commands>::open_file(self, MediaSource::new(&url))
    }

    fn finish_series(&mut self, id: i64) -> Result<(), CommandError> {
        self.finish_series_at(id)
    }

    fn hide_series(&mut self, id: i64) -> Result<(), CommandError> {
        self.hide_series_at(id)
    }

    fn reidentify_series(&mut self, id: i64) -> Result<(), CommandError> {
        self.reidentify_series_at(id)?;
        <Self as nkuplay::Commands>::refresh_library(self)
    }

    fn reveal_series(&mut self, id: i64) -> Result<(), CommandError> {
        self.reveal_series_at(id)
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
                Arrival::Media(generation, info) => self.take_media(generation, info),
                Arrival::Searched(generation, found) => self.take_search(generation, found),
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
                    // 这里拿不到唤醒器，只看缓存；缺的图由取资料的那件后台的事顺便取。
                    self.forget_looked_up();
                    self.read_thumbs();
                    self.want_images(false);
                    self.reset_strip();
                    self.publish_detail();
                }
                Arrival::Shot(url, path, black) => {
                    self.library.shot(&url, path, black);
                    self.save_library();
                }
                Arrival::ShotLooks(looks) => {
                    for recent in &mut self.library.recents {
                        let look = looks
                            .iter()
                            .find(|(path, _)| Some(path) == recent.shot.as_ref());
                        if let Some((_, black)) = look {
                            recent.shot_black = Some(*black);
                        }
                    }
                    self.save_library();
                }
                Arrival::Airtimes(table) => {
                    self.airtimes = table;
                    self.publish_shin();
                    self.publish_today();
                }
                Arrival::Thumb(key, episode, path) => {
                    let wanted = self.detail_tmdb();
                    if let (Some(detail), Some((tv, season))) = (self.detail.as_mut(), wanted)
                        && key == stills_key(tv, season, episode)
                    {
                        detail.thumbs.insert(episode, path);
                        self.publish_detail();
                    }
                }
                Arrival::PlayImages(generation) => {
                    if generation == self.generation && self.state.has_video {
                        self.publish_play();
                    }
                }
                Arrival::HomeStills => {
                    self.forget_looked_up();
                    self.publish_library();
                }
                Arrival::Stills(key, found) => {
                    self.forget_looked_up();
                    let wanted = self.wanted_stills().map(|(wanted, ..)| wanted);
                    if let Some(detail) = self.detail.as_mut()
                        && wanted.as_deref() == Some(key.as_str())
                        && !found.is_empty()
                    {
                        // 正在看的那一张还在，就接着看它。
                        let shown = detail.stills.get(detail.still_at).cloned();
                        detail.still_at = shown
                            .and_then(|shown| found.iter().position(|path| *path == shown))
                            .unwrap_or(0);
                        detail.stills_for = Some(key);
                        detail.stills = found;
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
            airtimes: airtimes::Source::new(None, None),
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
        assert!(engine.state.detail_can_resume);
        assert!(!engine.state.detail_started);
        let first = engine.state.detail_resume.clone();
        assert_eq!(first, engine.state.detail_episodes[0].source);
        // 播放一集，回来时详情页还在。
        engine.open_file(first).unwrap();
        engine.close().unwrap();
        assert!(engine.state.detail_open);
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
        assert_eq!(state.detail_tags.len(), 4);
        assert_eq!(
            (
                state.detail_main_count,
                state.detail_special_count,
                state.detail_extra_count
            ),
            (3, 1, 1)
        );
        assert!(!state.detail_can_resume);
        assert_eq!(state.detail_episodes.len(), 3);
        assert_eq!(state.detail_native_title, "さよならララ");
        assert_eq!(state.detail_episode_count, 3);
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
    fn episodes_are_shown_one_group_at_a_time_and_the_strip_pages() {
        let mut engine = engine_with_details(details::tests::FakeApi::lara());
        engine.open_anime(18643).unwrap();
        engine.wake();
        let codes = |engine: &NkuplayEngine| -> Vec<String> {
            let shown = engine.state.detail_episodes.iter();
            shown.map(|e| e.code.clone()).collect()
        };
        assert_eq!(engine.state.detail_group, DetailGroup::Main);
        engine.show_group(DetailGroup::Special).unwrap();
        assert_eq!(codes(&engine), ["S1"]);
        engine.show_group(DetailGroup::Extra).unwrap();
        assert_eq!(codes(&engine), ["C1"]);
        engine.show_group(DetailGroup::Main).unwrap();
        assert_eq!(codes(&engine), ["", "", ""]);

        // 翻页落在两头；每翻一次序号都变，界面才会滚。
        let ids: Vec<i64> = engine.state.detail_episodes.iter().map(|e| e.id).collect();
        let before = engine.state.detail_strip_serial;
        engine.page_episodes(ids[0], 1).unwrap();
        assert_eq!(engine.state.detail_strip_at, ids[0]);
        engine.page_episodes(ids[2], -5).unwrap();
        assert_eq!(engine.state.detail_strip_at, ids[0]);
        // 最后一屏也露出一整屏。
        assert_eq!((last_page(3), last_page(12)), (0, 8));
        assert_eq!(engine.state.detail_strip_serial, before + 2);
        // 关着的时候什么也不做。
        engine.close_details().unwrap();
        engine.page_episodes(1, 1).unwrap();
    }

    #[test]
    fn marking_watched_moves_continue_to_the_next_episode() {
        let anime = shows("shelf-mark");
        let mut engine = engine(None, at_once);
        engine.add_folder(MediaSource::file(&anime.0)).unwrap();
        engine.wake();
        engine.open_series(2).unwrap();
        let second = engine.state.detail_episodes[1].source.clone();
        engine.mark_watched().unwrap();
        assert!(engine.state.detail_episodes[0].finished);
        assert_eq!(engine.state.detail_resume, second);
        engine.mark_watched().unwrap();
        assert!(engine.state.detail_episodes[1].finished);
    }

    /// 详情来自假弹弹play、剧照来自假 TMDB 的引擎。
    fn engine_with_stills(data: Option<PathBuf>, cache: PathBuf) -> NkuplayEngine {
        NkuplayEngine::new(
            None,
            Sources {
                details: details::Source::new(
                    Some(Box::new(details::tests::FakeApi::lara())),
                    Some(cache.join("bangumi")),
                ),
                stills: tmdb::Stills::new(
                    Some(Box::new(tmdb::tests::FakeApi::with(&[
                        "/a.jpg", "/b.jpg", "/c.jpg",
                    ]))),
                    Some(cache.join("stills")),
                ),
                ..no_sources()
            },
            library::Store::new(data),
            today(),
            at_once,
        )
    }

    #[test]
    fn the_backdrop_rotates_until_fixed_and_the_choice_is_remembered() {
        let data = Scratch::named("backdrop-data");
        let cache = Scratch::named("backdrop-cache");
        let start = || engine_with_stills(Some(data.0.clone()), cache.0.clone());
        let shown = |engine: &NkuplayEngine| engine.state.detail_backdrop[0].source.url.clone();

        let mut engine = start();
        engine.open_anime(18643).unwrap();
        engine.wake();
        assert!(engine.state.detail_rotates && !engine.state.detail_fixed);
        assert!(engine.state.detail_can_switch);
        assert!(engine.state.detail_backdrop[0].still);
        assert!(shown(&engine).ends_with("a.jpg"));
        // 每集的缩略图是它的第一张剧照，不跟着轮换。
        assert!(engine.state.detail_episodes.iter().all(|e| e.has_thumb));
        let thumb = engine.state.detail_episodes[0].thumb.clone();
        engine.next_backdrop().unwrap();
        assert!(shown(&engine).ends_with("b.jpg"));
        assert_eq!(engine.state.detail_episodes[0].thumb, thumb);

        // 固定住现在这一张；再切换就固定到下一张。
        engine.set_backdrop_fixed(true).unwrap();
        assert!(engine.state.detail_fixed && !engine.state.detail_rotates);
        assert!(shown(&engine).ends_with("b.jpg"));
        engine.next_backdrop().unwrap();
        assert!(shown(&engine).ends_with("c.jpg"));

        // 下次打开还是固定的那一张；别的番照旧轮换。
        let mut engine = start();
        engine.open_anime(18643).unwrap();
        engine.wake();
        assert!(engine.state.detail_fixed);
        assert!(shown(&engine).ends_with("c.jpg"));
        engine.set_backdrop_fixed(false).unwrap();
        assert!(engine.state.detail_rotates);
        engine.open_anime(1).unwrap();
        assert!(!engine.state.detail_fixed);
    }

    #[test]
    fn the_home_page_pages_through_the_shows_being_watched() {
        let mut engine = engine(None, at_once);
        assert!(engine.state.hero.is_empty());
        engine.step_hero(1).unwrap();
        for file in ["C:/a.mkv", "C:/b.mkv", "C:/c.mkv"] {
            engine.open_file(MediaSource::file(file)).unwrap();
        }
        assert_eq!(engine.state.hero_dots.len(), 3);
        let at = |engine: &NkuplayEngine| engine.state.hero[0].id;
        engine.show_hero(1).unwrap();
        engine.step_hero(1).unwrap();
        assert_eq!(at(&engine), 2);
        engine.step_hero(2).unwrap();
        assert_eq!(at(&engine), 1);
        engine.step_hero(-1).unwrap();
        assert_eq!(at(&engine), 3);
        assert!(engine.state.hero_dots[2].on);
        // 没有的编号什么也不做。
        engine.show_hero(9).unwrap();
        assert_eq!(at(&engine), 3);
    }

    #[test]
    fn the_home_picture_uses_the_still_of_the_episode_when_it_is_cached() {
        let cache = Scratch::named("home-stills");
        let mut engine = engine_with_stills(None, cache.0.clone());
        engine.open_file(MediaSource::file("C:/lara.mkv")).unwrap();
        let url = engine.state.recents[0].source.url.clone();
        engine
            .library
            .identified(&url, 18643, "再见，拉拉", "第2话 奔走于滋贺", None);
        engine.save_library();
        // 详情还没缓存过：不为大图向弹弹play 发请求，也就没有剧照。
        engine.load_home().unwrap();
        assert!(!engine.state.recents[0].has_backdrop);
        // 打开过详情页以后就有了。
        engine.open_anime(18643).unwrap();
        engine.wake();
        engine.home_loaded = false;
        engine.load_home().unwrap();
        engine.wake();
        let recent = &engine.state.recents[0];
        assert!(recent.sharp && recent.backdrop.url.ends_with("a.jpg"));
        assert_eq!(engine.state.hero[0].backdrop, recent.backdrop);
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
        assert_eq!(engine.state.detail_resume_fraction, 0.5);
        assert_eq!(engine.state.detail_minutes, 24);
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
        // 今天（周一）更新的，不跟着选的那一天变。
        assert_eq!(engine.state.today_count, 1);
        assert_eq!(engine.state.today[0].title, "周一的番");
        assert!(engine.state.shin_shown[0].has_poster);

        engine.show_day(Weekday::Sunday).unwrap();
        assert_eq!(titles(&engine), ["周日的番"]);
        engine.show_day(Weekday::Tuesday).unwrap();
        assert!(engine.state.shin_shown.is_empty());
    }

    #[test]
    fn the_air_time_comes_with_the_home_page_and_orders_the_day() {
        let mut list = shin::tests::FakeApi::season();
        list.list
            .insert(0, shin::tests::intro(5, "没有时间的番", 1));
        let mut engine = NkuplayEngine::new(
            None,
            Sources {
                shin: shin::Source::new(Some(Box::new(list)), None),
                airtimes: airtimes::Source::new(
                    Some(Box::new(airtimes::tests::FakeApi::default())),
                    None,
                ),
                ..no_sources()
            },
            library::Store::new(None),
            today(),
            at_once,
        );
        engine.load_shin().unwrap();
        engine.load_home().unwrap();
        engine.wake();
        let times: Vec<_> = engine
            .state
            .today
            .iter()
            .map(|anime| (anime.title.as_str(), anime.time.as_str()))
            .collect();
        assert_eq!(times, [("周一的番", "23:30"), ("没有时间的番", "")]);
        assert_eq!(engine.state.shin_year, "2026");
        assert_eq!(engine.state.shin_month, 10);
        let on: Vec<_> = engine.state.shin_days.iter().map(|day| day.on).collect();
        assert_eq!(on, [true, false, false, false, false, false, false]);
    }

    #[test]
    fn cards_show_the_part_and_a_short_bar_from_the_start() {
        assert_eq!(part_of("第5话 救救我"), "第5话");
        assert_eq!(part_of("特别篇"), "");
        assert_eq!(part_of(""), "");
        assert_eq!((bar_of(0), bar_of(38), bar_of(100)), (0.02, 0.38, 1.0));
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
