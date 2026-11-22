//! Nkuplay 的协议：界面显示的状态，以及改变它的命令。
//! 界面、命令行和 MCP 服务器都通过这些命令操作。

#[bern::protocol(namespace = "nkuplay", version = 1)]
pub mod nkuplay {
    use bern::prelude::*;

    /// 当前视频的弹幕处于什么阶段。
    #[value]
    pub enum DanmakuStatus {
        /// 没有打开视频。
        #[label(tr! { "en" => "Off", "zh-Hans" => "未开启" })]
        Off,
        /// 这个构建没有弹弹play 的凭证，弹幕功能不可用。
        #[label(tr! { "en" => "Not configured", "zh-Hans" => "未配置" })]
        Unavailable,
        /// 正在识别文件并获取弹幕。
        #[label(tr! { "en" => "Looking up…", "zh-Hans" => "正在识别…" })]
        Matching,
        /// 已识别出是哪一集，弹幕已取回。
        #[label(tr! { "en" => "Loaded", "zh-Hans" => "已加载" })]
        Loaded,
        /// 弹弹play 认不出这个文件。
        #[label(tr! { "en" => "No match", "zh-Hans" => "未识别" })]
        NoMatch,
        /// 识别或获取弹幕时出错，原因在 `danmaku_error` 里。
        #[label(tr! { "en" => "Failed", "zh-Hans" => "获取失败" })]
        Failed,
    }

    /// 一周中的一天，用来按更新日筛选新番。
    #[value]
    pub enum Weekday {
        #[label(tr! { "en" => "Mon", "zh-Hans" => "周一" })]
        Monday,
        #[label(tr! { "en" => "Tue", "zh-Hans" => "周二" })]
        Tuesday,
        #[label(tr! { "en" => "Wed", "zh-Hans" => "周三" })]
        Wednesday,
        #[label(tr! { "en" => "Thu", "zh-Hans" => "周四" })]
        Thursday,
        #[label(tr! { "en" => "Fri", "zh-Hans" => "周五" })]
        Friday,
        #[label(tr! { "en" => "Sat", "zh-Hans" => "周六" })]
        Saturday,
        #[label(tr! { "en" => "Sun", "zh-Hans" => "周日" })]
        Sunday,
    }

    /// 本季新番列表处于什么阶段。列表来自弹弹play，存在硬盘上：
    /// 第一次打开首页时取一次，之后只读缓存，用户点“刷新”才重新取。
    #[value]
    pub enum ShinStatus {
        /// 首页还没打开过，没有读缓存也没有取。
        #[label(tr! { "en" => "Not loaded", "zh-Hans" => "未获取" })]
        NotLoaded,
        /// 这个构建没有弹弹play 的凭证，又没有缓存。
        #[label(tr! { "en" => "Not configured", "zh-Hans" => "未配置" })]
        Unavailable,
        #[label(tr! { "en" => "Loading…", "zh-Hans" => "正在获取…" })]
        Loading,
        #[label(tr! { "en" => "Loaded", "zh-Hans" => "已获取" })]
        Loaded,
        /// 获取失败，原因在 `shin_error` 里。
        #[label(tr! { "en" => "Failed", "zh-Hans" => "获取失败" })]
        Failed,
    }

    /// 详情页剧集的分组。
    #[value]
    pub enum DetailGroup {
        #[label(tr! { "en" => "Episodes", "zh-Hans" => "正片" })]
        Main,
        #[label(tr! { "en" => "Specials", "zh-Hans" => "特别篇" })]
        Special,
        #[label(tr! { "en" => "OP / ED", "zh-Hans" => "OP / ED" })]
        Extra,
    }

    /// 继续播放里的一部番（最近看的那一集），或没认出的一个视频。
    #[value]
    pub struct Recent {
        pub id: i64,
        /// 作品名；弹弹play 认出之前是文件名。
        #[content]
        pub title: String,
        /// 剧集名；没认出时为空。
        #[content]
        pub episode: String,
        pub source: MediaSource,
        /// 作品的海报（弹弹play 认出后才有）；`has_poster` 为假时没有意义。
        pub poster: MediaSource,
        pub has_poster: bool,
        /// 卡片上的画面：上次离开时的，没有时是这一集的剧照；`has_shot` 为假时没有意义。
        pub shot: MediaSource,
        pub has_shot: bool,
        /// 看了百分之几。
        pub percent: i64,
        /// 卡片上的进度条画多长，0 到 1；刚开始看的也露出一点。
        pub bar: f64,
        /// 剧集名里的“第N话”；没有时为空。
        #[content]
        pub part: String,
        /// 作品类别，例如“TV动画”；不知道时为空。
        #[content]
        pub kind: String,
        /// 弹弹play 的作品编号，有详情页；没认出时为 0。
        pub anime_id: i64,
        /// 首页大图；`has_backdrop` 为假时没有意义。
        pub backdrop: MediaSource,
        pub has_backdrop: bool,
        /// 大图是剧照或离开时的画面（清晰显示）；否则是海报（虚化显示）。
        pub sharp: bool,
    }

    /// 首页大图下面的一个翻页点。
    #[value]
    pub struct HeroDot {
        pub id: i64,
        /// 正在显示这一张。
        pub on: bool,
    }

    /// 首页上一个看不见的记号，用来知道滚到了哪里。
    #[value]
    pub struct HomeMark {
        /// 从 100 起。滚动位置按各项的编号报告，所以和首页别的列表（大图、翻页点是 1 起，
        /// 星期是 201 起）错开。
        pub id: i64,
        /// 到下一个记号有多高。
        pub height: f64,
    }

    /// 本季新番上面的一个星期按钮。
    #[value]
    pub struct DayChip {
        pub id: i64,
        pub day: Weekday,
        /// 正在看这一天。
        pub on: bool,
    }

    /// 本季新番里的一部作品。
    #[value]
    pub struct ShinAnime {
        /// 弹弹play 的作品编号。
        pub id: i64,
        #[content]
        pub title: String,
        /// 海报地址；`has_poster` 为假时没有意义。
        pub poster: MediaSource,
        pub has_poster: bool,
    }

    /// 媒体库里的一个文件夹。
    #[value]
    pub struct Folder {
        pub id: i64,
        #[content]
        pub name: String,
        /// 里面（含子文件夹）的视频文件数。
        pub videos: i64,
        /// 还在扫描这个文件夹。
        pub scanning: bool,
    }

    /// 媒体库海报墙上的一部番：同一作品在各个文件夹里的剧集合在一起。
    #[value]
    pub struct Series {
        pub id: i64,
        /// 弹弹play 认出的作品名；没认出时是从文件名里认出的。
        #[content]
        pub title: String,
        /// 海报；`has_poster` 为假时没有意义。
        pub poster: MediaSource,
        pub has_poster: bool,
        /// 有几集。
        pub episodes: i64,
    }

    /// 详情页剧集列表里的一集：弹弹play 的一集（对上了本地文件或没对上），或本地多出来的文件。
    #[value]
    pub struct DetailEpisode {
        pub id: i64,
        /// 特别篇和 OP/ED 的短编号（`S1`、`C1`）；正片和本地文件为空。
        #[content]
        pub code: String,
        /// 第几集；特别篇和认不出集数的文件为 0。
        pub number: i64,
        /// 剧集标题（不含“第N话”），本地多出来的文件是文件名。
        #[content]
        pub title: String,
        /// 本地有这一集，点了能播放。
        pub local: bool,
        /// 本地文件；`local` 为假时没有意义。
        pub source: MediaSource,
        /// 看了百分之几；没看过为 0。
        pub percent: i64,
        /// 看完了（播到 90% 以上）。
        pub finished: bool,
        /// “继续”指向这一集。
        pub current: bool,
        /// 缩略图（这一集的第一张剧照）；`has_thumb` 为假时没有意义。
        pub thumb: MediaSource,
        pub has_thumb: bool,
    }

    /// 作品的一个标签。
    #[value]
    pub struct DetailTag {
        pub id: i64,
        #[content]
        pub name: String,
    }

    /// 一条短评。
    #[value]
    pub struct DetailComment {
        pub id: i64,
        #[content]
        pub user: String,
        /// 1 到 10；没打分时为 0。
        pub score: i64,
        #[content]
        pub text: String,
    }

    /// 一个网站的评分，例如“Bangumi评分”和“6.4”。
    #[value]
    pub struct DetailScore {
        pub id: i64,
        #[content]
        pub site: String,
        #[content]
        pub score: String,
    }

    /// 制作信息的一条，例如“导演”和“小出卓史”。
    #[value]
    pub struct DetailFact {
        pub id: i64,
        #[content]
        pub key: String,
        #[content]
        pub value: String,
    }

    /// 详情页的背景图。换图时编号变，界面借此淡入淡出。
    #[value]
    pub struct Backdrop {
        pub id: i64,
        pub source: MediaSource,
        /// 是剧照（清晰显示）；否则是海报（虚化显示）。
        pub still: bool,
    }

    #[state]
    pub struct State {
        /// 已经打开了一个视频；在此之前窗口显示首页。
        pub has_video: bool,
        /// 视频视图播放的内容。只在 `has_video` 为真时有意义。
        pub source: MediaSource,
        /// 当前视频的弹幕阶段。
        pub danmaku: DanmakuStatus,
        /// 弹幕已加载：下面的作品、剧集和条数有意义。
        pub danmaku_matched: bool,
        /// 识别出的作品标题。
        #[content]
        pub danmaku_anime: String,
        /// 识别出的剧集标题。
        #[content]
        pub danmaku_episode: String,
        /// 取回的弹幕条数。
        pub danmaku_count: i64,
        /// 弹幕获取失败的原因；没有失败时为空。
        #[content]
        pub danmaku_error: String,
        /// 弹幕显示在画面上；换视频时保持不变。
        pub danmaku_visible: bool,

        /// 每打开一次视频加一。界面看到它变化，就让播放器载入 `source`。
        pub opened: i64,
        /// 每请求一次跳转加一；界面看到它变化，就让播放器跳到 `seek_ms`。
        /// 界面不能做算术，进度条给的比例由引擎换算成毫秒。
        pub seek_serial: i64,
        /// 要跳到的位置，毫秒。
        pub seek_ms: i64,
        /// 想要的音量，0 到 1。界面看到它变化就交给播放器；换视频时保持不变。
        pub volume: f64,

        /// 继续播放：每部番只留最近看的一集，最近的在前。
        pub recents: Vec<Recent>,
        /// `recents` 的个数。
        pub recent_count: i64,
        /// 首页大图正在显示的那一部：没有或一个。
        pub hero: Vec<Recent>,
        /// 大图的翻页点，和 `recents` 一一对应。
        pub hero_dots: Vec<HeroDot>,
        /// 首页大图旁的记号：到了 102，大图就滚出去了。
        pub home_marks: Vec<HomeMark>,

        /// 今天更新的新番，和今天是星期几。
        pub today: Vec<ShinAnime>,
        pub today_count: i64,
        pub today_day: Weekday,

        /// 本季新番列表的阶段。
        pub shin: ShinStatus,
        /// 缓存的新番列表是上一季或更早取的，应当刷新。
        pub shin_outdated: bool,
        /// 列表是哪一年（文字，免得显示成 2,026）、从几月开始的那一季；没有列表时年份为空。
        #[content]
        pub shin_year: String,
        pub shin_month: i64,
        /// 正在看哪一天更新的新番；默认是今天。
        pub shin_day: Weekday,
        /// 周一到周日的按钮。
        pub shin_days: Vec<DayChip>,
        /// `shin_day` 那天更新的新番。
        pub shin_shown: Vec<ShinAnime>,
        /// `shin_shown` 的个数。
        pub shin_count: i64,
        /// 获取新番失败的原因；没有失败时为空。
        #[content]
        pub shin_error: String,

        /// 媒体库里的文件夹，按添加顺序。
        pub folders: Vec<Folder>,
        /// `folders` 的个数。
        pub folder_count: i64,

        /// 媒体库海报墙，按作品名排好。
        pub series: Vec<Series>,
        /// `series` 的个数。
        pub series_count: i64,
        /// 还有文件夹在扫描或识别。
        pub library_scanning: bool,

        /// 番剧详情页打开着：窗口不显示导航，只显示这一页。
        pub detail_open: bool,
        /// 作品名。
        #[content]
        pub detail_title: String,
        /// 日文标题；没有或和作品名一样时为空。
        #[content]
        pub detail_native_title: String,
        pub detail_poster: MediaSource,
        pub detail_has_poster: bool,

        /// 背景图：没有或一张。
        pub detail_backdrop: Vec<Backdrop>,
        /// 背景在轮换（不固定，而且有不止一张剧照）：界面定时换下一张。
        pub detail_rotates: bool,
        /// 背景固定一张；每部番各记各的，默认轮换。
        pub detail_fixed: bool,
        /// 有不止一张剧照可以切换。
        pub detail_can_switch: bool,
        /// 弹弹play 的资料已经有了（下面的评分、类别、简介等有意义）。
        pub detail_known: bool,
        /// 正在向弹弹play 取资料。
        pub detail_loading: bool,
        /// 取资料失败的原因；没有失败时为空。
        #[content]
        pub detail_error: String,
        /// 综合评分，0 到 10；没有评分时为 0。
        pub detail_rating: f64,
        /// 评分写成一位小数。
        #[content]
        pub detail_rating_text: String,
        /// 作品类别，例如“TV动画”。
        #[content]
        pub detail_kind: String,
        /// 开播的年份（文字，免得显示成 2,026）和月份；`detail_has_premiere` 为假时没有意义。
        #[content]
        pub detail_year: String,
        pub detail_month: i64,
        pub detail_has_premiere: bool,
        /// 还在连载。
        pub detail_on_air: bool,
        pub detail_tags: Vec<DetailTag>,
        #[content]
        pub detail_summary: String,

        /// 正在看哪一组剧集，以及这一组的剧集。
        pub detail_group: DetailGroup,
        pub detail_episodes: Vec<DetailEpisode>,
        /// 剧集条该滚到哪一集；`detail_strip_serial` 每变一次就滚一次。
        pub detail_strip_at: i64,
        pub detail_strip_serial: i64,
        /// 三组各有几集，弹弹play 列出的正片集数，和一集多少分钟（不知道时为 0）。
        pub detail_main_count: i64,
        pub detail_special_count: i64,
        pub detail_extra_count: i64,
        pub detail_episode_count: i64,
        pub detail_minutes: i64,
        /// 本地有可以播放的一集：“继续”按钮可用，播放 `detail_resume`。
        pub detail_can_resume: bool,
        pub detail_resume: MediaSource,
        /// 这部番看过（按钮写“继续”，否则写“播放”）。
        pub detail_started: bool,
        /// “继续”那一集看到总时长的几分之几，0 到 1。
        pub detail_resume_fraction: f64,

        /// 短评（第一页）、一共多少条、来自哪个网站。
        pub detail_comments: Vec<DetailComment>,
        /// 这一页的评论比显示的多，可以展开；展开着时为假。
        pub detail_comments_more: bool,
        /// 评论展开着（显示这一页全部）。
        pub detail_comments_expanded: bool,
        pub detail_comment_count: i64,
        #[content]
        pub detail_comment_source: String,
        pub detail_scores: Vec<DetailScore>,
        pub detail_facts: Vec<DetailFact>,
    }

    #[commands]
    pub trait Commands {
        /// 打开一个视频文件并播放，同时识别它并获取弹幕。上次看到一半的，从那里接着播。
        fn open_file(&mut self, file: MediaSource) -> Result<(), CommandError>;

        /// 停止播放，回到首页。
        #[idempotent]
        fn close(&mut self) -> Result<(), CommandError>;

        /// 跳到总时长的 `fraction` 处（0 到 1）。`duration_ms` 是播放器报的总时长，
        /// 不知道时为空，此时不跳。
        fn seek(&mut self, fraction: f64, duration_ms: Option<i64>) -> Result<(), CommandError>;

        /// 把音量设为 `volume`（0 到 1）。
        #[idempotent]
        fn set_volume(&mut self, volume: f64) -> Result<(), CommandError>;

        /// 音量加 `step`（可以是负数），结果限制在 0 到 1。
        fn nudge_volume(&mut self, step: f64) -> Result<(), CommandError>;

        /// 从 `position_ms`（播放器报的当前位置）往后跳 `seconds` 秒，负数往前跳，
        /// 结果限制在 0 到总时长。连按时在上一次要跳到的位置上接着加减。
        fn skip(
            &mut self,
            seconds: f64,
            position_ms: i64,
            duration_ms: Option<i64>,
        ) -> Result<(), CommandError>;

        /// 显示或隐藏画面上的弹幕。
        fn toggle_danmaku(&mut self) -> Result<(), CommandError>;

        /// 重新整理继续观看：去掉文件已经不在了的视频。
        fn refresh_recents(&mut self) -> Result<(), CommandError>;

        /// 准备本季新番列表：有缓存就读缓存，从没取过才向弹弹play 取一次。
        /// 已经准备过则什么也不做。
        #[idempotent]
        fn load_shin(&mut self) -> Result<(), CommandError>;

        /// 向弹弹play 重新取本季新番列表，替换缓存。
        fn refresh_shin(&mut self) -> Result<(), CommandError>;

        /// 只显示 `day` 那天更新的新番。
        #[idempotent]
        fn show_day(&mut self, day: Weekday) -> Result<(), CommandError>;

        /// 把一个文件夹加入媒体库，并在后台数出里面的视频。
        fn add_folder(&mut self, folder: MediaSource) -> Result<(), CommandError>;

        /// 重新扫描所有文件夹：归出番剧，对还没认出的每部请弹弹play 认一集。
        fn refresh_library(&mut self) -> Result<(), CommandError>;

        /// 打开海报墙上编号为 `id` 的番的详情页。
        #[idempotent]
        fn open_series(&mut self, id: i64) -> Result<(), CommandError>;

        /// 打开弹弹play 作品 `anime_id`（例如本季新番里的一部）的详情页。
        #[idempotent]
        fn open_anime(&mut self, anime_id: i64) -> Result<(), CommandError>;

        /// 关掉详情页，回到原来那一页。
        #[idempotent]
        fn close_details(&mut self) -> Result<(), CommandError>;

        /// 不管缓存，向弹弹play 重新取详情页的资料。
        fn refresh_details(&mut self) -> Result<(), CommandError>;

        /// 剧集条只显示 `group` 这一组。
        #[idempotent]
        fn show_group(&mut self, group: DetailGroup) -> Result<(), CommandError>;

        /// 剧集条从现在最前面的那一集（编号 `at`）往后翻 `pages` 屏，负数往前翻。
        fn page_episodes(&mut self, at: i64, pages: i64) -> Result<(), CommandError>;

        /// 把“继续”指向的那一集记成看完了。
        fn mark_watched(&mut self) -> Result<(), CommandError>;

        /// 背景固定一张（`fixed` 为真），或轮换。每部番各记各的。
        #[idempotent]
        fn set_backdrop_fixed(&mut self, fixed: bool) -> Result<(), CommandError>;

        /// 首页出现：为继续播放里的番准备大图。
        #[idempotent]
        fn load_home(&mut self) -> Result<(), CommandError>;

        /// 首页大图换成继续播放里编号为 `id` 的那一部。
        #[idempotent]
        fn show_hero(&mut self, id: i64) -> Result<(), CommandError>;

        /// 首页大图往后换 `step` 部，负数往前，到头了从另一头接着来。
        fn step_hero(&mut self, step: i64) -> Result<(), CommandError>;

        /// 忘掉详情页这部番每一集看到哪里。
        fn clear_progress(&mut self) -> Result<(), CommandError>;

        /// 评论在只显示几条和显示这一页全部之间切换。
        fn toggle_comments(&mut self) -> Result<(), CommandError>;

        /// 背景换下一张剧照；固定着的就固定在换到的那一张上。
        fn next_backdrop(&mut self) -> Result<(), CommandError>;

        /// 记下正在看的视频看到了哪里：`position_ms` 和 `duration_ms` 是播放器报的位置和总时长。
        /// 刚打开视频的几秒里不记（播放器报的可能还是上一个视频的）。
        fn record_progress(
            &mut self,
            position_ms: i64,
            duration_ms: Option<i64>,
        ) -> Result<(), CommandError>;

        /// 把编号为 `id` 的文件夹移出媒体库；不删除任何文件。
        #[idempotent]
        fn remove_folder(&mut self, id: i64) -> Result<(), CommandError>;
    }
}
